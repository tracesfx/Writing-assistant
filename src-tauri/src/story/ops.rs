//! Story Data Service commands beyond the basics: duplicate, split, merge,
//! project settings, quick capture, comments, the scratchpad, and changes
//! made to the project folder outside Margin.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::service::{
    DocEntry, OpenProject, StoryService, doc_meta, now_rfc3339, project_info, rel_path,
    revision_of, save_manifest, top_level_ids,
};
use super::tree::{self, Slot};
use super::types::*;
use crate::dialect::{self, Block, Document, Inline, pm::PmNode};
use crate::error::{MarginError, Result};
use crate::events::DomainEvent;
use crate::project::atomic::{TEMP_SUFFIX, atomic_write};
use crate::project::document::{DocFile, DocKind, Frontmatter, file_name};
use crate::project::manifest::{BinderEntry, Section, TRASH_FOLDER};

pub const COMMENTS_DIR: &str = "comments";
pub const SCRATCHPAD_FILE: &str = "scratchpad.md";
const COMMENTS_SCHEMA_VERSION: u32 = 1;

fn not_found(id: &str) -> MarginError {
    MarginError::NotFound(format!("document {id}"))
}

/// Replace a document's body, keeping its frontmatter. Returns the new
/// revision and word count. Does not check for conflicts: callers flush
/// editors first and announce the change with `DocContentReplaced`.
pub(super) fn write_body(p: &mut OpenProject, id: &str, doc: &Document) -> Result<(String, u32)> {
    let entry = p.docs.get_mut(id).ok_or_else(|| not_found(id))?;
    let file = DocFile {
        fm: entry.fm.clone(),
        body: dialect::serialize(doc),
        needs_rewrite: false,
    };
    let text = file.render()?;
    atomic_write(&entry.path, text.as_bytes())?;
    entry.revision = revision_of(text.as_bytes());
    entry.disk_seen = None;
    entry.word_count = dialect::word_count(doc);
    Ok((entry.revision.clone(), entry.word_count))
}

pub(super) fn read_body(entry: &DocEntry) -> Result<Document> {
    let text = fs::read_to_string(&entry.path)?;
    Ok(dialect::parse(&DocFile::parse(&text, &entry.path)?.body))
}

/// Ids of a section's documents in binder order (pre-order).
pub(super) fn section_ids(p: &OpenProject, s: Section) -> Vec<String> {
    p.manifest
        .binder
        .section(s)
        .iter()
        .flat_map(|e| e.ids())
        .collect()
}

fn new_entry(
    p: &mut OpenProject,
    folder: &str,
    fm: Frontmatter,
    body: &Document,
) -> Result<String> {
    let id = fm.id.clone();
    let path = p.root.join(folder).join(file_name(&fm.title, &id));
    let file = DocFile {
        fm: fm.clone(),
        body: dialect::serialize(body),
        needs_rewrite: false,
    };
    let text = file.render()?;
    atomic_write(&path, text.as_bytes())?;
    p.docs.insert(
        id.clone(),
        DocEntry {
            path,
            fm,
            word_count: dialect::word_count(body),
            revision: revision_of(text.as_bytes()),
            disk_seen: None,
        },
    );
    Ok(id)
}

impl StoryService {
    // -----------------------------------------------------------------------
    // Queries for other services
    // -----------------------------------------------------------------------

    /// Ids of documents in a section, in binder order.
    pub fn section_doc_ids(&self, s: Section) -> Result<Vec<String>> {
        self.read(|p| Ok(section_ids(p, s)))
    }

    /// The document and all its descendants, in binder order.
    pub fn subtree_ids(&self, id: &str) -> Result<Vec<String>> {
        self.read(|p| {
            Section::ALL
                .iter()
                .find_map(|&s| tree::find_entry(p.manifest.binder.section(s), id))
                .map(|e| e.ids())
                .ok_or_else(|| not_found(id))
        })
    }

    /// Absolute paths of every document, by id (including the Trash).
    pub fn doc_paths(&self) -> Result<Vec<(String, PathBuf)>> {
        self.read(|p| {
            Ok(p.docs
                .iter()
                .map(|(id, e)| (id.clone(), e.path.clone()))
                .collect())
        })
    }

