//! IPC commands for the services added after the first slice: binder and
//! document operations, comments, scratchpad, find/replace, history,
//! backups, stats and spelling. Same rules as `commands.rs`: every call runs
//! on a blocking worker thread.

use std::sync::Arc;

use tauri::State;

use crate::app_state::BackupConfig;
use crate::backup::{BackupInfo, BackupStatus, Backups};
use crate::commands::{App, Story, blocking};
use crate::dialect::pm::PmNode;
use crate::error::Result;
use crate::history::{DocAtSnapshot, DocDiff, DocVersion, History, SnapshotInfo, SnapshotKind};
use crate::spell::{Spell, SpellRange};
use crate::stats::{SessionRecord, Stats, StatsSummary};
use crate::story::types::*;

pub type Hist = Arc<History>;
pub type Back = Arc<Backups>;
pub type StatsSvc = Arc<Stats>;
pub type SpellSvc = Arc<Spell>;

#[tauri::command]
#[specta::specta]
pub async fn app_save_settings(app: State<'_, App>, settings: String) -> Result<()> {
    blocking(app.inner(), move |a| a.save_settings(settings)).await
}

// --- Binder and document operations ----------------------------------------------

/// The document's current text on disk, without making it the editor's
/// known revision (for comparing with unsaved edits).
#[tauri::command]
#[specta::specta]
pub async fn document_peek(story: State<'_, Story>, id: String) -> Result<PmNode> {
    blocking(story.inner(), move |s| {
        Ok(crate::dialect::pm::to_pm(&s.document_ast(&id)?))
    })
    .await
}

