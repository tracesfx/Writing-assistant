//! Event bus (SPEC §5.2): typed, versioned domain events.
//!
//! Backend services subscribe in-process; the Tauri layer subscribes once and
//! forwards every event to the frontend as a single `marginEvent` channel. The
//! Module SDK dispatches on `event.type`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use specta::Type;

/// Version of the event envelope and payload shapes. Bump on breaking changes
/// and add a migration note in `docs/sdk.md`.
pub const EVENTS_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type")]
pub enum DomainEvent {
    #[serde(rename = "project.opened", rename_all = "camelCase")]
    ProjectOpened {
        project_id: String,
        title: String,
        root: String,
    },
    #[serde(rename = "project.closed", rename_all = "camelCase")]
    ProjectClosed { project_id: String },
    /// Structure or metadata of binder nodes changed.
    #[serde(rename = "binder.changed", rename_all = "camelCase")]
    BinderChanged {
        reason: String,
        doc_ids: Vec<String>,
    },
    #[serde(rename = "doc.saved", rename_all = "camelCase")]
    DocSaved {
        doc_id: String,
        revision: String,
        word_count: u32,
    },
    /// A document's file changed outside Margin.
    #[serde(rename = "doc.changedOnDisk", rename_all = "camelCase")]
    DocChangedOnDisk { doc_id: String },
    #[serde(rename = "project.settingsChanged", rename_all = "camelCase")]
    ProjectSettingsChanged { project_id: String },
    /// Margin rewrote these documents' text outside the editor (replace-all,
    /// history restore, merge, split, quick capture). Open editors reload.
    #[serde(rename = "doc.contentReplaced", rename_all = "camelCase")]
    DocContentReplaced {
        doc_ids: Vec<String>,
        reason: String,
    },
    /// Writing statistics changed (words written, active time, goals).
    #[serde(rename = "stats.updated", rename_all = "camelCase")]
    StatsUpdated { today_words: i32 },
    /// A history snapshot was taken.
    #[serde(rename = "history.snapshot", rename_all = "camelCase")]
    HistorySnapshot { snapshot_id: String, name: String },
    /// The model server's status changed (SPEC §6.7).
    #[serde(rename = "ai.status", rename_all = "camelCase")]
    AiStatus {
        status: crate::ai::types::AiStatus,
        detail: String,
    },
    /// Progress of one ghost-text request (SPEC §7). `text` is the whole
    /// suggestion so far, already post-processed.
    #[serde(rename = "ai.ghost", rename_all = "camelCase")]
    AiGhost {
        request_id: String,
        state: crate::ai::types::GhostState,
        text: String,
        error: Option<String>,
    },
    /// The AI-accepted ranges of a document were saved (SPEC §7.6).
    #[serde(rename = "ai.provenanceChanged", rename_all = "camelCase")]
    ProvenanceChanged { doc_id: String, ai_words: u32 },
    /// A bake-off step started or the run ended (SPEC §6.6).
    #[serde(rename = "ai.bakeoffProgress", rename_all = "camelCase")]
    BakeoffProgress {
        preset_id: String,
        stage: String,
        finished: bool,
    },
    #[serde(rename = "backup.finished", rename_all = "camelCase")]
    BackupFinished {
        path: Option<String>,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct MarginEvent {
    pub v: u32,
    pub seq: u32,
    pub event: DomainEvent,
}

type Subscriber = Arc<dyn Fn(&MarginEvent) + Send + Sync>;

#[derive(Default)]
pub struct EventBus {
    subscribers: RwLock<Vec<Subscriber>>,
    seq: AtomicU32,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&self, f: impl Fn(&MarginEvent) + Send + Sync + 'static) {
        if let Ok(mut subs) = self.subscribers.write() {
            subs.push(Arc::new(f));
        }
    }

    pub fn publish(&self, event: DomainEvent) {
        let envelope = MarginEvent {
            v: EVENTS_VERSION,
            seq: self.seq.fetch_add(1, Ordering::Relaxed),
            event,
        };
        // Clone the list so subscribers may publish without deadlocking.
        let subs: Vec<Subscriber> = match self.subscribers.read() {
            Ok(s) => s.clone(),
            Err(_) => return,
        };
        for s in subs {
            s(&envelope);
        }
    }

    pub fn publish_all(&self, events: impl IntoIterator<Item = DomainEvent>) {
        for e in events {
            self.publish(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn delivers_in_order_with_sequence_numbers() {
        let bus = EventBus::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        bus.subscribe(move |e| s2.lock().unwrap().push((e.seq, e.event.clone())));
        bus.publish(DomainEvent::ProjectClosed {
            project_id: "a".into(),
        });
        bus.publish(DomainEvent::DocChangedOnDisk { doc_id: "d".into() });
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].0 + 1, seen[1].0);
    }

    #[test]
    fn serializes_with_dotted_type_tags() {
        let e = DomainEvent::DocSaved {
            doc_id: "x".into(),
            revision: "r".into(),
            word_count: 3,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "type": "doc.saved", "docId": "x", "revision": "r", "wordCount": 3 })
        );
    }
}
