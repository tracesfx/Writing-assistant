//! Managed mode (SPEC §6.1): launch and supervise `llama-server` from a
//! user-chosen binary and model with preset arguments. Restarts on crash up
//! to 3 times, then gives up and says so once.
//!
//! The supervisor only tracks the *process*. Whether the model has finished
//! loading is the health probe's job (`AiService`).

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::types::Preset;
use crate::error::{MarginError, Result};

pub const MAX_RESTARTS: u32 = 3;
const LOG_LINES: usize = 400;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, PartialEq)]
pub enum ProcState {
    Stopped,
    Running,
    /// Gave up after the restart limit, or could not start.
    Crashed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    pub binary: String,
    pub args: Vec<String>,
}

/// Command line for a preset: the model, loopback host, port, context and
/// slots, then the preset's own flags.
pub fn launch_for(preset: &Preset, port: u32) -> Launch {
    let mut args = vec![
        "-m".to_string(),
        preset.model.clone(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "-c".into(),
        preset.context_size.to_string(),
        "--parallel".into(),
        preset.parallel.to_string(),
    ];
    args.extend(preset.args.iter().cloned());
    Launch {
        binary: preset.binary.clone(),
        args,
    }
}

struct Shared {
    state: ProcState,
    restarts: u32,
    stop: Option<CancellationToken>,
    /// Bumped on every start so a stale task cannot overwrite newer state.
    generation: u32,
    /// Set the first time the restart limit is hit, until the next start.
    gave_up_notified: bool,
}

pub struct Supervisor {
    shared: Mutex<Shared>,
    log: Mutex<VecDeque<String>>,
    on_change: Arc<dyn Fn() + Send + Sync>,
}

impl Supervisor {
    pub fn new(on_change: Arc<dyn Fn() + Send + Sync>) -> Arc<Self> {
        Arc::new(Supervisor {
            shared: Mutex::new(Shared {
                state: ProcState::Stopped,
                restarts: 0,
                stop: None,
                generation: 0,
                gave_up_notified: false,
            }),
            log: Mutex::new(VecDeque::new()),
            on_change,
        })
    }

    fn shared(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn state(&self) -> ProcState {
        self.shared().state.clone()
    }

    pub fn restarts(&self) -> u32 {
        self.shared().restarts
    }

    pub fn log_tail(&self, n: usize) -> Vec<String> {
        let l = self.log.lock().unwrap_or_else(|e| e.into_inner());
        l.iter().rev().take(n).rev().cloned().collect()
    }

    pub fn last_log_line(&self) -> Option<String> {
        self.log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .back()
            .cloned()
    }

    fn push_log(&self, line: String) {
        let mut l = self.log.lock().unwrap_or_else(|e| e.into_inner());
        if l.len() >= LOG_LINES {
            l.pop_front();
        }
        l.push_back(line);
    }

    /// Returns true exactly once per start when the restart limit was hit.
    pub fn take_gave_up(&self) -> bool {
        let mut s = self.shared();
        let hit = matches!(s.state, ProcState::Crashed(_)) && !s.gave_up_notified;
        if hit {
            s.gave_up_notified = true;
        }
        hit
    }

    /// Launch the server. Any server this supervisor already runs is stopped first.
    pub async fn start(self: &Arc<Self>, launch: Launch) -> Result<()> {
        if !std::path::Path::new(&launch.binary).is_file() {
            return Err(MarginError::NotFound(format!(
                "llama-server not found at {}",
                launch.binary
            )));
        }
        if let Some(m) = launch
            .args
            .windows(2)
            .find(|w| w[0] == "-m")
            .map(|w| w[1].clone())
            && !std::path::Path::new(&m).is_file()
        {
            return Err(MarginError::NotFound(format!("model file not found: {m}")));
        }
        self.stop().await;
        let token = CancellationToken::new();
        let generation = {
            let mut s = self.shared();
            s.generation += 1;
            s.restarts = 0;
            s.gave_up_notified = false;
            s.state = ProcState::Running;
            s.stop = Some(token.clone());
            s.generation
        };
        self.push_log(format!(
            "[margin] starting {} {}",
            launch.binary,
            launch.args.join(" ")
        ));
        let me = self.clone();
        tokio::spawn(async move { me.supervise(launch, token, generation).await });
        (self.on_change)();
        Ok(())
    }

    /// Stop the server and wait (briefly) until the process is gone.
    pub async fn stop(&self) {
        let token = self.shared().stop.take();
        if let Some(t) = token {
            t.cancel();
            for _ in 0..100 {
                if self.shared().state != ProcState::Running {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }

    async fn supervise(self: Arc<Self>, launch: Launch, token: CancellationToken, generation: u32) {
        loop {
            let mut cmd = Command::new(&launch.binary);
            cmd.args(&launch.args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            #[cfg(windows)]
            cmd.creation_flags(CREATE_NO_WINDOW);
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    self.finish(
                        generation,
                        ProcState::Crashed(format!("could not start: {e}")),
                    );
                    return;
                }
            };
            if let Some(out) = child.stdout.take() {
                self.pipe(out);
            }
            if let Some(err) = child.stderr.take() {
                self.pipe(err);
            }
            tokio::select! {
                _ = token.cancelled() => {
                    let _ = child.kill().await;
                    self.push_log("[margin] server stopped".into());
                    self.finish(generation, ProcState::Stopped);
                    return;
                }
                status = child.wait() => {
                    let why = match status {
                        Ok(s) => format!("llama-server exited ({s})"),
                        Err(e) => format!("llama-server failed: {e}"),
                    };
                    self.push_log(format!("[margin] {why}"));
                    let n = {
                        let mut s = self.shared();
                        if s.generation != generation { return; }
                        s.restarts += 1;
                        s.restarts
                    };
                    (self.on_change)();
                    if n > MAX_RESTARTS {
                        self.finish(generation, ProcState::Crashed(format!("{why}; gave up after {MAX_RESTARTS} restarts")));
                        return;
                    }
                    self.push_log(format!("[margin] restarting ({n}/{MAX_RESTARTS})"));
                    tokio::select! {
                        _ = token.cancelled() => { self.finish(generation, ProcState::Stopped); return; }
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    }
                }
            }
        }
    }

    fn finish(&self, generation: u32, state: ProcState) {
        {
            let mut s = self.shared();
            if s.generation != generation {
                return;
            }
            s.state = state;
        }
        (self.on_change)();
    }

    fn pipe(self: &Arc<Self>, reader: impl AsyncRead + Unpin + Send + 'static) {
        let me = self.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                me.push_log(line);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::presets::builtin_presets;

    #[test]
    fn launch_line_binds_loopback_and_carries_preset_flags() {
        let p = &builtin_presets()[0];
        let l = launch_for(p, 8080);
        let a = l.args.join(" ");
        assert!(a.starts_with("-m D:/MODELS/gemma4/"));
        assert!(a.contains("--host 127.0.0.1 --port 8080"));
        assert!(a.contains("-c 32768 --parallel 2"));
        assert!(a.contains("--n-cpu-moe 17"));
    }

    #[tokio::test]
    async fn refuses_a_missing_binary_or_model_with_a_clear_message() {
        let sup = Supervisor::new(Arc::new(|| {}));
        let e = sup
            .start(Launch {
                binary: "Z:/nope/llama-server.exe".into(),
                args: vec![],
            })
            .await
            .unwrap_err();
        assert!(e.to_string().contains("llama-server not found"));
        // A real file as the "binary" but a missing model.
        let exe = std::env::current_exe().unwrap();
        let e = sup
            .start(Launch {
                binary: exe.to_string_lossy().into(),
                args: vec!["-m".into(), "Z:/nope/model.gguf".into()],
            })
            .await
            .unwrap_err();
        assert!(e.to_string().contains("model file not found"));
    }

    #[tokio::test]
    async fn restarts_a_crashing_server_three_times_then_gives_up_once() {
        let sup = Supervisor::new(Arc::new(|| {}));
        // `cmd /c exit 1` is a binary that exits immediately.
        let cmd = std::env::var("ComSpec").unwrap_or_else(|_| "C:/Windows/System32/cmd.exe".into());
        sup.start(Launch {
            binary: cmd,
            args: vec!["/c".into(), "exit".into(), "1".into()],
        })
        .await
        .unwrap();
        for _ in 0..200 {
            if matches!(sup.state(), ProcState::Crashed(_)) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            matches!(sup.state(), ProcState::Crashed(_)),
            "{:?}",
            sup.state()
        );
        assert_eq!(sup.restarts(), MAX_RESTARTS + 1);
        assert!(sup.take_gave_up());
        assert!(!sup.take_gave_up(), "notified only once");
        assert!(
            sup.log_tail(50)
                .iter()
                .any(|l| l.contains("restarting (3/3)"))
        );
    }

    #[tokio::test]
    async fn stop_kills_a_running_process() {
        let sup = Supervisor::new(Arc::new(|| {}));
        let cmd = std::env::var("ComSpec").unwrap_or_else(|_| "C:/Windows/System32/cmd.exe".into());
        // `ping -n 30` runs ~30 s.
        sup.start(Launch {
            binary: cmd,
            args: vec![
                "/c".into(),
                "ping".into(),
                "-n".into(),
                "30".into(),
                "127.0.0.1".into(),
            ],
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(sup.state(), ProcState::Running);
        sup.stop().await;
        assert_eq!(sup.state(), ProcState::Stopped);
    }
}
