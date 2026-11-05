//! Runs the bake-off in the background, keeps its state and persists results
//! to `.margin/bakeoff/<run>.json` (or the app config folder when no project
//! is open).

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

use super::AiService;
use super::bakeoff::*;
use super::types::*;
use crate::dialect::pm::{PmNode, to_pm};
use crate::error::{MarginError, Result};
use crate::events::{DomainEvent, EventBus};
use crate::project::atomic::atomic_write;
use crate::project::manifest::Section;
use crate::story::{APP_DIR, StoryService};

const DIR: &str = "bakeoff";
/// Below this the manuscript is too short and the sample text is used.
const MIN_MANUSCRIPT_WORDS: usize = 3000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub run_id: String,
    pub started_at: String,
    pub source: String,
    pub models: Vec<String>,
    pub finished: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BakeoffState {
    pub running: bool,
    pub stage: String,
    pub preset_id: Option<String>,
    pub run: Option<BakeoffRun>,
}

struct Inner {
    running: Option<CancellationToken>,
    stage: String,
    preset_id: Option<String>,
    run: Option<BakeoffRun>,
}

pub struct BakeoffService {
    ai: Arc<AiService>,
    story: Arc<StoryService>,
    bus: Arc<EventBus>,
    fallback_dir: PathBuf,
    inner: Mutex<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn paragraph_text(n: &PmNode, out: &mut String) {
    if let Some(t) = &n.text {
        out.push_str(t);
    }
    for c in n.content.iter().flatten() {
        paragraph_text(c, out);
    }
}

fn collect_paragraphs(n: &PmNode, out: &mut Vec<String>) {
    if n.kind == "paragraph" {
        let mut s = String::new();
        paragraph_text(n, &mut s);
        if !s.trim().is_empty() {
            out.push(s.trim().to_string());
        }
    } else {
        for c in n.content.iter().flatten() {
            collect_paragraphs(c, out);
        }
    }
}

fn manuscript_paragraphs(story: &StoryService) -> Vec<String> {
    fn walk(
        nodes: &[crate::story::types::BinderNode],
        story: &StoryService,
        out: &mut Vec<String>,
    ) {
        for n in nodes {
            if n.include
                && let Ok(ast) = story.document_ast(&n.id)
            {
                collect_paragraphs(&to_pm(&ast), out);
            }
            walk(&n.children, story, out);
        }
    }
    let mut out = vec![];
    if let Ok(tree) = story.binder_tree() {
        for s in tree
            .sections
            .iter()
            .filter(|s| s.section == Section::Manuscript)
        {
            walk(&s.nodes, story, &mut out);
        }
    }
    out
}

impl BakeoffService {
    pub fn new(
        ai: Arc<AiService>,
        story: Arc<StoryService>,
        bus: Arc<EventBus>,
        config_dir: PathBuf,
    ) -> Arc<Self> {
        Arc::new(BakeoffService {
            ai,
            story,
            bus,
            fallback_dir: config_dir.join(DIR),
            inner: Mutex::new(Inner {
                running: None,
                stage: String::new(),
                preset_id: None,
                run: None,
            }),
        })
    }

    fn dir(&self) -> PathBuf {
        self.story
            .project_root()
            .map(|r| r.join(APP_DIR).join(DIR))
            .unwrap_or_else(|_| self.fallback_dir.clone())
    }

    fn save(&self, run: &BakeoffRun) -> Result<()> {
        let json = serde_json::to_vec_pretty(run)?;
        atomic_write(&self.dir().join(format!("{}.json", run.run_id)), &json)?;
        Ok(())
    }

    pub fn candidates(&self) -> Vec<Candidate> {
        candidates(&self.ai.config().presets)
    }

    pub fn state(&self) -> BakeoffState {
        let i = lock(&self.inner);
        BakeoffState {
            running: i.running.is_some(),
            stage: i.stage.clone(),
            preset_id: i.preset_id.clone(),
            run: i.run.clone(),
        }
    }

    pub fn runs(&self) -> Vec<RunSummary> {
        let mut out: Vec<RunSummary> = fs::read_dir(self.dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| fs::read(e.path()).ok())
            .filter_map(|b| serde_json::from_slice::<BakeoffRun>(&b).ok())
            .map(|r| RunSummary {
                run_id: r.run_id,
                started_at: r.started_at,
                source: r.source,
                models: r.candidates.iter().map(|c| c.speed.name.clone()).collect(),
                finished: r.finished,
            })
            .collect();
        out.sort_by(|a, b| b.run_id.cmp(&a.run_id));
        out
    }

    pub fn load(&self, run_id: &str) -> Result<BakeoffRun> {
        if !run_id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(MarginError::Invalid("bad run id".into()));
        }
        let bytes = fs::read(self.dir().join(format!("{run_id}.json")))
            .map_err(|_| MarginError::NotFound(format!("bake-off run {run_id}")))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn cancel(&self) {
        if let Some(t) = lock(&self.inner).running.as_ref() {
            t.cancel();
        }
    }

    fn progress(&self, preset_id: &str, stage: &str) {
        {
            let mut i = lock(&self.inner);
            i.stage = stage.to_string();
            i.preset_id = Some(preset_id.to_string());
        }
        self.bus.publish(DomainEvent::BakeoffProgress {
            preset_id: preset_id.to_string(),
            stage: stage.to_string(),
            finished: false,
        });
    }

