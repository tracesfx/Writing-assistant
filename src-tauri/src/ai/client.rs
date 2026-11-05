//! The inference client (SPEC §6.1): llama-server native `/completion`
//! (preferred) with an OpenAI-compatible fallback, a capability probe, and
//! cancellation by dropping the response stream.
//!
//! Network rule (SPEC §2.8): this client only talks to loopback addresses.
//! The URL is validated, redirects and proxies are disabled, and the crate is
//! built without TLS, so nothing can leave the machine.

use std::future::Future;
use std::pin::Pin;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use super::template;
use super::types::*;
use crate::error::{MarginError, Result};

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One generation request, already rendered for the server.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    /// The raw prompt for native `/completion`.
    pub raw_prompt: String,
    /// The same request as chat parts, for the OpenAI fallback.
    pub system: String,
    pub user: String,
    pub assistant_prefix: String,
    pub sampling: Sampling,
    /// `id_slot` on the server (SPEC §6.4). `None` lets the server choose.
    pub slot: Option<u32>,
    /// Send `enable_thinking: false` on chat calls.
    pub disable_thinking: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Health {
    Ready,
    Loading(String),
    Down,
}

/// Providers are swappable behind this trait (SPEC §1.2: future cloud APIs).
pub trait InferenceClient: Send + Sync {
    /// Stream a generation. Deltas go to `tokens`; cancelling `cancel` drops the
    /// connection, which makes llama-server stop generating.
    fn complete<'a>(
        &'a self,
        req: CompletionRequest,
        tokens: UnboundedSender<String>,
        cancel: CancellationToken,
    ) -> BoxFut<'a, Completion>;
    fn probe<'a>(&'a self, preset: Option<&'a Preset>) -> BoxFut<'a, Capabilities>;
    fn health<'a>(&'a self) -> BoxFut<'a, Health>;
    fn slots<'a>(&'a self) -> BoxFut<'a, Vec<SlotInfo>>;
    fn tokenize<'a>(&'a self, text: &'a str) -> BoxFut<'a, Result<u32>>;
    fn endpoint(&self) -> String;
}

/// Accept only `http://127.0.0.1`, `http://localhost` and `http://[::1]`.
pub fn validate_local_url(url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(url.trim())
        .map_err(|e| MarginError::Invalid(format!("endpoint is not a URL: {e}")))?;
    if parsed.scheme() != "http" {
        return Err(MarginError::Invalid(
            "only plain http to this machine is allowed".into(),
        ));
    }
    let local = match parsed.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        None => false,
    };
    if !local {
        return Err(MarginError::Invalid(
            "Margin only connects to endpoints on this machine (127.0.0.1)".into(),
        ));
    }
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

pub struct LlamaClient {
    base: String,
    http: reqwest::Client,
    caps: RwLock<Option<Capabilities>>,
}

