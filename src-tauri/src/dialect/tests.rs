use super::ast::{Block, Document, Inline, Mark};
use super::pm::{from_pm, to_pm};
use super::{normalize, parse, serialize, word_count};

fn t(s: &str) -> Inline {
    Inline::text(s, vec![])
}
fn tm(s: &str, marks: &[Mark]) -> Inline {
    Inline::text(s, marks.to_vec())
}
fn p(c: Vec<Inline>) -> Block {
    Block::Paragraph(c)
}

#[track_caller]
fn assert_canonical(md: &str) {
    let doc = parse(md);
    let out = serialize(&doc);
    assert_eq!(
        out, md,
        "canonical round trip failed\n--- doc ---\n{doc:#?}"
    );
    // And the document survives the editor bridge unchanged.
    assert_eq!(
        normalize(&from_pm(&to_pm(&doc))),
        doc,
        "PM bridge changed the document"
    );
}

#[test]
fn canonical_round_trips() {
    let cases = [
        "",
        "The harbor bells had stopped ringing by the time *Gull's Wake* came round the point.\n",
        "One.\n\nTwo.\n",
        "## A heading\n\nBody text.\n",
        "#### Deep heading\n",
        "Before.\n\n***\n\nAfter.\n",
        "*italic* and **bold** and ***both***\n",
        "**bold with *italic* inside** after\n",
        "*italic with **bold** inside*\n",
        "un*believ*able and **x**y\n",
        "[Small Caps]{.sc} and [no break]{.nobreak}\n",
        "[*italic small caps*]{.sc}\n",
        "A [link](https://example.com/a_b) here.\n",
        "A [spaced link](<https://example.com/a b>) here.\n",
        "Line one\\\nline two\\\nline three\n",
        "Literal \\*stars\\* and \\_underscores\\_ and \\[brackets\\] and \\\\ and \\<tag>.\n",
        "\\# not a heading\n",
        "\\> not a quote\n",
        "\\- not a list\n",
        "1984\\. was a year\n",
        "\\::: not a fence\n",
        "> Quoted paragraph.\n>\n> Second quoted paragraph.\n",
        "> Quote with\\\n> a hard break.\n",
        "::: epigraph\nThe sea is everything.\n\n::: attribution\nJules Verne\n:::\n:::\n",
        "::: letter\nDear Mara,\n\nCome home.\n:::\n",
        "::: verse\nRoses are red\\\nViolets are blue\n:::\n",
        "::: center\n:::\n",
        "- one\n- two\n- three\n",
        "- one\n\n- two\n",
        "1. first\n2. second\n",
        "3. third\n4. fourth\n",
        "- outer\n  - inner\n  - inner two\n- outer two\n",
        "- item\n\n  second paragraph\n\n- next\n",
        // A bracket span ending inside bold is split, not the bold.
        "a[ ]{.nobreak}**[*2026*]{.nobreak} bells ringing**\n",
        // Emphasis spans overlapping at a space: the space moves outside the delimiter.
        "**a, *b c*** *d*\n",
        "1. a\n   - b\n2. c\n",
        "- outer\n  - inner\n\n  - inner loose\n- outer two\n",
        "-\n- after empty\n",
        "Smart “quotes” — and an ellipsis…\n",
        "A *“quoted”* phrase.\n",
        "Ends with italic *word*.\n",
        "Paragraph hash # inside\n",
        "## Act \\#2\n",
        // Footnotes and comment anchors.
        "A sentence.[^1] Another.[^2]\n\n[^1]: The *first* note.\n[^2]: Second.\n",
        "## Heading[^1]\n\nBody.\n\n[^1]: Heading note.\n",
        "Note with a break.[^1]\n\n[^1]: Line one\\\n    line two\n",
        "Empty note.[^1]\n\n[^1]:\n",
        "Wow\\![^1]\n\n[^1]: Not an image.\n",
        "A <!-- c:01ABC -->commented<!-- /c --> word.\n",
        "<!-- c:x1 -->*Italic comment*<!-- /c --> and <!-- c:x2 -->[caps]{.sc}<!-- /c -->\n",
        "Literal \\<!-- c:x --> stays text.\n",
        "Literal \\[^1\\] stays text.\n",
    ];
    for md in cases {
        assert_canonical(md);
    }
}

