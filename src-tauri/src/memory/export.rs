//! Memory export and import (SPEC §9.6): JSON (the full data model, and what
//! import reads back), CSV (one file per record type), Markdown (a readable
//! Story Bible) and GraphML (relations, for Gephi and friends).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::types::*;
use super::views::ExportFormat;
use super::{MemoryService, lock, mentions};
use crate::error::{MarginError, Result};
use crate::project::atomic::atomic_write;

pub const EXPORT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryExport {
    pub export_version: u32,
    pub exported_at: String,
    pub project: String,
    pub entities: Vec<Entity>,
    pub scenes: Vec<SceneCard>,
    pub events: Vec<TimelineEvent>,
    pub chapters: Vec<Summary>,
    pub synopsis: Option<Summary>,
    /// Scene titles by id, so the file reads without the project.
    pub scene_titles: BTreeMap<String, String>,
}

pub fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn csv_row(fields: &[String]) -> String {
    fields
        .iter()
        .map(|f| csv_field(f))
        .collect::<Vec<_>>()
        .join(",")
        + "\r\n"
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn status_word(s: Status) -> &'static str {
    match s {
        Status::Canon => "canon",
        Status::Inferred => "inferred",
    }
}

fn author_word(a: Author) -> &'static str {
    match a {
        Author::Author => "author",
        Author::Ai => "ai",
    }
}

impl MemoryService {
    pub fn export_data(&self) -> Result<MemoryExport> {
        let title = self.story.project_info()?.title;
        self.read_store(|s, i| {
            Ok(MemoryExport {
                export_version: EXPORT_VERSION,
                exported_at: now(),
                project: title,
                entities: s.entities.values().cloned().collect(),
                scenes: s.scenes.values().cloned().collect(),
                events: s.events.clone(),
                chapters: s.chapters.values().cloned().collect(),
                synopsis: s.synopsis.clone(),
                scene_titles: i
                    .layout
                    .titles
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            })
        })
    }

    /// Write an export into `dir` and return the files written.
    pub fn export(&self, dir: &Path, format: ExportFormat) -> Result<Vec<String>> {
        fs::create_dir_all(dir)?;
        let data = self.export_data()?;
        let mut written: Vec<PathBuf> = vec![];
        let mut put = |name: &str, text: String| -> Result<()> {
            let p = dir.join(name);
            atomic_write(&p, text.as_bytes())?;
            written.push(p);
            Ok(())
        };
        match format {
            ExportFormat::Json => put("story-memory.json", serde_json::to_string_pretty(&data)?)?,
            ExportFormat::Markdown => put("story-bible.md", self.markdown(&data))?,
            ExportFormat::Graphml => put("relations.graphml", graphml(&data))?,
            ExportFormat::Csv => {
                for (name, text) in self.csv_files(&data)? {
                    put(&name, text)?;
                }
            }
        }
        Ok(written
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect())
    }

