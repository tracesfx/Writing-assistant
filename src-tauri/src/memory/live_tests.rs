//! Opt-in tests against a real `llama-server` (CLAUDE.md: `MARGIN_LIVE_MODEL=1`,
//! optional `MARGIN_LIVE_URL`, default `http://127.0.0.1:8080`).
//!
//! `live_ask_my_book_on_a_50k_word_manuscript` is the M4 acceptance run: the
//! model reads a 52,000-word novel, then answers questions about it with
//! citations. Run it with
//! `MARGIN_LIVE_MODEL=1 cargo test --manifest-path src-tauri/Cargo.toml --lib live_ -- --nocapture`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::ai::types::{AiStatus, ConnectionMode};
use crate::dialect;
use crate::project::manifest::{Locale, ProjectType};
use crate::story::types::{BinderTarget, NewProject};

fn live_url() -> Option<String> {
    (std::env::var("MARGIN_LIVE_MODEL").ok().as_deref() == Some("1")).then(|| {
        std::env::var("MARGIN_LIVE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into())
    })
}

fn book() -> Vec<(String, String)> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/manuscript50k/pride-and-prejudice-50k.txt"
    );
    let text = std::fs::read_to_string(path).expect("the 50k-word fixture");
    text.split("\n\n=====\n\n")
        .filter_map(|c| {
            let (head, body) = c.trim().split_once("\n\n")?;
            Some((
                head.trim().trim_end_matches('.').to_string(),
                body.to_string(),
            ))
        })
        .collect()
}

struct Live {
    _dir: tempfile::TempDir,
    ai: Arc<AiService>,
    story: Arc<StoryService>,
    mem: Arc<MemoryService>,
    scenes: Vec<(String, String)>,
}

