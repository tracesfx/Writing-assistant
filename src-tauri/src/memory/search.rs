//! Retrieval over paragraphs (SPEC §9.5): lexical BM25 always, plus
//! semantic recall when an embedding model is running, fused by rank.
//!
//! Embeddings are a cache (`.margin/embeddings.json`): they are rebuilt from
//! the text and never hold anything the files do not. SPEC §4.1 names
//! SQLite + `sqlite-vec` for this; for a book (hundreds to a few thousand
//! paragraphs) a flat file and a brute-force cosine scan are as fast and need
//! no native extension (recorded in SPEC §1.2).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ai::client::{BoxFut, validate_local_url};
use crate::error::{MarginError, Result};
use crate::project::atomic::atomic_write;

// --- Lexical -------------------------------------------------------------------------

const STOP: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "of", "to", "in", "on", "at", "for", "with", "by",
    "from", "is", "are", "was", "were", "be", "been", "being", "am", "do", "does", "did", "has",
    "have", "had", "it", "its", "that", "this", "these", "those", "as", "he", "she", "they",
    "them", "his", "her", "their", "him", "i", "you", "we", "my", "me", "our", "your", "who",
    "whom", "what", "which", "when", "where", "why", "how", "not", "no", "so", "if", "then",
    "than", "there", "about", "into", "up", "out", "said", "say", "says", "will", "would", "can",
    "could", "tell",
];

fn stem(w: &str) -> String {
    let w = w.to_lowercase();
    let n = w.chars().count();
    for suf in ["ing", "ed", "es", "s"] {
        if n > suf.len() + 3 && w.ends_with(suf) {
            return w[..w.len() - suf.len()].to_string();
        }
    }
    w
}

/// Content words of a text, stemmed.
pub fn terms(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .filter(|w| !STOP.contains(&w.as_str()))
        .map(|w| stem(&w))
        .collect()
}

/// BM25 scores of `docs` for `query`, best first, zero scores dropped.
pub fn bm25(query: &str, docs: &[&str], k: usize) -> Vec<(usize, f64)> {
    let q: Vec<String> = {
        let mut seen = HashSet::new();
        terms(query)
            .into_iter()
            .filter(|t| seen.insert(t.clone()))
            .collect()
    };
    if q.is_empty() || docs.is_empty() {
        return vec![];
    }
    let toks: Vec<Vec<String>> = docs.iter().map(|d| terms(d)).collect();
    let n = docs.len() as f64;
    let avg = toks.iter().map(Vec::len).sum::<usize>() as f64 / n.max(1.0);
    let mut df: HashMap<&str, f64> = HashMap::new();
    for t in &toks {
        let uniq: HashSet<&str> = t.iter().map(String::as_str).collect();
        for u in uniq {
            *df.entry(u).or_default() += 1.0;
        }
    }
    let (k1, b) = (1.4, 0.75);
    let mut out: Vec<(usize, f64)> = toks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let len = t.len() as f64;
            let mut score = 0.0;
            for term in &q {
                let f = t.iter().filter(|x| *x == term).count() as f64;
                if f == 0.0 {
                    continue;
                }
                let d = df.get(term.as_str()).copied().unwrap_or(0.0);
                let idf = ((n - d + 0.5) / (d + 0.5) + 1.0).ln();
                score += idf * (f * (k1 + 1.0)) / (f + k1 * (1.0 - b + b * len / avg.max(1.0)));
            }
            (score > 0.0).then_some((i, score))
        })
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out.truncate(k);
    out
}

// --- Embeddings ----------------------------------------------------------------------

pub trait Embedder: Send + Sync {
    fn embed<'a>(&'a self, texts: Vec<String>) -> BoxFut<'a, Result<Vec<Vec<f32>>>>;
    fn model(&self) -> String;
}

/// `llama-server --embedding` on a loopback port, through its OpenAI-compatible
/// `/v1/embeddings`.
pub struct LlamaEmbedder {
    base: String,
    http: reqwest::Client,
    model: String,
}

