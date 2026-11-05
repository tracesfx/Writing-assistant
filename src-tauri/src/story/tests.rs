use std::fs;
use std::sync::{Arc, Mutex};

use super::StoryService;
use super::types::*;
use crate::dialect::{self, pm};
use crate::error::MarginError;
use crate::events::{DomainEvent, EventBus};
use crate::project::document::{AiToggle, DocKind};
use crate::project::manifest::{Locale, ProjectType, Section};

struct Fixture {
    _dir: tempfile::TempDir,
    svc: StoryService,
    events: Arc<Mutex<Vec<DomainEvent>>>,
    root: std::path::PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let bus = Arc::new(EventBus::new());
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    bus.subscribe(move |e| ev.lock().unwrap().push(e.event.clone()));
    let svc = StoryService::new(bus);
    let info = svc
        .create_project(&NewProject {
            parent_dir: dir.path().to_string_lossy().into(),
            title: "My Novel: Draft?".into(),
            author: "Jane Doe".into(),
            project_type: ProjectType::Fiction,
            locale: Locale::EnCa,
        })
        .unwrap();
    let root = std::path::PathBuf::from(&info.root);
    Fixture {
        _dir: dir,
        svc,
        events,
        root,
    }
}

fn section(tree: &BinderTree, s: Section) -> &BinderSectionView {
    tree.sections.iter().find(|x| x.section == s).unwrap()
}

fn md(s: &str) -> pm::PmNode {
    pm::to_pm(&dialect::parse(s))
}

fn first_scene(f: &Fixture) -> String {
    let t = f.svc.binder_tree().unwrap();
    section(&t, Section::Manuscript).nodes[0].children[0]
        .id
        .clone()
}

fn target(section: Section, parent: Option<&str>, index: Option<u32>) -> BinderTarget {
    BinderTarget {
        section,
        parent: parent.map(str::to_string),
        index,
    }
}

#[test]
fn creates_project_with_starter_structure() {
    let f = fixture();
    assert!(
        f.root.ends_with("My Novel Draft"),
        "Windows-unsafe characters are stripped: {:?}",
        f.root
    );
    assert!(f.root.join("project.yaml").is_file());
    let t = f.svc.binder_tree().unwrap();
    let ms = section(&t, Section::Manuscript);
    assert_eq!(ms.nodes.len(), 1);
    assert_eq!(ms.nodes[0].kind, DocKind::Chapter);
    assert_eq!(ms.nodes[0].children[0].kind, DocKind::Scene);
    assert_eq!(section(&t, Section::Notes).nodes[0].title, "Inbox");
    let info = f.svc.project_info().unwrap();
    assert_eq!(info.locale, Locale::EnCa);
    assert!(matches!(
        f.events.lock().unwrap()[0],
        DomainEvent::ProjectOpened { .. }
    ));
}

#[test]
fn saves_and_reloads_document() {
    let f = fixture();
    let id = first_scene(&f);
    let text =
        "The harbor bells had stopped ringing by the time *Gull's Wake* came round the point.\n";
    let res = f.svc.document_save(&id, &md(text), false).unwrap();
    assert_eq!(res.word_count, 15);
    let view = f.svc.document_get(&id).unwrap();
    assert_eq!(view.revision, res.revision);
    assert_eq!(dialect::serialize(&pm::from_pm(&view.content)), text);
    // Frontmatter is preserved, body is canonical Markdown.
    let file = fs::read_to_string(f.root.join(&view.meta.file)).unwrap();
    assert!(file.starts_with("---\nschemaVersion: 1\nid: "));
    assert!(file.ends_with(text));
    // Reopen: word counts come back from disk.
    f.svc.open_project(&f.root).unwrap();
    let t = f.svc.binder_tree().unwrap();
    assert_eq!(section(&t, Section::Manuscript).total_word_count, 15);
    assert!(
        f.events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, DomainEvent::DocSaved { .. }))
    );
}

