use std::sync::Arc;

use super::*;
use crate::dialect::pm;
use crate::events::EventBus;
use crate::project::manifest::{Locale, ProjectType};
use crate::story::types::NewProject;

struct Fx {
    _dir: tempfile::TempDir,
    story: Arc<StoryService>,
    history: Arc<History>,
    scene: String,
}

fn fx() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let bus = Arc::new(EventBus::new());
    let story = Arc::new(StoryService::new(bus.clone()));
    let history = History::new(story.clone(), bus);
    story
        .create_project(&NewProject {
            parent_dir: dir.path().to_string_lossy().into(),
            title: "Hist".into(),
            author: String::new(),
            project_type: ProjectType::Fiction,
            locale: Locale::EnUs,
        })
        .unwrap();
    let scene = story.section_doc_ids(Section::Manuscript).unwrap()[1].clone();
    // The session-start snapshot (empty scene) is taken in the background.
    history.wait_pending();
    Fx {
        _dir: dir,
        story,
        history,
        scene,
    }
}

fn save(f: &Fx, md: &str) {
    f.story
        .document_save(&f.scene, &pm::to_pm(&dialect::parse(md)), true)
        .unwrap();
}

#[test]
fn snapshots_versions_diff_and_restore() {
    let f = fx();
    save(&f, "The harbor was quiet.\n");
    let first = f
        .history
        .snapshot(Some("First draft"), SnapshotKind::Named)
        .unwrap()
        .unwrap();
    assert_eq!(first.name, "First draft");
    assert_eq!(first.kind, SnapshotKind::Named);
    // Nothing changed: an automatic snapshot is skipped.
    assert!(
        f.history
            .snapshot(None, SnapshotKind::Auto)
            .unwrap()
            .is_none()
    );

    save(&f, "The harbor was loud and busy.\n");
    let second = f
        .history
        .snapshot(None, SnapshotKind::Auto)
        .unwrap()
        .unwrap();
    let all = f.history.list(100).unwrap();
    assert_eq!(all[0].id, second.id);
    assert!(all.iter().any(|s| s.id == first.id));

    let versions = f.history.doc_versions(&f.scene).unwrap();
    // Newest first: second, first draft, and the empty scene at session start.
    assert_eq!(versions.len(), 3, "{versions:?}");
    assert_eq!(versions[0].snapshot.id, second.id);
    assert_eq!(versions[1].word_count, 4);
    assert_eq!(versions[2].snapshot.kind, SnapshotKind::Session);

    let d = f.history.diff(&f.scene, Some(&first.id), None).unwrap();
    assert!(d.words_added >= 3 && d.words_removed >= 1, "{d:?}");
    assert!(
        d.chunks
            .iter()
            .any(|c| c.kind == DiffKind::Delete && c.text.contains("quiet"))
    );

    let at = f.history.doc_at(&f.scene, &first.id).unwrap();
    assert_eq!(at.word_count, 4);

    // Restore the first version; a safety snapshot makes it undoable.
    let safety = f
        .history
        .restore_docs(&first.id, std::slice::from_ref(&f.scene), "Before restore")
        .unwrap()
        .unwrap();
    assert_eq!(
        dialect::serialize(&f.story.document_ast(&f.scene).unwrap()),
        "The harbor was quiet.\n"
    );
    f.history
        .restore_docs(&safety.id, std::slice::from_ref(&f.scene), "Undo restore")
        .unwrap();
    assert_eq!(
        dialect::serialize(&f.story.document_ast(&f.scene).unwrap()),
        "The harbor was loud and busy.\n"
    );
}

#[test]
fn history_excludes_app_folder_and_follows_renames() {
    let f = fx();
    save(&f, "One two.\n");
    f.history.snapshot(Some("a"), SnapshotKind::Named).unwrap();
    let root = f.story.project_root().unwrap();
    std::fs::write(root.join(".margin").join("cache.bin"), b"x").unwrap();
    // Rename the document: its file name changes, its id does not.
    f.story
        .document_update_meta(
            &f.scene,
            &crate::story::types::DocMetaPatch {
                title: Some("Renamed".into()),
                ..Default::default()
            },
        )
        .unwrap();
    save(&f, "One two three.\n");
    f.history.snapshot(Some("b"), SnapshotKind::Named).unwrap();
    let versions = f.history.doc_versions(&f.scene).unwrap();
    assert_eq!(versions.len(), 3, "b, a and the session start");
    assert_eq!(versions[0].title, "Renamed");
    assert_eq!(versions[1].title, "Scene 1");
    f.history
        .with_repo(|repo| {
            let head = repo
                .find_reference(BRANCH)
                .unwrap()
                .peel_to_commit()
                .unwrap();
            let tree = head.tree().unwrap();
            assert!(tree.get_name(".margin").is_none());
            assert!(tree.get_name("project.yaml").is_some());
            Ok(())
        })
        .unwrap();
}

#[test]
fn word_diff_marks_changes() {
    let d = word_diff("a quiet harbor", "a loud harbor");
    assert_eq!(d.words_added, 1);
    assert_eq!(d.words_removed, 1);
    let text: String = d
        .chunks
        .iter()
        .filter(|c| c.kind != DiffKind::Delete)
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(text, "a loud harbor");
}
