//! Merge what the model read from a scene into the store (SPEC §9.4 step 3),
//! respecting canon (SPEC §9.1).
//!
//! The model's answer is a claim, not a fact. Everything is checked against the
//! scene before it is kept:
//! * an entity must actually be named in the scene text,
//! * a fact's paragraph must exist, and its quote must be in that paragraph
//!   (otherwise the quote becomes the start of the paragraph),
//! * every source points at a real paragraph id, so each item is traceable.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::extract::Extraction;
use super::mentions::{count_mentions, dialogue_ratio};
use super::paragraphs::Para;
use super::store::Store;
use super::types::*;
use crate::error::Result;

/// Pronouns and generic words the model sometimes reports as names.
const NOT_NAMES: &[&str] = &[
    "he",
    "she",
    "it",
    "they",
    "i",
    "we",
    "you",
    "him",
    "her",
    "them",
    "the man",
    "the woman",
    "a man",
    "a woman",
    "someone",
    "somebody",
    "everyone",
    "the boy",
    "the girl",
    "the old man",
];

pub struct MergeOptions {
    /// Ids of the extractors that were asked for in this reading.
    pub extractors: Vec<String>,
    pub flag_conflicts: bool,
    pub project_type_fiction: bool,
}

/// What a merge changed, for `memory.updated`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Changes {
    pub entity_ids: Vec<String>,
    pub removed_entity_ids: Vec<String>,
    pub kinds: Vec<String>,
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let t: String = s.chars().take(max).collect();
    match t.rfind(' ') {
        Some(i) if i > max / 2 => t[..i].to_string(),
        _ => t,
    }
}

fn contains_norm(hay: &str, needle: &str) -> bool {
    let n = normalize(needle);
    !n.is_empty() && normalize(hay).contains(&n)
}

/// A source for paragraph number `para` (1-based) with a quote that really is
/// in the paragraph.
/// Spelling variants of a relation kind, mapped to the closed list.
fn relation_kind(k: &str) -> String {
    let k = normalize(k).replace(' ', "-");
    match k.as_str() {
        "neighbor" => "neighbour".into(),
        "brother" | "sister" | "siblings" => "sibling".into(),
        "mother" | "father" | "parent-of" => "parent".into(),
        "husband" | "wife" | "married" => "spouse".into(),
        "servant" | "employee" => "serves".into(),
        _ => k,
    }
}

fn source_for(scene: &str, paras: &[Para], para: usize, quote: &str) -> Option<Source> {
    let p = paras.get(para.checked_sub(1)?)?;
    let quote = quote.trim();
    let quote = if !quote.is_empty() && contains_norm(&p.text, quote) {
        quote.to_string()
    } else {
        clip(&p.text, 140)
    };
    Some(Source {
        doc: scene.to_string(),
        para: Some(p.id.clone()),
        block: Some(p.block),
        quote,
    })
}

/// Titles that do not tell two people apart.
const TITLES: &[&str] = &[
    "mr", "mrs", "miss", "ms", "dr", "sir", "lady", "lord", "the", "of", "and",
];