#[test]
fn save_detects_external_change() {
    let f = fixture();
    let id = first_scene(&f);
    let meta = f.svc.document_meta(&id).unwrap();
    let path = f.root.join(&meta.file);
    let edited = fs::read_to_string(&path).unwrap() + "Edited elsewhere.\n";
    fs::write(&path, edited).unwrap();
    let err = f.svc.document_save(&id, &md("Mine.\n"), false).unwrap_err();
    assert!(matches!(err, MarginError::Conflict(_)));
    // Keep mine.
    f.svc.document_save(&id, &md("Mine.\n"), true).unwrap();
    assert!(fs::read_to_string(&path).unwrap().ends_with("Mine.\n"));
}

#[test]
fn rename_changes_file_name_not_id() {
    let f = fixture();
    let id = first_scene(&f);
    f.svc
        .document_save(&id, &md("Body text.\n"), false)
        .unwrap();
    let meta = f
        .svc
        .document_update_meta(
            &id,
            &DocMetaPatch {
                title: Some("The Arrival".into()),
                pov: Some("Mara".into()),
                labels: Some(vec!["harbor".into(), " ".into()]),
                target: Some(2500),
                ai: Some(AiToggle::Off),
                ..DocMetaPatch::default()
            },
        )
        .unwrap();
    assert_eq!(meta.id, id);
    assert_eq!(meta.file, format!("manuscript/the-arrival--{id}.md"));
    assert_eq!(meta.labels, vec!["harbor"]);
    let file = fs::read_to_string(f.root.join(&meta.file)).unwrap();
    assert!(file.contains("title: The Arrival"));
    assert!(file.contains("ai: off"));
    assert!(
        file.ends_with("Body text.\n"),
        "body survives a metadata change"
    );
    // Clearing optional values.
    let meta = f
        .svc
        .document_update_meta(
            &id,
            &DocMetaPatch {
                pov: Some(String::new()),
                target: Some(0),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(meta.pov, None);
    assert_eq!(meta.target, None);
    // The next editor save does not conflict with our own metadata write.
    f.svc
        .document_save(&id, &md("Body text, revised.\n"), false)
        .unwrap();
}

#[test]
fn create_move_and_reorder() {
    let f = fixture();
    let t = f.svc.binder_tree().unwrap();
    let ch1 = section(&t, Section::Manuscript).nodes[0].id.clone();
    let s1 = first_scene(&f);
    let s2 = f
        .svc
        .document_create(
            DocKind::Scene,
            "Second",
            &target(Section::Manuscript, Some(&ch1), None),
        )
        .unwrap();
    let s3 = f
        .svc
        .document_create(
            DocKind::Scene,
            "",
            &target(Section::Manuscript, Some(&ch1), Some(0)),
        )
        .unwrap();
    assert_eq!(s3.title, "New Scene");
    let order = |f: &Fixture| -> Vec<String> {
        let t = f.svc.binder_tree().unwrap();
        section(&t, Section::Manuscript).nodes[0]
            .children
            .iter()
            .map(|n| n.id.clone())
            .collect()
    };
    assert_eq!(order(&f), vec![s3.id.clone(), s1.clone(), s2.id.clone()]);
    // Move s3 to the end of the same chapter.
    f.svc
        .binder_move(
            std::slice::from_ref(&s3.id),
            &target(Section::Manuscript, Some(&ch1), Some(3)),
        )
        .unwrap();
    assert_eq!(order(&f), vec![s1.clone(), s2.id.clone(), s3.id.clone()]);
    // Multi-select move to index 0 keeps relative order.
    f.svc
        .binder_move(
            &[s3.id.clone(), s2.id.clone()],
            &target(Section::Manuscript, Some(&ch1), Some(0)),
        )
        .unwrap();
    assert_eq!(order(&f), vec![s2.id.clone(), s3.id.clone(), s1.clone()]);
    // Moving a chapter into its own scene is rejected.
    let err = f
        .svc
        .binder_move(
            std::slice::from_ref(&ch1),
            &target(Section::Manuscript, Some(&s1), None),
        )
        .unwrap_err();
    assert!(matches!(err, MarginError::Invalid(_)));
    // Moving to Notes moves the file to notes/.
    f.svc
        .binder_move(
            std::slice::from_ref(&s1),
            &target(Section::Notes, None, None),
        )
        .unwrap();
    let meta = f.svc.document_meta(&s1).unwrap();
    assert!(meta.file.starts_with("notes/"), "{}", meta.file);
    assert_eq!(meta.section, Some(Section::Notes));
}

#[test]
fn trash_and_restore_is_lossless() {
    let f = fixture();
    let t = f.svc.binder_tree().unwrap();
    let ch1 = section(&t, Section::Manuscript).nodes[0].id.clone();
    let s1 = first_scene(&f);
    f.svc
        .document_save(&s1, &md("Precious words.\n"), false)
        .unwrap();
    f.svc.binder_trash(&[ch1.clone(), s1.clone()]).unwrap();
    let t = f.svc.binder_tree().unwrap();
    assert!(section(&t, Section::Manuscript).nodes.is_empty());
    assert_eq!(t.trash.len(), 1, "the scene goes with its chapter");
    assert_eq!(t.trash[0].node.children[0].id, s1);
    assert!(f.svc.document_meta(&s1).unwrap().file.starts_with("trash/"));
    // Survives a reopen.
    f.svc.open_project(&f.root).unwrap();
    assert_eq!(f.svc.binder_tree().unwrap().trash.len(), 1);
    f.svc.binder_restore(std::slice::from_ref(&ch1)).unwrap();
    let t = f.svc.binder_tree().unwrap();
    assert_eq!(section(&t, Section::Manuscript).nodes[0].id, ch1);
    assert_eq!(section(&t, Section::Manuscript).nodes[0].children[0].id, s1);
    assert!(t.trash.is_empty());
    let view = f.svc.document_get(&s1).unwrap();
    assert!(view.meta.file.starts_with("manuscript/"));
    assert_eq!(
        dialect::serialize(&pm::from_pm(&view.content)),
        "Precious words.\n"
    );
}

#[test]
fn reconciles_files_added_or_removed_outside_margin() {
    let f = fixture();
    let s1 = first_scene(&f);
    let s1_file = f.root.join(f.svc.document_meta(&s1).unwrap().file);
    // A plain Markdown file dropped into notes/, and a copy of a scene.
    fs::write(f.root.join("notes").join("ideas.md"), "Some ideas.\n").unwrap();
    fs::copy(&s1_file, f.root.join("manuscript").join("copy.md")).unwrap();
    // An unreadable file keeps its place in the binder.
    let ch1 = section(&f.svc.binder_tree().unwrap(), Section::Manuscript).nodes[0]
        .id
        .clone();
    let ch1_file = f.root.join(f.svc.document_meta(&ch1).unwrap().file);
    let ch1_backup = fs::read_to_string(&ch1_file).unwrap();
    fs::write(&ch1_file, "---\nid: [unclosed\n---\n").unwrap();

    let info = f.svc.open_project(&f.root).unwrap();
    assert_eq!(info.problems.len(), 1, "{:?}", info.problems);
    let t = f.svc.binder_tree().unwrap();
    let notes = section(&t, Section::Notes);
    assert!(notes.nodes.iter().any(|n| n.title == "ideas"));
    let ms = section(&t, Section::Manuscript);
    // The chapter is hidden but its scene is still listed; the copy got a new id.
    let ids: Vec<&str> = ms.nodes.iter().map(|n| n.id.as_str()).collect();
    assert!(ids.contains(&s1.as_str()));
    assert_eq!(ms.nodes.len(), 2, "{ids:?}");
    // The dropped-in file now has frontmatter with an id.
    assert!(
        fs::read_to_string(f.root.join("notes").join("ideas.md"))
            .unwrap()
            .starts_with("---\n")
    );

    // Fix the chapter file and reopen: it comes back in its original place.
    fs::write(&ch1_file, ch1_backup).unwrap();
    f.svc.open_project(&f.root).unwrap();
    let t = f.svc.binder_tree().unwrap();
    assert_eq!(section(&t, Section::Manuscript).nodes[0].id, ch1);
    assert_eq!(section(&t, Section::Manuscript).nodes[0].children[0].id, s1);
}

#[test]
fn removed_files_drop_out_of_the_binder() {
    let f = fixture();
    let s1 = first_scene(&f);
    fs::remove_file(f.root.join(f.svc.document_meta(&s1).unwrap().file)).unwrap();
    f.svc.open_project(&f.root).unwrap();
    let t = f.svc.binder_tree().unwrap();
    assert!(
        section(&t, Section::Manuscript).nodes[0]
            .children
            .is_empty()
    );
}

#[test]
fn recovery_journal_round_trip() {
    let f = fixture();
    let s1 = first_scene(&f);
    f.svc.document_save(&s1, &md("Saved.\n"), false).unwrap();
    f.svc
        .journal_write(&s1, &md("Saved. And then some unsaved words.\n"))
        .unwrap();
    // Simulate a crash + restart.
    f.svc.open_project(&f.root).unwrap();
    let entries = f.svc.journal_list().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].doc_id, s1);
    assert_eq!(entries[0].word_count, 6);
    let content = f.svc.journal_get(&s1).unwrap();
    f.svc.document_save(&s1, &content, false).unwrap();
    assert!(
        f.svc.journal_list().unwrap().is_empty(),
        "saving clears the journal"
    );
    // A journal entry identical to the file is dropped silently.
    f.svc.journal_write(&s1, &content).unwrap();
    assert!(f.svc.journal_list().unwrap().is_empty());
}

