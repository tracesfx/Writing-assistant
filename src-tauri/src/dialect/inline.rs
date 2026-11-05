//! Inline parsing and serialization.
//!
//! Parsing follows the CommonMark delimiter-run algorithm for `*`/`_`
//! emphasis (flanking rules and the "rule of 3"), so hand-written files read
//! the same way pandoc reads them. Bracketed constructs are links
//! `[text](url)` and pandoc spans `[text]{.sc}` / `[text]{.nobreak}`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::ast::{Inline, Mark};

// ---------------------------------------------------------------------------
// Footnote context
//
// Footnote definitions live at the end of the body, so inline parsing needs
// them in scope. While parsing a body, `PARSE_NOTES` maps labels to content;
// while serializing, `SER_NOTES` collects footnote contents in order (label
// = index + 1), and the verification parse of a paragraph reads them back.
// ---------------------------------------------------------------------------

thread_local! {
    static PARSE_NOTES: RefCell<HashMap<String, Vec<Inline>>> = RefCell::new(HashMap::new());
    static USED_NOTES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static SER_NOTES: RefCell<Vec<Vec<Inline>>> = const { RefCell::new(Vec::new()) };
}

/// Run `f` with `notes` as the footnote definitions in scope. Returns `f`'s
/// result and the labels that were referenced.
pub(crate) fn with_parse_notes<T>(
    notes: HashMap<String, Vec<Inline>>,
    f: impl FnOnce() -> T,
) -> (T, HashSet<String>) {
    let prev = PARSE_NOTES.with(|n| std::mem::replace(&mut *n.borrow_mut(), notes));
    let prev_used = USED_NOTES.with(|u| std::mem::take(&mut *u.borrow_mut()));
    let out = f();
    PARSE_NOTES.with(|n| *n.borrow_mut() = prev);
    let used = USED_NOTES.with(|u| std::mem::replace(&mut *u.borrow_mut(), prev_used));
    (out, used)
}

/// Run `f` with an empty serialization footnote list; returns the footnotes
/// collected while it ran.
pub(crate) fn with_ser_notes<T>(f: impl FnOnce() -> T) -> (T, Vec<Vec<Inline>>) {
    let prev = SER_NOTES.with(|n| std::mem::take(&mut *n.borrow_mut()));
    let out = f();
    let notes = SER_NOTES.with(|n| std::mem::replace(&mut *n.borrow_mut(), prev));
    (out, notes)
}

fn lookup_note(label: &str) -> Option<Vec<Inline>> {
    if let Some(c) = PARSE_NOTES.with(|n| n.borrow().get(label).cloned()) {
        USED_NOTES.with(|u| u.borrow_mut().insert(label.to_string()));
        return Some(c);
    }
    let k: usize = label.parse().ok()?;
    SER_NOTES.with(|n| n.borrow().get(k.checked_sub(1)?).cloned())
}

/// Parse inline content with no footnote definitions in scope (footnote
/// contents, editor attributes).
pub fn parse_inline_plain(src: &str) -> Vec<Inline> {
    with_parse_notes(HashMap::new(), || {
        let prev = SER_NOTES.with(|n| std::mem::take(&mut *n.borrow_mut()));
        let out = parse_inline(src);
        SER_NOTES.with(|n| *n.borrow_mut() = prev);
        out
    })
    .0
}

pub(crate) fn is_comment_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// `<!-- c:ID -->` → (Some(id), len); `<!-- /c -->` → (None, len).
fn comment_anchor(chars: &[char], i: usize) -> Option<(Option<String>, usize)> {
    const OPEN: &str = "<!-- c:";
    const CLOSE: &str = "<!-- /c -->";
    let rest: String = chars[i..chars.len().min(i + 80)].iter().collect();
    if rest.starts_with(CLOSE) {
        return Some((None, CLOSE.chars().count()));
    }
    let after = rest.strip_prefix(OPEN)?;
    let end = after.find(" -->")?;
    let id = &after[..end];
    if !is_comment_id(id) {
        return None;
    }
    Some((
        Some(id.to_string()),
        OPEN.chars().count() + id.chars().count() + 4,
    ))
}