    /// Replace document bodies (history restore, replace-all). Emits
    /// `doc.contentReplaced` so open editors reload.
    pub fn replace_bodies(&self, docs: &[(String, Document)], reason: &str) -> Result<()> {
        self.write(|p| {
            let mut ids = Vec::new();
            let mut events = Vec::new();
            for (id, doc) in docs {
                let (revision, word_count) = write_body(p, id, doc)?;
                events.push(DomainEvent::DocSaved {
                    doc_id: id.clone(),
                    revision,
                    word_count,
                });
                ids.push(id.clone());
            }
            events.push(DomainEvent::DocContentReplaced {
                doc_ids: ids,
                reason: reason.to_string(),
            });
            Ok(((), events))
        })
    }

    // -----------------------------------------------------------------------
    // Binder commands
    // -----------------------------------------------------------------------

    /// Copy nodes (with their subtrees) to just after the originals. The top
    /// copies get " (copy)" appended to their titles. Returns the new top ids.
    pub fn binder_duplicate(&self, ids: &[String]) -> Result<Vec<String>> {
        self.write(|p| {
            let tops = top_level_ids(p, ids);
            let mut created = Vec::new();
            let mut changed = Vec::new();
            for id in tops {
                let Some(slot) = tree::locate(&p.manifest.binder, &id) else {
                    continue;
                };
                let Some(entry) = Section::ALL
                    .iter()
                    .find_map(|&s| tree::find_entry(p.manifest.binder.section(s), &id))
                    .cloned()
                else {
                    continue;
                };
                let folder = slot.section.folder();
                let copy = copy_subtree(p, &entry, folder, true, &mut changed)?;
                let new_id = copy.id.clone();
                tree::insert(
                    &mut p.manifest.binder,
                    &Slot {
                        index: slot.index + 1,
                        ..slot
                    },
                    copy,
                );
                created.push(new_id);
            }
            save_manifest(p)?;
            Ok((
                created,
                vec![DomainEvent::BinderChanged {
                    reason: "duplicate".into(),
                    doc_ids: changed,
                }],
            ))
        })
    }

    /// Split a document at the cursor: `before` stays in the document,
    /// `after` becomes a new document right after it (same kind, POV,
    /// status and labels). Returns the new document.
    pub fn document_split(
        &self,
        id: &str,
        before: &PmNode,
        after: &PmNode,
        title: &str,
    ) -> Result<DocMeta> {
        self.write(|p| {
            let slot = tree::locate(&p.manifest.binder, id)
                .ok_or_else(|| MarginError::Invalid("only binder documents can be split".into()))?;
            let src = p.docs.get(id).ok_or_else(|| not_found(id))?.fm.clone();
            let (revision, word_count) = write_body(p, id, &dialect::pm::from_pm(before))?;
            let new_id = ulid::Ulid::generate().to_string();
            let title = if title.trim().is_empty() {
                format!("{} (2)", src.title)
            } else {
                title.trim().to_string()
            };
            let mut fm = Frontmatter::new(new_id.clone(), src.kind(), title);
            fm.status = src.status.clone();
            fm.pov = src.pov.clone();
            fm.labels = src.labels.clone();
            fm.include = src.include;
            fm.ai = src.ai;
            let after_doc = dialect::pm::from_pm(after);
            new_entry(p, slot.section.folder(), fm, &after_doc)?;
            // Comments anchored in the moved text follow it.
            move_comments(&p.root, id, &new_id, &anchored_comments(&after_doc))?;
            tree::insert(
                &mut p.manifest.binder,
                &Slot {
                    section: slot.section,
                    parent: slot.parent.clone(),
                    index: slot.index + 1,
                },
                BinderEntry::leaf(new_id.clone()),
            );
            save_manifest(p)?;
            let meta = doc_meta(p, &new_id)?;
            Ok((
                meta,
                // The binder change goes first: moving text between
                // documents is not writing, so the stats service takes
                // fresh counts before it sees the shortened source.
                vec![
                    DomainEvent::BinderChanged {
                        reason: "split".into(),
                        doc_ids: vec![id.to_string(), new_id],
                    },
                    DomainEvent::DocSaved {
                        doc_id: id.to_string(),
                        revision,
                        word_count,
                    },
                    DomainEvent::DocContentReplaced {
                        doc_ids: vec![id.to_string()],
                        reason: "split".into(),
                    },
                ],
            ))
        })
    }