#[test]
fn rejects_existing_non_empty_folder() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("Taken")).unwrap();
    fs::write(dir.path().join("Taken").join("x.txt"), "x").unwrap();
    let svc = StoryService::new(Arc::new(EventBus::new()));
    let err = svc
        .create_project(&NewProject {
            parent_dir: dir.path().to_string_lossy().into(),
            title: "Taken".into(),
            author: String::new(),
            project_type: ProjectType::NonFiction,
            locale: Locale::EnUs,
        })
        .unwrap_err();
    assert!(matches!(err, MarginError::Invalid(_)));
}

#[test]
fn queries_fail_without_project() {
    let svc = StoryService::new(Arc::new(EventBus::new()));
    assert!(matches!(svc.binder_tree(), Err(MarginError::NoProject)));
}

fn body(f: &Fixture, id: &str) -> String {
    dialect::serialize(&f.svc.document_ast(id).unwrap())
}

fn refs(hits: &[SearchHit]) -> Vec<HitRef> {
    hits.iter()
        .map(|h| HitRef {
            doc_id: h.doc_id.clone(),
            block: h.block,
            start: h.start,
            end: h.end,
        })
        .collect()
}

#[test]
fn duplicate_copies_subtree_after_original() {
    let f = fixture();
    let t = f.svc.binder_tree().unwrap();
    let chapter = section(&t, Section::Manuscript).nodes[0].id.clone();
    let scene = first_scene(&f);
    f.svc
        .document_save(&scene, &md("Original text.\n"), false)
        .unwrap();
    let new = f
        .svc
        .binder_duplicate(std::slice::from_ref(&chapter))
        .unwrap();
    assert_eq!(new.len(), 1);
    let t = f.svc.binder_tree().unwrap();
    let ms = section(&t, Section::Manuscript);
    assert_eq!(ms.nodes.len(), 2);
    assert_eq!(ms.nodes[1].id, new[0]);
    assert_eq!(ms.nodes[1].title, "Chapter 1 (copy)");
    let copy_scene = &ms.nodes[1].children[0];
    assert_ne!(copy_scene.id, scene, "copies get new ids");
    assert_eq!(copy_scene.title, "Scene 1", "only the top copy is renamed");
    assert_eq!(body(&f, &copy_scene.id), "Original text.\n");
}

