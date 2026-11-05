//! Document files: `<slug>--<id>.md` with YAML frontmatter (SPEC §13.1).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{MarginError, Result};

pub const DOC_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DocKind {
    Part,
    Chapter,
    Scene,
    Folder,
    Note,
}

impl DocKind {
    /// Containers group other nodes; they may still hold text.
    pub fn is_container(self) -> bool {
        matches!(self, DocKind::Part | DocKind::Chapter | DocKind::Folder)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "lowercase")]
pub enum AiToggle {
    #[default]
    On,
    Off,
}

fn yes() -> bool {
    true
}

/// Persisted frontmatter. Field order here is the key order in the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Frontmatter {
    #[serde(default = "default_schema")]
    pub schema_version: u32,
    #[serde(default)]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<DocKind>,
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pov: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synopsis: Option<String>,
    #[serde(default = "yes")]
    pub include: bool,
    #[serde(default)]
    pub ai: AiToggle,
    /// Unknown keys are preserved verbatim.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_norway::Value>,
}

fn default_schema() -> u32 {
    DOC_SCHEMA_VERSION
}

impl Frontmatter {
    pub fn new(id: String, kind: DocKind, title: String) -> Self {
        Frontmatter {
            schema_version: DOC_SCHEMA_VERSION,
            id,
            kind: Some(kind),
            title,
            status: None,
            pov: None,
            labels: Vec::new(),
            target: None,
            synopsis: None,
            include: true,
            ai: AiToggle::On,
            extra: BTreeMap::new(),
        }
    }

    pub fn kind(&self) -> DocKind {
        self.kind.unwrap_or(DocKind::Note)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocFile {
    pub fm: Frontmatter,
    /// Markdown body, LF line endings.
    pub body: String,
    /// True if the file had no frontmatter, or it lacked an id/kind and was filled in.
    pub needs_rewrite: bool,
}

/// Split `---\n<yaml>\n---\n<body>`. Returns `(None, text)` without frontmatter.
pub fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return (None, text);
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let l = line.trim_end_matches('\n');
        if l == "---" || l == "..." {
            let yaml = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return (Some(yaml), body);
        }
        offset += line.len();
    }
    (None, text)
}

impl DocFile {
    pub fn parse(text: &str, path_hint: &Path) -> Result<DocFile> {
        let text = text
            .strip_prefix('\u{feff}')
            .unwrap_or(text)
            .replace("\r\n", "\n");
        let (yaml, body) = split_frontmatter(&text);
        let mut needs_rewrite = false;
        let mut fm: Frontmatter = match yaml {
            Some(y) if !y.trim().is_empty() => serde_norway::from_str(y)
                .map_err(|e| MarginError::Parse(format!("{}: {e}", path_hint.display())))?,
            _ => {
                needs_rewrite = true;
                Frontmatter::new(String::new(), DocKind::Note, String::new())
            }
        };
        if fm.schema_version > DOC_SCHEMA_VERSION {
            return Err(MarginError::Invalid(format!(
                "{} has schemaVersion {}, newer than supported ({DOC_SCHEMA_VERSION})",
                path_hint.display(),
                fm.schema_version
            )));
        }
        if fm.id.trim().is_empty() {
            fm.id = ulid::Ulid::generate().to_string();
            needs_rewrite = true;
        }
        if fm.kind.is_none() {
            needs_rewrite = true;
        }
        if fm.title.trim().is_empty() {
            fm.title = title_from_path(path_hint);
        }
        Ok(DocFile {
            fm,
            body: body.to_string(),
            needs_rewrite,
        })
    }

    pub fn render(&self) -> Result<String> {
        let yaml = serde_norway::to_string(&self.fm)?;
        Ok(format!("---\n{yaml}---\n{}", self.body))
    }
}

/// Title from a file name: strip `--<id>` suffix and extension, de-slug.
pub fn title_from_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");
    let base = stem.rsplit_once("--").map_or(stem, |(a, _)| a);
    let t = base.replace(['-', '_'], " ").trim().to_string();
    if t.is_empty() {
        "Untitled".to_string()
    } else {
        t
    }
}

/// File-name slug of a title: ASCII lowercase alphanumerics and hyphens, ≤ 48 chars.
pub fn slugify(title: &str) -> String {
    let mut s = String::new();
    let mut dash = false;
    for c in title.chars() {
        let c = fold_accent(c);
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !s.is_empty() {
            s.push('-');
            dash = true;
        }
    }
    let mut s = s.trim_end_matches('-').to_string();
    if s.len() > 48 {
        s.truncate(48);
        s = s.trim_end_matches('-').to_string();
    }
    if s.is_empty() {
        "untitled".to_string()
    } else {
        s
    }
}

fn fold_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'a',
        'ç' | 'Ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => 'i',
        'ñ' | 'Ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => 'u',
        'ý' | 'ÿ' | 'Ý' => 'y',
        _ => c,
    }
}

pub fn file_name(title: &str, id: &str) -> String {
    format!("{}--{id}.md", slugify(title))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_spec_example() {
        let text = "---\nid: 01J8Z3K6Q\ntitle: The Arrival\nstatus: draft\npov: Mara\nlabels: [harbor, act-1]\ntarget: 2500\nsynopsis: Mara reaches Kessel and finds the harbor closed.\ninclude: true\nai: on\n---\nThe harbor bells had stopped ringing by the time *Gull's Wake* came round the point.\n";
        let d = DocFile::parse(text, &PathBuf::from("the-arrival--01J8Z3K6Q.md")).unwrap();
        assert_eq!(d.fm.id, "01J8Z3K6Q");
        assert_eq!(d.fm.labels, vec!["harbor", "act-1"]);
        assert_eq!(d.fm.ai, AiToggle::On);
        assert_eq!(d.fm.target, Some(2500));
        assert!(d.needs_rewrite, "kind is missing and must be filled in");
        assert!(d.body.starts_with("The harbor bells"));
    }

    #[test]
    fn render_round_trips_and_keeps_unknown_keys() {
        let text = "---\nschemaVersion: 1\nid: X\nkind: scene\ntitle: T\nlabels:\n- a\ninclude: false\nai: off\nmood: tense\n---\nBody\n";
        let d = DocFile::parse(text, &PathBuf::from("t--X.md")).unwrap();
        assert!(!d.needs_rewrite);
        assert_eq!(d.fm.ai, AiToggle::Off);
        assert!(!d.fm.include);
        let again = DocFile::parse(&d.render().unwrap(), &PathBuf::from("t--X.md")).unwrap();
        assert_eq!(again, d);
        assert!(d.render().unwrap().contains("mood: tense"));
    }

    #[test]
    fn file_without_frontmatter_gets_id() {
        let d = DocFile::parse("Just text.\n", &PathBuf::from("my-notes.md")).unwrap();
        assert!(d.needs_rewrite);
        assert_eq!(d.fm.title, "my notes");
        assert_eq!(d.body, "Just text.\n");
        assert!(!d.fm.id.is_empty());
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("The Arrival"), "the-arrival");
        assert_eq!(slugify("  Café — à la carte!  "), "cafe-a-la-carte");
        assert_eq!(slugify("???"), "untitled");
        assert!(slugify(&"long ".repeat(30)).len() <= 48);
        assert_eq!(file_name("Chapter One", "01ABC"), "chapter-one--01ABC.md");
    }
}
