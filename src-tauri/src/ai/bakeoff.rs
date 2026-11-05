//! Model bake-off (SPEC §6.6): a speed test and a blind quality test.
//!
//! For each candidate the runner starts a managed server, measures VRAM,
//! cold and warm time-to-first-token, decode speed and ghost latency over 30
//! simulated pauses, and writes 10 continuations for the blind test.
//! Continuations are shown as A/B/C/D…; the label → model key stays in the
//! backend until the author has answered. Results live in `.margin/bakeoff/`.
//!
//! "Ask my book" questions (SPEC §6.6 step 3) need Story Memory and arrive
//! with M4.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::mpsc::unbounded_channel;
use tokio_util::sync::CancellationToken;

use super::client::{CompletionRequest, InferenceClient};
use super::types::*;
use super::{AiService, read_vram, template};
use crate::error::{MarginError, Result};

pub const SCHEMA_VERSION: u32 = 1;
pub const SAMPLE_TEXT: &str =
    include_str!("../../../fixtures/sample/pride-and-prejudice-excerpt.txt");

/// The ghost-text role instruction (SPEC §7.4 [1]).
pub const GHOST_ROLE: &str = "You are a silent co-writer. Continue the author's text exactly where it stops, matching their voice, tense, point of view and spelling. Output only the continuation. Never summarise, explain, or add commentary.";

/// Go/no-go thresholds for the default model (SPEC §6.6).
pub const GO_WARM_P50_MS: f64 = 350.0;
pub const GO_WARM_P95_MS: f64 = 700.0;
pub const GO_DECODE_TPS: f64 = 20.0;
pub const GO_SENTENCE_MS: f64 = 2000.0;
pub const GO_HEADROOM_MB: u32 = 600;
/// `--n-cpu-moe` is the lowest value that keeps VRAM at or under this.
pub const VRAM_BUDGET_MB: u32 = 11_468; // ~11.2 GB

const PAUSES: usize = 30;
const CUT_POINTS: usize = 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub preset_id: String,
    pub name: String,
    pub available: bool,
    /// Why it cannot run (missing binary or model file).
    pub missing: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub cut_index: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SpeedResult {
    pub preset_id: String,
    pub name: String,
    pub error: Option<String>,
    pub load_seconds: f64,
    pub vram_baseline_mb: u32,
    pub vram_used_mb: u32,
    pub vram_total_mb: u32,
    pub headroom_mb: i32,
    pub cold_prompt_tokens: u32,
    pub cold_ttft_ms: f64,
    pub warm_ttft_p50_ms: f64,
    pub warm_ttft_p95_ms: f64,
    pub ghost_total_p50_ms: f64,
    pub decode_tps: f64,
    pub sentence_p50_ms: f64,
    /// The tuned `--n-cpu-moe`, when the preset has one.
    pub n_cpu_moe: Option<u32>,
    pub args: Vec<String>,
    pub template_warning: Option<String>,
    /// Which go/no-go limits were missed. Empty = go.
    pub misses: Vec<String>,
}

