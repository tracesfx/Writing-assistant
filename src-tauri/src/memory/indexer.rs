//! The indexing pipeline (SPEC §9.4): idle-only, one scene at a time.
//!
//! A scene is dirty when the hash of its paragraph text differs from the hash
//! on its scene card. It is read after it has been unchanged for the settle
//! time (60 s by default) and the author is idle (the scheduler holds P2 jobs
//! while there are keystrokes). Scenes nearest the one being edited go first.
//! Typing never waits for any of this.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::extract::{self, Extraction, SceneInfo};
use super::merge::{self, MergeOptions};
use super::search::Embedder;
use super::store::Store;
use super::types::*;
use super::{DocData, MemoryService, lock};
use crate::ai::scheduler::JobSpec;
use crate::ai::types::{Outcome, Priority, Prompt, PromptSection, Role};
use crate::error::{MarginError, Result};
use crate::project::manifest::ProjectType;

/// Wait before trying a failed scene again.
const RETRY_FAILED: Duration = Duration::from_secs(60);
const RETRY_UNREADABLE: Duration = Duration::from_secs(300);
const RETRY_CANCELLED: Duration = Duration::from_secs(10);
const EMBED_BATCH: usize = 16;

impl MemoryService {
    /// One heartbeat: refresh the embedding server's state, embed new
    /// paragraphs, then read at most one dirty scene.
    pub(crate) async fn tick(self: &Arc<Self>) {
        let Ok(_turn) = self.tick_lock.try_lock() else {
            return;
        };
        if lock(&self.inner).store.is_none() {
            return;
        }
        self.check_embedder().await;
        if self.embedder().is_some()
            && let Err(e) = self.embed_pending().await
        {
            lock(&self.inner).last_error = Some(format!("embeddings: {e}"));
        }
        let cfg = self.config();
        if !cfg.indexing || !self.ai.is_ready() {
            self.publish_progress();
            return;
        }
        let settle = Duration::from_secs(cfg.settle_seconds as u64);
        match self.next_dirty(settle) {
            Some(doc) => {
                if let Err(e) = self.index_scene_inner(&doc, Priority::P2).await {
                    lock(&self.inner).last_error = Some(e.to_string());
                }
            }
            None => {
                if let Err(e) = self.summarize_next().await {
                    lock(&self.inner).last_error = Some(e.to_string());
                }
            }
        }
        self.publish_progress();
    }

    /// The dirty scene to read next: settled, not backing off, nearest the focus.
    fn next_dirty(&self, settle: Duration) -> Option<String> {
        let i = lock(&self.inner);
        let now = Instant::now();
        let focus = i
            .focus
            .as_ref()
            .and_then(|f| i.layout.order.iter().position(|d| d == f))
            .unwrap_or(0) as i64;
        Self::pending_docs(&i, &self.wanted_extractors())
            .into_iter()
            .filter(|d| i.changed_at.get(d).is_none_or(|t| t.elapsed() >= settle))
            .filter(|d| i.retry_after.get(d).is_none_or(|t| now >= *t))
            .min_by_key(|d| {
                let pos = i.layout.order.iter().position(|x| x == d).unwrap_or(0) as i64;
                ((pos - focus).abs(), pos)
            })
    }

    async fn check_embedder(&self) {
        let cfg = self.config();
        let was = self.embed_ready.load(Ordering::Relaxed);
        let now = if cfg.embedding_enabled {
            match super::search::LlamaEmbedder::new(
                &format!("http://127.0.0.1:{}", cfg.embedding_port),
                &cfg.embedding_model,
            ) {
                Ok(e) => e.healthy().await,
                Err(_) => false,
            }
        } else {
            false
        };
        self.embed_ready.store(now, Ordering::Relaxed);
        if was != now {
            self.publish_progress();
        }
    }

