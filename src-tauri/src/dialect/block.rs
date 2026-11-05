//! Block-level parsing, normalization and serialization.

use std::collections::HashMap;

use super::ast::{Block, Document, Inline};
use super::inline::{
    normalize_inline, parse_inline, parse_inline_plain, serialize_inline, with_parse_notes,
    with_ser_notes,
};

// ---------------------------------------------------------------------------
// Line classification helpers
// ---------------------------------------------------------------------------

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// Leading indentation width (tab = 4) and the byte offset after it.
fn indent_of(line: &str) -> (usize, usize) {
    let mut width = 0;
    for (i, c) in line.char_indices() {
        match c {
            ' ' => width += 1,
            '\t' => width += 4 - (width % 4),
            _ => return (width, i),
        }
    }
    (width, line.len())
}

/// Strip up to `n` columns of indentation.
fn strip_indent(line: &str, n: usize) -> &str {
    let mut width = 0;
    for (i, c) in line.char_indices() {
        if width >= n {
            return &line[i..];
        }
        match c {
            ' ' => width += 1,
            '\t' => width += 4 - (width % 4),
            _ => return &line[i..],
        }
    }
    ""
}

fn is_thematic_break(line: &str) -> bool {
    let (ind, off) = indent_of(line);
    if ind > 3 {
        return false;
    }
    let rest = &line[off..];
    let mut ch = None;
    let mut count = 0;
    for c in rest.chars() {
        match c {
            ' ' | '\t' => {}
            '*' | '-' | '_' => {
                if ch.is_some_and(|x| x != c) {
                    return false;
                }
                ch = Some(c);
                count += 1;
            }
            _ => return false,
        }
    }
    count >= 3
}

