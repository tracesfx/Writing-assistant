//! AI Scheduler (SPEC §6.4).
//!
//! | Priority | Role | Slot | Runs when | Pre-emption |
//! |---|---|---|---|---|
//! | P0 | Ghost text | 0 | typing pause | cancelled by any keystroke |
//! | P1 | On request | 1 | author asks | pre-empts P2 |
//! | P2 | Idle work | 1 | author idle ≥ threshold | paused on keystroke, resumed when idle |
//!
//! P2 never runs while a P0 is in flight. A paused P2 job (pre-empted or held
//! back by typing) goes back to the front of its queue and starts over when
//! the author is idle again.
//!
//! Cancelling means dropping the HTTP stream (see `client.rs`). `note_keystroke`
//! only takes a lock and flips cancellation tokens, so it is safe to call
//! straight from the IPC thread: the stream stops within a millisecond or two.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

use super::client::{CompletionRequest, InferenceClient};
use super::template;
use super::types::*;

#[derive(Debug, Clone)]
pub struct JobSpec {
    pub priority: Priority,
    pub role: Role,
    pub prompt: Prompt,
    pub sampling: Sampling,
    /// Put the job back in the queue instead of failing when it is pre-empted.
    pub resumable: bool,
    /// Record the request in the context log.
    pub log: bool,
}

#[derive(Debug, Clone)]
pub enum JobEvent {
    Started,
    Delta(String),
    /// The job was pre-empted and is back in the queue.
    Paused,
    Finished(Completion),
}

pub struct JobHandle {
    pub id: String,
    pub events: UnboundedReceiver<JobEvent>,
    cancel: CancellationToken,
}

impl JobHandle {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Wait for the job to finish, discarding deltas.
    pub async fn finish(mut self) -> Completion {
        loop {
            match self.events.recv().await {
                Some(JobEvent::Finished(c)) => return c,
                Some(_) => {}
                None => {
                    return Completion {
                        text: String::new(),
                        outcome: Outcome::Failed,
                        error: Some("the scheduler stopped".into()),
                        timings: Timings::default(),
                    };
                }
            }
        }
    }
}

/// What the recorder (context log, status) learns about a finished job.
#[derive(Debug, Clone)]
pub struct FinishedJob {
    pub id: String,
    pub spec: JobSpec,
    pub slot: u32,
    pub raw_prompt: String,
    pub model: String,
    pub completion: Completion,
}

/// What the scheduler needs to know about the current connection.
#[derive(Clone)]
pub struct Target {
    pub client: Option<Arc<dyn InferenceClient>>,
    pub preset: Preset,
    pub total_slots: u32,
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Activity {
    pub running: u32,
    pub queued: u32,
    /// Idle work is waiting for the author to stop typing.
    pub held_for_typing: bool,
}

struct Queued {
    id: String,
    spec: JobSpec,
    tx: UnboundedSender<JobEvent>,
    cancel: CancellationToken,
}

struct Running {
    id: String,
    priority: Priority,
    slot: u32,
    attempt: CancellationToken,
    preempted: Arc<AtomicBool>,
}

struct State {
    enabled: bool,
    idle: Duration,
    last_input: Option<Instant>,
    target: Option<Target>,
    running: Vec<Running>,
    p1: VecDeque<Queued>,
    p2: VecDeque<Queued>,
    next_id: u32,
}

type OnFinish = Arc<dyn Fn(FinishedJob) + Send + Sync>;
type OnChange = Arc<dyn Fn() + Send + Sync>;

struct Inner {
    state: Mutex<State>,
    on_finish: OnFinish,
    on_change: OnChange,
}

#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<Inner>,
    handle: tokio::runtime::Handle,
}

fn slot_for(priority: Priority, total_slots: u32) -> u32 {
    if priority == Priority::P0 || total_slots < 2 {
        0
    } else {
        1
    }
}

