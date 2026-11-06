//! Ghost text (SPEC §7): prompt construction (§7.4) and post-processing (§7.5).
//!
//! Pure functions, so they are unit-tested without a model. The AI layer only
//! produces a suggestion as data; the editor shows it as a decoration and
//! inserts it only when the author accepts (SPEC §2.1).

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::types::{Prompt, PromptSection, Sampling};
use crate::project::manifest::Locale;

/// How much to suggest (SPEC §7.1). Phrase is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GhostLength {
    Word,
    Phrase,
    Sentence,
}

/// Which smart-typography rules the author has on (SPEC §11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Typography {
    pub quotes: bool,
    pub dashes: bool,
    pub ellipsis: bool,
}

/// One ghost-text request from the editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GhostRequest {
    /// Chosen by the editor so it can match results to the pause that asked.
    pub request_id: String,
    pub doc_id: String,
    /// The document's plain text up to the cursor; paragraphs are separated by a blank line.
    pub text_before: String,
    pub length: GhostLength,
    /// 0 is the first suggestion; 1 and 2 are the lazy alternatives (SPEC §7.2).
    pub variant: u32,
    pub typography: Typography,
}

/// What the context builder knows about the project and the scene.
#[derive(Debug, Clone, PartialEq)]
pub struct GhostContext {
    pub project_title: String,
    pub locale: Locale,
    pub doc_title: String,
    pub pov: Option<String>,
    pub synopsis: Option<String>,
}

/// Ghost-text prompt budget (SPEC §7.4): ~8K tokens, [1–2] ≤ 800, [3] ≤ 600.
pub const BUDGET_TOKENS: u32 = 8000;
const PRIMER_MAX: u32 = 800;
const NOTES_MAX: u32 = 600;
/// Room for the completion and the template's own markers.
const RESERVE_TOKENS: u32 = 600;
/// The prefilled assistant text (`[6]`) may take at most this share of what is left.
const PREFILL_SHARE: f64 = 0.4;

pub const ROLE: &str = "You are a silent co-writer. Continue the author's text exactly where it stops, matching their voice, tense, point of view and spelling. Output only the continuation. Never summarise, explain, or add commentary.";

/// A rough token count. Margin has no tokenizer in Rust, so it assumes 3.6
/// characters per token, which errs on the high side for English prose.
pub fn estimate_tokens(s: &str) -> u32 {
    (s.chars().count() as f64 / 3.6).ceil() as u32
}

/// The prompt, plus what the post-processor needs to know about the author's text.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltPrompt {
    pub prompt: Prompt,
    /// Everything the author has typed before the cursor, as the editor sent it.
    pub before: String,
}

fn spelling_line(locale: Locale) -> &'static str {
    match locale {
        Locale::EnCa => "Spelling: Canadian English (colour, centre, -ize, travelled).",
        Locale::EnUs => "Spelling: American English (color, center, -ize, traveled).",
    }
}

/// Keep the end of `s` within `max_tokens`, cutting at a sentence or word start.
fn tail_within(s: &str, max_tokens: u32) -> &str {
    if estimate_tokens(s) <= max_tokens {
        return s;
    }
    let max_chars = (max_tokens as f64 * 3.6) as usize;
    let total = s.chars().count();
    let skip = total.saturating_sub(max_chars);
    let start = s.char_indices().nth(skip).map_or(s.len(), |(i, _)| i);
    let tail = &s[start..];
    // Prefer to start after a sentence end, else after a space.
    if let Some(i) = tail.find(['.', '!', '?'])
        && let Some(rest) = tail.get(i + 1..)
        && rest.starts_with(' ')
        && estimate_tokens(rest.trim_start()) + 8 < max_tokens
    {
        return rest.trim_start();
    }
    match tail.find(' ') {
        Some(i) => tail[i + 1..].trim_start(),
        None => tail,
    }
}

