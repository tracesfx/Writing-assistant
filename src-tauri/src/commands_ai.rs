//! IPC commands for the AI layer (SPEC §6): connection, status, the
//! context log and the bake-off. Same rules as `commands.rs`; none of these
//! touch manuscript text.

use std::sync::Arc;

use tauri::State;

use crate::ai::AiService;
use crate::ai::bakeoff::{BakeoffRun, BlindQuestion, Candidate, QualityScore};
use crate::ai::bakeoff_service::{BakeoffService, BakeoffState, RunSummary};
use crate::ai::context_log::{ContextLogEntry, ContextLogSummary};
use crate::ai::types::*;
use crate::error::Result;

pub type Ai = Arc<AiService>;
pub type Bake = Arc<BakeoffService>;

#[tauri::command]
#[specta::specta]
pub async fn ai_config_get(ai: State<'_, Ai>) -> Result<AiConfig> {
    Ok(ai.config())
}

#[tauri::command]
#[specta::specta]
pub async fn ai_config_set(ai: State<'_, Ai>, config: AiConfig) -> Result<AiConfig> {
    ai.set_config(config)
}

#[tauri::command]
#[specta::specta]
pub async fn ai_panel(ai: State<'_, Ai>) -> Result<ModelPanel> {
    Ok(ai.panel().await)
}

/// Start (managed) or connect (external) using the saved settings.
#[tauri::command]
#[specta::specta]
pub async fn ai_connect(ai: State<'_, Ai>) -> Result<()> {
    ai.inner().connect().await
}

#[tauri::command]
#[specta::specta]
pub async fn ai_disconnect(ai: State<'_, Ai>) -> Result<()> {
    ai.inner().disconnect().await;
    Ok(())
}

/// Every keystroke: cancels ghost text, pauses idle work (SPEC §2.2).
#[tauri::command]
#[specta::specta]
pub async fn ai_keystroke(ai: State<'_, Ai>) -> Result<()> {
    ai.note_keystroke();
    Ok(())
}

/// The Model panel's "Try it" box. Logged like any other request.
#[tauri::command]
#[specta::specta]
pub async fn ai_try(ai: State<'_, Ai>, text: String) -> Result<Completion> {
    Ok(ai.try_it(text).await)
}

#[tauri::command]
#[specta::specta]
pub async fn context_log_list(ai: State<'_, Ai>, limit: u32) -> Result<Vec<ContextLogSummary>> {
    let ai = ai.inner().clone();
    tauri::async_runtime::spawn_blocking(move || ai.context_log.list(limit as usize))
        .await
        .map_err(|e| crate::error::MarginError::Internal(e.to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn context_log_get(ai: State<'_, Ai>, id: String) -> Result<ContextLogEntry> {
    ai.context_log.get(&id)
}

#[tauri::command]
#[specta::specta]
pub async fn context_log_clear(ai: State<'_, Ai>) -> Result<u32> {
    ai.context_log.clear()
}

// --- Bake-off -----------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn bakeoff_candidates(b: State<'_, Bake>) -> Result<Vec<Candidate>> {
    Ok(b.candidates())
}

#[tauri::command]
#[specta::specta]
pub async fn bakeoff_start(
    b: State<'_, Bake>,
    preset_ids: Vec<String>,
    tune: bool,
) -> Result<String> {
    b.inner().start(preset_ids, tune)
}

#[tauri::command]
#[specta::specta]
pub async fn bakeoff_cancel(b: State<'_, Bake>) -> Result<()> {
    b.cancel();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn bakeoff_state(b: State<'_, Bake>) -> Result<BakeoffState> {
    Ok(b.state())
}

#[tauri::command]
#[specta::specta]
pub async fn bakeoff_runs(b: State<'_, Bake>) -> Result<Vec<RunSummary>> {
    Ok(b.runs())
}

#[tauri::command]
#[specta::specta]
pub async fn bakeoff_load(b: State<'_, Bake>, run_id: String) -> Result<BakeoffRun> {
    b.load(&run_id)
}

#[tauri::command]
#[specta::specta]
pub async fn bakeoff_questions(b: State<'_, Bake>, run_id: String) -> Result<Vec<BlindQuestion>> {
    b.blind_questions(&run_id)
}

#[tauri::command]
#[specta::specta]
pub async fn bakeoff_answer(
    b: State<'_, Bake>,
    run_id: String,
    index: u32,
    label: Option<String>,
) -> Result<()> {
    b.answer(&run_id, index, label)
}

#[tauri::command]
#[specta::specta]
pub async fn bakeoff_scores(b: State<'_, Bake>, run_id: String) -> Result<Vec<QualityScore>> {
    b.scores(&run_id)
}

#[tauri::command]
#[specta::specta]
pub async fn bakeoff_use_model(
    b: State<'_, Bake>,
    run_id: String,
    preset_id: String,
) -> Result<AiConfig> {
    b.use_model(&run_id, &preset_id)
}