impl LlamaEmbedder {
    pub fn new(url: &str, model: &str) -> Result<Self> {
        let base = validate_local_url(url)?;
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(120))
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|e| MarginError::Internal(format!("http client: {e}")))?;
        Ok(LlamaEmbedder {
            base,
            http,
            model: model.to_string(),
        })
    }

    pub async fn healthy(&self) -> bool {
        match self
            .http
            .get(format!("{}/health", self.base))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            Ok(r) => r.status().is_success(),
            Err(_) => false,
        }
    }
}

impl Embedder for LlamaEmbedder {
    fn embed<'a>(&'a self, texts: Vec<String>) -> BoxFut<'a, Result<Vec<Vec<f32>>>> {
        Box::pin(async move {
            let resp = self
                .http
                .post(format!("{}/v1/embeddings", self.base))
                .json(&json!({"input": texts, "model": "embedding"}))
                .send()
                .await
                .map_err(|e| MarginError::Internal(format!("embedding server: {e}")))?;
            if !resp.status().is_success() {
                return Err(MarginError::Internal(format!(
                    "embedding server answered {}",
                    resp.status()
                )));
            }
            let v: Value = resp
                .json()
                .await
                .map_err(|e| MarginError::Internal(format!("embedding reply: {e}")))?;
            let mut rows: Vec<(usize, Vec<f32>)> = v["data"]
                .as_array()
                .ok_or_else(|| MarginError::Internal("embedding reply without data".into()))?
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    let idx = d["index"].as_u64().map_or(i, |x| x as usize);
                    let vec = d["embedding"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_f64().map(|f| f as f32))
                                .collect()
                        })
                        .unwrap_or_default();
                    (idx, vec)
                })
                .collect();
            rows.sort_by_key(|r| r.0);
            let out: Vec<Vec<f32>> = rows.into_iter().map(|r| normalise(r.1)).collect();
            if out.len() != texts.len() || out.iter().any(Vec::is_empty) {
                return Err(MarginError::Internal(
                    "the embedding server returned the wrong number of vectors".into(),
                ));
            }
            Ok(out)
        })
    }

    fn model(&self) -> String {
        self.model.clone()
    }
}

/// Unit length, so cosine similarity is a dot product.
pub fn normalise(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        for x in &mut v {
            *x /= n;
        }
    }
    v
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    model: String,
    items: HashMap<String, (String, Vec<f32>)>,
}

/// Paragraph vectors by paragraph id, each tagged with the text hash it was made from.
#[derive(Default)]
pub struct VectorIndex {
    path: Option<PathBuf>,
    model: String,
    items: HashMap<String, (String, Vec<f32>)>,
    dirty: bool,
}

