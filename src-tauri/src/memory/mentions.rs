//! Mentions (SPEC §9.3): where each entry's name or alias appears. Derived
//! from the text, never stored as truth, and so always up to date.

/// How many times any of `names` occurs in `text` as a whole word
/// (case-insensitive; a possessive `'s` still counts).
pub fn count_mentions(names: &[&str], text: &str) -> u32 {
    count_in_lower(names, &text.to_lowercase())
}

/// [`count_mentions`] on text that is already lowercase (cached by the caller).
pub fn count_in_lower(names: &[&str], hay: &str) -> u32 {
    let mut total = 0;
    // Longest first, and blank out matches so "Mara Voss" is not also "Mara".
    let mut needles: Vec<String> = names
        .iter()
        .map(|n| n.trim().to_lowercase())
        .filter(|n| n.chars().count() >= 2)
        .collect();
    needles.sort_by_key(|n| std::cmp::Reverse(n.len()));
    needles.dedup();
    let mut spans: Vec<(usize, usize)> = vec![];
    for n in &needles {
        let mut from = 0;
        while let Some(i) = hay[from..].find(n.as_str()) {
            let start = from + i;
            let end = start + n.len();
            from = end;
            let before_ok = hay[..start]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric());
            let after_ok = hay[end..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric());
            if before_ok && after_ok && !spans.iter().any(|&(s, e)| start < e && end > s) {
                spans.push((start, end));
                total += 1;
            }
        }
    }
    total
}

/// Does the text mention any of the names?
pub fn mentions_any(names: &[&str], text: &str) -> bool {
    count_mentions(names, text) > 0
}

/// Share of the characters in `text` that sit inside dialogue quotes (0.0–1.0).
pub fn dialogue_ratio(text: &str) -> f64 {
    let total = text.chars().filter(|c| !c.is_whitespace()).count();
    if total == 0 {
        return 0.0;
    }
    let mut inside = false;
    let mut spoken = 0usize;
    for c in text.chars() {
        match c {
            '“' | '"' if !inside => inside = true,
            '”' | '"' if inside => inside = false,
            c if inside && !c.is_whitespace() => spoken += 1,
            _ => {}
        }
    }
    spoken as f64 / total as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_whole_words_only() {
        assert_eq!(
            count_mentions(&["Mara"], "Mara ran. Maras? Mara's coat. A mara."),
            3
        );
        assert_eq!(count_mentions(&["Kessel"], "Kesselring is not it."), 0);
        assert_eq!(count_mentions(&["Ann"], "Anna and Ann."), 1);
    }

    #[test]
    fn a_longer_name_is_not_also_counted_as_its_short_form() {
        assert_eq!(
            count_mentions(&["Mara Voss", "Mara"], "Mara Voss met Mara."),
            2
        );
        assert_eq!(count_mentions(&["Mara", "Mara Voss"], "Mara Voss"), 1);
    }

    #[test]
    fn short_names_and_empty_text_are_safe() {
        assert_eq!(count_mentions(&["A", ""], "A a A"), 0);
        assert_eq!(count_mentions(&["Mara"], ""), 0);
        assert!(!mentions_any(&["Zoë"], "zoe"));
        assert!(mentions_any(&["Zoë"], "Then ZOË left."));
    }

    #[test]
    fn dialogue_ratio_counts_quoted_text() {
        let r = dialogue_ratio("“Go home,” she said. “Now.”");
        assert!(r > 0.4 && r < 0.8, "{r}");
        assert_eq!(dialogue_ratio("No quotes here."), 0.0);
        assert_eq!(dialogue_ratio(""), 0.0);
    }
}
