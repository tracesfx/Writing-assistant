//! The Margin panel's AI (SPEC §8): Ideas, Ask my book, Phrasing, and the
//! bible cards that go into ghost-text prompts (§7.4 [4]).
//!
//! Nothing here writes manuscript text. Ideas and alternatives are returned as
//! data; the author decides what to do with them. "To note" appends to a note
//! document in the Notes section, an explicit author action.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;

use super::mentions::count_mentions;
use super::paragraphs::Para;
use super::search::bm25;
use super::store::Store;
use super::types::*;
use super::views::*;
use super::{MemoryService, lock};
use crate::ai::ghost::estimate_tokens;
use crate::ai::scheduler::JobSpec;
use crate::ai::types::{Outcome, Priority, Prompt, PromptSection, Role};
use crate::error::{MarginError, Result};
use crate::project::atomic::atomic_write;
use crate::project::document::DocKind;
use crate::project::manifest::{ProjectType, Section};
use crate::story::types::BinderTarget;

const IDEAS_FICTION: &[&str] = &[
    "next", "stakes", "reaction", "sensory", "thread", "question",
];
const IDEAS_NONFICTION: &[&str] = &[
    "point",
    "counter",
    "evidence",
    "term",
    "transition",
    "question",
];
/// Token budget for Ask's evidence (SPEC §9.5: ~14K inside the 16K slot; leave room to answer).
const ASK_EVIDENCE_TOKENS: u32 = 9000;
const IDEA_MAX_CHARS: usize = 240;

