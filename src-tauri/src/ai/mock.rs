//! A mock `llama-server` for tests (CLAUDE.md: AI tests must not need a GPU).
//!
//! It speaks just enough HTTP: `/health`, `/props`, `/slots`, `/tokenize`,
//! `/v1/models`, `/apply-template`, streaming `/completion` and
//! `/v1/chat/completions`, with artificial latency. It records when a client
//! closes a stream, so tests can measure cancel latency.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

#[derive(Clone)]
pub struct MockConfig {
    pub tokens: Vec<String>,
    pub first_token_delay: Duration,
    pub token_delay: Duration,
    pub total_slots: u32,
    /// Answer `/health` with 503 "Loading model" for this long after start.
    pub loading_for: Duration,
    pub native: bool,
    /// Decide the reply from the request body (the prompt for `/completion`, the messages
    /// for chat); returns the streamed tokens. `None` streams `tokens`.
    pub responder: Option<Responder>,
}

pub type Responder = Arc<dyn Fn(&Value) -> Vec<String> + Send + Sync>;

impl Default for MockConfig {
    fn default() -> Self {
        MockConfig {
            tokens: "The quick brown fox jumps over the lazy dog and keeps running far away"
                .split(' ')
                .map(|w| format!(" {w}"))
                .collect(),
            first_token_delay: Duration::from_millis(20),
            token_delay: Duration::from_millis(20),
            total_slots: 2,
            loading_for: Duration::ZERO,
            native: true,
            responder: None,
        }
    }
}

#[derive(Default)]
pub struct MockState {
    /// (path, body) of every request, in arrival order.
    pub requests: Mutex<Vec<(String, Value)>>,
    pub streams_started: AtomicU32,
    pub streams_completed: AtomicU32,
    /// When the most recent streaming client went away mid-generation.
    pub last_disconnect: Mutex<Option<Instant>>,
    pub disconnected: AtomicU32,
    pub changed: Notify,
}

