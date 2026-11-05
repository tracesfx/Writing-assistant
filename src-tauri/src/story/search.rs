//! Project-wide find and replace (SPEC §11.6).
//!
//! Text is searched per text block (paragraph or heading) in its plain
//! form, where a footnote reference is one character. Offsets are UTF-16 so
//! the editor can map them to positions directly.

use std::collections::{BTreeMap, HashSet};

use regex::{Regex, RegexBuilder};

use super::ops::{read_body, section_ids};
use super::service::{OpenProject, StoryService};
use super::tree;
use super::types::*;
use crate::dialect::{self, Document, Inline, ast::inline_plain_text};
use crate::error::{MarginError, Result};
use crate::project::manifest::Section;

const MAX_HITS: usize = 2000;
const CONTEXT_CHARS: usize = 48;

pub fn build_regex(q: &SearchQuery) -> Result<Regex> {
    if q.text.is_empty() {
        return Err(MarginError::Invalid("type something to search for".into()));
    }
    let core = if q.regex {
        q.text.clone()
    } else {
        regex::escape(&q.text)
    };
    let pattern = if q.whole_word {
        format!(r"\b(?:{core})\b")
    } else {
        core
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!q.case_sensitive)
        .size_limit(1 << 22)
        .build()
        .map_err(|e| MarginError::Invalid(format!("invalid pattern: {e}")))
}

fn scope_ids(p: &OpenProject, q: &SearchQuery) -> Result<Vec<String>> {
    let need_id = || {
        q.scope_id
            .clone()
            .ok_or_else(|| MarginError::Invalid("this scope needs a document".into()))
    };
    Ok(match q.scope {
        SearchScopeKind::Document => vec![need_id()?],
        SearchScopeKind::Chapter => {
            let id = need_id()?;
            Section::ALL
                .iter()
                .find_map(|&s| tree::find_entry(p.manifest.binder.section(s), &id))
                .map(|e| e.ids())
                .unwrap_or_default()
        }
        SearchScopeKind::Manuscript => section_ids(p, Section::Manuscript),
        SearchScopeKind::Notes => {
            let mut v = section_ids(p, Section::Notes);
            v.extend(section_ids(p, Section::Research));
            v
        }
        SearchScopeKind::All => Section::ALL
            .iter()
            .flat_map(|&s| section_ids(p, s))
            .collect(),
    })
}

fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// Byte offset → UTF-16 offset within `s`.
fn to_utf16(s: &str, byte: usize) -> usize {
    utf16_len(&s[..byte])
}

fn context_before(s: &str, byte: usize) -> String {
    let chars: Vec<char> = s[..byte].chars().collect();
    let from = chars.len().saturating_sub(CONTEXT_CHARS);
    let mut out: String = chars[from..].iter().collect();
    if from > 0 {
        out.insert(0, '…');
    }
    out.replace(dialect::ast::FOOTNOTE_CHAR, "")
}

fn context_after(s: &str, byte: usize) -> String {
    let chars: Vec<char> = s[byte..].chars().collect();
    let mut out: String = chars.iter().take(CONTEXT_CHARS).collect();
    if chars.len() > CONTEXT_CHARS {
        out.push('…');
    }
    out.replace(dialect::ast::FOOTNOTE_CHAR, "")
}

/// Plain text of every text block of a document, in order.
fn blocks_text(doc: &Document) -> Vec<String> {
    let mut v = Vec::new();
    doc.for_each_text(&mut |t| v.push(t.to_string()));
    v
}

impl StoryService {
    pub fn search(&self, q: &SearchQuery) -> Result<SearchResult> {
        let re = build_regex(q)?;
        self.read(|p| {
            let mut hits = Vec::new();
            let mut truncated = false;
            'docs: for id in scope_ids(p, q)? {
                let Some(entry) = p.docs.get(&id) else {
                    continue;
                };
                let Ok(doc) = read_body(entry) else {
                    continue;
                };
                for (bi, text) in blocks_text(&doc).iter().enumerate() {
                    for m in re.find_iter(text) {
                        if m.as_str().is_empty() {
                            continue;
                        }
                        if hits.len() >= MAX_HITS {
                            truncated = true;
                            break 'docs;
                        }
                        hits.push(SearchHit {
                            doc_id: id.clone(),
                            title: entry.fm.title.clone(),
                            block: bi as u32,
                            start: to_utf16(text, m.start()) as u32,
                            end: to_utf16(text, m.end()) as u32,
                            before: context_before(text, m.start()),
                            matched: m.as_str().replace(dialect::ast::FOOTNOTE_CHAR, ""),
                            after: context_after(text, m.end()),
                        });
                    }
                }
            }
            Ok(SearchResult { hits, truncated })
        })
    }

    /// Compute the new bodies for a replace-all over the chosen hits. The
    /// caller snapshots history, then writes them with `replace_bodies`.
    pub fn plan_replace(
        &self,
        q: &SearchQuery,
        replacement: &str,
        chosen: &[HitRef],
    ) -> Result<(Vec<(String, Document)>, u32)> {
        let re = build_regex(q)?;
        let mut by_doc: BTreeMap<&str, HashSet<(u32, u32, u32)>> = BTreeMap::new();
        for h in chosen {
            by_doc
                .entry(h.doc_id.as_str())
                .or_default()
                .insert((h.block, h.start, h.end));
        }
        self.read(|p| {
            let mut out = Vec::new();
            let mut count = 0u32;
            for (id, wanted) in by_doc {
                let entry = p
                    .docs
                    .get(id)
                    .ok_or_else(|| MarginError::NotFound(format!("document {id}")))?;
                let mut doc = read_body(entry)?;
                let mut bi = 0u32;
                let mut changed = 0u32;
                doc.for_each_text_block_mut(&mut |runs| {
                    let text = inline_plain_text(runs);
                    let mut edits: Vec<(usize, usize, String)> = Vec::new();
                    for caps in re.captures_iter(&text) {
                        let Some(m) = caps.get(0) else { continue };
                        let key = (
                            bi,
                            to_utf16(&text, m.start()) as u32,
                            to_utf16(&text, m.end()) as u32,
                        );
                        if m.as_str().is_empty() || !wanted.contains(&key) {
                            continue;
                        }
                        let mut repl = String::new();
                        if q.regex {
                            caps.expand(replacement, &mut repl);
                        } else {
                            repl = replacement.to_string();
                        }
                        edits.push((key.1 as usize, key.2 as usize, repl));
                    }
                    for (s, e, r) in edits.into_iter().rev() {
                        if replace_range(runs, s, e, &r) {
                            changed += 1;
                        }
                    }
                    bi += 1;
                });
                if changed > 0 {
                    count += changed;
                    out.push((id.to_string(), doc));
                }
            }
            Ok((out, count))
        })
    }
}

