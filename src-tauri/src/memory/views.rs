//! Read models for the frontend: what the Story Data Service returns for
//! memory queries (SPEC §5.1) and what the Memory Inspector shows (§9.6).

use serde::{Deserialize, Serialize};
use specta::Type;

use super::types::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EntitySummary {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub status: Status,
    pub created_by: Author,
    pub aliases: Vec<String>,
    pub fact_count: u32,
    pub inferred_fact_count: u32,
    pub relation_count: u32,
    pub mention_count: u32,
    pub updated: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RelationView {
    pub id: String,
    pub from: String,
    pub from_name: String,
    pub to: String,
    pub to_name: String,
    pub kind: String,
    pub directed: bool,
    pub status: Status,
    pub source: Option<Source>,
    pub since: Option<String>,
    pub created_by: Author,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MentionRow {
    pub scene_id: String,
    pub scene_title: String,
    pub count: u32,
    /// First paragraph that mentions the entry in this scene.
    pub first: Option<Source>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EntityDetail {
    pub entity: Entity,
    /// Relations that point at this entry, from other entries.
    pub relations_in: Vec<RelationView>,
    pub relations_out: Vec<RelationView>,
    pub mentions: Vec<MentionRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub mentions: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RelationGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<RelationView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MatrixCell {
    pub entity_id: String,
    pub scene_id: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SceneRef {
    pub id: String,
    pub title: String,
    /// Position in manuscript order.
    pub order: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MentionMatrix {
    pub entities: Vec<EntitySummary>,
    pub scenes: Vec<SceneRef>,
    pub cells: Vec<MatrixCell>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SceneCardView {
    pub card: SceneCard,
    pub title: String,
    pub order: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EventView {
    pub event: TimelineEvent,
    pub entity_names: Vec<String>,
    pub scene_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThreadView {
    pub id: String,
    pub title: String,
    pub status: ThreadStatus,
    pub status_canon: bool,
    pub introduced_in: Option<SceneRef>,
    pub touches: Vec<SceneRef>,
    pub resolved_in: Option<SceneRef>,
    pub entry_status: Status,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MetricPoint {
    pub scene_id: String,
    pub title: String,
    pub order: u32,
    pub value: f64,
}

/// One line in the Memory Inspector: a record of any kind, with where it came
/// from. Bulk actions address rows by `item`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InspectorRow {
    /// `entity`, `fact`, `relation`, `event`, `thread`, `scene`, `mention`, `metric`.
    pub kind: String,
    pub id: String,
    /// What it is called: the entry's name, the fact text, "Mara → Jon: sibling".
    pub label: String,
    pub detail: String,
    pub status: Option<Status>,
    pub created_by: Option<Author>,
    pub updated: String,
    pub source: Option<Source>,
    /// The entry this belongs to (for facts, relations, mentions).
    pub entity_id: Option<String>,
    pub entity_name: Option<String>,
    /// Set for kinds that bulk actions can address.
    pub item: Option<super::store::ItemRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PassageHit {
    pub doc: String,
    pub doc_title: String,
    pub para: String,
    pub block: u32,
    pub text: String,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingStatus {
    pub enabled: bool,
    pub ready: bool,
    pub model: String,
    pub vectors: u32,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum IndexState {
    /// Memory is turned off, or no project is open.
    Off,
    UpToDate,
    /// Scenes are waiting (for the author to be idle, or for the model).
    Pending,
    Working,
    /// Held back while the author types.
    Paused,
    /// The model is not running.
    NoModel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStatus {
    pub state: IndexState,
    pub pending: u32,
    pub total: u32,
    pub current: Option<String>,
    pub last_error: Option<String>,
    pub embeddings: EmbeddingStatus,
    pub entries: u32,
}

/// Memory settings kept per machine (paths are machine-specific).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MemoryConfig {
    pub schema_version: u32,
    /// Read scenes in the background (SPEC §9.4).
    pub indexing: bool,
    /// Seconds a scene must stay unchanged before it is read.
    pub settle_seconds: u32,
    /// Flag inferred facts that disagree with canon (SPEC §9.1). Off by default.
    pub flag_conflicts: bool,
    pub embedding_enabled: bool,
    pub embedding_binary: String,
    pub embedding_model: String,
    pub embedding_port: u32,
    /// Start the embedding server when a project opens.
    pub embedding_auto_start: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        MemoryConfig {
            schema_version: 1,
            indexing: true,
            settle_seconds: 60,
            flag_conflicts: false,
            embedding_enabled: false,
            embedding_binary: crate::ai::presets::BONSAI_BIN.into(),
            embedding_model: String::new(),
            embedding_port: 8082,
            embedding_auto_start: false,
        }
    }
}

// --- Margin panel ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PassageRef {
    pub doc: String,
    pub doc_title: String,
    pub para: Option<String>,
    pub block: Option<u32>,
    pub quote: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Idea {
    pub id: String,
    /// `next`, `stakes`, `reaction`, `sensory`, `thread`, `question`, `point`,
    /// `counter`, `evidence`, `term`, `transition`.
    #[serde(rename = "type")]
    pub kind: String,
    pub text: String,
    pub refs: Vec<PassageRef>,
    /// The context-log entry of the request that made it ("Why?").
    pub context_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IdeasResult {
    pub ideas: Vec<Idea>,
    pub context_id: String,
    /// Set when the model was not asked or could not answer.
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PinnedIdea {
    pub idea: Idea,
    pub doc: String,
    pub pinned_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Citation {
    /// The number the answer uses: [1], [2], …
    pub label: u32,
    /// `passage`, `fact`, `relation`, `scene`.
    pub kind: String,
    pub title: String,
    pub text: String,
    pub source: Option<PassageRef>,
    pub entity_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AskAnswer {
    pub answer: String,
    /// False when the book holds nothing that answers the question.
    pub found: bool,
    pub citations: Vec<Citation>,
    pub context_id: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PhrasingResult {
    pub alternatives: Vec<String>,
    pub context_id: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Json,
    Csv,
    Markdown,
    Graphml,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TypeInfo {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedIdea {
    pub text: String,
    pub context_id: String,
}
