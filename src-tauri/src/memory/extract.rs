//! Scene extraction (SPEC §9.4): the prompt, the JSON schema the server turns
//! into a grammar, and a forgiving reader for what comes back.
//!
//! A scene is read in one call. A scene too long for the 16K slot is read in
//! windows of whole paragraphs (numbered across the scene, so a fact's
//! paragraph number always points at the right paragraph), and the results are
//! merged.

use serde_json::{Value, json};

use super::paragraphs::Para;
use crate::ai::ghost::estimate_tokens;
use crate::ai::types::{Prompt, PromptSection};
use crate::project::manifest::ProjectType;

/// Tokens of scene text per call (the slot is 16K; the answer needs ~1.8K).
pub const WINDOW_TOKENS: u32 = 4500;
/// Known-entry names listed in the prompt.
const KNOWN_MAX_TOKENS: u32 = 700;

pub const ROLE: &str = "You are a careful reader building a private story bible for the author. You read a scene and report what it states, as JSON. You never invent details, and you copy names exactly as written.";

pub fn entity_types(pt: ProjectType) -> &'static [&'static str] {
    match pt {
        ProjectType::Fiction => &["character", "location", "item", "group", "term"],
        ProjectType::Both => &[
            "character",
            "location",
            "item",
            "group",
            "concept",
            "claim",
            "source",
            "person",
            "term",
        ],
        ProjectType::NonFiction => &["concept", "claim", "source", "person", "term"],
    }
}

/// Relation kinds the model may use. A closed list keeps the graph readable and
/// stops invented kinds ("employer" for someone who merely called on a neighbour).
pub const RELATION_KINDS: &[&str] = &[
    "sibling",
    "parent",
    "spouse",
    "relative",
    "friend",
    "ally",
    "rival",
    "enemy",
    "romantic",
    "mentor",
    "acquaintance",
    "neighbour",
    "serves",
    "member-of",
    "located-in",
    "owns",
];

/// An optional per-scene measurement the reading also reports (SPEC §5.3
/// `memoryExtractors`). Its value lands in the scene card's `metrics` under
/// `metric_key`, ready for charts. Adding one is one entry here: the schema,
/// the prompt, the parser and the dirty check all follow the list.
pub struct Extractor {
    pub id: &'static str,
    pub metric_key: &'static str,
    /// Key inside `g_metrics` (letter-prefixed, see `schema`).
    pub schema_key: &'static str,
    pub max: u8,
    /// One line for the extraction rules.
    pub rule: &'static str,
}

/// How tense the scene is for the reader, 0–10 (SPEC §10 Structure overview).
pub const TENSION: Extractor = Extractor {
    id: "tension",
    metric_key: "tension",
    schema_key: "a_tension",
    max: 10,
    rule: "a_tension: a whole number 0-10 for how tense this part is for the reader (0 calm, 3 mild unease, 5 real conflict or worry, 8 danger or a crisis, 10 life or death).",
};

pub const EXTRACTORS: &[&Extractor] = &[&TENSION];

/// The extractors the author has switched on.
pub fn enabled(tension: bool) -> Vec<&'static Extractor> {
    EXTRACTORS
        .iter()
        .copied()
        .filter(|x| x.id != "tension" || tension)
        .collect()
}