/// Replace a document's text outside the editor (undo of merge or split).
/// Open editors reload it.
#[tauri::command]
#[specta::specta]
pub async fn document_replace(story: State<'_, Story>, id: String, content: PmNode) -> Result<()> {
    blocking(story.inner(), move |s| {
        s.replace_bodies(&[(id, crate::dialect::pm::from_pm(&content))], "undo")
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn binder_duplicate(story: State<'_, Story>, ids: Vec<String>) -> Result<Vec<String>> {
    blocking(story.inner(), move |s| s.binder_duplicate(&ids)).await
}

#[tauri::command]
#[specta::specta]
pub async fn document_split(
    story: State<'_, Story>,
    id: String,
    before: PmNode,
    after: PmNode,
    title: String,
) -> Result<DocMeta> {
    blocking(story.inner(), move |s| {
        s.document_split(&id, &before, &after, &title)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn document_merge(story: State<'_, Story>, ids: Vec<String>) -> Result<MergeResult> {
    blocking(story.inner(), move |s| s.document_merge(&ids)).await
}

#[tauri::command]
#[specta::specta]
pub async fn project_update(story: State<'_, Story>, patch: ProjectPatch) -> Result<ProjectInfo> {
    blocking(story.inner(), move |s| s.project_update(&patch)).await
}

#[tauri::command]
#[specta::specta]
pub async fn capture(story: State<'_, Story>, text: String) -> Result<String> {
    blocking(story.inner(), move |s| s.capture(&text)).await
}

#[tauri::command]
#[specta::specta]
pub async fn comments_list(story: State<'_, Story>, doc_id: String) -> Result<Vec<Comment>> {
    blocking(story.inner(), move |s| s.comments_list(&doc_id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn comments_put(
    story: State<'_, Story>,
    doc_id: String,
    comment: Comment,
) -> Result<Vec<Comment>> {
    blocking(story.inner(), move |s| s.comments_put(&doc_id, &comment)).await
}

#[tauri::command]
#[specta::specta]
pub async fn comments_delete(
    story: State<'_, Story>,
    doc_id: String,
    comment_id: String,
) -> Result<Vec<Comment>> {
    blocking(story.inner(), move |s| {
        s.comments_delete(&doc_id, &comment_id)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn scratchpad_get(story: State<'_, Story>) -> Result<String> {
    blocking(story.inner(), |s| s.scratchpad_get()).await
}

#[tauri::command]
#[specta::specta]
pub async fn scratchpad_put(story: State<'_, Story>, text: String) -> Result<()> {
    blocking(story.inner(), move |s| s.scratchpad_put(&text)).await
}

// --- Find and replace ------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn search(story: State<'_, Story>, query: SearchQuery) -> Result<SearchResult> {
    blocking(story.inner(), move |s| s.search(&query)).await
}

/// Replace the chosen hits. A history snapshot is taken first; restoring it
/// (`history_restore` with the returned id) undoes the whole replace-all.
#[tauri::command]
#[specta::specta]
pub async fn search_replace(
    history: State<'_, Hist>,
    query: SearchQuery,
    replacement: String,
    hits: Vec<HitRef>,
) -> Result<ReplaceReport> {
    blocking(history.inner(), move |h| {
        let story = h.story();
        let (docs, replaced) = story.plan_replace(&query, &replacement, &hits)?;
        if docs.is_empty() {
            return Ok(ReplaceReport {
                replaced: 0,
                doc_ids: Vec::new(),
                snapshot_id: None,
            });
        }
        let label = format!(
            "Before replacing \u{201c}{}\u{201d} with \u{201c}{}\u{201d}",
            query.text, replacement
        );
        let snap = h.snapshot(Some(&label), SnapshotKind::Safety)?;
        story.replace_bodies(&docs, "replace")?;
        Ok(ReplaceReport {
            replaced,
            doc_ids: docs.into_iter().map(|(id, _)| id).collect(),
            snapshot_id: snap.map(|s| s.id),
        })
    })
    .await
}

// --- History ---------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn history_snapshot(
    history: State<'_, Hist>,
    name: String,
) -> Result<Option<SnapshotInfo>> {
    blocking(history.inner(), move |h| {
        h.snapshot(Some(&name), SnapshotKind::Named)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn history_list(history: State<'_, Hist>, limit: u32) -> Result<Vec<SnapshotInfo>> {
    blocking(history.inner(), move |h| h.list(limit as usize)).await
}

#[tauri::command]
#[specta::specta]
pub async fn history_doc_versions(
    history: State<'_, Hist>,
    doc_id: String,
) -> Result<Vec<DocVersion>> {
    blocking(history.inner(), move |h| h.doc_versions(&doc_id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn history_doc_at(
    history: State<'_, Hist>,
    doc_id: String,
    snapshot: String,
) -> Result<DocAtSnapshot> {
    blocking(history.inner(), move |h| h.doc_at(&doc_id, &snapshot)).await
}

/// `None` stands for the current file on disk.
#[tauri::command]
#[specta::specta]
pub async fn history_diff(
    history: State<'_, Hist>,
    doc_id: String,
    from: Option<String>,
    to: Option<String>,
) -> Result<DocDiff> {
    blocking(history.inner(), move |h| {
        h.diff(&doc_id, from.as_deref(), to.as_deref())
    })
    .await
}

/// Restore documents from a snapshot; returns the safety snapshot taken first.
#[tauri::command]
#[specta::specta]
pub async fn history_restore(
    history: State<'_, Hist>,
    snapshot: String,
    doc_ids: Vec<String>,
    label: String,
) -> Result<Option<SnapshotInfo>> {
    blocking(history.inner(), move |h| {
        h.restore_docs(&snapshot, &doc_ids, &label)
    })
    .await
}

// --- Backups ---------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn backup_status(backups: State<'_, Back>) -> Result<BackupStatus> {
    blocking(backups.inner(), |b| b.status()).await
}

#[tauri::command]
#[specta::specta]
pub async fn backup_set_config(
    backups: State<'_, Back>,
    config: BackupConfig,
) -> Result<BackupStatus> {
    blocking(backups.inner(), move |b| b.set_config(config)).await
}

#[tauri::command]
#[specta::specta]
pub async fn backup_now(backups: State<'_, Back>) -> Result<BackupInfo> {
    blocking(backups.inner(), |b| b.backup_now()).await
}

#[tauri::command]
#[specta::specta]
pub async fn backup_restore(backups: State<'_, Back>, path: String) -> Result<String> {
    blocking(backups.inner(), move |b| b.restore(&path)).await
}

/// Called before the window closes: the writing session is recorded and a
/// final backup is written if anything changed.
#[tauri::command]
#[specta::specta]
pub async fn app_before_close(backups: State<'_, Back>, stats: State<'_, StatsSvc>) -> Result<()> {
    blocking(stats.inner(), |s| s.flush()).await?;
    blocking(backups.inner(), |b| b.backup_if_dirty().map(|_| ())).await
}

// --- Stats -----------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn stats_summary(stats: State<'_, StatsSvc>) -> Result<StatsSummary> {
    blocking(stats.inner(), |s| s.summary()).await
}

#[tauri::command]
#[specta::specta]
pub async fn stats_activity(stats: State<'_, StatsSvc>, seconds: u32) -> Result<()> {
    blocking(stats.inner(), move |s| s.activity(seconds)).await
}

#[tauri::command]
#[specta::specta]
pub async fn stats_sessions(stats: State<'_, StatsSvc>) -> Result<Vec<SessionRecord>> {
    blocking(stats.inner(), |s| s.sessions()).await
}

// --- Spelling --------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn spell_check(
    spell: State<'_, SpellSvc>,
    texts: Vec<String>,
) -> Result<Vec<Vec<SpellRange>>> {
    blocking(spell.inner(), move |s| s.check(&texts)).await
}

#[tauri::command]
#[specta::specta]
pub async fn spell_suggest(spell: State<'_, SpellSvc>, word: String) -> Result<Vec<String>> {
    blocking(spell.inner(), move |s| s.suggest(&word)).await
}

#[tauri::command]
#[specta::specta]
pub async fn spell_add(spell: State<'_, SpellSvc>, word: String) -> Result<()> {
    blocking(spell.inner(), move |s| s.add_word(&word)).await
}
