//! The memory store: entities, scene cards, the timeline and summaries, as
//! plain files in the project folder (SPEC §9.3) with the canon rules
//! (SPEC §9.1) enforced here, not in the UI.
//!
//! * Author commands (`author_*`) may change anything. What they write is canon.
//! * AI commands (`ai_*`) may only create and update **inferred** records. Every
//!   one of them refuses to touch a canon record, whatever the caller passes.
//!
//! Files:
//! ```text
//! bible/<type>/<slug>--<id>.md        entity: YAML frontmatter + the author's notes
//! bible/.trash/<type>/…               deleted entities (recoverable)
//! memory/scenes/<sceneId>.json        scene cards
//! memory/timeline.json                events
//! memory/synopsis.json, memory/chapters/<id>.json   summaries
//! memory/settings.json                documents excluded from memory
//! memory/paragraphs/<docId>.json      paragraph id maps (see `paragraphs`)
//! ```

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::types::*;
use crate::error::{MarginError, Result};
use crate::project::atomic::atomic_write;

pub const BIBLE_DIR: &str = "bible";
pub const MEMORY_DIR: &str = "memory";
const TRASH_DIR: &str = ".trash";

#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    pub entities: BTreeMap<String, Entity>,
    pub scenes: BTreeMap<String, SceneCard>,
    pub events: Vec<TimelineEvent>,
    pub synopsis: Option<Summary>,
    pub chapters: BTreeMap<String, Summary>,
    pub project: ProjectMemory,
    /// Files that could not be read when the store loaded.
    pub problems: Vec<String>,
    paths: HashMap<String, PathBuf>,
}

#[derive(Serialize, Deserialize, Default)]
struct TimelineFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    events: Vec<TimelineEvent>,
}

fn invalid(msg: impl Into<String>) -> MarginError {
    MarginError::Invalid(msg.into())
}

/// Refuse an AI write to canon (SPEC §9.1, §17 check 9).
pub fn guard_ai(status: Status, what: &str) -> Result<()> {
    if status == Status::Canon {
        return Err(invalid(format!(
            "the AI cannot change canon ({what}); only the author can"
        )));
    }
    Ok(())
}

pub fn render_entity(e: &Entity) -> Result<String> {
    // Notes are the body, not frontmatter.
    let yaml = serde_norway::to_string(&Entity {
        notes: String::new(),
        ..e.clone()
    })?;
    let notes = e.notes.trim_end();
    Ok(if notes.is_empty() {
        format!("---\n{yaml}---\n")
    } else {
        format!("---\n{yaml}---\n{notes}\n")
    })
}

pub fn parse_entity(text: &str) -> Result<Entity> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .ok_or_else(|| MarginError::Parse("bible entry without frontmatter".into()))?;
    let end = rest
        .find("\n---\n")
        .map(|i| (i + 1, i + 5))
        .or_else(|| {
            rest.strip_suffix("\n---")
                .map(|r| (r.len() + 1, rest.len()))
        })
        .or_else(|| rest.find("\n---\r\n").map(|i| (i + 1, i + 6)))
        .ok_or_else(|| MarginError::Parse("unterminated frontmatter".into()))?;
    let mut e: Entity = serde_norway::from_str(&rest[..end.0])?;
    e.notes = rest[end.1.min(rest.len())..].trim().to_string();
    Ok(e)
}

fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    atomic_write(path, &serde_json::to_vec_pretty(v)?)?;
    Ok(())
}