fn atx_heading(line: &str) -> Option<(u8, &str)> {
    let (ind, off) = indent_of(line);
    if ind > 3 {
        return None;
    }
    let rest = &line[off..];
    let level = rest.chars().take_while(|&c| c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let after = &rest[level..];
    if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    let mut content = after.trim();
    // Optional closing sequence: spaces followed by #s only.
    let without = content.trim_end_matches('#');
    if without.len() != content.len()
        && (without.is_empty() || without.ends_with(' ') || without.ends_with('\t'))
    {
        content = without.trim_end();
    }
    Some((level as u8, content))
}

/// `::: name`, `::: {.name}` (opening) → Some(Some(name)); bare `:::` → Some(None).
fn fence(line: &str) -> Option<Option<String>> {
    let (ind, off) = indent_of(line);
    if ind > 3 {
        return None;
    }
    let rest = &line[off..];
    let colons = rest.chars().take_while(|&c| c == ':').count();
    if colons < 3 {
        return None;
    }
    let attr = rest[colons..].trim().trim_end_matches(':').trim();
    if attr.is_empty() {
        return Some(None);
    }
    let name = attr
        .trim_start_matches('{')
        .trim_end_matches('}')
        .trim()
        .trim_start_matches('.');
    if name.is_empty()
        || !name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(Some(name.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListKind {
    Bullet(char),
    Ordered(char),
}

struct ListMarker {
    kind: ListKind,
    number: u32,
    /// Column at which item content starts.
    content_col: usize,
    /// Byte offset in the line where item content starts.
    content_off: usize,
}

fn list_marker(line: &str) -> Option<ListMarker> {
    let (ind, off) = indent_of(line);
    if ind > 3 {
        return None;
    }
    let rest = &line[off..];
    let (kind, number, mlen) = match rest.chars().next()? {
        c @ ('-' | '+' | '*') => (ListKind::Bullet(c), 1, 1),
        c if c.is_ascii_digit() => {
            let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
            if digits > 9 {
                return None;
            }
            let delim = rest[digits..].chars().next()?;
            if delim != '.' && delim != ')' {
                return None;
            }
            let number = rest[..digits].parse().ok()?;
            (ListKind::Ordered(delim), number, digits + 1)
        }
        _ => return None,
    };
    let after = &rest[mlen..];
    if after.is_empty() || is_blank(after) {
        return Some(ListMarker {
            kind,
            number,
            content_col: ind + mlen + 1,
            content_off: line.len(),
        });
    }
    if !(after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    let (sp, sp_off) = indent_of(after);
    let spaces = if sp > 4 { 1 } else { sp };
    let content_off = if sp > 4 {
        off + mlen + 1
    } else {
        off + mlen + sp_off
    };
    Some(ListMarker {
        kind,
        number,
        content_col: ind + mlen + spaces,
        content_off,
    })
}

/// Can this line interrupt a paragraph?
fn interrupts_paragraph(line: &str) -> bool {
    if atx_heading(line).is_some() || is_thematic_break(line) || fence(line).is_some() {
        return true;
    }
    let (ind, off) = indent_of(line);
    if ind <= 3 && line[off..].starts_with('>') {
        return true;
    }
    if let Some(m) = list_marker(line) {
        let empty = m.content_off >= line.len();
        return !empty && (matches!(m.kind, ListKind::Bullet(_)) || m.number == 1);
    }
    false
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

pub fn parse_body(body: &str) -> Document {
    let normalized = body.replace("\r\n", "\n").replace('\r', "\n");
    let all: Vec<&str> = normalized.split('\n').collect();
    let (lines, defs) = extract_footnote_defs(&all);
    let mut notes = HashMap::new();
    let mut order = Vec::new();
    for (label, raw) in &defs {
        notes.insert(label.clone(), parse_inline_plain(raw));
        order.push(label.clone());
    }
    let (mut blocks, used) = with_parse_notes(notes.clone(), || parse_blocks(&lines));
    // Definitions nobody references are kept as literal text, never dropped.
    for label in order {
        if !used.contains(&label)
            && let Some(content) = notes.remove(&label)
        {
            let mut c = vec![Inline::text(format!("[^{label}]: "), Vec::new())];
            c.extend(content);
            blocks.push(Block::Paragraph(c));
        }
    }
    Document { blocks }
}

/// `[^label]: text` at the start of a line (indent ≤ 3).
fn footnote_def(line: &str) -> Option<(String, &str)> {
    let (ind, off) = indent_of(line);
    if ind > 3 {
        return None;
    }
    let rest = line[off..].strip_prefix("[^")?;
    let end = rest.find("]:")?;
    let label = &rest[..end];
    if label.is_empty()
        || label.len() > 40
        || label.contains(char::is_whitespace)
        || label.contains('[')
    {
        return None;
    }
    let text = &rest[end + 2..];
    Some((
        label.to_string(),
        text.strip_prefix([' ', '\t']).unwrap_or(text),
    ))
}

/// Remove footnote definitions from the body. A definition continues on
/// lines indented by 4 or more; paragraphs inside it become hard breaks.
fn extract_footnote_defs<'a>(lines: &[&'a str]) -> (Vec<&'a str>, Vec<(String, String)>) {
    let mut body = Vec::with_capacity(lines.len());
    let mut defs: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some((label, first)) = footnote_def(lines[i]) else {
            body.push(lines[i]);
            i += 1;
            continue;
        };
        let mut raw = first.trim_end().to_string();
        i += 1;
        while i < lines.len() {
            if indent_of(lines[i]).0 >= 4 && !is_blank(lines[i]) {
                raw.push('\n');
                raw.push_str(strip_indent(lines[i], 4));
                i += 1;
                continue;
            }
            // A blank line followed by an indented line continues the note.
            let mut j = i;
            while j < lines.len() && is_blank(lines[j]) {
                j += 1;
            }
            if j > i && j < lines.len() && indent_of(lines[j]).0 >= 4 {
                raw.push('\\'); // the next line's `\n` completes the hard break
                i = j;
                continue;
            }
            break;
        }
        if !defs.iter().any(|(l, _)| *l == label) {
            defs.push((label, raw));
        }
    }
    (body, defs)
}

fn parse_blocks(lines: &[&str]) -> Vec<Block> {
    parse_blocks_ex(lines).0
}

/// Parse blocks; also reports whether a blank line separated any two
/// top-level blocks (which makes an enclosing list item loose).
fn parse_blocks_ex(lines: &[&str]) -> (Vec<Block>, bool) {
    let mut blocks = Vec::new();
    let mut blank_between = false;
    let mut saw_blank = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if is_blank(line) {
            saw_blank = true;
            i += 1;
            continue;
        }
        if saw_blank && !blocks.is_empty() {
            blank_between = true;
        }
        saw_blank = false;
        // Fenced div.
        if let Some(Some(class)) = fence(line) {
            let mut depth = 1;
            let mut j = i + 1;
            while j < lines.len() {
                match fence(lines[j]) {
                    Some(Some(_)) => depth += 1,
                    Some(None) => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    None => {}
                }
                j += 1;
            }
            let inner = parse_blocks(&lines[i + 1..j.min(lines.len())]);
            blocks.push(Block::Div {
                class,
                content: inner,
            });
            i = j + 1;
            continue;
        }
        if let Some((level, content)) = atx_heading(line) {
            blocks.push(Block::Heading {
                level,
                content: parse_inline(content),
            });
            i += 1;
            continue;
        }
        if is_thematic_break(line) {
            blocks.push(Block::SceneBreak);
            i += 1;
            continue;
        }
        let (ind, off) = indent_of(line);
        if ind <= 3 && line[off..].starts_with('>') {
            let mut inner: Vec<&str> = Vec::new();
            while i < lines.len() {
                let (ind, off) = indent_of(lines[i]);
                if ind > 3 || !lines[i][off..].starts_with('>') {
                    break;
                }
                let rest = &lines[i][off + 1..];
                inner.push(rest.strip_prefix(' ').unwrap_or(rest));
                i += 1;
            }
            blocks.push(Block::BlockQuote(parse_blocks(&inner)));
            continue;
        }
        if let Some(first) = list_marker(line) {
            i = parse_list(lines, i, first, &mut blocks);
            continue;
        }
        // Paragraph.
        let mut para: Vec<&str> = vec![line];
        i += 1;
        while i < lines.len() && !is_blank(lines[i]) && !interrupts_paragraph(lines[i]) {
            para.push(lines[i]);
            i += 1;
        }
        let raw = para
            .iter()
            .map(|l| l.trim_start())
            .collect::<Vec<_>>()
            .join("\n");
        blocks.push(Block::Paragraph(parse_inline(
            raw.trim_end_matches([' ', '\t']),
        )));
    }
    (blocks, blank_between)
}

fn parse_list(lines: &[&str], mut i: usize, first: ListMarker, blocks: &mut Vec<Block>) -> usize {
    let kind = first.kind;
    let start = first.number;
    let mut items: Vec<Vec<Block>> = Vec::new();
    let mut tight = true;
    let mut marker = Some(first);
    while let Some(m) = marker.take() {
        let mut item: Vec<String> = vec![lines[i][m.content_off..].to_string()];
        i += 1;
        let mut ended_with_blank = false;
        while i < lines.len() {
            let l = lines[i];
            if is_blank(l) {
                // Does the item continue after the blank line(s)?
                let mut j = i;
                while j < lines.len() && is_blank(lines[j]) {
                    j += 1;
                }
                if j < lines.len() && indent_of(lines[j]).0 >= m.content_col {
                    for _ in i..j {
                        item.push(String::new());
                    }
                    i = j;
                    continue;
                }
                ended_with_blank = true;
                break;
            }
            if indent_of(l).0 >= m.content_col {
                item.push(strip_indent(l, m.content_col).to_string());
                i += 1;
                continue;
            }
            if list_marker(l).is_some() || interrupts_paragraph(l) {
                break;
            }
            // Lazy paragraph continuation.
            let last_is_text = item.last().is_some_and(|x| !x.trim().is_empty());
            if last_is_text {
                item.push(l.trim_start().to_string());
                i += 1;
                continue;
            }
            break;
        }
        // Blank lines between blocks inside the item make the list loose.
        let refs: Vec<&str> = item.iter().map(String::as_str).collect();
        let trimmed_end = refs.iter().rposition(|l| !is_blank(l)).map_or(0, |p| p + 1);
        let inner = &refs[..trimmed_end];
        let (mut item_blocks, loose_item) = parse_blocks_ex(inner);
        if loose_item {
            tight = false;
        }
        if item_blocks.is_empty() {
            item_blocks.push(Block::Paragraph(Vec::new()));
        }
        items.push(item_blocks);

        // Next item?
        let mut j = i;
        while j < lines.len() && is_blank(lines[j]) {
            j += 1;
        }
        if j < lines.len()
            && !is_thematic_break(lines[j])
            && let Some(next) = list_marker(lines[j])
            && next.kind == kind
        {
            if ended_with_blank || j > i {
                tight = false;
            }
            i = j;
            marker = Some(next);
        }
    }
    blocks.push(match kind {
        ListKind::Bullet(_) => Block::BulletList { tight, items },
        ListKind::Ordered(_) => Block::OrderedList {
            tight,
            start,
            items,
        },
    });
    i
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// Canonical form of a document. `parse(serialize(normalize(d))) == normalize(d)`.
pub fn normalize(doc: &Document) -> Document {
    Document {
        blocks: normalize_blocks(&doc.blocks, false),
    }
}

fn has_text(c: &[Inline]) -> bool {
    !c.is_empty()
}

fn normalize_blocks(blocks: &[Block], in_list_item: bool) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for b in blocks {
        let nb = match b {
            Block::Paragraph(c) => {
                let c = normalize_inline(c, false);
                if !has_text(&c) {
                    continue;
                }
                Block::Paragraph(c)
            }
            Block::Heading { level, content } => Block::Heading {
                level: (*level).clamp(1, 6),
                content: normalize_inline(content, true),
            },
            Block::SceneBreak => Block::SceneBreak,
            Block::BlockQuote(inner) => {
                let inner = normalize_blocks(inner, false);
                if inner.is_empty() {
                    continue;
                }
                Block::BlockQuote(inner)
            }
            Block::Div { class, content } => Block::Div {
                class: class.clone(),
                content: normalize_blocks(content, false),
            },
            Block::BulletList { tight, items } => {
                let items = normalize_items(items);
                if items.is_empty() {
                    continue;
                }
                Block::BulletList {
                    tight: *tight && tight_ok(&items),
                    items,
                }
            }
            Block::OrderedList {
                tight,
                start,
                items,
            } => {
                let items = normalize_items(items);
                if items.is_empty() {
                    continue;
                }
                Block::OrderedList {
                    tight: *tight && tight_ok(&items),
                    start: *start,
                    items,
                }
            }
        };
        // Adjacent lists of the same kind cannot be told apart in Markdown: merge them.
        match (out.last_mut(), nb) {
            (
                Some(Block::BulletList {
                    tight: t1,
                    items: i1,
                }),
                Block::BulletList {
                    tight: t2,
                    items: i2,
                },
            ) => {
                *t1 = *t1 && t2;
                i1.extend(i2);
            }
            (
                Some(Block::OrderedList {
                    tight: t1,
                    items: i1,
                    ..
                }),
                Block::OrderedList {
                    tight: t2,
                    items: i2,
                    ..
                },
            ) => {
                *t1 = *t1 && t2;
                i1.extend(i2);
            }
            (_, nb) => out.push(nb),
        }
    }
    let _ = in_list_item;
    for b in &mut out {
        if let Block::BulletList { tight, items } | Block::OrderedList { tight, items, .. } = b {
            *tight = if items.len() == 1 && items[0].len() <= 1 {
                true
            } else {
                *tight && tight_ok(items)
            };
        }
    }
    out
}

fn normalize_items(items: &[Vec<Block>]) -> Vec<Vec<Block>> {
    items
        .iter()
        .map(|item| {
            let mut blocks = normalize_blocks(item, true);
            if blocks.is_empty() {
                blocks.push(Block::Paragraph(Vec::new()));
            }
            blocks
        })
        .collect()
}

/// A list can be written tight only if no item needs a blank line between
/// its blocks: items are `[P]`, `[P, list]` or `[list]`.
fn tight_ok(items: &[Vec<Block>]) -> bool {
    items.iter().all(|item| match item.as_slice() {
        [] | [_] => true,
        // A sub-list may follow its paragraph without a blank line only if it
        // can interrupt a paragraph (bullet, or ordered starting at 1).
        [Block::Paragraph(c), l] => {
            !c.is_empty()
                && (matches!(l, Block::BulletList { .. })
                    || matches!(l, Block::OrderedList { start: 1, .. }))
        }
        _ => false,
    })
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

/// Serialize a document body. The input is normalized first, so any
/// document serializes to its canonical form.
pub fn serialize_body(doc: &Document) -> String {
    let mut s = render(doc);
    // Overlapping emphasis spans can make the first rendering move a space
    // outside a delimiter, after which a different nesting is chosen. Settle
    // on the fixpoint so every file Margin writes round-trips byte for byte.
    for _ in 0..3 {
        let again = render(&parse_body(&s));
        if again == s {
            break;
        }
        s = again;
    }
    s
}

fn render(doc: &Document) -> String {
    let doc = normalize(doc);
    let (mut s, notes) = with_ser_notes(|| serialize_blocks(&doc.blocks, "\n\n"));
    if !notes.is_empty() {
        // Definitions in reference order. Their content has no footnotes, so
        // serializing it registers nothing new.
        let defs: Vec<String> = with_ser_notes(|| {
            notes
                .iter()
                .enumerate()
                .map(|(k, c)| {
                    let text = serialize_inline(c, false).replace('\n', "\n    ");
                    if text.is_empty() {
                        format!("[^{}]:", k + 1)
                    } else {
                        format!("[^{}]: {text}", k + 1)
                    }
                })
                .collect()
        })
        .0;
        s.push_str("\n\n");
        s.push_str(&defs.join("\n"));
    }
    if s.is_empty() { s } else { s + "\n" }
}

fn serialize_blocks(blocks: &[Block], sep: &str) -> String {
    blocks
        .iter()
        .map(serialize_block)
        .collect::<Vec<_>>()
        .join(sep)
}

fn serialize_block(b: &Block) -> String {
    match b {
        Block::Paragraph(c) => serialize_inline(c, false),
        Block::Heading { level, content } => {
            let hashes = "#".repeat(*level as usize);
            let text = serialize_inline(content, true);
            if text.is_empty() {
                hashes
            } else {
                format!("{hashes} {text}")
            }
        }
        Block::SceneBreak => "***".to_string(),
        Block::BlockQuote(inner) => serialize_blocks(inner, "\n\n")
            .split('\n')
            .map(|l| {
                if l.is_empty() {
                    ">".to_string()
                } else {
                    format!("> {l}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Div { class, content } => {
            let inner = serialize_blocks(content, "\n\n");
            if inner.is_empty() {
                format!("::: {class}\n:::")
            } else {
                format!("::: {class}\n{inner}\n:::")
            }
        }
        Block::BulletList { tight, items } => serialize_list(items, *tight, |_| "- ".to_string()),
        Block::OrderedList {
            tight,
            start,
            items,
        } => serialize_list(items, *tight, |k| format!("{}. ", *start as u64 + k as u64)),
    }
}

fn serialize_list(items: &[Vec<Block>], tight: bool, marker: impl Fn(usize) -> String) -> String {
    let inner_sep = if tight { "\n" } else { "\n\n" };
    items
        .iter()
        .enumerate()
        .map(|(k, item)| {
            let m = marker(k);
            let pad = " ".repeat(m.len());
            let body = serialize_blocks(item, inner_sep);
            if body.is_empty() {
                return m.trim_end().to_string();
            }
            body.split('\n')
                .enumerate()
                .map(|(n, l)| {
                    if n == 0 {
                        format!("{m}{l}")
                    } else if l.is_empty() {
                        String::new()
                    } else {
                        format!("{pad}{l}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join(inner_sep)
}