    /// Embed paragraphs that have no vector yet (SPEC §9.4 step 3.1).
    pub(crate) async fn embed_pending(&self) -> Result<()> {
        let Some(embedder) = self.embedder() else {
            return Ok(());
        };
        let model = std::path::Path::new(&self.config().embedding_model)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let todo: Vec<(String, String, String)> = {
            let mut i = lock(&self.inner);
            let live: std::collections::HashSet<String> = Self::all_paras(&i)
                .iter()
                .map(|(_, _, p)| p.id.clone())
                .collect();
            let Some(v) = i.vectors.as_mut() else {
                return Ok(());
            };
            v.use_model(&model);
            v.retain(&live);
            Self::all_paras(&i)
                .iter()
                .filter(|(_, _, p)| !i.vectors.as_ref().is_some_and(|v| v.has(&p.id, &p.hash)))
                .map(|(_, _, p)| (p.id.clone(), p.hash.clone(), p.text.clone()))
                .take(EMBED_BATCH * 4)
                .collect()
        };
        for chunk in todo.chunks(EMBED_BATCH) {
            let texts: Vec<String> = chunk
                .iter()
                .map(|(_, _, t)| t.chars().take(2000).collect())
                .collect();
            let vecs = embedder.embed(texts).await?;
            let mut i = lock(&self.inner);
            if let Some(v) = i.vectors.as_mut() {
                for ((id, hash, _), vec) in chunk.iter().zip(vecs) {
                    v.put(id, hash, vec);
                }
            }
        }
        if !todo.is_empty() {
            let mut i = lock(&self.inner);
            if let Some(v) = i.vectors.as_mut() {
                v.flush()?;
            }
        }
        Ok(())
    }

    /// Re-read one scene now (a command, so it runs as a request, not idle work).
    pub async fn reindex_scene(self: &Arc<Self>, doc: &str) -> Result<()> {
        let _turn = self.tick_lock.lock().await;
        {
            let mut i = lock(&self.inner);
            i.retry_after.remove(doc);
            i.changed_at.remove(doc);
            if !i.docs.contains_key(doc) {
                return Err(MarginError::NotFound(format!("scene {doc}")));
            }
        }
        self.index_scene_inner(doc, Priority::P1).await
    }

    /// Forget everything the model read and read the whole book again. Canon
    /// stays; the model's records are regenerated.
    pub fn rebuild_all(&self) -> Result<u32> {
        let docs: Vec<String> = {
            let i = lock(&self.inner);
            i.docs.keys().cloned().collect()
        };
        let all = self.read_store(|_, i| {
            Ok(i.docs
                .values()
                .map(|d| d.lower.clone())
                .collect::<Vec<_>>()
                .join("\n"))
        })?;
        let _ = all;
        self.with_store(|s| {
            for d in &docs {
                s.ai_forget_scene(d)?;
            }
            // Inferred entries lose their support with their facts; they come back
            // when the scenes are read again.
            s.prune_orphans(&|_| false)?;
            s.synopsis = None;
            s.save_synopsis()?;
            let ids: Vec<String> = s.chapters.keys().cloned().collect();
            s.chapters.clear();
            for id in ids {
                s.save_chapter(&id)?;
            }
            Ok(())
        })?;
        {
            let mut i = lock(&self.inner);
            i.retry_after.clear();
            i.changed_at.clear();
            i.last_error = None;
        }
        self.emit(
            vec![],
            docs.clone(),
            &[
                "entities",
                "facts",
                "relations",
                "events",
                "scenes",
                "summaries",
            ],
        );
        Ok(docs.len() as u32)
    }

    /// Read one scene: build the prompt, ask the model, check and merge the answer.
    pub(crate) async fn index_scene_inner(
        self: &Arc<Self>,
        doc: &str,
        priority: Priority,
    ) -> Result<()> {
        let project = self.story.project_info()?;
        let meta = self.story.document_meta(doc)?;
        let want = extract::enabled(self.config().extract_tension);
        let (data, known, generation): (DocData, Vec<(String, String)>, u64) = {
            let mut i = lock(&self.inner);
            let data = i
                .docs
                .get(doc)
                .cloned()
                .ok_or_else(|| MarginError::NotFound(format!("scene {doc}")))?;
            let store = i.store.as_ref().ok_or(MarginError::NoProject)?;
            // Entries named in this scene first, then the rest, so the list stays short.
            let mut ents: Vec<(u32, &Entity)> = store
                .entities
                .values()
                .filter(|e| e.kind != "thread")
                .map(|e| {
                    (
                        super::mentions::count_in_lower(&Store::names_of(e), &data.lower),
                        e,
                    )
                })
                .collect();
            ents.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
            let known = ents
                .into_iter()
                .take(60)
                .map(|(_, e)| (e.name.clone(), e.kind.clone()))
                .collect();
            i.busy = true;
            i.current = Some(doc.to_string());
            (data, known, i.generation)
        };
        self.publish_progress();
        let result = self
            .read_scene(
                doc,
                &data,
                &known,
                &meta.title,
                meta.pov.as_deref(),
                project.project_type,
                &project.title,
                priority,
                &want,
            )
            .await;
        let finish = |me: &Self| {
            let mut i = lock(&me.inner);
            i.busy = false;
            i.current = None;
        };
        let exts = match result {
            Ok(Some(e)) => e,
            Ok(None) => {
                // Cancelled (typing, the kill switch): try again later.
                lock(&self.inner)
                    .retry_after
                    .insert(doc.to_string(), Instant::now() + RETRY_CANCELLED);
                finish(self);
                return Ok(());
            }
            Err((e, wait)) => {
                lock(&self.inner)
                    .retry_after
                    .insert(doc.to_string(), Instant::now() + wait);
                finish(self);
                return Err(e);
            }
        };

        // Apply under the lock, unless the project or the text moved on meanwhile.
        let changes = {
            let mut i = lock(&self.inner);
            let stale = i.generation != generation
                || i.docs
                    .get(doc)
                    .is_none_or(|d| d.text_hash != data.text_hash);
            if stale {
                i.busy = false;
                i.current = None;
                None
            } else {
                let all: String = Self::memory_text(&i);
                let opts = MergeOptions {
                    extractors: want.iter().map(|x| x.id.to_string()).collect(),
                    flag_conflicts: self.config().flag_conflicts,
                    project_type_fiction: project.project_type != ProjectType::NonFiction,
                };
                let store = i.store.as_mut().ok_or(MarginError::NoProject)?;
                let r = merge::apply_scene(
                    store,
                    doc,
                    &data.paras,
                    &data.text_hash,
                    &exts,
                    &opts,
                    &|| all.clone(),
                );
                i.version += 1;
                i.busy = false;
                i.current = None;
                i.last_error = None;
                Some(r?)
            }
        };
        if let Some(c) = changes {
            let kinds: Vec<&str> = c.kinds.iter().map(String::as_str).collect();
            let mut ids = c.entity_ids.clone();
            ids.extend(c.removed_entity_ids.clone());
            self.emit(ids, vec![doc.to_string()], &kinds);
        } else {
            self.publish_progress();
        }
        Ok(())
    }

