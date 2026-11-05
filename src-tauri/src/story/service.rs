//! Story Data Service (SPEC §5.1): the single source of truth for project
//! structure and documents. Reads are open; writes are validated commands
//! that persist atomically and emit domain events.
//!
//! The API is synchronous. The Tauri layer runs every call on a blocking
//! worker thread, never on the main thread.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use sha2::{Digest, Sha256};

use super::tree::{self, Slot};
use super::types::*;
use crate::dialect::{self, pm::PmNode};
use crate::error::{MarginError, Result};
use crate::events::{DomainEvent, EventBus};
use crate::project::atomic::{atomic_write, cleanup_temp_files, rename_with_retry};
use crate::project::document::{DocFile, DocKind, Frontmatter, file_name};
use crate::project::manifest::{
    BinderEntry, PROJECT_FILE, ProjectManifest, Section, TRASH_FOLDER, TrashEntry, TrashOrigin,
};

pub const APP_DIR: &str = ".margin";
const RECOVERY_DIR: &str = "recovery";

#[derive(Debug, Clone)]
pub(super) struct DocEntry {
    pub(super) path: PathBuf,
    pub(super) fm: Frontmatter,
    pub(super) word_count: u32,
    /// Hash of the file bytes as last read or written by Margin.
    pub(super) revision: String,
    /// Revision of an outside change already reported, so it is announced once.
    pub(super) disk_seen: Option<String>,
}

#[derive(Debug)]
pub(super) struct OpenProject {
    pub(super) root: PathBuf,
    pub(super) manifest: ProjectManifest,
    pub(super) docs: HashMap<String, DocEntry>,
    pub(super) problems: Vec<String>,
}

pub struct StoryService {
    state: Mutex<Option<OpenProject>>,
    bus: Arc<EventBus>,
}