    fn csv_files(&self, d: &MemoryExport) -> Result<Vec<(String, String)>> {
        let name_of = |id: &str| {
            d.entities
                .iter()
                .find(|e| e.id == id)
                .map_or_else(|| id.to_string(), |e| e.name.clone())
        };
        let scene = |id: &str| {
            d.scene_titles
                .get(id)
                .cloned()
                .unwrap_or_else(|| id.to_string())
        };
        let h = |cols: &[&str]| csv_row(&cols.iter().map(|c| c.to_string()).collect::<Vec<_>>());
        let mut ents = h(&[
            "id",
            "type",
            "name",
            "aliases",
            "status",
            "created_by",
            "tags",
            "updated",
            "notes",
        ]);
        let mut facts = h(&[
            "id",
            "entity_id",
            "entity",
            "text",
            "field",
            "status",
            "created_by",
            "scene",
            "paragraph",
            "quote",
            "updated",
        ]);
        let mut rels = h(&[
            "id",
            "from_id",
            "from",
            "to_id",
            "to",
            "kind",
            "directed",
            "status",
            "created_by",
            "scene",
            "quote",
            "since",
        ]);
        let mut threads = h(&[
            "id",
            "title",
            "status",
            "status_canon",
            "introduced_in",
            "touches",
            "resolved_in",
        ]);
        for e in &d.entities {
            let en = e;
            ents += &csv_row(&[
                en.id.clone(),
                en.kind.clone(),
                en.name.clone(),
                en.aliases
                    .iter()
                    .map(|a| a.text.clone())
                    .collect::<Vec<_>>()
                    .join("; "),
                status_word(en.status).into(),
                author_word(en.created_by).into(),
                en.tags.join("; "),
                en.updated.clone(),
                en.notes.clone(),
            ]);
            for f in &en.facts {
                facts += &csv_row(&[
                    f.id.clone(),
                    en.id.clone(),
                    en.name.clone(),
                    f.text.clone(),
                    f.field.clone().unwrap_or_default(),
                    status_word(f.status).into(),
                    author_word(f.created_by).into(),
                    f.source.as_ref().map(|s| scene(&s.doc)).unwrap_or_default(),
                    f.source
                        .as_ref()
                        .and_then(|s| s.para.clone())
                        .unwrap_or_default(),
                    f.source
                        .as_ref()
                        .map(|s| s.quote.clone())
                        .unwrap_or_default(),
                    f.updated.clone(),
                ]);
            }
            for r in &en.relations {
                rels += &csv_row(&[
                    r.id.clone(),
                    en.id.clone(),
                    en.name.clone(),
                    r.to.clone(),
                    name_of(&r.to),
                    r.kind.clone(),
                    r.directed.to_string(),
                    status_word(r.status).into(),
                    author_word(r.created_by).into(),
                    r.source.as_ref().map(|s| scene(&s.doc)).unwrap_or_default(),
                    r.source
                        .as_ref()
                        .map(|s| s.quote.clone())
                        .unwrap_or_default(),
                    r.since.clone().unwrap_or_default(),
                ]);
            }
            if let Some(t) = &en.thread {
                threads += &csv_row(&[
                    en.id.clone(),
                    en.name.clone(),
                    format!("{:?}", t.status).to_lowercase(),
                    t.status_canon.to_string(),
                    t.introduced_in.as_deref().map(scene).unwrap_or_default(),
                    t.touches
                        .iter()
                        .map(|s| scene(s))
                        .collect::<Vec<_>>()
                        .join("; "),
                    t.resolved_in.as_deref().map(scene).unwrap_or_default(),
                ]);
            }
        }
        let mut events = h(&[
            "id",
            "title",
            "story_time",
            "scene",
            "entities",
            "status",
            "created_by",
            "quote",
        ]);
        for e in &d.events {
            events += &csv_row(&[
                e.id.clone(),
                e.title.clone(),
                e.story_time
                    .as_ref()
                    .map(|t| t.label.clone())
                    .unwrap_or_default(),
                e.narrative_pos.as_deref().map(scene).unwrap_or_default(),
                e.entities
                    .iter()
                    .map(|x| name_of(x))
                    .collect::<Vec<_>>()
                    .join("; "),
                status_word(e.status).into(),
                author_word(e.created_by).into(),
                e.source
                    .as_ref()
                    .map(|s| s.quote.clone())
                    .unwrap_or_default(),
            ]);
        }
        let mut scenes = h(&[
            "scene_id",
            "scene",
            "summary",
            "pov",
            "tense",
            "location",
            "story_time",
            "entities_present",
            "indexed_at",
        ]);
        let mut metrics = h(&["scene_id", "scene", "metric", "value"]);
        for c in &d.scenes {
            scenes += &csv_row(&[
                c.scene_id.clone(),
                scene(&c.scene_id),
                c.summary.clone(),
                c.pov.clone().unwrap_or_default(),
                c.tense.clone().unwrap_or_default(),
                c.location.clone().unwrap_or_default(),
                c.story_time
                    .as_ref()
                    .map(|t| t.label.clone())
                    .unwrap_or_default(),
                c.entities_present
                    .iter()
                    .map(|x| name_of(x))
                    .collect::<Vec<_>>()
                    .join("; "),
                c.indexed_at.clone(),
            ]);
            for (k, v) in &c.metrics {
                metrics += &csv_row(&[
                    c.scene_id.clone(),
                    scene(&c.scene_id),
                    k.clone(),
                    format!("{v}"),
                ]);
            }
        }
        // Mentions are derived from the text, so they come from the live corpus.
        let mut mention_csv = h(&["entity_id", "entity", "scene_id", "scene", "count"]);
        {
            let i = lock(&self.inner);
            for e in &d.entities {
                let names = Store::names_of(e);
                for id in &i.layout.order {
                    if let Some(doc) = i.docs.get(id) {
                        let c = mentions::count_in_lower(&names, &doc.lower);
                        if c > 0 {
                            mention_csv += &csv_row(&[
                                e.id.clone(),
                                e.name.clone(),
                                id.clone(),
                                doc.title.clone(),
                                c.to_string(),
                            ]);
                        }
                    }
                }
            }
        }
        Ok(vec![
            ("entities.csv".into(), ents),
            ("facts.csv".into(), facts),
            ("relations.csv".into(), rels),
            ("threads.csv".into(), threads),
            ("events.csv".into(), events),
            ("scenes.csv".into(), scenes),
            ("metrics.csv".into(), metrics),
            ("mentions.csv".into(), mention_csv),
        ])
    }

