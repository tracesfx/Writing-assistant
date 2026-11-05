//! `project.yaml` (SPEC §13): project metadata and the binder ID tree.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

pub const PROJECT_FILE: &str = "project.yaml";
pub const PROJECT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectType {
    Fiction,
    NonFiction,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum Locale {
    #[serde(rename = "en-US")]
    EnUs,
    #[serde(rename = "en-CA")]
    EnCa,
}

/// Top-level binder sections. Each maps to a folder in the project.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "camelCase")]
pub enum Section {
    Manuscript,
    FrontMatter,
    BackMatter,
    Notes,
    Research,
}

impl Section {
    pub const ALL: [Section; 5] = [
        Section::Manuscript,
        Section::FrontMatter,
        Section::BackMatter,
        Section::Notes,
        Section::Research,
    ];

    pub fn folder(self) -> &'static str {
        match self {
            Section::Manuscript => "manuscript",
            Section::FrontMatter => "front-matter",
            Section::BackMatter => "back-matter",
            Section::Notes => "notes",
            Section::Research => "research",
        }
    }
}

pub const TRASH_FOLDER: &str = "trash";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinderEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<BinderEntry>,
}

impl BinderEntry {
    pub fn leaf(id: impl Into<String>) -> Self {
        BinderEntry {
            id: id.into(),
            children: Vec::new(),
        }
    }

    /// All ids in this subtree, pre-order.
    pub fn ids(&self) -> Vec<String> {
        let mut out = vec![self.id.clone()];
        for c in &self.children {
            out.extend(c.ids());
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrashOrigin {
    pub section: Section,
    /// Parent node id, or `None` for the section root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<BinderEntry>,
    pub from: TrashOrigin,
    pub trashed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Binder {
    #[serde(default)]
    pub manuscript: Vec<BinderEntry>,
    #[serde(default)]
    pub front_matter: Vec<BinderEntry>,
    #[serde(default)]
    pub back_matter: Vec<BinderEntry>,
    #[serde(default)]
    pub notes: Vec<BinderEntry>,
    #[serde(default)]
    pub research: Vec<BinderEntry>,
    #[serde(default)]
    pub trash: Vec<TrashEntry>,
}

impl Binder {
    pub fn section(&self, s: Section) -> &Vec<BinderEntry> {
        match s {
            Section::Manuscript => &self.manuscript,
            Section::FrontMatter => &self.front_matter,
            Section::BackMatter => &self.back_matter,
            Section::Notes => &self.notes,
            Section::Research => &self.research,
        }
    }

    pub fn section_mut(&mut self, s: Section) -> &mut Vec<BinderEntry> {
        match s {
            Section::Manuscript => &mut self.manuscript,
            Section::FrontMatter => &mut self.front_matter,
            Section::BackMatter => &mut self.back_matter,
            Section::Notes => &mut self.notes,
            Section::Research => &mut self.research,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct LabelDef {
    pub name: String,
    /// CSS colour, e.g. `#c0392b`.
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "camelCase")]
pub struct Goals {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_words: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_words: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_words: Option<u32>,
    /// ISO date `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<String>,
}

fn default_statuses() -> Vec<String> {
    ["idea", "draft", "revised", "final"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManifest {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(rename = "type")]
    pub project_type: ProjectType,
    pub locale: Locale,
    #[serde(default = "default_statuses")]
    pub statuses: Vec<String>,
    #[serde(default)]
    pub labels: Vec<LabelDef>,
    #[serde(default)]
    pub goals: Goals,
    #[serde(default)]
    pub binder: Binder,
    /// Unknown keys are preserved verbatim.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_norway::Value>,
}

impl ProjectManifest {
    pub fn new(title: &str, author: &str, project_type: ProjectType, locale: Locale) -> Self {
        ProjectManifest {
            schema_version: PROJECT_SCHEMA_VERSION,
            id: ulid::Ulid::generate().to_string(),
            title: title.to_string(),
            author: author.to_string(),
            project_type,
            locale,
            statuses: default_statuses(),
            labels: Vec::new(),
            goals: Goals::default(),
            binder: Binder::default(),
            extra: BTreeMap::new(),
        }
    }

    pub fn to_yaml(&self) -> crate::error::Result<String> {
        Ok(serde_norway::to_string(self)?)
    }

    pub fn from_yaml(s: &str) -> crate::error::Result<Self> {
        let m: ProjectManifest = serde_norway::from_str(s)?;
        if m.schema_version > PROJECT_SCHEMA_VERSION {
            return Err(crate::error::MarginError::Invalid(format!(
                "project.yaml schemaVersion {} is newer than this version of Margin supports ({})",
                m.schema_version, PROJECT_SCHEMA_VERSION
            )));
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_preserves_unknown_keys() {
        let src = "schemaVersion: 1\nid: 01J8Z3K6Q0000000000000000P\ntitle: My Novel\nauthor: Jane Doe\ntype: fiction\nlocale: en-CA\nbinder:\n  manuscript:\n  - id: A\n    children:\n    - id: B\n  trash:\n  - id: Z\n    from:\n      section: manuscript\n      parent: A\n      index: 2\n    trashedAt: 2026-11-04T10:00:00Z\ncustomKey:\n  nested: true\n";
        let m = ProjectManifest::from_yaml(src).unwrap();
        assert_eq!(m.locale, Locale::EnCa);
        assert_eq!(m.binder.manuscript[0].children[0].id, "B");
        assert_eq!(m.binder.trash[0].from.index, 2);
        assert!(m.extra.contains_key("customKey"));
        assert_eq!(m.statuses, default_statuses());
        let again = ProjectManifest::from_yaml(&m.to_yaml().unwrap()).unwrap();
        assert_eq!(again, m);
    }

    #[test]
    fn rejects_newer_schema() {
        let src = "schemaVersion: 99\nid: x\ntitle: t\ntype: fiction\nlocale: en-US\n";
        assert!(ProjectManifest::from_yaml(src).is_err());
    }
}
