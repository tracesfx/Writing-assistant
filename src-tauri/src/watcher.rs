//! File watcher (SPEC §11.3 "External edits"): watches the open project
//! folder and reports changed documents to the Story Data Service, which
//! ignores Margin's own writes by content hash.

use std::path::PathBuf;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::events::{DomainEvent, EventBus};
use crate::story::{APP_DIR, StoryService};

const DEBOUNCE: Duration = Duration::from_millis(400);

pub struct ProjectWatcher {
    current: Mutex<Option<RecommendedWatcher>>,
}

impl ProjectWatcher {
    pub fn new(story: Arc<StoryService>, bus: Arc<EventBus>) -> Arc<Self> {
        let w = Arc::new(ProjectWatcher {
            current: Mutex::new(None),
        });
        let weak = Arc::downgrade(&w);
        bus.subscribe(move |e| {
            let Some(w) = weak.upgrade() else { return };
            match &e.event {
                DomainEvent::ProjectOpened { root, .. } => {
                    w.watch(story.clone(), PathBuf::from(root))
                }
                DomainEvent::ProjectClosed { .. } => {
                    if let Ok(mut c) = w.current.lock() {
                        *c = None;
                    }
                }
                _ => {}
            }
        });
        w
    }

    fn watch(&self, story: Arc<StoryService>, root: PathBuf) {
        let (tx, rx) = channel::<PathBuf>();
        let app_dir = root.join(APP_DIR);
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                for p in ev.paths {
                    if !p.starts_with(&app_dir) {
                        let _ = tx.send(p);
                    }
                }
            }
        });
        let mut watcher = match watcher {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[margin] file watcher unavailable: {e}");
                return;
            }
        };
        if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
            eprintln!("[margin] cannot watch {}: {e}", root.display());
            return;
        }
        // Debounce on a worker thread; it ends when the watcher (and with it
        // the sender) is dropped.
        std::thread::spawn(move || {
            loop {
                let Ok(first) = rx.recv() else { return };
                let mut batch = vec![first];
                loop {
                    match rx.recv_timeout(DEBOUNCE) {
                        Ok(p) => batch.push(p),
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                batch.sort();
                batch.dedup();
                if let Err(e) = story.on_fs_change(&batch) {
                    eprintln!("[margin] handling outside changes failed: {e}");
                }
            }
        });
        if let Ok(mut c) = self.current.lock() {
            *c = Some(watcher);
        }
    }
}