/// `[^label]` at `i` → (label, chars consumed).
fn footnote_ref(chars: &[char], i: usize) -> Option<(String, usize)> {
    if chars.get(i + 1) != Some(&'^') {
        return None;
    }
    let mut k = i + 2;
    let mut label = String::new();
    while let Some(&c) = chars.get(k) {
        if c == ']' {
            break;
        }
        if c.is_whitespace() || c == '[' || label.len() > 40 {
            return None;
        }
        label.push(c);
        k += 1;
    }
    if chars.get(k) != Some(&']') || label.is_empty() {
        return None;
    }
    Some((label, k + 1 - i))
}

#[derive(Debug)]
struct Delim {
    ch: char,
    orig: usize,
    count: usize,
    can_open: bool,
    can_close: bool,
}

#[derive(Debug)]
enum Node {
    Text(String),
    HardBreak,
    Delim(Delim),
    /// An unmatched `[`; rendered literally unless a `]` resolves it.
    Bracket,
    /// An unmatched comment opener; dropped unless a closer resolves it.
    CommentOpen(String),
    Footnote(Vec<Inline>),
    Group {
        mark: Mark,
        children: Vec<Node>,
    },
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation() || (!c.is_alphanumeric() && !c.is_whitespace() && !c.is_control())
}

fn is_ws(c: Option<char>) -> bool {
    c.is_none_or(char::is_whitespace)
}

fn is_p(c: Option<char>) -> bool {
    c.is_some_and(is_punct)
}

