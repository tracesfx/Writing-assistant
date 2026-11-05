//! Tauri IPC commands. Thin wrappers: every call runs on a blocking worker
//! thread so IO never touches the main thread. Types are exported to
//! TypeScript by tauri-specta (`src/sdk/bindings.ts`).

use std::sync::Arc;

use tauri::State;

use crate::app_state::{AppSession, AppState};
use crate::dialect::pm::PmNode;
use crate::error::{MarginError, Result};
use crate::project::document::DocKind;
use crate::story::StoryService;
use crate::story::types::*;

pub type Story = Arc<StoryService>;
pub type App = Arc<AppState>;

pub(crate) async fn blocking<S, T>(
    svc: &Arc<S>,
    f: impl FnOnce(&S) -> Result<T> + Send + 'static,
) -> Result<T>
where
    S: Send + Sync + 'static,
    T: Send + 'static,
{
    let svc = svc.clone();
    tauri::async_runtime::spawn_blocking(move || f(&svc))
        .await
        .map_err(|e| MarginError::Internal(e.to_string()))?
}

// --- Project -----------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn project_create(
    story: State<'_, Story>,
    app: State<'_, App>,
    req: NewProject,
) -> Result<ProjectInfo> {
    let info = blocking(story.inner(), move |s| s.create_project(&req)).await?;
    app.note_opened(&info.root, &info.title)?;
    Ok(info)
}

#[tauri::command]
#[specta::specta]
pub async fn project_open(
    story: State<'_, Story>,
    app: State<'_, App>,
    root: String,
) -> Result<ProjectInfo> {
    let info = blocking(story.inner(), move |s| {
        s.open_project(std::path::Path::new(&root))
    })
    .await?;
    app.note_opened(&info.root, &info.title)?;
    Ok(info)
}

#[tauri::command]
#[specta::specta]
pub async fn project_close(story: State<'_, Story>) -> Result<()> {
    blocking(story.inner(), |s| s.close_project()).await
}

#[tauri::command]
#[specta::specta]
pub async fn project_get(story: State<'_, Story>) -> Result<ProjectInfo> {
    blocking(story.inner(), |s| s.project_info()).await
}

// --- Binder ------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn binder_tree(story: State<'_, Story>) -> Result<BinderTree> {
    blocking(story.inner(), |s| s.binder_tree()).await
}

#[tauri::command]
#[specta::specta]
pub async fn binder_move(
    story: State<'_, Story>,
    ids: Vec<String>,
    target: BinderTarget,
) -> Result<()> {
    blocking(story.inner(), move |s| s.binder_move(&ids, &target)).await
}

#[tauri::command]
#[specta::specta]
pub async fn binder_trash(story: State<'_, Story>, ids: Vec<String>) -> Result<()> {
    blocking(story.inner(), move |s| s.binder_trash(&ids)).await
}

#[tauri::command]
#[specta::specta]
pub async fn binder_restore(story: State<'_, Story>, ids: Vec<String>) -> Result<()> {
    blocking(story.inner(), move |s| s.binder_restore(&ids)).await
}

// --- Documents -----------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn document_get(story: State<'_, Story>, id: String) -> Result<DocumentView> {
    blocking(story.inner(), move |s| s.document_get(&id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn document_meta(story: State<'_, Story>, id: String) -> Result<DocMeta> {
    blocking(story.inner(), move |s| s.document_meta(&id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn document_save(
    story: State<'_, Story>,
    id: String,
    content: PmNode,
    force: bool,
) -> Result<SaveResult> {
    blocking(story.inner(), move |s| {
        s.document_save(&id, &content, force)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn document_create(
    story: State<'_, Story>,
    kind: DocKind,
    title: String,
    target: BinderTarget,
) -> Result<DocMeta> {
    blocking(story.inner(), move |s| {
        s.document_create(kind, &title, &target)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn document_update_meta(
    story: State<'_, Story>,
    id: String,
    patch: DocMetaPatch,
) -> Result<DocMeta> {
    blocking(story.inner(), move |s| s.document_update_meta(&id, &patch)).await
}

// --- Recovery journal ----------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn journal_write(story: State<'_, Story>, id: String, content: PmNode) -> Result<()> {
    blocking(story.inner(), move |s| s.journal_write(&id, &content)).await
}

#[tauri::command]
#[specta::specta]
pub async fn journal_list(story: State<'_, Story>) -> Result<Vec<RecoveryEntry>> {
    blocking(story.inner(), |s| s.journal_list()).await
}

#[tauri::command]
#[specta::specta]
pub async fn journal_get(story: State<'_, Story>, id: String) -> Result<PmNode> {
    blocking(story.inner(), move |s| s.journal_get(&id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn journal_discard(story: State<'_, Story>, id: String) -> Result<()> {
    blocking(story.inner(), move |s| s.journal_discard(&id)).await
}

// --- App session -----------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn app_session(app: State<'_, App>) -> Result<AppSession> {
    blocking(app.inner(), |a| a.get()).await
}

#[tauri::command]
#[specta::specta]
pub async fn app_save_ui(app: State<'_, App>, project_id: String, ui: String) -> Result<()> {
    blocking(app.inner(), move |a| a.save_ui(&project_id, ui)).await
}

#[tauri::command]
#[specta::specta]
pub async fn app_forget_recent(app: State<'_, App>, root: String) -> Result<()> {
    blocking(app.inner(), move |a| a.forget_recent(&root)).await
}
