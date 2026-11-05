//! Version history (SPEC §11.3): a hidden git repository in
//! `.margin/history/` holding snapshots of every project file outside
//! `.margin/`. Snapshots are built straight from the files (no index or
//! working tree), so the project folder never contains git metadata.
//!
//! - automatic snapshot at session start and every 15 minutes of changes
//! - named snapshots
//! - per-document timeline, content at a snapshot, word diff, restore

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use git2::{ObjectType, Oid, Repository, Signature, Tree, TreeBuilder};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use specta::Type;

use crate::dialect::{self, pm::PmNode};
use crate::error::{MarginError, Result};
use crate::events::{DomainEvent, EventBus};
use crate::project::atomic::TEMP_SUFFIX;
use crate::project::document::DocFile;
use crate::project::manifest::{Section, TRASH_FOLDER};
use crate::story::{APP_DIR, StoryService};

const HISTORY_DIR: &str = "history";
const BRANCH: &str = "refs/heads/main";
const KIND_TRAILER: &str = "Margin-Kind: ";
pub const AUTO_INTERVAL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotKind {
    /// Taken when a project is opened.
    Session,
    /// Every 15 minutes of changes.
    Auto,
    /// Named by the author.
    Named,
    /// Taken by Margin before a large change (replace-all, restore).
    Safety,
}

