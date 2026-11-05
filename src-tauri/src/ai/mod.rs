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
#[cfg(test)]
pub mod mock;
pub mod presets;
pub mod scheduler;
pub mod supervisor;
pub mod template;
pub mod types;

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
use scheduler::{Activity, FinishedJob, JobHandle, JobSpec, Scheduler, Target};
use supervisor::{ProcState, Supervisor, launch_for};
use types::*;

const CONFIG_FILE: &str = "ai-config.json";

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
        presets::upgrade(&mut config);
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
}