#[test]
fn footnotes_renumber_and_keep_orphans() {
    let d = parse(
        "See[^note] here.\n\n[^note]: Named\n    continues.\n\n[^orphan]: Nobody refers to me.\n",
    );
    assert_eq!(
        serialize(&d),
        "See[^1] here.\n\n\\[^orphan\\]: Nobody refers to me.\n\n[^1]: Named continues.\n"
    );
    let Block::Paragraph(c) = &d.blocks[0] else {
        panic!("expected a paragraph")
    };
    assert_eq!(c[1], Inline::Footnote(vec![t("Named continues.")]));
    // A footnote counts as one character of plain text, like the editor's atom.
    assert_eq!(super::ast::inline_plain_text(c), "See\u{FFFC} here.");
    assert_eq!(word_count(&d), 7); // the orphan paragraph counts too
}

#[test]
fn comments_nest_and_drop_unmatched_anchors() {
    let d = parse(
        "<!-- c:a -->x <!-- c:b -->y<!-- /c --> z<!-- /c --> <!-- /c -->w <!-- c:dangling -->v\n",
    );
    let Block::Paragraph(c) = &d.blocks[0] else {
        panic!("expected a paragraph")
    };
    let a = Mark::Comment { id: "a".into() };
    let b = Mark::Comment { id: "b".into() };
    assert_eq!(
        c,
        &vec![
            tm("x ", std::slice::from_ref(&a)),
            tm("y", &[a.clone(), b]),
            tm(" z", &[a]),
            t(" w v"),
        ]
    );
}

#[test]
fn parses_marks() {
    let d = parse("a *b* **c** [d]{.sc} [e](u)");
    assert_eq!(
        d.blocks,
        vec![p(vec![
            t("a "),
            tm("b", &[Mark::Italic]),
            t(" "),
            tm("c", &[Mark::Bold]),
            t(" "),
            tm("d", &[Mark::SmallCaps]),
            t(" "),
            tm("e", &[Mark::Link { href: "u".into() }]),
        ])]
    );
}

#[test]
fn reads_external_markdown_forms() {
    // Underscore emphasis, soft-wrapped paragraphs, `* * *` breaks, `+` bullets.
    let d = parse("_it_ and __bold__\nwrapped line\n\n* * *\n\n+ a\n+ b\n");
    assert_eq!(
        serialize(&d),
        "*it* and **bold** wrapped line\n\n***\n\n- a\n- b\n"
    );
    // Two trailing spaces make a hard break.
    let d = parse("a  \nb\n");
    assert_eq!(serialize(&d), "a\\\nb\n");
    // CRLF line endings.
    let d = parse("One.\r\n\r\nTwo.\r\n");
    assert_eq!(serialize(&d), "One.\n\nTwo.\n");
    // Intraword underscores are not emphasis.
    let d = parse("snake_case_name");
    assert_eq!(d.blocks, vec![p(vec![t("snake_case_name")])]);
    // Spaced asterisks are not emphasis.
    let d = parse("2 * 3 * 4");
    assert_eq!(d.blocks, vec![p(vec![t("2 * 3 * 4")])]);
    // Emphasis written as spans (the serializer's fallback form) reads as emphasis.
    let d = parse("[a]{.em} [b]{.strong}");
    assert_eq!(
        d.blocks,
        vec![p(vec![
            tm("a", &[Mark::Italic]),
            t(" "),
            tm("b", &[Mark::Bold])
        ])]
    );
    assert_eq!(serialize(&d), "*a* **b**\n");
    // Unclosed fence runs to the end.
    let d = parse("::: verse\nline");
    assert_eq!(
        d.blocks,
        vec![Block::Div {
            class: "verse".into(),
            content: vec![p(vec![t("line")])]
        }]
    );
}

#[test]
fn normalization_moves_whitespace_out_of_emphasis() {
    let doc = Document {
        blocks: vec![p(vec![t("a"), tm(" b ", &[Mark::Italic]), t("c")])],
    };
    let n = normalize(&doc);
    assert_eq!(
        n.blocks,
        vec![p(vec![t("a "), tm("b", &[Mark::Italic]), t(" c")])]
    );
    assert_eq!(serialize(&doc), "a *b* c\n");
}