/// Parse the raw text of one paragraph or heading. Lines are separated by
/// `\n`; a soft line break becomes a space, and a trailing `\` or two
/// trailing spaces make a hard break.
pub fn parse_inline(src: &str) -> Vec<Inline> {
    let chars: Vec<char> = src.chars().collect();
    let len = chars.len();
    let mut nodes: Vec<Node> = Vec::new();
    let mut brackets: Vec<usize> = Vec::new();
    let mut comments: Vec<usize> = Vec::new();
    let mut text = String::new();

    fn flush(text: &mut String, nodes: &mut Vec<Node>) {
        if !text.is_empty() {
            nodes.push(Node::Text(std::mem::take(text)));
        }
    }
    fn skip_line_indent(chars: &[char], mut i: usize) -> usize {
        while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
            i += 1;
        }
        i
    }

    let mut i = 0;
    while i < len {
        let c = chars[i];
        match c {
            '\\' => match chars.get(i + 1) {
                Some('\n') => {
                    flush(&mut text, &mut nodes);
                    nodes.push(Node::HardBreak);
                    i = skip_line_indent(&chars, i + 2);
                }
                Some(&n) if n.is_ascii_punctuation() => {
                    text.push(n);
                    i += 2;
                }
                _ => {
                    text.push('\\');
                    i += 1;
                }
            },
            '\n' => {
                let trimmed = text.trim_end_matches([' ', '\t']);
                let trailing = text.len() - trimmed.len();
                let hard = text[trimmed.len()..].chars().filter(|&c| c == ' ').count() >= 2;
                text.truncate(text.len() - trailing);
                if hard {
                    flush(&mut text, &mut nodes);
                    nodes.push(Node::HardBreak);
                } else {
                    text.push(' ');
                }
                i = skip_line_indent(&chars, i + 1);
            }
            '*' | '_' => {
                let start = i;
                while i < len && chars[i] == c {
                    i += 1;
                }
                let prev = if start == 0 {
                    None
                } else {
                    Some(chars[start - 1])
                };
                let next = chars.get(i).copied();
                let left = !is_ws(next) && (!is_p(next) || is_ws(prev) || is_p(prev));
                let right = !is_ws(prev) && (!is_p(prev) || is_ws(next) || is_p(next));
                let (can_open, can_close) = if c == '*' {
                    (left, right)
                } else {
                    (
                        left && (!right || is_p(prev)),
                        right && (!left || is_p(next)),
                    )
                };
                flush(&mut text, &mut nodes);
                let n = i - start;
                nodes.push(Node::Delim(Delim {
                    ch: c,
                    orig: n,
                    count: n,
                    can_open,
                    can_close,
                }));
            }
            '[' => {
                if let Some((label, consumed)) = footnote_ref(&chars, i)
                    && let Some(content) = lookup_note(&label)
                {
                    flush(&mut text, &mut nodes);
                    nodes.push(Node::Footnote(content));
                    i += consumed;
                    continue;
                }
                flush(&mut text, &mut nodes);
                brackets.push(nodes.len());
                nodes.push(Node::Bracket);
                i += 1;
            }
            ']' => {
                if let Some(open) = brackets.pop()
                    && let Some((mark, consumed)) = parse_bracket_suffix(&chars, i + 1)
                {
                    flush(&mut text, &mut nodes);
                    let inner: Vec<Node> = nodes.drain(open + 1..).collect();
                    nodes.pop(); // the Bracket placeholder
                    // Comment openers inside the span can no longer be closed.
                    comments.retain(|&c| c < open);
                    nodes.push(Node::Group {
                        mark,
                        children: process_emphasis(inner),
                    });
                    i += 1 + consumed;
                    continue;
                }
                text.push(']');
                i += 1;
            }
            '<' => match comment_anchor(&chars, i) {
                Some((Some(id), consumed)) => {
                    flush(&mut text, &mut nodes);
                    comments.push(nodes.len());
                    nodes.push(Node::CommentOpen(id));
                    i += consumed;
                }
                Some((None, consumed)) => {
                    flush(&mut text, &mut nodes);
                    if let Some(open) = comments.pop() {
                        let inner: Vec<Node> = nodes.drain(open + 1..).collect();
                        let id = match nodes.pop() {
                            Some(Node::CommentOpen(id)) => id,
                            _ => String::new(),
                        };
                        brackets.retain(|&b| b < open);
                        nodes.push(Node::Group {
                            mark: Mark::Comment { id },
                            children: process_emphasis(inner),
                        });
                    }
                    i += consumed;
                }
                None => {
                    text.push('<');
                    i += 1;
                }
            },
            '\r' => i += 1,
            _ => {
                text.push(c);
                i += 1;
            }
        }
    }
    flush(&mut text, &mut nodes);
    let nodes = process_emphasis(nodes);
    let mut out = Vec::new();
    flatten(nodes, &mut Vec::new(), &mut out);
    merge_runs(out)
}

/// After `]`: a link destination `(url)` / `(<url>)` or a span attribute
/// block `{.sc}` / `{.nobreak}`. Returns the mark and chars consumed.
fn parse_bracket_suffix(chars: &[char], j: usize) -> Option<(Mark, usize)> {
    match chars.get(j)? {
        '(' => {
            let mut k = j + 1;
            let mut href = String::new();
            if chars.get(k) == Some(&'<') {
                k += 1;
                loop {
                    match chars.get(k)? {
                        '>' => break,
                        '\n' => return None,
                        '\\' if chars.get(k + 1).is_some_and(|c| c.is_ascii_punctuation()) => {
                            href.push(chars[k + 1]);
                            k += 2;
                        }
                        &c => {
                            href.push(c);
                            k += 1;
                        }
                    }
                }
                k += 1;
                if chars.get(k) != Some(&')') {
                    return None;
                }
            } else {
                loop {
                    match chars.get(k)? {
                        ')' => break,
                        c if c.is_whitespace() => return None,
                        '\\' if chars.get(k + 1).is_some_and(|c| c.is_ascii_punctuation()) => {
                            href.push(chars[k + 1]);
                            k += 2;
                        }
                        &c => {
                            href.push(c);
                            k += 1;
                        }
                    }
                }
            }
            Some((Mark::Link { href }, k + 1 - j))
        }
        '{' => {
            let mut k = j + 1;
            let mut attr = String::new();
            loop {
                match chars.get(k)? {
                    '}' => break,
                    '\n' => return None,
                    &c => attr.push(c),
                }
                k += 1;
            }
            let mark = match attr.trim() {
                ".sc" | ".smallcaps" => Mark::SmallCaps,
                ".em" => Mark::Italic,
                ".strong" => Mark::Bold,
                ".nobreak" => Mark::NoBreak,
                _ => return None,
            };
            Some((mark, k + 1 - j))
        }
        _ => None,
    }
}

