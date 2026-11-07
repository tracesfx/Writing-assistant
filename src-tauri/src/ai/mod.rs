//! AI layer (SPEC §6): model connection, scheduler, context log, bake-off.
//!
//! * `client`     — `InferenceClient` trait + llama-server implementation
//! * `scheduler`  — P0/P1/P2 queue, slot routing, pre-emption
//! * `supervisor` — managed-mode `llama-server` process
//! * `context_log`— every request's exact prompt and output (§9.6)
//! * `presets`, `template` — per-model prompt and launch settings
//! * `bakeoff`    — speed and blind-quality comparison (§6.6)
//!
//! `AiService` ties them together. Nothing here touches manuscript text: the
//! AI layer produces suggestions as data and the editor decides what to show.

pub mod bakeoff;
pub mod bakeoff_service;
pub mod client;
pub mod context_log;
pub mod ghost;
#[cfg(test)]
pub mod mock;
pub mod presets;
pub mod scheduler;
pub mod supervisor;
pub mod template;
pub mod types;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::error::{MarginError, Result};
use crate::events::{DomainEvent, EventBus};
use crate::project::atomic::atomic_write;
use crate::story::{APP_DIR, StoryService};

use client::{Health, InferenceClient, LlamaClient, validate_local_url};
use context_log::ContextLog;
use ghost::{GhostContext, GhostRequest, PostCtx};
use scheduler::{Activity, FinishedJob, JobHandle, JobSpec, Scheduler, Target};
use supervisor::{ProcState, Supervisor, launch_for};
use types::*;

const CONFIG_FILE: &str = "ai-config.json";

/// Ghost requests in flight, and cancels that arrived before their request.
#[derive(Default)]
struct GhostBook {
    active: HashMap<String, CancellationToken>,
    early_cancelled: VecDeque<String>,
}

#[derive(Default)]
struct Conn {
    client: Option<Arc<dyn InferenceClient>>,
    health: Option<Health>,
    caps: Capabilities,
    /// The preset in use right now (may be a bake-off candidate, not the saved one).
    preset: Option<Preset>,
    endpoint: String,
    mode: Option<ConnectionMode>,
    error: Option<String>,
    poll: Option<CancellationToken>,
}

