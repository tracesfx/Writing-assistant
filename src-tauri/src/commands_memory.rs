//! IPC commands for Story Memory (SPEC §5.1, §8, §9). The canon rules live in
//! `memory::store`; these only translate. None of them touch manuscript text.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::error::{MarginError, Result};
use crate::memory::MemoryService;
use crate::memory::margin::{IdeasRequest, PhrasingRequest};
use crate::memory::store::{EntityPatch, ItemRef};
use crate::memory::types::*;
use crate::memory::views::*;

pub type Mem = Arc<MemoryService>;

/// Run blocking work (file IO, scans) off the async runtime.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| MarginError::Internal(e.to_string()))?
}

macro_rules! cmd {
    ($name:ident($($arg:ident : $ty:ty),*) -> $ret:ty = |$mem:ident| $body:expr) => {
        #[tauri::command]
        #[specta::specta]
        pub async fn $name(mem: State<'_, Mem>, $($arg: $ty),*) -> Result<$ret> {
            let $mem = mem.inner().clone();
            $body
        }
    };
}

cmd!(memory_status() -> MemoryStatus = |m| Ok(m.status()));
cmd!(memory_config_get() -> MemoryConfig = |m| Ok(m.config()));
cmd!(memory_config_set(config: MemoryConfig) -> MemoryConfig = |m| m.set_config(config));
cmd!(memory_focus(doc_id: Option<String>) -> () = |m| {
    m.focus(doc_id);
    Ok(())
});
cmd!(memory_reindex_scene(doc_id: String) -> () = |m| m.reindex_scene(&doc_id).await);
cmd!(memory_rebuild_all() -> u32 = |m| blocking(move || m.rebuild_all()).await);
cmd!(memory_forget_document(doc_id: String, forget: bool) -> () =
    |m| blocking(move || m.forget_document(&doc_id, forget)).await);
cmd!(memory_excluded() -> Vec<String> = |m| m.excluded_documents());
cmd!(embedding_start() -> () = |m| m.embedding_start().await);
cmd!(embedding_stop() -> () = |m| {
    m.embedding_stop().await;
    Ok(())
});

cmd!(entities_list(kind: Option<String>, query: Option<String>) -> Vec<EntitySummary> =
    |m| blocking(move || m.entities_list(kind.as_deref(), query.as_deref())).await);
cmd!(entity_get(id: String) -> EntityDetail = |m| blocking(move || m.entity_get(&id)).await);
cmd!(entity_create(kind: String, name: String, notes: String) -> Entity =
    |m| blocking(move || m.entity_create(&kind, &name, &notes)).await);
cmd!(entity_update(id: String, patch: EntityPatch) -> Entity =
    |m| blocking(move || m.entity_update(&id, &patch)).await);
cmd!(entity_delete(id: String) -> Entity = |m| blocking(move || m.entity_delete(&id)).await);
cmd!(entity_restore(entity: Entity) -> Entity = |m| blocking(move || m.entity_restore(entity)).await);
cmd!(fact_add(entity: String, text: String, field: Option<String>, source: Option<Source>) -> Fact =
    |m| blocking(move || m.fact_add(&entity, &text, field.as_deref(), source)).await);
cmd!(fact_update(entity: String, fact: String, text: String, field: Option<String>) -> Fact =
    |m| blocking(move || m.fact_update(&entity, &fact, &text, field.as_deref())).await);
cmd!(fact_delete(entity: String, fact: String) -> () =
    |m| blocking(move || m.fact_delete(&entity, &fact)).await);
cmd!(relation_add(from: String, to: String, kind: String, since: Option<String>) -> Relation =
    |m| blocking(move || m.relation_add(&from, &to, &kind, since.as_deref())).await);
cmd!(relation_delete(entity: String, relation: String) -> () =
    |m| blocking(move || m.relation_delete(&entity, &relation)).await);
cmd!(memory_make_canon(items: Vec<ItemRef>) -> u32 = |m| blocking(move || m.make_canon(&items)).await);
cmd!(memory_delete_inferred(items: Vec<ItemRef>) -> u32 =
    |m| blocking(move || m.delete_inferred(&items)).await);

cmd!(relations_list(entity: Option<String>, kind: Option<String>) -> Vec<RelationView> =
    |m| blocking(move || m.relations_list(entity.as_deref(), kind.as_deref())).await);
cmd!(relations_graph() -> RelationGraph = |m| blocking(move || m.relations_graph()).await);
cmd!(mentions_by_entity(id: String) -> Vec<MentionRow> =
    |m| blocking(move || m.mentions_by_entity(&id)).await);
cmd!(mentions_matrix(types: Vec<String>) -> MentionMatrix =
    |m| blocking(move || m.mentions_matrix(&types)).await);
cmd!(timeline_events() -> Vec<EventView> = |m| blocking(move || m.timeline_events()).await);
cmd!(threads_list(status: Option<ThreadStatus>) -> Vec<ThreadView> =
    |m| blocking(move || m.threads_list(status)).await);
cmd!(scene_cards() -> Vec<SceneCardView> = |m| blocking(move || m.scene_cards()).await);
cmd!(metrics_series(key: String) -> Vec<MetricPoint> =
    |m| blocking(move || m.metrics_series(&key)).await);
cmd!(bible_types() -> Vec<TypeInfo> = |m| Ok(m
    .bible_types()
    .into_iter()
    .map(|(id, title)| TypeInfo { id, title })
    .collect()));
cmd!(inspector_rows() -> Vec<InspectorRow> = |m| blocking(move || m.inspector_rows()).await);
cmd!(search_text(query: String, k: u32) -> Vec<PassageHit> =
    |m| blocking(move || m.search_text(&query, k as usize)).await);
cmd!(search_semantic(query: String, k: u32) -> Vec<PassageHit> =
    |m| m.search_semantic(&query, k as usize).await);
cmd!(resolve_source(source: Source) -> Option<PassageHit> =
    |m| blocking(move || m.resolve_source(&source)).await);

cmd!(ideas_generate(request: IdeasRequest) -> IdeasResult = |m| m.ideas(request).await);
cmd!(idea_expand(doc_id: String, idea: String, text_before: String) -> ExpandedIdea = |m| {
    let (text, context_id) = m.idea_expand(&doc_id, &idea, &text_before).await?;
    Ok(ExpandedIdea { text, context_id })
});
cmd!(ideas_pinned() -> Vec<PinnedIdea> = |m| blocking(move || m.ideas_pinned()).await);
cmd!(idea_pin(idea: Idea, doc_id: String) -> Vec<PinnedIdea> =
    |m| blocking(move || m.idea_pin(idea, &doc_id)).await);
cmd!(idea_unpin(id: String) -> Vec<PinnedIdea> = |m| blocking(move || m.idea_unpin(&id)).await);
cmd!(idea_to_note(doc_id: String, text: String) -> String =
    |m| blocking(move || m.idea_to_note(&doc_id, &text)).await);
cmd!(ask_book(question: String) -> AskAnswer = |m| m.ask(&question).await);
cmd!(phrasing_generate(request: PhrasingRequest) -> PhrasingResult = |m| m.phrasing(request).await);

cmd!(memory_export(dir: String, format: ExportFormat) -> Vec<String> =
    |m| blocking(move || m.export(&PathBuf::from(dir), format)).await);
cmd!(memory_import(path: String) -> u32 =
    |m| blocking(move || m.import_json(&PathBuf::from(path))).await);