    /// Ask the model about each window of the scene. `Ok(None)` = cancelled.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    async fn read_scene(
        &self,
        doc: &str,
        data: &DocData,
        known: &[(String, String)],
        title: &str,
        pov: Option<&str>,
        pt: ProjectType,
        project_title: &str,
        priority: Priority,
        extractors: &[&'static extract::Extractor],
    ) -> std::result::Result<Option<Vec<Extraction>>, (MarginError, Duration)> {
        let info = SceneInfo {
            extractors,
            project_title,
            project_type: pt,
            title,
            pov,
            known,
        };
        let schema = extract::schema(pt, extractors);
        let mut out = vec![];
        for w in extract::windows(&data.paras) {
            let prompt = extract::build_prompt(&info, &data.paras, w);
            let mut sampling = self.ai.active_preset().sampling_for(Role::Extraction);
            sampling.json_schema = Some(schema.clone());
            let job = self.ai.submit(JobSpec {
                priority,
                role: Role::Extraction,
                prompt,
                sampling,
                resumable: true,
                log: true,
            });
            let c = job.finish().await;
            match c.outcome {
                Outcome::Cancelled => return Ok(None),
                Outcome::Failed => {
                    return Err((
                        MarginError::Internal(format!(
                            "reading {title}: {}",
                            c.error.unwrap_or_else(|| "the model failed".into())
                        )),
                        RETRY_FAILED,
                    ));
                }
                Outcome::Completed => {}
            }
            match extract::parse_output(&c.text) {
                Some(x) => out.push(x),
                None => {
                    return Err((
                        MarginError::Internal(format!(
                            "reading {title}: the model's answer was not usable"
                        )),
                        RETRY_UNREADABLE,
                    ));
                }
            }
        }
        let _ = doc;
        Ok(Some(out))
    }

    // --- Chapter and book summaries (SPEC §9.3, §9.5 hierarchy) ----------------

