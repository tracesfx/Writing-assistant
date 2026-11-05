//! Spellcheck (SPEC §11.7): bundled Hunspell dictionaries (en-US, en-CA,
//! SCOWL-derived, see `dictionaries/LICENSE-SCOWL.txt`) plus a per-project
//! personal dictionary in `dictionary.txt` (one word per line).
//!
//! Checking runs on a worker thread (the Tauri command layer), never in
//! the editor; the editor only receives misspelled ranges.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use specta::Type;
use spellbook::Dictionary;

use crate::error::{MarginError, Result};
use crate::events::{DomainEvent, EventBus};
use crate::project::atomic::atomic_write;
use crate::project::manifest::Locale;
use crate::story::StoryService;

pub const DICTIONARY_FILE: &str = "dictionary.txt";

const EN_US_AFF: &str = include_str!("../../dictionaries/en_US.aff");
const EN_US_DIC: &str = include_str!("../../dictionaries/en_US.dic");
const EN_CA_AFF: &str = include_str!("../../dictionaries/en_CA.aff");
const EN_CA_DIC: &str = include_str!("../../dictionaries/en_CA.dic");

/// A misspelled word: UTF-16 offsets within the checked text.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpellRange {
    pub start: u32,
    pub end: u32,
}

struct Loaded {
    locale: Locale,
    dict: Dictionary,
    personal: HashSet<String>,
    root: PathBuf,
}

pub struct Spell {
    story: Arc<StoryService>,
    state: Mutex<Option<Loaded>>,
}

fn load_dictionary(locale: Locale) -> Result<Dictionary> {
    let (aff, dic) = match locale {
        Locale::EnUs => (EN_US_AFF, EN_US_DIC),
        Locale::EnCa => (EN_CA_AFF, EN_CA_DIC),
    };
    Dictionary::new(aff, dic).map_err(|e| MarginError::Internal(format!("dictionary: {e}")))
}