/// Build the ordered, cache-friendly prompt (SPEC §7.4). `budget` is the most
/// tokens the whole prompt may use.
pub fn build_prompt(ctx: &GhostContext, text_before: &str, budget: u32) -> BuiltPrompt {
    let budget = budget.min(BUDGET_TOKENS);

    // [1] Role instructions and [2] project primer: stable for the session.
    let role = ROLE.to_string();
    let mut primer = format!(
        "Project: {}.\n{}",
        ctx.project_title.trim(),
        spelling_line(ctx.locale)
    );
    primer = tail_within(&primer, PRIMER_MAX).to_string();

    // [3] Scene notes: change at scene boundaries only.
    let mut notes = format!("Scene: {}", ctx.doc_title.trim());
    if let Some(p) = ctx.pov.as_deref().filter(|p| !p.trim().is_empty()) {
        notes.push_str(&format!("\nPoint of view: {}", p.trim()));
    }
    if let Some(s) = ctx.synopsis.as_deref().filter(|s| !s.trim().is_empty()) {
        notes.push_str(&format!("\nSynopsis: {}", s.trim()));
    }
    let notes = tail_within(&notes, NOTES_MAX).to_string();

    // [5] Earlier scene text and [6] the prefilled assistant turn.
    let paragraphs: Vec<&str> = text_before.split("\n\n").collect();
    let fixed = estimate_tokens(&role) + estimate_tokens(&primer) + estimate_tokens(&notes) + 40;
    let room = budget.saturating_sub(fixed + RESERVE_TOKENS).max(200);

    let split = paragraphs.len().saturating_sub(2);
    let prefill_full = paragraphs[split..].join("\n\n");
    let prefill_max = ((room as f64) * PREFILL_SHARE) as u32;
    let prefill = tail_within(&prefill_full, prefill_max.max(100));
    // Whitespace at the very end of the prefill hurts tokenization; the
    // post-processor re-adds or removes the space (see `fix_leading_space`).
    let assistant_prefix = prefill.trim_end().to_string();

    let earlier_room = room.saturating_sub(estimate_tokens(&assistant_prefix));
    let mut kept: Vec<&str> = vec![];
    let mut used = 0;
    for p in paragraphs[..split].iter().rev() {
        let t = estimate_tokens(p) + 1;
        if used + t > earlier_room {
            break;
        }
        used += t;
        kept.push(p);
    }
    let omitted = kept.len() < split;
    kept.reverse();
    let earlier = kept.join("\n\n");

    let scene_text = if earlier.is_empty() {
        "The scene begins in the reply below.".to_string()
    } else if omitted {
        format!("The scene so far (earlier parts omitted):\n\n{earlier}")
    } else {
        format!("The scene so far:\n\n{earlier}")
    };
    let user = format!("{notes}\n\n{scene_text}");
    let system = format!("{role}\n\n{primer}");

    let section = |label: &str, text: &str| PromptSection {
        label: label.into(),
        text: text.into(),
        tokens: estimate_tokens(text),
    };
    BuiltPrompt {
        prompt: Prompt {
            system,
            user,
            assistant_prefix: assistant_prefix.clone(),
            sections: vec![
                section("[1] Role instructions", &role),
                section("[2] Project primer", &primer),
                section("[3] Scene notes", &notes),
                section("[5] Scene text", &scene_text),
                section("[6] Assistant prefill", &assistant_prefix),
            ],
        },
        before: text_before.to_string(),
    }
}

/// Sampling for a ghost request (SPEC §6.5): the length sets the token cap and
/// each alternative is warmer and looser, so it differs from the one before.
pub fn sampling_for(base: Sampling, length: GhostLength, variant: u32) -> Sampling {
    let mut s = base;
    s.max_tokens = match length {
        GhostLength::Word => 8,
        GhostLength::Phrase => s.max_tokens.max(20),
        GhostLength::Sentence => 56,
    };
    // Alternatives: hotter and with a looser cut-off, so they differ from the first one.
    let v = variant.min(2) as f64;
    s.temperature = (s.temperature + 0.3 * v).min(1.3);
    if variant > 0 {
        s.min_p = s.min_p.min(0.02);
        s.top_p = s.top_p.max(0.95);
    }
    s
}

// --- Post-processing (SPEC §7.5) ----------------------------------------------------

/// What the post-processor needs.
#[derive(Debug, Clone, Copy)]
pub struct PostCtx<'a> {
    /// The author's text before the cursor, exactly as typed (trailing space included).
    pub before: &'a str,
    pub length: GhostLength,
    pub locale: Locale,
    pub typography: Typography,
    /// The model may have stopped mid-word (it hit the token cap, or it is still
    /// streaming). The last word is then dropped.
    pub truncated: bool,
}