pub fn revision_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .take(10)
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(super) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub(super) fn rel_path(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Folder name for a new project, safe on Windows.
fn folder_name_for(title: &str) -> String {
    let cleaned: String = title
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
    if t.is_empty() {
        "Untitled Project".to_string()
    } else {
        t
    }
}

impl StoryService {
    pub fn new(bus: Arc<EventBus>) -> Self {
        StoryService {
            state: Mutex::new(None),
            bus,
        }
    }

    pub fn bus(&self) -> &Arc<EventBus> {
        &self.bus
    }

    fn lock(&self) -> Result<MutexGuard<'_, Option<OpenProject>>> {
        self.state
            .lock()
            .map_err(|_| MarginError::Internal("project state lock poisoned".into()))
    }

    pub(super) fn read<T>(&self, f: impl FnOnce(&OpenProject) -> Result<T>) -> Result<T> {
        let g = self.lock()?;
        let p = g.as_ref().ok_or(MarginError::NoProject)?;
        f(p)
    }

    /// Run a write; events are published after the lock is released.
    pub(super) fn write<T>(
        &self,
        f: impl FnOnce(&mut OpenProject) -> Result<(T, Vec<DomainEvent>)>,
    ) -> Result<T> {
        let (out, events) = {
            let mut g = self.lock()?;
            let p = g.as_mut().ok_or(MarginError::NoProject)?;
            f(p)?
        };
        self.bus.publish_all(events);
        Ok(out)
    }

    pub fn project_root(&self) -> Result<PathBuf> {
        self.read(|p| Ok(p.root.clone()))
    }

    // -----------------------------------------------------------------------
    // Project lifecycle
    // -----------------------------------------------------------------------

    pub fn create_project(&self, req: &NewProject) -> Result<ProjectInfo> {
        let title = req.title.trim();
        if title.is_empty() {
            return Err(MarginError::Invalid("a project needs a title".into()));
        }
        let parent = PathBuf::from(&req.parent_dir);
        if !parent.is_dir() {
            return Err(MarginError::NotFound(format!(
                "folder {}",
                parent.display()
            )));
        }
        let root = parent.join(folder_name_for(title));
        if root.exists() && fs::read_dir(&root)?.next().is_some() {
            return Err(MarginError::Invalid(format!(
                "{} already exists and is not empty",
                root.display()
            )));
        }
        fs::create_dir_all(&root)?;
        for s in Section::ALL {
            fs::create_dir_all(root.join(s.folder()))?;
        }
        fs::create_dir_all(root.join(TRASH_FOLDER))?;
        fs::create_dir_all(root.join(APP_DIR))?;

        let mut manifest =
            ProjectManifest::new(title, req.author.trim(), req.project_type, req.locale);
        let mk = |kind: DocKind, title: &str, section: Section| -> Result<String> {
            let id = ulid::Ulid::generate().to_string();
            let mut fm = Frontmatter::new(id.clone(), kind, title.to_string());
            if kind == DocKind::Scene {
                fm.status = Some("draft".into());
            }
            let doc = DocFile {
                fm,
                body: String::new(),
                needs_rewrite: false,
            };
            atomic_write(
                &root.join(section.folder()).join(file_name(title, &id)),
                doc.render()?.as_bytes(),
            )?;
            Ok(id)
        };
        let chapter = mk(DocKind::Chapter, "Chapter 1", Section::Manuscript)?;
        let scene = mk(DocKind::Scene, "Scene 1", Section::Manuscript)?;
        let inbox = mk(DocKind::Note, "Inbox", Section::Notes)?;
        manifest.binder.manuscript.push(BinderEntry {
            id: chapter,
            children: vec![BinderEntry::leaf(scene)],
        });
        manifest.binder.notes.push(BinderEntry::leaf(inbox));
        atomic_write(&root.join(PROJECT_FILE), manifest.to_yaml()?.as_bytes())?;
        self.open_project(&root)
    }

    pub fn open_project(&self, root: &Path) -> Result<ProjectInfo> {
        let project = scan(root)?;
        let info = project_info(&project);
        {
            let mut g = self.lock()?;
            *g = Some(project);
        }
        self.bus.publish(DomainEvent::ProjectOpened {
            project_id: info.id.clone(),
            title: info.title.clone(),
            root: info.root.clone(),
        });
        Ok(info)
    }

    /// Re-read the project folder after changes made outside Margin (files
    /// added, removed or renamed). Documents whose content changed keep the
    /// revision Margin knew, so an open editor still detects the conflict.
    pub fn rescan(&self) -> Result<()> {
        self.write(|p| {
            let mut fresh = scan(&p.root)?;
            let mut changed = Vec::new();
            for (id, e) in fresh.docs.iter_mut() {
                if let Some(old) = p.docs.get(id)
                    && old.revision != e.revision
                {
                    if old.disk_seen.as_deref() != Some(e.revision.as_str()) {
                        changed.push(id.clone());
                    }
                    e.disk_seen = Some(e.revision.clone());
                    e.revision = old.revision.clone();
                }
            }
            *p = fresh;
            let mut events: Vec<DomainEvent> = changed
                .into_iter()
                .map(|doc_id| DomainEvent::DocChangedOnDisk { doc_id })
                .collect();
            events.push(DomainEvent::BinderChanged {
                reason: "external".into(),
                doc_ids: Vec::new(),
            });
            Ok(((), events))
        })
    }
}

