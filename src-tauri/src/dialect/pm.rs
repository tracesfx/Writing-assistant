//! ProseMirror-shaped JSON exchanged with the editor.
//!
//! The node and mark names here are the editor schema's names
//! (`src/editor/schema.ts`). Unknown nodes are degraded gracefully so text
//! is never dropped: unknown blocks are unwrapped, unknown inline nodes keep
//! their text, unknown marks are ignored.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::ast::{Block, Document, Inline, Mark};
use super::inline::{parse_inline_plain, serialize_inline, with_ser_notes};

/// Footnote content travels to the editor as inline Markdown in the
/// `text` attribute of an inline atom node `footnote`.
fn note_source(c: &[Inline]) -> String {
    with_ser_notes(|| serialize_inline(&super::inline::normalize_inline(c, false), false)).0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(untagged)]
pub enum AttrValue {
    Bool(bool),
    Number(f64),
    String(String),
    Null,
}

pub type PmAttrs = BTreeMap<String, AttrValue>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct PmMark {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attrs: Option<PmAttrs>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct PmNode {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attrs: Option<PmAttrs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<PmNode>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marks: Option<Vec<PmMark>>,
}

impl PmNode {
    fn new(kind: &str) -> Self {
        PmNode {
            kind: kind.to_string(),
            attrs: None,
            content: None,
            text: None,
            marks: None,
        }
    }
    fn with_content(kind: &str, content: Vec<PmNode>) -> Self {
        PmNode {
            content: Some(content),
            ..Self::new(kind)
        }
    }
    fn with_attrs(mut self, attrs: PmAttrs) -> Self {
        self.attrs = Some(attrs);
        self
    }
    fn attr_str(&self, key: &str) -> Option<&str> {
        match self.attrs.as_ref()?.get(key)? {
            AttrValue::String(s) => Some(s),
            _ => None,
        }
    }
    fn attr_num(&self, key: &str) -> Option<f64> {
        match self.attrs.as_ref()?.get(key)? {
            AttrValue::Number(n) => Some(*n),
            _ => None,
        }
    }
    fn attr_bool(&self, key: &str) -> Option<bool> {
        match self.attrs.as_ref()?.get(key)? {
            AttrValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
    fn children(&self) -> &[PmNode] {
        self.content.as_deref().unwrap_or(&[])
    }
}

fn attrs(pairs: &[(&str, AttrValue)]) -> PmAttrs {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// Document → PM
// ---------------------------------------------------------------------------

pub fn to_pm(doc: &Document) -> PmNode {
    let mut blocks = blocks_to_pm(&doc.blocks);
    if blocks.is_empty() {
        blocks.push(PmNode::new("paragraph"));
    }
    PmNode::with_content("doc", blocks)
}

fn blocks_to_pm(blocks: &[Block]) -> Vec<PmNode> {
    blocks.iter().map(block_to_pm).collect()
}

/// Container nodes in the editor schema require at least one block.
fn non_empty(mut v: Vec<PmNode>) -> Vec<PmNode> {
    if v.is_empty() {
        v.push(PmNode::new("paragraph"));
    }
    v
}

fn block_to_pm(b: &Block) -> PmNode {
    match b {
        Block::Paragraph(c) => para(c),
        Block::Heading { level, content } => {
            let mut n = PmNode::new("heading")
                .with_attrs(attrs(&[("level", AttrValue::Number(*level as f64))]));
            n.content = inline_to_pm(content);
            n
        }
        Block::SceneBreak => PmNode::new("sceneBreak"),
        Block::BlockQuote(inner) => {
            PmNode::with_content("blockquote", non_empty(blocks_to_pm(inner)))
        }
        Block::Div { class, content } => {
            PmNode::with_content("styledBlock", non_empty(blocks_to_pm(content)))
                .with_attrs(attrs(&[("style", AttrValue::String(class.clone()))]))
        }
        Block::BulletList { tight, items } => {
            PmNode::with_content("bulletList", items_to_pm(items))
                .with_attrs(attrs(&[("tight", AttrValue::Bool(*tight))]))
        }
        Block::OrderedList {
            tight,
            start,
            items,
        } => PmNode::with_content("orderedList", items_to_pm(items)).with_attrs(attrs(&[
            ("tight", AttrValue::Bool(*tight)),
            ("start", AttrValue::Number(*start as f64)),
        ])),
    }
}

fn para(c: &[Inline]) -> PmNode {
    let mut n = PmNode::new("paragraph");
    n.content = inline_to_pm(c);
    n
}

fn items_to_pm(items: &[Vec<Block>]) -> Vec<PmNode> {
    items
        .iter()
        .map(|item| {
            let mut blocks = blocks_to_pm(item);
            // The editor's listItem must start with a paragraph.
            if blocks.first().is_none_or(|b| b.kind != "paragraph") {
                blocks.insert(0, PmNode::new("paragraph"));
            }
            PmNode::with_content("listItem", blocks)
        })
        .collect()
}

fn inline_to_pm(c: &[Inline]) -> Option<Vec<PmNode>> {
    if c.is_empty() {
        return None;
    }
    Some(
        c.iter()
            .map(|i| match i {
                Inline::HardBreak => PmNode::new("hardBreak"),
                Inline::Footnote(c) => PmNode::new("footnote")
                    .with_attrs(attrs(&[("text", AttrValue::String(note_source(c)))])),
                Inline::Text { text, marks } => {
                    let mut n = PmNode::new("text");
                    n.text = Some(text.clone());
                    if !marks.is_empty() {
                        n.marks = Some(marks.iter().map(mark_to_pm).collect());
                    }
                    n
                }
            })
            .collect(),
    )
}

fn mark_to_pm(m: &Mark) -> PmMark {
    let (kind, a) = match m {
        Mark::Comment { id } => (
            "comment",
            Some(attrs(&[("id", AttrValue::String(id.clone()))])),
        ),
        Mark::Link { href } => (
            "link",
            Some(attrs(&[("href", AttrValue::String(href.clone()))])),
        ),
        Mark::SmallCaps => ("smallCaps", None),
        Mark::NoBreak => ("noBreak", None),
        Mark::Bold => ("bold", None),
        Mark::Italic => ("italic", None),
    };
    PmMark {
        kind: kind.to_string(),
        attrs: a,
    }
}

// ---------------------------------------------------------------------------
// PM → Document
// ---------------------------------------------------------------------------

pub fn from_pm(node: &PmNode) -> Document {
    let blocks = if node.kind == "doc" {
        pm_to_blocks(node.children())
    } else {
        pm_to_blocks(std::slice::from_ref(node))
    };
    Document { blocks }
}

fn pm_to_blocks(nodes: &[PmNode]) -> Vec<Block> {
    let mut out = Vec::new();
    let mut pending_inline: Vec<PmNode> = Vec::new();
    let flush = |pending: &mut Vec<PmNode>, out: &mut Vec<Block>| {
        if !pending.is_empty() {
            out.push(Block::Paragraph(pm_to_inline(pending)));
            pending.clear();
        }
    };
    for n in nodes {
        match n.kind.as_str() {
            "paragraph" => {
                flush(&mut pending_inline, &mut out);
                out.push(Block::Paragraph(pm_to_inline(n.children())));
            }
            "heading" => {
                flush(&mut pending_inline, &mut out);
                let level = n.attr_num("level").unwrap_or(2.0).clamp(1.0, 6.0) as u8;
                out.push(Block::Heading {
                    level,
                    content: pm_to_inline(n.children()),
                });
            }
            "sceneBreak" | "horizontalRule" => {
                flush(&mut pending_inline, &mut out);
                out.push(Block::SceneBreak);
            }
            "blockquote" => {
                flush(&mut pending_inline, &mut out);
                out.push(Block::BlockQuote(pm_to_blocks(n.children())));
            }
            "styledBlock" => {
                flush(&mut pending_inline, &mut out);
                let class = n.attr_str("style").unwrap_or("center").to_string();
                out.push(Block::Div {
                    class,
                    content: pm_to_blocks(n.children()),
                });
            }
            "bulletList" | "orderedList" => {
                flush(&mut pending_inline, &mut out);
                let items: Vec<Vec<Block>> = n
                    .children()
                    .iter()
                    .map(|li| pm_to_blocks(li.children()))
                    .collect();
                let tight = n.attr_bool("tight").unwrap_or(true);
                if n.kind == "bulletList" {
                    out.push(Block::BulletList { tight, items });
                } else {
                    let start = n.attr_num("start").unwrap_or(1.0).max(0.0) as u32;
                    out.push(Block::OrderedList {
                        tight,
                        start,
                        items,
                    });
                }
            }
            "text" | "hardBreak" | "footnote" => pending_inline.push(n.clone()),
            _ => {
                // Unknown node: keep its text by unwrapping.
                flush(&mut pending_inline, &mut out);
                if n.text.is_some() {
                    pending_inline.push(n.clone());
                } else {
                    out.extend(pm_to_blocks(n.children()));
                }
            }
        }
    }
    flush(&mut pending_inline, &mut out);
    out
}

fn pm_to_inline(nodes: &[PmNode]) -> Vec<Inline> {
    let mut out = Vec::new();
    for n in nodes {
        match n.kind.as_str() {
            "hardBreak" => out.push(Inline::HardBreak),
            "footnote" => out.push(Inline::Footnote(parse_inline_plain(
                n.attr_str("text").unwrap_or(""),
            ))),
            _ => {
                if let Some(t) = &n.text {
                    let marks = n
                        .marks
                        .as_deref()
                        .unwrap_or(&[])
                        .iter()
                        .filter_map(pm_to_mark)
                        .collect();
                    out.push(Inline::text(t.clone(), marks));
                } else {
                    out.extend(pm_to_inline(n.children()));
                }
            }
        }
    }
    out
}

fn pm_to_mark(m: &PmMark) -> Option<Mark> {
    Some(match m.kind.as_str() {
        "italic" => Mark::Italic,
        "bold" => Mark::Bold,
        "smallCaps" => Mark::SmallCaps,
        "noBreak" => Mark::NoBreak,
        "comment" => {
            let id = match m.attrs.as_ref()?.get("id")? {
                AttrValue::String(s) if super::inline::is_comment_id(s) => s.clone(),
                _ => return None,
            };
            Mark::Comment { id }
        }
        "link" => {
            let href = match m.attrs.as_ref()?.get("href")? {
                AttrValue::String(s) => s.clone(),
                _ => return None,
            };
            Mark::Link { href }
        }
        _ => return None,
    })
}