#[test]
fn split_and_merge() {
    let f = fixture();
    let scene = first_scene(&f);
    f.svc
        .document_update_meta(
            &scene,
            &DocMetaPatch {
                pov: Some("Mara".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let second = f
        .svc
        .document_split(&scene, &md("First half.\n"), &md("Second half.\n"), "")
        .unwrap();
    assert_eq!(second.title, "Scene 1 (2)");
    assert_eq!(second.pov.as_deref(), Some("Mara"), "split copies POV");
    assert_eq!(body(&f, &scene), "First half.\n");
    assert_eq!(body(&f, &second.id), "Second half.\n");
    let t = f.svc.binder_tree().unwrap();
    let kids: Vec<&str> = section(&t, Section::Manuscript).nodes[0]
        .children
        .iter()
        .map(|n| n.id.as_str())
        .collect();
    assert_eq!(kids, [scene.as_str(), second.id.as_str()]);

    // Merge in binder order regardless of the order given.
    let r = f
        .svc
        .document_merge(&[second.id.clone(), scene.clone()])
        .unwrap();
    assert_eq!(r.target, scene);
    assert_eq!(r.trashed, vec![second.id.clone()]);
    assert_eq!(body(&f, &scene), "First half.\n\nSecond half.\n");
    let t = f.svc.binder_tree().unwrap();
    assert_eq!(t.trash.len(), 1, "the merged-away scene is recoverable");
    // Containers with children cannot be merged.
    let chapter = section(&t, Section::Manuscript).nodes[0].id.clone();
    assert!(f.svc.document_merge(&[chapter, scene]).is_err());
}

#[test]
fn capture_appends_to_inbox() {
    let f = fixture();
    let id = f.svc.capture("Remember the bells.").unwrap();
    f.svc.capture("Second thought\n\nThird").unwrap();
    assert_eq!(f.svc.document_meta(&id).unwrap().title, "Inbox");
    assert_eq!(
        body(&f, &id),
        "Remember the bells.\n\nSecond thought\n\nThird\n"
    );
    assert!(
        f.events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, DomainEvent::DocContentReplaced { .. }))
    );
}

#[test]
fn outside_edits_are_detected_once_and_keep_conflict_protection() {
    let f = fixture();
    let scene = first_scene(&f);
    f.svc.document_save(&scene, &md("Mine.\n"), false).unwrap();
    let path = f.root.join(f.svc.document_meta(&scene).unwrap().file);
    let changed = |f: &Fixture| {
        f.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, DomainEvent::DocChangedOnDisk { .. }))
            .count()
    };
    // Margin's own write is not reported.
    f.svc.on_fs_change(std::slice::from_ref(&path)).unwrap();
    assert_eq!(changed(&f), 0);
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("Mine.", "Theirs, longer now.");
    fs::write(&path, text).unwrap();
    f.svc.on_fs_change(std::slice::from_ref(&path)).unwrap();
    f.svc.on_fs_change(std::slice::from_ref(&path)).unwrap();
    assert_eq!(changed(&f), 1, "reported once");
    assert_eq!(f.svc.document_meta(&scene).unwrap().word_count, 3);
    // An editor still holding the old text gets a conflict, not a silent overwrite.
    let err = f
        .svc
        .document_save(&scene, &md("Mine, edited.\n"), false)
        .unwrap_err();
    assert!(matches!(err, MarginError::Conflict(_)));
    // Reloading accepts the disk version.
    f.svc.document_get(&scene).unwrap();
    f.svc
        .document_save(&scene, &md("Merged.\n"), false)
        .unwrap();

    // A file added outside Margin appears in the binder after a rescan.
    let extra = f.root.join("notes").join("from-outside.md");
    fs::write(&extra, "Hello from outside.\n").unwrap();
    f.svc.on_fs_change(&[extra]).unwrap();
    let t = f.svc.binder_tree().unwrap();
    assert!(
        section(&t, Section::Notes)
            .nodes
            .iter()
            .any(|n| n.title == "from outside")
    );
}