/// Read a project folder: the manifest, every document, and a binder
/// reconciled with the files actually present.
fn scan(root: &Path) -> Result<OpenProject> {
    {
        let root = root.to_path_buf();
        let manifest_path = root.join(PROJECT_FILE);
        if !manifest_path.is_file() {
            return Err(MarginError::NotFound(format!(
                "{} (not a Margin project)",
                manifest_path.display()
            )));
        }
        let mut manifest = ProjectManifest::from_yaml(&fs::read_to_string(&manifest_path)?)?;
        let _ = cleanup_temp_files(&root);
        for s in Section::ALL {
            fs::create_dir_all(root.join(s.folder()))?;
        }
        fs::create_dir_all(root.join(TRASH_FOLDER))?;
        fs::create_dir_all(root.join(APP_DIR))?;

        let mut docs: HashMap<String, DocEntry> = HashMap::new();
        let mut problems = Vec::new();
        let mut unreadable: HashSet<String> = HashSet::new();
        // Where each file was found, to place unreferenced files.
        let mut found_in: Vec<(String, Option<Section>)> = Vec::new();

        let folders: Vec<(Option<Section>, PathBuf)> = Section::ALL
            .iter()
            .map(|&s| (Some(s), root.join(s.folder())))
            .chain(std::iter::once((None, root.join(TRASH_FOLDER))))
            .collect();
        for (section, dir) in folders {
            let mut paths: Vec<PathBuf> = fs::read_dir(&dir)?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("md"))
                })
                .collect();
            // Canonically named files (`<slug>--<ULID>.md`) first, so that when a
            // file was copied outside Margin the original keeps its id.
            let canonical = |p: &PathBuf| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.rsplit_once("--"))
                    .is_some_and(|(_, id)| ulid::Ulid::from_string(id).is_ok())
            };
            paths.sort_by_key(|p| (!canonical(p), p.clone()));
            for path in paths {
                let text = match fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(e) => {
                        problems.push(format!("{}: {e}", rel_path(&root, &path)));
                        continue;
                    }
                };
                let mut doc = match DocFile::parse(&text, &path) {
                    Ok(d) => d,
                    Err(e) => {
                        problems.push(e.to_string());
                        if let Some(id) = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .and_then(|s| s.rsplit_once("--"))
                        {
                            unreadable.insert(id.1.to_string());
                        }
                        continue;
                    }
                };
                let mut path = path;
                if doc.fm.kind.is_none() {
                    doc.fm.kind = Some(if section == Some(Section::Manuscript) {
                        DocKind::Scene
                    } else {
                        DocKind::Note
                    });
                }
                if docs.contains_key(&doc.fm.id) {
                    // A copied file: give the copy its own identity.
                    doc.fm.id = ulid::Ulid::generate().to_string();
                    doc.needs_rewrite = true;
                    let new_path = path.with_file_name(file_name(&doc.fm.title, &doc.fm.id));
                    rename_with_retry(&path, &new_path)?;
                    path = new_path;
                }
                let mut bytes_text = text;
                if doc.needs_rewrite {
                    bytes_text = doc.render()?;
                    atomic_write(&path, bytes_text.as_bytes())?;
                }
                let word_count = dialect::word_count(&dialect::parse(&doc.body));
                found_in.push((doc.fm.id.clone(), section));
                docs.insert(
                    doc.fm.id.clone(),
                    DocEntry {
                        path,
                        fm: doc.fm,
                        word_count,
                        revision: revision_of(bytes_text.as_bytes()),
                        disk_seen: None,
                    },
                );
            }
        }

        // Reconcile the binder with the files on disk.
        let before = manifest.binder.clone();
        let keep = |id: &str| docs.contains_key(id) || unreadable.contains(id);
        for s in Section::ALL {
            tree::retain(manifest.binder.section_mut(s), &keep);
        }
        manifest.binder.trash.retain(|t| keep(&t.id));
        for t in &mut manifest.binder.trash {
            tree::retain(&mut t.children, &keep);
        }
        let mut referenced: HashSet<String> = tree::all_section_ids(&manifest.binder)
            .into_iter()
            .collect();
        for t in &manifest.binder.trash {
            referenced.insert(t.id.clone());
            for c in &t.children {
                referenced.extend(c.ids());
            }
        }
        for (id, section) in found_in {
            if referenced.contains(&id) {
                continue;
            }
            match section {
                Some(s) => manifest
                    .binder
                    .section_mut(s)
                    .push(BinderEntry::leaf(id.clone())),
                None => manifest.binder.trash.push(TrashEntry {
                    id: id.clone(),
                    children: Vec::new(),
                    from: TrashOrigin {
                        section: Section::Manuscript,
                        parent: None,
                        index: u32::MAX,
                    },
                    trashed_at: now_rfc3339(),
                }),
            }
            referenced.insert(id);
        }
        if manifest.binder != before {
            atomic_write(&manifest_path, manifest.to_yaml()?.as_bytes())?;
        }

        Ok(OpenProject {
            root: root.clone(),
            manifest,
            docs,
            problems,
        })
    }
}

impl StoryService {
    pub fn close_project(&self) -> Result<()> {
        let closed = {
            let mut g = self.lock()?;
            g.take()
        };
        if let Some(p) = closed {
            self.bus.publish(DomainEvent::ProjectClosed {
                project_id: p.manifest.id,
            });
        }
        Ok(())
    }

