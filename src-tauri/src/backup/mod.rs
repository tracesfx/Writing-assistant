//! Backups (SPEC §11.3): scheduled zip archives of the project folder
//! (everything except `.margin/`) to a chosen folder, with retention:
//! the 10 most recent, plus the newest per day for 14 days, plus the newest
//! per week for 8 weeks. Restoring unpacks into a new folder; it never
//! overwrites the project.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::app_state::{AppState, BackupConfig};
use crate::error::{MarginError, Result};
use crate::events::{DomainEvent, EventBus};
use crate::project::atomic::{TEMP_SUFFIX, rename_with_retry};
use crate::project::manifest::{PROJECT_FILE, ProjectManifest};
use crate::story::StoryService;

const STAMP: &str = "%Y-%m-%d %H%M%S";

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub path: String,
    pub name: String,
    /// Local time, `YYYY-MM-DD HH:MM:SS`.
    pub created_at: String,
    pub size_bytes: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    pub config: BackupConfig,
    /// The folder backups of this project go to.
    pub folder: String,
    pub backups: Vec<BackupInfo>,
}

struct State {
    dirty: bool,
    last: Option<Instant>,
}

pub struct Backups {
    story: Arc<StoryService>,
    app: Arc<AppState>,
    bus: Arc<EventBus>,
    default_dir: PathBuf,
    state: Mutex<State>,
    running: Mutex<()>,
}

/// Windows-safe folder or file name.
fn safe_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect();
    let t = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.trim_end_matches(['.', ' ']).to_string();
    if t.is_empty() { "Project".into() } else { t }
}

fn stamp_of(path: &Path) -> Option<NaiveDateTime> {
    let stem = path.file_stem()?.to_str()?;
    let n = stem.chars().count();
    if n < 17 {
        return None;
    }
    let tail: String = stem.chars().skip(n - 17).collect();
    NaiveDateTime::parse_from_str(&tail, STAMP).ok()
}

/// Which backups to delete under the retention policy.
pub fn to_delete(backups: &[(PathBuf, NaiveDateTime)], now: NaiveDateTime) -> Vec<PathBuf> {
    let mut sorted: Vec<&(PathBuf, NaiveDateTime)> = backups.iter().collect();
    sorted.sort_by_key(|x| std::cmp::Reverse(x.1)); // newest first
    let mut keep: HashSet<&PathBuf> = sorted.iter().take(10).map(|(p, _)| p).collect();
    let mut days_seen = HashSet::new();
    let mut weeks_seen = HashSet::new();
    for (p, t) in &sorted {
        let age = now.signed_duration_since(*t);
        if age.num_days() < 14 && days_seen.insert(t.date()) {
            keep.insert(p);
        }
        let week = t.date().iso_week();
        if age.num_weeks() < 8 && weeks_seen.insert((week.year(), week.week())) {
            keep.insert(p);
        }
    }
    sorted
        .iter()
        .filter(|(p, _)| !keep.contains(p))
        .map(|(p, _)| p.clone())
        .collect()
}

fn add_dir(zip: &mut zip::ZipWriter<File>, root: &Path, dir: &Path) -> Result<()> {
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut entries: Vec<_> = fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.ends_with(TEMP_SUFFIX) {
            continue;
        }
        let path = e.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let ft = e.file_type()?;
        if ft.is_dir() {
            zip.add_directory(format!("{rel}/"), opts)
                .map_err(zip_err)?;
            add_dir(zip, root, &path)?;
        } else if ft.is_file() {
            let Ok(bytes) = fs::read(&path) else { continue };
            zip.start_file(rel, opts).map_err(zip_err)?;
            zip.write_all(&bytes)?;
        }
    }
    Ok(())
}

fn zip_err(e: zip::result::ZipError) -> MarginError {
    MarginError::Io(format!("zip: {e}"))
}