    /// Start a run over the chosen presets. Returns the run id at once.
    pub fn start(self: &Arc<Self>, preset_ids: Vec<String>, tune: bool) -> Result<String> {
        if lock(&self.inner).running.is_some() {
            return Err(MarginError::Invalid("a bake-off is already running".into()));
        }
        let cfg = self.ai.config();
        let presets: Vec<Preset> = preset_ids
            .iter()
            .filter_map(|id| cfg.presets.iter().find(|p| &p.id == id).cloned())
            .collect();
        if presets.is_empty() {
            return Err(MarginError::Invalid("choose at least one model".into()));
        }
        if let Some(c) = candidates(&presets).iter().find(|c| !c.available) {
            return Err(MarginError::NotFound(c.missing.clone().unwrap_or_default()));
        }
        let manuscript = manuscript_paragraphs(&self.story);
        let corpus = if manuscript
            .iter()
            .map(|p| p.split_whitespace().count())
            .sum::<usize>()
            >= MIN_MANUSCRIPT_WORDS
        {
            Corpus::from_manuscript(manuscript)
        } else {
            Corpus::sample()
        };
        let run_id = ulid::Ulid::generate().to_string();
        let run = BakeoffRun {
            schema_version: SCHEMA_VERSION,
            run_id: run_id.clone(),
            started_at: chrono::Local::now().to_rfc3339(),
            source: corpus.source.to_string(),
            finished: false,
            candidates: vec![],
            questions: vec![],
        };
        let token = CancellationToken::new();
        {
            let mut i = lock(&self.inner);
            i.running = Some(token.clone());
            i.run = Some(run.clone());
            i.stage = "starting".into();
        }
        let me = self.clone();
        let port = cfg.port;
        self.ai.handle().spawn(async move {
            me.run(run, presets, corpus, port, tune, token).await;
        });
        Ok(run_id)
    }

    async fn run(
        self: Arc<Self>,
        mut run: BakeoffRun,
        presets: Vec<Preset>,
        corpus: Corpus,
        port: u32,
        tune: bool,
        token: CancellationToken,
    ) {
        // The bake-off owns the GPU: stop whatever server Margin has running.
        self.ai.disconnect().await;
        let me = self.clone();
        let progress: Progress = Arc::new(move |id, stage| me.progress(id, stage));
        for preset in &presets {
            if token.is_cancelled() {
                break;
            }
            let c = run_candidate(&self.ai, preset, port, &corpus, tune, &progress, &token).await;
            run.candidates.push(c);
            run.questions = build_questions(&corpus, &run.candidates, seed_of(&run.run_id));
            lock(&self.inner).run = Some(run.clone());
            if let Err(e) = self.save(&run) {
                eprintln!("[margin] could not save bake-off results: {e}");
            }
        }
        run.finished = !token.is_cancelled();
        lock(&self.inner).run = Some(run.clone());
        if let Err(e) = self.save(&run) {
            eprintln!("[margin] could not save bake-off results: {e}");
        }
        {
            let mut i = lock(&self.inner);
            i.running = None;
            i.stage = if token.is_cancelled() {
                "cancelled"
            } else {
                "done"
            }
            .into();
        }
        self.bus.publish(DomainEvent::BakeoffProgress {
            preset_id: String::new(),
            stage: lock(&self.inner).stage.clone(),
            finished: true,
        });
    }

    pub fn blind_questions(&self, run_id: &str) -> Result<Vec<BlindQuestion>> {
        Ok(blind_view(&self.run_for(run_id)?))
    }

    /// Record an answer: a label, or `None` for "none of them".
    pub fn answer(&self, run_id: &str, index: u32, label: Option<String>) -> Result<()> {
        let mut run = self.run_for(run_id)?;
        let q = run
            .questions
            .iter_mut()
            .find(|q| q.index == index)
            .ok_or_else(|| MarginError::NotFound(format!("question {index}")))?;
        if let Some(l) = &label
            && !q.options.iter().any(|o| &o.label == l)
        {
            return Err(MarginError::Invalid(format!("no option {l}")));
        }
        q.answer = label;
        q.answered = true;
        self.save(&run)?;
        let mut i = lock(&self.inner);
        if i.run.as_ref().is_some_and(|r| r.run_id == run.run_id) {
            i.run = Some(run);
        }
        Ok(())
    }

    /// The scores reveal model names, so only once every question is answered.
    pub fn scores(&self, run_id: &str) -> Result<Vec<QualityScore>> {
        let run = self.run_for(run_id)?;
        if run.questions.iter().any(|q| !q.answered) {
            return Err(MarginError::Invalid(
                "answer every question to reveal which model wrote what".into(),
            ));
        }
        Ok(score(&run))
    }

    /// "Use this model": make the preset active, with its tuned `--n-cpu-moe`.
    pub fn use_model(&self, run_id: &str, preset_id: &str) -> Result<AiConfig> {
        let run = self.run_for(run_id)?;
        let result = run
            .candidates
            .iter()
            .find(|c| c.speed.preset_id == preset_id)
            .ok_or_else(|| MarginError::NotFound(preset_id.to_string()))?;
        let mut cfg = self.ai.config();
        for p in cfg.presets.iter_mut().filter(|p| p.id == preset_id) {
            if result.speed.error.is_none() {
                p.args = result.speed.args.clone();
            }
        }
        cfg.preset_id = preset_id.to_string();
        self.ai.set_config(cfg)
    }

    fn run_for(&self, run_id: &str) -> Result<BakeoffRun> {
        if let Some(r) = lock(&self.inner)
            .run
            .as_ref()
            .filter(|r| r.run_id == run_id)
        {
            return Ok(r.clone());
        }
        self.load(run_id)
    }
}

fn seed_of(run_id: &str) -> u64 {
    run_id.bytes().fold(1469598103934665603u64, |h, b| {
        (h ^ b as u64).wrapping_mul(1099511628211)
    })
}