#[test]
fn search_and_plan_replace() {
    let f = fixture();
    let scene = first_scene(&f);
    f.svc
        .document_save(
            &scene,
            &md("The grey cat.\n\nA *grey* sky, greyer still.\n"),
            false,
        )
        .unwrap();
    let q = SearchQuery {
        text: "grey".into(),
        regex: false,
        case_sensitive: false,
        whole_word: true,
        scope: SearchScopeKind::Manuscript,
        scope_id: None,
    };
    let r = f.svc.search(&q).unwrap();
    assert_eq!(r.hits.len(), 2, "whole word skips greyer");
    assert_eq!((r.hits[1].block, r.hits[1].start, r.hits[1].end), (1, 2, 6));
    assert_eq!(r.hits[1].before, "A ");
    let (docs, n) = f.svc.plan_replace(&q, "gray", &refs(&r.hits)).unwrap();
    assert_eq!(n, 2);
    f.svc.replace_bodies(&docs, "replace").unwrap();
    assert_eq!(
        body(&f, &scene),
        "The gray cat.\n\nA *gray* sky, greyer still.\n"
    );
    // Regex with captures.
    let q2 = SearchQuery {
        text: r"(\w+)er still".into(),
        regex: true,
        whole_word: false,
        ..q
    };
    let hits = f.svc.search(&q2).unwrap().hits;
    let (docs, _) = f
        .svc
        .plan_replace(&q2, "even ${1}er", &refs(&hits))
        .unwrap();
    f.svc.replace_bodies(&docs, "replace").unwrap();
    assert!(
        body(&f, &scene).ends_with("sky, even greyer.\n"),
        "{}",
        body(&f, &scene)
    );
}