const TEMPLATE_MARKERS: [&str; 9] = [
    "<|", "|>", "<turn", "turn|>", "[INST]", "[/INST]", "</s>", "<think", "</think",
];

const META_STARTS: [&str; 16] = [
    "sure,",
    "sure!",
    "sure.",
    "certainly,",
    "certainly!",
    "of course,",
    "of course!",
    "here is",
    "here's",
    "here are",
    "i'm sorry",
    "i am sorry",
    "as an ai",
    "as a language model",
    "continuation:",
    "okay,",
];

const ABBREVIATIONS: [&str; 12] = [
    "mr.", "mrs.", "ms.", "dr.", "st.", "sr.", "jr.", "prof.", "vs.", "etc.", "no.", "mt.",
];

const CLOSERS: &[char] = &['"', '\'', '”', '’', ')', ']'];

fn is_sentence_end(word: &str) -> bool {
    let w = word.trim_end_matches(CLOSERS);
    if !w.ends_with(['.', '!', '?']) || w.ends_with("..") || w.ends_with('…') {
        return false;
    }
    let lower = w.to_lowercase();
    if ABBREVIATIONS.contains(&lower.as_str()) {
        return false;
    }
    // An initial such as "J." is not the end of a sentence.
    let letters: Vec<char> = w.trim_end_matches(['.', '!', '?']).chars().collect();
    !(w.ends_with('.') && letters.len() == 1 && letters[0].is_uppercase())
}

fn is_clause_end(word: &str) -> bool {
    let w = word.trim_end_matches(CLOSERS);
    w.ends_with([',', ';', ':', '—']) || is_sentence_end(word)
}

fn norm_word(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

/// Byte offset just after the `k`-th word of `s`.
fn end_of_word(s: &str, k: usize) -> Option<usize> {
    let mut seen = 0;
    let mut in_word = false;
    for (i, c) in s.char_indices() {
        if c.is_whitespace() {
            if in_word {
                seen += 1;
                in_word = false;
                if seen == k {
                    return Some(i);
                }
            }
        } else {
            in_word = true;
        }
    }
    (in_word && seen + 1 == k).then_some(s.len())
}

/// Drop words at the start of `body` that repeat the author's last words.
fn strip_repeat<'a>(before: &str, body: &'a str) -> &'a str {
    let prev: Vec<String> = before.split_whitespace().map(norm_word).collect();
    let words: Vec<String> = body.split_whitespace().map(norm_word).collect();
    for k in (1..=6.min(prev.len()).min(words.len())).rev() {
        let tail = &prev[prev.len() - k..];
        // One repeated word only counts when it is a real word, not "the" or "a".
        let enough = k >= 2 || tail[0].chars().count() >= 5;
        if enough
            && tail == &words[..k]
            && let Some(end) = end_of_word(body, k)
        {
            return &body[end..];
        }
    }
    body
}

/// Fix the space between the author's text and the suggestion.
fn fix_leading_space(before: &str, s: &str) -> String {
    let last = before.chars().last();
    let body = s.trim_start();
    let starts_word = body.chars().next().is_some_and(|c| c.is_alphanumeric());
    match last {
        // After a space, a line start or an opening quote or bracket: no leading space.
        None => body.to_string(),
        Some(c) if c.is_whitespace() || "([{“‘\"'—-".contains(c) => body.to_string(),
        // After sentence punctuation or a closing quote a new word needs a space.
        Some(c) if (".!?,;:”’)]".contains(c) || c == '…') && starts_word => {
            format!(" {body}")
        }
        // Mid-word: the model continues the word, or starts a new one with a space.
        Some(_) => s.to_string(),
    }
}