impl LlamaClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let base = validate_local_url(base_url)?;
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|e| MarginError::Internal(format!("http client: {e}")))?;
        Ok(LlamaClient {
            base,
            http,
            caps: RwLock::new(None),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    fn use_native(&self) -> bool {
        // Native unless a probe positively found it missing.
        self.caps
            .read()
            .ok()
            .and_then(|c| c.as_ref().map(|c| c.native_completion || !c.openai_chat))
            .unwrap_or(true)
    }

    async fn get_json(&self, path: &str) -> Option<Value> {
        let r = self
            .http
            .get(self.url(path))
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .ok()?;
        if !r.status().is_success() {
            return None;
        }
        r.json().await.ok()
    }

    fn native_body(req: &CompletionRequest) -> Value {
        let s = &req.sampling;
        let mut body = json!({
            "prompt": req.raw_prompt,
            "stream": true,
            "n_predict": s.max_tokens,
            "temperature": s.temperature,
            "top_p": s.top_p,
            "min_p": s.min_p,
            "stop": s.stop,
            "cache_prompt": true,
        });
        if let Some(slot) = req.slot {
            body["id_slot"] = json!(slot);
        }
        if let Some(schema) = s
            .json_schema
            .as_deref()
            .and_then(|t| serde_json::from_str::<Value>(t).ok())
        {
            body["json_schema"] = schema;
        }
        body
    }

    fn chat_body(req: &CompletionRequest) -> Value {
        let s = &req.sampling;
        let mut messages = vec![
            json!({"role": "system", "content": req.system}),
            json!({"role": "user", "content": req.user}),
        ];
        if !req.assistant_prefix.is_empty() {
            messages.push(json!({"role": "assistant", "content": req.assistant_prefix}));
        }
        let mut body = json!({
            "messages": messages,
            "stream": true,
            "max_tokens": s.max_tokens,
            "temperature": s.temperature,
            "top_p": s.top_p,
            "min_p": s.min_p,
            "stop": s.stop,
        });
        if req.disable_thinking {
            body["chat_template_kwargs"] = json!({"enable_thinking": false});
        }
        if let Some(slot) = req.slot {
            body["id_slot"] = json!(slot);
            body["cache_prompt"] = json!(true);
        }
        body
    }

    async fn run(
        &self,
        req: CompletionRequest,
        tokens: UnboundedSender<String>,
        cancel: CancellationToken,
    ) -> Completion {
        let started = Instant::now();
        let native = self.use_native();
        let (path, body) = if native {
            ("/completion", Self::native_body(&req))
        } else {
            ("/v1/chat/completions", Self::chat_body(&req))
        };
        let mut out = Completion {
            text: String::new(),
            outcome: Outcome::Completed,
            error: None,
            timings: Timings::default(),
        };
        let fail = |mut c: Completion, msg: String, started: Instant| {
            c.outcome = Outcome::Failed;
            c.error = Some(msg);
            c.timings.total_ms = started.elapsed().as_secs_f64() * 1000.0;
            c
        };
        let send = self.http.post(self.url(path)).json(&body).send();
        let resp = tokio::select! {
            _ = cancel.cancelled() => {
                out.outcome = Outcome::Cancelled;
                out.timings.total_ms = started.elapsed().as_secs_f64() * 1000.0;
                return out;
            }
            r = send => r,
        };
        let resp = match resp {
            Ok(r) => r,
            Err(e) => return fail(out, format!("cannot reach the model server: {e}"), started),
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return fail(
                out,
                format!("server answered {status}: {}", clip(&text, 300)),
                started,
            );
        }
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        'outer: loop {
            let next = tokio::select! {
                _ = cancel.cancelled() => {
                    out.outcome = Outcome::Cancelled;
                    break 'outer;
                }
                // A stalled server must not hang a request forever.
                n = tokio::time::timeout(Duration::from_secs(300), stream.next()) => n,
            };
            let chunk = match next {
                Err(_) => return fail(out, "the model server stopped responding".into(), started),
                Ok(None) => break,
                Ok(Some(Err(e))) => return fail(out, format!("stream error: {e}"), started),
                Ok(Some(Ok(c))) => c,
            };
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                let line = line.trim();
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data == "[DONE]" {
                    break 'outer;
                }
                let Ok(v) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                if let Some(err) = v.get("error") {
                    return fail(out, clip(&err.to_string(), 300), started);
                }
                let delta = if native {
                    v.get("content").and_then(Value::as_str)
                } else {
                    v.pointer("/choices/0/delta/content")
                        .and_then(Value::as_str)
                };
                if let Some(d) = delta.filter(|d| !d.is_empty()) {
                    if out.timings.ttft_ms.is_none() {
                        out.timings.ttft_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
                    }
                    out.text.push_str(d);
                    // The receiver may be gone (cancelled elsewhere); that is fine.
                    let _ = tokens.send(d.to_string());
                }
                read_stats(&v, native, &mut out.timings);
            }
        }
        // Dropping `stream` closes the connection: llama-server stops generating.
        drop(stream);
        out.timings.total_ms = started.elapsed().as_secs_f64() * 1000.0;
        out
    }

    async fn run_probe(&self, preset: Option<&Preset>) -> Capabilities {
        let mut caps = Capabilities::default();
        let health = self.run_health().await;
        caps.reachable = health != Health::Down;
        if health != Health::Ready {
            *self.caps.write().unwrap_or_else(|e| e.into_inner()) = Some(caps.clone());
            return caps;
        }
        if let Some(props) = self.get_json("/props").await {
            caps.native_completion = props.get("default_generation_settings").is_some()
                || props.get("total_slots").is_some();
            caps.total_slots = props
                .get("total_slots")
                .and_then(Value::as_u64)
                .unwrap_or(1) as u32;
            caps.n_ctx = props
                .pointer("/default_generation_settings/n_ctx")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32;
            caps.model = props
                .get("model_alias")
                .or_else(|| props.get("model_path"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            caps.build_info = props
                .get("build_info")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            caps.has_chat_template = props
                .get("chat_template")
                .and_then(Value::as_str)
                .is_some_and(|t| !t.is_empty());
        }
        caps.openai_chat = self.get_json("/v1/models").await.is_some();
        caps.tokenize = self.run_tokenize("hello world").await.is_ok();
        caps.slots_endpoint = self.get_json("/slots").await.is_some();
        if let Some(p) = preset {
            caps.template_warning = self.check_template(p).await;
        }
        *self.caps.write().unwrap_or_else(|e| e.into_inner()) = Some(caps.clone());
        caps
    }

    /// Compare our rendering of a sample chat with the server's own template.
    async fn check_template(&self, preset: &Preset) -> Option<String> {
        let mut body = json!({"messages": [
            {"role": "system", "content": "SYS"},
            {"role": "user", "content": "USR"},
        ]});
        if preset.thinking != ThinkingSwitch::None {
            body["chat_template_kwargs"] = json!({"enable_thinking": false});
        }
        let r = self
            .http
            .post(self.url("/apply-template"))
            .json(&body)
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .ok()?;
        if !r.status().is_success() {
            return None;
        }
        let v: Value = r.json().await.ok()?;
        let theirs = v.get("prompt")?.as_str()?;
        let ours = template::render_chat_only(preset.template, "SYS", "USR");
        let norm = |s: &str| {
            s.replace("<bos>", "")
                .replace("<s>", "")
                .split_whitespace()
                .collect::<String>()
        };
        // Thinking blocks and newlines the server may add after the turn marker don't matter.
        let (n_theirs, n_ours) = (norm(theirs), norm(&ours));
        if n_theirs == n_ours || n_theirs.starts_with(&n_ours) {
            None
        } else {
            Some(format!(
                "This model's own chat template differs from the \"{:?}\" template Margin sends. Server: {} / Margin: {}",
                preset.template,
                clip(&theirs.replace('\n', "⏎"), 160),
                clip(&ours.replace('\n', "⏎"), 160)
            ))
        }
    }

    async fn run_health(&self) -> Health {
        let r = self
            .http
            .get(self.url("/health"))
            .timeout(Duration::from_secs(2))
            .send()
            .await;
        match r {
            Err(_) => Health::Down,
            Ok(r) if r.status().is_success() => Health::Ready,
            Ok(r) if r.status().as_u16() == 503 => {
                let msg = r
                    .json::<Value>()
                    .await
                    .ok()
                    .and_then(|v| {
                        v.pointer("/error/message")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| "loading".into());
                Health::Loading(msg)
            }
            Ok(_) => Health::Down,
        }
    }

    async fn run_tokenize(&self, text: &str) -> Result<u32> {
        let r = self
            .http
            .post(self.url("/tokenize"))
            .json(&json!({"content": text}))
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| MarginError::Io(e.to_string()))?;
        if !r.status().is_success() {
            return Err(MarginError::Io(format!("tokenize answered {}", r.status())));
        }
        let v: Value = r
            .json()
            .await
            .map_err(|e| MarginError::Parse(e.to_string()))?;
        Ok(v.get("tokens")
            .and_then(Value::as_array)
            .map_or(0, |t| t.len()) as u32)
    }
}

fn read_stats(v: &Value, native: bool, t: &mut Timings) {
    if native {
        if let Some(n) = v.get("tokens_evaluated").and_then(Value::as_u64) {
            t.prompt_tokens = n as u32;
        }
        if let Some(n) = v.get("tokens_predicted").and_then(Value::as_u64) {
            t.output_tokens = n as u32;
        }
        if let Some(tm) = v.get("timings") {
            let u = |k: &str| tm.get(k).and_then(Value::as_u64).unwrap_or(0) as u32;
            let (prompt_n, cache_n) = (u("prompt_n"), u("cache_n"));
            if prompt_n + cache_n > 0 {
                t.prompt_tokens = prompt_n + cache_n;
                t.cached_tokens = cache_n;
            }
            if let Some(n) = tm.get("predicted_n").and_then(Value::as_u64) {
                t.output_tokens = n as u32;
            }
            t.decode_tps = tm.get("predicted_per_second").and_then(Value::as_f64);
        }
    } else if let Some(u) = v.get("usage") {
        let g = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0) as u32;
        t.prompt_tokens = g("prompt_tokens");
        t.output_tokens = g("completion_tokens");
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push('…');
        t
    }
}