impl Backups {
    pub fn new(
        story: Arc<StoryService>,
        app: Arc<AppState>,
        bus: Arc<EventBus>,
        default_dir: PathBuf,
    ) -> Arc<Self> {
        let b = Arc::new(Backups {
            story,
            app,
            bus: bus.clone(),
            default_dir,
            state: Mutex::new(State {
                dirty: false,
                last: None,
            }),
            running: Mutex::new(()),
        });
        let weak = Arc::downgrade(&b);
        bus.subscribe(move |e| {
            let Some(b) = weak.upgrade() else { return };
            let Ok(mut s) = b.state.lock() else { return };
            match &e.event {
                DomainEvent::DocSaved { .. } | DomainEvent::BinderChanged { .. } => s.dirty = true,
                DomainEvent::ProjectOpened { .. } => {
                    s.dirty = false;
                    s.last = Some(Instant::now());
                }
                _ => {}
            }
        });
        b
    }

    fn project_id(&self) -> Result<String> {
        Ok(self.story.project_info()?.id)
    }

    pub fn config(&self) -> Result<BackupConfig> {
        Ok(self.app.backup_config(&self.project_id()?))
    }

    pub fn set_config(&self, cfg: BackupConfig) -> Result<BackupStatus> {
        let cfg = BackupConfig {
            interval_minutes: cfg.interval_minutes.clamp(5, 24 * 60),
            dir: cfg.dir.filter(|d| !d.trim().is_empty()),
            ..cfg
        };
        self.app.set_backup_config(&self.project_id()?, cfg)?;
        self.status()
    }

    /// The folder receiving this project's backups.
    fn folder(&self) -> Result<PathBuf> {
        let info = self.story.project_info()?;
        let cfg = self.app.backup_config(&info.id);
        let base = cfg
            .dir
            .map(PathBuf::from)
            .unwrap_or_else(|| self.default_dir.clone());
        Ok(base.join(safe_name(&info.title)))
    }

    pub fn status(&self) -> Result<BackupStatus> {
        let folder = self.folder()?;
        Ok(BackupStatus {
            config: self.config()?,
            folder: folder.to_string_lossy().to_string(),
            backups: list_in(&folder),
        })
    }

    /// Write a backup now and apply the retention policy.
    pub fn backup_now(&self) -> Result<BackupInfo> {
        let _running = self
            .running
            .lock()
            .map_err(|_| MarginError::Internal("backup lock poisoned".into()))?;
        let result = self.write_backup();
        self.bus.publish(match &result {
            Ok(info) => DomainEvent::BackupFinished {
                path: Some(info.path.clone()),
                error: None,
            },
            Err(e) => DomainEvent::BackupFinished {
                path: None,
                error: Some(e.to_string()),
            },
        });
        if result.is_ok()
            && let Ok(mut s) = self.state.lock()
        {
            s.dirty = false;
            s.last = Some(Instant::now());
        }
        result
    }

