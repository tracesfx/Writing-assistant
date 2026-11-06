//! Stats Service (SPEC §11.5): words written per day and per session,
//! active writing time, streaks and goal arithmetic. Stored in
//! `stats/sessions.json` inside the project (it cannot be rebuilt from the
//! manuscript, so it is versioned and backed up with it).
//!
//! Words written are the net change in manuscript word counts, measured on
//! every save. Moving documents in or out of the manuscript is not writing
//! and is not counted.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{MarginError, Result};
use crate::events::{DomainEvent, EventBus};
use crate::project::atomic::atomic_write;
use crate::project::manifest::{Goals, Section};
use crate::story::StoryService;

pub mod provenance;

pub const STATS_FILE: &str = "stats/sessions.json";
const SCHEMA_VERSION: u32 = 1;
const MAX_SESSIONS: usize = 1000;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DayStats {
    /// Net words written.
    #[serde(default)]
    pub words: i32,
    #[serde(default)]
    pub active_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub start: String,
    pub end: String,
    pub words: i32,
    pub active_seconds: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatsFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    days: BTreeMap<String, DayStats>,
    #[serde(default)]
    sessions: Vec<SessionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DayPoint {
    pub date: String,
    pub words: i32,
    pub active_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StatsSummary {
    /// Today's local date, `YYYY-MM-DD`.
    pub date: String,
    pub today: DayStats,
    pub session_words: i32,
    pub session_active_seconds: u32,
    pub session_start: String,
    /// Consecutive days with words written, ending today (or yesterday).
    pub streak_days: u32,
    pub project_words: u32,
    pub goals: Goals,
    /// Days with activity, oldest first.
    pub days: Vec<DayPoint>,
    /// Words per day needed to reach the project goal by the deadline.
    pub words_per_day_needed: Option<u32>,
    /// Days left until the deadline, including today.
    pub days_left: Option<u32>,
}

struct Open {
    root: PathBuf,
    file: StatsFile,
    /// Last known word count of each manuscript document.
    counts: HashMap<String, u32>,
    session_start: String,
    session_words: i32,
    session_active: u32,
}

pub struct Stats {
    story: Arc<StoryService>,
    bus: Arc<EventBus>,
    state: Mutex<Option<Open>>,
}

fn today() -> String {
    Local::now().date_naive().format("%Y-%m-%d").to_string()
}

fn now_rfc3339() -> String {
    Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

fn load(root: &Path) -> StatsFile {
    fs::read_to_string(root.join(STATS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<StatsFile>(&t).ok())
        .filter(|f| f.schema_version <= SCHEMA_VERSION)
        .unwrap_or(StatsFile {
            schema_version: SCHEMA_VERSION,
            ..StatsFile::default()
        })
}

fn save(o: &Open) -> Result<()> {
    let mut f = o.file.clone();
    f.schema_version = SCHEMA_VERSION;
    atomic_write(
        &o.root.join(STATS_FILE),
        serde_json::to_string_pretty(&f)?.as_bytes(),
    )?;
    Ok(())
}

/// Consecutive days with words written ending at `today` (or the day before
/// if nothing was written today yet).
pub fn streak(days: &BTreeMap<String, DayStats>, today: NaiveDate) -> u32 {
    let wrote = |d: NaiveDate| {
        days.get(&d.format("%Y-%m-%d").to_string())
            .is_some_and(|s| s.words > 0)
    };
    let mut d = if wrote(today) {
        today
    } else {
        today.pred_opt().unwrap_or(today)
    };
    let mut n = 0;
    while wrote(d) {
        n += 1;
        match d.pred_opt() {
            Some(p) => d = p,
            None => break,
        }
    }
    n
}

/// Words per day to reach `goal` by `deadline` (inclusive), and days left.
pub fn deadline_math(
    goal: Option<u32>,
    words: u32,
    deadline: Option<&str>,
    today: NaiveDate,
) -> (Option<u32>, Option<u32>) {
    let Some(deadline) = deadline.and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
    else {
        return (None, None);
    };
    let days_left = (deadline - today).num_days() + 1;
    if days_left <= 0 {
        return (None, Some(0));
    }
    let days_left = days_left as u32;
    let needed = goal.map(|g| g.saturating_sub(words).div_ceil(days_left));
    (needed, Some(days_left))
}

impl Stats {
    pub fn new(story: Arc<StoryService>, bus: Arc<EventBus>) -> Arc<Self> {
        let s = Arc::new(Stats {
            story,
            bus: bus.clone(),
            state: Mutex::new(None),
        });
        let weak = Arc::downgrade(&s);
        bus.subscribe(move |e| {
            let Some(s) = weak.upgrade() else { return };
            let r = match &e.event {
                DomainEvent::ProjectOpened { .. } => s.open(),
                DomainEvent::ProjectClosed { .. } => s.close(),
                DomainEvent::DocSaved {
                    doc_id, word_count, ..
                } => s.on_saved(doc_id, *word_count),
                DomainEvent::BinderChanged { .. } => s.sync_membership(),
                _ => Ok(()),
            };
            if let Err(err) = r {
                eprintln!("[margin] stats: {err}");
            }
        });
        s
    }

    fn manuscript_counts(&self) -> Result<HashMap<String, u32>> {
        let ids = self.story.section_doc_ids(Section::Manuscript)?;
        let mut out = HashMap::new();
        for id in ids {
            if let Ok(m) = self.story.document_meta(&id) {
                out.insert(id, m.word_count);
            }
        }
        Ok(out)
    }

    fn open(&self) -> Result<()> {
        let root = self.story.project_root()?;
        let counts = self.manuscript_counts()?;
        let mut g = self.lock()?;
        *g = Some(Open {
            file: load(&root),
            root,
            counts,
            session_start: now_rfc3339(),
            session_words: 0,
            session_active: 0,
        });
        Ok(())
    }

    fn close(&self) -> Result<()> {
        let mut g = self.lock()?;
        if let Some(mut o) = g.take()
            && (o.session_words != 0 || o.session_active > 0)
        {
            o.file.sessions.push(SessionRecord {
                start: o.session_start.clone(),
                end: now_rfc3339(),
                words: o.session_words,
                active_seconds: o.session_active,
            });
            let excess = o.file.sessions.len().saturating_sub(MAX_SESSIONS);
            o.file.sessions.drain(..excess);
            save(&o)?;
        }
        Ok(())
    }

    /// Record the session so far (called before the app closes).
    pub fn flush(&self) -> Result<()> {
        self.close()?;
        self.open()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Option<Open>>> {
        self.state
            .lock()
            .map_err(|_| MarginError::Internal("stats lock poisoned".into()))
    }

    fn on_saved(&self, doc_id: &str, word_count: u32) -> Result<()> {
        let in_manuscript = self
            .story
            .section_doc_ids(Section::Manuscript)?
            .iter()
            .any(|id| id == doc_id);
        let today_words = {
            let mut g = self.lock()?;
            let Some(o) = g.as_mut() else { return Ok(()) };
            if !in_manuscript {
                o.counts.remove(doc_id);
                return Ok(());
            }
            let old = o.counts.insert(doc_id.to_string(), word_count).unwrap_or(0);
            let delta = word_count as i64 - old as i64;
            if delta == 0 {
                return Ok(());
            }
            let delta = delta.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            let day = o.file.days.entry(today()).or_default();
            day.words = day.words.saturating_add(delta);
            let today_words = day.words;
            o.session_words = o.session_words.saturating_add(delta);
            save(o)?;
            today_words
        };
        self.bus.publish(DomainEvent::StatsUpdated { today_words });
        Ok(())
    }

    /// A binder change re-baselines every manuscript count: documents moved
    /// in or out, duplicated, split or merged are not writing. Author edits
    /// are always saved (and counted) before a structural operation runs, and
    /// split/merge publish their binder change ahead of their own saves.
    fn sync_membership(&self) -> Result<()> {
        let fresh = self.manuscript_counts()?;
        let mut g = self.lock()?;
        let Some(o) = g.as_mut() else { return Ok(()) };
        o.counts = fresh;
        Ok(())
    }

    /// Add active writing time (reported by the editor while the author types).
    pub fn activity(&self, seconds: u32) -> Result<()> {
        let seconds = seconds.min(600);
        let today_words = {
            let mut g = self.lock()?;
            let Some(o) = g.as_mut() else { return Ok(()) };
            let day = o.file.days.entry(today()).or_default();
            day.active_seconds = day.active_seconds.saturating_add(seconds);
            let w = day.words;
            o.session_active = o.session_active.saturating_add(seconds);
            save(o)?;
            w
        };
        self.bus.publish(DomainEvent::StatsUpdated { today_words });
        Ok(())
    }

    pub fn summary(&self) -> Result<StatsSummary> {
        let info = self.story.project_info()?;
        let project_words = self
            .story
            .binder_tree()?
            .sections
            .iter()
            .find(|s| s.section == Section::Manuscript)
            .map_or(0, |s| s.total_word_count);
        let g = self.lock()?;
        let o = g.as_ref().ok_or(MarginError::NoProject)?;
        let date = today();
        let today_date = Local::now().date_naive();
        let (needed, days_left) = deadline_math(
            info.goals.project_words,
            project_words,
            info.goals.deadline.as_deref(),
            today_date,
        );
        Ok(StatsSummary {
            today: o.file.days.get(&date).copied().unwrap_or_default(),
            date,
            session_words: o.session_words,
            session_active_seconds: o.session_active,
            session_start: o.session_start.clone(),
            streak_days: streak(&o.file.days, today_date),
            project_words,
            goals: info.goals,
            days: o
                .file
                .days
                .iter()
                .map(|(d, s)| DayPoint {
                    date: d.clone(),
                    words: s.words,
                    active_seconds: s.active_seconds,
                })
                .collect(),
            words_per_day_needed: needed,
            days_left,
        })
    }

    pub fn sessions(&self) -> Result<Vec<SessionRecord>> {
        let g = self.lock()?;
        Ok(g.as_ref()
            .map(|o| o.file.sessions.clone())
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::{self, pm};
    use crate::project::manifest::{Locale, ProjectType};
    use crate::story::types::NewProject;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn streaks() {
        let mut days = BTreeMap::new();
        for (k, w) in [("2026-11-01", 10), ("2026-11-02", 5), ("2026-11-03", 7)] {
            days.insert(
                k.to_string(),
                DayStats {
                    words: w,
                    active_seconds: 0,
                },
            );
        }
        assert_eq!(streak(&days, d("2026-11-03")), 3);
        // Nothing yet today: the streak through yesterday still counts.
        assert_eq!(streak(&days, d("2026-11-04")), 3);
        assert_eq!(streak(&days, d("2026-11-06")), 0);
        days.insert(
            "2026-11-02".into(),
            DayStats {
                words: -3,
                active_seconds: 0,
            },
        );
        assert_eq!(streak(&days, d("2026-11-03")), 1);
    }

    #[test]
    fn deadline() {
        assert_eq!(
            deadline_math(Some(1000), 400, Some("2026-11-10"), d("2026-11-05")),
            (Some(100), Some(6))
        );
        assert_eq!(
            deadline_math(Some(1000), 1200, Some("2026-11-10"), d("2026-11-05")),
            (Some(0), Some(6))
        );
        assert_eq!(
            deadline_math(Some(1000), 0, Some("2026-11-01"), d("2026-11-05")),
            (None, Some(0))
        );
        assert_eq!(deadline_math(None, 0, None, d("2026-11-05")), (None, None));
    }

    #[test]
    fn counts_manuscript_writing_only() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new());
        let story = Arc::new(StoryService::new(bus.clone()));
        let stats = Stats::new(story.clone(), bus);
        story
            .create_project(&NewProject {
                parent_dir: dir.path().to_string_lossy().into(),
                title: "Stats".into(),
                author: String::new(),
                project_type: ProjectType::Fiction,
                locale: Locale::EnUs,
            })
            .unwrap();
        let scene = story.section_doc_ids(Section::Manuscript).unwrap()[1].clone();
        let inbox = story.section_doc_ids(Section::Notes).unwrap()[0].clone();
        let save = |id: &str, md: &str| {
            story
                .document_save(id, &pm::to_pm(&dialect::parse(md)), true)
                .unwrap();
        };
        save(&scene, "One two three four.\n");
        save(&scene, "One two three.\n");
        save(&inbox, "Notes do not count toward writing.\n");
        let s = stats.summary().unwrap();
        assert_eq!(s.today.words, 3);
        assert_eq!(s.session_words, 3);
        assert_eq!(s.project_words, 3);
        stats.activity(42).unwrap();
        assert_eq!(stats.summary().unwrap().today.active_seconds, 42);
        stats.flush().unwrap();
        assert_eq!(stats.sessions().unwrap().len(), 1);
        // Persisted in the project.
        let root = story.project_root().unwrap();
        let text = fs::read_to_string(root.join(STATS_FILE)).unwrap();
        assert!(text.contains("\"words\": 3"), "{text}");
    }

    #[test]
    fn split_duplicate_and_merge_are_not_writing() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new());
        let story = Arc::new(StoryService::new(bus.clone()));
        let stats = Stats::new(story.clone(), bus);
        story
            .create_project(&NewProject {
                parent_dir: dir.path().to_string_lossy().into(),
                title: "Moves".into(),
                author: String::new(),
                project_type: ProjectType::Fiction,
                locale: Locale::EnUs,
            })
            .unwrap();
        let scene = story.section_doc_ids(Section::Manuscript).unwrap()[1].clone();
        story
            .document_save(
                &scene,
                &pm::to_pm(&dialect::parse(
                    "One two three.

Four five six seven.
",
                )),
                true,
            )
            .unwrap();
        assert_eq!(stats.summary().unwrap().today.words, 7);

        let before = pm::to_pm(&dialect::parse(
            "One two three.
",
        ));
        let after = pm::to_pm(&dialect::parse(
            "Four five six seven.
",
        ));
        let second = story.document_split(&scene, &before, &after, "").unwrap();
        assert_eq!(stats.summary().unwrap().today.words, 7, "split");

        story
            .binder_duplicate(std::slice::from_ref(&second.id))
            .unwrap();
        assert_eq!(stats.summary().unwrap().today.words, 7, "duplicate");

        story
            .document_merge(&[scene.clone(), second.id.clone()])
            .unwrap();
        let s = stats.summary().unwrap();
        assert_eq!(s.today.words, 7, "merge");
        assert_eq!(s.project_words, 11);

        // Writing after the moves still counts.
        story
            .document_save(
                &scene,
                &pm::to_pm(&dialect::parse(
                    "One two three.

Four five six seven eight.
",
                )),
                true,
            )
            .unwrap();
        assert_eq!(stats.summary().unwrap().today.words, 8);
    }
}