impl Store {
    pub fn empty(root: &Path) -> Store {
        Store {
            root: root.to_path_buf(),
            entities: BTreeMap::new(),
            scenes: BTreeMap::new(),
            events: vec![],
            synopsis: None,
            chapters: BTreeMap::new(),
            project: ProjectMemory::default(),
            problems: vec![],
            paths: HashMap::new(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn bible(&self) -> PathBuf {
        self.root.join(BIBLE_DIR)
    }

    fn memory(&self) -> PathBuf {
        self.root.join(MEMORY_DIR)
    }

    pub fn load(root: &Path) -> Store {
        let mut s = Store::empty(root);
        if let Ok(types) = fs::read_dir(s.bible()) {
            for t in types.flatten() {
                let name = t.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || !t.path().is_dir() {
                    continue;
                }
                for f in fs::read_dir(t.path()).into_iter().flatten().flatten() {
                    let p = f.path();
                    if p.extension().is_none_or(|x| !x.eq_ignore_ascii_case("md")) {
                        continue;
                    }
                    match fs::read_to_string(&p)
                        .map_err(MarginError::from)
                        .and_then(|t| parse_entity(&t))
                    {
                        Ok(mut e) => {
                            // The folder is the type; trust it over a copied-in file.
                            if e.kind != name && is_valid_type(&name) {
                                e.kind = name.clone();
                            }
                            s.paths.insert(e.id.clone(), p);
                            s.entities.insert(e.id.clone(), e);
                        }
                        Err(err) => s.problems.push(format!(
                            "{}: {err}",
                            p.strip_prefix(root).unwrap_or(&p).display()
                        )),
                    }
                }
            }
        }
        let mem = s.memory();
        for f in fs::read_dir(mem.join("scenes"))
            .into_iter()
            .flatten()
            .flatten()
        {
            if let Some(c) = read_json::<SceneCard>(&f.path()) {
                s.scenes.insert(c.scene_id.clone(), c);
            }
        }
        for f in fs::read_dir(mem.join("chapters"))
            .into_iter()
            .flatten()
            .flatten()
        {
            if let Some(c) = read_json::<Summary>(&f.path()) {
                s.chapters.insert(c.id.clone(), c);
            }
        }
        s.synopsis = read_json(&mem.join("synopsis.json"));
        if let Some(t) = read_json::<TimelineFile>(&mem.join("timeline.json")) {
            s.events = t.events;
        }
        s.project = read_json(&mem.join("settings.json")).unwrap_or_default();
        s
    }

    // --- Persistence ------------------------------------------------------------

    fn entity_path(&self, e: &Entity) -> PathBuf {
        self.bible()
            .join(&e.kind)
            .join(format!("{}--{}.md", slug(&e.name), e.id))
    }

    /// Write an entity (and drop its old file if the name or type changed).
    pub fn save_entity(&mut self, id: &str) -> Result<()> {
        let e = self
            .entities
            .get(id)
            .ok_or_else(|| MarginError::NotFound(format!("entry {id}")))?;
        let path = self.entity_path(e);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        atomic_write(&path, render_entity(e)?.as_bytes())?;
        if let Some(old) = self.paths.insert(id.to_string(), path.clone())
            && old != path
        {
            let _ = fs::remove_file(old);
        }
        Ok(())
    }

    pub fn save_scene(&self, id: &str) -> Result<()> {
        match self.scenes.get(id) {
            Some(c) => write_json(&self.memory().join("scenes").join(format!("{id}.json")), c),
            None => {
                let _ = fs::remove_file(self.memory().join("scenes").join(format!("{id}.json")));
                Ok(())
            }
        }
    }

    pub fn save_timeline(&self) -> Result<()> {
        write_json(
            &self.memory().join("timeline.json"),
            &TimelineFile {
                schema_version: SCHEMA_VERSION,
                events: self.events.clone(),
            },
        )
    }

    pub fn save_synopsis(&self) -> Result<()> {
        match &self.synopsis {
            Some(s) => write_json(&self.memory().join("synopsis.json"), s),
            None => {
                let _ = fs::remove_file(self.memory().join("synopsis.json"));
                Ok(())
            }
        }
    }

    pub fn save_chapter(&self, id: &str) -> Result<()> {
        let path = self.memory().join("chapters").join(format!("{id}.json"));
        match self.chapters.get(id) {
            Some(s) => write_json(&path, s),
            None => {
                let _ = fs::remove_file(path);
                Ok(())
            }
        }
    }

    pub fn save_project(&mut self) -> Result<()> {
        self.project.schema_version = SCHEMA_VERSION;
        write_json(&self.memory().join("settings.json"), &self.project)
    }

    // --- Lookup -----------------------------------------------------------------

    /// All names an entity answers to.
    pub fn names_of(e: &Entity) -> Vec<&str> {
        std::iter::once(e.name.as_str())
            .chain(e.aliases.iter().map(|a| a.text.as_str()))
            .collect()
    }

    /// Find an entry by name or alias: exact match first, then a unique entry
    /// whose name contains the query's words ("Mara" for "Mara Voss").
    pub fn find_by_name(&self, name: &str, kind: Option<&str>) -> Option<&Entity> {
        let q = normalize(name);
        if q.is_empty() {
            return None;
        }
        let of_kind = |e: &&Entity| kind.is_none_or(|k| e.kind == k);
        if let Some(e) = self
            .entities
            .values()
            .filter(of_kind)
            .find(|e| Self::names_of(e).iter().any(|n| normalize(n) == q))
        {
            return Some(e);
        }
        let qw: Vec<&str> = q.split(' ').collect();
        let mut hits = self.entities.values().filter(of_kind).filter(|e| {
            Self::names_of(e).iter().any(|n| {
                let nn = normalize(n);
                let nw: Vec<&str> = nn.split(' ').collect();
                (qw.len() < nw.len() && nw.len() <= 4 && qw.iter().all(|w| nw.contains(w)))
                    || (nw.len() < qw.len() && qw.len() <= 4 && nw.iter().all(|w| qw.contains(w)))
            })
        });
        match (hits.next(), hits.next()) {
            (Some(e), None) => Some(e),
            _ => None,
        }
    }

    // --- Author commands (canon) -----------------------------------------------

    pub fn author_create(&mut self, kind: &str, name: &str, notes: &str) -> Result<Entity> {
        let name = name.trim();
        if name.is_empty() {
            return Err(invalid("an entry needs a name"));
        }
        if !is_valid_type(kind) {
            return Err(invalid(format!("bad entry type: {kind}")));
        }
        if self.find_by_name_exact(name, kind).is_some() {
            return Err(invalid(format!("there is already a {kind} named {name}")));
        }
        let t = now();
        let e = Entity {
            schema_version: SCHEMA_VERSION,
            id: new_id(),
            kind: kind.to_string(),
            name: name.to_string(),
            aliases: vec![],
            fields: BTreeMap::new(),
            facts: vec![],
            relations: vec![],
            tags: vec![],
            status: Status::Canon,
            created_by: Author::Author,
            created: t.clone(),
            updated: t,
            thread: (kind == "thread").then(ThreadData::default),
            notes: notes.trim().to_string(),
        };
        let id = e.id.clone();
        self.entities.insert(id.clone(), e);
        self.save_entity(&id)?;
        Ok(self.entities[&id].clone())
    }

    fn find_by_name_exact(&self, name: &str, kind: &str) -> Option<&Entity> {
        self.find_exact(name, Some(kind))
    }

    /// An entry whose name or alias is exactly this (ignoring case and punctuation).
    pub fn find_exact(&self, name: &str, kind: Option<&str>) -> Option<&Entity> {
        let q = normalize(name);
        if q.is_empty() {
            return None;
        }
        self.entities
            .values()
            .filter(|e| kind.is_none_or(|k| e.kind == k))
            .find(|e| Self::names_of(e).iter().any(|n| normalize(n) == q))
    }

    /// Edit an entry as the author. Whatever the author sets is canon, and
    /// confirming an inferred entry's name or aliases makes them canon too.
    pub fn author_update(&mut self, id: &str, p: &EntityPatch) -> Result<Entity> {
        let e = self
            .entities
            .get_mut(id)
            .ok_or_else(|| MarginError::NotFound(format!("entry {id}")))?;
        if let Some(name) = &p.name {
            let name = name.trim();
            if name.is_empty() {
                return Err(invalid("an entry needs a name"));
            }
            e.name = name.to_string();
        }
        if let Some(kind) = &p.kind
            && *kind != e.kind
        {
            if !is_valid_type(kind) {
                return Err(invalid(format!("bad entry type: {kind}")));
            }
            e.kind = kind.clone();
            if kind == "thread" && e.thread.is_none() {
                e.thread = Some(ThreadData::default());
            }
        }
        if let Some(aliases) = &p.aliases {
            let mut out: Vec<Alias> = vec![];
            for a in aliases {
                let a = a.trim();
                if a.is_empty() || out.iter().any(|x| normalize(&x.text) == normalize(a)) {
                    continue;
                }
                // What the author lists is canon, even if the AI proposed it.
                out.push(Alias {
                    text: a.to_string(),
                    status: Status::Canon,
                });
            }
            e.aliases = out;
        }
        if let Some(f) = &p.fields {
            e.fields = f
                .iter()
                .filter(|(k, v)| !k.trim().is_empty() && !v.trim().is_empty())
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                .collect();
        }
        if let Some(t) = &p.tags {
            e.tags = t
                .iter()
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect();
        }
        if let Some(n) = &p.notes {
            e.notes = n.trim().to_string();
        }
        if let Some(s) = p.thread_status {
            let t = e.thread.get_or_insert_with(ThreadData::default);
            t.status = s;
            t.status_canon = true;
        }
        e.status = Status::Canon;
        e.updated = now();
        self.save_entity(id)?;
        Ok(self.entities[id].clone())
    }

    /// Delete an entry. It moves to `bible/.trash/`, so it can come back.
    pub fn author_delete(&mut self, id: &str) -> Result<Entity> {
        let e = self
            .entities
            .remove(id)
            .ok_or_else(|| MarginError::NotFound(format!("entry {id}")))?;
        if let Some(path) = self.paths.remove(id) {
            let dest = self
                .bible()
                .join(TRASH_DIR)
                .join(&e.kind)
                .join(path.file_name().unwrap_or_default());
            if let Some(dir) = dest.parent() {
                fs::create_dir_all(dir)?;
            }
            // A move keeps the exact bytes; fall back to copy + delete across volumes.
            if fs::rename(&path, &dest).is_err() {
                fs::copy(&path, &dest)?;
                fs::remove_file(&path)?;
            }
        }
        Ok(e)
    }

    /// Bring a deleted entry back (undo).
    pub fn author_restore(&mut self, e: Entity) -> Result<Entity> {
        let id = e.id.clone();
        if self.entities.contains_key(&id) {
            return Ok(self.entities[&id].clone());
        }
        let trash = self.bible().join(TRASH_DIR).join(&e.kind);
        for f in fs::read_dir(&trash).into_iter().flatten().flatten() {
            if f.file_name()
                .to_string_lossy()
                .ends_with(&format!("--{id}.md"))
            {
                let _ = fs::remove_file(f.path());
            }
        }
        self.entities.insert(id.clone(), e);
        self.save_entity(&id)?;
        Ok(self.entities[&id].clone())
    }

    pub fn author_add_fact(
        &mut self,
        entity: &str,
        text: &str,
        field: Option<&str>,
        source: Option<Source>,
    ) -> Result<Fact> {
        let text = text.trim();
        if text.is_empty() {
            return Err(invalid("a fact needs text"));
        }
        let t = now();
        let f = Fact {
            id: new_id(),
            text: text.to_string(),
            field: field
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from),
            status: Status::Canon,
            source,
            history: vec![],
            conflicts_with: None,
            created_by: Author::Author,
            updated: t.clone(),
        };
        let e = self.entity_mut(entity)?;
        e.facts.push(f.clone());
        e.updated = t;
        self.save_entity(entity)?;
        Ok(f)
    }

    /// Edit a fact as the author. The edit makes it canon and keeps the old text.
    pub fn author_update_fact(
        &mut self,
        entity: &str,
        fact: &str,
        text: &str,
        field: Option<&str>,
    ) -> Result<Fact> {
        let text = text.trim();
        if text.is_empty() {
            return Err(invalid("a fact needs text"));
        }
        let e = self.entity_mut(entity)?;
        let f = e
            .facts
            .iter_mut()
            .find(|f| f.id == fact)
            .ok_or_else(|| MarginError::NotFound(format!("fact {fact}")))?;
        if f.text != text {
            f.history.push(HistoryEntry {
                at: now(),
                text: f.text.clone(),
                status: f.status,
                by: Author::Author,
                note: "edited by the author".into(),
            });
            f.text = text.to_string();
        }
        f.field = field
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        f.status = Status::Canon;
        f.conflicts_with = None;
        f.updated = now();
        let out = f.clone();
        e.updated = now();
        self.save_entity(entity)?;
        Ok(out)
    }

    pub fn author_delete_fact(&mut self, entity: &str, fact: &str) -> Result<()> {
        let e = self.entity_mut(entity)?;
        let n = e.facts.len();
        e.facts.retain(|f| f.id != fact);
        if e.facts.len() == n {
            return Err(MarginError::NotFound(format!("fact {fact}")));
        }
        e.updated = now();
        self.save_entity(entity)
    }

    pub fn author_add_relation(
        &mut self,
        from: &str,
        to: &str,
        kind: &str,
        since: Option<&str>,
    ) -> Result<Relation> {
        let kind = kind.trim().to_lowercase();
        if kind.is_empty() {
            return Err(invalid("a relation needs a kind"));
        }
        if from == to {
            return Err(invalid("an entry cannot relate to itself"));
        }
        if !self.entities.contains_key(to) {
            return Err(MarginError::NotFound(format!("entry {to}")));
        }
        let r = Relation {
            id: new_id(),
            to: to.to_string(),
            directed: !is_symmetric(&kind),
            kind,
            status: Status::Canon,
            source: None,
            since: since
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from),
            created_by: Author::Author,
            updated: now(),
        };
        let e = self.entity_mut(from)?;
        e.relations.push(r.clone());
        e.updated = now();
        self.save_entity(from)?;
        Ok(r)
    }

    pub fn author_delete_relation(&mut self, entity: &str, relation: &str) -> Result<()> {
        let e = self.entity_mut(entity)?;
        let n = e.relations.len();
        e.relations.retain(|r| r.id != relation);
        if e.relations.len() == n {
            return Err(MarginError::NotFound(format!("relation {relation}")));
        }
        e.updated = now();
        self.save_entity(entity)
    }

    /// Confirm an inferred record as canon ("✓ make canon").
    pub fn author_make_canon(&mut self, item: &ItemRef) -> Result<()> {
        let t = now();
        match item.kind {
            ItemKind::Entity => {
                let e = self.entity_mut(&item.id)?;
                e.status = Status::Canon;
                for a in &mut e.aliases {
                    a.status = Status::Canon;
                }
                e.updated = t;
                self.save_entity(&item.id)
            }
            ItemKind::Fact => {
                let eid = item
                    .entity
                    .clone()
                    .ok_or_else(|| invalid("fact without entry"))?;
                let e = self.entity_mut(&eid)?;
                let f = e
                    .facts
                    .iter_mut()
                    .find(|f| f.id == item.id)
                    .ok_or_else(|| MarginError::NotFound(format!("fact {}", item.id)))?;
                if f.status != Status::Canon {
                    f.history.push(HistoryEntry {
                        at: t.clone(),
                        text: f.text.clone(),
                        status: f.status,
                        by: Author::Author,
                        note: "made canon".into(),
                    });
                }
                f.status = Status::Canon;
                f.conflicts_with = None;
                f.updated = t;
                // The entry it belongs to is part of the story now.
                e.status = Status::Canon;
                self.save_entity(&eid)
            }
            ItemKind::Relation => {
                let eid = item
                    .entity
                    .clone()
                    .ok_or_else(|| invalid("relation without entry"))?;
                let e = self.entity_mut(&eid)?;
                let r = e
                    .relations
                    .iter_mut()
                    .find(|r| r.id == item.id)
                    .ok_or_else(|| MarginError::NotFound(format!("relation {}", item.id)))?;
                r.status = Status::Canon;
                r.updated = t;
                e.status = Status::Canon;
                self.save_entity(&eid)
            }
            ItemKind::Event => {
                let ev = self
                    .events
                    .iter_mut()
                    .find(|e| e.id == item.id)
                    .ok_or_else(|| MarginError::NotFound(format!("event {}", item.id)))?;
                ev.status = Status::Canon;
                ev.updated = t;
                self.save_timeline()
            }
        }
    }

    /// Delete inferred records in bulk. Canon records are refused (not skipped
    /// silently): the caller learns which ones.
    pub fn delete_inferred(&mut self, items: &[ItemRef]) -> Result<u32> {
        let mut n = 0;
        for item in items {
            match item.kind {
                ItemKind::Entity => {
                    let e = self
                        .entities
                        .get(&item.id)
                        .ok_or_else(|| MarginError::NotFound(format!("entry {}", item.id)))?;
                    guard_ai(e.status, &e.name)?;
                    self.author_delete(&item.id)?;
                }
                ItemKind::Fact => {
                    let eid = item
                        .entity
                        .clone()
                        .ok_or_else(|| invalid("fact without entry"))?;
                    let e = self.entity_mut(&eid)?;
                    let f = e
                        .facts
                        .iter()
                        .find(|f| f.id == item.id)
                        .ok_or_else(|| MarginError::NotFound(format!("fact {}", item.id)))?;
                    guard_ai(f.status, &f.text)?;
                    e.facts.retain(|f| f.id != item.id);
                    self.save_entity(&eid)?;
                }
                ItemKind::Relation => {
                    let eid = item
                        .entity
                        .clone()
                        .ok_or_else(|| invalid("relation without entry"))?;
                    let e = self.entity_mut(&eid)?;
                    let r = e
                        .relations
                        .iter()
                        .find(|r| r.id == item.id)
                        .ok_or_else(|| MarginError::NotFound(format!("relation {}", item.id)))?;
                    guard_ai(r.status, &r.kind)?;
                    e.relations.retain(|r| r.id != item.id);
                    self.save_entity(&eid)?;
                }
                ItemKind::Event => {
                    let ev = self
                        .events
                        .iter()
                        .find(|e| e.id == item.id)
                        .ok_or_else(|| MarginError::NotFound(format!("event {}", item.id)))?;
                    guard_ai(ev.status, &ev.title)?;
                    self.events.retain(|e| e.id != item.id);
                    self.save_timeline()?;
                }
            }
            n += 1;
        }
        Ok(n)
    }

    pub fn entity_mut(&mut self, id: &str) -> Result<&mut Entity> {
        self.entities
            .get_mut(id)
            .ok_or_else(|| MarginError::NotFound(format!("entry {id}")))
    }

    // --- AI commands (inferred only) -------------------------------------------

    /// Find or create an entry for a name the model read in the text. An
    /// existing entry (canon or not) is never changed except for *inferred*
    /// aliases added to it.
    pub fn ai_upsert_entity(
        &mut self,
        kind: &str,
        name: &str,
        aliases: &[String],
    ) -> Result<String> {
        let name = name.trim();
        if name.is_empty() || !is_valid_type(kind) {
            return Err(invalid("not a usable entry"));
        }
        // The same kind first (a short form may stand for a longer name); across kinds
        // only an exact name counts, so "Mr. Bingley" never absorbs a thread about him.
        let found = if kind == "thread" {
            self.find_exact(name, Some("thread"))
        } else {
            self.find_by_name(name, Some(kind))
                .or_else(|| self.find_exact(name, None).filter(|e| e.kind != "thread"))
        };
        let id = match found.map(|e| e.id.clone()) {
            Some(id) => id,
            None => {
                let t = now();
                let e = Entity {
                    schema_version: SCHEMA_VERSION,
                    id: new_id(),
                    kind: kind.to_string(),
                    name: name.to_string(),
                    aliases: vec![],
                    fields: BTreeMap::new(),
                    facts: vec![],
                    relations: vec![],
                    tags: vec![],
                    status: Status::Inferred,
                    created_by: Author::Ai,
                    created: t.clone(),
                    updated: t,
                    thread: (kind == "thread").then(ThreadData::default),
                    notes: String::new(),
                };
                let id = e.id.clone();
                self.entities.insert(id.clone(), e);
                id
            }
        };
        let mut candidates: Vec<String> = aliases.to_vec();
        candidates.push(name.to_string());
        let e = self.entity_mut(&id)?;
        let mut changed = false;
        for a in candidates {
            let a = a.trim();
            if a.is_empty()
                || Store::names_of(e)
                    .iter()
                    .any(|n| normalize(n) == normalize(a))
            {
                continue;
            }
            e.aliases.push(Alias {
                text: a.to_string(),
                status: Status::Inferred,
            });
            changed = true;
        }
        if changed {
            e.updated = now();
        }
        self.save_entity(&id)?;
        Ok(id)
    }

    /// Add or refresh an inferred fact. Returns `Ok(None)` when a canon fact
    /// already covers it (and the model's version is dropped).
    pub fn ai_put_fact(
        &mut self,
        entity: &str,
        text: &str,
        field: Option<&str>,
        source: Source,
        flag_conflicts: bool,
    ) -> Result<Option<String>> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let field = field
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(normalize);
        let t = now();
        let e = self.entity_mut(entity)?;
        // The same statement is already there: refresh the source of an inferred
        // one, leave a canon one alone.
        if let Some(f) = e
            .facts
            .iter_mut()
            .find(|f| similarity(&f.text, text) >= 0.8)
        {
            if f.status == Status::Inferred {
                f.source = Some(source);
                f.updated = t;
            }
            let id = f.id.clone();
            self.save_entity(entity)?;
            return Ok(Some(id));
        }
        // Same attribute, different value.
        if let Some(fld) = &field {
            let same = |f: &Fact| f.field.as_deref().map(normalize).as_ref() == Some(fld);
            if let Some(canon_id) = e
                .facts
                .iter()
                .find(|f| f.status == Status::Canon && same(f))
                .map(|f| f.id.clone())
            {
                if !flag_conflicts {
                    return Ok(None); // canon wins; nothing to record
                }
                // Opt-in: keep the model's reading, flagged against the canon fact.
                let id = new_id();
                e.facts.push(Fact {
                    id: id.clone(),
                    text: text.to_string(),
                    field: Some(fld.clone()),
                    status: Status::Inferred,
                    source: Some(source),
                    history: vec![],
                    conflicts_with: Some(canon_id),
                    created_by: Author::Ai,
                    updated: t,
                });
                self.save_entity(entity)?;
                return Ok(Some(id));
            }
            if let Some(f) = e
                .facts
                .iter_mut()
                .find(|f| f.status == Status::Inferred && same(f))
            {
                // The text now says something else: update the fact, keep the old reading.
                f.history.push(HistoryEntry {
                    at: t.clone(),
                    text: f.text.clone(),
                    status: f.status,
                    by: Author::Ai,
                    note: "replaced by a newer reading of the text".into(),
                });
                f.text = text.to_string();
                f.source = Some(source);
                f.updated = t;
                let id = f.id.clone();
                self.save_entity(entity)?;
                return Ok(Some(id));
            }
        }
        let id = new_id();
        e.facts.push(Fact {
            id: id.clone(),
            text: text.to_string(),
            field,
            status: Status::Inferred,
            source: Some(source),
            history: vec![],
            conflicts_with: None,
            created_by: Author::Ai,
            updated: t.clone(),
        });
        e.updated = t;
        self.save_entity(entity)?;
        Ok(Some(id))
    }