impl SpeedResult {
    pub fn go(&self) -> bool {
        self.error.is_none() && self.misses.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CandidateRun {
    pub speed: SpeedResult,
    pub samples: Vec<Sample>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub index: u32,
    /// The last words before the cut.
    pub cut_text: String,
    /// Continuations in shuffled order; `preset_id` keys the label.
    pub options: Vec<QuestionOption>,
    /// The label the author picked; `None` = not answered yet.
    pub answer: Option<String>,
    pub answered: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    pub text: String,
    pub preset_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BakeoffRun {
    pub schema_version: u32,
    pub run_id: String,
    pub started_at: String,
    /// "sample" or "manuscript".
    pub source: String,
    pub finished: bool,
    pub candidates: Vec<CandidateRun>,
    pub questions: Vec<Question>,
}

/// The author's view of a question: no model identities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BlindQuestion {
    pub index: u32,
    pub cut_text: String,
    pub options: Vec<BlindOption>,
    pub answered: bool,
    pub answer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BlindOption {
    pub label: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QualityScore {
    pub preset_id: String,
    pub name: String,
    pub wins: u32,
    pub answered: u32,
}

pub fn candidates(presets: &[Preset]) -> Vec<Candidate> {
    presets
        .iter()
        .map(|p| {
            let missing = if !std::path::Path::new(&p.binary).is_file() {
                Some(format!("llama-server not found: {}", p.binary))
            } else if !std::path::Path::new(&p.model).is_file() {
                Some(format!("model file not found: {}", p.model))
            } else {
                None
            };
            Candidate {
                preset_id: p.id.clone(),
                name: p.name.clone(),
                available: missing.is_none(),
                missing,
            }
        })
        .collect()
}

// --- Statistics ---------------------------------------------------------------------

/// Nearest-rank percentile (`p` in 0–100). 0 for an empty list.
pub fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = ((p / 100.0) * v.len() as f64).ceil().max(1.0) as usize;
    v[rank.min(v.len()) - 1]
}

/// Which §6.6 limits a result misses.
pub fn go_no_go(r: &SpeedResult) -> Vec<String> {
    let mut m = vec![];
    if r.warm_ttft_p50_ms > GO_WARM_P50_MS {
        m.push(format!(
            "warm TTFT p50 {:.0} ms > {GO_WARM_P50_MS:.0} ms",
            r.warm_ttft_p50_ms
        ));
    }
    if r.warm_ttft_p95_ms > GO_WARM_P95_MS {
        m.push(format!(
            "warm TTFT p95 {:.0} ms > {GO_WARM_P95_MS:.0} ms",
            r.warm_ttft_p95_ms
        ));
    }
    if r.decode_tps < GO_DECODE_TPS {
        m.push(format!(
            "decode {:.1} tok/s < {GO_DECODE_TPS:.0}",
            r.decode_tps
        ));
    }
    if r.sentence_p50_ms > GO_SENTENCE_MS {
        m.push(format!(
            "sentence ghost {:.0} ms > {GO_SENTENCE_MS:.0} ms",
            r.sentence_p50_ms
        ));
    }
    if r.headroom_mb < GO_HEADROOM_MB as i32 {
        m.push(format!(
            "VRAM headroom {} MB < {GO_HEADROOM_MB} MB",
            r.headroom_mb
        ));
    }
    m
}

// --- Corpus -------------------------------------------------------------------------

/// The text the bake-off reads from.
pub struct Corpus {
    pub paragraphs: Vec<String>,
    pub source: &'static str,
}

impl Corpus {
    pub fn sample() -> Corpus {
        Corpus {
            paragraphs: SAMPLE_TEXT
                .split("\n\n")
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect(),
            source: "sample",
        }
    }

    pub fn from_manuscript(paragraphs: Vec<String>) -> Corpus {
        Corpus {
            paragraphs,
            source: "manuscript",
        }
    }

    pub fn word_count(&self) -> usize {
        self.paragraphs
            .iter()
            .map(|p| p.split_whitespace().count())
            .sum()
    }

    pub fn words(&self) -> Vec<&str> {
        self.paragraphs
            .iter()
            .flat_map(|p| p.split_whitespace())
            .collect()
    }
}

fn ghost_prompt(context: &str, prefix: &str) -> Prompt {
    Prompt {
        system: GHOST_ROLE.into(),
        user: context.to_string(),
        assistant_prefix: prefix.to_string(),
        sections: vec![],
    }
}

fn request(preset: &Preset, prompt: &Prompt, sampling: Sampling, slot: u32) -> CompletionRequest {
    let mut s = sampling;
    for stop in &preset.stop {
        if !s.stop.contains(stop) {
            s.stop.push(stop.clone());
        }
    }
    CompletionRequest {
        raw_prompt: template::render(preset, prompt),
        system: prompt.system.clone(),
        user: prompt.user.clone(),
        assistant_prefix: prompt.assistant_prefix.clone(),
        sampling: s,
        slot: Some(slot),
        disable_thinking: preset.thinking != ThinkingSwitch::None,
    }
}

async fn generate(client: &Arc<dyn InferenceClient>, req: CompletionRequest) -> Completion {
    let (tx, _rx) = unbounded_channel();
    client.complete(req, tx, CancellationToken::new()).await
}

/// How many words make about `target` tokens, calibrated with the server's tokenizer.
async fn words_for_tokens(client: &Arc<dyn InferenceClient>, words: &[&str], target: u32) -> usize {
    let probe = words.len().min(800);
    let tokens = client
        .tokenize(&words[..probe].join(" "))
        .await
        .unwrap_or(0);
    if tokens == 0 || probe == 0 {
        return (target as usize * 3 / 4).min(words.len());
    }
    ((target as f64 * probe as f64 / tokens as f64) as usize).clamp(1, words.len())
}

/// Speed measurements against a ready server (SPEC §6.6 step 2), without VRAM
/// and load time, which the caller owns.
pub struct Measured {
    pub cold_prompt_tokens: u32,
    pub cold_ttft_ms: f64,
    pub warm_ttft_p50_ms: f64,
    pub warm_ttft_p95_ms: f64,
    pub ghost_total_p50_ms: f64,
    pub decode_tps: f64,
    pub sentence_p50_ms: f64,
}

pub async fn measure_speed(
    client: &Arc<dyn InferenceClient>,
    preset: &Preset,
    corpus: &Corpus,
    pauses: usize,
    cancel: &CancellationToken,
) -> Result<Measured> {
    let words = corpus.words();
    if words.len() < 1500 {
        return Err(MarginError::Invalid(
            "the bake-off needs at least 1,500 words of text".into(),
        ));
    }
    // A ~6K-token scene (SPEC §6.6), leaving room for the growing prefix.
    let n = words_for_tokens(client, &words, 6000).await;
    let start = 0;
    let end = (start + n).min(words.len().saturating_sub(pauses * 12 + 80));
    let context = words[start..end].join(" ");
    let ghost = |max: u32| {
        let mut s = preset.sampling_for(Role::Ghost);
        s.max_tokens = max;
        s
    };
    let fail = |c: &Completion| c.error.clone().map(MarginError::Internal);

    // Cold: a fresh slot, the whole prompt to process.
    let cold = generate(
        client,
        request(preset, &ghost_prompt(&context, ""), ghost(1), 0),
    )
    .await;
    if let Some(e) = fail(&cold) {
        return Err(e);
    }
    let cold_ttft = cold.timings.ttft_ms.unwrap_or(cold.timings.total_ms);

    // 30 simulated pauses: the author has typed ~12 more words each time, and
    // the model continues a phrase. The prefix before the cursor is cached.
    let mut ttfts = vec![];
    let mut totals = vec![];
    for i in 1..=pauses {
        if cancel.is_cancelled() {
            return Err(MarginError::Invalid("cancelled".into()));
        }
        let prefix = words[end..end + i * 12].join(" ");
        let c = generate(
            client,
            request(preset, &ghost_prompt(&context, &prefix), ghost(16), 0),
        )
        .await;
        if let Some(e) = fail(&c) {
            return Err(e);
        }
        ttfts.push(c.timings.ttft_ms.unwrap_or(c.timings.total_ms));
        totals.push(c.timings.total_ms);
    }

    // Decode speed: a long free-running generation.
    let mut long = ghost(128);
    long.stop.clear();
    long.min_p = 0.05;
    let d = generate(
        client,
        request(
            preset,
            &ghost_prompt(&context, &words[end..end + 12].join(" ")),
            long,
            1,
        ),
    )
    .await;
    if let Some(e) = fail(&d) {
        return Err(e);
    }
    let decode_tps = d.timings.decode_tps.unwrap_or_else(|| {
        let gen_ms = d.timings.total_ms - d.timings.ttft_ms.unwrap_or(0.0);
        if gen_ms > 0.0 {
            d.timings.output_tokens as f64 / (gen_ms / 1000.0)
        } else {
            0.0
        }
    });

    // Sentence ghost: up to 40 tokens, ending at a sentence boundary.
    let mut sentence_ms = vec![];
    for i in 0..5 {
        // Keep typing from where the 30 pauses ended, so the prefix stays cached.
        let prefix = words[end..end + (pauses + i + 1) * 12].join(" ");
        let mut s = ghost(40);
        s.stop.extend([". ".into(), ".\u{201d}".into()]);
        let c = generate(
            client,
            request(preset, &ghost_prompt(&context, &prefix), s, 0),
        )
        .await;
        if let Some(e) = fail(&c) {
            return Err(e);
        }
        sentence_ms.push(c.timings.total_ms);
    }

    Ok(Measured {
        cold_prompt_tokens: cold.timings.prompt_tokens,
        cold_ttft_ms: cold_ttft,
        warm_ttft_p50_ms: percentile(&ttfts, 50.0),
        warm_ttft_p95_ms: percentile(&ttfts, 95.0),
        ghost_total_p50_ms: percentile(&totals, 50.0),
        decode_tps,
        sentence_p50_ms: percentile(&sentence_ms, 50.0),
    })
}

// --- Blind quality samples ---------------------------------------------------------

/// Sentence-end positions (in words) inside paragraphs long enough to continue.
fn cut_points(corpus: &Corpus, count: usize) -> Vec<(usize, usize)> {
    // (paragraph index, words kept from that paragraph)
    let mut spots = vec![];
    let mut words_before = 0usize;
    for (i, p) in corpus.paragraphs.iter().enumerate() {
        let w: Vec<&str> = p.split_whitespace().collect();
        if words_before > 1200 && w.len() >= 40 {
            // First sentence end after at least 20 words.
            if let Some(k) = (20..w.len() - 5).find(|&k| w[k].ends_with(['.', '!', '?'])) {
                spots.push((i, k + 1));
            }
        }
        words_before += w.len();
    }
    if spots.len() <= count {
        return spots;
    }
    (0..count).map(|j| spots[j * spots.len() / count]).collect()
}

/// A tiny deterministic shuffle (xorshift): enough to hide the label order.
pub fn shuffled<T: Clone>(items: &[T], seed: u64) -> Vec<T> {
    let mut v = items.to_vec();
    let mut x = seed | 1;
    for i in (1..v.len()).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.swap(i, (x % (i as u64 + 1)) as usize);
    }
    v
}

pub async fn generate_samples(
    client: &Arc<dyn InferenceClient>,
    preset: &Preset,
    corpus: &Corpus,
    cancel: &CancellationToken,
) -> Vec<Sample> {
    let mut out = vec![];
    for (n, (pi, kept)) in cut_points(corpus, CUT_POINTS).into_iter().enumerate() {
        if cancel.is_cancelled() {
            break;
        }
        // Context: up to ~1,500 words before the paragraph.
        let before: Vec<&str> = corpus.paragraphs[..pi]
            .iter()
            .flat_map(|p| p.split_whitespace())
            .collect();
        let from = before.len().saturating_sub(1500);
        let context = before[from..].join(" ");
        let para: Vec<&str> = corpus.paragraphs[pi].split_whitespace().collect();
        let prefix = para[..kept].join(" ");
        let mut s = preset.sampling_for(Role::Ghost);
        s.max_tokens = 40;
        s.stop.extend([". ".into(), ".\u{201d}".into()]);
        let c = generate(
            client,
            request(preset, &ghost_prompt(&context, &prefix), s, 0),
        )
        .await;
        out.push(Sample {
            cut_index: n as u32,
            text: c.text.trim().to_string(),
        });
    }
    out
}

/// Build the blind questions from every candidate's samples.
pub fn build_questions(corpus: &Corpus, runs: &[CandidateRun], seed: u64) -> Vec<Question> {
    let cuts = cut_points(corpus, CUT_POINTS);
    let labels = ["A", "B", "C", "D", "E", "F", "G", "H"];
    cuts.iter()
        .enumerate()
        .map(|(i, (pi, kept))| {
            let para: Vec<&str> = corpus.paragraphs[*pi].split_whitespace().collect();
            let from = kept.saturating_sub(60);
            let options: Vec<(String, String)> = runs
                .iter()
                .filter_map(|r| {
                    r.samples
                        .iter()
                        .find(|s| s.cut_index as usize == i && !s.text.is_empty())
                        .map(|s| (r.speed.preset_id.clone(), s.text.clone()))
                })
                .collect();
            let options = shuffled(&options, seed.wrapping_add(i as u64 * 7919));
            Question {
                index: i as u32,
                cut_text: para[from..*kept].join(" "),
                options: options
                    .into_iter()
                    .enumerate()
                    .map(|(k, (preset_id, text))| QuestionOption {
                        label: labels[k.min(labels.len() - 1)].to_string(),
                        text,
                        preset_id,
                    })
                    .collect(),
                answer: None,
                answered: false,
            }
        })
        .collect()
}

pub fn blind_view(run: &BakeoffRun) -> Vec<BlindQuestion> {
    run.questions
        .iter()
        .map(|q| BlindQuestion {
            index: q.index,
            cut_text: q.cut_text.clone(),
            options: q
                .options
                .iter()
                .map(|o| BlindOption {
                    label: o.label.clone(),
                    text: o.text.clone(),
                })
                .collect(),
            answered: q.answered,
            answer: q.answer.clone(),
        })
        .collect()
}

pub fn score(run: &BakeoffRun) -> Vec<QualityScore> {
    run.candidates
        .iter()
        .map(|c| {
            let mut wins = 0;
            let mut answered = 0;
            for q in &run.questions {
                if !q.answered {
                    continue;
                }
                // Only questions this model took part in count.
                if q.options.iter().any(|o| o.preset_id == c.speed.preset_id) {
                    answered += 1;
                }
                if let Some(label) = &q.answer
                    && q.options
                        .iter()
                        .any(|o| &o.label == label && o.preset_id == c.speed.preset_id)
                {
                    wins += 1;
                }
            }
            QualityScore {
                preset_id: c.speed.preset_id.clone(),
                name: c.speed.name.clone(),
                wins,
                answered,
            }
        })
        .collect()
}

pub fn n_cpu_moe_of(args: &[String]) -> Option<u32> {
    args.windows(2)
        .find(|w| w[0] == "--n-cpu-moe")
        .and_then(|w| w[1].parse().ok())
}

pub fn with_n_cpu_moe(preset: &Preset, n: u32) -> Preset {
    let mut p = preset.clone();
    // Placement must be exactly what we asked for: no automatic fitting.
    if !p.args.iter().any(|a| a == "--fit") {
        p.args.extend(["--fit".into(), "off".into()]);
    }
    if let Some(i) = p.args.iter().position(|a| a == "--n-cpu-moe") {
        if i + 1 < p.args.len() {
            p.args[i + 1] = n.to_string();
        }
    } else {
        p.args.extend(["--n-cpu-moe".into(), n.to_string()]);
    }
    p
}

/// Smallest `n` in `lo..=hi` for which `fits(n)` is true, assuming `fits` is
/// monotonic (more experts on the CPU always uses less VRAM). `None` if even
/// `hi` does not fit. Calls `fits` as few times as it can.
pub async fn lowest_fitting<F, Fut>(lo: u32, hi: u32, mut fits: F) -> Option<u32>
where
    F: FnMut(u32) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    if !fits(hi).await {
        return None;
    }
    let (mut l, mut h) = (lo, hi);
    while l < h {
        let mid = l + (h - l) / 2;
        if fits(mid).await {
            h = mid;
        } else {
            l = mid + 1;
        }
    }
    Some(h)
}

// --- Running a candidate -----------------------------------------------------------

pub type Progress = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Start a candidate's server and wait until it answers. Returns seconds taken.
async fn start_and_wait(
    svc: &Arc<AiService>,
    preset: &Preset,
    port: u32,
    cancel: &CancellationToken,
) -> Result<f64> {
    let t0 = Instant::now();
    svc.connect_with(ConnectionMode::Managed, preset.clone(), port, String::new())
        .await?;
    let deadline = Duration::from_secs(900);
    loop {
        if cancel.is_cancelled() {
            return Err(MarginError::Invalid("cancelled".into()));
        }
        if svc.is_ready() {
            return Ok(t0.elapsed().as_secs_f64());
        }
        // Crashing while loading (e.g. out of VRAM) restarts; one restart is enough to know.
        if svc.supervisor.restarts() >= 1
            || matches!(
                svc.supervisor.state(),
                super::supervisor::ProcState::Crashed(_)
            )
        {
            let tail = svc.supervisor.log_tail(6).join(" | ");
            return Err(MarginError::Internal(format!(
                "the server exited while loading: {tail}"
            )));
        }
        if t0.elapsed() > deadline {
            return Err(MarginError::Internal(
                "the model did not load within 15 minutes".into(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// VRAM in use after a load plus a warm-up request, so KV buffers are allocated.
async fn settled_vram(
    client: &Arc<dyn InferenceClient>,
    preset: &Preset,
    corpus: &Corpus,
) -> Option<Vram> {
    let words = corpus.words();
    let n = words_for_tokens(client, &words, 6000).await;
    let ctx = words[..n.min(words.len())].join(" ");
    let mut s = preset.sampling_for(Role::Ghost);
    s.max_tokens = 4;
    let _ = generate(client, request(preset, &ghost_prompt(&ctx, ""), s, 0)).await;
    read_vram().await
}

pub async fn run_candidate(
    svc: &Arc<AiService>,
    preset: &Preset,
    port: u32,
    corpus: &Corpus,
    tune: bool,
    progress: &Progress,
    cancel: &CancellationToken,
) -> CandidateRun {
    let baseline = read_vram().await.map_or(0, |v| v.used_mb);
    let mut result = SpeedResult {
        preset_id: preset.id.clone(),
        name: preset.name.clone(),
        error: None,
        load_seconds: 0.0,
        vram_baseline_mb: baseline,
        vram_used_mb: 0,
        vram_total_mb: 0,
        headroom_mb: 0,
        cold_prompt_tokens: 0,
        cold_ttft_ms: 0.0,
        warm_ttft_p50_ms: 0.0,
        warm_ttft_p95_ms: 0.0,
        ghost_total_p50_ms: 0.0,
        decode_tps: 0.0,
        sentence_p50_ms: 0.0,
        n_cpu_moe: n_cpu_moe_of(&preset.args),
        args: preset.args.clone(),
        template_warning: None,
        misses: vec![],
    };
    let mut samples = vec![];
    let outcome = run_candidate_inner(
        svc,
        preset,
        port,
        corpus,
        tune,
        progress,
        cancel,
        &mut result,
        &mut samples,
    )
    .await;
    if let Err(e) = outcome {
        result.error = Some(e.to_string());
    } else {
        result.misses = go_no_go(&result);
    }
    svc.disconnect().await;
    CandidateRun {
        speed: result,
        samples,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_candidate_inner(
    svc: &Arc<AiService>,
    preset: &Preset,
    port: u32,
    corpus: &Corpus,
    tune: bool,
    progress: &Progress,
    cancel: &CancellationToken,
    result: &mut SpeedResult,
    samples: &mut Vec<Sample>,
) -> Result<()> {
    let mut preset = preset.clone();
    if tune && let Some(start) = n_cpu_moe_of(&preset.args) {
        progress(
            &preset.id,
            "tuning --n-cpu-moe (lowest value that fits in VRAM)",
        );
        let tried = std::sync::Mutex::new(Vec::<(u32, bool)>::new());
        let base = preset.clone();
        let (svc2, corpus_ref, cancel2) = (svc.clone(), corpus, cancel.clone());
        let found = lowest_fitting(0, start.max(30) + 10, |n| {
            let p = with_n_cpu_moe(&base, n);
            let (svc, cancel) = (svc2.clone(), cancel2.clone());
            let progress = progress.clone();
            let tried = &tried;
            async move {
                progress(&p.id, &format!("trying --n-cpu-moe {n}"));
                let fits = match start_and_wait(&svc, &p, port, &cancel).await {
                    Ok(_) => match svc.client() {
                        Some(c) => settled_vram(&c, &p, corpus_ref)
                            .await
                            .is_some_and(|v| v.used_mb <= VRAM_BUDGET_MB),
                        None => false,
                    },
                    Err(_) => false,
                };
                svc.disconnect().await;
                if let Ok(mut t) = tried.lock() {
                    t.push((n, fits));
                }
                fits
            }
        })
        .await;
        match found {
            Some(n) => preset = with_n_cpu_moe(&preset, n),
            None => {
                return Err(MarginError::Internal(
                    "does not fit in VRAM even with every expert on the CPU".into(),
                ));
            }
        }
    }
    result.n_cpu_moe = n_cpu_moe_of(&preset.args);
    result.args = preset.args.clone();

    progress(&preset.id, "loading the model");
    result.load_seconds = start_and_wait(svc, &preset, port, cancel).await?;
    let client = svc
        .client()
        .ok_or_else(|| MarginError::Internal("lost the server connection".into()))?;
    result.template_warning = svc_caps_warning(svc);
    progress(
        &preset.id,
        "speed test: cold prompt, 30 ghost pauses, decode",
    );
    let m = measure_speed(&client, &preset, corpus, PAUSES, cancel).await?;
    result.cold_prompt_tokens = m.cold_prompt_tokens;
    result.cold_ttft_ms = m.cold_ttft_ms;
    result.warm_ttft_p50_ms = m.warm_ttft_p50_ms;
    result.warm_ttft_p95_ms = m.warm_ttft_p95_ms;
    result.ghost_total_p50_ms = m.ghost_total_p50_ms;
    result.decode_tps = m.decode_tps;
    result.sentence_p50_ms = m.sentence_p50_ms;
    if let Some(v) = read_vram().await {
        result.vram_used_mb = v.used_mb;
        result.vram_total_mb = v.total_mb;
        result.headroom_mb = v.total_mb as i32 - v.used_mb as i32;
    }
    progress(&preset.id, "writing 10 continuations for the blind test");
    *samples = generate_samples(&client, &preset, corpus, cancel).await;
    Ok(())
}

fn svc_caps_warning(svc: &Arc<AiService>) -> Option<String> {
    super::lock(&svc.conn).caps.template_warning.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::client::LlamaClient;
    use crate::ai::mock::{MockConfig, MockServer};
    use crate::ai::presets::builtin_presets;

    fn run_with(ids: &[&str]) -> BakeoffRun {
        let corpus = Corpus::sample();
        let runs: Vec<CandidateRun> = ids
            .iter()
            .map(|id| CandidateRun {
                speed: SpeedResult {
                    preset_id: (*id).into(),
                    name: format!("Model {id}"),
                    error: None,
                    load_seconds: 1.0,
                    vram_baseline_mb: 0,
                    vram_used_mb: 9000,
                    vram_total_mb: 12288,
                    headroom_mb: 3288,
                    cold_prompt_tokens: 6000,
                    cold_ttft_ms: 3000.0,
                    warm_ttft_p50_ms: 200.0,
                    warm_ttft_p95_ms: 400.0,
                    ghost_total_p50_ms: 500.0,
                    decode_tps: 40.0,
                    sentence_p50_ms: 1200.0,
                    n_cpu_moe: None,
                    args: vec![],
                    template_warning: None,
                    misses: vec![],
                },
                samples: (0..10)
                    .map(|i| Sample {
                        cut_index: i,
                        text: format!("{id} continues {i}"),
                    })
                    .collect(),
            })
            .collect();
        BakeoffRun {
            schema_version: SCHEMA_VERSION,
            run_id: "r".into(),
            started_at: "now".into(),
            source: "sample".into(),
            finished: true,
            questions: build_questions(&corpus, &runs, 42),
            candidates: runs,
        }
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let v: Vec<f64> = (1..=10).map(|x| x as f64).collect();
        assert_eq!(percentile(&v, 50.0), 5.0);
        assert_eq!(percentile(&v, 95.0), 10.0);
        assert_eq!(percentile(&[], 50.0), 0.0);
        assert_eq!(percentile(&[7.0], 95.0), 7.0);
    }

    #[test]
    fn go_no_go_names_every_miss() {
        let mut r = run_with(&["a"]).candidates.remove(0).speed;
        assert!(r.go());
        r.warm_ttft_p50_ms = 351.0;
        r.warm_ttft_p95_ms = 701.0;
        r.decode_tps = 19.0;
        r.sentence_p50_ms = 2001.0;
        r.headroom_mb = 599;
        assert_eq!(go_no_go(&r).len(), 5);
    }

    #[test]
    fn the_shuffle_is_a_deterministic_permutation() {
        let v: Vec<u32> = (0..8).collect();
        let a = shuffled(&v, 5);
        assert_eq!(a, shuffled(&v, 5));
        let mut sorted = a.clone();
        sorted.sort();
        assert_eq!(sorted, v);
        assert_ne!(a, v);
    }

    #[test]
    fn the_sample_text_yields_ten_cut_points() {
        let c = Corpus::sample();
        assert!(c.word_count() >= 6900);
        let cuts = cut_points(&c, CUT_POINTS);
        assert_eq!(cuts.len(), CUT_POINTS);
        for (pi, kept) in cuts {
            let w: Vec<&str> = c.paragraphs[pi].split_whitespace().collect();
            assert!(w[kept - 1].ends_with(['.', '!', '?']));
        }
    }

    #[test]
    fn blind_view_hides_model_identity_and_labels_are_unique() {
        let run = run_with(&["gemma", "nemo", "bonsai", "e4b"]);
        assert_eq!(run.questions.len(), 10);
        let json = serde_json::to_string(&blind_view(&run)).unwrap();
        for id in ["gemma", "nemo", "bonsai", "e4b"] {
            // The text contains "<id> continues", so check the field names instead.
            assert!(!json.contains("presetId"), "{id}");
        }
        for q in &run.questions {
            let mut labels: Vec<_> = q.options.iter().map(|o| o.label.clone()).collect();
            labels.sort();
            assert_eq!(labels, vec!["A", "B", "C", "D"]);
        }
        // Order differs between questions: not always the same model as "A".
        let firsts: std::collections::HashSet<_> = run
            .questions
            .iter()
            .map(|q| q.options[0].preset_id.clone())
            .collect();
        assert!(firsts.len() > 1);
    }

    #[test]
    fn scoring_counts_wins_per_model_from_the_hidden_key() {
        let mut run = run_with(&["a", "b"]);
        for q in run.questions.iter_mut() {
            // Always pick whichever option belongs to model "a".
            let label = q
                .options
                .iter()
                .find(|o| o.preset_id == "a")
                .unwrap()
                .label
                .clone();
            q.answer = Some(label);
            q.answered = true;
        }
        run.questions[0].answer = None; // "none of them"
        let s = score(&run);
        assert_eq!((s[0].wins, s[0].answered), (9, 10));
        assert_eq!((s[1].wins, s[1].answered), (0, 10));
    }

    #[test]
    fn n_cpu_moe_is_read_and_replaced_in_args() {
        let p = &builtin_presets()[0];
        assert_eq!(n_cpu_moe_of(&p.args), Some(12));
        assert_eq!(n_cpu_moe_of(&with_n_cpu_moe(p, 20).args), Some(20));
        // Tuned runs pin the placement: --fit off, added once.
        let tuned = with_n_cpu_moe(&with_n_cpu_moe(p, 20), 21);
        assert_eq!(tuned.args.iter().filter(|a| *a == "--fit").count(), 1);
        let bonsai = &builtin_presets()[1];
        assert_eq!(n_cpu_moe_of(&bonsai.args), None);
        assert_eq!(n_cpu_moe_of(&with_n_cpu_moe(bonsai, 4).args), Some(4));
    }

    #[tokio::test]
    async fn the_tuner_finds_the_lowest_fitting_value_in_few_loads() {
        let calls = std::sync::Mutex::new(vec![]);
        let found = lowest_fitting(0, 40, |n| {
            calls.lock().unwrap().push(n);
            async move { n >= 17 }
        })
        .await;
        assert_eq!(found, Some(17));
        assert!(
            calls.lock().unwrap().len() <= 8,
            "{:?}",
            calls.lock().unwrap()
        );
        assert_eq!(lowest_fitting(0, 10, |_| async { false }).await, None);
        assert_eq!(lowest_fitting(0, 10, |_| async { true }).await, Some(0));
    }

    #[tokio::test]
    async fn the_speed_test_runs_against_the_mock_and_reuses_one_slot_for_ghost() {
        let mock = MockServer::start(MockConfig {
            first_token_delay: Duration::from_millis(5),
            token_delay: Duration::from_millis(1),
            ..MockConfig::default()
        })
        .await;
        let client: Arc<dyn InferenceClient> = Arc::new(LlamaClient::new(&mock.url()).unwrap());
        let preset = builtin_presets()[1].clone();
        let m = measure_speed(
            &client,
            &preset,
            &Corpus::sample(),
            30,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(m.warm_ttft_p50_ms > 0.0 && m.warm_ttft_p95_ms >= m.warm_ttft_p50_ms);
        assert!(m.decode_tps > 0.0);
        let bodies = mock.state.bodies_for("/completion");
        // cold + 30 pauses + decode + 5 sentences
        assert_eq!(bodies.len(), 37);
        // Every ghost request shares the same cached prefix: only the tail differs.
        let first = bodies[1]["prompt"].as_str().unwrap();
        let last = bodies[30]["prompt"].as_str().unwrap();
        let common = first
            .chars()
            .zip(last.chars())
            .take_while(|(a, b)| a == b)
            .count();
        assert!(
            common * 100 / first.len() > 90,
            "shared prefix too short: {common}/{}",
            first.len()
        );
        assert!(bodies[1..31].iter().all(|b| b["id_slot"] == 0));
        assert_eq!(bodies[31]["id_slot"], 1);
    }

    #[tokio::test]
    async fn samples_cover_every_cut_point() {
        let mock = MockServer::start(MockConfig {
            first_token_delay: Duration::from_millis(1),
            token_delay: Duration::from_millis(1),
            ..MockConfig::default()
        })
        .await;
        let client: Arc<dyn InferenceClient> = Arc::new(LlamaClient::new(&mock.url()).unwrap());
        let s = generate_samples(
            &client,
            &builtin_presets()[1],
            &Corpus::sample(),
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(s.len(), 10);
        assert!(s.iter().all(|x| !x.text.is_empty()));
    }
}