    /// Merge documents into the first one (in binder order) and move the
    /// rest to the Trash. Documents with children cannot be merged.
    pub fn document_merge(&self, ids: &[String]) -> Result<MergeResult> {
        let result = self.write(|p| {
            let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
            let ordered: Vec<String> = Section::ALL
                .iter()
                .flat_map(|&s| section_ids(p, s))
                .filter(|id| wanted.contains(id.as_str()))
                .collect();
            if ordered.len() < 2 {
                return Err(MarginError::Invalid(
                    "select at least two documents to merge".into(),
                ));
            }
            for id in &ordered {
                let has_children = Section::ALL
                    .iter()
                    .find_map(|&s| tree::find_entry(p.manifest.binder.section(s), id))
                    .is_some_and(|e| !e.children.is_empty());
                if has_children {
                    let title = p.docs.get(id).map_or("A document", |e| e.fm.title.as_str());
                    return Err(MarginError::Invalid(format!(
                        "{title} contains other documents and cannot be merged"
                    )));
                }
            }
            let mut blocks: Vec<Block> = Vec::new();
            for id in &ordered {
                let entry = p.docs.get(id).ok_or_else(|| not_found(id))?;
                blocks.extend(read_body(entry)?.blocks);
            }
            let target = ordered[0].clone();
            let merged = Document { blocks };
            let anchored = anchored_comments(&merged);
            let (revision, word_count) = write_body(p, &target, &merged)?;
            for id in &ordered[1..] {
                move_comments(&p.root, id, &target, &anchored)?;
            }
            Ok((
                (
                    MergeResult {
                        target: target.clone(),
                        trashed: ordered[1..].to_vec(),
                    },
                    revision,
                    word_count,
                ),
                Vec::new(),
            ))
        })?;
        let (merge, revision, word_count) = result;
        // Trash first (a binder change) so the stats service re-baselines
        // before the merged target's save: merging is not writing.
        self.binder_trash(&merge.trashed)?;
        self.bus().publish_all([
            DomainEvent::DocSaved {
                doc_id: merge.target.clone(),
                revision,
                word_count,
            },
            DomainEvent::DocContentReplaced {
                doc_ids: vec![merge.target.clone()],
                reason: "merge".into(),
            },
        ]);
        Ok(merge)
    }

    // -----------------------------------------------------------------------
    // Project settings
    // -----------------------------------------------------------------------

    pub fn project_update(&self, patch: &ProjectPatch) -> Result<ProjectInfo> {
        self.write(|p| {
            let m = &mut p.manifest;
            if let Some(t) = &patch.title
                && !t.trim().is_empty()
            {
                m.title = t.trim().to_string();
            }
            if let Some(a) = &patch.author {
                m.author = a.trim().to_string();
            }
            if let Some(t) = patch.project_type {
                m.project_type = t;
            }
            if let Some(l) = patch.locale {
                m.locale = l;
            }
            if let Some(g) = &patch.goals {
                m.goals = g.clone();
            }
            if let Some(s) = &patch.statuses {
                let s: Vec<String> = s
                    .iter()
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect();
                if !s.is_empty() {
                    m.statuses = s;
                }
            }
            if let Some(l) = &patch.labels {
                m.labels = l.clone();
            }
            save_manifest(p)?;
            let info = project_info(p);
            Ok((
                info.clone(),
                vec![DomainEvent::ProjectSettingsChanged {
                    project_id: info.id,
                }],
            ))
        })
    }

    // -----------------------------------------------------------------------
    // Quick capture, comments, scratchpad
    // -----------------------------------------------------------------------