impl VectorIndex {
    pub fn open(project_root: &Path) -> Self {
        let path = project_root
            .join(crate::story::APP_DIR)
            .join("embeddings.json");
        let file: Option<VectorFile> = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        match file {
            Some(f) if f.schema_version == 1 => VectorIndex {
                path: Some(path),
                model: f.model,
                items: f.items,
                dirty: false,
            },
            _ => VectorIndex {
                path: Some(path),
                ..VectorIndex::default()
            },
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Keep vectors only for the model in use.
    pub fn use_model(&mut self, model: &str) {
        if self.model != model {
            self.items.clear();
            self.model = model.to_string();
            self.dirty = true;
        }
    }

    pub fn has(&self, id: &str, hash: &str) -> bool {
        self.items.get(id).is_some_and(|(h, _)| h == hash)
    }

    pub fn put(&mut self, id: &str, hash: &str, v: Vec<f32>) {
        self.items.insert(id.to_string(), (hash.to_string(), v));
        self.dirty = true;
    }

    /// Forget vectors of paragraphs that no longer exist.
    pub fn retain(&mut self, live: &HashSet<String>) {
        let n = self.items.len();
        self.items.retain(|k, _| live.contains(k));
        if self.items.len() != n {
            self.dirty = true;
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.dirty = true;
    }

    pub fn flush(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(p) = &self.path {
            if let Some(d) = p.parent() {
                fs::create_dir_all(d)?;
            }
            let f = VectorFile {
                schema_version: 1,
                model: self.model.clone(),
                items: self.items.clone(),
            };
            atomic_write(p, &serde_json::to_vec(&f)?)?;
        }
        self.dirty = false;
        Ok(())
    }

    /// Top `k` paragraph ids by cosine similarity to `q` (unit length).
    pub fn nearest(&self, q: &[f32], k: usize) -> Vec<(String, f32)> {
        let mut all: Vec<(String, f32)> = self
            .items
            .iter()
            .filter(|(_, (_, v))| v.len() == q.len())
            .map(|(id, (_, v))| (id.clone(), dot(q, v)))
            .collect();
        all.sort_by(|a, b| b.1.total_cmp(&a.1));
        all.truncate(k);
        all
    }
}

/// Reciprocal-rank fusion of ranked id lists.
pub fn fuse(lists: &[Vec<String>], k: usize) -> Vec<(String, f64)> {
    let mut score: HashMap<String, f64> = HashMap::new();
    for l in lists {
        for (rank, id) in l.iter().enumerate() {
            *score.entry(id.clone()).or_default() += 1.0 / (60.0 + rank as f64);
        }
    }
    let mut out: Vec<(String, f64)> = score.into_iter().collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out.truncate(k);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bm25_ranks_the_passage_that_answers_the_question() {
        let docs = [
            "The harbour bells had stopped ringing by the time the ferry came round the point.",
            "Jon was the harbour pilot for ten years and knew every rock in the channel.",
            "She bought bread and cheese at the market and walked home slowly.",
        ];
        let hits = bm25("Who is the harbour pilot?", &docs, 3);
        assert_eq!(hits[0].0, 1);
        assert!(hits.iter().all(|h| h.0 != 2), "unrelated text scores zero");
        assert!(bm25("", &docs, 3).is_empty());
        assert!(
            bm25("the of and", &docs, 3).is_empty(),
            "stop words alone match nothing"
        );
    }

    #[test]
    fn plurals_and_endings_match() {
        let hits = bm25("pilots", &["The pilot waited.", "Nothing here."], 2);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 0);
    }

    #[test]
    fn nearest_orders_by_cosine_and_the_index_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ix = VectorIndex::open(tmp.path());
        ix.use_model("m1");
        ix.put("a", "h1", normalise(vec![1.0, 0.0]));
        ix.put("b", "h2", normalise(vec![0.7, 0.7]));
        ix.put("c", "h3", normalise(vec![0.0, 1.0]));
        let near = ix.nearest(&normalise(vec![1.0, 0.1]), 2);
        assert_eq!(near[0].0, "a");
        assert_eq!(near[1].0, "b");
        ix.flush().unwrap();
        let again = VectorIndex::open(tmp.path());
        assert_eq!(again.len(), 3);
        assert!(again.has("a", "h1") && !again.has("a", "other"));
        // Another model invalidates everything.
        let mut again = again;
        again.use_model("m2");
        assert!(again.is_empty());
    }

    #[test]
    fn fusion_rewards_agreement() {
        let f = fuse(
            &[
                vec!["a".into(), "b".into(), "c".into()],
                vec!["b".into(), "d".into(), "a".into()],
            ],
            3,
        );
        assert_eq!(f[0].0, "b");
        assert_eq!(f.len(), 3);
    }

    #[test]
    fn the_embedder_refuses_non_local_servers() {
        assert!(LlamaEmbedder::new("http://example.com:8082", "m").is_err());
        assert!(LlamaEmbedder::new("http://127.0.0.1:8082", "m").is_ok());
    }
}