    fn markdown(&self, d: &MemoryExport) -> String {
        let scene = |id: &str| {
            d.scene_titles
                .get(id)
                .cloned()
                .unwrap_or_else(|| id.to_string())
        };
        let name_of = |id: &str| {
            d.entities
                .iter()
                .find(|e| e.id == id)
                .map_or_else(|| "(deleted)".to_string(), |e| e.name.clone())
        };
        let mut out = format!("# Story Bible: {}\n\n", d.project);
        out.push_str("_Canon is what the author wrote or confirmed. “Inferred” items were read from the text by the local model and are marked._\n\n");
        if let Some(s) = &d.synopsis {
            out.push_str(&format!("## Synopsis\n\n{}\n\n", s.summary));
        }
        let mut kinds: Vec<&str> = d.entities.iter().map(|e| e.kind.as_str()).collect();
        kinds.sort();
        kinds.dedup();
        for kind in kinds {
            let label = FICTION_TYPES
                .iter()
                .chain(NONFICTION_TYPES)
                .chain(COMMON_TYPES)
                .find(|(k, _)| *k == kind)
                .map_or(kind, |(_, l)| *l);
            out.push_str(&format!("## {label}s\n\n"));
            let mut group: Vec<&Entity> = d.entities.iter().filter(|e| e.kind == kind).collect();
            group.sort_by_key(|e| e.name.to_lowercase());
            for e in group {
                let en = e;
                out.push_str(&format!(
                    "### {}{}\n\n",
                    en.name,
                    if en.status == Status::Inferred {
                        " _(inferred)_"
                    } else {
                        ""
                    }
                ));
                if !en.aliases.is_empty() {
                    out.push_str(&format!(
                        "Also called: {}\n\n",
                        en.aliases
                            .iter()
                            .map(|a| a.text.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                for (k, v) in &en.fields {
                    out.push_str(&format!("- **{k}:** {v}\n"));
                }
                if !en.fields.is_empty() {
                    out.push('\n');
                }
                if !e.notes.trim().is_empty() {
                    out.push_str(&format!("{}\n\n", e.notes.trim()));
                }
                for f in &en.facts {
                    let src = f
                        .source
                        .as_ref()
                        .map(|s| format!(" — {}", scene(&s.doc)))
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "- {}{}{}\n",
                        f.text,
                        if f.status == Status::Inferred {
                            " _(inferred)_"
                        } else {
                            ""
                        },
                        src
                    ));
                }
                for r in &en.relations {
                    out.push_str(&format!(
                        "- {} {}{}\n",
                        r.kind,
                        name_of(&r.to),
                        if r.status == Status::Inferred {
                            " _(inferred)_"
                        } else {
                            ""
                        }
                    ));
                }
                out.push('\n');
            }
        }
        if !d.scenes.is_empty() {
            out.push_str("## Scenes\n\n");
            let mut scenes: Vec<&SceneCard> = d.scenes.iter().collect();
            scenes.sort_by_key(|c| scene(&c.scene_id));
            for c in scenes {
                out.push_str(&format!("- **{}:** {}\n", scene(&c.scene_id), c.summary));
            }
            out.push('\n');
        }
        if !d.events.is_empty() {
            out.push_str("## Timeline\n\n");
            for e in &d.events {
                out.push_str(&format!(
                    "- {}{} ({})\n",
                    e.title,
                    e.story_time
                        .as_ref()
                        .map(|t| format!(", {}", t.label))
                        .unwrap_or_default(),
                    e.narrative_pos.as_deref().map(scene).unwrap_or_default()
                ));
            }
            out.push('\n');
        }
        out
    }

    /// Read a JSON export back. An author action: it writes canon-capable
    /// records exactly as they are in the file. Returns the number of entries.
    pub fn import_json(&self, path: &Path) -> Result<u32> {
        let data: MemoryExport = serde_json::from_slice(&fs::read(path)?)
            .map_err(|e| MarginError::Parse(format!("{}: {e}", path.display())))?;
        if data.export_version > EXPORT_VERSION {
            return Err(MarginError::Invalid(
                "this export is from a newer version of Margin".into(),
            ));
        }
        let n = data.entities.len() as u32;
        self.with_store(|s| {
            for e in &data.entities {
                let mut en = e.clone();
                en.notes = e.notes.clone();
                let id = en.id.clone();
                s.entities.insert(id.clone(), en);
                s.save_entity(&id)?;
            }
            for c in &data.scenes {
                s.scenes.insert(c.scene_id.clone(), c.clone());
                s.save_scene(&c.scene_id)?;
            }
            s.events = data.events.clone();
            s.save_timeline()?;
            for c in &data.chapters {
                s.chapters.insert(c.id.clone(), c.clone());
                s.save_chapter(&c.id)?;
            }
            s.synopsis = data.synopsis.clone();
            s.save_synopsis()
        })?;
        self.emit(
            vec![],
            vec![],
            &[
                "entities",
                "facts",
                "relations",
                "events",
                "scenes",
                "summaries",
            ],
        );
        Ok(n)
    }
}

use super::store::Store;

fn graphml(d: &MemoryExport) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<graphml xmlns=\"http://graphml.graphdrawing.org/xmlns\">\n\
  <key id=\"name\" for=\"node\" attr.name=\"name\" attr.type=\"string\"/>\n\
  <key id=\"type\" for=\"node\" attr.name=\"type\" attr.type=\"string\"/>\n\
  <key id=\"status\" for=\"node\" attr.name=\"status\" attr.type=\"string\"/>\n\
  <key id=\"kind\" for=\"edge\" attr.name=\"kind\" attr.type=\"string\"/>\n\
  <key id=\"estatus\" for=\"edge\" attr.name=\"status\" attr.type=\"string\"/>\n\
  <graph id=\"relations\" edgedefault=\"directed\">\n",
    );
    for e in &d.entities {
        let en = e;
        out.push_str(&format!(
            "    <node id=\"{}\"><data key=\"name\">{}</data><data key=\"type\">{}</data><data key=\"status\">{}</data></node>\n",
            xml(&en.id),
            xml(&en.name),
            xml(&en.kind),
            status_word(en.status)
        ));
    }
    for e in &d.entities {
        for r in &e.relations {
            if !d.entities.iter().any(|x| x.id == r.to) {
                continue;
            }
            out.push_str(&format!(
                "    <edge id=\"{}\" source=\"{}\" target=\"{}\"{}><data key=\"kind\">{}</data><data key=\"estatus\">{}</data></edge>\n",
                xml(&r.id),
                xml(&e.id),
                xml(&r.to),
                if r.directed { "" } else { " directed=\"false\"" },
                xml(&r.kind),
                status_word(r.status)
            ));
        }
    }
    out.push_str("  </graph>\n</graphml>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_only_when_needed() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("two\nlines"), "\"two\nlines\"");
    }

    #[test]
    fn graphml_is_escaped() {
        assert_eq!(xml("a<b & \"c\">"), "a&lt;b &amp; &quot;c&quot;&gt;");
    }
}