pub struct AiService {
    bus: Arc<EventBus>,
    cfg_path: PathBuf,
    config: Mutex<AiConfig>,
    conn: Mutex<Conn>,
    last_request: Mutex<Option<LastRequest>>,
    last_status: Mutex<Option<(AiStatus, String)>>,
    ghosts: Mutex<GhostBook>,
    story: Arc<StoryService>,
    /// Story Memory, set once after both services exist (it also uses this service).
    memory: std::sync::OnceLock<Weak<crate::memory::MemoryService>>,
    pub scheduler: Scheduler,
    pub supervisor: Arc<Supervisor>,
    pub context_log: Arc<ContextLog>,
    handle: tokio::runtime::Handle,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl AiService {
    pub fn new(
        config_dir: PathBuf,
        story: Arc<StoryService>,
        bus: Arc<EventBus>,
        handle: tokio::runtime::Handle,
    ) -> Arc<Self> {
        let cfg_path = config_dir.join(CONFIG_FILE);
        let mut config = std::fs::read(&cfg_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<AiConfig>(&b).ok())
            .unwrap_or_else(presets::default_config);
        config.presets = presets::merge_presets(std::mem::take(&mut config.presets));
        let saved_revision = config.preset_revision;
        presets::upgrade(&mut config);
        if config.preset_revision != saved_revision {
            // An older file was migrated: write it back once, so the file on disk
            // matches what the app uses (a failure just means we migrate again next time).
            match serde_json::to_vec_pretty(&config) {
                Ok(json) => {
                    if let Err(e) = atomic_write(&cfg_path, &json) {
                        eprintln!("[margin] could not save the migrated model settings: {e}");
                    }
                }
                Err(e) => eprintln!("[margin] could not encode the migrated model settings: {e}"),
            }
        }
        let context_log = Arc::new(ContextLog::new());
        context_log.set_limit(config.context_log_limit as usize);

        let svc = Arc::new_cyclic(|weak: &Weak<AiService>| {
            let w1 = weak.clone();
            let w2 = weak.clone();
            let w3 = weak.clone();
            let scheduler = Scheduler::new(
                handle.clone(),
                Arc::new(move |job| {
                    if let Some(s) = w1.upgrade() {
                        s.on_job_finished(job);
                    }
                }),
                Arc::new(move || {
                    if let Some(s) = w2.upgrade() {
                        s.publish_status();
                    }
                }),
            );
            let supervisor = Supervisor::new(Arc::new(move || {
                if let Some(s) = w3.upgrade() {
                    s.publish_status();
                }
            }));
            AiService {
                bus: bus.clone(),
                cfg_path,
                config: Mutex::new(config),
                conn: Mutex::new(Conn::default()),
                last_request: Mutex::new(None),
                last_status: Mutex::new(None),
                ghosts: Mutex::new(GhostBook::default()),
                story: story.clone(),
                memory: std::sync::OnceLock::new(),
                scheduler,
                supervisor,
                context_log: context_log.clone(),
                handle,
            }
        });
        {
            let c = lock(&svc.config).clone();
            svc.scheduler.set_enabled(c.enabled);
            svc.scheduler
                .set_idle_threshold(Duration::from_secs(c.idle_seconds as u64));
        }
        // The context log follows the open project.
        let cl = context_log;
        let st = story;
        bus.subscribe(move |e| match &e.event {
            DomainEvent::ProjectOpened { .. } => {
                cl.set_project(st.project_root().ok().map(|r| r.join(APP_DIR)));
            }
            DomainEvent::ProjectClosed { .. } => cl.set_project(None),
            _ => {}
        });
        svc
    }

    /// Connect Story Memory (called once at startup).
    pub fn set_memory(&self, m: Weak<crate::memory::MemoryService>) {
        let _ = self.memory.set(m);
    }

    pub fn memory(&self) -> Option<Arc<crate::memory::MemoryService>> {
        self.memory.get().and_then(Weak::upgrade)
    }

    // --- Configuration -----------------------------------------------------------

    pub fn config(&self) -> AiConfig {
        lock(&self.config).clone()
    }

    pub fn set_config(&self, mut new: AiConfig) -> Result<AiConfig> {
        if new.port == 0 || new.port > 65535 {
            return Err(MarginError::Invalid("port must be 1–65535".into()));
        }
        new.external_url = validate_local_url(&new.external_url)?;
        new.idle_seconds = new.idle_seconds.clamp(1, 600);
        new.context_log_limit = new.context_log_limit.clamp(10, 5000);
        new.presets = presets::merge_presets(new.presets);
        presets::upgrade(&mut new);
        if !new.presets.iter().any(|p| p.id == new.preset_id) {
            return Err(MarginError::Invalid(format!(
                "unknown preset {}",
                new.preset_id
            )));
        }
        let json = serde_json::to_vec_pretty(&new)?;
        atomic_write(&self.cfg_path, &json)?;
        *lock(&self.config) = new.clone();
        self.scheduler.set_enabled(new.enabled);
        self.scheduler
            .set_idle_threshold(Duration::from_secs(new.idle_seconds as u64));
        self.context_log.set_limit(new.context_log_limit as usize);
        self.publish_status();
        Ok(new)
    }

    pub fn active_preset(&self) -> Preset {
        let c = lock(&self.config);
        c.presets
            .iter()
            .find(|p| p.id == c.preset_id)
            .or_else(|| c.presets.first())
            .cloned()
            .unwrap_or_else(|| presets::builtin_presets().remove(0))
    }

    // --- Connection --------------------------------------------------------------

    /// Connect using the saved configuration.
    pub async fn connect(self: &Arc<Self>) -> Result<()> {
        let cfg = self.config();
        let preset = self.active_preset();
        self.connect_with(cfg.mode, preset, cfg.port, cfg.external_url)
            .await
    }

    /// Connect with an explicit preset (the bake-off tries candidates this way
    /// without changing the saved configuration).
    pub async fn connect_with(
        self: &Arc<Self>,
        mode: ConnectionMode,
        preset: Preset,
        port: u32,
        external_url: String,
    ) -> Result<()> {
        if !self.config().enabled {
            self.publish_status();
            return Err(MarginError::Invalid("AI is turned off".into()));
        }
        self.drop_connection().await;
        let endpoint = match mode {
            ConnectionMode::Managed => format!("http://127.0.0.1:{port}"),
            ConnectionMode::External => external_url,
        };
        let client: Arc<dyn InferenceClient> = Arc::new(LlamaClient::new(&endpoint)?);
        let token = CancellationToken::new();
        {
            let mut c = lock(&self.conn);
            *c = Conn {
                client: Some(client.clone()),
                health: None,
                caps: Capabilities::default(),
                preset: Some(preset.clone()),
                endpoint: client.endpoint(),
                mode: Some(mode),
                error: None,
                poll: Some(token.clone()),
            };
        }
        if mode == ConnectionMode::Managed {
            // A server of ours may have outlived the last session: adopt it.
            let already = client.health().await;
            if already == Health::Down {
                let launch = launch_for(&preset, port);
                if let Err(e) = self.supervisor.start(launch).await {
                    lock(&self.conn).error = Some(e.to_string());
                    self.publish_status();
                    return Err(e);
                }
            }
        }
        let me = self.clone();
        self.handle.spawn(async move { me.poll_loop(token).await });
        self.publish_status();
        Ok(())
    }

    /// Stop talking to the server and (managed mode) stop the process.
    pub async fn disconnect(self: &Arc<Self>) {
        self.drop_connection().await;
        self.supervisor.stop().await;
        self.publish_status();
    }

    async fn drop_connection(&self) {
        let poll = {
            let mut c = lock(&self.conn);
            let p = c.poll.take();
            c.client = None;
            c.health = None;
            c.caps = Capabilities::default();
            c.error = None;
            p
        };
        if let Some(p) = poll {
            p.cancel();
        }
        self.scheduler.set_target(None);
        // A server we started for a previous connection must not linger.
        self.supervisor.stop().await;
    }

    async fn poll_loop(self: Arc<Self>, token: CancellationToken) {
        loop {
            let (client, preset) = {
                let c = lock(&self.conn);
                (c.client.clone(), c.preset.clone())
            };
            let Some(client) = client else { return };
            let health = client.health().await;
            let was_ready = lock(&self.conn).health == Some(Health::Ready);
            if health == Health::Ready && !was_ready {
                let caps = client.probe(preset.as_ref()).await;
                self.scheduler.set_target(Some(Target {
                    client: Some(client.clone()),
                    preset: preset.clone().unwrap_or_else(|| self.active_preset()),
                    total_slots: caps.total_slots.max(1),
                    model: caps.model.clone(),
                }));
                lock(&self.conn).caps = caps;
            } else if health != Health::Ready && was_ready {
                self.scheduler.set_target(None);
                lock(&self.conn).caps.reachable = health != Health::Down;
            }
            lock(&self.conn).health = Some(health.clone());
            self.publish_status();
            let wait = if health == Health::Ready { 2000 } else { 500 };
            tokio::select! {
                _ = token.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_millis(wait)) => {}
            }
        }
    }