    fn write_backup(&self) -> Result<BackupInfo> {
        let root = self.story.project_root()?;
        let info = self.story.project_info()?;
        let folder = self.folder()?;
        fs::create_dir_all(&folder)?;
        let now = Local::now().naive_local();
        let mut path = folder.join(format!(
            "{} {}.zip",
            safe_name(&info.title),
            now.format(STAMP)
        ));
        if path.exists() {
            path = folder.join(format!(
                "{} {}.zip",
                safe_name(&info.title),
                (now + chrono::Duration::seconds(1)).format(STAMP)
            ));
        }
        let tmp = path.with_extension(format!("zip{TEMP_SUFFIX}"));
        let written = (|| -> Result<()> {
            let file = File::create(&tmp)?;
            let mut zip = zip::ZipWriter::new(file);
            add_dir(&mut zip, &root, &root)?;
            let file = zip.finish().map_err(zip_err)?;
            file.sync_all()?;
            rename_with_retry(&tmp, &path)?;
            Ok(())
        })();
        if let Err(e) = written {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        // Retention.
        let all: Vec<(PathBuf, NaiveDateTime)> = list_in(&folder)
            .into_iter()
            .filter_map(|b| {
                let p = PathBuf::from(&b.path);
                stamp_of(&p).map(|t| (p, t))
            })
            .collect();
        for p in to_delete(&all, now) {
            let _ = fs::remove_file(p);
        }
        Ok(info_of(&path))
    }

    /// Called every minute: back up when enabled, changed, and due.
    pub fn tick(&self) {
        let Ok(cfg) = self.config() else { return };
        if !cfg.enabled {
            return;
        }
        let due = self
            .state
            .lock()
            .map(|s| {
                s.dirty
                    && s.last.is_none_or(|t| {
                        t.elapsed() >= Duration::from_secs(cfg.interval_minutes as u64 * 60)
                    })
            })
            .unwrap_or(false);
        if due && let Err(e) = self.backup_now() {
            eprintln!("[margin] scheduled backup failed: {e}");
        }
    }

    /// Back up before closing if anything changed since the last backup.
    pub fn backup_if_dirty(&self) -> Result<Option<BackupInfo>> {
        let cfg = self.config()?;
        let dirty = self.state.lock().map(|s| s.dirty).unwrap_or(false);
        if cfg.enabled && dirty {
            return self.backup_now().map(Some);
        }
        Ok(None)
    }

    /// Unpack a backup into a new folder next to the current project (or
    /// next to the backup if no project is open). Returns the new root.
    /// The restored copy gets a new project id so both can be opened.
    pub fn restore(&self, zip_path: &str) -> Result<String> {
        let zip_path = PathBuf::from(zip_path);
        let parent = match self.story.project_root() {
            Ok(r) => r.parent().map(Path::to_path_buf),
            Err(_) => None,
        }
        .or_else(|| zip_path.parent().map(Path::to_path_buf))
        .ok_or_else(|| MarginError::Invalid("no folder to restore into".into()))?;
        let stem = zip_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Backup")
            .to_string();
        let base = format!(
            "{} (restored {})",
            safe_name(&stem),
            Local::now().format("%Y-%m-%d %H%M")
        );
        let mut dest = parent.join(&base);
        let mut n = 2;
        while dest.exists() {
            dest = parent.join(format!("{base} {n}"));
            n += 1;
        }
        let mut archive = zip::ZipArchive::new(File::open(&zip_path)?).map_err(zip_err)?;
        if archive.by_name(PROJECT_FILE).is_err() {
            return Err(MarginError::Invalid(format!(
                "{} is not a Margin backup",
                zip_path.display()
            )));
        }
        fs::create_dir_all(&dest)?;
        for i in 0..archive.len() {
            let mut f = archive.by_index(i).map_err(zip_err)?;
            // `enclosed_name` rejects absolute paths and `..` (zip slip).
            let Some(rel) = f.enclosed_name() else {
                continue;
            };
            let out = dest.join(rel);
            if f.is_dir() {
                fs::create_dir_all(&out)?;
                continue;
            }
            if let Some(p) = out.parent() {
                fs::create_dir_all(p)?;
            }
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes)?;
            fs::write(&out, bytes)?;
        }
        let manifest_path = dest.join(PROJECT_FILE);
        let mut m = ProjectManifest::from_yaml(&fs::read_to_string(&manifest_path)?)?;
        m.id = ulid::Ulid::generate().to_string();
        crate::project::atomic::atomic_write(&manifest_path, m.to_yaml()?.as_bytes())?;
        Ok(dest.to_string_lossy().to_string())
    }
}

fn info_of(path: &Path) -> BackupInfo {
    BackupInfo {
        path: path.to_string_lossy().to_string(),
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        created_at: stamp_of(path)
            .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default(),
        size_bytes: fs::metadata(path).map(|m| m.len() as f64).unwrap_or(0.0),
    }
}