#[test]
fn comments_and_scratchpad_live_in_project_files() {
    let f = fixture();
    let scene = first_scene(&f);
    let c = Comment {
        id: "01COMMENT".into(),
        body: "Check this.".into(),
        resolved: false,
        created_at: String::new(),
        updated_at: String::new(),
    };
    let list = f.svc.comments_put(&scene, &c).unwrap();
    assert_eq!(list.len(), 1);
    assert!(!list[0].created_at.is_empty());
    assert!(
        f.root
            .join("comments")
            .join(format!("{scene}.json"))
            .is_file()
    );
    let list = f
        .svc
        .comments_put(
            &scene,
            &Comment {
                resolved: true,
                ..c.clone()
            },
        )
        .unwrap();
    assert!(list[0].resolved);
    assert!(f.svc.comments_delete(&scene, &c.id).unwrap().is_empty());

    assert_eq!(f.svc.scratchpad_get().unwrap(), "");
    f.svc.scratchpad_put("ideas\n").unwrap();
    assert_eq!(
        fs::read_to_string(f.root.join("scratchpad.md")).unwrap(),
        "ideas\n"
    );
}

#[test]
fn project_settings_update() {
    let f = fixture();
    let info = f
        .svc
        .project_update(&ProjectPatch {
            locale: Some(Locale::EnUs),
            goals: Some(crate::project::manifest::Goals {
                project_words: Some(80_000),
                daily_words: Some(1000),
                session_words: None,
                deadline: Some("2027-06-01".into()),
            }),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(info.locale, Locale::EnUs);
    assert_eq!(info.goals.project_words, Some(80_000));
    let yaml = fs::read_to_string(f.root.join("project.yaml")).unwrap();
    assert!(yaml.contains("projectWords: 80000"), "{yaml}");
}

#[test]
fn comments_follow_their_text_through_split_duplicate_and_merge() {
    let f = fixture();
    let scene = first_scene(&f);
    let comment = |id: &str| Comment {
        id: id.into(),
        body: format!("about {id}"),
        resolved: false,
        created_at: String::new(),
        updated_at: String::new(),
    };
    let ids = |list: Vec<Comment>| list.into_iter().map(|c| c.id).collect::<Vec<_>>();
    f.svc
        .document_save(
            &scene,
            &md("<!-- c:01A -->First<!-- /c --> half.\n\n<!-- c:01B -->Second<!-- /c --> half.\n"),
            true,
        )
        .unwrap();
    f.svc.comments_put(&scene, &comment("01A")).unwrap();
    f.svc.comments_put(&scene, &comment("01B")).unwrap();

    let second = f
        .svc
        .document_split(
            &scene,
            &md("<!-- c:01A -->First<!-- /c --> half.\n"),
            &md("<!-- c:01B -->Second<!-- /c --> half.\n"),
            "",
        )
        .unwrap();
    assert_eq!(ids(f.svc.comments_list(&scene).unwrap()), ["01A"]);
    assert_eq!(
        ids(f.svc.comments_list(&second.id).unwrap()),
        ["01B"],
        "split moves the comment with its text"
    );

    let copies = f
        .svc
        .binder_duplicate(std::slice::from_ref(&second.id))
        .unwrap();
    assert_eq!(
        ids(f.svc.comments_list(&copies[0]).unwrap()),
        ["01B"],
        "a duplicate keeps its comments"
    );
    assert_eq!(ids(f.svc.comments_list(&second.id).unwrap()), ["01B"]);

    f.svc
        .document_merge(&[scene.clone(), second.id.clone()])
        .unwrap();
    assert_eq!(
        ids(f.svc.comments_list(&scene).unwrap()),
        ["01A", "01B"],
        "merge gathers comments"
    );
    assert!(f.svc.comments_list(&second.id).unwrap().is_empty());
}
