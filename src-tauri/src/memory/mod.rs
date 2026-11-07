//! Story Memory (SPEC §9): the bible, scene cards, the timeline, paragraph
//! ids, retrieval, and the background indexer.
//!
//! * `types`, `store`   — the records and the canon rules (§9.1, §9.3)
//! * `paragraphs`       — stable paragraph ids (sidecar maps)
//! * `extract`, `merge` — reading a scene with the model, checking and merging it
//! * `search`           — BM25 + embeddings
//! * `indexer`          — the idle-only pipeline (§9.4)
//! * `margin`           — Ideas, Ask my book, Phrasing (§8)
//! * `export`           — JSON, CSV, Markdown, GraphML (§9.6)
//!
//! Everything the model tracks is a plain file in the project folder and can be
//! read, edited and exported by the author. The AI only ever writes `inferred`
//! records (enforced in `store`).

pub mod export;
pub mod extract;
pub mod indexer;
pub mod margin;
pub mod mentions;
pub mod merge;
pub mod paragraphs;
pub mod search;
pub mod store;
pub mod types;
pub mod views;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use crate::ai::AiService;
use crate::ai::supervisor::{Launch, Supervisor};
use crate::error::{MarginError, Result};
use crate::events::{DomainEvent, EventBus};
use crate::project::atomic::atomic_write;
use crate::project::document::{AiToggle, DocKind};
use crate::project::manifest::Section;
use crate::story::StoryService;
use crate::story::types::BinderNode;
use paragraphs::{Para, ParaStore};
use search::VectorIndex;
use store::{EntityPatch, ItemKind, ItemRef, Store};
use types::*;
use views::*;

const CONFIG_FILE: &str = "memory-config.json";

/// What the last `index.progress` event said: state, pending, total, current scene.
type Progress = (IndexState, u32, u32, Option<String>);

#[derive(Clone)]
pub(crate) struct DocData {
    pub title: String,
    pub paras: Vec<Para>,
    pub text_hash: String,
    /// All paragraph text lowercased and joined by newlines (mention counting).
    pub lower: String,
    pub words: u32,
}

#[derive(Default, Clone)]
pub(crate) struct Layout {
    /// Manuscript documents with text, in binder order.
    pub order: Vec<String>,
    pub titles: HashMap<String, String>,
    pub parent: HashMap<String, String>,
    pub kinds: HashMap<String, DocKind>,
    pub ai_off: HashSet<String>,
}

#[derive(Default)]
pub(crate) struct Inner {
    pub store: Option<Store>,
    pub docs: HashMap<String, DocData>,
    pub layout: Layout,
    pub vectors: Option<VectorIndex>,
    pub changed_at: HashMap<String, Instant>,
    pub retry_after: HashMap<String, Instant>,
    pub focus: Option<String>,
    pub busy: bool,
    pub current: Option<String>,
    pub last_error: Option<String>,
    /// Bumps on every change, to drop caches.
    pub version: u64,
    pub mention_cache: Option<(u64, HashMap<String, u32>)>,
    /// Bumps when a project opens or closes, so old background work gives up.
    pub generation: u64,
}

pub struct MemoryService {
    pub(crate) story: Arc<StoryService>,
    pub(crate) bus: Arc<EventBus>,
    pub(crate) ai: Arc<AiService>,
    pub(crate) handle: tokio::runtime::Handle,
    cfg_path: PathBuf,
    config: Mutex<MemoryConfig>,
    pub(crate) inner: Mutex<Inner>,
    pub embed_supervisor: Arc<Supervisor>,
    pub(crate) embed_ready: AtomicBool,
    pub(crate) tick_lock: tokio::sync::Mutex<()>,
    last_progress: Mutex<Option<Progress>>,
}