static SPELLING: LazyLock<(HashMap<String, String>, HashMap<String, String>)> =
    LazyLock::new(|| {
        // (American, Canadian) stems; suffixes are added to both.
        const STEMS: &[(&str, &str)] = &[
            ("color", "colour"),
            ("favor", "favour"),
            ("honor", "honour"),
            ("neighbor", "neighbour"),
            ("labor", "labour"),
            ("humor", "humour"),
            ("rumor", "rumour"),
            ("harbor", "harbour"),
            ("behavior", "behaviour"),
            ("flavor", "flavour"),
            ("savior", "saviour"),
            ("vapor", "vapour"),
            ("odor", "odour"),
            ("armor", "armour"),
            ("candor", "candour"),
            ("clamor", "clamour"),
            ("glamor", "glamour"),
            ("rigor", "rigour"),
            ("valor", "valour"),
            ("vigor", "vigour"),
            ("parlor", "parlour"),
            ("tumor", "tumour"),
            ("endeavor", "endeavour"),
            ("splendor", "splendour"),
            ("ardor", "ardour"),
            ("demeanor", "demeanour"),
            ("fervor", "fervour"),
            ("center", "centre"),
            ("theater", "theatre"),
            ("fiber", "fibre"),
            ("somber", "sombre"),
            ("luster", "lustre"),
            ("saber", "sabre"),
            ("caliber", "calibre"),
            ("meager", "meagre"),
            ("specter", "spectre"),
            ("mold", "mould"),
            ("plow", "plough"),
            ("smolder", "smoulder"),
            ("defense", "defence"),
            ("offense", "offence"),
            ("pretense", "pretence"),
            ("jewelry", "jewellery"),
            ("travel", "travell"),
            ("cancel", "cancell"),
            ("label", "labell"),
            ("level", "levell"),
            ("model", "modell"),
            ("signal", "signall"),
            ("counselor", "counsellor"),
        ];
        const SUFFIXES: &[&str] = &[
            "", "s", "ed", "ing", "ful", "less", "able", "ably", "er", "ers", "ist", "ists",
        ];
        // The doubled-l verbs only double before a vowel suffix, and the base
        // forms ("travel", "level", "model") are the same in both spellings.
        const DOUBLED: &[&str] = &["travel", "cancel", "label", "level", "model", "signal"];
        let mut us_to_ca = HashMap::new();
        let mut ca_to_us = HashMap::new();
        for (us, ca) in STEMS {
            let doubled = DOUBLED.contains(us);
            for suf in SUFFIXES {
                if doubled && !["ed", "ing", "er", "ers"].contains(suf) {
                    continue;
                }
                us_to_ca.insert(format!("{us}{suf}"), format!("{ca}{suf}"));
                ca_to_us.insert(format!("{ca}{suf}"), format!("{us}{suf}"));
            }
        }
        (us_to_ca, ca_to_us)
    });

fn match_case(original: &str, replacement: &str) -> String {
    if original.chars().count() > 1 && original.chars().all(|c| !c.is_lowercase()) {
        replacement.to_uppercase()
    } else if original.chars().next().is_some_and(|c| c.is_uppercase()) {
        let mut cs = replacement.chars();
        cs.next()
            .map(|f| f.to_uppercase().collect::<String>() + cs.as_str())
            .unwrap_or_default()
    } else {
        replacement.to_string()
    }
}

/// Use the project's spelling convention where the swap is unambiguous
/// (colour/color, centre/center, travelled/traveled). Capitalised words in the
/// middle of a sentence are left alone: they may be names.
pub fn normalise_spelling(text: &str, before: &str, locale: Locale) -> String {
    let map = match locale {
        Locale::EnCa => &SPELLING.0,
        Locale::EnUs => &SPELLING.1,
    };
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let mut prev_word_end = before
        .trim_end()
        .chars()
        .last()
        .is_none_or(|c| ".!?\"”’".contains(c));
    let flush = |word: &mut String, out: &mut String, prev_end: &mut bool| {
        if word.is_empty() {
            return;
        }
        let lower = word.to_lowercase();
        let capital = word.chars().next().is_some_and(|c| c.is_uppercase());
        let swap = map
            .get(&lower)
            .filter(|_| !capital || *prev_end || word.chars().skip(1).all(|c| !c.is_lowercase()));
        match swap {
            Some(r) => out.push_str(&match_case(word, r)),
            None => out.push_str(word),
        }
        *prev_end = word.ends_with(['.', '!', '?']);
        word.clear();
    };
    for c in text.chars() {
        if c.is_alphabetic() {
            word.push(c);
        } else {
            flush(&mut word, &mut out, &mut prev_word_end);
            if ".!?".contains(c) {
                prev_word_end = true;
            } else if !c.is_whitespace() && !"\"”’'”".contains(c) {
                prev_word_end = false;
            }
            out.push(c);
        }
    }
    flush(&mut word, &mut out, &mut prev_word_end);
    out
}

