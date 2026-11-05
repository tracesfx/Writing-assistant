//! Types exposed through the Story Data Service (and so through the SDK).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::dialect::pm::PmNode;
use crate::project::document::{AiToggle, DocKind};
use crate::project::manifest::{Goals, LabelDef, Locale, ProjectType, Section, TrashOrigin};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub id: String,
    pub root: String,
    pub title: String,
    pub author: String,
    pub project_type: ProjectType,
    pub locale: Locale,
    pub statuses: Vec<String>,
    pub labels: Vec<LabelDef>,
    pub goals: Goals,
    /// Files that could not be read when the project was opened.
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocMeta {
    pub id: String,
    pub kind: DocKind,
    pub title: String,
    pub status: Option<String>,
    pub pov: Option<String>,
    pub labels: Vec<String>,
    pub target: Option<u32>,
    pub synopsis: Option<String>,
    pub include: bool,
    pub ai: AiToggle,
    /// `None` while the document is in the Trash.
    pub section: Option<Section>,
    pub word_count: u32,
    /// Path relative to the project root, `/`-separated.
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentView {
    pub meta: DocMeta,
    pub content: PmNode,
    /// Content hash of the file on disk.
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub revision: String,
    pub word_count: u32,
}

/// Partial update of document properties. `None` leaves a field unchanged.
/// For optional text fields an empty string clears the value; for `target`, 0 clears it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocMetaPatch {
    pub title: Option<String>,
    pub kind: Option<DocKind>,
    pub status: Option<String>,
    pub pov: Option<String>,
    pub labels: Option<Vec<String>>,
    pub target: Option<u32>,
    pub synopsis: Option<String>,
    pub include: Option<bool>,
    pub ai: Option<AiToggle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BinderNode {
    pub id: String,
    pub kind: DocKind,
    pub title: String,
    pub status: Option<String>,
    pub labels: Vec<String>,
    pub target: Option<u32>,
    pub include: bool,
    pub ai: AiToggle,
    pub has_synopsis: bool,
    pub word_count: u32,
    /// Words in this node and all descendants.
    pub total_word_count: u32,
    pub children: Vec<BinderNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BinderSectionView {
    pub section: Section,
    pub nodes: Vec<BinderNode>,
    pub total_word_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrashNode {
    pub node: BinderNode,
    pub from: TrashOrigin,
    pub trashed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BinderTree {
    pub sections: Vec<BinderSectionView>,
    pub trash: Vec<TrashNode>,
}

/// Where to put a node: a section, an optional parent node, and an index
/// among the parent's children (`None` = append).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BinderTarget {
    pub section: Section,
    pub parent: Option<String>,
    pub index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewProject {
    /// Folder in which the project folder is created.
    pub parent_dir: String,
    pub title: String,
    pub author: String,
    pub project_type: ProjectType,
    pub locale: Locale,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryEntry {
    pub doc_id: String,
    pub title: String,
    pub saved_at: String,
    pub word_count: u32,
}

/// Partial update of project settings. `None` leaves a field unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPatch {
    pub title: Option<String>,
    pub author: Option<String>,
    pub project_type: Option<ProjectType>,
    pub locale: Option<Locale>,
    pub goals: Option<Goals>,
    pub statuses: Option<Vec<String>>,
    pub labels: Option<Vec<LabelDef>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MergeResult {
    /// The document that now holds the merged text.
    pub target: String,
    /// The other documents, now in the Trash.
    pub trashed: Vec<String>,
}

/// An inline comment. The anchor is a `comment` mark in the text; the body
/// lives in `comments/<docId>.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    pub body: String,
    #[serde(default)]
    pub resolved: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// Which documents a search covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SearchScopeKind {
    /// One document (`scopeId`).
    Document,
    /// A container and everything inside it (`scopeId`).
    Chapter,
    Manuscript,
    /// Notes and Research.
    Notes,
    /// Every section (not the Trash).
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub text: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub scope: SearchScopeKind,
    pub scope_id: Option<String>,
}

/// One match. `block` is the index of the text block (paragraph or heading,
/// in document order); `start`/`end` are UTF-16 offsets in its plain text,
/// where a footnote counts as one character, matching the editor.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub doc_id: String,
    pub title: String,
    pub block: u32,
    pub start: u32,
    pub end: u32,
    pub before: String,
    pub matched: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    /// More matches exist than were returned.
    pub truncated: bool,
}

/// A hit chosen for replacement.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HitRef {
    pub doc_id: String,
    pub block: u32,
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceReport {
    pub replaced: u32,
    pub doc_ids: Vec<String>,
    /// History snapshot taken just before; restoring it undoes the replace.
    pub snapshot_id: Option<String>,
}