/// The words of a name that identify it ("Bennet" in "Mr. Bennet").
fn name_words(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .flat_map(|n| {
            normalize(n)
                .split(' ')
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .filter(|w| w.chars().count() >= 3 && !TITLES.contains(&w.as_str()))
        .collect()
}

/// Does the text name this entry (by any word of its name or aliases)?
fn names_entity(text: &str, names: &[&str]) -> bool {
    let words = name_words(names);
    let t = normalize(text);
    let have: Vec<&str> = t.split(' ').collect();
    words.iter().any(|w| have.contains(&w.as_str()))
}

fn plausible_name(name: &str, kind: &str) -> bool {
    let n = name.trim();
    if n.chars().count() < 2 || n.chars().count() > 60 {
        return false;
    }
    if NOT_NAMES.contains(&n.to_lowercase().as_str()) {
        return false;
    }
    // People, places and groups are proper names.
    if matches!(kind, "character" | "location" | "group" | "person") {
        return n.chars().next().is_some_and(char::is_uppercase);
    }
    true
}

/// Apply the extractions of one scene. `exts` has one entry per window read.
pub fn apply_scene(
    store: &mut Store,
    scene: &str,
    paras: &[Para],
    text_hash: &str,
    exts: &[Extraction],
    opts: &MergeOptions,
    all_text: &dyn Fn() -> String,
) -> Result<Changes> {
    let scene_text: String = paras
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let mut changes = Changes::default();
    let mut touched: HashSet<String> = HashSet::new();

    // What this scene owned before: whatever is not read again is dropped.
    let mut old_facts: HashSet<(String, String)> = HashSet::new();
    let mut old_rels: HashSet<(String, String)> = HashSet::new();
    for e in store.entities.values() {
        for f in &e.facts {
            if f.status == Status::Inferred && f.source.as_ref().is_some_and(|s| s.doc == scene) {
                old_facts.insert((e.id.clone(), f.id.clone()));
            }
        }
        for r in &e.relations {
            if r.status == Status::Inferred && r.source.as_ref().is_some_and(|s| s.doc == scene) {
                old_rels.insert((e.id.clone(), r.id.clone()));
            }
        }
    }
    let mut kept_facts: HashSet<(String, String)> = HashSet::new();
    let mut kept_rels: HashSet<(String, String)> = HashSet::new();

    // 1. Entities named in the scene.
    let mut by_name: HashMap<String, String> = HashMap::new();
    for x in exts.iter().flat_map(|e| &e.entities) {
        let kind = if x.kind.is_empty() {
            "character"
        } else {
            &x.kind
        };
        if !is_valid_type(kind) || !plausible_name(&x.name, kind) {
            continue;
        }
        if count_mentions(&[&x.name], &scene_text) == 0 {
            continue; // not in the text: the model made it up
        }
        let aliases: Vec<String> = x
            .aliases
            .iter()
            .filter(|a| {
                a.split_whitespace().count() <= 4
                    && plausible_name(a, kind)
                    && count_mentions(&[a], &scene_text) > 0
            })
            .cloned()
            .collect();
        let id = store.ai_upsert_entity(kind, &x.name, &aliases)?;
        by_name.insert(normalize(&x.name), id.clone());
        for a in &aliases {
            by_name.insert(normalize(a), id.clone());
        }
        touched.insert(id);
    }
    let resolve =
        |store: &Store, by_name: &HashMap<String, String>, name: &str| -> Option<String> {
            by_name
                .get(&normalize(name))
                .cloned()
                .or_else(|| store.find_by_name(name, None).map(|e| e.id.clone()))
        };

    // 2. Facts.
    let mut n_facts = 0;
    for f in exts.iter().flat_map(|e| &e.facts) {
        let Some(eid) = resolve(store, &by_name, &f.entity) else {
            continue;
        };
        let Some(src) = source_for(scene, paras, f.para, &f.quote) else {
            continue;
        };
        // A fact is a sentence that names its entry ("Tom is a ferryman."), not a fragment.
        let names_ok = store
            .entities
            .get(&eid)
            .is_some_and(|e| names_entity(&f.text, &Store::names_of(e)));
        if f.text.split_whitespace().count() < 3 || !names_ok {
            continue;
        }
        let field = (!f.field.is_empty()).then_some(f.field.as_str());
        let text = clip(&f.text, 240);
        if let Some(fid) = store.ai_put_fact(&eid, &text, field, src, opts.flag_conflicts)? {
            kept_facts.insert((eid.clone(), fid));
            n_facts += 1;
        }
        touched.insert(eid);
    }

    // 3. Relations.
    let mut n_rels = 0;
    for r in exts.iter().flat_map(|e| &e.relations) {
        let (Some(a), Some(b)) = (
            resolve(store, &by_name, &r.from),
            resolve(store, &by_name, &r.to),
        ) else {
            continue;
        };
        let Some(src) = source_for(scene, paras, r.para, &r.quote) else {
            continue;
        };
        // A closed vocabulary of kinds, and the cited paragraph must name both entries.
        let kind = relation_kind(&r.kind);
        if !super::extract::RELATION_KINDS.contains(&kind.as_str()) {
            continue;
        }
        let para_text = &paras[r.para - 1].text;
        let both_named = [&a, &b].iter().all(|id| {
            store
                .entities
                .get(*id)
                .is_some_and(|e| names_entity(para_text, &Store::names_of(e)))
        });
        if !both_named {
            continue;
        }
        // The relation may have been stored on the other entity (symmetric kinds).
        if let Some(rid) = store.ai_put_relation(&a, &b, &kind, src)? {
            let owner = if store.entities[&a].relations.iter().any(|x| x.id == rid) {
                a.clone()
            } else {
                b.clone()
            };
            kept_rels.insert((owner, rid));
            n_rels += 1;
        }
        touched.insert(a);
        touched.insert(b);
    }

    // Drop what this scene used to say and no longer does.
    for (eid, fid) in old_facts.difference(&kept_facts) {
        if let Some(e) = store.entities.get_mut(eid) {
            e.facts
                .retain(|f| !(f.id == *fid && f.status == Status::Inferred));
            touched.insert(eid.clone());
        }
    }
    for (eid, rid) in old_rels.difference(&kept_rels) {
        if let Some(e) = store.entities.get_mut(eid) {
            e.relations
                .retain(|r| !(r.id == *rid && r.status == Status::Inferred));
            touched.insert(eid.clone());
        }
    }

    // 4. Threads.
    let mut thread_ids: Vec<String> = vec![];
    for t in exts.iter().flat_map(|e| &e.threads) {
        if t.title.chars().count() < 3 {
            continue;
        }
        let status = match t.status.as_str() {
            "resolved" => ThreadStatus::Resolved,
            "dropped" => ThreadStatus::Dropped,
            _ => ThreadStatus::Open,
        };
        let id = store.ai_upsert_entity("thread", clip(&t.title, 80).as_str(), &[])?;
        store.ai_touch_thread(&id, scene, status)?;
        if !thread_ids.contains(&id) {
            thread_ids.push(id.clone());
        }
        touched.insert(id);
    }

    // 5. Events: this scene's inferred events are replaced.
    store.events.retain(|e| {
        !(e.status == Status::Inferred && e.source.as_ref().is_some_and(|s| s.doc == scene))
    });
    let mut n_events = 0;
    for ev in exts.iter().flat_map(|e| &e.events) {
        let ids: Vec<String> = ev
            .entities
            .iter()
            .filter_map(|n| resolve(store, &by_name, n))
            .collect();
        let story_time = (!ev.story_time.is_empty()).then(|| StoryTime {
            label: ev.story_time.clone(),
            order: None,
        });
        store.events.push(TimelineEvent {
            id: new_id(),
            title: clip(&ev.title, 120),
            story_time,
            narrative_pos: Some(scene.to_string()),
            entities: ids,
            source: source_for(scene, paras, ev.para.max(1), "")
                .or_else(|| source_for(scene, paras, 1, "")),
            status: Status::Inferred,
            created_by: Author::Ai,
            updated: now(),
        });
        n_events += 1;
    }
    store.save_timeline()?;

    // 6. The scene card. Who is present comes from the text, not the model.
    let present: Vec<String> = store
        .entities
        .values()
        .filter(|e| e.kind != "thread")
        .filter(|e| count_mentions(&Store::names_of(e), &scene_text) > 0)
        .map(|e| e.id.clone())
        .collect();
    let first = |f: &dyn Fn(&Extraction) -> &String| -> Option<String> {
        exts.iter().map(f).find(|s| !s.is_empty()).cloned()
    };
    let summary = exts
        .iter()
        .map(|e| e.summary.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut metrics: BTreeMap<String, f64> = BTreeMap::new();
    metrics.insert(
        "words".into(),
        crate::dialect::count_words(&scene_text) as f64,
    );
    metrics.insert("dialogueRatio".into(), dialogue_ratio(&scene_text));
    metrics.insert("entityCount".into(), present.len() as f64);
    // Extractor values: the mean over the windows that reported one.
    for x in super::extract::EXTRACTORS
        .iter()
        .filter(|x| opts.extractors.iter().any(|w| w == x.id))
    {
        let vals: Vec<f64> = exts
            .iter()
            .filter_map(|e| e.metrics.get(x.metric_key).copied())
            .collect();
        if !vals.is_empty() {
            metrics.insert(
                x.metric_key.into(),
                (vals.iter().sum::<f64>() / vals.len() as f64 * 10.0).round() / 10.0,
            );
        }
    }
    store.scenes.insert(
        scene.to_string(),
        SceneCard {
            schema_version: SCHEMA_VERSION,
            scene_id: scene.to_string(),
            summary: clip(&summary, 700),
            pov: first(&|e| &e.pov),
            tense: first(&|e| &e.tense),
            location: first(&|e| &e.location),
            story_time: first(&|e| &e.story_time).map(|label| StoryTime { label, order: None }),
            entities_present: present,
            threads_touched: thread_ids,
            metrics,
            extractors: opts.extractors.clone(),
            source_hash: text_hash.to_string(),
            indexed_at: now(),
            created_by: Author::Ai,
        },
    );
    store.save_scene(scene)?;

    // 7. Entries nothing supports any more.
    let all = all_text();
    let dead = store.prune_orphans(&|e| count_mentions(&Store::names_of(e), &all) > 0)?;

    // Persist the entities whose facts were removed above.
    for id in &touched {
        if store.entities.contains_key(id) {
            store.save_entity(id)?;
        }
    }
    changes.entity_ids = touched.into_iter().filter(|i| !dead.contains(i)).collect();
    changes.entity_ids.sort();
    changes.removed_entity_ids = dead;
    changes.kinds = vec!["entities".into(), "scenes".into()];
    if n_facts > 0 {
        changes.kinds.push("facts".into());
    }
    if n_rels > 0 {
        changes.kinds.push("relations".into());
    }
    if n_events > 0 {
        changes.kinds.push("events".into());
    }
    let _ = opts.project_type_fiction;
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::extract::*;

    fn paras() -> Vec<Para> {
        [
            "Mara Voss stepped off the ferry at Kessel. Her brother Jon waited on the quay.",
            "Jon had grey eyes, and he had been the harbour pilot for ten years.",
            "\"The harbour is closed,\" said Jon. Nobody knew why.",
        ]
        .iter()
        .enumerate()
        .map(|(i, t)| Para {
            id: format!("P{i}"),
            block: i as u32,
            hash: format!("h{i}"),
            text: t.to_string(),
        })
        .collect()
    }

    fn opts() -> MergeOptions {
        MergeOptions {
            extractors: vec![],
            flag_conflicts: false,
            project_type_fiction: true,
        }
    }

    fn ext() -> Extraction {
        Extraction {
            summary: "Mara arrives and finds the harbour closed.".into(),
            pov: "Mara".into(),
            tense: "past".into(),
            location: "Kessel".into(),
            story_time: "that evening".into(),
            entities: vec![
                XEntity {
                    name: "Mara Voss".into(),
                    kind: "character".into(),
                    aliases: vec!["Mara".into()],
                },
                XEntity {
                    name: "Jon".into(),
                    kind: "character".into(),
                    aliases: vec![],
                },
                XEntity {
                    name: "Kessel".into(),
                    kind: "location".into(),
                    aliases: vec![],
                },
                XEntity {
                    name: "Zorg".into(),
                    kind: "character".into(),
                    aliases: vec![],
                }, // not in the text
                XEntity {
                    name: "he".into(),
                    kind: "character".into(),
                    aliases: vec![],
                },
            ],
            facts: vec![
                XFact {
                    entity: "Jon".into(),
                    field: "eye colour".into(),
                    text: "Jon has grey eyes.".into(),
                    para: 2,
                    quote: "grey eyes".into(),
                },
                XFact {
                    entity: "Jon".into(),
                    field: "occupation".into(),
                    text: "Jon is the harbour pilot.".into(),
                    para: 2,
                    quote: "he had NOT said this".into(),
                },
                XFact {
                    entity: "Jon".into(),
                    field: "".into(),
                    text: "Jon is nowhere".into(),
                    para: 9,
                    quote: "".into(),
                }, // bad paragraph
                XFact {
                    entity: "Nobody Known".into(),
                    field: "".into(),
                    text: "x".into(),
                    para: 1,
                    quote: "".into(),
                },
            ],
            relations: vec![XRelation {
                from: "Jon".into(),
                to: "Mara".into(),
                kind: "Sibling".into(),
                para: 1,
                quote: "Her brother Jon".into(),
            }],
            events: vec![XEvent {
                title: "Mara arrives at Kessel".into(),
                story_time: "".into(),
                entities: vec!["Mara".into()],
                para: 1,
            }],
            threads: vec![XThread {
                title: "Why the harbour is closed".into(),
                status: "open".into(),
            }],
            metrics: Default::default(),
        }
    }

    fn run(store: &mut Store, e: Extraction) -> Changes {
        apply_scene(store, "S1", &paras(), "hash1", &[e], &opts(), &|| {
            paras()
                .iter()
                .map(|p| p.text.clone())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap()
    }

    #[test]
    fn the_scene_is_checked_before_anything_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Store::empty(tmp.path());
        let ch = run(&mut s, ext());
        let names: Vec<_> = s.entities.values().map(|e| e.name.as_str()).collect();
        assert!(
            names.contains(&"Mara Voss") && names.contains(&"Jon") && names.contains(&"Kessel")
        );
        assert!(
            !names.contains(&"Zorg"),
            "an entity absent from the text is dropped"
        );
        assert!(!names.contains(&"he"));
        let jon = s.find_by_name("Jon", None).unwrap();
        assert_eq!(
            jon.facts.len(),
            2,
            "bad paragraph and unknown entity are dropped"
        );
        let eyes = jon
            .facts
            .iter()
            .find(|f| f.field.as_deref() == Some("eye colour"))
            .unwrap();
        let src = eyes.source.as_ref().unwrap();
        assert_eq!(
            (src.doc.as_str(), src.para.as_deref(), src.quote.as_str()),
            ("S1", Some("P1"), "grey eyes")
        );
        let job = jon
            .facts
            .iter()
            .find(|f| f.field.as_deref() == Some("occupation"))
            .unwrap();
        assert!(
            job.source
                .as_ref()
                .unwrap()
                .quote
                .starts_with("Jon had grey eyes"),
            "an invented quote becomes the paragraph start"
        );
        assert_eq!(ch.kinds.iter().filter(|k| *k == "facts").count(), 1);
        // The relation is stored once, as symmetric.
        let rels: usize = s.entities.values().map(|e| e.relations.len()).sum();
        assert_eq!(rels, 1);
        // The card: presence comes from the text.
        let card = &s.scenes["S1"];
        assert_eq!(card.entities_present.len(), 3);
        assert_eq!(card.metrics["words"], 38.0);
        assert!(card.metrics["dialogueRatio"] > 0.0);
        assert_eq!(card.source_hash, "hash1");
        assert_eq!(s.events.len(), 1);
        let thread = s.find_by_name("Why the harbour is closed", None).unwrap();
        assert_eq!(thread.thread.as_ref().unwrap().touches, ["S1"]);
        // Everything was written to files.
        let loaded = Store::load(tmp.path());
        assert_eq!(loaded.entities, s.entities);
        assert_eq!(loaded.scenes, s.scenes);
        assert_eq!(loaded.events, s.events);
    }

    #[test]
    fn indexing_again_keeps_ids_and_drops_what_the_scene_no_longer_says() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Store::empty(tmp.path());
        run(&mut s, ext());
        let ids_before: Vec<_> = s
            .entities
            .values()
            .flat_map(|e| e.facts.iter().map(|f| f.id.clone()))
            .collect();
        let mut again = ext();
        again.facts.truncate(1); // only the eye colour is read this time
        run(&mut s, again);
        let jon = s.find_by_name("Jon", None).unwrap();
        assert_eq!(jon.facts.len(), 1);
        assert!(
            ids_before.contains(&jon.facts[0].id),
            "the same fact keeps its id"
        );
        assert_eq!(s.events.len(), 1, "events are replaced, not duplicated");
    }

    #[test]
    fn canon_survives_a_contradicting_scene() {
        // SPEC §17 check 9 at the merge level.
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Store::empty(tmp.path());
        let jon = s.author_create("character", "Jon", "").unwrap();
        let canon = s
            .author_add_fact(&jon.id, "Jon has blue eyes.", Some("eye colour"), None)
            .unwrap();
        run(&mut s, ext());
        let e = &s.entities[&jon.id];
        assert_eq!(e.status, Status::Canon);
        assert_eq!(e.facts.iter().find(|f| f.id == canon.id).unwrap(), &canon);
        assert!(
            !e.facts.iter().any(|f| f.text.contains("grey eyes")),
            "the contradicting reading is not kept"
        );
        assert!(
            e.facts.iter().any(|f| f.text.contains("harbour pilot")),
            "other facts still arrive"
        );
        // Re-reading changes nothing about canon either.
        run(&mut s, ext());
        assert_eq!(
            s.entities[&jon.id]
                .facts
                .iter()
                .find(|f| f.id == canon.id)
                .unwrap(),
            &canon
        );
    }

    #[test]
    fn a_canon_entry_survives_pruning_and_rebuilding() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Store::empty(tmp.path());
        let k = s
            .author_create("location", "Elsewhere", "Never in the text.")
            .unwrap();
        run(&mut s, Extraction::default());
        assert!(s.entities.contains_key(&k.id));
    }
}