#[test]
fn normalization_drops_empty_paragraphs_and_edge_breaks() {
    let doc = Document {
        blocks: vec![
            p(vec![]),
            p(vec![Inline::HardBreak, t("  x  "), Inline::HardBreak]),
            p(vec![t("   ")]),
        ],
    };
    assert_eq!(normalize(&doc).blocks, vec![p(vec![t("x")])]);
}

#[test]
fn empty_document_maps_to_single_empty_paragraph() {
    let pm = to_pm(&Document::default());
    assert_eq!(pm.kind, "doc");
    assert_eq!(pm.content.as_ref().map(Vec::len), Some(1));
    assert_eq!(serialize(&from_pm(&pm)), "");
}

#[test]
fn pm_json_shape() {
    let doc = parse("## H\n\n*a*\n");
    let json = serde_json::to_value(to_pm(&doc)).expect("serialize");
    assert_eq!(
        json,
        serde_json::json!({
            "type": "doc",
            "content": [
                { "type": "heading", "attrs": { "level": 2.0 }, "content": [ { "type": "text", "text": "H" } ] },
                { "type": "paragraph", "content": [ { "type": "text", "text": "a", "marks": [ { "type": "italic" } ] } ] }
            ]
        })
    );
    // Editor JSON with extra attrs and unknown marks still maps.
    let from_editor: super::pm::PmNode = serde_json::from_value(serde_json::json!({
        "type": "doc",
        "content": [
            { "type": "paragraph", "content": [
                { "type": "text", "text": "x", "marks": [ { "type": "underline" }, { "type": "link", "attrs": { "href": "h", "target": null } } ] }
            ] },
            { "type": "mysteryBlock", "content": [ { "type": "paragraph", "content": [ { "type": "text", "text": "kept" } ] } ] }
        ]
    }))
    .expect("deserialize");
    assert_eq!(serialize(&from_pm(&from_editor)), "[x](h)\n\nkept\n");
}

#[test]
fn counts_words() {
    let d = parse("Hello, world — it's *nine* o'clock.\n\n***\n\n- one two\n");
    assert_eq!(word_count(&d), 7);
    assert_eq!(super::count_words("well—known … ok"), 3);
}

// ---------------------------------------------------------------------------
// Property test: random documents survive serialize → parse.
// ---------------------------------------------------------------------------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

const WORDS: &[&str] = &[
    "the",
    "harbor",
    "Mara",
    "bells",
    "had",
    "stopped",
    "ringing",
    "2026",
    "it's",
    "“quoted”",
    "end.",
    "comma,",
    "a*b",
    "snake_case",
    "[x]",
    "\\",
    "<tag>",
    "#5",
    "-",
    "1.",
    ":::",
    "…",
    "—",
    "well—known",
    "!",
    "café",
];

fn rand_marks(rng: &mut Rng) -> Vec<Mark> {
    let mut m = Vec::new();
    if rng.chance(25) {
        m.push(Mark::Italic);
    }
    if rng.chance(15) {
        m.push(Mark::Bold);
    }
    if rng.chance(8) {
        m.push(Mark::SmallCaps);
    }
    if rng.chance(5) {
        m.push(Mark::NoBreak);
    }
    if rng.chance(5) {
        m.push(Mark::Link {
            href: if rng.chance(50) {
                "https://x.y/a".into()
            } else {
                "a b(c)".into()
            },
        });
    }
    if rng.chance(6) {
        m.push(Mark::Comment {
            id: if rng.chance(50) {
                "c1".into()
            } else {
                "c2".into()
            },
        });
    }
    m
}

fn rand_inline(rng: &mut Rng, allow_break: bool) -> Vec<Inline> {
    let mut out = Vec::new();
    let runs = 1 + rng.below(6);
    for k in 0..runs {
        if allow_break && k > 0 && rng.chance(10) {
            out.push(Inline::HardBreak);
        }
        let words = 1 + rng.below(4);
        let mut s = String::new();
        // Runs are joined at word boundaries (see the module's known limitation).
        if k > 0 {
            s.push(' ');
        }
        for w in 0..words {
            if w > 0 {
                s.push(' ');
            }
            s.push_str(WORDS[rng.below(WORDS.len() as u64) as usize]);
        }
        if rng.chance(20) {
            s.push(' ');
        }
        out.push(Inline::text(s, rand_marks(rng)));
        if allow_break && rng.chance(6) {
            out.push(Inline::Footnote(rand_inline(rng, false)));
        }
    }
    out
}