fn read_personal(root: &Path) -> HashSet<String> {
    fs::read_to_string(root.join(DICTIONARY_FILE))
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Words in `text` as (byte start, byte end). A word is letters with inner
/// apostrophes; hyphenated compounds are checked part by part.
pub fn words(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (start, c) = chars[i];
        if !c.is_alphanumeric() {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut has_digit = c.is_ascii_digit();
        while j < chars.len() {
            let c = chars[j].1;
            if c.is_alphanumeric() {
                has_digit |= c.is_ascii_digit();
                j += 1;
            } else if (c == '\'' || c == '’')
                && chars.get(j + 1).is_some_and(|n| n.1.is_alphabetic())
            {
                j += 1;
            } else {
                break;
            }
        }
        let end = chars.get(j).map_or(text.len(), |x| x.0);
        if !has_digit {
            out.push((start, end));
        }
        i = j;
    }
    out
}

fn utf16(text: &str, byte: usize) -> u32 {
    text[..byte].chars().map(char::len_utf16).sum::<usize>() as u32
}

impl Loaded {
    fn ok(&self, word: &str) -> bool {
        let w = word.replace('’', "'");
        if w.chars().count() < 2 && !w.chars().any(char::is_uppercase) {
            return true;
        }
        if self.personal.contains(&w) || self.personal.contains(&w.to_lowercase()) {
            return true;
        }
        if self.dict.check(&w) {
            return true;
        }
        // Possessives of names the author added: "Mara's".
        if let Some(base) = w.strip_suffix("'s")
            && (self.personal.contains(base) || self.dict.check(base))
        {
            return true;
        }
        false
    }
}

impl Spell {
    pub fn new(story: Arc<StoryService>, bus: Arc<EventBus>) -> Arc<Self> {
        let s = Arc::new(Spell {
            story,
            state: Mutex::new(None),
        });
        let weak = Arc::downgrade(&s);
        bus.subscribe(move |e| {
            let Some(s) = weak.upgrade() else { return };
            match &e.event {
                DomainEvent::ProjectOpened { .. } | DomainEvent::ProjectSettingsChanged { .. } => {
                    // Parsing a dictionary takes a moment: do it off the event thread.
                    std::thread::spawn(move || {
                        if let Err(err) = s.ensure_loaded() {
                            eprintln!("[margin] spellcheck: {err}");
                        }
                    });
                }
                DomainEvent::ProjectClosed { .. } => {
                    if let Ok(mut g) = s.state.lock() {
                        *g = None;
                    }
                }
                _ => {}
            }
        });
        s
    }

    /// Load (or reload, after a locale change) the dictionary for the open project.
    fn ensure_loaded(&self) -> Result<()> {
        let info = self.story.project_info()?;
        let root = PathBuf::from(&info.root);
        {
            let g = self.lock()?;
            if let Some(l) = g.as_ref()
                && l.locale == info.locale
                && l.root == root
            {
                return Ok(());
            }
        }
        let dict = load_dictionary(info.locale)?;
        let personal = read_personal(&root);
        *self.lock()? = Some(Loaded {
            locale: info.locale,
            dict,
            personal,
            root,
        });
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Option<Loaded>>> {
        self.state
            .lock()
            .map_err(|_| MarginError::Internal("spellcheck lock poisoned".into()))
    }

    /// Misspelled ranges for each text (UTF-16 offsets).
    pub fn check(&self, texts: &[String]) -> Result<Vec<Vec<SpellRange>>> {
        self.ensure_loaded()?;
        let g = self.lock()?;
        let l = g.as_ref().ok_or(MarginError::NoProject)?;
        Ok(texts
            .iter()
            .map(|t| {
                words(t)
                    .into_iter()
                    .filter(|&(s, e)| !l.ok(&t[s..e]))
                    .map(|(s, e)| SpellRange {
                        start: utf16(t, s),
                        end: utf16(t, e),
                    })
                    .collect()
            })
            .collect())
    }

    pub fn suggest(&self, word: &str) -> Result<Vec<String>> {
        self.ensure_loaded()?;
        let g = self.lock()?;
        let l = g.as_ref().ok_or(MarginError::NoProject)?;
        let mut out = Vec::new();
        l.dict.suggest(&word.replace('’', "'"), &mut out);
        // Keep the author's apostrophe style.
        if word.contains('’') {
            for s in &mut out {
                *s = s.replace('\'', "’");
            }
        }
        out.truncate(8);
        Ok(out)
    }

    /// Add a word to the project's personal dictionary.
    pub fn add_word(&self, word: &str) -> Result<()> {
        let word = word.trim().replace('’', "'");
        if word.is_empty() || word.contains(char::is_whitespace) {
            return Err(MarginError::Invalid("add one word at a time".into()));
        }
        self.ensure_loaded()?;
        let mut g = self.lock()?;
        let l = g.as_mut().ok_or(MarginError::NoProject)?;
        if l.personal.insert(word) {
            let mut words: Vec<&String> = l.personal.iter().collect();
            words.sort_by_key(|w| w.to_lowercase());
            let text = words
                .iter()
                .map(|w| w.as_str())
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            atomic_write(&l.root.join(DICTIONARY_FILE), text.as_bytes())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_words() {
        let t = "Mara’s well-known 2nd boat, don't R2D2 x.";
        let w: Vec<&str> = words(t).into_iter().map(|(s, e)| &t[s..e]).collect();
        assert_eq!(w, ["Mara’s", "well", "known", "boat", "don't", "x"]);
    }

    #[test]
    fn dictionaries_differ_by_locale() {
        let us = load_dictionary(Locale::EnUs).unwrap();
        let ca = load_dictionary(Locale::EnCa).unwrap();
        assert!(us.check("color") && !us.check("colour"));
        assert!(ca.check("colour"));
        assert!(!us.check("teh"));
        let mut s = Vec::new();
        us.suggest("teh", &mut s);
        assert!(s.iter().any(|x| x == "the"), "{s:?}");
    }

    #[test]
    fn personal_words_and_possessives() {
        let l = Loaded {
            locale: Locale::EnUs,
            dict: load_dictionary(Locale::EnUs).unwrap(),
            personal: ["Kessel".to_string()].into_iter().collect(),
            root: PathBuf::new(),
        };
        assert!(l.ok("Kessel"));
        assert!(l.ok("Kessel’s"));
        assert!(l.ok("don’t"));
        assert!(!l.ok("Kesel"));
    }
}
