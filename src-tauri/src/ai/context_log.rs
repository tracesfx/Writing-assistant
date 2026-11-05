//! The context log (SPEC §9.6): for every AI request, the exact prompt (split
//! into labelled sections), token counts, raw output and latency. A ring
//! buffer of the last N requests (default 500), one JSON file per request in
//! `<project>/.margin/context-log/`, newest last. Clearable.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::scheduler::FinishedJob;
use super::types::*;
use crate::error::{MarginError, Result};
use crate::project::atomic::atomic_write;

pub const LOG_DIR: &str = "context-log";
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContextLogEntry {
    pub schema_version: u32,
    /// A ULID: sorts by time.
    pub id: String,
    pub timestamp: String,
    pub role: Role,
    pub priority: Priority,
    pub slot: u32,
    pub model: String,
    /// The labelled sections [1]–[6] (SPEC §7.4).
    pub sections: Vec<PromptSection>,
    /// The exact string sent to the server (after templating and thinking prefill).
    pub raw_prompt: String,
    pub raw_output: String,
    pub outcome: Outcome,
    pub error: Option<String>,
    pub timings: Timings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContextLogSummary {
    pub id: String,
    pub timestamp: String,
    pub role: Role,
    pub model: String,
    pub outcome: Outcome,
    pub prompt_tokens: u32,
    pub output_tokens: u32,
    pub total_ms: f64,
    /// First 120 characters of the output.
    pub preview: String,
}

pub struct ContextLog {
    dir: Mutex<Option<PathBuf>>,
    limit: Mutex<usize>,
}

impl Default for ContextLog {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextLog {
    pub fn new() -> Self {
        ContextLog {
            dir: Mutex::new(None),
            limit: Mutex::new(500),
        }
    }

    /// Point at a project's `.margin/` folder (or `None` when none is open).
    pub fn set_project(&self, margin_dir: Option<PathBuf>) {
        if let Ok(mut d) = self.dir.lock() {
            *d = margin_dir.map(|p| p.join(LOG_DIR));
        }
    }

    pub fn set_limit(&self, limit: usize) {
        if let Ok(mut l) = self.limit.lock() {
            *l = limit.max(10);
        }
    }

    fn dir(&self) -> Option<PathBuf> {
        self.dir.lock().ok().and_then(|d| d.clone())
    }

    /// Record a finished request. A no-op without an open project.
    pub fn record(&self, job: &FinishedJob) -> Result<()> {
        let Some(dir) = self.dir() else {
            return Ok(());
        };
        let c = &job.completion;
        let entry = ContextLogEntry {
            schema_version: SCHEMA_VERSION,
            id: ulid::Ulid::generate().to_string(),
            timestamp: chrono::Local::now().to_rfc3339(),
            role: job.spec.role,
            priority: job.spec.priority,
            slot: job.slot,
            model: job.model.clone(),
            sections: job.spec.prompt.sections.clone(),
            raw_prompt: job.raw_prompt.clone(),
            raw_output: c.text.clone(),
            outcome: c.outcome.clone(),
            error: c.error.clone(),
            timings: c.timings.clone(),
        };
        fs::create_dir_all(&dir)?;
        let json = serde_json::to_vec_pretty(&entry)?;
        atomic_write(&dir.join(format!("{}.json", entry.id)), &json)?;
        self.prune(&dir)
    }

    fn ids(dir: &PathBuf) -> Vec<String> {
        let mut ids: Vec<String> = fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                n.strip_suffix(".json").map(str::to_string)
            })
            .collect();
        ids.sort();
        ids
    }

    fn prune(&self, dir: &PathBuf) -> Result<()> {
        let limit = self.limit.lock().map(|l| *l).unwrap_or(500);
        let ids = Self::ids(dir);
        if ids.len() > limit {
            for id in &ids[..ids.len() - limit] {
                let _ = fs::remove_file(dir.join(format!("{id}.json")));
            }
        }
        Ok(())
    }

    /// Newest first.
    pub fn list(&self, limit: usize) -> Vec<ContextLogSummary> {
        let Some(dir) = self.dir() else {
            return vec![];
        };
        Self::ids(&dir)
            .into_iter()
            .rev()
            .take(limit)
            .filter_map(|id| self.get(&id).ok())
            .map(|e| ContextLogSummary {
                preview: e.raw_output.chars().take(120).collect(),
                id: e.id,
                timestamp: e.timestamp,
                role: e.role,
                model: e.model,
                outcome: e.outcome,
                prompt_tokens: e.timings.prompt_tokens,
                output_tokens: e.timings.output_tokens,
                total_ms: e.timings.total_ms,
            })
            .collect()
    }

    pub fn get(&self, id: &str) -> Result<ContextLogEntry> {
        // Ids are ULIDs; refuse anything that could leave the folder.
        if !id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(MarginError::Invalid("bad log id".into()));
        }
        let dir = self.dir().ok_or(MarginError::NoProject)?;
        let bytes = fs::read(dir.join(format!("{id}.json")))
            .map_err(|_| MarginError::NotFound(format!("log entry {id}")))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn clear(&self) -> Result<u32> {
        let Some(dir) = self.dir() else {
            return Ok(0);
        };
        let ids = Self::ids(&dir);
        for id in &ids {
            let _ = fs::remove_file(dir.join(format!("{id}.json")));
        }
        Ok(ids.len() as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::scheduler::JobSpec;

    fn job(n: u32) -> FinishedJob {
        FinishedJob {
            id: format!("job-{n}"),
            spec: JobSpec {
                priority: Priority::P0,
                role: Role::Ghost,
                prompt: Prompt {
                    system: "s".into(),
                    user: "u".into(),
                    assistant_prefix: "p".into(),
                    sections: vec![PromptSection {
                        label: "[1] Role".into(),
                        text: "You are quiet.".into(),
                        tokens: 4,
                    }],
                },
                sampling: default_sampling(Role::Ghost),
                resumable: false,
                log: true,
            },
            slot: 0,
            raw_prompt: format!("raw {n}"),
            model: "m".into(),
            completion: Completion {
                text: format!("output {n}"),
                outcome: Outcome::Completed,
                error: None,
                timings: Timings {
                    prompt_tokens: 10,
                    output_tokens: 3,
                    total_ms: 120.0,
                    ..Timings::default()
                },
            },
        }
    }

    #[test]
    fn records_lists_newest_first_and_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let log = ContextLog::new();
        log.set_project(Some(tmp.path().to_path_buf()));
        log.record(&job(1)).unwrap();
        log.record(&job(2)).unwrap();
        let list = log.list(10);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].preview, "output 2");
        let e = log.get(&list[1].id).unwrap();
        assert_eq!(e.raw_prompt, "raw 1");
        assert_eq!(e.sections[0].label, "[1] Role");
        assert_eq!(e.timings.prompt_tokens, 10);
    }

    #[test]
    fn keeps_only_the_newest_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let log = ContextLog::new();
        log.set_project(Some(tmp.path().to_path_buf()));
        log.set_limit(10);
        for n in 0..14 {
            log.record(&job(n)).unwrap();
        }
        let list = log.list(100);
        assert_eq!(list.len(), 10);
        assert_eq!(list[0].preview, "output 13");
        assert_eq!(list[9].preview, "output 4");
    }

    #[test]
    fn clear_empties_it_and_no_project_is_a_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let log = ContextLog::new();
        log.record(&job(1)).unwrap();
        assert!(log.list(5).is_empty());
        log.set_project(Some(tmp.path().to_path_buf()));
        log.record(&job(1)).unwrap();
        assert_eq!(log.clear().unwrap(), 1);
        assert!(log.list(5).is_empty());
    }

    #[test]
    fn ids_cannot_escape_the_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let log = ContextLog::new();
        log.set_project(Some(tmp.path().to_path_buf()));
        assert!(log.get("../../project").is_err());
    }
}
