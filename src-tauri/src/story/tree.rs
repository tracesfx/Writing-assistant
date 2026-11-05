//! Pure operations on the binder ID tree.

use crate::project::manifest::{Binder, BinderEntry, Section};

/// Where a node sits: section, parent node (None = section root), index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub section: Section,
    pub parent: Option<String>,
    pub index: usize,
}

fn find_in(
    entries: &[BinderEntry],
    id: &str,
    parent: Option<&str>,
) -> Option<(Option<String>, usize)> {
    for (i, e) in entries.iter().enumerate() {
        if e.id == id {
            return Some((parent.map(str::to_string), i));
        }
        if let Some(found) = find_in(&e.children, id, Some(&e.id)) {
            return Some(found);
        }
    }
    None
}

pub fn locate(binder: &Binder, id: &str) -> Option<Slot> {
    Section::ALL.iter().find_map(|&s| {
        find_in(binder.section(s), id, None).map(|(parent, index)| Slot {
            section: s,
            parent,
            index,
        })
    })
}

pub fn find_entry<'a>(entries: &'a [BinderEntry], id: &str) -> Option<&'a BinderEntry> {
    for e in entries {
        if e.id == id {
            return Some(e);
        }
        if let Some(f) = find_entry(&e.children, id) {
            return Some(f);
        }
    }
    None
}

fn find_entry_mut<'a>(entries: &'a mut [BinderEntry], id: &str) -> Option<&'a mut BinderEntry> {
    for e in entries.iter_mut() {
        if e.id == id {
            return Some(e);
        }
        if let Some(f) = find_entry_mut(&mut e.children, id) {
            return Some(f);
        }
    }
    None
}

/// The children vector at `section`/`parent`.
pub fn children_mut<'a>(
    binder: &'a mut Binder,
    section: Section,
    parent: Option<&str>,
) -> Option<&'a mut Vec<BinderEntry>> {
    let root = binder.section_mut(section);
    match parent {
        None => Some(root),
        Some(p) => find_entry_mut(root, p).map(|e| &mut e.children),
    }
}

/// Remove a node (with its subtree) and return it with its former slot.
pub fn remove(binder: &mut Binder, id: &str) -> Option<(BinderEntry, Slot)> {
    let slot = locate(binder, id)?;
    let siblings = children_mut(binder, slot.section, slot.parent.as_deref())?;
    let entry = siblings.remove(slot.index);
    Some((entry, slot))
}

/// Insert at a slot; the index is clamped to the sibling count.
pub fn insert(binder: &mut Binder, slot: &Slot, entry: BinderEntry) -> bool {
    let Some(siblings) = children_mut(binder, slot.section, slot.parent.as_deref()) else {
        return false;
    };
    let idx = slot.index.min(siblings.len());
    siblings.insert(idx, entry);
    true
}

pub fn contains(entry: &BinderEntry, id: &str) -> bool {
    entry.id == id || entry.children.iter().any(|c| contains(c, id))
}

/// All ids in every section (not trash), pre-order.
pub fn all_section_ids(binder: &Binder) -> Vec<String> {
    let mut out = Vec::new();
    for s in Section::ALL {
        for e in binder.section(s) {
            out.extend(e.ids());
        }
    }
    out
}

/// Remove every id not accepted by `keep`, lifting the children of removed
/// nodes into their place so no descendants are lost.
pub fn retain(entries: &mut Vec<BinderEntry>, keep: &dyn Fn(&str) -> bool) {
    let old = std::mem::take(entries);
    for mut e in old {
        retain(&mut e.children, keep);
        if keep(&e.id) {
            entries.push(e);
        } else {
            entries.append(&mut e.children);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Binder {
        Binder {
            manuscript: vec![BinderEntry {
                id: "P".into(),
                children: vec![BinderEntry {
                    id: "C".into(),
                    children: vec![BinderEntry::leaf("S1"), BinderEntry::leaf("S2")],
                }],
            }],
            notes: vec![BinderEntry::leaf("N")],
            ..Binder::default()
        }
    }

    #[test]
    fn locate_remove_insert() {
        let mut b = sample();
        assert_eq!(
            locate(&b, "S2"),
            Some(Slot {
                section: Section::Manuscript,
                parent: Some("C".into()),
                index: 1
            })
        );
        let (e, slot) = remove(&mut b, "S1").unwrap();
        assert_eq!(slot.index, 0);
        assert!(insert(
            &mut b,
            &Slot {
                section: Section::Notes,
                parent: None,
                index: 99
            },
            e
        ));
        assert_eq!(
            b.notes.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["N", "S1"]
        );
        assert_eq!(all_section_ids(&b), ["P", "C", "S2", "N", "S1"]);
    }

    #[test]
    fn retain_lifts_orphaned_children() {
        let mut b = sample();
        retain(&mut b.manuscript, &|id| id != "C");
        assert_eq!(
            b.manuscript[0]
                .children
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["S1", "S2"]
        );
    }
}