impl MockState {
    pub fn bodies_for(&self, path: &str) -> Vec<Value> {
        self.requests
            .lock()
            .map(|r| {
                r.iter()
                    .filter(|(p, _)| p == path)
                    .map(|(_, b)| b.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

pub struct MockServer {
    pub port: u16,
    pub state: Arc<MockState>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl MockServer {
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub async fn start(cfg: MockConfig) -> MockServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let state = Arc::new(MockState::default());
        let started = Instant::now();
        let (st, cfg2) = (state.clone(), cfg);
        let task = tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let (st, cfg) = (st.clone(), cfg2.clone());
                tokio::spawn(async move {
                    let _ = handle(sock, st, cfg, started).await;
                });
            }
        });
        MockServer { port, state, task }
    }
}

async fn read_request(sock: &mut TcpStream) -> std::io::Result<(String, String, Value)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split(' ');
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let len = head
        .lines()
        .find_map(|l| {
            let l = l.to_ascii_lowercase();
            l.strip_prefix("content-length:")
                .and_then(|v| v.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    while buf.len() < head_end + len {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let body = serde_json::from_slice(&buf[head_end..]).unwrap_or(Value::Null);
    Ok((method, path, body))
}

async fn respond_json(sock: &mut TcpStream, status: &str, v: &Value) -> std::io::Result<()> {
    let body = v.to_string();
    let msg = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    sock.write_all(msg.as_bytes()).await?;
    sock.shutdown().await
}

async fn handle(
    mut sock: TcpStream,
    st: Arc<MockState>,
    cfg: MockConfig,
    started: Instant,
) -> std::io::Result<()> {
    let (method, path, body) = read_request(&mut sock).await?;
    if let Ok(mut r) = st.requests.lock() {
        r.push((path.clone(), body.clone()));
    }
    st.changed.notify_waiters();
    match (method.as_str(), path.as_str()) {
        ("GET", "/health") => {
            if started.elapsed() < cfg.loading_for {
                respond_json(
                    &mut sock,
                    "503 Service Unavailable",
                    &json!({"error": {"code": 503, "message": "Loading model"}}),
                )
                .await
            } else {
                respond_json(&mut sock, "200 OK", &json!({"status": "ok"})).await
            }
        }
        ("GET", "/props") if cfg.native => {
            respond_json(
                &mut sock,
                "200 OK",
                &json!({
                    "total_slots": cfg.total_slots,
                    "model_alias": "mock-model",
                    "build_info": "mock",
                    "chat_template": "{{ messages }}",
                    "default_generation_settings": {"n_ctx": 8192},
                }),
            )
            .await
        }
        ("GET", "/slots") => {
            let slots: Vec<Value> = (0..cfg.total_slots)
                .map(|i| json!({"id": i, "n_ctx": 4096, "is_processing": false}))
                .collect();
            respond_json(&mut sock, "200 OK", &Value::Array(slots)).await
        }
        ("GET", "/v1/models") => {
            respond_json(&mut sock, "200 OK", &json!({"data": [{"id": "mock-model"}]})).await
        }
        ("POST", "/tokenize") => {
            let n = body
                .get("content")
                .and_then(Value::as_str)
                .map_or(0, |t| t.split_whitespace().count());
            respond_json(&mut sock, "200 OK", &json!({"tokens": vec![1; n]})).await
        }
        ("POST", "/apply-template") => {
            respond_json(
                &mut sock,
                "200 OK",
                &json!({"prompt": "<|im_start|>system\nSYS<|im_end|>\n<|im_start|>user\nUSR<|im_end|>\n<|im_start|>assistant\n"}),
            )
            .await
        }
        ("POST", "/v1/embeddings") => {
            let inputs: Vec<String> = match body.get("input") {
                Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
                Some(Value::String(s)) => vec![s.clone()],
                _ => vec![],
            };
            let data: Vec<Value> = inputs
                .iter()
                .enumerate()
                .map(|(i, t)| json!({"index": i, "embedding": fake_embedding(t)}))
                .collect();
            respond_json(&mut sock, "200 OK", &json!({"data": data})).await
        }
        ("POST", "/completion") if cfg.native => stream(sock, st, cfg, true, body).await,
        ("POST", "/v1/chat/completions") => stream(sock, st, cfg, false, body).await,
        _ => respond_json(&mut sock, "404 Not Found", &json!({"error": "not found"})).await,
    }
}

async fn stream(
    mut sock: TcpStream,
    st: Arc<MockState>,
    cfg: MockConfig,
    native: bool,
    body: Value,
) -> std::io::Result<()> {
    let cfg = match &cfg.responder {
        Some(f) => MockConfig {
            tokens: f(&body),
            ..cfg.clone()
        },
        None => cfg,
    };
    st.streams_started.fetch_add(1, Ordering::SeqCst);
    sock.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
    )
    .await?;
    let (mut rd, mut wr) = sock.into_split();
    // Resolves when the client closes its side.
    let gone = Arc::new(Notify::new());
    let gone2 = gone.clone();
    tokio::spawn(async move {
        let mut b = [0u8; 64];
        loop {
            match rd.read(&mut b).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
        gone2.notify_one();
    });
    let mut wait = cfg.first_token_delay;
    for (i, tok) in cfg.tokens.iter().enumerate() {
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = gone.notified() => return client_left(&st),
        }
        wait = cfg.token_delay;
        let chunk = if native {
            json!({"content": tok, "stop": false, "index": i})
        } else {
            json!({"choices": [{"delta": {"content": tok}}]})
        };
        if wr
            .write_all(format!("data: {chunk}\n\n").as_bytes())
            .await
            .is_err()
        {
            return client_left(&st);
        }
    }
    let last = if native {
        json!({"content": "", "stop": true, "tokens_evaluated": 50, "tokens_predicted": cfg.tokens.len(),
               "timings": {"prompt_n": 8, "cache_n": 42, "predicted_n": cfg.tokens.len(), "predicted_per_second": 50.0}})
    } else {
        json!({"choices": [{"delta": {}}], "usage": {"prompt_tokens": 50, "completion_tokens": cfg.tokens.len()}})
    };
    let _ = wr.write_all(format!("data: {last}\n\n").as_bytes()).await;
    if !native {
        let _ = wr.write_all(b"data: [DONE]\n\n").await;
    }
    st.streams_completed.fetch_add(1, Ordering::SeqCst);
    st.changed.notify_waiters();
    let _ = wr.shutdown().await;
    Ok(())
}

fn client_left(st: &MockState) -> std::io::Result<()> {
    if let Ok(mut t) = st.last_disconnect.lock() {
        *t = Some(Instant::now());
    }
    st.disconnected.fetch_add(1, Ordering::SeqCst);
    st.changed.notify_waiters();
    Ok(())
}

/// A deterministic stand-in embedding: words hashed into 32 buckets, so texts that
/// share words are close.
pub fn fake_embedding(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; 32];
    for w in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
    {
        let h = w
            .to_lowercase()
            .bytes()
            .fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
        v[(h % 32) as usize] += 1.0;
    }
    v
}