impl SnapshotKind {
    fn tag(self) -> &'static str {
        match self {
            SnapshotKind::Session => "session",
            SnapshotKind::Auto => "auto",
            SnapshotKind::Named => "named",
            SnapshotKind::Safety => "safety",
        }
    }
    fn from_tag(s: &str) -> Self {
        match s {
            "session" => SnapshotKind::Session,
            "named" => SnapshotKind::Named,
            "safety" => SnapshotKind::Safety,
            _ => SnapshotKind::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    pub id: String,
    pub name: String,
    pub kind: SnapshotKind,
    /// RFC 3339.
    pub time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocVersion {
    pub snapshot: SnapshotInfo,
    pub title: String,
    pub word_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocAtSnapshot {
    pub title: String,
    pub content: PmNode,
    pub word_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DiffKind {
    Equal,
    Insert,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiffChunk {
    pub kind: DiffKind,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocDiff {
    pub chunks: Vec<DiffChunk>,
    pub words_added: u32,
    pub words_removed: u32,
}

struct State {
    dirty: bool,
    last: Option<Instant>,
}

pub struct History {
    story: Arc<StoryService>,
    bus: Arc<EventBus>,
    state: Mutex<State>,
    /// Serializes repository access.
    git: Mutex<()>,
    pending: Mutex<Option<std::thread::JoinHandle<()>>>,
}

fn git_err(e: git2::Error) -> MarginError {
    MarginError::Internal(format!("history: {}", e.message()))
}

fn open_repo(root: &Path) -> Result<Repository> {
    let dir = root.join(APP_DIR).join(HISTORY_DIR);
    match Repository::open_bare(&dir) {
        Ok(r) => Ok(r),
        Err(_) => {
            fs::create_dir_all(&dir)?;
            Repository::init_bare(&dir).map_err(git_err)
        }
    }
}

/// Files and folders that belong in history: everything except hidden
/// entries (`.margin`, dot-files) and atomic-write leftovers.
fn tracked(name: &str) -> bool {
    !name.starts_with('.') && !name.ends_with(TEMP_SUFFIX)
}

/// Build a tree object from a directory. Returns `None` for an empty tree.
fn build_tree(repo: &Repository, dir: &Path) -> Result<Option<Oid>> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    let mut tb: TreeBuilder = repo.treebuilder(None).map_err(git_err)?;
    let mut any = false;
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        if !tracked(&name) {
            continue;
        }
        let ft = e.file_type()?;
        if ft.is_dir() {
            if let Some(oid) = build_tree(repo, &e.path())? {
                tb.insert(&name, oid, 0o040000).map_err(git_err)?;
                any = true;
            }
        } else if ft.is_file() {
            let Ok(bytes) = fs::read(e.path()) else {
                continue; // vanished or locked: the next snapshot gets it
            };
            let oid = repo.blob(&bytes).map_err(git_err)?;
            tb.insert(&name, oid, 0o100644).map_err(git_err)?;
            any = true;
        }
    }
    if !any {
        return Ok(None);
    }
    Ok(Some(tb.write().map_err(git_err)?))
}

fn info_of(c: &git2::Commit) -> SnapshotInfo {
    let msg = c.message().unwrap_or("");
    let name = msg.lines().next().unwrap_or("").to_string();
    let kind = msg
        .lines()
        .find_map(|l| l.strip_prefix(KIND_TRAILER))
        .map_or(SnapshotKind::Auto, |k| SnapshotKind::from_tag(k.trim()));
    let time = chrono::DateTime::from_timestamp(c.time().seconds(), 0)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default();
    SnapshotInfo {
        id: c.id().to_string(),
        name,
        kind,
        time,
    }
}

/// Folders a document file can live in.
fn doc_folders() -> Vec<&'static str> {
    Section::ALL
        .iter()
        .map(|s| s.folder())
        .chain(std::iter::once(TRASH_FOLDER))
        .collect()
}

/// The blob of document `id` in a snapshot tree, found by its id suffix so
/// renames and moves between sections are followed.
fn find_doc_blob(repo: &Repository, tree: &Tree, id: &str) -> Option<Oid> {
    let suffix = format!("--{id}.md");
    for folder in doc_folders() {
        let Ok(entry) = tree.get_path(Path::new(folder)) else {
            continue;
        };
        let Ok(obj) = entry.to_object(repo) else {
            continue;
        };
        let Some(sub) = obj.as_tree() else { continue };
        for e in sub.iter() {
            if e.kind() == Some(ObjectType::Blob) && e.name().is_ok_and(|n| n.ends_with(&suffix)) {
                return Some(e.id());
            }
        }
    }
    None
}

fn parse_blob(repo: &Repository, oid: Oid) -> Result<(String, dialect::Document)> {
    let blob = repo.find_blob(oid).map_err(git_err)?;
    let text = String::from_utf8_lossy(blob.content()).to_string();
    let file = DocFile::parse(&text, Path::new("snapshot.md"))?;
    Ok((file.fm.title.clone(), dialect::parse(&file.body)))
}

/// Plain text of a document for diffing: paragraphs separated by blank lines.
fn diff_text(doc: &dialect::Document) -> String {
    let mut parts = Vec::new();
    doc.for_each_text(&mut |t| parts.push(t.replace(dialect::ast::FOOTNOTE_CHAR, "")));
    parts.join("\n\n")
}

pub fn word_diff(old: &str, new: &str) -> DocDiff {
    let diff = TextDiff::configure()
        .algorithm(similar::Algorithm::Patience)
        .timeout(Duration::from_secs(2))
        .diff_words(old, new);
    let mut chunks: Vec<DiffChunk> = Vec::new();
    let (mut added, mut removed) = (0u32, 0u32);
    for change in diff.iter_all_changes() {
        let kind = match change.tag() {
            ChangeTag::Equal => DiffKind::Equal,
            ChangeTag::Insert => DiffKind::Insert,
            ChangeTag::Delete => DiffKind::Delete,
        };
        let text = change.value();
        let words = dialect::count_words(text);
        match kind {
            DiffKind::Insert => added += words,
            DiffKind::Delete => removed += words,
            DiffKind::Equal => {}
        }
        match chunks.last_mut() {
            Some(last) if last.kind == kind => last.text.push_str(text),
            _ => chunks.push(DiffChunk {
                kind,
                text: text.to_string(),
            }),
        }
    }
    DocDiff {
        chunks,
        words_added: added,
        words_removed: removed,
    }
}

impl History {
    pub fn new(story: Arc<StoryService>, bus: Arc<EventBus>) -> Arc<Self> {
        let h = Arc::new(History {
            story,
            bus: bus.clone(),
            state: Mutex::new(State {
                dirty: false,
                last: None,
            }),
            git: Mutex::new(()),
            pending: Mutex::new(None),
        });
        let weak = Arc::downgrade(&h);
        bus.subscribe(move |e| {
            let Some(h) = weak.upgrade() else { return };
            match &e.event {
                DomainEvent::DocSaved { .. } | DomainEvent::BinderChanged { .. } => h.mark_dirty(),
                DomainEvent::ProjectOpened { .. } => {
                    if let Ok(mut s) = h.state.lock() {
                        s.dirty = false;
                        s.last = Some(Instant::now());
                    }
                    let h2 = h.clone();
                    let handle = std::thread::spawn(move || {
                        if let Err(err) = h2.snapshot(Some("Session start"), SnapshotKind::Session)
                        {
                            eprintln!("[margin] session snapshot failed: {err}");
                        }
                    });
                    if let Ok(mut p) = h.pending.lock() {
                        *p = Some(handle);
                    }
                }
                _ => {}
            }
        });
        h
    }

    /// Wait for the session-start snapshot (runs in the background on open).
    pub fn wait_pending(&self) {
        let handle = self.pending.lock().ok().and_then(|mut p| p.take());
        if let Some(h) = handle {
            let _ = h.join();
        }
    }

    fn mark_dirty(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.dirty = true;
        }
    }

    /// Called periodically: snapshot if there were changes and 15 minutes
    /// have passed since the last snapshot.
    pub fn tick(&self) {
        let due = self
            .state
            .lock()
            .map(|s| s.dirty && s.last.is_none_or(|t| t.elapsed() >= AUTO_INTERVAL))
            .unwrap_or(false);
        if due && let Err(e) = self.snapshot(None, SnapshotKind::Auto) {
            eprintln!("[margin] auto snapshot failed: {e}");
        }
    }

    /// Take a snapshot. Unnamed snapshots are skipped when nothing changed
    /// (returns `None`); named and safety snapshots are always recorded.
    pub fn snapshot(&self, name: Option<&str>, kind: SnapshotKind) -> Result<Option<SnapshotInfo>> {
        let root = self.story.project_root()?;
        let _g = self
            .git
            .lock()
            .map_err(|_| MarginError::Internal("history lock poisoned".into()))?;
        let repo = open_repo(&root)?;
        let tree_oid = match build_tree(&repo, &root)? {
            Some(t) => t,
            None => repo
                .treebuilder(None)
                .and_then(|tb| tb.write())
                .map_err(git_err)?,
        };
        let parent = repo
            .find_reference(BRANCH)
            .ok()
            .and_then(|r| r.peel_to_commit().ok());
        let unchanged = parent.as_ref().is_some_and(|c| c.tree_id() == tree_oid);
        if unchanged && matches!(kind, SnapshotKind::Auto | SnapshotKind::Session) {
            if let Ok(mut s) = self.state.lock() {
                s.dirty = false;
                s.last = Some(Instant::now());
            }
            return Ok(None);
        }
        let label = name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or(match kind {
                SnapshotKind::Session => "Session start",
                SnapshotKind::Auto => "Automatic snapshot",
                SnapshotKind::Named => "Snapshot",
                SnapshotKind::Safety => "Before a change",
            });
        let message = format!("{label}\n\n{KIND_TRAILER}{}\n", kind.tag());
        let sig = Signature::now("Margin", "margin@localhost").map_err(git_err)?;
        let tree = repo.find_tree(tree_oid).map_err(git_err)?;
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        let oid = repo
            .commit(Some(BRANCH), &sig, &sig, &message, &tree, &parents)
            .map_err(git_err)?;
        let commit = repo.find_commit(oid).map_err(git_err)?;
        let info = info_of(&commit);
        if let Ok(mut s) = self.state.lock() {
            s.dirty = false;
            s.last = Some(Instant::now());
        }
        self.bus.publish(DomainEvent::HistorySnapshot {
            snapshot_id: info.id.clone(),
            name: info.name.clone(),
        });
        Ok(Some(info))
    }

    fn with_repo<T>(&self, f: impl FnOnce(&Repository) -> Result<T>) -> Result<T> {
        let root = self.story.project_root()?;
        let _g = self
            .git
            .lock()
            .map_err(|_| MarginError::Internal("history lock poisoned".into()))?;
        f(&open_repo(&root)?)
    }

    /// All snapshots, newest first.
    pub fn list(&self, limit: usize) -> Result<Vec<SnapshotInfo>> {
        self.with_repo(|repo| {
            let Ok(head) = repo.find_reference(BRANCH).and_then(|r| r.peel_to_commit()) else {
                return Ok(Vec::new());
            };
            let mut walk = repo.revwalk().map_err(git_err)?;
            walk.push(head.id()).map_err(git_err)?;
            let mut out = Vec::new();
            for oid in walk.take(limit) {
                let oid = oid.map_err(git_err)?;
                out.push(info_of(&repo.find_commit(oid).map_err(git_err)?));
            }
            Ok(out)
        })
    }

    /// Snapshots in which a document's file differs from the snapshot
    /// before, newest first.
    pub fn doc_versions(&self, doc_id: &str) -> Result<Vec<DocVersion>> {
        self.with_repo(|repo| {
            let Ok(head) = repo.find_reference(BRANCH).and_then(|r| r.peel_to_commit()) else {
                return Ok(Vec::new());
            };
            let mut walk = repo.revwalk().map_err(git_err)?;
            walk.push(head.id()).map_err(git_err)?;
            let mut seq: Vec<(git2::Commit, Option<Oid>)> = Vec::new();
            for oid in walk.take(5000) {
                let c = repo.find_commit(oid.map_err(git_err)?).map_err(git_err)?;
                let blob = c.tree().ok().and_then(|t| find_doc_blob(repo, &t, doc_id));
                seq.push((c, blob));
            }
            let mut out = Vec::new();
            for i in 0..seq.len() {
                let Some(blob) = seq[i].1 else { continue };
                let older = seq.get(i + 1).and_then(|x| x.1);
                if older == Some(blob) {
                    continue;
                }
                let (title, doc) = parse_blob(repo, blob)?;
                out.push(DocVersion {
                    snapshot: info_of(&seq[i].0),
                    title,
                    word_count: dialect::word_count(&doc),
                });
            }
            Ok(out)
        })
    }

    fn doc_ast_at(
        &self,
        repo: &Repository,
        doc_id: &str,
        snapshot: &str,
    ) -> Result<(String, dialect::Document)> {
        let oid = Oid::from_str(snapshot).map_err(git_err)?;
        let commit = repo.find_commit(oid).map_err(git_err)?;
        let tree = commit.tree().map_err(git_err)?;
        let blob = find_doc_blob(repo, &tree, doc_id)
            .ok_or_else(|| MarginError::NotFound("this document is not in that snapshot".into()))?;
        parse_blob(repo, blob)
    }

    pub fn doc_at(&self, doc_id: &str, snapshot: &str) -> Result<DocAtSnapshot> {
        self.with_repo(|repo| {
            let (title, doc) = self.doc_ast_at(repo, doc_id, snapshot)?;
            Ok(DocAtSnapshot {
                title,
                word_count: dialect::word_count(&doc),
                content: dialect::pm::to_pm(&doc),
            })
        })
    }

    /// Word diff between two versions of a document. `None` = the file on disk now.
    pub fn diff(&self, doc_id: &str, from: Option<&str>, to: Option<&str>) -> Result<DocDiff> {
        let current = || self.story.document_ast(doc_id);
        self.with_repo(|repo| {
            let old = match from {
                Some(s) => self.doc_ast_at(repo, doc_id, s)?.1,
                None => current()?,
            };
            let new = match to {
                Some(s) => self.doc_ast_at(repo, doc_id, s)?.1,
                None => current()?,
            };
            Ok(word_diff(&diff_text(&old), &diff_text(&new)))
        })
    }

    /// Restore documents' text from a snapshot. A safety snapshot is taken
    /// first and returned, so the restore itself can be undone.
    pub fn restore_docs(
        &self,
        snapshot: &str,
        doc_ids: &[String],
        label: &str,
    ) -> Result<Option<SnapshotInfo>> {
        let docs = self.with_repo(|repo| {
            doc_ids
                .iter()
                .map(|id| Ok((id.clone(), self.doc_ast_at(repo, id, snapshot)?.1)))
                .collect::<Result<Vec<_>>>()
        })?;
        let safety = self.snapshot(Some(label), SnapshotKind::Safety)?;
        self.story.replace_bodies(&docs, "restore")?;
        Ok(safety)
    }

    pub fn story(&self) -> &Arc<StoryService> {
        &self.story
    }

    pub fn repo_dir(root: &Path) -> PathBuf {
        root.join(APP_DIR).join(HISTORY_DIR)
    }
}

#[cfg(test)]
mod tests;