    // --- Status ------------------------------------------------------------------

    pub fn status(&self) -> (AiStatus, String) {
        let cfg = self.config();
        if !cfg.enabled {
            return (AiStatus::Off, "AI is turned off".into());
        }
        let (health, error, has_client, mode) = {
            let c = lock(&self.conn);
            (
                c.health.clone(),
                c.error.clone(),
                c.client.is_some(),
                c.mode,
            )
        };
        if !has_client {
            return (
                AiStatus::NotRunning,
                error.unwrap_or_else(|| "The model server is not running".into()),
            );
        }
        if let ProcState::Crashed(why) = self.supervisor.state()
            && mode == Some(ConnectionMode::Managed)
        {
            return (AiStatus::NotRunning, why);
        }
        match health {
            None => (AiStatus::Starting, "Connecting…".into()),
            Some(Health::Down) => {
                let managed_running = mode == Some(ConnectionMode::Managed)
                    && self.supervisor.state() == ProcState::Running;
                if managed_running {
                    (
                        AiStatus::Starting,
                        self.supervisor
                            .last_log_line()
                            .unwrap_or_else(|| "Starting…".into()),
                    )
                } else {
                    (AiStatus::NotRunning, "Cannot reach the model server".into())
                }
            }
            Some(Health::Loading(msg)) => (AiStatus::Starting, msg),
            Some(Health::Ready) => {
                let a: Activity = self.scheduler.activity();
                if a.running > 0 {
                    (AiStatus::Thinking, "Working".into())
                } else if a.held_for_typing {
                    (
                        AiStatus::PausedTyping,
                        "Idle work waits while you type".into(),
                    )
                } else {
                    (AiStatus::Ready, "Ready".into())
                }
            }
        }
    }

    /// Publish `ai.status` if it changed.
    pub fn publish_status(&self) {
        let now = self.status();
        {
            let mut last = lock(&self.last_status);
            if last.as_ref() == Some(&now) {
                return;
            }
            *last = Some(now.clone());
        }
        // A one-time notice that managed mode gave up (SPEC §6.1).
        if self.supervisor.take_gave_up() {
            eprintln!("[margin] model server crashed repeatedly; giving up");
        }
        self.bus.publish(DomainEvent::AiStatus {
            status: now.0,
            detail: now.1,
        });
    }