    /// Append a paragraph to the Inbox note (created if missing). Returns the
    /// Inbox id.
    pub fn capture(&self, text: &str) -> Result<String> {
        let text = text.trim();
        if text.is_empty() {
            return Err(MarginError::Invalid("nothing to capture".into()));
        }
        self.write(|p| {
            let inbox = section_ids(p, Section::Notes).into_iter().find(|id| {
                p.docs
                    .get(id)
                    .is_some_and(|e| e.fm.title.eq_ignore_ascii_case("inbox"))
            });
            let mut events = Vec::new();
            let id = match inbox {
                Some(id) => id,
                None => {
                    let id = ulid::Ulid::generate().to_string();
                    let fm = Frontmatter::new(id.clone(), DocKind::Note, "Inbox".into());
                    new_entry(p, Section::Notes.folder(), fm, &Document::default())?;
                    p.manifest
                        .binder
                        .notes
                        .insert(0, BinderEntry::leaf(id.clone()));
                    save_manifest(p)?;
                    events.push(DomainEvent::BinderChanged {
                        reason: "create".into(),
                        doc_ids: vec![id.clone()],
                    });
                    id
                }
            };
            let entry = p.docs.get(&id).ok_or_else(|| not_found(&id))?;
            let mut doc = read_body(entry)?;
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                doc.blocks.push(Block::Paragraph(vec![Inline::text(
                    line.trim(),
                    Vec::new(),
                )]));
            }
            let (revision, word_count) = write_body(p, &id, &doc)?;
            events.push(DomainEvent::DocSaved {
                doc_id: id.clone(),
                revision,
                word_count,
            });
            events.push(DomainEvent::DocContentReplaced {
                doc_ids: vec![id.clone()],
                reason: "capture".into(),
            });
            Ok((id, events))
        })
    }

    pub fn comments_list(&self, doc_id: &str) -> Result<Vec<Comment>> {
        self.read(|p| Ok(read_comments(&p.root, doc_id)?.comments))
    }

    /// Create or update a comment (matched by id).
    pub fn comments_put(&self, doc_id: &str, comment: &Comment) -> Result<Vec<Comment>> {
        self.read(|p| {
            if !p.docs.contains_key(doc_id) {
                return Err(not_found(doc_id));
            }
            let mut file = read_comments(&p.root, doc_id)?;
            match file.comments.iter_mut().find(|c| c.id == comment.id) {
                Some(c) => {
                    c.body = comment.body.clone();
                    c.resolved = comment.resolved;
                    c.updated_at = now_rfc3339();
                }
                None => {
                    let mut c = comment.clone();
                    c.created_at = now_rfc3339();
                    c.updated_at = c.created_at.clone();
                    file.comments.push(c);
                }
            }
            write_comments(&p.root, doc_id, &file)?;
            Ok(file.comments)
        })
    }

    pub fn comments_delete(&self, doc_id: &str, comment_id: &str) -> Result<Vec<Comment>> {
        self.read(|p| {
            let mut file = read_comments(&p.root, doc_id)?;
            file.comments.retain(|c| c.id != comment_id);
            write_comments(&p.root, doc_id, &file)?;
            Ok(file.comments)
        })
    }

    pub fn scratchpad_get(&self) -> Result<String> {
        self.read(|p| match fs::read_to_string(p.root.join(SCRATCHPAD_FILE)) {
            Ok(t) => Ok(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e.into()),
        })
    }

    pub fn scratchpad_put(&self, text: &str) -> Result<()> {
        self.read(|p| {
            Ok(atomic_write(
                &p.root.join(SCRATCHPAD_FILE),
                text.as_bytes(),
            )?)
        })
    }

    // -----------------------------------------------------------------------
    // Outside changes (file watcher)
    // -----------------------------------------------------------------------

    /// React to changed paths reported by the file watcher. Margin's own
    /// writes are recognised by their content hash and ignored.
    pub fn on_fs_change(&self, paths: &[PathBuf]) -> Result<()> {
        let need_rescan = self.write(|p| {
            let mut rescan = false;
            let mut events = Vec::new();
            for path in paths {
                if !is_doc_path(&p.root, path) {
                    continue;
                }
                let known = p.docs.iter_mut().find(|(_, e)| e.path == *path);
                match known {
                    Some((id, entry)) => {
                        let Ok(bytes) = fs::read(path) else {
                            rescan = true; // deleted or renamed
                            continue;
                        };
                        let rev = revision_of(&bytes);
                        if rev == entry.revision || entry.disk_seen.as_deref() == Some(rev.as_str())
                        {
                            continue;
                        }
                        let text = String::from_utf8_lossy(&bytes);
                        if let Ok(doc) = DocFile::parse(&text, path) {
                            if doc.fm.id != *id {
                                rescan = true;
                                continue;
                            }
                            entry.word_count = dialect::word_count(&dialect::parse(&doc.body));
                            if !doc.needs_rewrite {
                                entry.fm = doc.fm;
                            }
                        }
                        entry.disk_seen = Some(rev);
                        events.push(DomainEvent::DocChangedOnDisk { doc_id: id.clone() });
                        events.push(DomainEvent::BinderChanged {
                            reason: "external".into(),
                            doc_ids: vec![id.clone()],
                        });
                    }
                    None => {
                        if path.exists() {
                            rescan = true;
                        }
                    }
                }
            }
            Ok((rescan, events))
        })?;
        if need_rescan {
            self.rescan()?;
        }
        Ok(())
    }

    /// Project-relative path of a document (for history lookups).
    pub fn doc_rel_path(&self, id: &str) -> Result<String> {
        self.read(|p| {
            let e = p.docs.get(id).ok_or_else(|| not_found(id))?;
            Ok(rel_path(&p.root, &e.path))
        })
    }
}