    /// Add an inferred relation, or refresh the matching one. Canon relations
    /// are never touched.
    pub fn ai_put_relation(
        &mut self,
        from: &str,
        to: &str,
        kind: &str,
        source: Source,
    ) -> Result<Option<String>> {
        let kind = normalize(kind).replace(' ', "-");
        if kind.is_empty() || from == to || !self.entities.contains_key(to) {
            return Ok(None);
        }
        let symmetric = is_symmetric(&kind);
        // The reverse direction of a symmetric relation counts as the same one.
        let reverse = self
            .entities
            .get(to)
            .and_then(|e| e.relations.iter().find(|r| r.to == from && r.kind == kind))
            .map(|r| r.id.clone());
        if symmetric && reverse.is_some() {
            return Ok(reverse);
        }
        let t = now();
        let e = self.entity_mut(from)?;
        if let Some(r) = e
            .relations
            .iter_mut()
            .find(|r| r.to == to && r.kind == kind)
        {
            if r.status == Status::Inferred {
                r.source = Some(source);
                r.updated = t;
            }
            let id = r.id.clone();
            self.save_entity(from)?;
            return Ok(Some(id));
        }
        let id = new_id();
        e.relations.push(Relation {
            id: id.clone(),
            to: to.to_string(),
            kind,
            directed: !symmetric,
            status: Status::Inferred,
            source: Some(source),
            since: None,
            created_by: Author::Ai,
            updated: t.clone(),
        });
        e.updated = t;
        self.save_entity(from)?;
        Ok(Some(id))
    }