fn run_len(r: &Inline) -> usize {
    match r {
        Inline::Text { text, .. } => utf16_len(text),
        Inline::HardBreak | Inline::Footnote(_) => 1,
    }
}

/// Byte index of a UTF-16 offset within `s` (clamped).
fn byte_at(s: &str, u16off: usize) -> usize {
    let mut acc = 0;
    for (i, c) in s.char_indices() {
        if acc >= u16off {
            return i;
        }
        acc += c.len_utf16();
    }
    s.len()
}

/// Replace UTF-16 range `[start, end)` of the runs' plain text with `repl`,
/// which takes the marks of the first replaced character. Ranges that cover
/// a line break or footnote are left alone (returns false).
pub fn replace_range(runs: &mut Vec<Inline>, start: usize, end: usize, repl: &str) -> bool {
    let mut pos = 0;
    let mut out: Vec<Inline> = Vec::with_capacity(runs.len() + 2);
    let mut inserted = false;
    for r in runs.iter() {
        let len = run_len(r);
        let (rs, re_) = (pos, pos + len);
        pos = re_;
        if re_ <= start || rs >= end {
            if rs >= end && !inserted {
                return false;
            }
            out.push(r.clone());
            continue;
        }
        let Inline::Text { text, marks } = r else {
            return false;
        };
        let a = byte_at(text, start.saturating_sub(rs));
        let b = byte_at(text, end.min(re_) - rs);
        if a > 0 {
            out.push(Inline::text(&text[..a], marks.clone()));
        }
        if !inserted {
            out.push(Inline::text(repl, marks.clone()));
            inserted = true;
        }
        if b < text.len() {
            out.push(Inline::text(&text[b..], marks.clone()));
        }
    }
    if !inserted {
        return false;
    }
    *runs = dialect::ast::merge(out);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::Mark;

    fn q(text: &str) -> SearchQuery {
        SearchQuery {
            text: text.into(),
            regex: false,
            case_sensitive: false,
            whole_word: false,
            scope: SearchScopeKind::All,
            scope_id: None,
        }
    }

    #[test]
    fn regex_options() {
        assert!(build_regex(&q("a.b")).unwrap().is_match("A.B"));
        assert!(!build_regex(&q("a.b")).unwrap().is_match("axb"));
        let mut w = q("cat");
        w.whole_word = true;
        assert!(!build_regex(&w).unwrap().is_match("concatenate"));
        let mut c = q("Cat");
        c.case_sensitive = true;
        assert!(!build_regex(&c).unwrap().is_match("cat"));
        let mut bad = q("(");
        bad.regex = true;
        assert!(build_regex(&bad).is_err());
    }

    #[test]
    fn replace_keeps_marks_of_first_char() {
        let mut runs = vec![
            Inline::text("the ", vec![]),
            Inline::text("grey", vec![Mark::Italic]),
            Inline::text(" cat", vec![]),
        ];
        assert!(replace_range(&mut runs, 4, 8, "gray"));
        assert_eq!(
            runs,
            vec![
                Inline::text("the ", vec![]),
                Inline::text("gray", vec![Mark::Italic]),
                Inline::text(" cat", vec![]),
            ]
        );
        // Across a mark boundary: the replacement takes the first char's marks.
        assert!(replace_range(&mut runs, 2, 6, "XY"));
        assert_eq!(inline_plain_text(&runs), "thXYay cat");
    }

    #[test]
    fn replace_refuses_to_cross_footnotes() {
        let mut runs = vec![
            Inline::text("ab", vec![]),
            Inline::Footnote(vec![Inline::text("n", vec![])]),
            Inline::text("cd", vec![]),
        ];
        assert!(!replace_range(&mut runs, 1, 4, "x"));
        assert!(replace_range(&mut runs, 3, 5, "XY"));
        assert_eq!(inline_plain_text(&runs), "ab\u{FFFC}XY");
    }

    #[test]
    fn utf16_offsets() {
        let s = "é😀x";
        assert_eq!(to_utf16(s, s.find('x').unwrap()), 3);
        assert_eq!(byte_at(s, 3), s.find('x').unwrap());
    }
}