fn process_emphasis(mut nodes: Vec<Node>) -> Vec<Node> {
    let mut ci = 0;
    while ci < nodes.len() {
        let (ch, c_count, c_orig, c_open) = match &nodes[ci] {
            Node::Delim(d) if d.can_close && d.count > 0 => (d.ch, d.count, d.orig, d.can_open),
            _ => {
                ci += 1;
                continue;
            }
        };
        let mut found = None;
        let mut oi = ci;
        while oi > 0 {
            oi -= 1;
            if let Node::Delim(o) = &nodes[oi]
                && o.ch == ch
                && o.can_open
                && o.count > 0
            {
                let rule_of_3 = (o.can_close || c_open)
                    && (o.orig + c_orig) % 3 == 0
                    && !(o.orig % 3 == 0 && c_orig % 3 == 0);
                if !rule_of_3 {
                    found = Some((oi, o.count));
                    break;
                }
            }
        }
        let Some((oi, o_count)) = found else {
            ci += 1;
            continue;
        };
        let used = if c_count >= 2 && o_count >= 2 { 2 } else { 1 };
        let mark = if used == 2 { Mark::Bold } else { Mark::Italic };
        let inner: Vec<Node> = nodes.drain(oi + 1..ci).collect();
        nodes.insert(
            oi + 1,
            Node::Group {
                mark,
                children: inner,
            },
        );
        let mut closer = oi + 2;
        if let Node::Delim(o) = &mut nodes[oi] {
            o.count -= used;
        }
        if let Node::Delim(c) = &mut nodes[closer] {
            c.count -= used;
        }
        if matches!(&nodes[oi], Node::Delim(o) if o.count == 0) {
            nodes.remove(oi);
            closer -= 1;
        }
        if matches!(&nodes[closer], Node::Delim(c) if c.count == 0) {
            nodes.remove(closer);
        }
        ci = closer;
    }
    nodes
}

fn flatten(nodes: Vec<Node>, marks: &mut Vec<Mark>, out: &mut Vec<Inline>) {
    for n in nodes {
        match n {
            Node::Text(t) => out.push(Inline::text(t, marks.clone())),
            Node::HardBreak => out.push(Inline::HardBreak),
            Node::Delim(d) => out.push(Inline::text(
                d.ch.to_string().repeat(d.count),
                marks.clone(),
            )),
            Node::Bracket => out.push(Inline::text("[", marks.clone())),
            Node::CommentOpen(_) => {}
            Node::Footnote(c) => out.push(Inline::Footnote(c)),
            Node::Group { mark, children } => {
                marks.push(mark);
                flatten(children, marks, out);
                marks.pop();
            }
        }
    }
}

/// Merge adjacent text runs with identical marks and drop empty runs.
pub fn merge_runs(runs: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(runs.len());
    for r in runs {
        match r {
            Inline::Text { text, marks } => {
                if text.is_empty() {
                    continue;
                }
                if let Some(Inline::Text {
                    text: prev,
                    marks: pm,
                }) = out.last_mut()
                    && *pm == marks
                {
                    prev.push_str(&text);
                    continue;
                }
                out.push(Inline::Text { text, marks });
            }
            Inline::HardBreak => out.push(Inline::HardBreak),
            Inline::Footnote(c) => out.push(Inline::Footnote(c)),
        }
    }
    out
}