/// A Markdown file directly inside a section folder or the Trash.
fn is_doc_path(root: &Path, path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('.')
        || name.ends_with(TEMP_SUFFIX)
        || !name.to_ascii_lowercase().ends_with(".md")
    {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    Section::ALL
        .iter()
        .map(|s| root.join(s.folder()))
        .chain(std::iter::once(root.join(TRASH_FOLDER)))
        .any(|d| d == parent)
}

fn copy_subtree(
    p: &mut OpenProject,
    entry: &BinderEntry,
    folder: &str,
    top: bool,
    changed: &mut Vec<String>,
) -> Result<BinderEntry> {
    let src = p.docs.get(&entry.id).ok_or_else(|| not_found(&entry.id))?;
    let body = read_body(src)?;
    let mut fm = src.fm.clone();
    fm.id = ulid::Ulid::generate().to_string();
    if top {
        fm.title = format!("{} (copy)", fm.title);
    }
    let id = new_entry(p, folder, fm, &body)?;
    let comments = read_comments(&p.root, &entry.id)?;
    if !comments.comments.is_empty() {
        write_comments(&p.root, &id, &comments)?;
    }
    changed.push(id.clone());
    let mut children = Vec::new();
    for c in &entry.children {
        children.push(copy_subtree(p, c, folder, false, changed)?);
    }
    Ok(BinderEntry { id, children })
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentsFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    comments: Vec<Comment>,
}

fn comments_path(root: &Path, doc_id: &str) -> PathBuf {
    root.join(COMMENTS_DIR).join(format!("{doc_id}.json"))
}

fn read_comments(root: &Path, doc_id: &str) -> Result<CommentsFile> {
    match fs::read_to_string(comments_path(root, doc_id)) {
        Ok(t) => {
            let f: CommentsFile = serde_json::from_str(&t)?;
            if f.schema_version > COMMENTS_SCHEMA_VERSION {
                return Err(MarginError::Invalid(format!(
                    "comments for {doc_id} use a newer format"
                )));
            }
            Ok(f)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(CommentsFile {
            schema_version: COMMENTS_SCHEMA_VERSION,
            comments: Vec::new(),
        }),
        Err(e) => Err(e.into()),
    }
}

/// Ids of the comments anchored in a document body.
fn anchored_comments(doc: &Document) -> HashSet<String> {
    const OPEN: &str = "<!-- c:";
    let md = dialect::serialize(doc);
    md.match_indices(OPEN)
        .filter_map(|(i, _)| {
            let rest = &md[i + OPEN.len()..];
            rest.find(" -->").map(|end| rest[..end].to_string())
        })
        .collect()
}

/// Move the comment records of `from` whose anchors are in `anchored` to
/// `to`. The destination is written first, so a crash in between leaves a
/// duplicate rather than losing a comment.
fn move_comments(root: &Path, from: &str, to: &str, anchored: &HashSet<String>) -> Result<()> {
    if from == to || anchored.is_empty() {
        return Ok(());
    }
    let src = read_comments(root, from)?;
    let (moving, keep): (Vec<Comment>, Vec<Comment>) = src
        .comments
        .into_iter()
        .partition(|c| anchored.contains(&c.id));
    if moving.is_empty() {
        return Ok(());
    }
    let mut dst = read_comments(root, to)?;
    for c in moving {
        if !dst.comments.iter().any(|d| d.id == c.id) {
            dst.comments.push(c);
        }
    }
    write_comments(root, to, &dst)?;
    write_comments(
        root,
        from,
        &CommentsFile {
            schema_version: COMMENTS_SCHEMA_VERSION,
            comments: keep,
        },
    )
}

fn write_comments(root: &Path, doc_id: &str, f: &CommentsFile) -> Result<()> {
    let f = CommentsFile {
        schema_version: COMMENTS_SCHEMA_VERSION,
        comments: f.comments.clone(),
    };
    atomic_write(
        &comments_path(root, doc_id),
        serde_json::to_string_pretty(&f)?.as_bytes(),
    )?;
    Ok(())
}