async fn live_rig(url: &str, limit: usize) -> Live {
    let dir = tempfile::tempdir().unwrap();
    let bus = Arc::new(EventBus::new());
    let story = Arc::new(StoryService::new(bus.clone()));
    let cfg_dir = dir.path().join("cfg");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    let ai = AiService::new(
        cfg_dir.clone(),
        story.clone(),
        bus.clone(),
        tokio::runtime::Handle::current(),
    );
    let mem = MemoryService::new(
        cfg_dir,
        story.clone(),
        ai.clone(),
        bus.clone(),
        tokio::runtime::Handle::current(),
    );
    story
        .create_project(&NewProject {
            parent_dir: dir.path().to_string_lossy().into(),
            title: "Pride and Prejudice".into(),
            author: "Jane Austen".into(),
            project_type: ProjectType::Fiction,
            locale: Locale::EnUs,
        })
        .unwrap();
    let mut scenes = vec![];
    for (title, body) in book().into_iter().take(limit) {
        let id = story
            .document_create(
                DocKind::Scene,
                &title,
                &BinderTarget {
                    section: Section::Manuscript,
                    parent: None,
                    index: None,
                },
            )
            .unwrap()
            .id;
        story
            .replace_bodies(&[(id.clone(), dialect::parse(&body))], "live test")
            .unwrap();
        scenes.push((id, title));
    }
    let mut cfg = ai.config();
    cfg.mode = ConnectionMode::External;
    cfg.external_url = url.to_string();
    ai.set_config(cfg).unwrap();
    ai.connect().await.unwrap();
    for _ in 0..200 {
        if ai.status().0 == AiStatus::Ready {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(ai.status().0, AiStatus::Ready, "start a llama-server first");
    let mut mc = mem.config();
    mc.settle_seconds = 0;
    mem.set_config(mc).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let m = mem.clone();
    tokio::task::spawn_blocking(move || m.load_project().unwrap())
        .await
        .unwrap();
    Live {
        _dir: dir,
        ai,
        story,
        mem,
        scenes,
    }
}

fn norm_contains(hay: &str, needle: &str) -> bool {
    normalize(hay).contains(&normalize(needle))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn live_ask_my_book_on_a_50k_word_manuscript() {
    let Some(url) = live_url() else {
        eprintln!("skipped: set MARGIN_LIVE_MODEL=1 to run against a real model");
        return;
    };
    let r = live_rig(&url, usize::MAX).await;
    let words: u32 = r
        .mem
        .read_store(|_, i| Ok(i.docs.values().map(|d| d.words).sum()))
        .unwrap();
    eprintln!("manuscript: {} chapters, {words} words", r.scenes.len());
    assert!(words >= 50_000, "{words}");

    // A canon fact that the novel contradicts: it must survive the whole run.
    let darcy = r.mem.entity_create("character", "Mr. Darcy", "").unwrap();
    let canon = r
        .mem
        .fact_add(
            &darcy.id,
            "Mr. Darcy is a village schoolteacher.",
            Some("occupation"),
            None,
        )
        .unwrap();

    // Read the whole book, the way the idle indexer does.
    let started = Instant::now();
    let mut ticks = 0;
    while r.mem.status().pending > 0 && started.elapsed() < Duration::from_secs(40 * 60) {
        let t = Instant::now();
        r.mem.tick().await;
        ticks += 1;
        let st = r.mem.status();
        eprintln!(
            "  tick {ticks}: {:>5.1}s  pending {:>2}  entries {:>3}  {}",
            t.elapsed().as_secs_f64(),
            st.pending,
            st.entries,
            st.last_error.clone().unwrap_or_default()
        );
    }
    let cards = r.mem.scene_cards().unwrap();
    eprintln!(
        "indexed {} of {} scenes in {:.0}s",
        cards.len(),
        r.scenes.len(),
        started.elapsed().as_secs_f64()
    );
    assert!(
        cards.len() * 10 >= r.scenes.len() * 9,
        "at least 90% of the scenes were read"
    );

    let (entities, facts, rels, events, threads) = r
        .mem
        .read_store(|s, _| {
            Ok((
                s.entities.len(),
                s.entities.values().map(|e| e.facts.len()).sum::<usize>(),
                s.entities
                    .values()
                    .map(|e| e.relations.len())
                    .sum::<usize>(),
                s.events.len(),
                s.entities.values().filter(|e| e.thread.is_some()).count(),
            ))
        })
        .unwrap();
    eprintln!(
        "memory: {entities} entries, {facts} facts, {rels} relations, {events} events, {threads} threads"
    );
    assert!(entities > 20 && facts > 50);

    // The people who matter are there.
    for who in ["Bingley", "Elizabeth", "Jane", "Collins"] {
        assert!(
            r.mem
                .read_store(|s, _| Ok(s.entities.values().any(|e| e.name.contains(who))))
                .unwrap(),
            "no entry for {who}"
        );
    }

    // Canon survived (SPEC §17 check 9, with a real model).
    let d = r.mem.entity_get(&darcy.id).unwrap().entity;
    assert_eq!(d.status, Status::Canon);
    assert_eq!(d.facts.iter().find(|f| f.id == canon.id).unwrap(), &canon);
    assert!(
        !d.facts.iter().any(|f| f.status == Status::Inferred
            && f.field
                .as_deref()
                .is_some_and(|x| normalize(x) == "occupation")),
        "the model's reading of Darcy's occupation must not sit beside canon"
    );

    // Every source traces to a real passage that contains its quote (check 10).
    let (mut total, mut resolved, mut quoted) = (0, 0, 0);
    let sources: Vec<Source> = r
        .mem
        .read_store(|s, _| {
            Ok(s.entities
                .values()
                .flat_map(|e| {
                    e.facts
                        .iter()
                        .filter_map(|f| f.source.clone())
                        .chain(e.relations.iter().filter_map(|x| x.source.clone()))
                        .collect::<Vec<_>>()
                })
                .collect())
        })
        .unwrap();
    for src in &sources {
        total += 1;
        if let Some(hit) = r.mem.resolve_source(src).unwrap() {
            resolved += 1;
            if norm_contains(&hit.text, src.quote.trim_end_matches('…')) {
                quoted += 1;
            }
        }
    }
    eprintln!(
        "sources: {total} total, {resolved} resolve to a paragraph, {quoted} contain their quote"
    );
    assert_eq!(resolved, total);
    assert_eq!(quoted, total);

    // Ask my book. (question, words a correct answer's cited passages must contain, any of)
    let cases: &[(&str, &[&str])] = &[
        ("Who is Mr. Bingley's closest friend?", &["Darcy"]),
        (
            "Who is Mr. Collins and how is he related to the Bennet family?",
            &["cousin", "entail"],
        ),
        (
            "Which of the Bennet sisters does Mr. Bingley admire most?",
            &["Jane"],
        ),
        ("Where do the Lucases live?", &["Lucas Lodge"]),
        (
            "Why is Mrs. Bennet eager to meet Mr. Bingley?",
            &["daughters", "marri", "fortune"],
        ),
    ];
    let mut good = 0;
    let mut cited = 0;
    let mut cited_ok = 0;
    for (q, any_of) in cases {
        let t = Instant::now();
        let a = r.mem.ask(q).await.unwrap();
        eprintln!(
            "\nQ: {q}\nA ({:.1}s, found={}): {}",
            t.elapsed().as_secs_f64(),
            a.found,
            a.answer
        );
        for c in &a.citations {
            let text = match c.source.as_ref() {
                Some(s) => r
                    .mem
                    .resolve_source(&Source {
                        doc: s.doc.clone(),
                        para: s.para.clone(),
                        block: s.block,
                        quote: s.quote.clone(),
                    })
                    .unwrap()
                    .map(|h| h.text)
                    .unwrap_or_default(),
                None => c.text.clone(),
            };
            cited += 1;
            let relevant = any_of
                .iter()
                .any(|k| text.to_lowercase().contains(&k.to_lowercase()));
            if relevant {
                cited_ok += 1;
            }
            eprintln!(
                "   [{}] {} {} :: {}",
                c.label,
                if relevant { "✓" } else { "·" },
                c.title,
                c.text.chars().take(110).collect::<String>()
            );
        }
        let any_cited = a.citations.iter().any(|c| {
            let text = c
                .source
                .as_ref()
                .and_then(|s| {
                    r.mem
                        .resolve_source(&Source {
                            doc: s.doc.clone(),
                            para: s.para.clone(),
                            block: s.block,
                            quote: s.quote.clone(),
                        })
                        .ok()
                        .flatten()
                        .map(|h| h.text)
                })
                .unwrap_or_else(|| c.text.clone());
            any_of
                .iter()
                .any(|k| text.to_lowercase().contains(&k.to_lowercase()))
        });
        if a.found && any_cited {
            good += 1;
        }
    }
    eprintln!(
        "\nAsk my book: {good}/{} answered with a relevant cited passage; {cited_ok}/{cited} citations relevant",
        cases.len()
    );
    assert!(
        good >= 4,
        "at least four of five questions answered with correct citations"
    );

    // A question the book cannot answer says so.
    let none = r
        .mem
        .ask("What is the name of Elizabeth Bennet's pet dog?")
        .await
        .unwrap();
    eprintln!("\nQ: pet dog\nA (found={}): {}", none.found, none.answer);
    assert!(!none.found, "{none:?}");
    assert!(none.citations.is_empty());

    let _ = &r.story;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_ideas_and_phrasing_return_usable_results() {
    let Some(url) = live_url() else {
        eprintln!("skipped: set MARGIN_LIVE_MODEL=1 to run against a real model");
        return;
    };
    let r = live_rig(&url, 2).await;
    let (id, _) = r.scenes[0].clone();
    let text: String = r
        .mem
        .read_store(|_, i| {
            Ok(i.docs[&id]
                .paras
                .iter()
                .map(|p| p.text.clone())
                .collect::<Vec<_>>()
                .join("\n\n"))
        })
        .unwrap();
    let before: String = text.split("\n\n").take(6).collect::<Vec<_>>().join("\n\n");
    let ideas = r
        .mem
        .ideas(super::margin::IdeasRequest {
            doc_id: id.clone(),
            text_before: before.clone(),
            avoid: vec![],
            auto: false,
        })
        .await
        .unwrap();
    eprintln!(
        "ideas: {:#?}",
        ideas
            .ideas
            .iter()
            .map(|i| format!("{}: {}", i.kind, i.text))
            .collect::<Vec<_>>()
    );
    assert!(!ideas.ideas.is_empty(), "{ideas:?}");
    assert!(ideas.ideas.iter().all(|i| i.text.chars().count() <= 241));

    let sel = "It is a truth universally acknowledged, that a single man in possession of a good fortune must be in want of a wife.";
    let alts = r
        .mem
        .phrasing(super::margin::PhrasingRequest {
            doc_id: id,
            selected: sel.into(),
            before: String::new(),
            after: "However little known the feelings or views of such a man may be".into(),
        })
        .await
        .unwrap();
    eprintln!("phrasing: {:#?}", alts.alternatives);
    assert!(alts.alternatives.len() >= 2, "{alts:?}");
    assert!(
        alts.alternatives
            .iter()
            .all(|a| normalize(a) != normalize(sel))
    );
}

/// A quick look at what the real model makes of the first chapters.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn live_probe_extraction_of_two_chapters() {
    let Some(url) = live_url() else {
        eprintln!("skipped: set MARGIN_LIVE_MODEL=1 to run against a real model");
        return;
    };
    let r = live_rig(&url, 2).await;
    let started = Instant::now();
    for _ in 0..4 {
        r.mem.tick().await;
        if r.mem.status().pending == 0 {
            break;
        }
    }
    eprintln!(
        "read in {:.1}s; last error: {:?}",
        started.elapsed().as_secs_f64(),
        r.mem.status().last_error
    );
    if let Some(first) = r.ai.context_log.list(10).last() {
        let e = r.ai.context_log.get(&first.id).unwrap();
        eprintln!(
            "--- raw output of the first extraction ({} prompt tokens, {} out) ---
{}",
            e.timings.prompt_tokens, e.timings.output_tokens, e.raw_output
        );
    }
    r.mem
        .read_store(|s, _| {
            for e in s.entities.values() {
                eprintln!(
                    "{} [{}] aliases={:?}",
                    e.name,
                    e.kind,
                    e.aliases.iter().map(|a| &a.text).collect::<Vec<_>>()
                );
                for f in &e.facts {
                    eprintln!(
                        "    fact: {} ({:?}) <- {:?}",
                        f.text,
                        f.field,
                        f.source.as_ref().map(|x| &x.quote)
                    );
                }
                for rel in &e.relations {
                    eprintln!(
                        "    rel: {} -> {:?}",
                        rel.kind,
                        s.entities.get(&rel.to).map(|t| &t.name)
                    );
                }
            }
            for c in s.scenes.values() {
                eprintln!(
                    "card: {} | pov={:?} loc={:?} time={:?}",
                    c.summary,
                    c.pov,
                    c.location,
                    c.story_time.as_ref().map(|t| &t.label)
                );
            }
            for ev in &s.events {
                eprintln!("event: {}", ev.title);
            }
            Ok(())
        })
        .unwrap();
}