fn rand_blocks(rng: &mut Rng, depth: u32) -> Vec<Block> {
    let n = 1 + rng.below(4);
    (0..n)
        .map(|_| {
            let roll = if depth >= 2 {
                rng.below(60)
            } else {
                rng.below(100)
            };
            match roll {
                0..=44 => Block::Paragraph(rand_inline(rng, true)),
                45..=51 => Block::Heading {
                    level: 1 + rng.below(6) as u8,
                    content: rand_inline(rng, true),
                },
                52..=59 => Block::SceneBreak,
                60..=69 => Block::BlockQuote(rand_blocks(rng, depth + 1)),
                70..=79 => Block::Div {
                    class: ["epigraph", "letter", "verse", "center", "textmsg"]
                        [rng.below(5) as usize]
                        .into(),
                    content: rand_blocks(rng, depth + 1),
                },
                80..=89 => Block::BulletList {
                    tight: rng.chance(50),
                    items: (0..1 + rng.below(3))
                        .map(|_| rand_blocks(rng, depth + 1))
                        .collect(),
                },
                _ => Block::OrderedList {
                    tight: rng.chance(50),
                    start: rng.below(12) as u32,
                    items: (0..1 + rng.below(3))
                        .map(|_| rand_blocks(rng, depth + 1))
                        .collect(),
                },
            }
        })
        .collect()
}

#[test]
fn random_documents_round_trip() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for case in 0..3000 {
        let doc = Document {
            blocks: rand_blocks(&mut rng, 0),
        };
        let norm = normalize(&doc);
        assert_eq!(
            normalize(&norm),
            norm,
            "normalize not idempotent (case {case})"
        );
        let md = serialize(&doc);
        let back = parse(&md);
        let (got, want) = (
            format!("{:?}", erase_ws_emphasis(&back)),
            format!("{:?}", erase_ws_emphasis(&norm)),
        );
        if got != want {
            let at = got
                .chars()
                .zip(want.chars())
                .take_while(|(a, b)| a == b)
                .count();
            let ctx = |s: &str| {
                s.chars()
                    .skip(at.saturating_sub(150))
                    .take(300)
                    .collect::<String>()
            };
            panic!(
                "case {case}: parse(serialize(doc)) != normalize(doc)\n--- md ---\n{md}\n--- got ---\n{}\n--- want ---\n{}",
                ctx(&got),
                ctx(&want)
            );
        }
        let again = serialize(&back);
        if again != md {
            let at = again
                .chars()
                .zip(md.chars())
                .take_while(|(a, b)| a == b)
                .count();
            let ctx = |s: &str| {
                s.chars()
                    .skip(at.saturating_sub(60))
                    .take(120)
                    .collect::<String>()
            };
            panic!(
                "case {case}: canonical md not stable\n--- first ---\n{}\n--- again ---\n{}",
                ctx(&md),
                ctx(&again)
            );
        }
        assert_eq!(
            normalize(&from_pm(&to_pm(&norm))),
            norm,
            "case {case}: PM bridge"
        );
    }
}

/// The dialect contract holds exactly except for emphasis on whitespace-only
/// characters at a point where two emphasis spans overlap (see `serialize_inline`).
fn erase_ws_emphasis(doc: &Document) -> Document {
    fn inline(c: &[Inline]) -> Vec<Inline> {
        super::inline::erase_ws_emphasis(c)
    }
    fn blocks(b: &[Block]) -> Vec<Block> {
        b.iter()
            .map(|b| match b {
                Block::Paragraph(c) => Block::Paragraph(inline(c)),
                Block::Heading { level, content } => Block::Heading {
                    level: *level,
                    content: inline(content),
                },
                Block::SceneBreak => Block::SceneBreak,
                Block::BlockQuote(i) => Block::BlockQuote(blocks(i)),
                Block::Div { class, content } => Block::Div {
                    class: class.clone(),
                    content: blocks(content),
                },
                Block::BulletList { tight, items } => Block::BulletList {
                    tight: *tight,
                    items: items.iter().map(|i| blocks(i)).collect(),
                },
                Block::OrderedList {
                    tight,
                    start,
                    items,
                } => Block::OrderedList {
                    tight: *tight,
                    start: *start,
                    items: items.iter().map(|i| blocks(i)).collect(),
                },
            })
            .collect()
    }
    Document {
        blocks: blocks(&doc.blocks),
    }
}
