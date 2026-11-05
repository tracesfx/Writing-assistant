//! The Margin Markdown dialect (SPEC §13.2).
//!
//! Losslessness contract (SPEC §1.2):
//! - `serialize(parse(md)) == md` for canonical files (anything this module wrote);
//! - `parse(serialize(doc)) == normalize(doc)` for any document, except that
//!   where two emphasis spans overlap at a space, that space may lose its
//!   (invisible) emphasis.
//!
//! Every paragraph is verified as it is serialized. Overlaps that CommonMark
//! `*` delimiters cannot express (emphasis crossing spans or links, emphasis
//! inside a word next to punctuation) fall back to pandoc spans
//! `[x]{.em}` / `[x]{.strong}` for that paragraph, so formatting is never lost.

pub mod ast;
mod block;
mod inline;
pub mod pm;

pub use ast::{Block, Document, Inline, Mark};
pub use block::normalize;

/// Parse a Markdown body (no frontmatter).
pub fn parse(body: &str) -> Document {
    block::parse_body(body)
}

/// Serialize a document to its canonical Markdown body.
pub fn serialize(doc: &Document) -> String {
    block::serialize_body(doc)
}

/// Word count of a document: whitespace- and dash-separated tokens that
/// contain at least one letter or digit.
pub fn word_count(doc: &Document) -> u32 {
    let mut n = 0u32;
    doc.for_each_text(&mut |t| n = n.saturating_add(count_words(t)));
    n
}

pub fn count_words(text: &str) -> u32 {
    text.split(|c: char| c.is_whitespace() || c == '—' || c == '–')
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count() as u32
}

#[cfg(test)]
mod tests;