fn kind_hint(kind: &str) -> &'static str {
    match kind {
        "next" => "what could happen next",
        "stakes" => "a way to raise the stakes",
        "reaction" => "a hidden reaction or thought",
        "sensory" => "a sensory detail to add",
        "thread" => "an unresolved thread to remember",
        "question" => "a question to ask yourself",
        "point" => "the next point to make",
        "counter" => "a counter-argument to address",
        "evidence" => "missing evidence to find",
        "term" => "a term to clarify",
        "transition" => "a transition to the next idea",
        _ => "an idea",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IdeasRequest {
    pub doc_id: String,
    /// The scene's plain text up to the cursor; paragraphs are separated by a blank line.
    pub text_before: String,
    /// Ideas the author dismissed this session: do not suggest them again.
    pub avoid: Vec<String>,
    /// Made by the panel on its own (idle work) rather than asked for.
    pub auto: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PhrasingRequest {
    pub doc_id: String,
    pub selected: String,
    pub before: String,
    pub after: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct IdeasFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    pinned: Vec<PinnedIdea>,
    /// Scene id → the note document its ideas were appended to.
    #[serde(default)]
    notes: std::collections::BTreeMap<String, String>,
}

/// An evidence item for Ask.
struct Ev {
    kind: &'static str,
    title: String,
    text: String,
    source: Option<Source>,
    entity: Option<String>,
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.trim().to_string();
    }
    let t: String = s.chars().take(max).collect();
    match t.rfind(' ') {
        Some(i) if i > max / 2 => format!("{}…", t[..i].trim_end()),
        _ => format!("{t}…"),
    }
}

fn section(label: &str, text: &str) -> PromptSection {
    PromptSection {
        label: label.into(),
        text: text.into(),
        tokens: estimate_tokens(text),
    }
}

impl MemoryService {
    // --- Bible cards for ghost text (SPEC §7.4 [4]) ------------------------------

    /// Short cards for the entries named in the text so far, in order of first
    /// appearance (so the block stays the same as the scene grows, which keeps
    /// the model's prompt cache). Canon facts come first.
    pub fn cards_for(&self, text_before: &str, max_tokens: u32) -> Vec<String> {
        let i = lock(&self.inner);
        let Some(store) = &i.store else { return vec![] };
        let lower = text_before.to_lowercase();
        let mut found: Vec<(usize, &Entity)> = store
            .entities
            .values()
            .filter(|e| e.kind != "thread")
            .filter_map(|e| {
                let first = Store::names_of(e)
                    .iter()
                    .filter(|n| n.chars().count() >= 2)
                    .filter_map(|n| {
                        let n = n.to_lowercase();
                        lower
                            .find(&n)
                            .filter(|_| count_mentions(&[n.as_str()], &lower) > 0)
                    })
                    .min()?;
                Some((first, e))
            })
            .collect();
        found.sort_by_key(|(pos, e)| (*pos, e.name.clone()));
        let mut out = vec![];
        let mut used = 0;
        for (_, e) in found {
            let mut facts: Vec<&Fact> = e
                .facts
                .iter()
                .filter(|f| f.conflicts_with.is_none())
                .collect();
            facts.sort_by_key(|f| f.status);
            let mut parts: Vec<String> = e
                .fields
                .iter()
                .take(3)
                .map(|(k, v)| format!("{k}: {v}"))
                .collect();
            parts.extend(
                facts
                    .iter()
                    .take(4)
                    .map(|f| f.text.trim_end_matches('.').to_string()),
            );
            for r in e.relations.iter().take(2) {
                if let Some(t) = store.entities.get(&r.to) {
                    parts.push(format!("{} of {}", r.kind, t.name));
                }
            }
            // Relations stored on the other entry (symmetric kinds are stored once).
            for (o, r) in store
                .entities
                .values()
                .flat_map(|o| o.relations.iter().map(move |r| (o, r)))
                .filter(|(_, r)| r.to == e.id && !r.directed)
                .take(2)
            {
                parts.push(format!("{} of {}", r.kind, o.name));
            }
            if parts.is_empty() {
                continue;
            }
            let card = format!("{} ({}): {}.", e.name, e.kind, parts.join("; "));
            used += estimate_tokens(&card);
            if used > max_tokens {
                break;
            }
            out.push(card);
        }
        out
    }

    // --- Hierarchy context -------------------------------------------------------

    /// Book synopsis → chapter summary → the previous two scene cards (SPEC §9.5).
    fn hierarchy(&self, doc: &str) -> (Vec<String>, String) {
        let i = lock(&self.inner);
        let Some(store) = &i.store else {
            return (vec![], String::new());
        };
        let mut lines = vec![];
        if let Some(s) = &store.synopsis {
            lines.push(format!("Book so far: {}", s.summary));
        }
        if let Some(ch) = i.layout.parent.get(doc)
            && let Some(s) = store.chapters.get(ch)
        {
            lines.push(format!(
                "This chapter ({}): {}",
                i.layout.titles.get(ch).cloned().unwrap_or_default(),
                s.summary
            ));
        }
        let pos = i.layout.order.iter().position(|d| d == doc).unwrap_or(0);
        let prev: Vec<String> = i.layout.order[..pos]
            .iter()
            .rev()
            .filter_map(|d| {
                store.scenes.get(d).map(|c| {
                    format!(
                        "Earlier scene \"{}\": {}",
                        i.layout.titles.get(d).cloned().unwrap_or_default(),
                        c.summary
                    )
                })
            })
            .take(2)
            .collect();
        lines.extend(prev.into_iter().rev());
        let title = i.layout.titles.get(doc).cloned().unwrap_or_default();
        (lines, title)
    }

    /// Find the stored paragraph a piece of editor text corresponds to.
    fn find_para(&self, doc: &str, text: &str) -> Option<Para> {
        let i = lock(&self.inner);
        let d = i.docs.get(doc)?;
        let n = normalize(text);
        if n.is_empty() {
            return None;
        }
        d.paras.iter().find(|p| normalize(&p.text) == n).cloned()
    }

    async fn run_job(
        &self,
        priority: Priority,
        role: Role,
        prompt: Prompt,
        schema: Option<String>,
        max_tokens: Option<u32>,
    ) -> Result<(String, String)> {
        let mut sampling = self.ai.active_preset().sampling_for(role);
        sampling.json_schema = schema;
        if let Some(m) = max_tokens {
            sampling.max_tokens = m;
        }
        let job = self.ai.submit(JobSpec {
            priority,
            role,
            prompt,
            sampling,
            resumable: false,
            log: true,
        });
        let id = job.id.clone();
        let c = job.finish().await;
        match c.outcome {
            Outcome::Completed => Ok((c.text, id)),
            Outcome::Cancelled => Err(MarginError::Internal("the request was cancelled".into())),
            Outcome::Failed => Err(MarginError::Internal(
                c.error.unwrap_or_else(|| "the model failed".into()),
            )),
        }
    }

    // --- Ideas (SPEC §8.1) ---------------------------------------------------------

    pub async fn ideas(self: &Arc<Self>, req: IdeasRequest) -> Result<IdeasResult> {
        let project = self.story.project_info()?;
        let fiction = project.project_type != ProjectType::NonFiction;
        let allowed = if fiction {
            IDEAS_FICTION
        } else {
            IDEAS_NONFICTION
        };
        let meta = self.story.document_meta(&req.doc_id)?;
        if meta.ai == crate::project::document::AiToggle::Off {
            return Ok(IdeasResult {
                ideas: vec![],
                context_id: String::new(),
                note: Some("AI is off for this scene.".into()),
            });
        }
        let (hier, _) = self.hierarchy(&req.doc_id);
        let cards = self.cards_for(&req.text_before, 700);
        // The last few paragraphs, numbered so the model can point at them.
        let paragraphs: Vec<&str> = req
            .text_before
            .split("\n\n")
            .filter(|p| !p.trim().is_empty())
            .collect();
        let recent: Vec<&str> = paragraphs.iter().rev().take(6).rev().copied().collect();
        let first_no = paragraphs.len() - recent.len() + 1;
        let mut recent_text = String::new();
        let mut budget = 1800;
        let mut used: Vec<(usize, &str)> = vec![];
        for (k, p) in recent.iter().enumerate().rev() {
            let t = estimate_tokens(p) + 2;
            if t > budget && !used.is_empty() {
                break;
            }
            budget = budget.saturating_sub(t);
            used.push((first_no + k, p));
        }
        used.reverse();
        for (n, p) in &used {
            recent_text.push_str(&format!("[{n}] {p}\n\n"));
        }

        let role = if fiction {
            "You are a quiet writing partner for a novelist. Offer short ideas the author may use or ignore. Never write prose they could paste in, never continue the text, never judge it. One or two sentences per idea."
        } else {
            "You are a quiet writing partner for a non-fiction author. Offer short ideas the author may use or ignore. Never write prose they could paste in, never judge the text. One or two sentences per idea."
        };
        let primer = format!("Project: {}.", project.title.trim());
        let mut notes = format!("Scene: {}", meta.title.trim());
        if let Some(p) = meta.pov.as_deref().filter(|p| !p.trim().is_empty()) {
            notes.push_str(&format!("\nPoint of view: {}", p.trim()));
        }
        if let Some(s) = meta.synopsis.as_deref().filter(|s| !s.trim().is_empty()) {
            notes.push_str(&format!("\nSynopsis: {}", s.trim()));
        }
        let story_so_far = if hier.is_empty() {
            "No summaries yet.".to_string()
        } else {
            hier.join("\n")
        };
        let cards_text = if cards.is_empty() {
            "No bible entries mentioned yet.".to_string()
        } else {
            cards.join("\n")
        };
        let kinds = allowed
            .iter()
            .map(|k| format!("{k} ({})", kind_hint(k)))
            .collect::<Vec<_>>()
            .join(", ");
        let avoid = if req.avoid.is_empty() {
            String::new()
        } else {
            format!(
                "\nThe author already dismissed these, so do not suggest anything like them:\n{}",
                req.avoid
                    .iter()
                    .rev()
                    .take(8)
                    .map(|a| format!("- {}", clip(a, 120)))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        let task = format!(
            "Give three or four ideas. Each has a type (one of: {kinds}), a text of at most {IDEA_MAX_CHARS} characters, and refs: the numbers of the paragraphs it relates to (or an empty list).{avoid}"
        );
        let user = format!(
            "{notes}\n\nStory so far:\n{story_so_far}\n\nBible entries in this scene:\n{cards_text}\n\nThe scene so far (last paragraphs):\n\n{recent_text}{task}"
        );
        let prompt = Prompt {
            system: format!("{role}\n\n{primer}"),
            user: user.clone(),
            assistant_prefix: String::new(),
            sections: vec![
                section("[1] Role instructions", role),
                section("[2] Project primer", &primer),
                section("[3] Scene notes", &notes),
                section(
                    "[4] Story so far and bible cards",
                    &format!("{story_so_far}\n\n{cards_text}"),
                ),
                section("[5] Scene text", recent_text.trim_end()),
                section("[6] Task", &task),
            ],
        };
        let schema = json!({
            "type": "object",
            "properties": {"ideas": {"type": "array", "maxItems": 4, "items": {
                "type": "object",
                "properties": {
                    "type": {"type": "string", "enum": allowed},
                    "text": {"type": "string"},
                    "refs": {"type": "array", "maxItems": 3, "items": {"type": "integer"}}
                },
                "required": ["type", "text", "refs"]
            }}},
            "required": ["ideas"]
        })
        .to_string();
        let priority = if req.auto { Priority::P2 } else { Priority::P1 };
        let (text, context_id) = self
            .run_job(priority, Role::Ideas, prompt, Some(schema), Some(500))
            .await?;
        let v = super::extract::parse_json_lenient(&text)
            .ok_or_else(|| MarginError::Internal("the model's ideas were not readable".into()))?;
        let mut ideas: Vec<Idea> = vec![];
        for it in v["ideas"].as_array().cloned().unwrap_or_default() {
            let kind = it["type"].as_str().unwrap_or("").to_lowercase();
            let text = clip(it["text"].as_str().unwrap_or(""), IDEA_MAX_CHARS);
            if text.chars().count() < 8 || !allowed.contains(&kind.as_str()) {
                continue;
            }
            if req.avoid.iter().any(|a| similarity(a, &text) >= 0.7)
                || ideas.iter().any(|x| similarity(&x.text, &text) >= 0.7)
            {
                continue;
            }
            let refs = it["refs"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|n| n.as_u64().map(|n| n as usize))
                .filter_map(|n| used.iter().find(|(k, _)| *k == n).map(|(_, p)| *p))
                .map(|p| {
                    let stored = self.find_para(&req.doc_id, p);
                    PassageRef {
                        doc: req.doc_id.clone(),
                        doc_title: meta.title.clone(),
                        para: stored.as_ref().map(|s| s.id.clone()),
                        block: stored.as_ref().map(|s| s.block),
                        quote: clip(p, 140),
                    }
                })
                .collect();
            ideas.push(Idea {
                id: new_id(),
                kind,
                text,
                refs,
                context_id: context_id.clone(),
            });
        }
        Ok(IdeasResult {
            note: ideas
                .is_empty()
                .then(|| "The model had no new ideas for this spot.".into()),
            ideas,
            context_id,
        })
    }

    /// "Expand": more detail on an idea, still an idea (SPEC §8.2).
    pub async fn idea_expand(
        self: &Arc<Self>,
        doc_id: &str,
        idea: &str,
        text_before: &str,
    ) -> Result<(String, String)> {
        let project = self.story.project_info()?;
        let meta = self.story.document_meta(doc_id)?;
        let cards = self.cards_for(text_before, 500);
        let tail: String = {
            let ps: Vec<&str> = text_before
                .split("\n\n")
                .filter(|p| !p.trim().is_empty())
                .collect();
            ps.iter()
                .rev()
                .take(3)
                .rev()
                .copied()
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        let role = "You are a quiet writing partner. Expand one idea with more detail: who is affected, what it changes, what could go wrong. Stay an idea. Do not write scene prose. At most five sentences.";
        let user = format!(
            "Project: {}. Scene: {}.\n\nBible entries:\n{}\n\nEnd of the scene so far:\n{}\n\nIdea to expand: {}",
            project.title.trim(),
            meta.title.trim(),
            if cards.is_empty() {
                "none".into()
            } else {
                cards.join("\n")
            },
            tail,
            idea
        );
        let prompt = Prompt {
            system: role.into(),
            user: user.clone(),
            assistant_prefix: String::new(),
            sections: vec![
                section("[1] Role instructions", role),
                section("[5] Context and idea", &user),
            ],
        };
        let (text, id) = self
            .run_job(Priority::P1, Role::Ideas, prompt, None, Some(300))
            .await?;
        Ok((clip(text.trim(), 900), id))
    }

    // --- Pins and notes ---------------------------------------------------------

    fn ideas_path(&self) -> Result<std::path::PathBuf> {
        Ok(self.story.project_root()?.join("memory").join("ideas.json"))
    }

    fn read_ideas(&self) -> IdeasFile {
        self.ideas_path()
            .ok()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn write_ideas(&self, mut f: IdeasFile) -> Result<()> {
        f.schema_version = SCHEMA_VERSION;
        let p = self.ideas_path()?;
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        atomic_write(&p, &serde_json::to_vec_pretty(&f)?)?;
        Ok(())
    }

    pub fn ideas_pinned(&self) -> Result<Vec<PinnedIdea>> {
        self.story.project_root()?;
        Ok(self.read_ideas().pinned)
    }

    pub fn idea_pin(&self, idea: Idea, doc: &str) -> Result<Vec<PinnedIdea>> {
        let mut f = self.read_ideas();
        if !f.pinned.iter().any(|p| p.idea.id == idea.id) {
            f.pinned.push(PinnedIdea {
                idea,
                doc: doc.to_string(),
                pinned_at: now(),
            });
        }
        let out = f.pinned.clone();
        self.write_ideas(f)?;
        Ok(out)
    }

    pub fn idea_unpin(&self, id: &str) -> Result<Vec<PinnedIdea>> {
        let mut f = self.read_ideas();
        f.pinned.retain(|p| p.idea.id != id);
        let out = f.pinned.clone();
        self.write_ideas(f)?;
        Ok(out)
    }

    /// "To note": append the idea to the scene's note document (created in the
    /// Notes section on first use). Returns the note's id.
    pub fn idea_to_note(&self, doc_id: &str, text: &str) -> Result<String> {
        let meta = self.story.document_meta(doc_id)?;
        let mut f = self.read_ideas();
        let existing = f
            .notes
            .get(doc_id)
            .filter(|id| {
                self.story
                    .document_meta(id)
                    .is_ok_and(|m| m.section.is_some())
            })
            .cloned();
        let note_id = match existing {
            Some(id) => id,
            None => {
                let m = self.story.document_create(
                    DocKind::Note,
                    &format!("{} — notes", meta.title),
                    &BinderTarget {
                        section: Section::Notes,
                        parent: None,
                        index: None,
                    },
                )?;
                f.notes.insert(doc_id.to_string(), m.id.clone());
                self.write_ideas(f)?;
                m.id
            }
        };
        let mut ast = self.story.document_ast(&note_id)?;
        let md = crate::dialect::serialize(&ast);
        let body = format!(
            "{}{}{}\n",
            md.trim_end(),
            if md.trim().is_empty() { "" } else { "\n\n" },
            text.trim()
        );
        ast = crate::dialect::parse(&body);
        self.story
            .replace_bodies(&[(note_id.clone(), ast)], "idea to note")?;
        Ok(note_id)
    }

    // --- Phrasing (SPEC §8.1.3) ---------------------------------------------------

    pub async fn phrasing(self: &Arc<Self>, req: PhrasingRequest) -> Result<PhrasingResult> {
        let selected = req.selected.trim();
        if selected.is_empty() {
            return Err(MarginError::Invalid("select some text first".into()));
        }
        let project = self.story.project_info()?;
        let meta = self.story.document_meta(&req.doc_id)?;
        if meta.ai == crate::project::document::AiToggle::Off {
            return Ok(PhrasingResult {
                alternatives: vec![],
                context_id: String::new(),
                note: Some("AI is off for this scene.".into()),
            });
        }
        let before: String = {
            let c: Vec<char> = req.before.chars().collect();
            c[c.len().saturating_sub(1200)..].iter().collect()
        };
        let after: String = req.after.chars().take(300).collect();
        let cards = self.cards_for(&format!("{before} {selected} {after}"), 400);
        let role = "You are a copy-editor offering alternative wordings. Keep the meaning, the voice, the tense, the point of view and the spelling convention. Do not add information. Keep roughly the same length. Return only wordings of the selected text.";
        let spelling = match project.locale {
            crate::project::manifest::Locale::EnCa => "Canadian English",
            crate::project::manifest::Locale::EnUs => "American English",
        };
        let user = format!(
            "Spelling: {spelling}.\nBible entries:\n{}\n\nBefore the selection:\n{}\n\nSELECTED TEXT:\n{}\n\nAfter the selection:\n{}\n\nGive four different ways to word the selected text.",
            if cards.is_empty() {
                "none".into()
            } else {
                cards.join("\n")
            },
            before,
            selected,
            after
        );
        let prompt = Prompt {
            system: role.into(),
            user: user.clone(),
            assistant_prefix: String::new(),
            sections: vec![
                section("[1] Role instructions", role),
                section("[5] Selection and context", &user),
            ],
        };
        let schema = json!({
            "type": "object",
            "properties": {"alternatives": {"type": "array", "maxItems": 5, "items": {"type": "string"}}},
            "required": ["alternatives"]
        })
        .to_string();
        let (text, context_id) = self
            .run_job(
                Priority::P1,
                Role::Phrasing,
                prompt,
                Some(schema),
                Some(500),
            )
            .await?;
        let v = super::extract::parse_json_lenient(&text)
            .ok_or_else(|| MarginError::Internal("the alternatives were not readable".into()))?;
        let mut alts: Vec<String> = vec![];
        for a in v["alternatives"].as_array().cloned().unwrap_or_default() {
            let a = a.as_str().unwrap_or("").trim().to_string();
            let a = crate::ai::ghost::normalise_spelling(&a, &before, project.locale);
            if a.is_empty()
                || normalize(&a) == normalize(selected)
                || alts.iter().any(|x| normalize(x) == normalize(&a))
                || a.chars().count() > selected.chars().count() * 3 + 40
            {
                continue;
            }
            alts.push(a);
        }
        alts.truncate(5);
        Ok(PhrasingResult {
            note: alts
                .is_empty()
                .then(|| "No usable alternatives this time. Try again.".into()),
            alternatives: alts,
            context_id,
        })
    }

    // --- Ask my book (SPEC §8.1.2) ----------------------------------------------

    fn source_ref(&self, s: &Source) -> PassageRef {
        let title = lock(&self.inner)
            .layout
            .titles
            .get(&s.doc)
            .cloned()
            .unwrap_or_default();
        PassageRef {
            doc: s.doc.clone(),
            doc_title: title,
            para: s.para.clone(),
            block: s.block,
            quote: s.quote.clone(),
        }
    }

    /// The evidence for a question, best first: facts and relations of the
    /// entries it names, scene summaries, then passages.
    async fn gather_evidence(&self, question: &str) -> Result<Vec<Ev>> {
        let mut out: Vec<Ev> = vec![];
        {
            let i = lock(&self.inner);
            let store = i.store.as_ref().ok_or(MarginError::NoProject)?;
            let qlower = question.to_lowercase();
            for e in store.entities.values() {
                if super::mentions::count_in_lower(&Store::names_of(e), &qlower) == 0 {
                    continue;
                }
                let mut facts: Vec<&Fact> = e.facts.iter().collect();
                facts.sort_by_key(|f| f.status);
                for f in facts.into_iter().take(8) {
                    out.push(Ev {
                        kind: "fact",
                        title: format!(
                            "{} ({})",
                            e.name,
                            if f.status == Status::Canon {
                                "canon"
                            } else {
                                "from the text"
                            }
                        ),
                        text: f.text.clone(),
                        source: f.source.clone(),
                        entity: Some(e.id.clone()),
                    });
                }
                for r in e.relations.iter().take(6) {
                    if let Some(t) = store.entities.get(&r.to) {
                        out.push(Ev {
                            kind: "relation",
                            title: e.name.clone(),
                            text: if r.directed {
                                format!("{} is {} of {}", e.name, r.kind, t.name)
                            } else {
                                format!(
                                    "{} and {} are in a \"{}\" relationship",
                                    e.name, t.name, r.kind
                                )
                            },
                            source: r.source.clone(),
                            entity: Some(e.id.clone()),
                        });
                    }
                }
            }
            // Scene summaries that match.
            let cards: Vec<(&String, String)> = store
                .scenes
                .iter()
                .map(|(id, c)| {
                    (
                        id,
                        format!(
                            "{} {}",
                            i.layout.titles.get(id).cloned().unwrap_or_default(),
                            c.summary
                        ),
                    )
                })
                .collect();
            let texts: Vec<&str> = cards.iter().map(|(_, t)| t.as_str()).collect();
            for (n, _) in bm25(question, &texts, 3) {
                let (id, _) = &cards[n];
                if let Some(c) = store.scenes.get(*id) {
                    out.push(Ev {
                        kind: "scene",
                        title: format!(
                            "Scene summary: {}",
                            i.layout.titles.get(*id).cloned().unwrap_or_default()
                        ),
                        text: c.summary.clone(),
                        source: Some(Source {
                            doc: (*id).clone(),
                            para: None,
                            block: None,
                            quote: clip(&c.summary, 140),
                        }),
                        entity: None,
                    });
                }
            }
        }
        for h in self.search_semantic(question, 8).await? {
            out.push(Ev {
                kind: "passage",
                title: format!("Passage in {}", h.doc_title),
                text: clip(&h.text, 1200),
                source: Some(Source {
                    doc: h.doc.clone(),
                    para: Some(h.para.clone()),
                    block: Some(h.block),
                    quote: clip(&h.text, 140),
                }),
                entity: None,
            });
        }
        // Budget.
        let mut used = 0;
        out.retain(|e| {
            used += estimate_tokens(&e.text) + 12;
            used <= ASK_EVIDENCE_TOKENS
        });
        Ok(out)
    }

    pub async fn ask(self: &Arc<Self>, question: &str) -> Result<AskAnswer> {
        let question = question.trim();
        if question.is_empty() {
            return Err(MarginError::Invalid("ask a question first".into()));
        }
        let ev = self.gather_evidence(question).await?;
        if ev.is_empty() {
            return Ok(AskAnswer {
                answer: "I can't find anything about that in your book yet.".into(),
                found: false,
                citations: vec![],
                context_id: None,
                note: Some("No bible entry or passage matched the question. Memory fills in as scenes are read.".into()),
            });
        }
        let project = self.story.project_info()?;
        let role = "You answer questions about the author's own book using ONLY the numbered evidence below. Never use outside knowledge and never guess. If the evidence does not answer the question, say so plainly and set found to false. Answer in one to four sentences and cite the numbers of the evidence you used.";
        let evidence: String = ev
            .iter()
            .enumerate()
            .map(|(n, e)| format!("[{}] {}: {}", n + 1, e.title, e.text))
            .collect::<Vec<_>>()
            .join("\n\n");
        let task = "Reply as JSON: found (true or false), answer (text), citations (the evidence numbers you used).";
        let user = format!(
            "Project: {}.\n\nEvidence:\n\n{evidence}\n\nQuestion: {question}\n\n{task}",
            project.title.trim()
        );
        let prompt = Prompt {
            system: role.into(),
            user: user.clone(),
            assistant_prefix: String::new(),
            sections: vec![
                section("[1] Role instructions", role),
                section(
                    "[2] Project primer",
                    &format!("Project: {}.", project.title.trim()),
                ),
                section("[4] Evidence", &evidence),
                section("[5] Question", question),
                section("[6] Task", task),
            ],
        };
        let schema = json!({
            "type": "object",
            "properties": {
                "found": {"type": "boolean"},
                "answer": {"type": "string"},
                "citations": {"type": "array", "maxItems": 6, "items": {"type": "integer"}}
            },
            "required": ["found", "answer", "citations"]
        })
        .to_string();
        let (text, context_id) = self
            .run_job(Priority::P1, Role::Ask, prompt, Some(schema), Some(500))
            .await?;
        let v = super::extract::parse_json_lenient(&text)
            .ok_or_else(|| MarginError::Internal("the answer was not readable".into()))?;
        let answer = v["answer"].as_str().unwrap_or("").trim().to_string();
        let found = v["found"].as_bool().unwrap_or(true) && !answer.is_empty();
        if !found {
            return Ok(AskAnswer {
                answer: if answer.is_empty() {
                    "I can't find that in your book.".into()
                } else {
                    answer
                },
                found: false,
                citations: vec![],
                context_id: Some(context_id),
                note: None,
            });
        }
        // Only evidence that really exists can be cited.
        let mut labels: Vec<usize> = vec![];
        for n in v["citations"].as_array().cloned().unwrap_or_default() {
            if let Some(n) = n.as_u64().map(|n| n as usize)
                && (1..=ev.len()).contains(&n)
                && !labels.contains(&n)
            {
                labels.push(n);
            }
        }
        let mut note = None;
        if labels.is_empty() {
            // The model cited nothing: choose the evidence that best matches the answer.
            let texts: Vec<&str> = ev.iter().map(|e| e.text.as_str()).collect();
            labels = bm25(&answer, &texts, 2)
                .into_iter()
                .map(|(n, _)| n + 1)
                .collect();
            note =
                Some("The model gave no sources, so these were matched to the answer.".to_string());
        }
        let citations = labels
            .iter()
            .enumerate()
            .map(|(k, n)| {
                let e = &ev[n - 1];
                Citation {
                    label: (k + 1) as u32,
                    kind: e.kind.into(),
                    title: e.title.clone(),
                    text: e.text.clone(),
                    source: e.source.as_ref().map(|s| self.source_ref(s)),
                    entity_id: e.entity.clone(),
                }
            })
            .collect();
        // The answer cites the model's numbers; rewrite them to the shown order.
        let mut shown = answer.clone();
        for (k, n) in labels.iter().enumerate() {
            shown = shown.replace(&format!("[{n}]"), &format!("[#{}]", k + 1));
        }
        let shown = shown.replace("[#", "[");
        Ok(AskAnswer {
            answer: shown,
            found: true,
            citations,
            context_id: Some(context_id),
            note,
        })
    }
}
