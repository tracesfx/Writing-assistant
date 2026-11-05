//! App-level state outside any project: recent projects and the opaque UI
//! session blob used for session restore (SPEC §11.4). Stored as JSON in the
//! app config directory.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{MarginError, Result};
use crate::project::atomic::atomic_write;

const FILE: &str = "app-state.json";
const MAX_RECENT: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecentProject {
    pub root: String,
    pub title: String,
    pub opened_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSession {
    #[serde(default = "one")]
    pub schema_version: u32,
    #[serde(default)]
    pub recent_projects: Vec<RecentProject>,
    /// Frontend-owned JSON (open tabs, layout, cursor positions), keyed by
    /// project id. Opaque to the backend.
    #[serde(default)]
    pub ui: std::collections::BTreeMap<String, String>,
    /// Frontend-owned app settings JSON (theme, editor, keymap, modules).
    #[serde(default)]
    pub settings: Option<String>,
    /// Machine-specific per-project preferences, keyed by project id.
    #[serde(default)]
    pub projects: std::collections::BTreeMap<String, ProjectPrefs>,
}

/// Backup settings (SPEC §11.3). Stored per machine, since the folder is a
/// local path.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupConfig {
    pub enabled: bool,
    /// Folder that receives `<project>/<project> <date>.zip`. `None` = the default.
    pub dir: Option<String>,
    pub interval_minutes: u32,
}

impl Default for BackupConfig {
    fn default() -> Self {
        BackupConfig {
            enabled: true,
            dir: None,
            interval_minutes: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPrefs {
    #[serde(default)]
    pub backup: BackupConfig,
}

fn one() -> u32 {
    1
}

pub struct AppState {
    path: PathBuf,
    data: Mutex<AppSession>,
}

impl AppState {
    pub fn load(config_dir: PathBuf) -> Self {
        let path = config_dir.join(FILE);
        let data = fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<AppSession>(&t).ok())
            .unwrap_or_else(|| AppSession {
                schema_version: 1,
                ..AppSession::default()
            });
        AppState {
            path,
            data: Mutex::new(data),
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut AppSession) -> T) -> Result<T> {
        let mut g = self
            .data
            .lock()
            .map_err(|_| MarginError::Internal("app state lock poisoned".into()))?;
        let out = f(&mut g);
        atomic_write(&self.path, serde_json::to_string_pretty(&*g)?.as_bytes())?;
        Ok(out)
    }

    pub fn get(&self) -> Result<AppSession> {
        let g = self
            .data
            .lock()
            .map_err(|_| MarginError::Internal("app state lock poisoned".into()))?;
        Ok(g.clone())
    }

    pub fn note_opened(&self, root: &str, title: &str) -> Result<()> {
        self.with(|s| {
            s.recent_projects.retain(|r| r.root != root);
            s.recent_projects.insert(
                0,
                RecentProject {
                    root: root.to_string(),
                    title: title.to_string(),
                    opened_at: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                },
            );
            s.recent_projects.truncate(MAX_RECENT);
        })
    }

    pub fn forget_recent(&self, root: &str) -> Result<()> {
        self.with(|s| s.recent_projects.retain(|r| r.root != root))
    }

    pub fn save_ui(&self, project_id: &str, ui: String) -> Result<()> {
        self.with(|s| {
            s.ui.insert(project_id.to_string(), ui);
        })
    }

    pub fn save_settings(&self, settings: String) -> Result<()> {
        self.with(|s| s.settings = Some(settings))
    }

    pub fn backup_config(&self, project_id: &str) -> BackupConfig {
        self.get()
            .ok()
            .and_then(|s| s.projects.get(project_id).map(|p| p.backup.clone()))
            .unwrap_or_default()
    }

    pub fn set_backup_config(&self, project_id: &str, cfg: BackupConfig) -> Result<()> {
        self.with(|s| {
            s.projects.entry(project_id.to_string()).or_default().backup = cfg;
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_recent_projects() {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState::load(dir.path().to_path_buf());
        st.note_opened("C:/a", "A").unwrap();
        st.note_opened("C:/b", "B").unwrap();
        st.note_opened("C:/a", "A").unwrap();
        st.save_ui("p1", "{\"tabs\":[]}".into()).unwrap();
        let again = AppState::load(dir.path().to_path_buf()).get().unwrap();
        assert_eq!(
            again
                .recent_projects
                .iter()
                .map(|r| r.root.as_str())
                .collect::<Vec<_>>(),
            ["C:/a", "C:/b"]
        );
        assert_eq!(
            again.ui.get("p1").map(String::as_str),
            Some("{\"tabs\":[]}")
        );
    }
}