    /// Move a plot thread's state from the text. Canon status is the author's.
    pub fn ai_touch_thread(&mut self, id: &str, scene: &str, status: ThreadStatus) -> Result<()> {
        let e = self.entity_mut(id)?;
        let t = e.thread.get_or_insert_with(ThreadData::default);
        if t.introduced_in.is_none() {
            t.introduced_in = Some(scene.to_string());
        }
        if !t.touches.iter().any(|s| s == scene) {
            t.touches.push(scene.to_string());
        }
        if !t.status_canon {
            t.status = status;
            t.resolved_in = (status == ThreadStatus::Resolved).then(|| scene.to_string());
        }
        e.updated = now();
        self.save_entity(id)
    }

    /// Forget what the model read from one scene: its inferred facts, relations,
    /// events and card. Used before a scene is read again.
    pub fn ai_forget_scene(&mut self, scene: &str) -> Result<()> {
        let ids: Vec<String> = self.entities.keys().cloned().collect();
        for id in ids {
            let Some(e) = self.entities.get_mut(&id) else {
                continue;
            };
            let (nf, nr) = (e.facts.len(), e.relations.len());
            let from_scene = |s: &Option<Source>| s.as_ref().is_some_and(|s| s.doc == scene);
            e.facts
                .retain(|f| !(f.status == Status::Inferred && from_scene(&f.source)));
            e.relations
                .retain(|r| !(r.status == Status::Inferred && from_scene(&r.source)));
            if let Some(t) = &mut e.thread {
                t.touches.retain(|s| s != scene);
            }
            if e.facts.len() != nf || e.relations.len() != nr {
                self.save_entity(&id)?;
            }
        }
        let n = self.events.len();
        self.events.retain(|e| {
            !(e.status == Status::Inferred && e.source.as_ref().is_some_and(|s| s.doc == scene))
        });
        if self.events.len() != n {
            self.save_timeline()?;
        }
        if self.scenes.remove(scene).is_some() {
            self.save_scene(scene)?;
        }
        Ok(())
    }