/// Backups in a folder, newest first.
fn list_in(folder: &Path) -> Vec<BackupInfo> {
    let Ok(rd) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut v: Vec<(NaiveDateTime, BackupInfo)> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("zip")))
        .filter_map(|p| stamp_of(&p).map(|t| (t, info_of(&p))))
        .collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.0));
    v.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventBus;
    use crate::project::manifest::{Locale, ProjectType};
    use crate::story::types::NewProject;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    #[test]
    fn retention_keeps_recent_daily_and_weekly() {
        let now = t("2026-11-04 12:00");
        let mut v = Vec::new();
        // Every hour for 3 days, then every day for 120 days.
        for h in 0..72 {
            let ts = now - chrono::Duration::hours(h);
            v.push((PathBuf::from(format!("h{h}")), ts));
        }
        for d in 3..120 {
            let ts = now - chrono::Duration::days(d);
            v.push((PathBuf::from(format!("d{d}")), ts));
        }
        let del: HashSet<PathBuf> = to_delete(&v, now).into_iter().collect();
        let kept: Vec<&(PathBuf, NaiveDateTime)> =
            v.iter().filter(|(p, _)| !del.contains(p)).collect();
        // The 10 newest are kept.
        for h in 0..10 {
            assert!(!del.contains(&PathBuf::from(format!("h{h}"))));
        }
        // At least one per day for the last 14 days.
        for d in 0..14 {
            let date = (now - chrono::Duration::days(d)).date();
            assert!(kept.iter().any(|(_, ts)| ts.date() == date), "day {d}");
        }
        // Nothing older than 8 weeks survives.
        assert!(
            kept.iter()
                .all(|(_, ts)| now.signed_duration_since(*ts).num_weeks() < 8)
        );
        // Weekly coverage between 2 and 8 weeks.
        let weeks: HashSet<_> = kept
            .iter()
            .map(|(_, ts)| (ts.date().iso_week().year(), ts.date().iso_week().week()))
            .collect();
        assert!(weeks.len() >= 8);
        assert!(kept.len() < 40, "kept {}", kept.len());
    }

    #[test]
    fn backup_and_restore_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new());
        let story = Arc::new(StoryService::new(bus.clone()));
        let app = Arc::new(AppState::load(dir.path().join("config")));
        let info = story
            .create_project(&NewProject {
                parent_dir: dir.path().to_string_lossy().into(),
                title: "Back Me Up".into(),
                author: String::new(),
                project_type: ProjectType::Fiction,
                locale: Locale::EnUs,
            })
            .unwrap();
        let root = PathBuf::from(&info.root);
        fs::write(root.join(".margin").join("cache.bin"), b"skip me").unwrap();
        let backups = Backups::new(story.clone(), app, bus, dir.path().join("backups"));
        let b = backups.backup_now().unwrap();
        assert!(b.name.starts_with("Back Me Up "));
        assert_eq!(backups.status().unwrap().backups.len(), 1);

        let restored = PathBuf::from(backups.restore(&b.path).unwrap());
        assert!(restored.join(PROJECT_FILE).is_file());
        assert!(restored.join("manuscript").is_dir());
        assert!(!restored.join(".margin").exists());
        let m =
            ProjectManifest::from_yaml(&fs::read_to_string(restored.join(PROJECT_FILE)).unwrap())
                .unwrap();
        assert_ne!(m.id, info.id, "a restored copy gets its own id");
        assert_eq!(m.title, "Back Me Up");
    }

    #[test]
    fn parses_stamps_from_names() {
        let p = PathBuf::from("C:/b/My Book 2026-11-04 183012.zip");
        assert_eq!(
            stamp_of(&p),
            Some(t("2026-11-04 18:30") + chrono::Duration::seconds(12))
        );
        assert_eq!(stamp_of(Path::new("random.zip")), None);
    }
}