/// Normalized footnote content: one paragraph, no nested footnotes, and a
/// fixpoint of Markdown serialization (footnote content travels to the
/// editor as Markdown source, so the bridge must not change it).
fn normalize_note(c: &[Inline]) -> Vec<Inline> {
    let flat: Vec<Inline> = c
        .iter()
        .filter(|r| !matches!(r, Inline::Footnote(_)))
        .cloned()
        .collect();
    let mut cur = normalize_inline(&flat, false);
    for _ in 0..4 {
        let s = with_ser_notes(|| serialize_inline(&cur, false)).0;
        let back = normalize_inline(&parse_inline_plain(&s), false);
        if back == cur {
            break;
        }
        cur = back;
    }
    cur
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// Bring inline content into the canonical form that the serializer can
/// express exactly: no newlines inside text, no whitespace at line edges,
/// no leading/trailing hard breaks, and no whitespace at the inner edges of
/// emphasis spans (it is moved outside the span).
pub fn normalize_inline(content: &[Inline], in_heading: bool) -> Vec<Inline> {
    let mut runs: Vec<Inline> = content
        .iter()
        .map(|r| match r {
            Inline::Text { text, marks } => {
                Inline::text(text.replace(['\r', '\n'], " "), marks.clone())
            }
            Inline::HardBreak if in_heading => Inline::text(" ", Vec::new()),
            Inline::HardBreak => Inline::HardBreak,
            Inline::Footnote(c) => Inline::Footnote(normalize_note(c)),
        })
        .collect();
    runs = merge_runs(runs);

    // Trim whitespace at line edges (paragraph edges and around hard breaks).
    loop {
        let mut changed = false;
        let n = runs.len();
        for k in 0..n {
            let at_line_start = k == 0 || matches!(runs[k - 1], Inline::HardBreak);
            let at_line_end = k + 1 == n || matches!(runs[k + 1], Inline::HardBreak);
            if let Inline::Text { text, .. } = &mut runs[k] {
                if at_line_start {
                    let t = text.trim_start_matches([' ', '\t']);
                    if t.len() != text.len() {
                        *text = t.to_string();
                        changed = true;
                    }
                }
                if at_line_end {
                    let t = text.trim_end_matches([' ', '\t']);
                    if t.len() != text.len() {
                        *text = t.to_string();
                        changed = true;
                    }
                }
            }
        }
        runs = merge_runs(runs);
        // Drop hard breaks at the edges.
        while matches!(runs.first(), Some(Inline::HardBreak)) {
            runs.remove(0);
            changed = true;
        }
        while matches!(runs.last(), Some(Inline::HardBreak)) {
            runs.pop();
            changed = true;
        }
        if !changed {
            break;
        }
    }

    // Move whitespace out of the inner edges of emphasis spans.
    loop {
        let mut changed = false;
        let mut next: Vec<Inline> = Vec::with_capacity(runs.len() + 2);
        let n = runs.len();
        for k in 0..n {
            let Inline::Text { text, marks } = &runs[k] else {
                next.push(runs[k].clone());
                continue;
            };
            let prev_marks = match k.checked_sub(1).map(|p| &runs[p]) {
                Some(Inline::Text { marks, .. }) => Some(marks),
                _ => None,
            };
            let next_marks = match runs.get(k + 1) {
                Some(Inline::Text { marks, .. }) => Some(marks),
                _ => None,
            };
            let starts: Vec<&Mark> = marks
                .iter()
                .filter(|m| m.is_emphasis() && !prev_marks.is_some_and(|pm| pm.contains(m)))
                .collect();
            let ends: Vec<&Mark> = marks
                .iter()
                .filter(|m| m.is_emphasis() && !next_marks.is_some_and(|nm| nm.contains(m)))
                .collect();
            let body_start = text.len() - text.trim_start().len();
            let body_end = text.trim_end().len();
            if body_start >= body_end {
                // Whitespace-only run: drop emphasis marks that start or end here.
                let kept: Vec<Mark> = marks
                    .iter()
                    .filter(|m| !(starts.contains(m) || ends.contains(m)))
                    .cloned()
                    .collect();
                if kept.len() != marks.len() {
                    changed = true;
                }
                next.push(Inline::text(text.clone(), kept));
                continue;
            }
            let lead = &text[..body_start];
            let body = &text[body_start..body_end];
            let trail = &text[body_end..];
            if !lead.is_empty() && !starts.is_empty() {
                let m: Vec<Mark> = marks
                    .iter()
                    .filter(|m| !starts.contains(m))
                    .cloned()
                    .collect();
                next.push(Inline::text(lead, m));
                changed = true;
            } else if !lead.is_empty() {
                next.push(Inline::text(lead, marks.clone()));
            }
            next.push(Inline::text(body, marks.clone()));
            if !trail.is_empty() && !ends.is_empty() {
                let m: Vec<Mark> = marks
                    .iter()
                    .filter(|m| !ends.contains(m))
                    .cloned()
                    .collect();
                next.push(Inline::text(trail, m));
                changed = true;
            } else if !trail.is_empty() {
                next.push(Inline::text(trail, marks.clone()));
            }
        }
        runs = merge_runs(next);
        if !changed {
            break;
        }
    }
    runs
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

fn opener(m: &Mark, spans: bool) -> String {
    match m {
        Mark::Comment { id } => format!("<!-- c:{id} -->"),
        Mark::Link { .. } | Mark::SmallCaps | Mark::NoBreak => "[".into(),
        Mark::Bold | Mark::Italic if spans => "[".into(),
        Mark::Bold => "**".into(),
        Mark::Italic => "*".into(),
    }
}

fn closer(m: &Mark, spans: bool) -> String {
    match m {
        Mark::Comment { .. } => "<!-- /c -->".to_string(),
        Mark::Link { href } => {
            if href.is_empty()
                || href
                    .chars()
                    .any(|c| c.is_whitespace() || c == '(' || c == ')')
            {
                format!("](<{}>)", href.replace('\\', "\\\\").replace('>', "\\>"))
            } else {
                format!("]({})", href.replace('\\', "\\\\"))
            }
        }
        Mark::SmallCaps => "]{.sc}".to_string(),
        Mark::NoBreak => "]{.nobreak}".to_string(),
        Mark::Bold if spans => "]{.strong}".to_string(),
        Mark::Italic if spans => "]{.em}".to_string(),
        Mark::Bold => "**".to_string(),
        Mark::Italic => "*".to_string(),
    }
}

const ALWAYS_ESCAPE: &[char] = &['\\', '*', '_', '[', ']', '`', '<'];

fn escape_text(text: &str, line_start: bool, in_heading: bool, out: &mut String) {
    let chars: Vec<char> = text.chars().collect();
    if line_start {
        // Characters that would start a block construct at the beginning of a line.
        if let Some(&c) = chars.first() {
            let starts_block = (matches!(c, '#' | '>' | '+' | '-' | '=' | '~' | '|')
                && !in_heading)
                || (c == ':' && chars.get(1) == Some(&':'));
            if starts_block {
                out.push('\\');
            } else if c.is_ascii_digit() {
                let digits = chars.iter().take_while(|c| c.is_ascii_digit()).count();
                if digits <= 9 && matches!(chars.get(digits), Some('.') | Some(')')) {
                    for &d in &chars[..digits] {
                        out.push(d);
                    }
                    out.push('\\');
                    for &c in &chars[digits..] {
                        push_escaped(c, in_heading, out);
                    }
                    return;
                }
            }
        }
    }
    for c in chars {
        push_escaped(c, in_heading, out);
    }
}

fn push_escaped(c: char, in_heading: bool, out: &mut String) {
    if ALWAYS_ESCAPE.contains(&c) || (in_heading && c == '#') {
        out.push('\\');
    }
    out.push(c);
}

/// Can emphasis delimiters be placed between `runs[b - 1]` and `runs[b]`?
/// Not if either side of the boundary is whitespace or a line break.
fn boundary_safe(runs: &[Inline], b: usize) -> bool {
    let before_ok = match b.checked_sub(1).and_then(|i| runs.get(i)) {
        Some(Inline::Text { text, .. }) => !text.ends_with(char::is_whitespace),
        Some(Inline::HardBreak) => false,
        Some(Inline::Footnote(_)) | None => true,
    };
    let after_ok = match runs.get(b) {
        Some(Inline::Text { text, .. }) => !text.starts_with(char::is_whitespace),
        Some(Inline::HardBreak) => false,
        Some(Inline::Footnote(_)) | None => true,
    };
    before_ok && after_ok
}

/// Serialize normalized inline content.
///
/// When overlapping emphasis spans force a delimiter onto a whitespace
/// boundary, the whitespace is emitted outside the delimiter so it still
/// parses; only the emphasis flag of that whitespace is lost.
fn render_inline(runs: &[Inline], in_heading: bool, spans: bool, note_base: usize) -> String {
    let mut note_no = note_base;
    // In span mode emphasis is written as bracket spans, which have no flanking rules.
    let delim = |m: &Mark| m.is_emphasis() && !spans;
    let mut out = String::new();
    let mut active: Vec<Mark> = Vec::new();
    let mut line_start = true;

    let close_to = |active: &mut Vec<Mark>, keep: usize, out: &mut String| {
        while active.len() > keep {
            if let Some(m) = active.pop() {
                if delim(&m) {
                    // A closing `*` must not follow whitespace.
                    let trimmed = out.trim_end_matches([' ', '\t']).len();
                    let ws = out.split_off(trimmed);
                    out.push_str(&closer(&m, spans));
                    out.push_str(&ws);
                } else {
                    out.push_str(&closer(&m, spans));
                }
            }
        }
    };

    for (k, run) in runs.iter().enumerate() {
        match run {
            Inline::HardBreak => {
                // Close marks that do not continue past the break so no
                // delimiter ends up at the start of the next line.
                let next_marks: &[Mark] = runs[k + 1..]
                    .iter()
                    .find_map(|r| match r {
                        Inline::Text { marks, .. } => Some(marks.as_slice()),
                        Inline::Footnote(_) => Some(&[][..]),
                        Inline::HardBreak => None,
                    })
                    .unwrap_or(&[]);
                let keep = active.iter().take_while(|m| next_marks.contains(m)).count();
                close_to(&mut active, keep, &mut out);
                out.push_str("\\\n");
                line_start = true;
            }
            Inline::Footnote(_) => {
                close_to(&mut active, 0, &mut out);
                note_no += 1;
                if out.ends_with('!') && !out.ends_with("\\!") {
                    out.insert(out.len() - 1, '\\');
                }
                out.push_str(&format!("[^{note_no}]"));
                line_start = false;
            }
            Inline::Text { text, marks } => {
                // How many runs, from here, a mark continues for.
                let extent = |m: &Mark| {
                    runs[k..]
                        .iter()
                        .take_while(|r| match r {
                            Inline::Text { marks, .. } => marks.contains(m),
                            Inline::HardBreak => true,
                            Inline::Footnote(_) => false,
                        })
                        .count()
                };
                let mut keep = active.iter().take_while(|m| marks.contains(m)).count();
                // Overlapping spans cannot nest, so one must be split. Keeping an
                // active mark open splits a longer new mark where the active one
                // ends; closing it now splits the active mark here instead. An
                // emphasis delimiter next to whitespace would not parse, so pick
                // the split that keeps every `*` delimiter flanking. Bracket
                // spans can be split anywhere.
                let new_marks: Vec<&Mark> = marks
                    .iter()
                    .filter(|m| !active[..keep].contains(m))
                    .collect();
                let here_safe = boundary_safe(runs, k);
                if let Some(cut) = (0..keep).find(|&i| {
                    let a = &active[i];
                    let ea = extent(a);
                    // Cutting at `i` also closes every mark above it here.
                    let closable = here_safe || active[i..keep].iter().all(|m| !delim(m));
                    closable
                        && new_marks
                            .iter()
                            .any(|m| delim(m) && extent(m) > ea && !boundary_safe(runs, k + ea))
                }) {
                    keep = cut;
                }
                close_to(&mut active, keep, &mut out);
                let mut to_open: Vec<&Mark> =
                    marks.iter().filter(|m| !active.contains(m)).collect();
                // Marks that extend further open first (outermost).
                to_open.sort_by(|a, b| extent(b).cmp(&extent(a)).then(a.cmp(b)));
                if text.starts_with([' ', '\t']) && to_open.iter().any(|m| delim(m)) {
                    // Leading whitespace is emitted before the first emphasis
                    // opener; open bracket spans first so it stays inside them.
                    to_open.sort_by_key(|m| delim(m));
                }
                let mut rest: &str = text;
                for m in to_open {
                    if delim(m) {
                        // An opening `*` must not precede whitespace.
                        let body = rest.trim_start_matches([' ', '\t']);
                        if body.len() != rest.len() {
                            escape_text(
                                &rest[..rest.len() - body.len()],
                                line_start,
                                in_heading,
                                &mut out,
                            );
                            line_start = false;
                            rest = body;
                        }
                        // Whitespace-only run: open the delimiter at the next run instead.
                        if rest.is_empty() {
                            continue;
                        }
                    }
                    let o = opener(m, spans);
                    if o == "[" && out.ends_with('!') && !out.ends_with("\\!") {
                        out.insert(out.len() - 1, '\\');
                    }
                    out.push_str(&o);
                    active.push(m.clone());
                    line_start = false;
                }
                escape_text(rest, line_start, in_heading, &mut out);
                line_start = false;
            }
        }
    }
    close_to(&mut active, 0, &mut out);
    out
}

/// Serialize normalized inline content, verified: the output is parsed back
/// and, if any formatting would be lost (CommonMark cannot express some
/// overlaps of emphasis with spans and punctuation), the content is written
/// again with emphasis as bracket spans (`[x]{.em}`, `[x]{.strong}`), which
/// always round-trip.
pub fn serialize_inline(runs: &[Inline], in_heading: bool) -> String {
    // Register this content's footnotes once; both renderings number them
    // from the same base, and the verification parse looks them up.
    let base = SER_NOTES.with(|n| {
        let mut n = n.borrow_mut();
        let base = n.len();
        for r in runs {
            if let Inline::Footnote(c) = r {
                n.push(c.clone());
            }
        }
        base
    });
    let s = render_inline(runs, in_heading, false, base);
    let back = with_parse_notes(HashMap::new(), || {
        normalize_inline(&parse_inline(&s), in_heading)
    })
    .0;
    if erase_ws_emphasis(&back) == erase_ws_emphasis(runs) {
        s
    } else {
        render_inline(runs, in_heading, true, base)
    }
}

/// Inline content with emphasis removed from whitespace-only characters: the
/// one difference a delimiter round trip may introduce.
pub(crate) fn erase_ws_emphasis(runs: &[Inline]) -> Vec<Inline> {
    let mut out = Vec::new();
    for r in runs {
        match r {
            Inline::HardBreak => out.push(Inline::HardBreak),
            Inline::Footnote(c) => out.push(Inline::Footnote(erase_ws_emphasis(c))),
            Inline::Text { text, marks } => {
                let mut seg = String::new();
                let mut seg_ws: Option<bool> = None;
                let flush = |seg: &mut String, ws: bool, out: &mut Vec<Inline>| {
                    if !seg.is_empty() {
                        let m: Vec<Mark> = marks
                            .iter()
                            .filter(|m| !(ws && m.is_emphasis()))
                            .cloned()
                            .collect();
                        out.push(Inline::text(std::mem::take(seg), m));
                    }
                };
                for ch in text.chars() {
                    let ws = ch == ' ' || ch == '\t';
                    if let Some(prev) = seg_ws
                        && prev != ws
                    {
                        flush(&mut seg, prev, &mut out);
                    }
                    seg_ws = Some(ws);
                    seg.push(ch);
                }
                flush(&mut seg, seg_ws.unwrap_or(false), &mut out);
            }
        }
    }
    merge_runs(out)
}
