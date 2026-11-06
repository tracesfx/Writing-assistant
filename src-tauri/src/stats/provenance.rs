//! Provenance of accepted AI text (SPEC §7.6).
//!
//! The editor tracks which characters came from an accepted ghost suggestion
//! and saves them here as text ranges plus a hash of the document text. They
//! are never written into the Markdown. If the hash no longer matches (the file
//! was edited outside Margin) the editor drops the record and says so once.
//!
//! One file per document: `.margin/provenance/<doc-id>.json`, written atomically.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{MarginError, Result};
use crate::project::atomic::atomic_write;
use crate::story::{APP_DIR, StoryService};

pub const DIR: &str = "provenance";
const SCHEMA_VERSION: u32 = 1;
const MAX_RANGES: usize = 20_000;

/// A half-open range of UTF-16 code units in the plain text of a document
/// (paragraphs joined by a blank line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TextRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceRecord {
    pub schema_version: u32,
    pub doc_id: String,
    /// SHA-256 (lowercase hex) of the plain text the ranges refer to.
    pub text_hash: String,
    /// Sorted, non-overlapping ranges of AI-accepted text.
    pub ranges: Vec<TextRange>,
    /// Words inside those ranges when the record was saved.
    pub ai_words: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocProvenance {
    pub doc_id: String,
    pub ai_words: u32,
}

/// AI-accepted words per manuscript document, and in all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceTotals {
    pub project_ai_words: u32,
    pub docs: Vec<DocProvenance>,
}

fn check_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(MarginError::Invalid(format!("bad document id: {id}")));
    }
    Ok(())
}

fn validate(r: &ProvenanceRecord) -> Result<()> {
    check_id(&r.doc_id)?;
    if r.text_hash.len() != 64 || !r.text_hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(MarginError::Invalid("text hash must be SHA-256 hex".into()));
    }
    if r.ranges.len() > MAX_RANGES {
        return Err(MarginError::Invalid("too many provenance ranges".into()));
    }
    let mut last = 0;
    for x in &r.ranges {
        if x.start >= x.end || x.start < last {
            return Err(MarginError::Invalid(
                "provenance ranges must be sorted, non-empty and not overlapping".into(),
            ));
        }
        last = x.end;
    }
    Ok(())
}

pub fn read(dir: &Path, doc_id: &str) -> Result<Option<ProvenanceRecord>> {
    check_id(doc_id)?;
    let path = dir.join(format!("{doc_id}.json"));
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    // A file we cannot read is dropped rather than blocking the editor.
    Ok(serde_json::from_slice::<ProvenanceRecord>(&bytes)
        .ok()
        .filter(|r| r.schema_version == SCHEMA_VERSION && r.doc_id == doc_id))
}

/// Save a record. An empty record deletes the file.
pub fn write(dir: &Path, record: &ProvenanceRecord) -> Result<()> {
    validate(record)?;
    let path = dir.join(format!("{}.json", record.doc_id));
    if record.ranges.is_empty() {
        return match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        };
    }
    fs::create_dir_all(dir)?;
    let mut rec = record.clone();
    rec.schema_version = SCHEMA_VERSION;
    atomic_write(&path, &serde_json::to_vec_pretty(&rec)?)?;
    Ok(())
}

/// Every record in the directory.
pub fn read_all(dir: &Path) -> Vec<ProvenanceRecord> {
    let Ok(rd) = fs::read_dir(dir) else {
        return vec![];
    };
    rd.filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let id = name.strip_suffix(".json")?.to_string();
            read(dir, &id).ok().flatten()
        })
        .collect()
}

fn dir_of(story: &StoryService) -> Result<std::path::PathBuf> {
    Ok(story.project_root()?.join(APP_DIR).join(DIR))
}

pub fn get(story: &StoryService, doc_id: &str) -> Result<Option<ProvenanceRecord>> {
    read(&dir_of(story)?, doc_id)
}

pub fn put(story: &StoryService, record: &ProvenanceRecord) -> Result<()> {
    write(&dir_of(story)?, record)
}

/// Totals over documents that still exist and are not in the Trash.
pub fn totals(story: &StoryService) -> Result<ProvenanceTotals> {
    let mut docs: Vec<DocProvenance> = read_all(&dir_of(story)?)
        .into_iter()
        .filter(|r| {
            story
                .document_meta(&r.doc_id)
                .is_ok_and(|m| m.section.is_some())
        })
        .map(|r| DocProvenance {
            doc_id: r.doc_id,
            ai_words: r.ai_words,
        })
        .collect();
    docs.sort_by(|a, b| a.doc_id.cmp(&b.doc_id));
    Ok(ProvenanceTotals {
        project_ai_words: docs.iter().map(|d| d.ai_words).sum(),
        docs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, ranges: &[(u32, u32)], words: u32) -> ProvenanceRecord {
        ProvenanceRecord {
            schema_version: SCHEMA_VERSION,
            doc_id: id.into(),
            text_hash: "ab".repeat(32),
            ranges: ranges
                .iter()
                .map(|&(start, end)| TextRange { start, end })
                .collect(),
            ai_words: words,
        }
    }

    #[test]
    fn a_record_round_trips_and_an_empty_one_deletes_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("provenance");
        assert_eq!(read(&dir, "DOC1").unwrap(), None);
        let r = rec("DOC1", &[(3, 9), (20, 25)], 3);
        write(&dir, &r).unwrap();
        assert_eq!(read(&dir, "DOC1").unwrap(), Some(r));
        write(&dir, &rec("DOC1", &[], 0)).unwrap();
        assert_eq!(read(&dir, "DOC1").unwrap(), None);
        // Deleting what is not there is fine.
        write(&dir, &rec("DOC1", &[], 0)).unwrap();
    }

    #[test]
    fn bad_records_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        for bad in [
            rec("../evil", &[(1, 2)], 1),
            rec("", &[(1, 2)], 1),
            rec("D", &[(5, 5)], 0),
            rec("D", &[(5, 9), (8, 12)], 1),
            rec("D", &[(9, 12), (1, 2)], 1),
        ] {
            assert!(write(dir, &bad).is_err(), "{bad:?}");
        }
        let mut short = rec("D", &[(1, 2)], 1);
        short.text_hash = "abc".into();
        assert!(write(dir, &short).is_err());
        assert!(read(dir, "../x").is_err());
    }

    #[test]
    fn unreadable_or_foreign_files_read_as_no_record() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::write(dir.join("D1.json"), b"{ not json").unwrap();
        assert_eq!(read(dir, "D1").unwrap(), None);
        // A record filed under the wrong id is ignored.
        write(dir, &rec("D2", &[(1, 2)], 1)).unwrap();
        fs::rename(dir.join("D2.json"), dir.join("D3.json")).unwrap();
        assert_eq!(read(dir, "D3").unwrap(), None);
    }

    #[test]
    fn read_all_skips_junk() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write(dir, &rec("A1", &[(1, 4)], 2)).unwrap();
        write(dir, &rec("B2", &[(1, 4)], 5)).unwrap();
        fs::write(dir.join("notes.txt"), b"x").unwrap();
        fs::write(dir.join("C3.json"), b"junk").unwrap();
        let mut all = read_all(dir);
        all.sort_by(|a, b| a.doc_id.cmp(&b.doc_id));
        assert_eq!(all.iter().map(|r| r.ai_words).collect::<Vec<_>>(), [2, 5]);
    }
}