    pub fn project_info(&self) -> Result<ProjectInfo> {
        self.read(|p| Ok(project_info(p)))
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    pub fn binder_tree(&self) -> Result<BinderTree> {
        self.read(|p| {
            let sections = Section::ALL
                .iter()
                .map(|&s| {
                    let nodes = view_nodes(p, p.manifest.binder.section(s));
                    let total_word_count = nodes.iter().map(|n| n.total_word_count).sum();
                    BinderSectionView {
                        section: s,
                        nodes,
                        total_word_count,
                    }
                })
                .collect();
            let trash = p
                .manifest
                .binder
                .trash
                .iter()
                .filter_map(|t| {
                    let entry = BinderEntry {
                        id: t.id.clone(),
                        children: t.children.clone(),
                    };
                    view_nodes(p, std::slice::from_ref(&entry))
                        .into_iter()
                        .next()
                        .map(|node| TrashNode {
                            node,
                            from: t.from.clone(),
                            trashed_at: t.trashed_at.clone(),
                        })
                })
                .collect();
            Ok(BinderTree { sections, trash })
        })
    }

    pub fn document_meta(&self, id: &str) -> Result<DocMeta> {
        self.read(|p| doc_meta(p, id))
    }

    /// Load a document into an editor. The file on disk becomes the known
    /// revision: saves from this editor are checked against it.
    pub fn document_get(&self, id: &str) -> Result<DocumentView> {
        self.write(|p| {
            let entry = p
                .docs
                .get_mut(id)
                .ok_or_else(|| MarginError::NotFound(format!("document {id}")))?;
            let text = fs::read_to_string(&entry.path)?;
            let doc = DocFile::parse(&text, &entry.path)?;
            let ast = dialect::parse(&doc.body);
            entry.revision = revision_of(text.as_bytes());
            entry.disk_seen = None;
            entry.word_count = dialect::word_count(&ast);
            if !doc.needs_rewrite {
                entry.fm = doc.fm;
            }
            Ok((
                DocumentView {
                    meta: doc_meta(p, id)?,
                    content: dialect::pm::to_pm(&ast),
                    revision: revision_of(text.as_bytes()),
                },
                Vec::new(),
            ))
        })
    }

    /// Plain body of a document as Markdown AST (for services).
    pub fn document_ast(&self, id: &str) -> Result<dialect::Document> {
        self.read(|p| {
            let entry = p
                .docs
                .get(id)
                .ok_or_else(|| MarginError::NotFound(format!("document {id}")))?;
            let text = fs::read_to_string(&entry.path)?;
            Ok(dialect::parse(&DocFile::parse(&text, &entry.path)?.body))
        })
    }

    // -----------------------------------------------------------------------
    // Commands
    // -----------------------------------------------------------------------

    /// Save editor content. Fails with `Conflict` if the file changed on disk
    /// since Margin last read or wrote it, unless `force` is set.
    pub fn document_save(&self, id: &str, content: &PmNode, force: bool) -> Result<SaveResult> {
        let doc = dialect::pm::from_pm(content);
        let body = dialect::serialize(&doc);
        let word_count = dialect::word_count(&doc);
        let result = self.write(|p| {
            let entry = p
                .docs
                .get_mut(id)
                .ok_or_else(|| MarginError::NotFound(format!("document {id}")))?;
            if !force
                && let Ok(disk) = fs::read(&entry.path)
                && revision_of(&disk) != entry.revision
            {
                return Err(MarginError::Conflict(format!(
                    "{} changed on disk",
                    entry.fm.title
                )));
            }
            let file = DocFile {
                fm: entry.fm.clone(),
                body,
                needs_rewrite: false,
            };
            let text = file.render()?;
            atomic_write(&entry.path, text.as_bytes())?;
            entry.revision = revision_of(text.as_bytes());
            entry.word_count = word_count;
            let rev = entry.revision.clone();
            let _ = fs::remove_file(recovery_path(&p.root, id));
            Ok((
                SaveResult {
                    revision: rev.clone(),
                    word_count,
                },
                vec![DomainEvent::DocSaved {
                    doc_id: id.to_string(),
                    revision: rev,
                    word_count,
                }],
            ))
        })?;
        Ok(result)
    }

    pub fn document_create(
        &self,
        kind: DocKind,
        title: &str,
        target: &BinderTarget,
    ) -> Result<DocMeta> {
        let title = if title.trim().is_empty() {
            default_title(kind)
        } else {
            title.trim().to_string()
        };
        self.write(|p| {
            if let Some(parent) = &target.parent
                && tree::locate(&p.manifest.binder, parent)
                    .is_none_or(|s| s.section != target.section)
            {
                return Err(MarginError::Invalid(format!(
                    "parent {parent} is not in {:?}",
                    target.section
                )));
            }
            let id = ulid::Ulid::generate().to_string();
            let mut fm = Frontmatter::new(id.clone(), kind, title.clone());
            if kind == DocKind::Scene {
                fm.status = Some("draft".into());
            }
            let path = p
                .root
                .join(target.section.folder())
                .join(file_name(&title, &id));
            let file = DocFile {
                fm: fm.clone(),
                body: String::new(),
                needs_rewrite: false,
            };
            let text = file.render()?;
            atomic_write(&path, text.as_bytes())?;
            p.docs.insert(
                id.clone(),
                DocEntry {
                    path,
                    fm,
                    word_count: 0,
                    revision: revision_of(text.as_bytes()),
                    disk_seen: None,
                },
            );
            let slot = Slot {
                section: target.section,
                parent: target.parent.clone(),
                index: target.index.map_or(usize::MAX, |i| i as usize),
            };
            tree::insert(&mut p.manifest.binder, &slot, BinderEntry::leaf(id.clone()));
            save_manifest(p)?;
            let meta = doc_meta(p, &id)?;
            Ok((
                meta,
                vec![DomainEvent::BinderChanged {
                    reason: "create".into(),
                    doc_ids: vec![id],
                }],
            ))
        })
    }

    pub fn document_update_meta(&self, id: &str, patch: &DocMetaPatch) -> Result<DocMeta> {
        self.write(|p| {
            let entry = p
                .docs
                .get_mut(id)
                .ok_or_else(|| MarginError::NotFound(format!("document {id}")))?;
            let disk = fs::read_to_string(&entry.path)?;
            let mut file = DocFile::parse(&disk, &entry.path)?;
            // The frontmatter in memory is authoritative for Margin-made changes;
            // the body comes from disk so an unsaved editor never loses text here.
            file.fm = entry.fm.clone();
            let fm = &mut file.fm;
            let mut renamed = false;
            if let Some(t) = &patch.title {
                let t = t.trim();
                if !t.is_empty() && t != fm.title {
                    fm.title = t.to_string();
                    renamed = true;
                }
            }
            if let Some(k) = patch.kind {
                fm.kind = Some(k);
            }
            let opt = |v: &Option<String>, field: &mut Option<String>| {
                if let Some(v) = v {
                    let v = v.trim();
                    *field = if v.is_empty() {
                        None
                    } else {
                        Some(v.to_string())
                    };
                }
            };
            opt(&patch.status, &mut fm.status);
            opt(&patch.pov, &mut fm.pov);
            opt(&patch.synopsis, &mut fm.synopsis);
            if let Some(l) = &patch.labels {
                fm.labels = l
                    .iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            if let Some(t) = patch.target {
                fm.target = if t == 0 { None } else { Some(t) };
            }
            if let Some(i) = patch.include {
                fm.include = i;
            }
            if let Some(a) = patch.ai {
                fm.ai = a;
            }
            if renamed {
                // Rename first, then rewrite: a crash in between leaves a
                // correctly identified file with a stale title only.
                let new_path = entry.path.with_file_name(file_name(&file.fm.title, id));
                if new_path != entry.path {
                    rename_with_retry(&entry.path, &new_path)?;
                    entry.path = new_path;
                }
            }
            let text = file.render()?;
            atomic_write(&entry.path, text.as_bytes())?;
            // If the disk body differed from what we knew, keep that as the
            // known revision only when it was ours.
            entry.fm = file.fm;
            entry.revision = revision_of(text.as_bytes());
            let meta = doc_meta(p, id)?;
            Ok((
                meta,
                vec![DomainEvent::BinderChanged {
                    reason: "meta".into(),
                    doc_ids: vec![id.to_string()],
                }],
            ))
        })
    }

    /// Move nodes (with their subtrees) to a target slot, keeping their
    /// relative order. Moving a node into its own subtree is rejected.
    pub fn binder_move(&self, ids: &[String], target: &BinderTarget) -> Result<()> {
        self.write(|p| {
            let ids = top_level_ids(p, ids);
            if ids.is_empty() {
                return Ok(((), vec![]));
            }
            if let Some(parent) = &target.parent {
                let ps = tree::locate(&p.manifest.binder, parent)
                    .ok_or_else(|| MarginError::NotFound(format!("parent {parent}")))?;
                if ps.section != target.section {
                    return Err(MarginError::Invalid("parent is in another section".into()));
                }
                for id in &ids {
                    let binder = &p.manifest.binder;
                    let inside_itself = Section::ALL
                        .iter()
                        .find_map(|&s| tree::find_entry(binder.section(s), id))
                        .is_some_and(|e| tree::contains(e, parent));
                    if inside_itself {
                        return Err(MarginError::Invalid(
                            "cannot move a node inside itself".into(),
                        ));
                    }
                }
            }
            let mut index = target.index.map_or(usize::MAX, |i| i as usize);
            let mut moved = Vec::new();
            for id in &ids {
                let Some((entry, from)) = tree::remove(&mut p.manifest.binder, id) else {
                    continue;
                };
                // Removing an earlier sibling shifts the target index.
                if from.section == target.section
                    && from.parent == target.parent
                    && from.index < index
                    && index != usize::MAX
                {
                    index -= 1;
                }
                moved.push((entry, from.section));
            }
            let mut changed_ids = Vec::new();
            for (k, (entry, from_section)) in moved.into_iter().enumerate() {
                let slot = Slot {
                    section: target.section,
                    parent: target.parent.clone(),
                    index: if index == usize::MAX {
                        usize::MAX
                    } else {
                        index + k
                    },
                };
                let subtree = entry.ids();
                tree::insert(&mut p.manifest.binder, &slot, entry);
                if from_section != target.section {
                    relocate(p, &subtree, target.section.folder())?;
                }
                changed_ids.extend(subtree);
            }
            save_manifest(p)?;
            Ok((
                (),
                vec![DomainEvent::BinderChanged {
                    reason: "move".into(),
                    doc_ids: changed_ids,
                }],
            ))
        })
    }

    /// Move nodes (with subtrees) to the Trash. Recoverable with [`Self::binder_restore`].
    pub fn binder_trash(&self, ids: &[String]) -> Result<()> {
        self.write(|p| {
            let ids = top_level_ids(p, ids);
            let mut changed = Vec::new();
            for id in ids {
                let Some((entry, slot)) = tree::remove(&mut p.manifest.binder, &id) else {
                    continue;
                };
                let subtree = entry.ids();
                relocate(p, &subtree, TRASH_FOLDER)?;
                p.manifest.binder.trash.insert(
                    0,
                    TrashEntry {
                        id: entry.id,
                        children: entry.children,
                        from: TrashOrigin {
                            section: slot.section,
                            parent: slot.parent,
                            index: slot.index as u32,
                        },
                        trashed_at: now_rfc3339(),
                    },
                );
                changed.extend(subtree);
            }
            save_manifest(p)?;
            Ok((
                (),
                vec![DomainEvent::BinderChanged {
                    reason: "trash".into(),
                    doc_ids: changed,
                }],
            ))
        })
    }

    /// Restore trashed nodes to where they were (or the section root if the
    /// old parent is gone).
    pub fn binder_restore(&self, ids: &[String]) -> Result<()> {
        self.write(|p| {
            let mut changed = Vec::new();
            // The trash list is newest-first; restoring in that order replays indices correctly.
            let mut to_restore: Vec<TrashEntry> = Vec::new();
            p.manifest.binder.trash.retain(|t| {
                if ids.contains(&t.id) {
                    to_restore.push(t.clone());
                    false
                } else {
                    true
                }
            });
            for t in to_restore {
                let parent_ok = t.from.parent.as_ref().is_none_or(|par| {
                    tree::locate(&p.manifest.binder, par)
                        .is_some_and(|s| s.section == t.from.section)
                });
                let slot = Slot {
                    section: t.from.section,
                    parent: if parent_ok {
                        t.from.parent.clone()
                    } else {
                        None
                    },
                    index: t.from.index as usize,
                };
                let entry = BinderEntry {
                    id: t.id,
                    children: t.children,
                };
                let subtree = entry.ids();
                tree::insert(&mut p.manifest.binder, &slot, entry);
                relocate(p, &subtree, t.from.section.folder())?;
                changed.extend(subtree);
            }
            save_manifest(p)?;
            Ok((
                (),
                vec![DomainEvent::BinderChanged {
                    reason: "restore".into(),
                    doc_ids: changed,
                }],
            ))
        })
    }

    // -----------------------------------------------------------------------
    // Recovery journal (SPEC §11.3)
    // -----------------------------------------------------------------------

    /// Record unsaved editor content. Called frequently while typing; the
    /// entry is removed by the next successful save.
    pub fn journal_write(&self, id: &str, content: &PmNode) -> Result<()> {
        self.read(|p| {
            let entry = p
                .docs
                .get(id)
                .ok_or_else(|| MarginError::NotFound(format!("document {id}")))?;
            let rec = JournalFile {
                schema_version: 1,
                doc_id: id.to_string(),
                title: entry.fm.title.clone(),
                saved_at: now_rfc3339(),
                base_revision: entry.revision.clone(),
                content: content.clone(),
            };
            atomic_write(
                &recovery_path(&p.root, id),
                serde_json::to_string(&rec)?.as_bytes(),
            )?;
            Ok(())
        })
    }

    /// Unsaved edits found from a previous session whose content differs
    /// from the file on disk.
    pub fn journal_list(&self) -> Result<Vec<RecoveryEntry>> {
        self.read(|p| {
            let dir = p.root.join(APP_DIR).join(RECOVERY_DIR);
            let mut out = Vec::new();
            let Ok(rd) = fs::read_dir(&dir) else {
                return Ok(out);
            };
            for e in rd.filter_map(|e| e.ok()) {
                let Ok(text) = fs::read_to_string(e.path()) else {
                    continue;
                };
                let Ok(rec) = serde_json::from_str::<JournalFile>(&text) else {
                    continue;
                };
                let Some(entry) = p.docs.get(&rec.doc_id) else {
                    continue;
                };
                let doc = dialect::pm::from_pm(&rec.content);
                let on_disk = fs::read_to_string(&entry.path)
                    .ok()
                    .and_then(|t| DocFile::parse(&t, &entry.path).ok())
                    .map(|d| dialect::normalize(&dialect::parse(&d.body)));
                if on_disk.as_ref() == Some(&dialect::normalize(&doc)) {
                    let _ = fs::remove_file(e.path());
                    continue;
                }
                out.push(RecoveryEntry {
                    doc_id: rec.doc_id,
                    title: entry.fm.title.clone(),
                    saved_at: rec.saved_at,
                    word_count: dialect::word_count(&doc),
                });
            }
            out.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
            Ok(out)
        })
    }

    pub fn journal_get(&self, id: &str) -> Result<PmNode> {
        self.read(|p| {
            let text = fs::read_to_string(recovery_path(&p.root, id))
                .map_err(|_| MarginError::NotFound(format!("recovery entry for {id}")))?;
            let rec: JournalFile = serde_json::from_str(&text)?;
            Ok(rec.content)
        })
    }

    pub fn journal_discard(&self, id: &str) -> Result<()> {
        self.read(|p| {
            let path = recovery_path(&p.root, id);
            if path.exists() {
                fs::remove_file(path)?;
            }
            Ok(())
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct JournalFile {
    schema_version: u32,
    doc_id: String,
    title: String,
    saved_at: String,
    base_revision: String,
    content: PmNode,
}

pub(super) fn recovery_path(root: &Path, id: &str) -> PathBuf {
    root.join(APP_DIR)
        .join(RECOVERY_DIR)
        .join(format!("{id}.json"))
}

fn default_title(kind: DocKind) -> String {
    match kind {
        DocKind::Part => "New Part",
        DocKind::Chapter => "New Chapter",
        DocKind::Scene => "New Scene",
        DocKind::Folder => "New Folder",
        DocKind::Note => "New Note",
    }
    .to_string()
}

pub(super) fn save_manifest(p: &OpenProject) -> Result<()> {
    atomic_write(&p.root.join(PROJECT_FILE), p.manifest.to_yaml()?.as_bytes())?;
    Ok(())
}

/// Drop ids that are descendants of other ids in the list (they move with
/// their ancestor), keeping binder order.
pub(super) fn top_level_ids(p: &OpenProject, ids: &[String]) -> Vec<String> {
    let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    fn walk(entries: &[BinderEntry], wanted: &HashSet<&str>, out: &mut Vec<String>) {
        for e in entries {
            if wanted.contains(e.id.as_str()) {
                out.push(e.id.clone());
            } else {
                walk(&e.children, wanted, out);
            }
        }
    }
    for s in Section::ALL {
        walk(p.manifest.binder.section(s), &wanted, &mut out);
    }
    out
}

/// Move the files of `ids` into `folder` (relative to the project root).
fn relocate(p: &mut OpenProject, ids: &[String], folder: &str) -> Result<()> {
    let dir = p.root.join(folder);
    fs::create_dir_all(&dir)?;
    for id in ids {
        if let Some(entry) = p.docs.get_mut(id) {
            let Some(name) = entry.path.file_name() else {
                continue;
            };
            let new_path = dir.join(name);
            if new_path != entry.path {
                rename_with_retry(&entry.path, &new_path)?;
                entry.path = new_path;
            }
        }
    }
    Ok(())
}

pub(super) fn project_info(p: &OpenProject) -> ProjectInfo {
    let m = &p.manifest;
    ProjectInfo {
        id: m.id.clone(),
        root: p.root.to_string_lossy().to_string(),
        title: m.title.clone(),
        author: m.author.clone(),
        project_type: m.project_type,
        locale: m.locale,
        statuses: m.statuses.clone(),
        labels: m.labels.clone(),
        goals: m.goals.clone(),
        problems: p.problems.clone(),
    }
}

pub(super) fn doc_meta(p: &OpenProject, id: &str) -> Result<DocMeta> {
    let e = p
        .docs
        .get(id)
        .ok_or_else(|| MarginError::NotFound(format!("document {id}")))?;
    let section = tree::locate(&p.manifest.binder, id).map(|s| s.section);
    Ok(DocMeta {
        id: id.to_string(),
        kind: e.fm.kind(),
        title: e.fm.title.clone(),
        status: e.fm.status.clone(),
        pov: e.fm.pov.clone(),
        labels: e.fm.labels.clone(),
        target: e.fm.target,
        synopsis: e.fm.synopsis.clone(),
        include: e.fm.include,
        ai: e.fm.ai,
        section,
        word_count: e.word_count,
        file: rel_path(&p.root, &e.path),
    })
}

fn view_nodes(p: &OpenProject, entries: &[BinderEntry]) -> Vec<BinderNode> {
    let mut out = Vec::new();
    for e in entries {
        let children = view_nodes(p, &e.children);
        match p.docs.get(&e.id) {
            Some(d) => {
                let total = d.word_count + children.iter().map(|c| c.total_word_count).sum::<u32>();
                out.push(BinderNode {
                    id: e.id.clone(),
                    kind: d.fm.kind(),
                    title: d.fm.title.clone(),
                    status: d.fm.status.clone(),
                    labels: d.fm.labels.clone(),
                    target: d.fm.target,
                    include: d.fm.include,
                    ai: d.fm.ai,
                    has_synopsis: d.fm.synopsis.as_ref().is_some_and(|s| !s.is_empty()),
                    word_count: d.word_count,
                    total_word_count: total,
                    children,
                });
            }
            // Unreadable file: hide the node but keep its children visible.
            None => out.extend(children),
        }
    }
    out
}