/// Curly quotes, em dashes and ellipses, following the author's settings and
/// the same rules as typing (`src/editor/typography.ts`).
pub fn smart_typography(text: &str, before: &str, t: &Typography) -> String {
    let mut s = text.to_string();
    if t.ellipsis {
        s = s.replace("...", "…");
    }
    if t.dashes {
        s = s.replace("--", "—");
    }
    if !t.quotes {
        return s;
    }
    let mut prev = before.chars().last();
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let opens = prev.is_none_or(|p| p.is_whitespace() || "([{—–“‘-".contains(p));
        let r = match c {
            '"' => {
                if opens {
                    '“'
                } else {
                    '”'
                }
            }
            '\'' => {
                if opens {
                    '‘'
                } else {
                    '’'
                }
            }
            other => other,
        };
        out.push(r);
        prev = Some(r);
    }
    out
}

/// Turn the model's raw output into what the author sees, or `None` when
/// there is nothing worth showing.
pub fn post_process(raw: &str, ctx: &PostCtx) -> Option<String> {
    let mut s: &str = raw;
    let mut truncated = ctx.truncated;
    // Chat-template tokens end the suggestion where they start.
    if let Some(i) = TEMPLATE_MARKERS.iter().filter_map(|m| s.find(m)).min() {
        s = &s[..i];
        truncated = false;
    }
    // One line only: a suggestion never contains a paragraph break.
    if let Some(i) = s.find('\n') {
        s = &s[..i];
        truncated = false;
    }
    let lower = s.trim_start().to_lowercase();
    if META_STARTS.iter().any(|m| lower.starts_with(m)) {
        return None;
    }

    let body = strip_repeat(ctx.before, s);
    let fixed = fix_leading_space(ctx.before, body);
    let spaced = fixed.starts_with(' ');
    let text = fixed.trim_start();
    if text.is_empty() {
        return None;
    }

    // Complete words only: a cut-off last word is dropped.
    let mut words: Vec<&str> = text.split_whitespace().collect();
    if truncated {
        if words.len() <= 1 {
            return None;
        }
        words.pop();
    }
    let take = match ctx.length {
        GhostLength::Word => 1,
        GhostLength::Phrase => {
            let window = words.len().min(10);
            match words[..window].iter().position(|w| is_sentence_end(w)) {
                Some(i) => i + 1,
                None if words.len() > 10 => words[..10]
                    .iter()
                    .rposition(|w| is_clause_end(w))
                    .filter(|i| *i >= 3)
                    .map_or(10, |i| i + 1),
                None => window,
            }
        }
        GhostLength::Sentence => {
            let window = words.len().min(60);
            words[..window]
                .iter()
                .position(|w| is_sentence_end(w))
                .map_or(window, |i| i + 1)
        }
    };
    let mut out = words[..take.min(words.len())].join(" ");
    out = normalise_spelling(&out, ctx.before, ctx.locale);
    // Quotes open or close by the character before, which is the author's own
    // last character, or the space this suggestion starts with.
    let prior = if spaced {
        format!("{} ", ctx.before.trim_end())
    } else {
        ctx.before.to_string()
    };
    out = smart_typography(&out, &prior, &ctx.typography);
    if !out.chars().any(|c| c.is_alphanumeric()) {
        return None;
    }
    Some(if spaced { format!(" {out}") } else { out })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> GhostContext {
        GhostContext {
            project_title: "The Salt Road".into(),
            locale: Locale::EnCa,
            doc_title: "The ferry".into(),
            pov: Some("Mara".into()),
            synopsis: Some("Mara misses the last ferry.".into()),
        }
    }

    const T_ON: Typography = Typography {
        quotes: true,
        dashes: true,
        ellipsis: true,
    };
    const T_OFF: Typography = Typography {
        quotes: false,
        dashes: false,
        ellipsis: false,
    };

    fn post(before: &str, raw: &str, length: GhostLength, truncated: bool) -> Option<String> {
        post_process(
            raw,
            &PostCtx {
                before,
                length,
                locale: Locale::EnCa,
                typography: T_ON,
                truncated,
            },
        )
    }

    // --- prompt -----------------------------------------------------------------

    #[test]
    fn the_prompt_follows_the_spec_order_and_uses_the_scene_notes() {
        let text = "One.\n\nTwo.\n\nThree.\n\nFour is half wri";
        let b = build_prompt(&ctx(), text, 8000);
        let labels: Vec<_> = b.prompt.sections.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "[1] Role instructions",
                "[2] Project primer",
                "[3] Scene notes",
                "[5] Scene text",
                "[6] Assistant prefill"
            ]
        );
        assert!(b.prompt.system.starts_with(ROLE));
        assert!(b.prompt.system.contains("Canadian English"));
        assert!(b.prompt.user.contains("Point of view: Mara"));
        assert!(
            b.prompt
                .user
                .contains("Synopsis: Mara misses the last ferry.")
        );
        // The last two paragraphs are the prefilled assistant turn; the rest is [5].
        assert_eq!(b.prompt.assistant_prefix, "Three.\n\nFour is half wri");
        assert!(b.prompt.user.contains("One.\n\nTwo."));
        assert!(!b.prompt.user.contains("Three."));
    }

    #[test]
    fn the_stable_prefix_survives_typing_within_a_paragraph() {
        let a = build_prompt(&ctx(), "One.\n\nTwo.\n\nThree is half", 8000);
        let b = build_prompt(&ctx(), "One.\n\nTwo.\n\nThree is half written", 8000);
        assert_eq!(a.prompt.system, b.prompt.system);
        assert_eq!(a.prompt.user, b.prompt.user);
        assert_ne!(a.prompt.assistant_prefix, b.prompt.assistant_prefix);
    }

    #[test]
    fn trailing_whitespace_is_not_prefilled() {
        let b = build_prompt(&ctx(), "She opened the ", 8000);
        assert_eq!(b.prompt.assistant_prefix, "She opened the");
        assert_eq!(b.before, "She opened the ");
    }

    #[test]
    fn a_scene_with_no_earlier_paragraphs_says_so() {
        let b = build_prompt(&ctx(), "Just starting.", 8000);
        assert!(
            b.prompt
                .user
                .contains("The scene begins in the reply below.")
        );
    }

    #[test]
    fn the_prompt_stays_inside_the_token_budget() {
        let para =
            "The harbour lay quiet under a low sky, and nobody on the quay spoke. ".repeat(14);
        let text = vec![para.trim().to_string(); 80].join("\n\n");
        for budget in [8000, 4000, 2000] {
            let b = build_prompt(&ctx(), &text, budget);
            let total: u32 = b.prompt.sections.iter().map(|s| s.tokens).sum();
            assert!(
                total + RESERVE_TOKENS <= budget.max(1400),
                "{total} vs {budget}"
            );
            assert!(b.prompt.user.contains("earlier parts omitted"));
        }
        // The most recent text is what survives.
        let b = build_prompt(&ctx(), &format!("{text}\n\nThe very last paragraph"), 2000);
        assert!(
            b.prompt
                .assistant_prefix
                .ends_with("The very last paragraph")
        );
    }

    #[test]
    fn a_huge_current_paragraph_is_cut_from_the_front_at_a_clean_start() {
        let long = "It rained. ".repeat(3000);
        let b = build_prompt(&ctx(), &format!("{long}and then it stopped"), 3000);
        let p = &b.prompt.assistant_prefix;
        assert!(estimate_tokens(p) <= 1100, "{}", estimate_tokens(p));
        assert!(p.ends_with("and then it stopped"));
        assert!(p.starts_with("It rained.") || p.starts_with("rained."));
    }

    #[test]
    fn sampling_follows_the_length_and_warms_up_for_alternatives() {
        let base = super::super::types::default_sampling(super::super::types::Role::Ghost);
        assert_eq!(
            sampling_for(base.clone(), GhostLength::Word, 0).max_tokens,
            8
        );
        assert_eq!(
            sampling_for(base.clone(), GhostLength::Phrase, 0).max_tokens,
            20
        );
        assert_eq!(
            sampling_for(base.clone(), GhostLength::Sentence, 0).max_tokens,
            56
        );
        let first = sampling_for(base.clone(), GhostLength::Phrase, 0);
        let second = sampling_for(base.clone(), GhostLength::Phrase, 1);
        let third = sampling_for(base, GhostLength::Phrase, 2);
        assert!(second.temperature > first.temperature + 0.2);
        assert!(third.temperature > second.temperature);
        assert!(third.temperature <= 1.3);
        // Only alternatives loosen the cut-off.
        assert!(first.min_p > second.min_p);
        assert_eq!(second.min_p, third.min_p);
    }

    // --- post-processing -----------------------------------------------------------

    #[test]
    fn spacing_follows_the_authors_last_character() {
        // After a space: no leading space. After a word: keep the model's space.
        assert_eq!(
            post("She opened the ", " door slowly", GhostLength::Word, false).as_deref(),
            Some("door")
        );
        assert_eq!(
            post("She opened the", " door slowly", GhostLength::Word, false).as_deref(),
            Some(" door")
        );
        // After a full stop a new sentence needs a space even if the model forgot it.
        assert_eq!(
            post("It was over.", "Nobody spoke", GhostLength::Word, false).as_deref(),
            Some(" Nobody")
        );
        // After an opening quote there is none.
        assert_eq!(
            post("She said, “", " We should go", GhostLength::Word, false).as_deref(),
            Some("We")
        );
        // Mid-word: the model finishes the word.
        assert_eq!(
            post(
                "She walked toward the harb",
                "our and stopped",
                GhostLength::Word,
                false
            )
            .as_deref(),
            Some("our")
        );
    }

    #[test]
    fn repeats_of_the_authors_last_words_are_removed() {
        assert_eq!(
            post(
                "She opened the door",
                " the door and stepped out",
                GhostLength::Phrase,
                false
            )
            .as_deref(),
            Some(" and stepped out")
        );
        // A single repeated short word is left alone ("had had").
        assert_eq!(
            post("She had", " had enough", GhostLength::Phrase, false).as_deref(),
            Some(" had enough")
        );
        // A single repeated long word is removed.
        assert_eq!(
            post(
                "She walked to the harbour",
                " harbour again",
                GhostLength::Phrase,
                false
            )
            .as_deref(),
            Some(" again")
        );
        // Everything repeated: nothing left.
        assert_eq!(
            post(
                "She opened the door",
                " the door",
                GhostLength::Phrase,
                false
            ),
            None
        );
    }

    #[test]
    fn meta_text_and_template_tokens_are_rejected() {
        for raw in [
            "Sure, here is the continuation",
            "Here's a continuation: she left.",
            "Certainly! She left.",
            "As an AI, I cannot",
            "I'm sorry, but",
        ] {
            assert_eq!(
                post("She stood.", raw, GhostLength::Sentence, false),
                None,
                "{raw}"
            );
        }
        assert_eq!(
            post("She stood.", "<|im_end|>", GhostLength::Sentence, false),
            None
        );
        // Text before a template token is kept; the token and anything after is not.
        assert_eq!(
            post(
                "She stood",
                " up<|im_end|> and more",
                GhostLength::Sentence,
                false
            )
            .as_deref(),
            Some(" up")
        );
        // "Surely" is a word, not meta-text.
        assert!(
            post(
                "He nodded.",
                "Surely she knew.",
                GhostLength::Sentence,
                false
            )
            .is_some()
        );
    }

    #[test]
    fn nothing_worth_showing_is_discarded() {
        assert_eq!(post("A b c", "", GhostLength::Phrase, false), None);
        assert_eq!(post("A b c", "   ", GhostLength::Phrase, false), None);
        assert_eq!(post("A b c", " —", GhostLength::Phrase, false), None);
    }

    #[test]
    fn a_suggestion_is_one_line() {
        assert_eq!(
            post(
                "She stood",
                " up and left.\n\nThe next day",
                GhostLength::Sentence,
                false
            )
            .as_deref(),
            Some(" up and left.")
        );
    }

    #[test]
    fn length_trims_to_a_clean_boundary() {
        let raw = " slowly toward the water, where the ferry had been, and the wind rose again.";
        assert_eq!(
            post("She walked", raw, GhostLength::Word, false).as_deref(),
            Some(" slowly")
        );
        // Phrase: at most ten words, preferring a clause boundary.
        assert_eq!(
            post("She walked", raw, GhostLength::Phrase, false).as_deref(),
            Some(" slowly toward the water, where the ferry had been,")
        );
        // Sentence: through the end of the sentence.
        assert_eq!(
            post("She walked", raw, GhostLength::Sentence, false).as_deref(),
            Some(" slowly toward the water, where the ferry had been, and the wind rose again.")
        );
        // A phrase stops at a sentence end inside the window.
        assert_eq!(
            post(
                "She walked",
                " home. Then she slept",
                GhostLength::Phrase,
                false
            )
            .as_deref(),
            Some(" home.")
        );
    }

    #[test]
    fn abbreviations_and_initials_do_not_end_a_sentence() {
        assert!(!is_sentence_end("Mr."));
        assert!(!is_sentence_end("J."));
        assert!(!is_sentence_end("etc."));
        assert!(is_sentence_end("home."));
        assert!(is_sentence_end("home.”"));
        assert!(is_sentence_end("what?"));
        assert!(!is_sentence_end("well…"));
        assert_eq!(
            post(
                "She met",
                " Mr. Hale at the quay. Then",
                GhostLength::Sentence,
                false
            )
            .as_deref(),
            Some(" Mr. Hale at the quay.")
        );
    }

    #[test]
    fn a_cut_off_last_word_is_dropped() {
        assert_eq!(
            post(
                "She walked",
                " slowly toward the wat",
                GhostLength::Phrase,
                true
            )
            .as_deref(),
            Some(" slowly toward the")
        );
        // A lone partial word is not worth showing yet.
        assert_eq!(post("She walked", " slo", GhostLength::Phrase, true), None);
        assert_eq!(post("She walked", " slo", GhostLength::Word, true), None);
        // A complete lone word that the model stopped on is shown.
        assert_eq!(
            post("She walked", " slowly", GhostLength::Word, false).as_deref(),
            Some(" slowly")
        );
    }

    #[test]
    fn spelling_follows_the_project_convention() {
        let ca = |s| normalise_spelling(s, "The ", Locale::EnCa);
        let us = |s| normalise_spelling(s, "The ", Locale::EnUs);
        assert_eq!(ca("the color of the harbor"), "the colour of the harbour");
        assert_eq!(
            normalise_spelling("Colors, colored and colorful", "It ended. ", Locale::EnCa),
            "Colours, coloured and colourful"
        );
        assert_eq!(ca("a center for the theater"), "a centre for the theatre");
        assert_eq!(
            ca("she traveled and canceled it"),
            "she travelled and cancelled it"
        );
        assert_eq!(ca("a model and a level"), "a model and a level");
        assert_eq!(ca("COLOR"), "COLOUR");
        assert_eq!(us("the colour of the harbour"), "the color of the harbor");
        assert_eq!(us("she travelled"), "she traveled");
        // Already right: untouched.
        assert_eq!(ca("colour and centre"), "colour and centre");
        // A capitalised word mid-sentence may be a name.
        assert_eq!(
            normalise_spelling("met Honor at Center Street", "She ", Locale::EnCa),
            "met Honor at Center Street"
        );
        // At the start of a sentence it is a word.
        assert_eq!(
            normalise_spelling("Color faded.", "It ended. ", Locale::EnCa),
            "Colour faded."
        );
    }

    #[test]
    fn the_whole_pipeline_applies_spelling_and_typography() {
        assert_eq!(
            post(
                "The light changed",
                " the color of the harbor -- \"gold\"...",
                GhostLength::Sentence,
                false
            )
            .as_deref(),
            Some(" the colour of the harbour — “gold”…")
        );
    }

    #[test]
    fn typography_follows_the_settings_and_the_authors_last_character() {
        assert_eq!(
            smart_typography("\"Go,\" he said", "", &T_ON),
            "“Go,” he said"
        );
        assert_eq!(smart_typography("it's", "", &T_ON), "it’s");
        assert_eq!(smart_typography("go\"", "He said, \"", &T_ON), "go”");
        assert_eq!(smart_typography("a -- b...", "", &T_ON), "a — b…");
        assert_eq!(
            smart_typography("\"a\" -- b...", "", &T_OFF),
            "\"a\" -- b..."
        );
        let only_dashes = Typography {
            dashes: true,
            ..T_OFF
        };
        assert_eq!(
            smart_typography("\"a\" -- b", "", &only_dashes),
            "\"a\" — b"
        );
    }
}