    pub async fn panel(&self) -> ModelPanel {
        let (status, detail) = self.status();
        let (client, caps, endpoint, mode, preset) = {
            let c = lock(&self.conn);
            (
                c.client.clone(),
                c.caps.clone(),
                c.endpoint.clone(),
                c.mode,
                c.preset.clone(),
            )
        };
        let cfg = self.config();
        let slots = match &client {
            Some(c) if caps.slots_endpoint => c.slots().await,
            _ => vec![],
        };
        ModelPanel {
            status,
            detail,
            mode: mode.unwrap_or(cfg.mode),
            endpoint: if endpoint.is_empty() {
                match cfg.mode {
                    ConnectionMode::Managed => format!("http://127.0.0.1:{}", cfg.port),
                    ConnectionMode::External => cfg.external_url.clone(),
                }
            } else {
                endpoint
            },
            preset_id: preset.map_or(cfg.preset_id, |p| p.id),
            restarts: self.supervisor.restarts(),
            log_tail: self.supervisor.log_tail(60),
            vram: read_vram().await,
            slots,
            capabilities: caps,
            last_request: lock(&self.last_request).clone(),
        }
    }

    // --- Requests ----------------------------------------------------------------

    pub fn submit(&self, spec: JobSpec) -> JobHandle {
        self.scheduler.submit(spec)
    }

    /// Every keystroke (SPEC §2.2): cancels ghost text, pauses idle work.
    pub fn note_keystroke(&self) {
        self.scheduler.note_keystroke();
    }

    fn on_job_finished(&self, job: FinishedJob) {
        *lock(&self.last_request) = Some(LastRequest {
            role: job.spec.role,
            outcome: job.completion.outcome.clone(),
            ttft_ms: job.completion.timings.ttft_ms,
            total_ms: job.completion.timings.total_ms,
        });
        if job.spec.log
            && let Err(e) = self.context_log.record(&job)
        {
            eprintln!("[margin] could not write the context log: {e}");
        }
    }

    // --- Ghost text (SPEC §7) ----------------------------------------------------

    fn publish_ghost(&self, id: &str, state: GhostState, text: String, error: Option<String>) {
        self.bus.publish(DomainEvent::AiGhost {
            request_id: id.to_string(),
            state,
            text,
            error,
        });
    }

    /// Cancel one ghost request. If the request has not arrived yet (IPC calls
    /// can overtake each other), remember the cancel so it never starts.
    pub fn ghost_cancel(&self, request_id: &str) {
        let mut g = lock(&self.ghosts);
        match g.active.get(request_id) {
            Some(t) => t.cancel(),
            None => {
                g.early_cancelled.push_back(request_id.to_string());
                while g.early_cancelled.len() > 64 {
                    g.early_cancelled.pop_front();
                }
            }
        }
    }

    /// What the prompt builder needs, or `None` when AI is off for this document.
    fn ghost_context(&self, doc_id: &str, text_before: &str) -> Result<Option<GhostContext>> {
        let meta = self.story.document_meta(doc_id)?;
        if meta.ai == crate::project::document::AiToggle::Off {
            return Ok(None);
        }
        let project = self.story.project_info()?;
        Ok(Some(GhostContext {
            project_title: project.title,
            locale: project.locale,
            doc_title: meta.title,
            pov: meta.pov,
            synopsis: meta.synopsis,
            cards: self
                .memory()
                .map(|m| m.cards_for(text_before, ghost::CARDS_BUDGET))
                .unwrap_or_default(),
        }))
    }

