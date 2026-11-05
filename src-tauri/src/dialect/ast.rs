//! Typed document AST for the Margin Markdown dialect (SPEC §13.2).
//!
//! Inline content is *flat*: a sequence of text runs, each carrying a set of
//! marks, exactly like ProseMirror. This makes the mapping to and from the
//! editor trivial and keeps mark nesting a serializer concern.

/// An inline mark. The derive order defines the canonical mark order
/// (outermost first when spans start at the same position).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mark {
    /// Inline comment anchor (`<!-- c:<id> -->` … `<!-- /c -->`). The
    /// comment body lives in `comments/<docId>.json`, never in the Markdown.
    Comment {
        id: String,
    },
    Link {
        href: String,
    },
    SmallCaps,
    NoBreak,
    Bold,
    Italic,
}

impl Mark {
    /// Marks written with `*`/`**` delimiters. Their spans must not start or
    /// end with whitespace, or the delimiters would not be flanking.
    pub fn is_emphasis(&self) -> bool {
        matches!(self, Mark::Bold | Mark::Italic)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    /// A run of text. `marks` is kept sorted and deduplicated.
    Text {
        text: String,
        marks: Vec<Mark>,
    },
    HardBreak,
    /// A footnote reference with its (single-paragraph) content. Written as
    /// `[^N]` in the text and `[^N]: content` at the end of the body.
    Footnote(Vec<Inline>),
}

/// Plain-text stand-in for a footnote reference, so text offsets match the
/// editor (where a footnote is one inline atom).
pub const FOOTNOTE_CHAR: char = '\u{FFFC}';

impl Inline {
    pub fn text(text: impl Into<String>, mut marks: Vec<Mark>) -> Self {
        marks.sort();
        marks.dedup();
        Inline::Text {
            text: text.into(),
            marks,
        }
    }
}

/// Classes allowed on fenced divs (`::: epigraph` … `:::`).
pub const DIV_CLASSES: &[&str] = &[
    "epigraph",
    "attribution",
    "letter",
    "verse",
    "center",
    "textmsg",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Inline>),
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    SceneBreak,
    BlockQuote(Vec<Block>),
    /// Fenced div with a semantic style class (see [`DIV_CLASSES`]).
    Div {
        class: String,
        content: Vec<Block>,
    },
    BulletList {
        tight: bool,
        items: Vec<Vec<Block>>,
    },
    OrderedList {
        tight: bool,
        start: u32,
        items: Vec<Vec<Block>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
}

/// Plain text of inline content (marks dropped, hard breaks become `\n`).
pub fn inline_plain_text(content: &[Inline]) -> String {
    let mut out = String::new();
    for i in content {
        match i {
            Inline::Text { text, .. } => out.push_str(text),
            Inline::HardBreak => out.push('\n'),
            Inline::Footnote(_) => out.push(FOOTNOTE_CHAR),
        }
    }
    out
}

impl Document {
    /// Visit the plain text of every textual block, in document order.
    pub fn for_each_text(&self, f: &mut dyn FnMut(&str)) {
        fn walk(blocks: &[Block], f: &mut dyn FnMut(&str)) {
            for b in blocks {
                match b {
                    Block::Paragraph(c) | Block::Heading { content: c, .. } => {
                        f(&inline_plain_text(c))
                    }
                    Block::SceneBreak => {}
                    Block::BlockQuote(inner) | Block::Div { content: inner, .. } => walk(inner, f),
                    Block::BulletList { items, .. } | Block::OrderedList { items, .. } => {
                        for item in items {
                            walk(item, f);
                        }
                    }
                }
            }
        }
        walk(&self.blocks, f);
    }

    /// Visit the inline content of every textual block, in the same order
    /// as [`Self::for_each_text`].
    pub fn for_each_text_block_mut(&mut self, f: &mut dyn FnMut(&mut Vec<Inline>)) {
        fn walk(blocks: &mut [Block], f: &mut dyn FnMut(&mut Vec<Inline>)) {
            for b in blocks {
                match b {
                    Block::Paragraph(c) | Block::Heading { content: c, .. } => f(c),
                    Block::SceneBreak => {}
                    Block::BlockQuote(inner) | Block::Div { content: inner, .. } => walk(inner, f),
                    Block::BulletList { items, .. } | Block::OrderedList { items, .. } => {
                        for item in items {
                            walk(item, f);
                        }
                    }
                }
            }
        }
        walk(&mut self.blocks, f);
    }
}

/// Merge adjacent text runs with identical marks and drop empty runs.
pub fn merge(runs: Vec<Inline>) -> Vec<Inline> {
    super::inline::merge_runs(runs)
}
