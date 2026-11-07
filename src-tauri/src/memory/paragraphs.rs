//! Stable paragraph ids (SPEC §9.3, §18).
//!
//! The Markdown files hold no ids. Instead each document has a sidecar map,
//! `memory/paragraphs/<docId>.json`, listing its paragraphs in order with an id,
//! a hash and the text. After every save the map is reconciled with the new
//! text: unchanged paragraphs keep their id, edited ones are matched by text
//! similarity (so citations survive edits), and new ones get a new id. If the
//! sidecar is lost, citations degrade to the scene (and its block hint) rather
//! than breaking.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::types::{SCHEMA_VERSION, new_id, similarity};
use crate::dialect::ast::{Document, FOOTNOTE_CHAR};
use crate::error::Result;
use crate::project::atomic::atomic_write;

pub const DIR: &str = "paragraphs";
const FUZZY_MATCH: f64 = 0.6;

/// One paragraph of a document, as the memory system sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Para {
    pub id: String,
    /// Index among the document's text blocks (what `story.search` calls `block`).
    pub block: u32,
    pub hash: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParaFile {
    pub schema_version: u32,
    pub doc_id: String,
    /// Hash of all the paragraph text: unchanged by formatting-only edits.
    pub text_hash: String,
    pub paras: Vec<Para>,
}