    /// Make one stale chapter or book summary. Idle work, after the scenes.
    async fn summarize_next(self: &Arc<Self>) -> Result<()> {
        struct Job {
            id: String,
            is_book: bool,
            title: String,
            parts: Vec<(String, String)>,
            hashes: Vec<String>,
        }
        let job: Option<Job> = {
            let i = lock(&self.inner);
            let Some(store) = &i.store else { return Ok(()) };
            let mut found = None;
            // Chapters: containers holding two or more scenes with cards.
            let mut chapters: Vec<String> = i
                .layout
                .kinds
                .iter()
                .filter(|(_, k)| **k == crate::project::document::DocKind::Chapter)
                .map(|(id, _)| id.clone())
                .collect();
            chapters.sort_by_key(|c| {
                i.layout
                    .order
                    .iter()
                    .position(|d| d == c)
                    .unwrap_or(usize::MAX)
            });
            for ch in &chapters {
                let kids: Vec<&String> = i
                    .layout
                    .order
                    .iter()
                    .filter(|d| i.layout.parent.get(*d) == Some(ch) && !Self::is_excluded(&i, d))
                    .collect();
                let cards: Vec<_> = kids
                    .iter()
                    .filter_map(|d| store.scenes.get(*d).map(|c| (*d, c)))
                    .collect();
                if cards.len() < 2 || cards.len() != kids.len() {
                    continue;
                }
                let hashes: Vec<String> =
                    cards.iter().map(|(_, c)| c.source_hash.clone()).collect();
                if store
                    .chapters
                    .get(ch)
                    .is_none_or(|s| s.source_hashes != hashes)
                {
                    found = Some(Job {
                        id: ch.clone(),
                        is_book: false,
                        title: i.layout.titles.get(ch).cloned().unwrap_or_default(),
                        parts: cards
                            .iter()
                            .map(|(d, c)| {
                                (
                                    i.layout.titles.get(*d).cloned().unwrap_or_default(),
                                    c.summary.clone(),
                                )
                            })
                            .collect(),
                        hashes,
                    });
                    break;
                }
            }
            if found.is_none() {
                // The book: chapter summaries where there are chapters, scene summaries otherwise.
                let mut parts = vec![];
                let mut hashes = vec![];
                for d in &i.layout.order {
                    if Self::is_excluded(&i, d) {
                        continue;
                    }
                    let parent_is_chapter = i
                        .layout
                        .parent
                        .get(d)
                        .is_some_and(|p| store.chapters.contains_key(p));
                    if parent_is_chapter {
                        continue;
                    }
                    if let Some(c) = store.scenes.get(d) {
                        parts.push((
                            i.layout.titles.get(d).cloned().unwrap_or_default(),
                            c.summary.clone(),
                        ));
                        hashes.push(c.source_hash.clone());
                    }
                }
                for (ch, s) in &store.chapters {
                    if i.layout.titles.contains_key(ch) {
                        parts.push((
                            i.layout.titles.get(ch).cloned().unwrap_or_default(),
                            s.summary.clone(),
                        ));
                        hashes.push(s.source_hashes.join(""));
                    }
                }
                if parts.len() >= 2
                    && Self::pending_docs(&i, &self.wanted_extractors()).is_empty()
                    && store
                        .synopsis
                        .as_ref()
                        .is_none_or(|s| s.source_hashes != hashes)
                {
                    found = Some(Job {
                        id: "book".into(),
                        is_book: true,
                        title: "the book".into(),
                        parts,
                        hashes,
                    });
                }
            }
            found
        };
        let Some(job) = job else { return Ok(()) };
        let project = self.story.project_info()?;
        let what = if job.is_book { "book" } else { "chapter" };
        let body = job
            .parts
            .iter()
            .map(|(t, s)| format!("{t}: {s}"))
            .collect::<Vec<_>>()
            .join("\n");
        let role = format!(
            "You write a short {what} summary for the author's private notes. Use only the summaries given. Three or four sentences, plain prose, past tense unless the book is in present tense. No preamble."
        );
        let user = format!(
            "Project: {}.\nSummarise {}.\n\n{body}",
            project.title, job.title
        );
        let prompt = Prompt {
            system: role.clone(),
            user: user.clone(),
            assistant_prefix: String::new(),
            sections: vec![
                PromptSection {
                    label: "[1] Role instructions".into(),
                    tokens: crate::ai::ghost::estimate_tokens(&role),
                    text: role,
                },
                PromptSection {
                    label: "[5] Summaries".into(),
                    tokens: crate::ai::ghost::estimate_tokens(&user),
                    text: user,
                },
            ],
        };
        let sampling = self.ai.active_preset().sampling_for(Role::Summary);
        let c = self
            .ai
            .submit(JobSpec {
                priority: Priority::P2,
                role: Role::Summary,
                prompt,
                sampling,
                resumable: true,
                log: true,
            })
            .finish()
            .await;
        if c.outcome != Outcome::Completed {
            return Ok(());
        }
        let text = c.text.trim().to_string();
        if text.is_empty() {
            return Ok(());
        }
        self.with_store(|s| {
            let sum = Summary {
                schema_version: SCHEMA_VERSION,
                id: job.id.clone(),
                summary: text,
                source_hashes: job.hashes.clone(),
                updated: now(),
            };
            if job.is_book {
                s.synopsis = Some(sum);
                s.save_synopsis()
            } else {
                s.chapters.insert(job.id.clone(), sum);
                s.save_chapter(&job.id)
            }
        })?;
        self.emit(vec![], vec![], &["summaries"]);
        Ok(())
    }
}