/// The JSON schema for one extraction call.
///
/// llama.cpp turns it into a grammar that writes object keys in **alphabetical**
/// order, whatever order the schema lists them in. The keys carry a letter prefix
/// (`a_`, `b_`, …) so the order they come out in is the order we want: the
/// summary and the cast first, and the least important parts last, in case a long
/// answer is cut off at the token limit.
pub fn schema(pt: ProjectType, extractors: &[&Extractor]) -> String {
    let types = entity_types(pt);
    let mut s = json!({
        "type": "object",
        "properties": {
            "a_scene": {"type": "object", "properties": {
                "a_summary": {"type": "string", "minLength": 20},
                "b_pov": {"type": "string"},
                "c_tense": {"type": "string"},
                "d_location": {"type": "string"},
                "e_when": {"type": "string"}
            }, "required": ["a_summary", "b_pov", "c_tense", "d_location", "e_when"]},
            "b_entities": {"type": "array", "maxItems": 14, "items": {
                "type": "object",
                "properties": {
                    "a_name": {"type": "string", "minLength": 2},
                    "b_type": {"type": "string", "enum": types},
                    "c_aliases": {"type": "array", "maxItems": 3, "items": {"type": "string"}}
                },
                "required": ["a_name", "b_type", "c_aliases"]
            }},
            "c_facts": {"type": "array", "maxItems": 14, "items": {
                "type": "object",
                "properties": {
                    "a_entity": {"type": "string"},
                    "b_fact": {"type": "string", "minLength": 12},
                    "c_field": {"type": "string"},
                    "d_para": {"type": "integer"},
                    "e_quote": {"type": "string"}
                },
                "required": ["a_entity", "b_fact", "c_field", "d_para", "e_quote"]
            }},
            "d_relations": {"type": "array", "maxItems": 6, "items": {
                "type": "object",
                "properties": {
                    "a_from": {"type": "string"},
                    "b_kind": {"type": "string", "enum": RELATION_KINDS},
                    "c_to": {"type": "string"},
                    "d_para": {"type": "integer"},
                    "e_quote": {"type": "string"}
                },
                "required": ["a_from", "b_kind", "c_to", "d_para", "e_quote"]
            }},
            "e_events": {"type": "array", "maxItems": 4, "items": {
                "type": "object",
                "properties": {
                    "a_title": {"type": "string", "minLength": 8},
                    "b_when": {"type": "string"},
                    "c_entities": {"type": "array", "maxItems": 4, "items": {"type": "string"}},
                    "d_para": {"type": "integer"}
                },
                "required": ["a_title", "b_when", "c_entities", "d_para"]
            }},
            "f_threads": {"type": "array", "maxItems": 3, "items": {
                "type": "object",
                "properties": {
                    "a_title": {"type": "string", "minLength": 8},
                    "b_status": {"type": "string", "enum": ["open", "resolved", "dropped"]}
                },
                "required": ["a_title", "b_status"]
            }}
        },
        "required": ["a_scene", "b_entities", "c_facts", "d_relations", "e_events", "f_threads"]
    });
    if !extractors.is_empty() {
        let props: serde_json::Map<String, Value> = extractors
            .iter()
            .map(|x| {
                (
                    x.schema_key.to_string(),
                    json!({"type": "integer", "minimum": 0, "maximum": x.max}),
                )
            })
            .collect();
        let required: Vec<&str> = extractors.iter().map(|x| x.schema_key).collect();
        s["properties"]["g_metrics"] =
            json!({"type": "object", "properties": props, "required": required});
        s["required"] = json!([
            "a_scene",
            "b_entities",
            "c_facts",
            "d_relations",
            "e_events",
            "f_threads",
            "g_metrics"
        ]);
    }
    s.to_string()
}

/// What the prompt builder knows about the scene.
pub struct SceneInfo<'a> {
    /// Extra measurements to report (`tension`).
    pub extractors: &'a [&'static Extractor],
    pub project_title: &'a str,
    pub project_type: ProjectType,
    pub title: &'a str,
    pub pov: Option<&'a str>,
    /// `(name, type)` of entries already in the bible, most relevant first.
    pub known: &'a [(String, String)],
}

fn known_line(known: &[(String, String)]) -> String {
    let mut out = String::new();
    let mut used = 0;
    for (name, kind) in known {
        let item = format!("{name} ({kind}); ");
        used += estimate_tokens(&item);
        if used > KNOWN_MAX_TOKENS {
            break;
        }
        out.push_str(&item);
    }
    out.trim_end_matches("; ").to_string()
}

/// Split paragraphs into windows of about [`WINDOW_TOKENS`], keeping order. Each
/// window is a range of indexes into `paras`.
pub fn windows(paras: &[Para]) -> Vec<std::ops::Range<usize>> {
    let mut out = vec![];
    let mut start = 0;
    let mut used = 0;
    for (i, p) in paras.iter().enumerate() {
        let t = estimate_tokens(&p.text) + 2;
        if used + t > WINDOW_TOKENS && i > start {
            out.push(start..i);
            start = i;
            used = 0;
        }
        used += t;
    }
    if start < paras.len() {
        out.push(start..paras.len());
    }
    out
}