impl InferenceClient for LlamaClient {
    fn complete<'a>(
        &'a self,
        req: CompletionRequest,
        tokens: UnboundedSender<String>,
        cancel: CancellationToken,
    ) -> BoxFut<'a, Completion> {
        Box::pin(self.run(req, tokens, cancel))
    }

    fn probe<'a>(&'a self, preset: Option<&'a Preset>) -> BoxFut<'a, Capabilities> {
        Box::pin(self.run_probe(preset))
    }

    fn health<'a>(&'a self) -> BoxFut<'a, Health> {
        Box::pin(self.run_health())
    }

    fn slots<'a>(&'a self) -> BoxFut<'a, Vec<SlotInfo>> {
        Box::pin(async move {
            let Some(Value::Array(slots)) = self.get_json("/slots").await else {
                return vec![];
            };
            slots
                .iter()
                .map(|s| SlotInfo {
                    id: s.get("id").and_then(Value::as_u64).unwrap_or(0) as u32,
                    busy: s
                        .get("is_processing")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    n_ctx: s.get("n_ctx").and_then(Value::as_u64).unwrap_or(0) as u32,
                })
                .collect()
        })
    }

    fn tokenize<'a>(&'a self, text: &'a str) -> BoxFut<'a, Result<u32>> {
        Box::pin(self.run_tokenize(text))
    }

    fn endpoint(&self) -> String {
        self.base.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_http_endpoints_are_accepted() {
        for ok in [
            "http://127.0.0.1:8080",
            "http://localhost:8081/",
            "http://[::1]:9000",
            "http://127.0.0.2:1",
        ] {
            assert!(validate_local_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "https://127.0.0.1:8080",
            "http://192.168.1.5:8080",
            "http://example.com",
            "http://0.0.0.0:8080",
            "http://127.0.0.1.evil.com",
            "ftp://127.0.0.1",
            "not a url",
        ] {
            assert!(validate_local_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn trailing_slash_is_trimmed() {
        assert_eq!(
            validate_local_url("http://localhost:8081/").unwrap(),
            "http://localhost:8081"
        );
    }
}