impl Scheduler {
    /// Must be created inside (or given) a tokio runtime. A background tick
    /// restarts held idle work once the author has been quiet long enough.
    pub fn new(handle: tokio::runtime::Handle, on_finish: OnFinish, on_change: OnChange) -> Self {
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                enabled: true,
                idle: Duration::from_secs(20),
                last_input: None,
                target: None,
                running: vec![],
                p1: VecDeque::new(),
                p2: VecDeque::new(),
                next_id: 1,
            }),
            on_finish,
            on_change,
        });
        let s = Scheduler { inner, handle };
        let weak: Weak<Inner> = Arc::downgrade(&s.inner);
        let h2 = s.handle.clone();
        s.handle.spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(100));
            loop {
                tick.tick().await;
                let Some(inner) = weak.upgrade() else { break };
                Scheduler {
                    inner,
                    handle: h2.clone(),
                }
                .pump();
            }
        });
        s
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic in another thread must not take the scheduler down.
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Point the scheduler at a (new) server. `None` = nothing to talk to.
    pub fn set_target(&self, target: Option<Target>) {
        let mut cancel = vec![];
        {
            let mut s = self.lock();
            if target.is_none() {
                cancel.extend(s.running.iter().map(|r| r.attempt.clone()));
            }
            s.target = target;
        }
        for c in cancel {
            c.cancel();
        }
        (self.inner.on_change)();
    }

    pub fn set_idle_threshold(&self, idle: Duration) {
        self.lock().idle = idle;
    }

    /// The master switch (SPEC §2.10): off cancels everything in flight.
    pub fn set_enabled(&self, enabled: bool) {
        let mut cancel = vec![];
        {
            let mut s = self.lock();
            s.enabled = enabled;
            if !enabled {
                cancel.extend(s.running.iter().map(|r| r.attempt.clone()));
                cancel.extend(s.p1.iter().chain(s.p2.iter()).map(|q| q.cancel.clone()));
            }
        }
        for c in cancel {
            c.cancel();
        }
        self.pump();
    }

    pub fn activity(&self) -> Activity {
        let s = self.lock();
        let held = !s.p2.is_empty() && !idle_enough(&s);
        Activity {
            running: s.running.len() as u32,
            queued: (s.p1.len() + s.p2.len()) as u32,
            held_for_typing: held,
        }
    }

    /// Every keystroke: cancel ghost text, pause idle work (SPEC §2.2, §6.4).
    pub fn note_keystroke(&self) {
        let mut s = self.lock();
        s.last_input = Some(Instant::now());
        for r in &s.running {
            match r.priority {
                Priority::P0 => r.attempt.cancel(),
                Priority::P2 => {
                    r.preempted.store(true, Ordering::SeqCst);
                    r.attempt.cancel();
                }
                Priority::P1 => {}
            }
        }
        drop(s);
        (self.inner.on_change)();
    }

    pub fn submit(&self, spec: JobSpec) -> JobHandle {
        let (tx, rx) = unbounded_channel();
        let cancel = CancellationToken::new();
        let id;
        let refuse;
        {
            let mut s = self.lock();
            id = format!("job-{}", s.next_id);
            s.next_id += 1;
            refuse = if !s.enabled {
                Some("AI is turned off")
            } else if s.target.as_ref().and_then(|t| t.client.as_ref()).is_none() {
                Some("the model server is not running")
            } else {
                None
            };
            if refuse.is_none() {
                let q = Queued {
                    id: id.clone(),
                    spec: spec.clone(),
                    tx: tx.clone(),
                    cancel: cancel.clone(),
                };
                match spec.priority {
                    Priority::P0 => {
                        // A newer ghost request replaces the older one.
                        for r in s.running.iter().filter(|r| r.priority == Priority::P0) {
                            r.attempt.cancel();
                        }
                        // Idle work yields to ghost text.
                        for r in s.running.iter().filter(|r| r.priority == Priority::P2) {
                            r.preempted.store(true, Ordering::SeqCst);
                            r.attempt.cancel();
                        }
                        self.start(&mut s, q);
                    }
                    Priority::P1 => s.p1.push_back(q),
                    Priority::P2 => s.p2.push_back(q),
                }
            }
        }
        if let Some(why) = refuse {
            let _ = tx.send(JobEvent::Finished(Completion {
                text: String::new(),
                outcome: Outcome::Failed,
                error: Some(why.into()),
                timings: Timings::default(),
            }));
        }
        self.pump();
        JobHandle {
            id,
            events: rx,
            cancel,
        }
    }

    /// Start whatever may start now. Cheap; called after every change and on a tick.
    pub fn pump(&self) {
        let mut finished_cancelled: Vec<Queued> = vec![];
        {
            let mut s = self.lock();
            if !s.enabled || s.target.is_none() {
                // Nothing can run. Queued jobs wait (or are cancelled by `set_enabled`).
                drop_cancelled(&mut s, &mut finished_cancelled);
            } else {
                drop_cancelled(&mut s, &mut finished_cancelled);
                self.start_ready(&mut s);
            }
        }
        for q in finished_cancelled {
            let _ = q.tx.send(JobEvent::Finished(Completion {
                text: String::new(),
                outcome: Outcome::Cancelled,
                error: None,
                timings: Timings::default(),
            }));
        }
        (self.inner.on_change)();
    }

    fn start_ready(&self, s: &mut State) {
        let total = s.target.as_ref().map_or(1, |t| t.total_slots);
        // P1: next in line, pre-empting idle work on its slot.
        while let Some(front) = s.p1.front() {
            let slot = slot_for(front.spec.priority, total);
            match s.running.iter().find(|r| r.slot == slot) {
                None => {}
                Some(r) if r.priority == Priority::P2 => {
                    r.preempted.store(true, Ordering::SeqCst);
                    r.attempt.cancel();
                    break; // starts once the slot is released
                }
                Some(_) => break,
            }
            if let Some(q) = s.p1.pop_front() {
                self.start(s, q);
            }
        }
        // P2: only while idle, never during P0, one at a time.
        let p0_running = s.running.iter().any(|r| r.priority == Priority::P0);
        let p1_waiting = !s.p1.is_empty();
        let p2_running = s.running.iter().any(|r| r.priority == Priority::P2);
        let slot = slot_for(Priority::P2, total);
        let slot_busy = s.running.iter().any(|r| r.slot == slot);
        if !p0_running
            && !p1_waiting
            && !p2_running
            && !slot_busy
            && idle_enough(s)
            && let Some(q) = s.p2.pop_front()
        {
            self.start(s, q);
        }
    }

    /// Spawn the job's run task. The caller holds the state lock.
    fn start(&self, s: &mut State, q: Queued) {
        let Some(target) = s.target.clone() else {
            return;
        };
        let Some(client) = target.client.clone() else {
            return;
        };
        let slot = slot_for(q.spec.priority, target.total_slots);
        let attempt = q.cancel.child_token();
        let preempted = Arc::new(AtomicBool::new(false));
        s.running.push(Running {
            id: q.id.clone(),
            priority: q.spec.priority,
            slot,
            attempt: attempt.clone(),
            preempted: preempted.clone(),
        });
        let me = self.clone();
        self.handle.spawn(async move {
            me.run_job(q, client, target, slot, attempt, preempted)
                .await;
        });
    }

    async fn run_job(
        &self,
        q: Queued,
        client: Arc<dyn InferenceClient>,
        target: Target,
        slot: u32,
        attempt: CancellationToken,
        preempted: Arc<AtomicBool>,
    ) {
        let _ = q.tx.send(JobEvent::Started);
        let raw_prompt = template::render(&target.preset, &q.spec.prompt);
        let mut sampling = q.spec.sampling.clone();
        for stop in &target.preset.stop {
            if !sampling.stop.contains(stop) {
                sampling.stop.push(stop.clone());
            }
        }
        let req = CompletionRequest {
            raw_prompt: raw_prompt.clone(),
            system: q.spec.prompt.system.clone(),
            user: q.spec.prompt.user.clone(),
            assistant_prefix: q.spec.prompt.assistant_prefix.clone(),
            sampling,
            slot: Some(slot),
            disable_thinking: target.preset.thinking != ThinkingSwitch::None,
        };
        let (dtx, mut drx) = unbounded_channel::<String>();
        let events = q.tx.clone();
        let forward = tokio::spawn(async move {
            while let Some(d) = drx.recv().await {
                let _ = events.send(JobEvent::Delta(d));
            }
        });
        let completion = client.complete(req, dtx, attempt).await;
        let _ = forward.await;

        let requeue = completion.outcome == Outcome::Cancelled
            && preempted.load(Ordering::SeqCst)
            && q.spec.resumable
            && !q.cancel.is_cancelled();
        {
            let mut s = self.lock();
            s.running.retain(|r| r.id != q.id);
            if requeue {
                let _ = q.tx.send(JobEvent::Paused);
                s.p2.push_front(Queued {
                    id: q.id.clone(),
                    spec: q.spec.clone(),
                    tx: q.tx.clone(),
                    cancel: q.cancel.clone(),
                });
            }
        }
        if !requeue {
            (self.inner.on_finish)(FinishedJob {
                id: q.id.clone(),
                spec: q.spec.clone(),
                slot,
                raw_prompt,
                model: target.model.clone(),
                completion: completion.clone(),
            });
            let _ = q.tx.send(JobEvent::Finished(completion));
        }
        self.pump();
    }
}