/// "that night" or "that night (story position 2.5)": what the Inspector and the exports show for an event.
pub(crate) fn story_time_text(t: Option<&StoryTime>) -> String {
    match t {
        None => String::new(),
        Some(t) => match t.order {
            Some(o) if t.label.is_empty() => format!("story position {o}"),
            Some(o) => format!("{} (story position {o})", t.label),
            None => t.label.clone(),
        },
    }
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn flatten(nodes: &[BinderNode], parent: Option<&str>, lay: &mut Layout) {
    for n in nodes {
        if n.word_count > 0 {
            lay.order.push(n.id.clone());
        }
        lay.titles.insert(n.id.clone(), n.title.clone());
        lay.kinds.insert(n.id.clone(), n.kind);
        if n.ai == AiToggle::Off {
            lay.ai_off.insert(n.id.clone());
        }
        if let Some(p) = parent {
            lay.parent.insert(n.id.clone(), p.to_string());
        }
        flatten(&n.children, Some(&n.id), lay);
    }
}

impl MemoryService {
    pub fn new(
        config_dir: PathBuf,
        story: Arc<StoryService>,
        ai: Arc<AiService>,
        bus: Arc<EventBus>,
        handle: tokio::runtime::Handle,
    ) -> Arc<Self> {
        let cfg_path = config_dir.join(CONFIG_FILE);
        let config = std::fs::read(&cfg_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<MemoryConfig>(&b).ok())
            .unwrap_or_default();
        let svc = Arc::new(MemoryService {
            story,
            bus: bus.clone(),
            ai: ai.clone(),
            handle: handle.clone(),
            cfg_path,
            config: Mutex::new(config),
            inner: Mutex::new(Inner::default()),
            embed_supervisor: Supervisor::new(Arc::new(|| {})),
            embed_ready: AtomicBool::new(false),
            tick_lock: tokio::sync::Mutex::new(()),
            last_progress: Mutex::new(None),
        });
        ai.set_memory(Arc::downgrade(&svc));

        let weak = Arc::downgrade(&svc);
        bus.subscribe(move |e| {
            let Some(s) = weak.upgrade() else { return };
            match &e.event {
                DomainEvent::ProjectOpened { .. } => {
                    let s2 = s.clone();
                    s.handle.spawn_blocking(move || {
                        if let Err(err) = s2.load_project() {
                            eprintln!("[margin] could not load story memory: {err}");
                        }
                    });
                }
                DomainEvent::ProjectClosed { .. } => s.unload(),
                DomainEvent::DocSaved { doc_id, .. } => {
                    let (s2, id) = (s.clone(), doc_id.clone());
                    s.handle.spawn_blocking(move || s2.doc_changed(&[id], true));
                }
                DomainEvent::DocContentReplaced { doc_ids, .. } => {
                    let (s2, ids) = (s.clone(), doc_ids.clone());
                    s.handle.spawn_blocking(move || s2.doc_changed(&ids, true));
                }
                DomainEvent::BinderChanged { doc_ids, .. } => {
                    let (s2, ids) = (s.clone(), doc_ids.clone());
                    s.handle.spawn_blocking(move || s2.doc_changed(&ids, false));
                }
                _ => {}
            }
        });

        // The indexer's heartbeat.
        let weak = Arc::downgrade(&svc);
        handle.spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let Some(s) = weak.upgrade() else { return };
                s.tick().await;
            }
        });
        svc
    }

    // --- Configuration -----------------------------------------------------------

    pub fn config(&self) -> MemoryConfig {
        lock(&self.config).clone()
    }

    pub fn set_config(&self, mut new: MemoryConfig) -> Result<MemoryConfig> {
        new.schema_version = 1;
        new.settle_seconds = new.settle_seconds.min(3600);
        if new.embedding_port == 0 || new.embedding_port > 65535 {
            return Err(MarginError::Invalid("port must be 1–65535".into()));
        }
        atomic_write(&self.cfg_path, &serde_json::to_vec_pretty(&new)?)?;
        *lock(&self.config) = new.clone();
        self.publish_progress();
        Ok(new)
    }

    // --- Project lifecycle -------------------------------------------------------

    fn load_project(&self) -> Result<()> {
        let root = self.story.project_root()?;
        let store = Store::load(&root);
        for p in &store.problems {
            eprintln!("[margin] story memory: {p}");
        }
        let vectors = VectorIndex::open(&root);
        let gen_ = {
            let mut i = lock(&self.inner);
            i.generation += 1;
            i.store = Some(store);
            i.vectors = Some(vectors);
            i.docs.clear();
            i.changed_at.clear();
            i.retry_after.clear();
            i.busy = false;
            i.current = None;
            i.last_error = None;
            i.generation
        };
        let ids = self.refresh_layout()?;
        let para_store = ParaStore::new(&root);
        for id in &ids {
            self.read_doc(&para_store, id);
        }
        {
            let mut i = lock(&self.inner);
            if i.generation == gen_ {
                i.version += 1;
            }
        }
        self.publish_progress();
        if self.config().embedding_enabled && self.config().embedding_auto_start {
            let me = self.handle.clone();
            let cfg = self.config();
            let sup = self.embed_supervisor.clone();
            me.spawn(async move {
                let _ = sup.start(embedding_launch(&cfg)).await;
            });
        }
        Ok(())
    }

    fn unload(&self) {
        {
            let mut i = lock(&self.inner);
            if let Some(v) = i.vectors.as_mut() {
                let _ = v.flush();
            }
            *i = Inner {
                generation: i.generation + 1,
                ..Inner::default()
            };
        }
        self.publish_progress();
    }

    /// Recompute the manuscript order and titles. Returns the document ids.
    pub(crate) fn refresh_layout(&self) -> Result<Vec<String>> {
        let tree = self.story.binder_tree()?;
        let mut lay = Layout::default();
        if let Some(ms) = tree
            .sections
            .iter()
            .find(|s| s.section == Section::Manuscript)
        {
            flatten(&ms.nodes, None, &mut lay);
        }
        let ids = lay.order.clone();
        let mut i = lock(&self.inner);
        // Documents that left the manuscript (or the Trash) leave memory's corpus.
        i.docs.retain(|k, _| lay.order.contains(k));
        for (k, d) in i.docs.iter_mut() {
            if let Some(t) = lay.titles.get(k) {
                d.title = t.clone();
            }
        }
        i.layout = lay;
        i.version += 1;
        Ok(ids)
    }

    /// Read a document, reconcile its paragraph ids and put it in the corpus.
    fn read_doc(&self, para_store: &ParaStore, id: &str) {
        let Ok(ast) = self.story.document_ast(id) else {
            return;
        };
        let Ok(file) = para_store.update(id, &ast) else {
            return;
        };
        let mut i = lock(&self.inner);
        let title = i.layout.titles.get(id).cloned().unwrap_or_default();
        let joined = file
            .paras
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let words = crate::dialect::count_words(&joined);
        i.docs.insert(
            id.to_string(),
            DocData {
                title,
                lower: joined.to_lowercase(),
                words,
                text_hash: file.text_hash,
                paras: file.paras,
            },
        );
    }

    /// A document was saved, replaced, or the binder changed.
    fn doc_changed(&self, ids: &[String], text_changed: bool) {
        let Ok(root) = self.story.project_root() else {
            return;
        };
        if lock(&self.inner).store.is_none() {
            return;
        }
        let Ok(manuscript) = self.refresh_layout() else {
            return;
        };
        let para_store = ParaStore::new(&root);
        for id in ids {
            if !manuscript.contains(id) {
                continue;
            }
            if text_changed {
                lock(&self.inner)
                    .changed_at
                    .insert(id.clone(), Instant::now());
            }
            self.read_doc(&para_store, id);
        }
        // Documents the layout now holds but the corpus lacks (just un-trashed).
        for id in &manuscript {
            if !lock(&self.inner).docs.contains_key(id) {
                self.read_doc(&para_store, id);
            }
        }
        lock(&self.inner).version += 1;
        self.publish_progress();
    }

    /// The editor tells which scene the author is in: it is read first.
    pub fn focus(&self, doc_id: Option<String>) {
        lock(&self.inner).focus = doc_id;
    }

    // --- Helpers -----------------------------------------------------------------

    pub(crate) fn with_store<T>(&self, f: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
        let mut i = lock(&self.inner);
        let out = {
            let s = i.store.as_mut().ok_or(MarginError::NoProject)?;
            f(s)?
        };
        i.version += 1;
        Ok(out)
    }

    pub(crate) fn read_store<T>(&self, f: impl FnOnce(&Store, &Inner) -> Result<T>) -> Result<T> {
        let i = lock(&self.inner);
        let s = i.store.as_ref().ok_or(MarginError::NoProject)?;
        f(s, &i)
    }

    pub(crate) fn emit(&self, entity_ids: Vec<String>, scene_ids: Vec<String>, kinds: &[&str]) {
        self.bus.publish(DomainEvent::MemoryUpdated {
            entity_ids,
            scene_ids,
            kinds: kinds.iter().map(|k| k.to_string()).collect(),
        });
        self.publish_progress();
    }

    /// Lowercased text of every document memory may use (not forgotten, not AI-off).
    pub(crate) fn memory_text(i: &Inner) -> String {
        i.layout
            .order
            .iter()
            .filter(|d| !Self::is_excluded(i, d))
            .filter_map(|d| i.docs.get(d).map(|x| x.lower.as_str()))
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    }

    pub(crate) fn is_excluded(i: &Inner, doc: &str) -> bool {
        i.layout.ai_off.contains(doc)
            || i.store
                .as_ref()
                .is_some_and(|s| s.project.excluded.iter().any(|d| d == doc))
    }

    /// Mention counts per entry id over the whole manuscript (cached).
    fn mention_counts(i: &mut Inner) -> HashMap<String, u32> {
        if let Some((v, m)) = &i.mention_cache
            && *v == i.version
        {
            return m.clone();
        }
        let mut out = HashMap::new();
        if let Some(store) = &i.store {
            for e in store.entities.values() {
                let names = Store::names_of(e);
                let n: u32 = i
                    .layout
                    .order
                    .iter()
                    .filter_map(|d| i.docs.get(d))
                    .map(|d| mentions::count_in_lower(&names, &d.lower))
                    .sum();
                out.insert(e.id.clone(), n);
            }
        }
        i.mention_cache = Some((i.version, out.clone()));
        out
    }

    fn scene_ref(i: &Inner, id: &str) -> Option<SceneRef> {
        let order = i.layout.order.iter().position(|d| d == id)?;
        Some(SceneRef {
            id: id.to_string(),
            title: i.layout.titles.get(id).cloned().unwrap_or_default(),
            order: order as u32,
        })
    }

    fn summary_of(e: &Entity, mentions: u32) -> EntitySummary {
        EntitySummary {
            id: e.id.clone(),
            kind: e.kind.clone(),
            name: e.name.clone(),
            status: e.status,
            created_by: e.created_by,
            aliases: e.aliases.iter().map(|a| a.text.clone()).collect(),
            fact_count: e.facts.len() as u32,
            inferred_fact_count: e
                .facts
                .iter()
                .filter(|f| f.status == Status::Inferred)
                .count() as u32,
            relation_count: e.relations.len() as u32,
            mention_count: mentions,
            updated: e.updated.clone(),
        }
    }

    // --- Queries (SPEC §5.1) -----------------------------------------------------

    pub fn entities_list(
        &self,
        kind: Option<&str>,
        query: Option<&str>,
    ) -> Result<Vec<EntitySummary>> {
        let mut i = lock(&self.inner);
        let counts = Self::mention_counts(&mut i);
        let store = i.store.as_ref().ok_or(MarginError::NoProject)?;
        let q = query.map(normalize).filter(|q| !q.is_empty());
        let mut out: Vec<EntitySummary> = store
            .entities
            .values()
            .filter(|e| kind.is_none_or(|k| e.kind == k))
            .filter(|e| {
                q.as_ref().is_none_or(|q| {
                    Store::names_of(e)
                        .iter()
                        .any(|n| normalize(n).contains(q.as_str()))
                        || e.facts
                            .iter()
                            .any(|f| normalize(&f.text).contains(q.as_str()))
                })
            })
            .map(|e| Self::summary_of(e, counts.get(&e.id).copied().unwrap_or(0)))
            .collect();
        out.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    }

    fn relation_view(store: &Store, from: &Entity, r: &Relation) -> RelationView {
        RelationView {
            id: r.id.clone(),
            from: from.id.clone(),
            from_name: from.name.clone(),
            to: r.to.clone(),
            to_name: store
                .entities
                .get(&r.to)
                .map_or_else(|| "(deleted)".into(), |e| e.name.clone()),
            kind: r.kind.clone(),
            directed: r.directed,
            status: r.status,
            source: r.source.clone(),
            since: r.since.clone(),
            created_by: r.created_by,
        }
    }

    fn mention_rows(i: &Inner, e: &Entity) -> Vec<MentionRow> {
        let names = Store::names_of(e);
        let mut out = vec![];
        for (n, id) in i.layout.order.iter().enumerate() {
            let Some(d) = i.docs.get(id) else { continue };
            let count = mentions::count_in_lower(&names, &d.lower);
            if count == 0 {
                continue;
            }
            let first = d
                .paras
                .iter()
                .find(|p| mentions::count_in_lower(&names, &p.text.to_lowercase()) > 0);
            out.push(MentionRow {
                scene_id: id.clone(),
                scene_title: d.title.clone(),
                count,
                first: first.map(|p| Source {
                    doc: id.clone(),
                    para: Some(p.id.clone()),
                    block: Some(p.block),
                    quote: p.text.chars().take(140).collect(),
                }),
            });
            let _ = n;
        }
        out
    }

    pub fn entity_get(&self, id: &str) -> Result<EntityDetail> {
        self.read_store(|s, i| {
            let e = s
                .entities
                .get(id)
                .ok_or_else(|| MarginError::NotFound(format!("entry {id}")))?;
            let relations_out = e
                .relations
                .iter()
                .map(|r| Self::relation_view(s, e, r))
                .collect();
            let relations_in = s
                .entities
                .values()
                .flat_map(|o| {
                    o.relations
                        .iter()
                        .filter(|r| r.to == id)
                        .map(move |r| (o, r))
                })
                .map(|(o, r)| Self::relation_view(s, o, r))
                .collect();
            Ok(EntityDetail {
                entity: e.clone(),
                relations_in,
                relations_out,
                mentions: Self::mention_rows(i, e),
            })
        })
    }

    pub fn relations_list(
        &self,
        entity: Option<&str>,
        kind: Option<&str>,
    ) -> Result<Vec<RelationView>> {
        self.read_store(|s, _| {
            Ok(s.entities
                .values()
                .flat_map(|e| e.relations.iter().map(move |r| (e, r)))
                .filter(|(e, r)| {
                    entity.is_none_or(|x| e.id == x || r.to == x)
                        && kind.is_none_or(|k| r.kind == k)
                })
                .filter(|(_, r)| s.entities.contains_key(&r.to))
                .map(|(e, r)| Self::relation_view(s, e, r))
                .collect())
        })
    }

    pub fn relations_graph(&self) -> Result<RelationGraph> {
        let mut i = lock(&self.inner);
        let counts = Self::mention_counts(&mut i);
        let s = i.store.as_ref().ok_or(MarginError::NoProject)?;
        let edges: Vec<RelationView> = s
            .entities
            .values()
            .flat_map(|e| e.relations.iter().map(move |r| (e, r)))
            .filter(|(_, r)| s.entities.contains_key(&r.to))
            .map(|(e, r)| Self::relation_view(s, e, r))
            .collect();
        let used: HashSet<&str> = edges
            .iter()
            .flat_map(|e| [e.from.as_str(), e.to.as_str()])
            .collect();
        let nodes = s
            .entities
            .values()
            .filter(|e| used.contains(e.id.as_str()))
            .map(|e| GraphNode {
                id: e.id.clone(),
                name: e.name.clone(),
                kind: e.kind.clone(),
                mentions: counts.get(&e.id).copied().unwrap_or(0),
            })
            .collect();
        Ok(RelationGraph { nodes, edges })
    }

    pub fn mentions_by_entity(&self, id: &str) -> Result<Vec<MentionRow>> {
        self.read_store(|s, i| {
            let e = s
                .entities
                .get(id)
                .ok_or_else(|| MarginError::NotFound(format!("entry {id}")))?;
            Ok(Self::mention_rows(i, e))
        })
    }

    pub fn mentions_matrix(&self, types: &[String]) -> Result<MentionMatrix> {
        let mut i = lock(&self.inner);
        let counts = Self::mention_counts(&mut i);
        let s = i.store.as_ref().ok_or(MarginError::NoProject)?;
        let ents: Vec<&Entity> = s
            .entities
            .values()
            .filter(|e| types.is_empty() || types.contains(&e.kind))
            .collect();
        let mut cells = vec![];
        for e in &ents {
            let names = Store::names_of(e);
            for id in &i.layout.order {
                if let Some(d) = i.docs.get(id) {
                    let c = mentions::count_in_lower(&names, &d.lower);
                    if c > 0 {
                        cells.push(MatrixCell {
                            entity_id: e.id.clone(),
                            scene_id: id.clone(),
                            count: c,
                        });
                    }
                }
            }
        }
        let scenes = i
            .layout
            .order
            .iter()
            .filter_map(|id| Self::scene_ref(&i, id))
            .collect();
        let entities = ents
            .iter()
            .map(|e| Self::summary_of(e, counts.get(&e.id).copied().unwrap_or(0)))
            .collect();
        Ok(MentionMatrix {
            entities,
            scenes,
            cells,
        })
    }

    pub fn timeline_events(&self) -> Result<Vec<EventView>> {
        self.read_store(|s, i| {
            let mut v: Vec<EventView> = s
                .events
                .iter()
                .map(|ev| EventView {
                    event: ev.clone(),
                    entity_names: ev
                        .entities
                        .iter()
                        .filter_map(|id| s.entities.get(id).map(|e| e.name.clone()))
                        .collect(),
                    scene_title: ev
                        .narrative_pos
                        .as_ref()
                        .and_then(|d| i.layout.titles.get(d).cloned()),
                })
                .collect();
            let pos = |ev: &EventView| {
                ev.event
                    .narrative_pos
                    .as_ref()
                    .and_then(|d| i.layout.order.iter().position(|x| x == d))
                    .unwrap_or(usize::MAX)
            };
            v.sort_by_key(pos);
            Ok(v)
        })
    }

    pub fn threads_list(&self, status: Option<ThreadStatus>) -> Result<Vec<ThreadView>> {
        self.read_store(|s, i| {
            let mut out = vec![];
            for e in s.entities.values().filter(|e| e.thread.is_some()) {
                let t = e.thread.clone().unwrap_or_default();
                if status.is_some_and(|st| st != t.status) {
                    continue;
                }
                let mut touches: Vec<SceneRef> = t
                    .touches
                    .iter()
                    .filter_map(|d| Self::scene_ref(i, d))
                    .collect();
                touches.sort_by_key(|r| r.order);
                out.push(ThreadView {
                    id: e.id.clone(),
                    title: e.name.clone(),
                    status: t.status,
                    status_canon: t.status_canon,
                    introduced_in: t
                        .introduced_in
                        .as_deref()
                        .and_then(|d| Self::scene_ref(i, d)),
                    touches,
                    resolved_in: t.resolved_in.as_deref().and_then(|d| Self::scene_ref(i, d)),
                    entry_status: e.status,
                });
            }
            Ok(out)
        })
    }

    pub fn scene_cards(&self) -> Result<Vec<SceneCardView>> {
        self.read_store(|s, i| {
            Ok(i.layout
                .order
                .iter()
                .enumerate()
                .filter_map(|(n, id)| {
                    s.scenes.get(id).map(|c| SceneCardView {
                        card: c.clone(),
                        title: i.layout.titles.get(id).cloned().unwrap_or_default(),
                        order: n as u32,
                    })
                })
                .collect())
        })
    }

    /// One numeric value per scene (`words`, `dialogueRatio`, `entityCount`, or an
    /// extractor's metric), in manuscript order.
    pub fn metrics_series(&self, key: &str) -> Result<Vec<MetricPoint>> {
        self.read_store(|s, i| {
            let mut out = vec![];
            for (n, id) in i.layout.order.iter().enumerate() {
                let title = i.layout.titles.get(id).cloned().unwrap_or_default();
                let live = i.docs.get(id);
                let stored = s.scenes.get(id).and_then(|c| c.metrics.get(key).copied());
                let value = match (key, live) {
                    // Built-in metrics follow the text, not the last extraction.
                    ("words", Some(d)) => Some(d.words as f64),
                    ("dialogueRatio", Some(d)) => Some(mentions::dialogue_ratio(
                        &d.paras
                            .iter()
                            .map(|p| p.text.as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )),
                    ("entityCount", Some(d)) => Some(
                        s.entities
                            .values()
                            .filter(|e| e.kind != "thread")
                            .filter(|e| mentions::count_in_lower(&Store::names_of(e), &d.lower) > 0)
                            .count() as f64,
                    ),
                    _ => stored,
                };
                if let Some(value) = value {
                    out.push(MetricPoint {
                        scene_id: id.clone(),
                        title,
                        order: n as u32,
                        value,
                    });
                }
            }
            Ok(out)
        })
    }

    pub fn bible_types(&self) -> Vec<(String, String)> {
        let pt = self
            .story
            .project_info()
            .map(|p| p.project_type)
            .unwrap_or(crate::project::manifest::ProjectType::Fiction);
        use crate::project::manifest::ProjectType as P;
        let mut v: Vec<(&str, &str)> = vec![];
        if matches!(pt, P::Fiction | P::Both) {
            v.extend(FICTION_TYPES);
        }
        if matches!(pt, P::NonFiction | P::Both) {
            v.extend(NONFICTION_TYPES);
        }
        v.extend(COMMON_TYPES);
        v.into_iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    // --- Passages ----------------------------------------------------------------

    pub(crate) fn all_paras(i: &Inner) -> Vec<(&str, &DocData, &Para)> {
        i.layout
            .order
            .iter()
            .filter(|d| !Self::is_excluded(i, d))
            .filter_map(|d| i.docs.get(d).map(|dd| (d.as_str(), dd)))
            .flat_map(|(d, dd)| dd.paras.iter().map(move |p| (d, dd, p)))
            .collect()
    }

    /// Lexical search over the manuscript.
    pub fn search_text(&self, query: &str, k: usize) -> Result<Vec<PassageHit>> {
        let i = lock(&self.inner);
        if i.store.is_none() {
            return Err(MarginError::NoProject);
        }
        let all = Self::all_paras(&i);
        let texts: Vec<&str> = all.iter().map(|(_, _, p)| p.text.as_str()).collect();
        Ok(search::bm25(query, &texts, k)
            .into_iter()
            .map(|(n, score)| {
                let (d, dd, p) = &all[n];
                PassageHit {
                    doc: d.to_string(),
                    doc_title: dd.title.clone(),
                    para: p.id.clone(),
                    block: p.block,
                    text: p.text.clone(),
                    score,
                }
            })
            .collect())
    }

    /// Hybrid search: BM25, plus embeddings when the embedding server is up.
    pub async fn search_semantic(&self, query: &str, k: usize) -> Result<Vec<PassageHit>> {
        let lexical = self.search_text(query, k * 2)?;
        let Some(embedder) = self.embedder() else {
            return Ok(lexical.into_iter().take(k).collect());
        };
        use search::Embedder;
        let qv = match embedder.embed(vec![query.to_string()]).await {
            Ok(mut v) => v.remove(0),
            Err(_) => return Ok(lexical.into_iter().take(k).collect()),
        };
        let i = lock(&self.inner);
        let Some(vecs) = i.vectors.as_ref() else {
            return Ok(lexical.into_iter().take(k).collect());
        };
        let near: Vec<String> = vecs
            .nearest(&qv, k * 2)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let lex_ids: Vec<String> = lexical.iter().map(|h| h.para.clone()).collect();
        let fused = search::fuse(&[lex_ids, near], k);
        let all = Self::all_paras(&i);
        Ok(fused
            .into_iter()
            .filter_map(|(id, score)| {
                all.iter()
                    .find(|(_, _, p)| p.id == id)
                    .map(|(d, dd, p)| PassageHit {
                        doc: d.to_string(),
                        doc_title: dd.title.clone(),
                        para: p.id.clone(),
                        block: p.block,
                        text: p.text.clone(),
                        score,
                    })
            })
            .collect())
    }

    /// Where a stored source points now (paragraph ids survive edits; the block
    /// is only a hint).
    pub fn resolve_source(&self, src: &Source) -> Result<Option<PassageHit>> {
        self.read_store(|_, i| {
            let Some(dd) = i.docs.get(&src.doc) else {
                return Ok(None);
            };
            let p = src
                .para
                .as_ref()
                .and_then(|id| dd.paras.iter().find(|p| p.id == *id))
                .or_else(|| {
                    let q = normalize(&src.quote);
                    (!q.is_empty())
                        .then(|| dd.paras.iter().find(|p| normalize(&p.text).contains(&q)))
                        .flatten()
                });
            Ok(p.map(|p| PassageHit {
                doc: src.doc.clone(),
                doc_title: dd.title.clone(),
                para: p.id.clone(),
                block: p.block,
                text: p.text.clone(),
                score: 1.0,
            }))
        })
    }

    // --- Commands: the author's bible edits --------------------------------------

    pub fn entity_create(&self, kind: &str, name: &str, notes: &str) -> Result<Entity> {
        let e = self.with_store(|s| s.author_create(kind, name, notes))?;
        self.emit(vec![e.id.clone()], vec![], &["entities"]);
        Ok(e)
    }

    pub fn entity_update(&self, id: &str, patch: &EntityPatch) -> Result<Entity> {
        let e = self.with_store(|s| s.author_update(id, patch))?;
        self.emit(vec![e.id.clone()], vec![], &["entities"]);
        Ok(e)
    }

    pub fn entity_delete(&self, id: &str) -> Result<Entity> {
        let e = self.with_store(|s| s.author_delete(id))?;
        self.emit(vec![id.to_string()], vec![], &["entities"]);
        Ok(e)
    }

    pub fn entity_restore(&self, e: Entity) -> Result<Entity> {
        let e = self.with_store(|s| s.author_restore(e))?;
        self.emit(vec![e.id.clone()], vec![], &["entities"]);
        Ok(e)
    }

    pub fn fact_add(
        &self,
        entity: &str,
        text: &str,
        field: Option<&str>,
        source: Option<Source>,
    ) -> Result<Fact> {
        let f = self.with_store(|s| s.author_add_fact(entity, text, field, source))?;
        self.emit(vec![entity.to_string()], vec![], &["facts"]);
        Ok(f)
    }

    pub fn fact_update(
        &self,
        entity: &str,
        fact: &str,
        text: &str,
        field: Option<&str>,
    ) -> Result<Fact> {
        let f = self.with_store(|s| s.author_update_fact(entity, fact, text, field))?;
        self.emit(vec![entity.to_string()], vec![], &["facts"]);
        Ok(f)
    }

    pub fn fact_delete(&self, entity: &str, fact: &str) -> Result<()> {
        self.with_store(|s| s.author_delete_fact(entity, fact))?;
        self.emit(vec![entity.to_string()], vec![], &["facts"]);
        Ok(())
    }

    pub fn relation_add(
        &self,
        from: &str,
        to: &str,
        kind: &str,
        since: Option<&str>,
    ) -> Result<Relation> {
        let r = self.with_store(|s| s.author_add_relation(from, to, kind, since))?;
        self.emit(
            vec![from.to_string(), to.to_string()],
            vec![],
            &["relations"],
        );
        Ok(r)
    }

    pub fn relation_delete(&self, entity: &str, relation: &str) -> Result<()> {
        self.with_store(|s| s.author_delete_relation(entity, relation))?;
        self.emit(vec![entity.to_string()], vec![], &["relations"]);
        Ok(())
    }

    pub fn make_canon(&self, items: &[ItemRef]) -> Result<u32> {
        self.with_store(|s| {
            for it in items {
                s.author_make_canon(it)?;
            }
            Ok(())
        })?;
        let ents = items
            .iter()
            .map(|i| i.entity.clone().unwrap_or_else(|| i.id.clone()))
            .collect();
        self.emit(ents, vec![], &["entities", "facts", "relations", "events"]);
        Ok(items.len() as u32)
    }

    pub fn set_event_order(&self, id: &str, order: Option<f64>) -> Result<()> {
        self.with_store(|s| s.author_set_event_order(id, order))?;
        self.emit(vec![], vec![], &["events"]);
        Ok(())
    }

    pub fn delete_inferred(&self, items: &[ItemRef]) -> Result<u32> {
        let n = self.with_store(|s| s.delete_inferred(items))?;
        let ents = items
            .iter()
            .map(|i| i.entity.clone().unwrap_or_else(|| i.id.clone()))
            .collect();
        self.emit(ents, vec![], &["entities", "facts", "relations", "events"]);
        Ok(n)
    }

    /// "Forget this document" (SPEC §9.1): excluded from memory entirely.
    pub fn forget_document(&self, doc: &str, forget: bool) -> Result<()> {
        self.with_store(|s| {
            s.project.excluded.retain(|d| d != doc);
            if forget {
                s.project.excluded.push(doc.to_string());
                s.ai_forget_scene(doc)?;
            }
            s.save_project()
        })?;
        let all = self.read_store(|_, i| Ok(Self::memory_text(i)))?;
        let dead = self.with_store(|s| {
            s.prune_orphans(&|e| mentions::count_in_lower(&Store::names_of(e), &all) > 0)
        })?;
        self.emit(
            dead,
            vec![doc.to_string()],
            &["entities", "facts", "relations", "events", "scenes"],
        );
        Ok(())
    }

    pub fn excluded_documents(&self) -> Result<Vec<String>> {
        self.read_store(|s, _| Ok(s.project.excluded.clone()))
    }

    // --- Status ------------------------------------------------------------------

    /// Ids of the extractors the author has on.
    pub(crate) fn wanted_extractors(&self) -> Vec<&'static str> {
        extract::enabled(self.config().extract_tension)
            .iter()
            .map(|x| x.id)
            .collect()
    }

    /// Scenes whose card is missing or out of date: the text changed, or an
    /// extractor was switched on since the card was made.
    pub(crate) fn pending_docs(i: &Inner, want: &[&str]) -> Vec<String> {
        let Some(store) = &i.store else { return vec![] };
        i.layout
            .order
            .iter()
            .filter(|d| !Self::is_excluded(i, d))
            .filter(|d| {
                i.docs.get(*d).is_some_and(|dd| {
                    dd.words >= 12
                        && store.scenes.get(*d).is_none_or(|c| {
                            c.source_hash != dd.text_hash
                                || want.iter().any(|w| !c.extractors.iter().any(|x| x == w))
                        })
                })
            })
            .cloned()
            .collect()
    }

    pub fn status(&self) -> MemoryStatus {
        let cfg = self.config();
        let (pending, total, current, last_error, has_project, entries, vectors, model, busy) = {
            let i = lock(&self.inner);
            let p = Self::pending_docs(&i, &self.wanted_extractors());
            (
                p.len() as u32,
                i.layout
                    .order
                    .iter()
                    .filter(|d| !Self::is_excluded(&i, d))
                    .count() as u32,
                i.current.clone(),
                i.last_error.clone(),
                i.store.is_some(),
                i.store.as_ref().map_or(0, |s| s.entities.len() as u32),
                i.vectors.as_ref().map_or(0, |v| v.len() as u32),
                i.vectors
                    .as_ref()
                    .map(|v| v.model().to_string())
                    .unwrap_or_default(),
                i.busy,
            )
        };
        let ready = self.embed_ready.load(Ordering::Relaxed);
        let embeddings = EmbeddingStatus {
            enabled: cfg.embedding_enabled,
            ready,
            model,
            vectors,
            detail: if !cfg.embedding_enabled {
                "Off: passage search uses keywords".into()
            } else if ready {
                "Ready".into()
            } else {
                "The embedding server is not running: passage search uses keywords".into()
            },
        };
        let ai = self.ai.status().0;
        use crate::ai::types::AiStatus;
        let state = if !has_project || !cfg.indexing || ai == AiStatus::Off {
            IndexState::Off
        } else if busy {
            IndexState::Working
        } else if pending == 0 {
            IndexState::UpToDate
        } else if matches!(ai, AiStatus::NotRunning | AiStatus::Starting) {
            IndexState::NoModel
        } else if ai == AiStatus::PausedTyping {
            IndexState::Paused
        } else {
            IndexState::Pending
        };
        MemoryStatus {
            state,
            pending,
            total,
            current,
            last_error,
            embeddings,
            entries,
        }
    }

    /// Publish `index.progress` when it changed.
    pub(crate) fn publish_progress(&self) {
        let s = self.status();
        let key = (s.state, s.pending, s.total, s.current.clone());
        {
            let mut last = lock(&self.last_progress);
            if last.as_ref() == Some(&key) {
                return;
            }
            *last = Some(key);
        }
        self.bus.publish(DomainEvent::IndexProgress {
            state: s.state,
            pending: s.pending,
            total: s.total,
            current: s.current,
        });
    }

    // --- Memory Inspector rows (SPEC §9.6) --------------------------------------

    pub fn inspector_rows(&self) -> Result<Vec<InspectorRow>> {
        let mut i = lock(&self.inner);
        let counts = Self::mention_counts(&mut i);
        let s = i.store.as_ref().ok_or(MarginError::NoProject)?;
        let mut rows = vec![];
        for e in s.entities.values() {
            if e.thread.is_some() {
                let t = e.thread.clone().unwrap_or_default();
                rows.push(InspectorRow {
                    kind: "thread".into(),
                    id: e.id.clone(),
                    label: e.name.clone(),
                    detail: format!("{:?}, touched in {} scene(s)", t.status, t.touches.len())
                        .to_lowercase(),
                    status: Some(e.status),
                    created_by: Some(e.created_by),
                    updated: e.updated.clone(),
                    source: None,
                    entity_id: Some(e.id.clone()),
                    entity_name: Some(e.name.clone()),
                    item: Some(ItemRef {
                        kind: ItemKind::Entity,
                        id: e.id.clone(),
                        entity: None,
                    }),
                });
            } else {
                rows.push(InspectorRow {
                    kind: "entity".into(),
                    id: e.id.clone(),
                    label: e.name.clone(),
                    detail: format!("{} · {} fact(s)", e.kind, e.facts.len()),
                    status: Some(e.status),
                    created_by: Some(e.created_by),
                    updated: e.updated.clone(),
                    source: None,
                    entity_id: Some(e.id.clone()),
                    entity_name: Some(e.name.clone()),
                    item: Some(ItemRef {
                        kind: ItemKind::Entity,
                        id: e.id.clone(),
                        entity: None,
                    }),
                });
            }
            for f in &e.facts {
                rows.push(InspectorRow {
                    kind: "fact".into(),
                    id: f.id.clone(),
                    label: f.text.clone(),
                    detail: f.field.clone().unwrap_or_default(),
                    status: Some(f.status),
                    created_by: Some(f.created_by),
                    updated: f.updated.clone(),
                    source: f.source.clone(),
                    entity_id: Some(e.id.clone()),
                    entity_name: Some(e.name.clone()),
                    item: Some(ItemRef {
                        kind: ItemKind::Fact,
                        id: f.id.clone(),
                        entity: Some(e.id.clone()),
                    }),
                });
            }
            for r in &e.relations {
                let to = s
                    .entities
                    .get(&r.to)
                    .map_or("(deleted)", |t| t.name.as_str());
                rows.push(InspectorRow {
                    kind: "relation".into(),
                    id: r.id.clone(),
                    label: format!("{} {} {}", e.name, if r.directed { "→" } else { "↔" }, to),
                    detail: r.kind.clone(),
                    status: Some(r.status),
                    created_by: Some(r.created_by),
                    updated: r.updated.clone(),
                    source: r.source.clone(),
                    entity_id: Some(e.id.clone()),
                    entity_name: Some(e.name.clone()),
                    item: Some(ItemRef {
                        kind: ItemKind::Relation,
                        id: r.id.clone(),
                        entity: Some(e.id.clone()),
                    }),
                });
            }
            if let Some(n) = counts.get(&e.id).filter(|n| **n > 0) {
                rows.push(InspectorRow {
                    kind: "mention".into(),
                    id: format!("m-{}", e.id),
                    label: e.name.clone(),
                    detail: format!("{n} mention(s)"),
                    status: None,
                    created_by: None,
                    updated: e.updated.clone(),
                    source: None,
                    entity_id: Some(e.id.clone()),
                    entity_name: Some(e.name.clone()),
                    item: None,
                });
            }
        }
        for ev in &s.events {
            rows.push(InspectorRow {
                kind: "event".into(),
                id: ev.id.clone(),
                label: ev.title.clone(),
                detail: story_time_text(ev.story_time.as_ref()),
                status: Some(ev.status),
                created_by: Some(ev.created_by),
                updated: ev.updated.clone(),
                source: ev.source.clone(),
                entity_id: None,
                entity_name: None,
                item: Some(ItemRef {
                    kind: ItemKind::Event,
                    id: ev.id.clone(),
                    entity: None,
                }),
            });
        }
        for (id, c) in &s.scenes {
            let title = i
                .layout
                .titles
                .get(id)
                .cloned()
                .unwrap_or_else(|| id.clone());
            rows.push(InspectorRow {
                kind: "scene".into(),
                id: id.clone(),
                label: title.clone(),
                detail: c.summary.clone(),
                status: Some(Status::Inferred),
                created_by: Some(c.created_by),
                updated: c.indexed_at.clone(),
                source: Some(Source {
                    doc: id.clone(),
                    para: None,
                    block: None,
                    quote: c.summary.chars().take(100).collect(),
                }),
                entity_id: None,
                entity_name: None,
                item: None,
            });
            for (k, v) in &c.metrics {
                rows.push(InspectorRow {
                    kind: "metric".into(),
                    id: format!("{id}:{k}"),
                    label: format!("{title} · {k}"),
                    detail: format!("{v:.2}"),
                    status: None,
                    created_by: Some(c.created_by),
                    updated: c.indexed_at.clone(),
                    source: Some(Source {
                        doc: id.clone(),
                        para: None,
                        block: None,
                        quote: String::new(),
                    }),
                    entity_id: None,
                    entity_name: None,
                    item: None,
                });
            }
        }
        rows.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
        });
        Ok(rows)
    }

    // --- Embedding server --------------------------------------------------------

    pub(crate) fn embedder(&self) -> Option<Arc<search::LlamaEmbedder>> {
        let cfg = self.config();
        if !cfg.embedding_enabled || !self.embed_ready.load(Ordering::Relaxed) {
            return None;
        }
        search::LlamaEmbedder::new(
            &format!("http://127.0.0.1:{}", cfg.embedding_port),
            &cfg.embedding_model,
        )
        .ok()
        .map(Arc::new)
    }

    pub async fn embedding_start(&self) -> Result<()> {
        let cfg = self.config();
        if cfg.embedding_model.trim().is_empty() {
            return Err(MarginError::Invalid(
                "choose an embedding model file in Settings → Memory first".into(),
            ));
        }
        self.embed_supervisor.start(embedding_launch(&cfg)).await
    }

    pub async fn embedding_stop(&self) {
        self.embed_supervisor.stop().await;
        self.embed_ready.store(false, Ordering::Relaxed);
        self.publish_progress();
    }
}

/// `llama-server --embedding` on CPU (SPEC §9.5), so it never competes for VRAM.
pub fn embedding_launch(cfg: &MemoryConfig) -> Launch {
    Launch {
        binary: cfg.embedding_binary.clone(),
        args: vec![
            "-m".into(),
            cfg.embedding_model.clone(),
            "--embedding".into(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            cfg.embedding_port.to_string(),
            "-c".into(),
            "2048".into(),
            "-ngl".into(),
            "0".into(),
            "--parallel".into(),
            "1".into(),
        ],
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod live_tests;