pub fn hash_text(s: &str) -> String {
    Sha256::digest(s.as_bytes())
        .iter()
        .take(10)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The non-empty text blocks of a document with their block index.
pub fn blocks_of(doc: &Document) -> Vec<(u32, String)> {
    let mut out = vec![];
    let mut i = 0u32;
    doc.for_each_text(&mut |t| {
        let text: String = t.chars().filter(|c| *c != FOOTNOTE_CHAR).collect();
        let text = text.trim().to_string();
        if !text.is_empty() {
            out.push((i, text));
        }
        i += 1;
    });
    out
}

/// Match the document's current blocks to the previous paragraph list.
pub fn reconcile(old: &[Para], blocks: &[(u32, String)]) -> Vec<Para> {
    let hashes: Vec<String> = blocks.iter().map(|(_, t)| hash_text(t)).collect();
    let mut taken = vec![false; old.len()];
    let mut ids: Vec<Option<String>> = vec![None; blocks.len()];

    // Pass 1: identical text. Prefer the next unused paragraph in order, so
    // repeated lines ("No.") keep their own ids.
    let mut cursor = 0usize;
    for (i, h) in hashes.iter().enumerate() {
        let found = (cursor..old.len())
            .chain(0..cursor)
            .find(|&j| !taken[j] && old[j].hash == *h);
        if let Some(j) = found {
            taken[j] = true;
            ids[i] = Some(old[j].id.clone());
            cursor = j + 1;
        }
    }
    // Pass 2: edited paragraphs, matched by similarity.
    for (i, (_, text)) in blocks.iter().enumerate() {
        if ids[i].is_some() {
            continue;
        }
        let mut best: Option<(usize, f64)> = None;
        for (j, o) in old.iter().enumerate() {
            if taken[j] {
                continue;
            }
            let s = similarity(text, &o.text);
            if s >= FUZZY_MATCH && best.is_none_or(|(_, b)| s > b) {
                best = Some((j, s));
            }
        }
        if let Some((j, _)) = best {
            taken[j] = true;
            ids[i] = Some(old[j].id.clone());
        }
    }
    blocks
        .iter()
        .enumerate()
        .map(|(i, (block, text))| Para {
            id: ids[i].take().unwrap_or_else(new_id),
            block: *block,
            hash: hashes[i].clone(),
            text: text.clone(),
        })
        .collect()
}

pub fn text_hash(paras: &[Para]) -> String {
    let mut h = Sha256::new();
    for p in paras {
        h.update(p.hash.as_bytes());
        h.update(b"\n");
    }
    h.finalize()
        .iter()
        .take(10)
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub struct ParaStore {
    dir: PathBuf,
}

impl ParaStore {
    pub fn new(project_root: &Path) -> Self {
        ParaStore {
            dir: project_root.join("memory").join(DIR),
        }
    }

    fn path(&self, doc_id: &str) -> PathBuf {
        self.dir.join(format!("{doc_id}.json"))
    }

    pub fn load(&self, doc_id: &str) -> Option<ParaFile> {
        let bytes = fs::read(self.path(doc_id)).ok()?;
        serde_json::from_slice::<ParaFile>(&bytes)
            .ok()
            .filter(|f| f.schema_version == SCHEMA_VERSION && f.doc_id == doc_id)
    }

    /// Reconcile a document with its stored map and save it if anything changed.
    pub fn update(&self, doc_id: &str, doc: &Document) -> Result<ParaFile> {
        let old = self.load(doc_id);
        let blocks = blocks_of(doc);
        let paras = reconcile(old.as_ref().map_or(&[][..], |f| &f.paras), &blocks);
        let file = ParaFile {
            schema_version: SCHEMA_VERSION,
            doc_id: doc_id.to_string(),
            text_hash: text_hash(&paras),
            paras,
        };
        if old.as_ref() != Some(&file) {
            fs::create_dir_all(&self.dir)?;
            atomic_write(&self.path(doc_id), &serde_json::to_vec(&file)?)?;
        }
        Ok(file)
    }

    pub fn remove(&self, doc_id: &str) {
        let _ = fs::remove_file(self.path(doc_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect;

    fn blocks(texts: &[&str]) -> Vec<(u32, String)> {
        texts
            .iter()
            .enumerate()
            .map(|(i, t)| (i as u32, t.to_string()))
            .collect()
    }

    #[test]
    fn unchanged_paragraphs_keep_their_ids_through_edits_inserts_and_moves() {
        let first = reconcile(
            &[],
            &blocks(&[
                "The harbor bells had stopped ringing by the time the ferry came round the point.",
                "Mara counted the lamps along the quay, one by one, and found three dark.",
                "No.",
                "The harbour master did not look up from his ledger when she came in.",
            ]),
        );
        // Edit one, insert one at the top, add a second "No.".
        let second = reconcile(
            &first,
            &blocks(&[
                "A new opening line that is nothing like the others at all.",
                "The harbor bells had stopped ringing by the time the ferry came round the point.",
                "Mara counted the lamps along the quay, one by one, and found four of them dark.",
                "No.",
                "The harbour master did not look up from his ledger when she came in.",
                "No.",
            ]),
        );
        assert_eq!(second[1].id, first[0].id, "identical text keeps its id");
        assert_eq!(second[2].id, first[1].id, "a small edit keeps its id");
        assert_eq!(second[3].id, first[2].id, "the first 'No.' keeps its id");
        assert_eq!(second[4].id, first[3].id);
        assert_ne!(
            second[5].id, first[2].id,
            "the repeated line is a new paragraph"
        );
        assert!(first.iter().all(|f| f.id != second[0].id));
        let mut ids: Vec<_> = second.iter().map(|p| p.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), second.len());
    }

    #[test]
    fn block_indexes_count_every_text_block() {
        let doc = dialect::parse("# Title\n\nFirst paragraph.\n\n***\n\nSecond paragraph.\n");
        let b = blocks_of(&doc);
        assert_eq!(b.iter().map(|(i, _)| *i).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(b[2].1, "Second paragraph.");
    }

    #[test]
    fn the_sidecar_round_trips_and_formatting_does_not_change_the_text_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ParaStore::new(tmp.path());
        let a = store
            .update("DOC1", &dialect::parse("She *ran* home.\n\nHe stayed.\n"))
            .unwrap();
        assert_eq!(store.load("DOC1").unwrap(), a);
        let b = store
            .update("DOC1", &dialect::parse("She ran home.\n\nHe stayed.\n"))
            .unwrap();
        assert_eq!(a.text_hash, b.text_hash);
        assert_eq!(a.paras[0].id, b.paras[0].id);
        let c = store
            .update("DOC1", &dialect::parse("She ran home.\n\nHe left.\n"))
            .unwrap();
        assert_ne!(a.text_hash, c.text_hash);
        assert_eq!(a.paras[0].id, c.paras[0].id);
    }
}