fn idle_enough(s: &State) -> bool {
    s.last_input.is_none_or(|t| t.elapsed() >= s.idle)
}

/// Move queued jobs whose owner cancelled them out of the queues.
fn drop_cancelled(s: &mut State, out: &mut Vec<Queued>) {
    for q in [&mut s.p1, &mut s.p2] {
        let mut keep = VecDeque::new();
        while let Some(j) = q.pop_front() {
            if j.cancel.is_cancelled() {
                out.push(j);
            } else {
                keep.push_back(j);
            }
        }
        *q = keep;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::client::LlamaClient;
    use crate::ai::mock::{MockConfig, MockServer};
    use crate::ai::presets::builtin_presets;
    use std::sync::atomic::Ordering::SeqCst;

    fn spec(priority: Priority, role: Role, resumable: bool) -> JobSpec {
        JobSpec {
            priority,
            role,
            prompt: Prompt {
                system: "sys".into(),
                user: "user".into(),
                assistant_prefix: "She opened the".into(),
                sections: vec![],
            },
            sampling: default_sampling(role),
            resumable,
            log: true,
        }
    }

    async fn setup(
        cfg: MockConfig,
        slots: u32,
    ) -> (MockServer, Scheduler, Arc<Mutex<Vec<FinishedJob>>>) {
        let mock = MockServer::start(cfg).await;
        let client: Arc<dyn InferenceClient> = Arc::new(LlamaClient::new(&mock.url()).unwrap());
        let finished = Arc::new(Mutex::new(vec![]));
        let f2 = finished.clone();
        let sched = Scheduler::new(
            tokio::runtime::Handle::current(),
            Arc::new(move |j| f2.lock().unwrap().push(j)),
            Arc::new(|| {}),
        );
        sched.set_target(Some(Target {
            client: Some(client),
            preset: builtin_presets()[1].clone(),
            total_slots: slots,
            model: "mock".into(),
        }));
        sched.set_idle_threshold(Duration::from_millis(150));
        (mock, sched, finished)
    }

    async fn until(mut f: impl FnMut() -> bool) {
        for _ in 0..400 {
            if f() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("condition not reached in 4 s");
    }

    fn slow() -> MockConfig {
        MockConfig {
            token_delay: Duration::from_millis(30),
            ..MockConfig::default()
        }
    }

    #[tokio::test]
    async fn a_keystroke_cancels_ghost_text_within_the_budget() {
        let (mock, sched, finished) = setup(slow(), 2).await;
        let mut h = sched.submit(spec(Priority::P0, Role::Ghost, false));
        // Wait until tokens are flowing.
        loop {
            if let Some(JobEvent::Delta(_)) = h.events.recv().await {
                break;
            }
        }
        let t0 = Instant::now();
        sched.note_keystroke();
        let done = loop {
            if let Some(JobEvent::Finished(c)) = h.events.recv().await {
                break c;
            }
        };
        let cancel_ms = t0.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(done.outcome, Outcome::Cancelled);
        // SPEC §14: stream stopped < 30 ms after the keystroke reaches the backend.
        assert!(cancel_ms < 30.0, "cancel took {cancel_ms:.1} ms");
        // The server saw the connection close (SPEC §17 check 3).
        until(|| mock.state.disconnected.load(SeqCst) == 1).await;
        let seen = mock.state.last_disconnect.lock().unwrap().unwrap();
        assert!(seen.saturating_duration_since(t0) < Duration::from_millis(100));
        assert_eq!(finished.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn ghost_uses_slot_0_and_other_roles_slot_1() {
        let (mock, sched, _) = setup(MockConfig::default(), 2).await;
        sched
            .submit(spec(Priority::P0, Role::Ghost, false))
            .finish()
            .await;
        sched
            .submit(spec(Priority::P1, Role::Ideas, false))
            .finish()
            .await;
        let slots: Vec<_> = mock
            .state
            .bodies_for("/completion")
            .iter()
            .map(|b| b["id_slot"].as_u64().unwrap())
            .collect();
        assert_eq!(slots, vec![0, 1]);
    }

    #[tokio::test]
    async fn a_single_slot_server_gets_everything_on_slot_0() {
        let (mock, sched, _) = setup(MockConfig::default(), 1).await;
        sched
            .submit(spec(Priority::P1, Role::Ask, false))
            .finish()
            .await;
        assert_eq!(mock.state.bodies_for("/completion")[0]["id_slot"], 0);
    }

    #[tokio::test]
    async fn requests_carry_the_prompt_stops_and_the_thinking_prefill() {
        let (mock, sched, _) = setup(MockConfig::default(), 2).await;
        sched
            .submit(spec(Priority::P0, Role::Ghost, false))
            .finish()
            .await;
        let b = &mock.state.bodies_for("/completion")[0];
        let p = b["prompt"].as_str().unwrap();
        assert!(p.ends_with("<think>\n\n</think>\n\nShe opened the"), "{p}");
        let stops: Vec<_> = b["stop"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        assert!(stops.contains(&"\n") && stops.contains(&"<|im_end|>"));
        assert_eq!(b["cache_prompt"], true);
    }

    #[tokio::test]
    async fn idle_work_waits_for_quiet_and_is_paused_by_typing() {
        let (_mock, sched, finished) = setup(slow(), 2).await;
        sched.note_keystroke();
        let mut h = sched.submit(spec(Priority::P2, Role::Extraction, true));
        // Held while the author has just typed.
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(sched.activity().held_for_typing);
        assert_eq!(sched.activity().running, 0);
        // Starts once idle.
        loop {
            if let Some(JobEvent::Started) = h.events.recv().await {
                break;
            }
        }
        // Typing pauses it; it goes back to the queue, not to "finished".
        loop {
            if let Some(JobEvent::Delta(_)) = h.events.recv().await {
                break;
            }
        }
        sched.note_keystroke();
        loop {
            if let Some(JobEvent::Paused) = h.events.recv().await {
                break;
            }
        }
        assert_eq!(finished.lock().unwrap().len(), 0);
        // And resumes, completing from scratch, when quiet again.
        let c = h.finish().await;
        assert_eq!(c.outcome, Outcome::Completed);
        assert!(!c.text.is_empty());
    }

    #[tokio::test]
    async fn on_request_work_preempts_idle_work_which_then_resumes() {
        let (mock, sched, _) = setup(slow(), 2).await;
        let mut idle = sched.submit(spec(Priority::P2, Role::Extraction, true));
        loop {
            if let Some(JobEvent::Delta(_)) = idle.events.recv().await {
                break;
            }
        }
        let on_request = sched.submit(spec(Priority::P1, Role::Ask, false));
        loop {
            if let Some(JobEvent::Paused) = idle.events.recv().await {
                break;
            }
        }
        let a = on_request.finish().await;
        assert_eq!(a.outcome, Outcome::Completed);
        let b = idle.finish().await;
        assert_eq!(b.outcome, Outcome::Completed);
        // idle (cancelled) + ask + idle (resumed) = 3 server calls.
        assert_eq!(mock.state.bodies_for("/completion").len(), 3);
    }

    #[tokio::test]
    async fn idle_work_never_runs_while_ghost_text_is_in_flight() {
        let (mock, sched, _) = setup(slow(), 2).await;
        let mut idle = sched.submit(spec(Priority::P2, Role::Extraction, true));
        loop {
            if let Some(JobEvent::Delta(_)) = idle.events.recv().await {
                break;
            }
        }
        let ghost = sched.submit(spec(Priority::P0, Role::Ghost, false));
        loop {
            if let Some(JobEvent::Paused) = idle.events.recv().await {
                break;
            }
        }
        // While ghost is in flight, idle work is not running.
        assert_eq!(sched.activity().running, 1);
        ghost.finish().await;
        assert_eq!(idle.finish().await.outcome, Outcome::Completed);
        assert_eq!(mock.state.streams_completed.load(SeqCst), 2);
    }

    #[tokio::test]
    async fn a_newer_ghost_request_replaces_the_older_one() {
        let (_mock, sched, _) = setup(slow(), 2).await;
        let first = sched.submit(spec(Priority::P0, Role::Ghost, false));
        tokio::time::sleep(Duration::from_millis(80)).await;
        let second = sched.submit(spec(Priority::P0, Role::Ghost, false));
        assert_eq!(first.finish().await.outcome, Outcome::Cancelled);
        assert_eq!(second.finish().await.outcome, Outcome::Completed);
    }

    #[tokio::test]
    async fn cancelling_a_queued_job_finishes_it_without_calling_the_server() {
        let (mock, sched, _) = setup(slow(), 2).await;
        let busy = sched.submit(spec(Priority::P1, Role::Ask, false));
        let queued = sched.submit(spec(Priority::P1, Role::Ideas, false));
        queued.cancel();
        assert_eq!(queued.finish().await.outcome, Outcome::Cancelled);
        busy.finish().await;
        assert_eq!(mock.state.bodies_for("/completion").len(), 1);
    }

    #[tokio::test]
    async fn the_kill_switch_cancels_everything_and_refuses_new_work() {
        let (_mock, sched, _) = setup(slow(), 2).await;
        let h = sched.submit(spec(Priority::P1, Role::Ask, false));
        tokio::time::sleep(Duration::from_millis(60)).await;
        sched.set_enabled(false);
        assert_eq!(h.finish().await.outcome, Outcome::Cancelled);
        let refused = sched
            .submit(spec(Priority::P1, Role::Ask, false))
            .finish()
            .await;
        assert_eq!(refused.outcome, Outcome::Failed);
        assert_eq!(refused.error.as_deref(), Some("AI is turned off"));
    }

    #[tokio::test]
    async fn without_a_server_requests_fail_fast() {
        let (_mock, sched, _) = setup(slow(), 2).await;
        sched.set_target(None);
        let c = sched
            .submit(spec(Priority::P0, Role::Ghost, false))
            .finish()
            .await;
        assert_eq!(c.outcome, Outcome::Failed);
    }

    /// Opt-in (`MARGIN_LIVE_MODEL=1`, optional `MARGIN_LIVE_URL`): a keystroke
    /// stops generation on a real llama-server, seen from its /slots state.
    #[tokio::test]
    async fn live_keystroke_stops_generation_on_the_real_server() {
        if std::env::var("MARGIN_LIVE_MODEL").is_err() {
            return;
        }
        let url =
            std::env::var("MARGIN_LIVE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
        let client: Arc<dyn InferenceClient> = Arc::new(LlamaClient::new(&url).unwrap());
        let caps = client.probe(None).await;
        assert!(caps.reachable, "no server at {url}");
        let sched = Scheduler::new(
            tokio::runtime::Handle::current(),
            Arc::new(|_| {}),
            Arc::new(|| {}),
        );
        sched.set_target(Some(Target {
            client: Some(client.clone()),
            preset: builtin_presets()[1].clone(),
            total_slots: caps.total_slots,
            model: caps.model.clone(),
        }));
        let mut s = spec(Priority::P0, Role::Ghost, false);
        s.sampling.max_tokens = 400;
        s.sampling.stop.clear();
        s.prompt.user = "Write a long story about a lighthouse keeper.".into();
        s.prompt.assistant_prefix = String::new();
        let mut h = sched.submit(s);
        let mut tokens = 0;
        while tokens < 5 {
            if let Some(JobEvent::Delta(_)) = h.events.recv().await {
                tokens += 1;
            }
        }
        assert!(
            client.slots().await.iter().any(|s| s.id == 0 && s.busy),
            "slot 0 should be generating"
        );
        let t0 = Instant::now();
        sched.note_keystroke();
        let done = loop {
            if let Some(JobEvent::Finished(c)) = h.events.recv().await {
                break c;
            }
        };
        let cancel_ms = t0.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(done.outcome, Outcome::Cancelled);
        assert!(cancel_ms < 30.0, "cancel took {cancel_ms:.1} ms");
        // The server stops generating: slot 0 goes idle well before 400 tokens would finish.
        let mut idle = false;
        for _ in 0..40 {
            if !client.slots().await.iter().any(|s| s.id == 0 && s.busy) {
                idle = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(idle, "slot 0 still busy 1 s after the keystroke");
        eprintln!("live cancel: {cancel_ms:.1} ms to Finished(Cancelled)");
    }
}