    /// Remove inferred entries nothing supports any more (no facts, relations,
    /// notes or fields, and not named in `still_mentioned`).
    pub fn prune_orphans(
        &mut self,
        still_mentioned: &dyn Fn(&Entity) -> bool,
    ) -> Result<Vec<String>> {
        let dead: Vec<String> = self
            .entities
            .values()
            .filter(|e| {
                e.status == Status::Inferred
                    && e.created_by == Author::Ai
                    && e.facts.is_empty()
                    && e.fields.is_empty()
                    && e.notes.is_empty()
                    && e.thread.as_ref().is_none_or(|t| t.touches.is_empty())
                    && e.relations.is_empty()
                    && !self
                        .entities
                        .values()
                        .any(|o| o.relations.iter().any(|r| r.to == e.id))
                    && !still_mentioned(e)
            })
            .map(|e| e.id.clone())
            .collect();
        for id in &dead {
            self.author_delete(id)?;
        }
        Ok(dead)
    }
}

/// Fields of an entry the author may change; `None` leaves one alone.
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EntityPatch {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub aliases: Option<Vec<String>>,
    pub fields: Option<BTreeMap<String, String>>,
    pub tags: Option<Vec<String>>,
    pub notes: Option<String>,
    pub thread_status: Option<ThreadStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Entity,
    Fact,
    Relation,
    Event,
}

