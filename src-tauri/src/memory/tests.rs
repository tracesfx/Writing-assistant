//! Story Memory end to end against the mock model server (no GPU):
//! indexing, canon (SPEC §17 check 9), rebuild and export (check 11), Ask
//! with citations (check 10), Ideas and Phrasing.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::export::MemoryExport;
use super::margin::{IdeasRequest, PhrasingRequest};
use super::*;
use crate::ai::mock::{MockConfig, MockServer};
use crate::ai::types::{AiStatus, ConnectionMode, Role};
use crate::dialect;
use crate::project::manifest::{Locale, ProjectType};
use crate::story::types::NewProject;

const SCENE_ONE: &str = "Mara Voss stepped off the ferry at Kessel. Her brother Jon waited on the quay.\n\nJon had grey eyes, and he had been the harbour pilot for ten years.\n\n\"The harbour is closed,\" said Jon. Nobody knew why.\n";
const SCENE_TWO: &str = "The next morning Mara walked the length of the quay and counted four dark lamps. Jon followed her without a word.\n\nAt the customs shed a clerk named Orrin Pike refused to open the ledger until noon.\n";

fn extraction_json() -> String {
    json!({
        "summary": "Mara arrives at Kessel and learns the harbour is closed.",
        "pov": "Mara", "tense": "past", "location": "Kessel", "storyTime": "evening",
        "entities": [
            {"name": "Mara Voss", "type": "character", "aliases": ["Mara"]},
            {"name": "Jon", "type": "character", "aliases": []},
            {"name": "Kessel", "type": "location", "aliases": []}
        ],
        "facts": [
            {"entity": "Jon", "field": "eye colour", "text": "Jon has grey eyes.", "para": 2, "quote": "grey eyes"},
            {"entity": "Jon", "field": "occupation", "text": "Jon has been the harbour pilot for ten years.", "para": 2, "quote": "harbour pilot for ten years"}
        ],
        "relations": [{"from": "Jon", "to": "Mara Voss", "type": "sibling", "para": 1, "quote": "Her brother Jon"}],
        "events": [{"title": "Mara arrives at Kessel", "storyTime": "evening", "entities": ["Mara Voss"], "para": 1}],
        "threads": [{"title": "Why the harbour is closed", "status": "open"}]
    })
    .to_string()
}

fn extraction_two_json() -> String {
    json!({
        "summary": "Mara counts the dark lamps and meets a customs clerk.",
        "pov": "Mara", "tense": "past", "location": "the quay", "storyTime": "next morning",
        "entities": [
            {"name": "Mara", "type": "character", "aliases": []},
            {"name": "Orrin Pike", "type": "character", "aliases": []}
        ],
        "facts": [{"entity": "Orrin Pike", "field": "occupation", "text": "Orrin Pike is a customs clerk.", "para": 2, "quote": "a clerk named Orrin Pike"}],
        "relations": [], "events": [], "threads": []
    })
    .to_string()
}

fn prompt_of(body: &Value) -> String {
    body["prompt"].as_str().unwrap_or_default().to_string()
}

/// The mock model: replies depend on which task the prompt is.
fn responder() -> crate::ai::mock::Responder {
    Arc::new(|body: &Value| {
        let p = prompt_of(body);
        let reply = if p.contains("Paragraphs:") {
            if p.contains("Orrin Pike") {
                extraction_two_json()
            } else {
                extraction_json()
            }
        } else if p.contains("short chapter summary") || p.contains("short book summary") {
            "Mara arrives at Kessel, finds the harbour closed, and begins to look for the reason."
                .to_string()
        } else if p.contains("Evidence:") {
            // Cite the first evidence item that mentions the pilot.
            let n = p
                .lines()
                .find(|l| l.starts_with('[') && l.contains("harbour pilot"))
                .and_then(|l| l[1..].split(']').next().and_then(|n| n.parse::<u64>().ok()))
                .unwrap_or(1);
            json!({"found": true, "answer": format!("Jon has been the harbour pilot for ten years [{n}]."), "citations": [n]}).to_string()
        } else if p.contains("alternative wordings") || p.contains("ways to word") {
            json!({"alternatives": ["Jon had worked the harbour for ten years.", "For a decade Jon piloted the harbour.", "Jon had grey eyes, and he had been the harbour pilot for ten years.", "Ten years a pilot, Jon knew the harbour."]}).to_string()
        } else if p.contains("ideas") {
            json!({"ideas": [
                {"type": "stakes", "text": "The closed harbour traps Mara on Kessel with no way home.", "refs": [3]},
                {"type": "question", "text": "Why does Jon not say who closed the harbour?", "refs": []},
                {"type": "bogus", "text": "This type is not allowed and must be dropped.", "refs": []}
            ]}).to_string()
        } else {
            "{}".to_string()
        };
        vec![reply]
    })
}