    /// Start a ghost-text request. Progress arrives as `ai.ghost` events keyed
    /// by `req.request_id`; this call returns as soon as the job is queued.
    /// Any keystroke cancels it (`note_keystroke`), and so does `ghost_cancel`.
    pub async fn ghost_start(self: &Arc<Self>, req: GhostRequest) -> Result<()> {
        let id = req.request_id.clone();
        let token = CancellationToken::new();
        {
            let mut g = lock(&self.ghosts);
            if let Some(i) = g.early_cancelled.iter().position(|x| *x == id) {
                g.early_cancelled.remove(i);
                drop(g);
                self.publish_ghost(&id, GhostState::Cancelled, String::new(), None);
                return Ok(());
            }
            g.active.insert(id.clone(), token.clone());
        }
        let me = self.clone();
        let doc_id = req.doc_id.clone();
        let before = req.text_before.clone();
        let ctx = tokio::task::spawn_blocking(move || me.ghost_context(&doc_id, &before))
            .await
            .map_err(|e| MarginError::Internal(e.to_string()))
            .and_then(|r| r);
        let ctx = match ctx {
            Ok(Some(c)) => c,
            Ok(None) => {
                lock(&self.ghosts).active.remove(&id);
                self.publish_ghost(&id, GhostState::Empty, String::new(), None);
                return Ok(());
            }
            Err(e) => {
                lock(&self.ghosts).active.remove(&id);
                return Err(e);
            }
        };
        if token.is_cancelled() {
            lock(&self.ghosts).active.remove(&id);
            self.publish_ghost(&id, GhostState::Cancelled, String::new(), None);
            return Ok(());
        }

        let n_ctx = lock(&self.conn).caps.n_ctx;
        // /props reports the context of one slot. Unknown: stay on the safe side.
        let budget = if n_ctx == 0 {
            6000
        } else {
            n_ctx.saturating_sub(300).min(ghost::BUDGET_TOKENS)
        };
        let built = ghost::build_prompt(&ctx, &req.text_before, budget);
        let sampling = ghost::sampling_for(
            self.active_preset().sampling_for(Role::Ghost),
            req.length,
            req.variant,
        );
        let max_tokens = sampling.max_tokens;
        let mut job = self.submit(JobSpec {
            priority: Priority::P0,
            role: Role::Ghost,
            prompt: built.prompt,
            sampling,
            resumable: false,
            log: true,
        });
        let me = self.clone();
        self.handle.spawn(async move {
            let post = |raw: &str, truncated: bool| {
                ghost::post_process(
                    raw,
                    &PostCtx {
                        before: &built.before,
                        length: req.length,
                        locale: ctx.locale,
                        typography: req.typography,
                        truncated,
                    },
                )
            };
            let mut raw = String::new();
            let mut shown = String::new();
            let mut cancel_sent = false;
            loop {
                // After a cancel, keep draining until the scheduler reports the end.
                let ev = if cancel_sent {
                    job.events.recv().await
                } else {
                    tokio::select! {
                        _ = token.cancelled() => {
                            job.cancel();
                            cancel_sent = true;
                            continue;
                        }
                        ev = job.events.recv() => ev,
                    }
                };
                match ev {
                    Some(scheduler::JobEvent::Delta(d)) => {
                        raw.push_str(&d);
                        // While streaming the last word may be cut off.
                        if let Some(t) = post(&raw, true)
                            && t != shown
                        {
                            shown = t.clone();
                            me.publish_ghost(&id, GhostState::Streaming, t, None);
                        }
                    }
                    Some(scheduler::JobEvent::Finished(c)) => {
                        let (state, text, error) = match c.outcome {
                            Outcome::Completed => {
                                let truncated = c.timings.output_tokens >= max_tokens;
                                match post(&c.text, truncated) {
                                    Some(t) => (GhostState::Done, t, None),
                                    None => (GhostState::Empty, String::new(), None),
                                }
                            }
                            Outcome::Cancelled => (GhostState::Cancelled, String::new(), None),
                            Outcome::Failed => (GhostState::Failed, String::new(), c.error),
                        };
                        lock(&me.ghosts).active.remove(&id);
                        me.publish_ghost(&id, state, text, error);
                        return;
                    }
                    Some(_) => {}
                    None => {
                        lock(&me.ghosts).active.remove(&id);
                        me.publish_ghost(
                            &id,
                            GhostState::Failed,
                            String::new(),
                            Some("the scheduler stopped".into()),
                        );
                        return;
                    }
                }
            }
        });
        Ok(())
    }

    /// A one-off request for the Model panel's "Try it" box.
    pub async fn try_it(&self, text: String) -> Completion {
        let mut sampling = self.active_preset().sampling_for(Role::Test);
        sampling.max_tokens = 96;
        let prompt = Prompt {
            system: "You are a concise assistant.".into(),
            user: text.clone(),
            assistant_prefix: String::new(),
            sections: vec![
                PromptSection {
                    label: "[1] Role instructions".into(),
                    text: "You are a concise assistant.".into(),
                    tokens: 6,
                },
                PromptSection {
                    label: "[5] Text".into(),
                    text: text.clone(),
                    tokens: (text.split_whitespace().count() as u32 * 4 / 3).max(1),
                },
            ],
        };
        self.submit(JobSpec {
            priority: Priority::P1,
            role: Role::Test,
            prompt,
            sampling,
            resumable: false,
            log: true,
        })
        .finish()
        .await
    }

    pub fn client(&self) -> Option<Arc<dyn InferenceClient>> {
        lock(&self.conn).client.clone()
    }

    pub fn handle(&self) -> &tokio::runtime::Handle {
        &self.handle
    }

    pub fn is_ready(&self) -> bool {
        lock(&self.conn).health == Some(Health::Ready)
    }
}

