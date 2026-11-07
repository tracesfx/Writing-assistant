//! Story Memory records (SPEC §9.3). Everything the model tracks is one of
//! these, lives in plain files in the project folder, and is reachable through
//! the Story Data Service.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

pub const SCHEMA_VERSION: u32 = 1;

/// Canon is what the author wrote or confirmed; inferred is what the model
/// read out of the text (SPEC §9.1). The AI never changes canon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Canon,
    Inferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Author {
    Author,
    Ai,
}

impl Author {
    pub fn status(self) -> Status {
        match self {
            Author::Author => Status::Canon,
            Author::Ai => Status::Inferred,
        }
    }
}

/// Where a fact, relation or event came from (SPEC §9.6: every item is
/// traceable to a passage).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    /// Document (scene) id.
    pub doc: String,
    /// Stable paragraph id, when the paragraph is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub para: Option<String>,
    /// Index of the text block when the source was recorded; a hint for jumping
    /// when the paragraph id can no longer be found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<u32>,
    pub quote: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Alias {
    pub text: String,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub at: String,
    pub text: String,
    pub status: Status,
    pub by: Author,
    /// What happened: "replaced by a newer reading", "edited", "made canon".
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    pub id: String,
    pub text: String,
    /// A named attribute ("eye colour"); facts that share one are alternatives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    /// Conflict flagging (opt-in): the canon fact this inferred fact disagrees with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflicts_with: Option<String>,
    pub created_by: Author,
    pub updated: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Relation {
    pub id: String,
    /// The entity this relation points to.
    pub to: String,
    /// `sibling`, `parent`, `ally`, `rival`, `romantic`, `member-of`, `located-in`,
    /// `owns`, `mentor`, or any custom word.
    pub kind: String,
    pub directed: bool,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// Story time the relation starts, as the author or the text words it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    pub created_by: Author,
    pub updated: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ThreadStatus {
    Open,
    Resolved,
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThreadData {
    pub status: ThreadStatus,
    /// Canon when the author set the status; the AI only moves an inferred one.
    pub status_canon: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced_in: Option<String>,
    #[serde(default)]
    pub touches: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_in: Option<String>,
}

impl Default for ThreadData {
    fn default() -> Self {
        ThreadData {
            status: ThreadStatus::Open,
            status_canon: false,
            introduced_in: None,
            touches: vec![],
            resolved_in: None,
        }
    }
}

/// A bible entry: character, location, item, … (SPEC §9.2). Stored as
/// `bible/<type>/<slug>--<id>.md`: YAML frontmatter plus the author's notes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Entity {
    pub schema_version: u32,
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<Alias>,
    /// The author's own fields (age, occupation, …). The AI never writes these.
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub facts: Vec<Fact>,
    #[serde(default)]
    pub relations: Vec<Relation>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub status: Status,
    pub created_by: Author,
    pub created: String,
    pub updated: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<ThreadData>,
    /// The author's notes: the body of the Markdown file, not part of the frontmatter.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StoryTime {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<f64>,
}

/// One AI-written card per scene (SPEC §9.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SceneCard {
    pub schema_version: u32,
    pub scene_id: String,
    pub summary: String,
    pub pov: Option<String>,
    pub tense: Option<String>,
    pub location: Option<String>,
    pub story_time: Option<StoryTime>,
    pub entities_present: Vec<String>,
    pub threads_touched: Vec<String>,
    pub metrics: BTreeMap<String, f64>,
    /// Extractors this reading asked for (`tension`), so a card the model could
    /// not answer is not read again for ever.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extractors: Vec<String>,
    /// Hash of the scene text this card was made from (dirty detection).
    pub source_hash: String,
    pub indexed_at: String,
    pub created_by: Author,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEvent {
    pub id: String,
    pub title: String,
    pub story_time: Option<StoryTime>,
    /// The scene where the reader meets the event.
    pub narrative_pos: Option<String>,
    pub entities: Vec<String>,
    pub source: Option<Source>,
    pub status: Status,
    pub created_by: Author,
    pub updated: String,
}

/// A chapter or book summary, made from the scene cards beneath it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub schema_version: u32,
    pub id: String,
    pub summary: String,
    /// Hashes of what it was made from; a change makes the summary stale.
    pub source_hashes: Vec<String>,
    pub updated: String,
}

/// `memory/settings.json`: per-project memory choices that travel with the project.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMemory {
    #[serde(default)]
    pub schema_version: u32,
    /// Documents the author excluded from memory ("Forget this document").
    #[serde(default)]
    pub excluded: Vec<String>,
}

/// Entry types (SPEC §9.2). Modules can add more through `bibleTypes`.
pub const FICTION_TYPES: &[(&str, &str)] = &[
    ("character", "Character"),
    ("location", "Location"),
    ("item", "Item"),
    ("group", "Faction or group"),
    ("event", "Event"),
    ("thread", "Plot thread"),
    ("term", "Glossary term"),
];
pub const NONFICTION_TYPES: &[(&str, &str)] = &[
    ("concept", "Concept or term"),
    ("claim", "Claim or argument"),
    ("source", "Source"),
    ("person", "Person or organisation"),
    ("outline", "Outline node"),
];
pub const COMMON_TYPES: &[(&str, &str)] = &[("style", "Style sheet")];

pub fn is_valid_type(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 40
        && t.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Relation kinds that read the same in both directions.
pub const SYMMETRIC_KINDS: &[&str] = &[
    "sibling",
    "ally",
    "rival",
    "romantic",
    "friend",
    "enemy",
    "spouse",
    "partner",
    "colleague",
];

pub fn is_symmetric(kind: &str) -> bool {
    SYMMETRIC_KINDS.contains(&kind)
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub fn new_id() -> String {
    ulid::Ulid::generate().to_string()
}

/// A lowercase file-name slug: letters and digits separated by `-`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let t = out.trim_end_matches('-');
    let t: String = t.chars().take(48).collect();
    if t.is_empty() { "untitled".into() } else { t }
}

/// Lowercase words only: for comparing names and facts.
pub fn normalize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Word-set overlap, 0.0–1.0.
pub fn similarity(a: &str, b: &str) -> f64 {
    use std::collections::HashSet;
    let na = normalize(a);
    let nb = normalize(b);
    let wa: HashSet<&str> = na.split(' ').filter(|w| !w.is_empty()).collect();
    let wb: HashSet<&str> = nb.split(' ').filter(|w| !w.is_empty()).collect();
    if wa.is_empty() || wb.is_empty() {
        return 0.0;
    }
    let inter = wa.intersection(&wb).count() as f64;
    let union = wa.union(&wb).count() as f64;
    inter / union
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_file_safe() {
        assert_eq!(slug("Mara Voss"), "mara-voss");
        assert_eq!(slug("  The Gull's Wake!  "), "the-gull-s-wake");
        assert_eq!(slug("???"), "untitled");
        assert_eq!(slug("Zoë"), "zoë");
    }

    #[test]
    fn similarity_ignores_case_and_punctuation() {
        assert!(similarity("Mara has grey eyes.", "mara has GREY eyes") > 0.99);
        assert!(similarity("Mara has grey eyes", "Mara has green eyes") < 0.8);
        assert_eq!(similarity("", "x"), 0.0);
    }

    #[test]
    fn type_names_are_checked() {
        assert!(is_valid_type("magic-system"));
        assert!(!is_valid_type("../x"));
        assert!(!is_valid_type(""));
    }
}