/// The extraction prompt for one window. Paragraph numbers are 1-based over the
/// whole scene.
pub fn build_prompt(info: &SceneInfo, paras: &[Para], window: std::ops::Range<usize>) -> Prompt {
    let kind = match info.project_type {
        ProjectType::Fiction => "novel",
        ProjectType::NonFiction => "non-fiction book",
        ProjectType::Both => "book",
    };
    let types = entity_types(info.project_type).join(", ");
    let primer = format!("Project: {} (a {kind}).", info.project_title.trim());
    let mut notes = format!("Scene: {}", info.title.trim());
    if let Some(p) = info.pov.filter(|p| !p.trim().is_empty()) {
        notes.push_str(&format!("\nPoint of view: {}", p.trim()));
    }
    let known = known_line(info.known);
    let known_section = if known.is_empty() {
        "No entries yet.".to_string()
    } else {
        format!(
            "Entries already in the bible (use these exact names when the text refers to them): {known}"
        )
    };
    let text: String = paras[window.clone()]
        .iter()
        .enumerate()
        .map(|(k, p)| format!("[{}] {}", window.start + k + 1, p.text))
        .collect::<Vec<_>>()
        .join("\n\n");
    let part = if window.len() < paras.len() {
        format!(
            "\n(This is part of a longer scene: paragraphs {}–{} of {}.)",
            window.start + 1,
            window.end,
            paras.len()
        )
    } else {
        String::new()
    };
    let kinds = RELATION_KINDS.join(", ");
    let metric_rules = if info.extractors.is_empty() {
        String::new()
    } else {
        format!(
            "- g_metrics: {}\n",
            info.extractors
                .iter()
                .map(|x| x.rule)
                .collect::<Vec<_>>()
                .join(" ")
        )
    };
    let metric_example = if info.extractors.is_empty() {
        String::new()
    } else {
        format!(
            ",\"g_metrics\":{{{}}}",
            info.extractors
                .iter()
                .map(|x| format!("\"{}\":2", x.schema_key))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let rules = format!(
        "Report what this text states, as JSON with these keys, in this order:\n\
- a_scene: a_summary (one to three sentences on what happens), b_pov, c_tense, d_location, e_when (when the scene is set, such as \"that night\"). Use \"\" for what is unclear.\n\
- b_entities: each named {types} in the text. a_name is the name as written, b_type, c_aliases are other names the text uses for the same one. Never list a pronoun or a phrase like \"his wife\" as an entity.\n\
- c_facts: things the text states about an entity: looks, age, job, history, abilities, feelings, possessions. b_fact is ONE COMPLETE SENTENCE that names the entity (\"Tom Hale is a ferryman.\", never just \"ferryman\"). c_field is the attribute in one or two words (\"occupation\") or \"\". d_para is the paragraph number. e_quote is up to 12 words copied exactly from that paragraph.\n\
- d_relations: how two named entities relate, with b_kind from: {kinds}. \"parent\" means a_from is the parent of c_to. Add d_para and e_quote.\n\
- e_events: things that happen and matter to the plot, with d_para.\n\
- f_threads: questions, promises or mysteries the text sets up or settles. b_status is open, resolved or dropped.\n\
{metric_rules}\
Report only what the text says. Do not guess. Empty lists are fine.\n\n\
Example. Text: [1] Tom Hale, the ferryman, rubbed his grey beard. His sister Ann laughed. [2] \"Not today,\" said Ann Hale.\n\
Answer: {{\"a_scene\":{{\"a_summary\":\"Tom and his sister Ann talk at the ferry.\",\"b_pov\":\"\",\"c_tense\":\"past\",\"d_location\":\"the ferry\",\"e_when\":\"\"}},\"b_entities\":[{{\"a_name\":\"Tom Hale\",\"b_type\":\"character\",\"c_aliases\":[]}},{{\"a_name\":\"Ann Hale\",\"b_type\":\"character\",\"c_aliases\":[\"Ann\"]}}],\"c_facts\":[{{\"a_entity\":\"Tom Hale\",\"b_fact\":\"Tom Hale is a ferryman with a grey beard.\",\"c_field\":\"occupation\",\"d_para\":1,\"e_quote\":\"Tom Hale, the ferryman\"}}],\"d_relations\":[{{\"a_from\":\"Ann Hale\",\"b_kind\":\"sibling\",\"c_to\":\"Tom Hale\",\"d_para\":1,\"e_quote\":\"His sister Ann\"}}],\"e_events\":[],\"f_threads\":[]{metric_example}}}"
    );
    let user = format!("{notes}{part}\n\n{known_section}\n\nParagraphs:\n\n{text}\n\n{rules}");
    let section = |label: &str, t: &str| PromptSection {
        label: label.into(),
        text: t.into(),
        tokens: estimate_tokens(t),
    };
    Prompt {
        system: format!("{ROLE}\n\n{primer}"),
        user: user.clone(),
        assistant_prefix: String::new(),
        sections: vec![
            section("[1] Role instructions", ROLE),
            section("[2] Project primer", &primer),
            section("[3] Scene notes", &notes),
            section("[4] Known entries", &known_section),
            section("[5] Scene text", &text),
            section("[6] Extraction rules", &rules),
        ],
    }
}

// --- Reading the answer ------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct XEntity {
    pub name: String,
    pub kind: String,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct XFact {
    pub entity: String,
    pub field: String,
    pub text: String,
    pub para: usize,
    pub quote: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct XRelation {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub para: usize,
    pub quote: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct XEvent {
    pub title: String,
    pub story_time: String,
    pub entities: Vec<String>,
    pub para: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct XThread {
    pub title: String,
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Extraction {
    pub summary: String,
    pub pov: String,
    pub tense: String,
    pub location: String,
    pub story_time: String,
    pub entities: Vec<XEntity>,
    pub facts: Vec<XFact>,
    pub relations: Vec<XRelation>,
    pub events: Vec<XEvent>,
    pub threads: Vec<XThread>,
    /// Extractor values by metric key (`tension`), from 0 to the extractor's max.
    pub metrics: std::collections::BTreeMap<String, f64>,
}

/// Parse JSON, repairing an answer that was cut off at the token limit by
/// dropping the unfinished tail and closing what is open.
pub fn parse_json_lenient(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    let text = &text[start..];
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        return Some(v);
    }
    if let Some(end) = text.rfind('}')
        && let Ok(v) = serde_json::from_str::<Value>(&text[..=end])
    {
        return Some(v);
    }
    // Walk the text, remembering the last point where a complete value ended
    // inside an object or array, and the brackets open at that point.
    let mut stack: Vec<char> = vec![];
    let mut in_str = false;
    let mut esc = false;
    let mut best: Option<(usize, Vec<char>)> = None;
    for (i, c) in text.char_indices() {
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                stack.pop();
                best = Some((i + c.len_utf8(), stack.clone()));
            }
            _ => {}
        }
    }
    let (end, open) = best?;
    let mut fixed = text[..end].trim_end_matches([',', ' ', '\n']).to_string();
    for c in open.iter().rev() {
        fixed.push(*c);
    }
    serde_json::from_str(&fixed).ok()
}

/// The value under the first of `keys` that is present: the current schema names
/// (`a_summary`) and the plain ones (`summary`) both read.
fn get<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| v.get(*k))
}

fn s(v: &Value, keys: &[&str]) -> String {
    get(v, keys)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn n(v: &Value, keys: &[&str]) -> usize {
    match get(v, keys) {
        Some(Value::Number(x)) => x.as_u64().unwrap_or(0) as usize,
        Some(Value::String(t)) => t.trim().trim_matches(['[', ']']).parse().unwrap_or(0),
        _ => 0,
    }
}

fn list(v: &Value, keys: &[&str]) -> Vec<Value> {
    get(v, keys)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn strings(v: &Value, keys: &[&str]) -> Vec<String> {
    list(v, keys)
        .iter()
        .filter_map(|x| x.as_str().map(|t| t.trim().to_string()))
        .filter(|t| !t.is_empty())
        .collect()
}

/// Read one call's output. `None` if it holds no usable JSON.
pub fn parse_output(text: &str) -> Option<Extraction> {
    let v = parse_json_lenient(text)?;
    if !v.is_object() {
        return None;
    }
    // The scene's own fields sit in `a_scene`, or at the top level in plain form.
    let scene = get(&v, &["a_scene", "scene"])
        .filter(|x| x.is_object())
        .unwrap_or(&v);
    Some(Extraction {
        summary: s(scene, &["a_summary", "summary"]),
        pov: s(scene, &["b_pov", "pov"]),
        tense: s(scene, &["c_tense", "tense"]),
        location: s(scene, &["d_location", "location"]),
        story_time: s(scene, &["e_when", "storyTime"]),
        entities: list(&v, &["b_entities", "entities"])
            .iter()
            .map(|e| XEntity {
                name: s(e, &["a_name", "name"]),
                kind: s(e, &["b_type", "type"]).to_lowercase(),
                aliases: strings(e, &["c_aliases", "aliases"]),
            })
            .filter(|e| !e.name.is_empty())
            .collect(),
        facts: list(&v, &["c_facts", "facts"])
            .iter()
            .map(|f| XFact {
                entity: s(f, &["a_entity", "entity"]),
                field: s(f, &["c_field", "field"]),
                text: s(f, &["b_fact", "text"]),
                para: n(f, &["d_para", "para"]),
                quote: s(f, &["e_quote", "quote"]),
            })
            .filter(|f| !f.entity.is_empty() && !f.text.is_empty())
            .collect(),
        relations: list(&v, &["d_relations", "relations"])
            .iter()
            .map(|r| XRelation {
                from: s(r, &["a_from", "from"]),
                to: s(r, &["c_to", "to"]),
                kind: s(r, &["b_kind", "type"]),
                para: n(r, &["d_para", "para"]),
                quote: s(r, &["e_quote", "quote"]),
            })
            .filter(|r| !r.from.is_empty() && !r.to.is_empty() && !r.kind.is_empty())
            .collect(),
        events: list(&v, &["e_events", "events"])
            .iter()
            .map(|e| XEvent {
                title: s(e, &["a_title", "title"]),
                story_time: s(e, &["b_when", "storyTime"]),
                entities: strings(e, &["c_entities", "entities"]),
                para: n(e, &["d_para", "para"]),
            })
            .filter(|e| !e.title.is_empty())
            .collect(),
        threads: list(&v, &["f_threads", "threads"])
            .iter()
            .map(|t| XThread {
                title: s(t, &["a_title", "title"]),
                status: s(t, &["b_status", "status"]).to_lowercase(),
            })
            .filter(|t| !t.title.is_empty())
            .collect(),
        metrics: EXTRACTORS
            .iter()
            .filter_map(|x| {
                let m = get(&v, &["g_metrics", "metrics"])?;
                let raw = get(m, &[x.schema_key, x.metric_key])?;
                let val = raw
                    .as_f64()
                    .or_else(|| raw.as_str().and_then(|s| s.trim().parse::<f64>().ok()))?;
                Some((x.metric_key.to_string(), val.clamp(0.0, f64::from(x.max))))
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paras(texts: &[&str]) -> Vec<Para> {
        texts
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

    #[test]
    fn the_prompt_numbers_paragraphs_and_lists_known_entries() {
        let p = paras(&["Mara came ashore.", "Jon waited."]);
        let known = vec![("Mara Voss".to_string(), "character".to_string())];
        let info = SceneInfo {
            extractors: &[],
            project_title: "The Salt Road",
            project_type: ProjectType::Fiction,
            title: "Arrival",
            pov: Some("Mara"),
            known: &known,
        };
        let prompt = build_prompt(&info, &p, 0..2);
        assert!(prompt.user.contains("[1] Mara came ashore."));
        assert!(prompt.user.contains("[2] Jon waited."));
        assert!(prompt.user.contains("Mara Voss (character)"));
        assert!(prompt.user.contains("Point of view: Mara"));
        assert_eq!(prompt.sections.len(), 6);
        assert!(prompt.sections[4].text.starts_with("[1] Mara"));
    }

    #[test]
    fn long_scenes_are_split_into_windows_with_global_numbers() {
        let long = "word ".repeat(2500); // ~3.5K tokens each
        let p = paras(&[&long, &long, &long]);
        let w = windows(&p);
        assert_eq!(w, vec![0..1, 1..2, 2..3]);
        let info = SceneInfo {
            extractors: &[],
            project_title: "T",
            project_type: ProjectType::Fiction,
            title: "S",
            pov: None,
            known: &[],
        };
        let prompt = build_prompt(&info, &p, 1..2);
        assert!(prompt.user.contains("[2] word"));
        assert!(!prompt.user.contains("[1] word"));
        assert!(prompt.user.contains("paragraphs 2–2 of 3"));
        assert_eq!(windows(&paras(&["short", "also short"])), vec![0..2]);
        assert!(windows(&[]).is_empty());
    }

    #[test]
    fn the_schema_is_valid_json_with_the_projects_entity_types() {
        let v: Value = serde_json::from_str(&schema(ProjectType::Fiction, &[])).unwrap();
        let types = &v["properties"]["b_entities"]["items"]["properties"]["b_type"]["enum"];
        assert!(types.as_array().unwrap().iter().any(|t| t == "character"));
        let v: Value = serde_json::from_str(&schema(ProjectType::NonFiction, &[])).unwrap();
        let types = &v["properties"]["b_entities"]["items"]["properties"]["b_type"]["enum"];
        assert!(types.as_array().unwrap().iter().any(|t| t == "concept"));
    }

    #[test]
    fn schema_keys_sort_into_the_order_the_model_should_write_them() {
        // llama.cpp writes keys alphabetically: the prefixes must make that the order we want.
        fn check(v: &Value, path: &str) {
            if let Some(props) = v.get("properties").and_then(Value::as_object) {
                let keys: Vec<&String> = props.keys().collect();
                for (i, k) in keys.iter().enumerate() {
                    let letter = (b'a' + i as u8) as char;
                    assert!(
                        k.starts_with(&format!("{letter}_")),
                        "{path}: {k} should start with {letter}_"
                    );
                    check(&props[*k], &format!("{path}.{k}"));
                }
            }
            if let Some(items) = v.get("items") {
                check(items, &format!("{path}[]"));
            }
        }
        let v: Value = serde_json::from_str(&schema(ProjectType::Fiction, &[])).unwrap();
        check(&v, "$");
        // The summary comes first, so a long answer cut off at the limit still has it.
        let scene = &v["properties"]["a_scene"]["properties"];
        assert!(scene.get("a_summary").is_some());
    }

    #[test]
    fn the_prompt_teaches_the_format_with_an_example() {
        let p = paras(&["Mara came ashore."]);
        let info = SceneInfo {
            extractors: &[],
            project_title: "T",
            project_type: ProjectType::Fiction,
            title: "S",
            pov: None,
            known: &[],
        };
        let prompt = build_prompt(&info, &p, 0..1);
        assert!(prompt.user.contains("Example. Text:"));
        assert!(prompt.user.contains("a_scene"));
        assert!(prompt.user.contains("sibling, parent, spouse"));
        // The example itself must parse with the reader.
        let ex = prompt.user.split("Answer: ").nth(1).unwrap().trim();
        let x = parse_output(ex).expect("the worked example is valid");
        assert_eq!(x.entities.len(), 2);
        assert_eq!(x.facts[0].para, 1);
        assert_eq!(x.relations[0].kind, "sibling");
    }

    #[test]
    fn the_current_key_names_are_read() {
        let out = r#"{"a_scene":{"a_summary":"Mara lands.","b_pov":"Mara","c_tense":"past","d_location":"Kessel","e_when":"dusk"},
          "b_entities":[{"a_name":"Mara","b_type":"character","c_aliases":["the pilot"]}],
          "c_facts":[{"a_entity":"Mara","b_fact":"Mara has grey eyes.","c_field":"eye colour","d_para":2,"e_quote":"grey eyes"}],
          "d_relations":[{"a_from":"Mara","b_kind":"sibling","c_to":"Jon","d_para":1,"e_quote":"her brother"}],
          "e_events":[{"a_title":"Mara lands","b_when":"dusk","c_entities":["Mara"],"d_para":1}],
          "f_threads":[{"a_title":"The closed harbour","b_status":"open"}]}"#;
        let x = parse_output(out).unwrap();
        assert_eq!(
            (
                x.summary.as_str(),
                x.pov.as_str(),
                x.location.as_str(),
                x.story_time.as_str()
            ),
            ("Mara lands.", "Mara", "Kessel", "dusk")
        );
        assert_eq!(x.entities[0].aliases, ["the pilot"]);
        assert_eq!(x.facts[0].text, "Mara has grey eyes.");
        assert_eq!(x.relations[0].to, "Jon");
        assert_eq!(x.events[0].story_time, "dusk");
        assert_eq!(x.threads[0].status, "open");
    }

    #[test]
    fn a_complete_answer_is_read() {
        let out = r#"{"summary":"Mara lands.","pov":"Mara","tense":"past","location":"Kessel","storyTime":"dusk",
          "entities":[{"name":"Mara","type":"Character","aliases":["the pilot"]},{"name":"","type":"character","aliases":[]}],
          "facts":[{"entity":"Mara","field":"eye colour","text":"Mara has grey eyes.","para":"[2]","quote":"grey eyes"}],
          "relations":[{"from":"Mara","to":"Jon","type":"sibling","para":1,"quote":"her brother"}],
          "events":[{"title":"Mara lands","storyTime":"dusk","entities":["Mara"],"para":1}],
          "threads":[{"title":"The closed harbour","status":"OPEN"}]}"#;
        let x = parse_output(out).unwrap();
        assert_eq!(x.summary, "Mara lands.");
        assert_eq!(x.entities.len(), 1, "a nameless entity is dropped");
        assert_eq!(x.entities[0].kind, "character");
        assert_eq!(
            x.facts[0].para, 2,
            "a paragraph given as text is understood"
        );
        assert_eq!(x.relations[0].kind, "sibling");
        assert_eq!(x.threads[0].status, "open");
    }

    #[test]
    fn an_answer_cut_off_at_the_token_limit_is_repaired() {
        let out = r#"{"summary":"Mara lands.","pov":"","tense":"","location":"","storyTime":"",
          "entities":[{"name":"Mara","type":"character","aliases":[]},{"name":"Jo"#;
        let x = parse_output(out).unwrap();
        assert_eq!(x.summary, "Mara lands.");
        assert_eq!(x.entities.len(), 1);
        assert_eq!(x.entities[0].name, "Mara");
    }

    #[test]
    fn garbage_is_no_extraction() {
        assert!(parse_output("I could not read that.").is_none());
        assert!(parse_output("[1,2,3]").is_none());
        assert!(parse_output("").is_none());
        // Text around the JSON is tolerated.
        assert!(parse_output("Sure! {\"summary\":\"x\"} done").is_some());
    }
}

#[cfg(test)]
mod extractor_tests {
    use super::*;

    #[test]
    fn tension_adds_a_required_last_key_to_the_schema_and_the_prompt() {
        let on = enabled(true);
        assert_eq!(on.len(), 1);
        let v: Value = serde_json::from_str(&schema(ProjectType::Fiction, &on)).unwrap();
        let m = &v["properties"]["g_metrics"];
        assert_eq!(m["properties"]["a_tension"]["maximum"], 10);
        assert!(
            v["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|k| *k == "g_metrics")
        );
        // Off: the schema is the plain one.
        assert!(enabled(false).is_empty());
        let v: Value = serde_json::from_str(&schema(ProjectType::Fiction, &[])).unwrap();
        assert!(v["properties"].get("g_metrics").is_none());

        let p = Para {
            id: "P1".into(),
            block: 0,
            hash: String::new(),
            text: "Mara ran.".into(),
        };
        let info = SceneInfo {
            extractors: &on,
            project_title: "T",
            project_type: ProjectType::Fiction,
            title: "S",
            pov: None,
            known: &[],
        };
        let prompt = build_prompt(&info, &[p], 0..1);
        assert!(prompt.user.contains("a_tension"));
        assert!(prompt.user.contains("\"g_metrics\":{\"a_tension\":2}"));
    }

    #[test]
    fn tension_is_read_clamped_and_optional() {
        let x =
            parse_output(r#"{"a_scene":{"a_summary":"x"},"g_metrics":{"a_tension":7}}"#).unwrap();
        assert_eq!(x.metrics["tension"], 7.0);
        let x = parse_output(r#"{"a_scene":{"a_summary":"x"},"g_metrics":{"a_tension":"99"}}"#)
            .unwrap();
        assert_eq!(x.metrics["tension"], 10.0);
        let x = parse_output(r#"{"a_scene":{"a_summary":"x"}}"#).unwrap();
        assert!(x.metrics.is_empty());
    }
}