/// `memory.updated` as seen on the bus: entity ids, scene ids, kinds.
type Update = (Vec<String>, Vec<String>, Vec<String>);

struct Rig {
    dir: tempfile::TempDir,
    story: Arc<StoryService>,
    ai: Arc<AiService>,
    mem: Arc<MemoryService>,
    mock: MockServer,
    s1: String,
    s2: String,
    updates: Arc<Mutex<Vec<Update>>>,
}

async fn rig() -> Rig {
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
    let updates: Arc<Mutex<Vec<_>>> = Arc::default();
    let u2 = updates.clone();
    bus.subscribe(move |e| {
        if let DomainEvent::MemoryUpdated {
            entity_ids,
            scene_ids,
            kinds,
        } = &e.event
        {
            u2.lock()
                .unwrap()
                .push((entity_ids.clone(), scene_ids.clone(), kinds.clone()));
        }
    });
    story
        .create_project(&NewProject {
            parent_dir: dir.path().to_string_lossy().into(),
            title: "The Salt Road".into(),
            author: "J".into(),
            project_type: ProjectType::Fiction,
            locale: Locale::EnCa,
        })
        .unwrap();
    let tree = story.binder_tree().unwrap();
    let ms = tree
        .sections
        .iter()
        .find(|s| s.section == Section::Manuscript)
        .unwrap();
    let chapter = ms.nodes[0].id.clone();
    let s1 = ms.nodes[0].children[0].id.clone();
    let s2 = story
        .document_create(
            DocKind::Scene,
            "The quay",
            &crate::story::types::BinderTarget {
                section: Section::Manuscript,
                parent: Some(chapter),
                index: None,
            },
        )
        .unwrap()
        .id;
    story
        .replace_bodies(
            &[
                (s1.clone(), dialect::parse(SCENE_ONE)),
                (s2.clone(), dialect::parse(SCENE_TWO)),
            ],
            "test",
        )
        .unwrap();

    let mock = MockServer::start(MockConfig {
        responder: Some(responder()),
        ..MockConfig::default()
    })
    .await;
    let mut cfg = ai.config();
    cfg.mode = ConnectionMode::External;
    cfg.external_url = mock.url();
    ai.set_config(cfg).unwrap();
    ai.connect().await.unwrap();
    for _ in 0..100 {
        if ai.status().0 == AiStatus::Ready {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(ai.status().0, AiStatus::Ready);
    let mut mc = mem.config();
    mc.settle_seconds = 0;
    mem.set_config(mc).unwrap();
    let r = Rig {
        dir,
        story,
        ai,
        mem,
        mock,
        s1,
        s2,
        updates,
    };
    r.reload().await;
    r
}

impl Rig {
    /// Let background loading finish, then load the project deterministically.
    async fn reload(&self) {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let m = self.mem.clone();
        tokio::task::spawn_blocking(move || m.load_project().unwrap())
            .await
            .unwrap();
    }

    async fn set_text(&self, doc: &str, md: &str) {
        self.story
            .replace_bodies(&[(doc.to_string(), dialect::parse(md))], "test")
            .unwrap();
        self.reload().await;
    }

    /// Run the indexer until nothing is pending.
    async fn index_all(&self) {
        for _ in 0..8 {
            self.mem.tick().await;
            if self.mem.status().pending == 0 {
                break;
            }
        }
        assert_eq!(self.mem.status().pending, 0, "{:?}", self.mem.status());
    }

    fn entity(&self, name: &str) -> Entity {
        self.mem
            .read_store(|s, _| Ok(s.find_by_name(name, None).cloned()))
            .unwrap()
            .unwrap_or_else(|| panic!("no entry {name}"))
    }

    fn completions(&self) -> usize {
        self.mock.state.bodies_for("/completion").len()
    }
}

#[tokio::test]
async fn paragraph_ids_exist_before_any_model_runs_and_survive_edits() {
    let r = rig().await;
    let first = r
        .mem
        .read_store(|_, i| Ok(i.docs[&r.s1].paras.clone()))
        .unwrap();
    assert_eq!(first.len(), 3);
    assert_eq!(
        r.completions(),
        0,
        "no model call is needed for paragraph ids"
    );
    // Edit paragraph 2 slightly and add one at the end.
    let md = SCENE_ONE.replace("ten years", "eleven years")
        + "\nA new last paragraph that is quite different.\n";
    r.set_text(&r.s1, &md).await;
    let after = r
        .mem
        .read_store(|_, i| Ok(i.docs[&r.s1].paras.clone()))
        .unwrap();
    assert_eq!(after.len(), 4);
    assert_eq!(after[0].id, first[0].id);
    assert_eq!(after[1].id, first[1].id, "an edited paragraph keeps its id");
    assert_eq!(after[2].id, first[2].id);
    assert!(first.iter().all(|f| f.id != after[3].id));
    // The sidecar is a plain file in the project.
    assert!(
        r.dir
            .path()
            .join("The Salt Road/memory/paragraphs")
            .join(format!("{}.json", r.s1))
            .exists()
    );
}

#[tokio::test]
async fn idle_indexing_reads_dirty_scenes_and_fills_the_bible() {
    let r = rig().await;
    assert_eq!(r.mem.status().pending, 2);
    r.index_all().await;
    let st = r.mem.status();
    assert_eq!(st.state, IndexState::UpToDate);
    let mara = r.entity("Mara Voss");
    assert_eq!(mara.status, Status::Inferred);
    assert!(mara.aliases.iter().any(|a| a.text == "Mara"));
    let jon = r.entity("Jon");
    assert_eq!(jon.facts.len(), 2);
    let eyes = jon
        .facts
        .iter()
        .find(|f| f.field.as_deref() == Some("eye colour"))
        .unwrap();
    // Traceable: the source is a real paragraph of the scene (SPEC §17 check 10).
    let hit = r
        .mem
        .resolve_source(eyes.source.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(hit.doc, r.s1);
    assert!(hit.text.contains("grey eyes"));
    assert!(
        r.entity("Orrin Pike").facts[0]
            .text
            .contains("customs clerk")
    );
    let cards = r.mem.scene_cards().unwrap();
    assert_eq!(cards.len(), 2);
    assert!(cards[0].card.summary.contains("harbour is closed"));
    assert_eq!(r.mem.timeline_events().unwrap().len(), 1);
    assert_eq!(r.mem.threads_list(None).unwrap().len(), 1);
    assert_eq!(r.mem.relations_graph().unwrap().edges.len(), 1);
    // Mentions are derived from the text; Mara shows up in both scenes.
    let m = r.mem.mentions_by_entity(&mara.id).unwrap();
    assert_eq!(m.len(), 2);
    assert!(m[0].first.is_some());
    // Everything went through the context log with its labelled sections.
    let log = r.ai.context_log.list(20);
    assert_eq!(log.iter().filter(|e| e.role == Role::Extraction).count(), 2);
    let entry =
        r.ai.context_log
            .get(&log.iter().find(|e| e.role == Role::Extraction).unwrap().id)
            .unwrap();
    assert!(
        entry
            .sections
            .iter()
            .any(|s| s.label.starts_with("[5] Scene text"))
    );
    // memory.updated fired with the scenes.
    assert!(
        r.updates
            .lock()
            .unwrap()
            .iter()
            .any(|(_, s, k)| s.contains(&r.s1) && k.contains(&"facts".to_string()))
    );
    // With every scene read, the idle pass writes the chapter summary once.
    r.mem.tick().await;
    let chapter = r
        .mem
        .read_store(|s, _| Ok(s.chapters.values().next().cloned()))
        .unwrap()
        .expect("a chapter summary");
    assert!(chapter.summary.starts_with("Mara arrives"));
    assert_eq!(chapter.source_hashes.len(), 2);
    // Nothing is stale now, so further heartbeats make no model call.
    let n = r.completions();
    r.mem.tick().await;
    r.mem.tick().await;
    assert_eq!(r.completions(), n);
}

#[tokio::test]
async fn canon_survives_a_contradicting_sentence_and_reindexing() {
    // SPEC §17 check 9.
    let r = rig().await;
    r.index_all().await;
    let jon = r.entity("Jon");
    let canon = r
        .mem
        .fact_add(&jon.id, "Jon has blue eyes.", Some("Eye colour"), None)
        .unwrap();
    // The text now contradicts it ("grey eyes" is what the mock model reads) and is re-indexed.
    r.set_text(&r.s1, &SCENE_ONE.replace("grey eyes", "green eyes"))
        .await;
    assert_eq!(
        r.mem.status().pending,
        1,
        "only the edited scene is dirty again"
    );
    r.index_all().await;
    let jon = r.entity("Jon");
    let kept = jon.facts.iter().find(|f| f.id == canon.id).unwrap();
    assert_eq!(kept, &canon, "canon is untouched");
    assert!(
        !jon.facts.iter().any(|f| f.text.contains("grey eyes")),
        "the contradicting reading is not recorded"
    );
    // The explicit rebuild also leaves canon alone.
    r.mem.rebuild_all().unwrap();
    r.index_all().await;
    assert_eq!(
        r.entity("Jon")
            .facts
            .iter()
            .find(|f| f.id == canon.id)
            .unwrap(),
        &canon
    );
}

#[tokio::test]
async fn a_scene_with_ai_off_or_forgotten_is_not_read() {
    let r = rig().await;
    r.story
        .document_update_meta(
            &r.s2,
            &crate::story::types::DocMetaPatch {
                ai: Some(AiToggle::Off),
                ..Default::default()
            },
        )
        .unwrap();
    r.reload().await;
    assert_eq!(r.mem.status().pending, 1, "AI-off scenes are left out");
    r.index_all().await;
    assert!(
        r.mem
            .read_store(|s, _| Ok(s.find_by_name("Orrin Pike", None).is_none()))
            .unwrap()
    );
    // "Forget this document" removes what was read from it, and keeps it out.
    r.mem.forget_document(&r.s1, true).unwrap();
    assert!(
        r.mem
            .read_store(|s, _| Ok(s.scenes.is_empty() && s.find_by_name("Jon", None).is_none()))
            .unwrap()
    );
    assert_eq!(r.mem.status().pending, 0);
    assert!(
        r.mem
            .search_text("harbour pilot", 5)
            .unwrap()
            .iter()
            .all(|h| h.doc != r.s1)
    );
    r.mem.forget_document(&r.s1, false).unwrap();
    assert_eq!(r.mem.status().pending, 1);
}

#[tokio::test]
async fn the_author_edits_the_bible_and_inferred_items_become_canon_on_request() {
    let r = rig().await;
    r.index_all().await;
    let jon = r.entity("Jon");
    // Make one inferred fact canon; the other stays inferred.
    let f = jon.facts[0].clone();
    r.mem
        .make_canon(&[store::ItemRef {
            kind: ItemKind::Fact,
            id: f.id.clone(),
            entity: Some(jon.id.clone()),
        }])
        .unwrap();
    let jon = r.entity("Jon");
    assert_eq!(
        jon.facts.iter().find(|x| x.id == f.id).unwrap().status,
        Status::Canon
    );
    assert_eq!(
        jon.facts
            .iter()
            .filter(|x| x.status == Status::Inferred)
            .count(),
        1
    );
    assert_eq!(
        jon.status,
        Status::Canon,
        "confirming a fact confirms the entry"
    );
    // Bulk delete of inferred records works, and refuses canon.
    let rows = r.mem.inspector_rows().unwrap();
    let inferred: Vec<_> = rows
        .iter()
        .filter(|x| x.kind == "fact" && x.status == Some(Status::Inferred))
        .filter_map(|x| x.item.clone())
        .collect();
    assert!(!inferred.is_empty());
    assert!(
        r.mem
            .delete_inferred(&[store::ItemRef {
                kind: ItemKind::Fact,
                id: f.id.clone(),
                entity: Some(jon.id.clone())
            }])
            .is_err()
    );
    let n = r.mem.delete_inferred(&inferred).unwrap();
    assert_eq!(n as usize, inferred.len());
    // The inspector lists every kind of record.
    let kinds: std::collections::HashSet<_> = r
        .mem
        .inspector_rows()
        .unwrap()
        .into_iter()
        .map(|x| x.kind)
        .collect();
    for k in [
        "entity", "fact", "relation", "event", "thread", "scene", "mention", "metric",
    ] {
        assert!(kinds.contains(k), "missing {k} rows: {kinds:?}");
    }
    // Delete and undo an entry.
    let gone = r.mem.entity_delete(&jon.id).unwrap();
    assert!(r.mem.entity_get(&jon.id).is_err());
    r.mem.entity_restore(gone).unwrap();
    assert!(r.mem.entity_get(&jon.id).is_ok());
}

#[tokio::test]
async fn rebuilding_the_caches_reproduces_the_same_entries_and_relations() {
    // SPEC §17 check 11, first half: `.margin/` is only a cache.
    let r = rig().await;
    r.index_all().await;
    let before = r.mem.export_data().unwrap();
    std::fs::remove_dir_all(r.dir.path().join("The Salt Road/.margin")).ok();
    std::fs::create_dir_all(r.dir.path().join("The Salt Road/.margin")).unwrap();
    r.reload().await;
    let after = r.mem.export_data().unwrap();
    assert_eq!(before.entities, after.entities);
    assert_eq!(before.scenes, after.scenes);
    assert_eq!(before.events, after.events);
    assert_eq!(
        r.mem.status().pending,
        0,
        "scene cards live in plain files, so nothing is re-read"
    );
}

#[tokio::test]
async fn the_json_export_round_trips() {
    // SPEC §17 check 11, second half.
    let r = rig().await;
    r.index_all().await;
    let jon = r.entity("Jon");
    r.mem
        .entity_update(
            &jon.id,
            &store::EntityPatch {
                notes: Some("Never sleeps.\n\nSecond note.".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let out = r.dir.path().join("export");
    let files = r.mem.export(&out, ExportFormat::Json).unwrap();
    assert!(files[0].ends_with("story-memory.json"));
    let exported: MemoryExport =
        serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
    assert!(
        exported
            .entities
            .iter()
            .any(|e| e.notes.contains("Second note"))
    );
    // Wipe the bible and memory folders, reload, import.
    let proj = r.dir.path().join("The Salt Road");
    std::fs::remove_dir_all(proj.join("bible")).unwrap();
    std::fs::remove_dir_all(proj.join("memory/scenes")).unwrap();
    std::fs::remove_file(proj.join("memory/timeline.json")).ok();
    r.reload().await;
    assert_eq!(r.mem.entities_list(None, None).unwrap().len(), 0);
    let n = r.mem.import_json(std::path::Path::new(&files[0])).unwrap();
    assert!(n >= 4);
    let again = r.mem.export_data().unwrap();
    assert_eq!(again.entities, exported.entities);
    assert_eq!(again.scenes, exported.scenes);
    assert_eq!(again.events, exported.events);
}

#[tokio::test]
async fn csv_markdown_and_graphml_exports_have_the_records() {
    let r = rig().await;
    r.index_all().await;
    let out = r.dir.path().join("export2");
    let csv = r.mem.export(&out, ExportFormat::Csv).unwrap();
    assert_eq!(csv.len(), 8);
    let facts = std::fs::read_to_string(out.join("facts.csv")).unwrap();
    assert!(facts.starts_with("id,entity_id,entity,text,field,status"));
    assert!(facts.contains("Jon has grey eyes."));
    assert!(facts.contains("inferred"));
    assert!(
        std::fs::read_to_string(out.join("mentions.csv"))
            .unwrap()
            .contains("Mara Voss")
    );
    let md =
        std::fs::read_to_string(&r.mem.export(&out, ExportFormat::Markdown).unwrap()[0]).unwrap();
    assert!(md.starts_with("# Story Bible: The Salt Road"));
    assert!(md.contains("### Jon _(inferred)_"));
    assert!(md.contains("sibling Mara Voss"));
    let g =
        std::fs::read_to_string(&r.mem.export(&out, ExportFormat::Graphml).unwrap()[0]).unwrap();
    assert!(g.contains("<graphml") && g.contains("<edge") && g.contains("sibling"));
}

#[tokio::test]
async fn ask_answers_with_citations_that_point_at_the_text_and_admits_when_it_cannot() {
    let r = rig().await;
    r.index_all().await;
    let a = r.mem.ask("Who is the harbour pilot?").await.unwrap();
    assert!(a.found, "{a:?}");
    assert!(a.answer.contains("harbour pilot"));
    assert!(!a.citations.is_empty());
    for c in &a.citations {
        // Every citation has a source that resolves to a real passage containing its words.
        let src = c.source.as_ref().expect("a source");
        let hit = r
            .mem
            .resolve_source(&Source {
                doc: src.doc.clone(),
                para: src.para.clone(),
                block: src.block,
                quote: src.quote.clone(),
            })
            .unwrap()
            .expect("passage exists");
        assert_eq!(hit.doc, r.s1);
        assert!(hit.text.contains("harbour pilot"), "{}", hit.text);
    }
    assert!(a.context_id.is_some(), "Why? needs the context-log id");
    assert!(r.ai.context_log.get(a.context_id.as_ref().unwrap()).is_ok());
    // A question the book has nothing about makes no model call and says so.
    let before = r.completions();
    let none = r.mem.ask("Zzyzx quarkle flumph?").await.unwrap();
    assert!(!none.found);
    assert!(none.citations.is_empty());
    assert_eq!(r.completions(), before);
}

#[tokio::test]
async fn ideas_are_validated_and_phrasing_drops_the_original() {
    let r = rig().await;
    r.index_all().await;
    let res = r
        .mem
        .ideas(IdeasRequest {
            doc_id: r.s1.clone(),
            text_before: SCENE_ONE.trim().to_string(),
            avoid: vec![],
            auto: false,
        })
        .await
        .unwrap();
    assert_eq!(
        res.ideas.len(),
        2,
        "the bogus type is dropped: {:?}",
        res.ideas
    );
    assert_eq!(res.ideas[0].kind, "stakes");
    // The reference points at paragraph 3, which exists in the scene.
    let r0 = &res.ideas[0].refs[0];
    assert_eq!(r0.doc, r.s1);
    assert!(r0.para.is_some());
    assert!(r0.quote.contains("harbour is closed"));
    assert!(r.ai.context_log.get(&res.context_id).is_ok());
    // A dismissed idea is not offered again.
    let again = r
        .mem
        .ideas(IdeasRequest {
            doc_id: r.s1.clone(),
            text_before: SCENE_ONE.trim().to_string(),
            avoid: vec![res.ideas[0].text.clone()],
            auto: false,
        })
        .await
        .unwrap();
    assert!(again.ideas.iter().all(|i| i.kind != "stakes"));
    // Pin and unpin persist in the project.
    r.mem.idea_pin(res.ideas[0].clone(), &r.s1).unwrap();
    assert_eq!(r.mem.ideas_pinned().unwrap().len(), 1);
    r.mem.idea_unpin(&res.ideas[0].id).unwrap();
    assert!(r.mem.ideas_pinned().unwrap().is_empty());
    // "To note" appends to a note document in the Notes section (twice → one note, two lines).
    let n1 = r
        .mem
        .idea_to_note(&r.s1, "Mara should lose her ticket.")
        .unwrap();
    let n2 = r
        .mem
        .idea_to_note(&r.s1, "Jon lies about the harbour.")
        .unwrap();
    assert_eq!(n1, n2);
    let note = dialect::serialize(&r.story.document_ast(&n1).unwrap());
    assert!(note.contains("lose her ticket") && note.contains("Jon lies"));
    assert_eq!(
        r.story.document_meta(&n1).unwrap().section,
        Some(Section::Notes)
    );

    let alts = r
        .mem
        .phrasing(PhrasingRequest {
            doc_id: r.s1.clone(),
            selected: "Jon had grey eyes, and he had been the harbour pilot for ten years.".into(),
            before: "Mara Voss stepped off the ferry.".into(),
            after: "\"The harbour is closed.\"".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        alts.alternatives.len(),
        3,
        "the unchanged original is dropped: {alts:?}"
    );
    assert!(
        alts.alternatives
            .iter()
            .all(|a| !a.starts_with("Jon had grey eyes"))
    );
}

#[tokio::test]
async fn ghost_prompts_carry_bible_cards_for_the_entries_named_so_far() {
    let r = rig().await;
    r.index_all().await;
    let jon = r.entity("Jon");
    r.mem
        .fact_add(&jon.id, "Jon limps in cold weather.", None, None)
        .unwrap();
    // Cards follow the text so far, in order of first appearance, canon facts first.
    let cards = r
        .mem
        .cards_for("Mara Voss stepped off the ferry. Jon waited.", 500);
    assert_eq!(cards.len(), 2);
    assert!(cards[0].starts_with("Mara Voss (character)"));
    assert!(
        cards[1].starts_with("Jon (character): Jon limps in cold weather"),
        "{}",
        cards[1]
    );
    assert!(r.mem.cards_for("Nothing named here.", 500).is_empty());
    // They reach the model, in the log as section [4].
    let before = r.completions();
    r.ai.ghost_start(crate::ai::ghost::GhostRequest {
        request_id: "g1".into(),
        doc_id: r.s1.clone(),
        text_before: "Mara Voss stepped off the ferry. Jon waited on the".into(),
        length: crate::ai::ghost::GhostLength::Phrase,
        variant: 0,
        typography: crate::ai::ghost::Typography {
            quotes: true,
            dashes: true,
            ellipsis: true,
        },
    })
    .await
    .unwrap();
    for _ in 0..100 {
        if r.completions() > before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let body = r.mock.state.bodies_for("/completion").pop().unwrap();
    let prompt = prompt_of(&body);
    assert!(prompt.contains("Story bible"), "{prompt}");
    assert!(prompt.contains("Jon limps in cold weather"));
}

#[tokio::test]
async fn nothing_is_read_while_the_model_is_off() {
    let r = rig().await;
    r.ai.disconnect().await;
    r.mem.tick().await;
    assert_eq!(r.completions(), 0);
    assert_eq!(r.mem.status().pending, 2);
    assert!(matches!(
        r.mem.status().state,
        IndexState::NoModel | IndexState::Off
    ));
    // The bible still works by hand.
    r.mem.entity_create("character", "Hand Made", "").unwrap();
    assert_eq!(
        r.mem.entities_list(Some("character"), None).unwrap().len(),
        1
    );
}

#[tokio::test]
async fn embeddings_feed_passage_search_when_the_server_is_up() {
    let r = rig().await;
    let emb = MockServer::start(MockConfig::default()).await;
    let mut mc = r.mem.config();
    mc.embedding_enabled = true;
    mc.embedding_port = emb.port as u32;
    mc.embedding_model = "mock-embed.gguf".into();
    r.mem.set_config(mc).unwrap();
    r.mem.tick().await; // checks the server, then embeds
    assert!(
        r.mem.status().embeddings.ready,
        "{:?}",
        r.mem.status().embeddings
    );
    assert_eq!(
        r.mem.status().embeddings.vectors,
        5,
        "every paragraph of both scenes"
    );
    assert_eq!(emb.state.bodies_for("/v1/embeddings").len(), 1);
    let hits = r
        .mem
        .search_semantic("harbour pilot for ten years", 3)
        .await
        .unwrap();
    assert!(hits[0].text.contains("harbour pilot"));
    // Unchanged paragraphs are not embedded twice.
    r.mem.tick().await;
    assert_eq!(
        emb.state.bodies_for("/v1/embeddings").len(),
        2,
        "only the query this time"
    );
}