/// A record in the Memory Inspector, addressed for bulk actions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ItemRef {
    pub kind: ItemKind,
    pub id: String,
    /// The owning entry, for facts and relations.
    pub entity: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(doc: &str, quote: &str) -> Source {
        Source {
            doc: doc.into(),
            para: Some("P1".into()),
            block: Some(0),
            quote: quote.into(),
        }
    }

    fn store() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let s = Store::empty(tmp.path());
        (tmp, s)
    }

    #[test]
    fn an_entry_round_trips_through_its_markdown_file() {
        let (tmp, mut s) = store();
        let e = s
            .author_create(
                "character",
                "Mara Voss",
                "Harbour pilot.\n\nSecond paragraph.",
            )
            .unwrap();
        s.author_add_fact(&e.id, "Mara has grey eyes.", Some("eye colour"), None)
            .unwrap();
        s.ai_upsert_entity("location", "Kessel", &[]).unwrap();
        let loaded = Store::load(tmp.path());
        assert_eq!(loaded.entities, s.entities);
        let path = std::fs::read_dir(tmp.path().join("bible/character"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("mara-voss--")
        );
        assert!(text.starts_with("---\n"));
        assert!(text.ends_with("Second paragraph.\n"));
        assert!(loaded.problems.is_empty());
    }

    #[test]
    fn renaming_moves_the_file_and_deleting_is_recoverable() {
        let (tmp, mut s) = store();
        let e = s.author_create("character", "Mara", "").unwrap();
        s.author_update(
            &e.id,
            &EntityPatch {
                name: Some("Mara Voss".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let files: Vec<_> = std::fs::read_dir(tmp.path().join("bible/character"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(files.len(), 1);
        assert!(
            files[0]
                .file_name()
                .to_string_lossy()
                .starts_with("mara-voss--")
        );
        let gone = s.author_delete(&e.id).unwrap();
        assert!(s.entities.is_empty());
        assert!(Store::load(tmp.path()).entities.is_empty());
        s.author_restore(gone).unwrap();
        assert_eq!(Store::load(tmp.path()).entities.len(), 1);
        assert_eq!(
            std::fs::read_dir(tmp.path().join("bible/.trash/character"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn the_ai_creates_inferred_entries_and_never_overwrites_canon() {
        let (_t, mut s) = store();
        let canon = s.author_create("character", "Mara Voss", "").unwrap();
        // The model reads the same person under a shorter name: same entry, new inferred alias.
        let id = s.ai_upsert_entity("character", "Mara", &[]).unwrap();
        assert_eq!(id, canon.id);
        let e = &s.entities[&id];
        assert_eq!(e.status, Status::Canon);
        assert_eq!(e.name, "Mara Voss");
        assert_eq!(e.aliases[0].text, "Mara");
        assert_eq!(e.aliases[0].status, Status::Inferred);
        // A new person is inferred.
        let jon = s
            .ai_upsert_entity("character", "Jon", &["Jonny".into()])
            .unwrap();
        assert_eq!(s.entities[&jon].status, Status::Inferred);
        assert_eq!(s.entities[&jon].created_by, Author::Ai);
    }

    #[test]
    fn a_thread_title_never_becomes_an_alias_of_a_person() {
        let (_t, mut s) = store();
        let bingley = s.ai_upsert_entity("character", "Mr. Bingley", &[]).unwrap();
        let t = s
            .ai_upsert_entity(
                "thread",
                "Will Mr. Bingley marry one of the Bennet daughters?",
                &[],
            )
            .unwrap();
        assert_ne!(t, bingley);
        assert!(s.entities[&bingley].aliases.is_empty());
        assert!(s.entities[&bingley].thread.is_none());
        assert!(s.entities[&t].thread.is_some());
        // A person called by a thread's exact title is still not the thread.
        let again = s
            .ai_upsert_entity(
                "thread",
                "Will Mr. Bingley marry one of the Bennet daughters?",
                &[],
            )
            .unwrap();
        assert_eq!(again, t);
        // Short forms still find the long name, within the same kind.
        assert_eq!(
            s.ai_upsert_entity("character", "Bingley", &[]).unwrap(),
            bingley
        );
    }

    #[test]
    fn a_contradicting_sentence_leaves_canon_unchanged() {
        // SPEC §17 check 9, at the store level.
        let (_t, mut s) = store();
        let e = s.author_create("character", "Mara", "").unwrap();
        let canon = s
            .author_add_fact(&e.id, "Mara has grey eyes.", Some("Eye colour"), None)
            .unwrap();
        let r = s
            .ai_put_fact(
                &e.id,
                "Mara has green eyes.",
                Some("eye colour"),
                src("S1", "green eyes"),
                false,
            )
            .unwrap();
        assert_eq!(r, None, "the model's version is dropped");
        let facts = &s.entities[&e.id].facts;
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0], canon);
        // With conflict flagging on, the inferred reading is kept, flagged, beside canon.
        let flagged = s
            .ai_put_fact(
                &e.id,
                "Mara has green eyes.",
                Some("eye colour"),
                src("S1", "green eyes"),
                true,
            )
            .unwrap()
            .unwrap();
        let facts = &s.entities[&e.id].facts;
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0], canon, "canon is untouched");
        let f = facts.iter().find(|f| f.id == flagged).unwrap();
        assert_eq!(f.status, Status::Inferred);
        assert_eq!(f.conflicts_with.as_deref(), Some(canon.id.as_str()));
    }

    #[test]
    fn a_changed_reading_updates_an_inferred_fact_and_keeps_its_history() {
        let (_t, mut s) = store();
        let id = s.ai_upsert_entity("character", "Jon", &[]).unwrap();
        let f1 = s
            .ai_put_fact(
                &id,
                "Jon is a baker.",
                Some("occupation"),
                src("S1", "baker"),
                false,
            )
            .unwrap()
            .unwrap();
        let f2 = s
            .ai_put_fact(
                &id,
                "Jon is a ferryman.",
                Some("occupation"),
                src("S2", "ferryman"),
                false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(f1, f2);
        let f = &s.entities[&id].facts[0];
        assert_eq!(f.text, "Jon is a ferryman.");
        assert_eq!(f.history.len(), 1);
        assert_eq!(f.history[0].text, "Jon is a baker.");
        assert_eq!(f.source.as_ref().unwrap().doc, "S2");
    }

    #[test]
    fn the_author_editing_a_fact_makes_it_canon_with_history() {
        let (_t, mut s) = store();
        let id = s.ai_upsert_entity("character", "Jon", &[]).unwrap();
        let f = s
            .ai_put_fact(&id, "Jon is a baker.", None, src("S1", "baker"), false)
            .unwrap()
            .unwrap();
        let edited = s
            .author_update_fact(&id, &f, "Jon is a retired baker.", None)
            .unwrap();
        assert_eq!(edited.status, Status::Canon);
        assert_eq!(edited.history.len(), 1);
        // Now the AI cannot replace it.
        let again = s
            .ai_put_fact(&id, "Jon bakes bread.", None, src("S3", "bakes"), false)
            .unwrap()
            .unwrap();
        assert_ne!(again, f, "a different statement is a new inferred fact");
        assert_eq!(
            s.entities[&id]
                .facts
                .iter()
                .find(|x| x.id == f)
                .unwrap()
                .text,
            "Jon is a retired baker."
        );
    }

    #[test]
    fn bulk_delete_refuses_canon() {
        let (_t, mut s) = store();
        let id = s.ai_upsert_entity("character", "Jon", &[]).unwrap();
        let f_inf = s
            .ai_put_fact(&id, "Jon is tall.", None, src("S1", "tall"), false)
            .unwrap()
            .unwrap();
        let f_can = s
            .author_add_fact(&id, "Jon is kind.", None, None)
            .unwrap()
            .id;
        let item = |f: &str| ItemRef {
            kind: ItemKind::Fact,
            id: f.into(),
            entity: Some(id.clone()),
        };
        assert!(s.delete_inferred(&[item(&f_can)]).is_err());
        assert_eq!(s.delete_inferred(&[item(&f_inf)]).unwrap(), 1);
        assert_eq!(s.entities[&id].facts.len(), 1);
        // Canon entries cannot be bulk-deleted either.
        let c = s.author_create("location", "Kessel", "").unwrap();
        assert!(
            s.delete_inferred(&[ItemRef {
                kind: ItemKind::Entity,
                id: c.id,
                entity: None
            }])
            .is_err()
        );
    }

    #[test]
    fn forgetting_a_scene_removes_only_its_inferred_records() {
        let (_t, mut s) = store();
        let id = s.ai_upsert_entity("character", "Jon", &[]).unwrap();
        s.ai_put_fact(&id, "Jon is tall.", None, src("S1", "tall"), false)
            .unwrap();
        s.ai_put_fact(&id, "Jon limps.", None, src("S2", "limps"), false)
            .unwrap();
        let canon = s
            .author_add_fact(&id, "Jon is kind.", None, Some(src("S1", "kind")))
            .unwrap();
        s.ai_forget_scene("S1").unwrap();
        let texts: Vec<_> = s.entities[&id]
            .facts
            .iter()
            .map(|f| f.text.as_str())
            .collect();
        assert_eq!(texts, ["Jon limps.", "Jon is kind."]);
        assert_eq!(s.entities[&id].facts[1], canon);
    }

    #[test]
    fn relations_dedupe_and_symmetric_ones_work_both_ways() {
        let (_t, mut s) = store();
        let a = s.ai_upsert_entity("character", "Ann", &[]).unwrap();
        let b = s.ai_upsert_entity("character", "Ben", &[]).unwrap();
        let r1 = s
            .ai_put_relation(&a, &b, "Sibling", src("S1", "her brother"))
            .unwrap()
            .unwrap();
        let r2 = s
            .ai_put_relation(&b, &a, "sibling", src("S2", "his sister"))
            .unwrap()
            .unwrap();
        assert_eq!(r1, r2);
        assert_eq!(s.entities[&b].relations.len(), 0);
        assert!(!s.entities[&a].relations[0].directed);
        // A directed kind keeps each direction separately.
        s.ai_put_relation(&a, &b, "mentor", src("S1", "taught"))
            .unwrap();
        s.ai_put_relation(&b, &a, "mentor", src("S1", "taught"))
            .unwrap();
        assert_eq!(s.entities[&b].relations.len(), 1);
    }

    #[test]
    fn orphans_are_pruned_but_supported_entries_stay() {
        let (_t, mut s) = store();
        let lone = s.ai_upsert_entity("character", "Nobody", &[]).unwrap();
        let backed = s.ai_upsert_entity("character", "Jon", &[]).unwrap();
        s.ai_put_fact(&backed, "Jon is tall.", None, src("S1", "tall"), false)
            .unwrap();
        let named = s.ai_upsert_entity("character", "Kate", &[]).unwrap();
        let canon = s.author_create("character", "Canon Person", "").unwrap();
        let dead = s.prune_orphans(&|e| e.name == "Kate").unwrap();
        assert_eq!(dead, vec![lone]);
        assert!(s.entities.contains_key(&backed) && s.entities.contains_key(&named));
        assert!(s.entities.contains_key(&canon.id));
    }

    #[test]
    fn parse_entity_rejects_files_without_frontmatter() {
        assert!(parse_entity("just notes").is_err());
        assert!(parse_entity("---\nid: x\n").is_err());
    }
}