/// GPU memory via `nvidia-smi` (SPEC §6.6). `None` when there is no NVIDIA GPU.
pub async fn read_vram() -> Option<Vram> {
    let mut cmd = tokio::process::Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=memory.used,memory.total",
        "--format=csv,noheader,nounits",
    ])
    .stdin(std::process::Stdio::null())
    .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let out = tokio::time::timeout(Duration::from_secs(3), cmd.output())
        .await
        .ok()?
        .ok()?;
    parse_vram(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_vram(text: &str) -> Option<Vram> {
    let line = text.lines().next()?;
    let mut parts = line.split(',').map(|p| p.trim().parse::<u32>());
    Some(Vram {
        used_mb: parts.next()?.ok()?,
        total_mb: parts.next()?.ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::mock::{MockConfig, MockServer};

    fn service(dir: &std::path::Path) -> Arc<AiService> {
        let bus = Arc::new(EventBus::new());
        let story = Arc::new(StoryService::new(bus.clone()));
        AiService::new(
            dir.to_path_buf(),
            story,
            bus,
            tokio::runtime::Handle::current(),
        )
    }

    #[test]
    fn parses_nvidia_smi_output() {
        assert_eq!(
            parse_vram("10608, 12288\n"),
            Some(Vram {
                used_mb: 10608,
                total_mb: 12288
            })
        );
        assert_eq!(parse_vram(""), None);
        assert_eq!(parse_vram("N/A, N/A"), None);
    }

    #[tokio::test]
    async fn an_old_config_is_migrated_and_written_back_once() {
        let tmp = tempfile::tempdir().unwrap();
        let mut old = presets::default_config();
        old.preset_revision = 2;
        old.preset_id = "gemma4-26b-a4b".into();
        for (id, template) in [
            ("gemma4-26b-a4b", Template::Gemma),
            ("nemo-12b", Template::Mistral),
        ] {
            let mut p = old.presets[0].clone();
            p.id = id.into();
            p.template = template;
            old.presets.push(p);
        }
        let path = tmp.path().join(CONFIG_FILE);
        std::fs::write(&path, serde_json::to_vec_pretty(&old).unwrap()).unwrap();

        let svc = service(tmp.path());
        let cfg = svc.config();
        assert_eq!(cfg.preset_id, presets::DEFAULT_PRESET);
        assert!(cfg.presets.iter().all(|p| p.id.starts_with("bonsai")));
        assert_eq!(svc.active_preset().id, "bonsai-9b-pq2");

        // The file on disk now matches.
        let saved: AiConfig = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.preset_revision, presets::PRESET_REVISION);
        assert_eq!(saved.preset_id, presets::DEFAULT_PRESET);
        assert!(saved.presets.iter().all(|p| p.id.starts_with("bonsai")));

        // Loading a current file does not rewrite it.
        let stamp = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let _again = service(tmp.path());
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), stamp);
    }

    #[tokio::test]
    async fn config_round_trips_and_rejects_remote_endpoints() {
        let tmp = tempfile::tempdir().unwrap();
        let svc = service(tmp.path());
        let mut cfg = svc.config();
        cfg.idle_seconds = 45;
        svc.set_config(cfg.clone()).unwrap();
        assert_eq!(service(tmp.path()).config().idle_seconds, 45);
        cfg.external_url = "http://example.com:8080".into();
        assert!(svc.set_config(cfg.clone()).is_err());
        cfg.external_url = "http://127.0.0.1:8081".into();
        cfg.preset_id = "nope".into();
        assert!(svc.set_config(cfg).is_err());
    }

    #[tokio::test]
    async fn external_mode_connects_probes_and_serves_requests() {
        let mock = MockServer::start(MockConfig::default()).await;
        let tmp = tempfile::tempdir().unwrap();
        let svc = service(tmp.path());
        let mut cfg = svc.config();
        cfg.mode = ConnectionMode::External;
        cfg.external_url = mock.url();
        cfg.preset_id = "bonsai-27b-ptq1".into();
        svc.set_config(cfg).unwrap();
        svc.connect().await.unwrap();
        for _ in 0..100 {
            if svc.status().0 == AiStatus::Ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(svc.status().0, AiStatus::Ready);
        let panel = svc.panel().await;
        assert!(panel.capabilities.native_completion);
        assert_eq!(panel.capabilities.total_slots, 2);
        assert_eq!(panel.capabilities.model, "mock-model");
        assert_eq!(panel.slots.len(), 2);
        let c = svc.try_it("Say hi".into()).await;
        assert_eq!(c.outcome, Outcome::Completed);
        assert!(c.text.contains("quick"));
        assert_eq!(c.timings.cached_tokens, 42);
        assert!(svc.panel().await.last_request.is_some());
    }

    #[tokio::test]
    async fn the_kill_switch_reports_off_and_stops_requests() {
        let mock = MockServer::start(MockConfig::default()).await;
        let tmp = tempfile::tempdir().unwrap();
        let svc = service(tmp.path());
        let mut cfg = svc.config();
        cfg.mode = ConnectionMode::External;
        cfg.external_url = mock.url();
        svc.set_config(cfg.clone()).unwrap();
        svc.connect().await.unwrap();
        cfg.enabled = false;
        svc.set_config(cfg).unwrap();
        assert_eq!(svc.status().0, AiStatus::Off);
        let c = svc.try_it("hi".into()).await;
        assert_eq!(c.outcome, Outcome::Failed);
    }

    #[tokio::test]
    async fn a_server_that_is_still_loading_reports_starting_then_ready() {
        let mock = MockServer::start(MockConfig {
            loading_for: Duration::from_millis(700),
            ..MockConfig::default()
        })
        .await;
        let tmp = tempfile::tempdir().unwrap();
        let svc = service(tmp.path());
        let mut cfg = svc.config();
        cfg.mode = ConnectionMode::External;
        cfg.external_url = mock.url();
        svc.set_config(cfg).unwrap();
        svc.connect().await.unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        let (s, detail) = svc.status();
        assert_eq!(s, AiStatus::Starting);
        assert_eq!(detail, "Loading model");
        for _ in 0..100 {
            if svc.status().0 == AiStatus::Ready {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("never became ready");
    }

    #[tokio::test]
    async fn an_unreachable_external_server_reports_not_running() {
        let tmp = tempfile::tempdir().unwrap();
        let svc = service(tmp.path());
        let mut cfg = svc.config();
        cfg.mode = ConnectionMode::External;
        cfg.external_url = "http://127.0.0.1:1".into();
        svc.set_config(cfg).unwrap();
        svc.connect().await.unwrap();
        // Windows takes ~2 s to report a refused connection.
        for _ in 0..100 {
            if svc.status().0 == AiStatus::NotRunning {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("still {:?}", svc.status());
    }

    // --- Ghost text end to end (SPEC §7, §17 checks 3 and 8) ----------------------

    use crate::project::manifest::{Locale, ProjectType, Section};
    use crate::story::types::{DocMetaPatch, NewProject};
    use std::sync::atomic::Ordering;

    type Seen = Arc<Mutex<Vec<(String, GhostState, String)>>>;

    struct GhostRig {
        _dir: tempfile::TempDir,
        svc: Arc<AiService>,
        story: Arc<StoryService>,
        doc_id: String,
        mock: MockServer,
        seen: Seen,
    }

    async fn ghost_rig() -> GhostRig {
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new());
        let story = Arc::new(StoryService::new(bus.clone()));
        let cfg_dir = dir.path().join("cfg");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        let svc = AiService::new(
            cfg_dir,
            story.clone(),
            bus.clone(),
            tokio::runtime::Handle::current(),
        );
        story
            .create_project(&NewProject {
                parent_dir: dir.path().to_string_lossy().into(),
                title: "The Salt Road".into(),
                author: "J".into(),
                project_type: ProjectType::Fiction,
                locale: Locale::EnCa,
            })
            .unwrap();
        let tree = story.binder_tree().unwrap();
        let ms = tree
            .sections
            .iter()
            .find(|s| s.section == Section::Manuscript)
            .unwrap();
        let doc_id = ms.nodes[0].children[0].id.clone();
        let seen: Seen = Arc::default();
        let s2 = seen.clone();
        bus.subscribe(move |e| {
            if let DomainEvent::AiGhost {
                request_id,
                state,
                text,
                ..
            } = &e.event
            {
                s2.lock()
                    .unwrap()
                    .push((request_id.clone(), *state, text.clone()));
            }
        });
        let mock = MockServer::start(MockConfig::default()).await;
        let mut cfg = svc.config();
        cfg.mode = ConnectionMode::External;
        cfg.external_url = mock.url();
        svc.set_config(cfg).unwrap();
        svc.connect().await.unwrap();
        for _ in 0..100 {
            if svc.status().0 == AiStatus::Ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(svc.status().0, AiStatus::Ready);
        GhostRig {
            _dir: dir,
            svc,
            story,
            doc_id,
            mock,
            seen,
        }
    }

    fn ghost_req(rig: &GhostRig, id: &str) -> GhostRequest {
        GhostRequest {
            request_id: id.into(),
            doc_id: rig.doc_id.clone(),
            text_before: "The ferry had gone.\n\nShe stood at the".into(),
            length: ghost::GhostLength::Phrase,
            variant: 0,
            typography: ghost::Typography {
                quotes: true,
                dashes: true,
                ellipsis: true,
            },
        }
    }

    async fn wait_state(rig: &GhostRig, id: &str, want: GhostState) -> Vec<(GhostState, String)> {
        for _ in 0..150 {
            let mine: Vec<_> = rig
                .seen
                .lock()
                .unwrap()
                .iter()
                .filter(|(i, _, _)| i == id)
                .map(|(_, s, t)| (*s, t.clone()))
                .collect();
            if mine.iter().any(|(s, _)| *s == want) {
                return mine;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!(
            "{id} never reached {want:?}: {:?}",
            rig.seen.lock().unwrap()
        );
    }

    #[tokio::test]
    async fn a_ghost_request_streams_then_finishes_with_the_cleaned_suggestion() {
        let rig = ghost_rig().await;
        rig.svc.ghost_start(ghost_req(&rig, "g1")).await.unwrap();
        let events = wait_state(&rig, "g1", GhostState::Done).await;
        let (state, text) = events.last().unwrap();
        assert_eq!(*state, GhostState::Done);
        // Phrase length: ten words, with the model's own space after "the".
        assert_eq!(text, " The quick brown fox jumps over the lazy dog and");
        // It streamed: growing text arrived before the end.
        let streamed: Vec<_> = events
            .iter()
            .filter(|(s, _)| *s == GhostState::Streaming)
            .collect();
        assert!(streamed.len() >= 3, "{events:?}");
        assert!(streamed.windows(2).all(|w| w[1].1.len() >= w[0].1.len()));
        // The prompt went to slot 0 with the prefilled assistant turn and the
        // scene notes, and was recorded in the context log.
        let body = &rig.mock.state.bodies_for("/completion")[0];
        assert_eq!(body["id_slot"], 0);
        let prompt = body["prompt"].as_str().unwrap();
        assert!(prompt.contains("Canadian English"));
        assert!(prompt.contains("The ferry had gone."));
        assert!(prompt.ends_with("She stood at the"));
        assert_eq!(rig.svc.context_log.list(10).len(), 1);
    }

    #[tokio::test]
    async fn a_keystroke_cancels_a_streaming_ghost_and_nothing_more_arrives() {
        let rig = ghost_rig().await;
        rig.svc.ghost_start(ghost_req(&rig, "g2")).await.unwrap();
        tokio::time::sleep(Duration::from_millis(90)).await;
        rig.svc.note_keystroke();
        let events = wait_state(&rig, "g2", GhostState::Cancelled).await;
        assert!(!events.iter().any(|(s, _)| *s == GhostState::Done));
        tokio::time::sleep(Duration::from_millis(300)).await;
        let after = rig
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(i, _, _)| i == "g2")
            .count();
        assert_eq!(after, events.len(), "no events after the cancel");
        // The server saw the client go away (SPEC §17 check 3).
        assert!(rig.mock.state.disconnected.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn a_cancel_that_overtakes_its_request_stops_it_before_it_starts() {
        let rig = ghost_rig().await;
        rig.svc.ghost_cancel("g3");
        rig.svc.ghost_start(ghost_req(&rig, "g3")).await.unwrap();
        wait_state(&rig, "g3", GhostState::Cancelled).await;
        assert!(rig.mock.state.bodies_for("/completion").is_empty());
        // The remembered cancel is used up: the same id may run again.
        rig.svc.ghost_start(ghost_req(&rig, "g3")).await.unwrap();
        wait_state(&rig, "g3", GhostState::Done).await;
    }

    #[tokio::test]
    async fn a_newer_ghost_request_replaces_the_older_one() {
        let rig = ghost_rig().await;
        rig.svc.ghost_start(ghost_req(&rig, "old")).await.unwrap();
        tokio::time::sleep(Duration::from_millis(60)).await;
        rig.svc.ghost_start(ghost_req(&rig, "new")).await.unwrap();
        wait_state(&rig, "old", GhostState::Cancelled).await;
        wait_state(&rig, "new", GhostState::Done).await;
    }

    #[tokio::test]
    async fn a_document_with_ai_off_gets_no_suggestion_and_no_request() {
        let rig = ghost_rig().await;
        rig.story
            .document_update_meta(
                &rig.doc_id,
                &DocMetaPatch {
                    ai: Some(crate::project::document::AiToggle::Off),
                    ..Default::default()
                },
            )
            .unwrap();
        rig.svc.ghost_start(ghost_req(&rig, "g5")).await.unwrap();
        wait_state(&rig, "g5", GhostState::Empty).await;
        assert!(rig.mock.state.bodies_for("/completion").is_empty());
    }

    #[tokio::test]
    async fn with_the_server_stopped_a_ghost_request_fails_quietly() {
        let rig = ghost_rig().await;
        rig.svc.disconnect().await;
        rig.svc.ghost_start(ghost_req(&rig, "g6")).await.unwrap();
        let events = wait_state(&rig, "g6", GhostState::Failed).await;
        assert_eq!(events.last().unwrap().1, "");
    }
}
