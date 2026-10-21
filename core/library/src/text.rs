//! Text keys for sorting and grouping.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Case-, accent- and article-insensitive sort key: "The Beatles" sorts as "beatles",
/// "Björk" next to "bjork".
pub(crate) fn sort_key(s: &str) -> String {
    let folded: String = s
        .nfkd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase();
    let t = folded.trim();
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = t.strip_prefix(article)
            && !rest.trim().is_empty()
        {
            return rest.trim().to_owned();
        }
    }
    t.to_owned()
}

/// Grouping identity: Unicode-normalised, case-folded, whitespace-collapsed. Accents are kept
/// ("Café" and "Cafe" are different albums), unlike [`sort_key`].
pub(crate) fn identity(s: &str) -> String {
    s.nfkc()
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parses the leading integer of "3", "03", "3/12".
pub(crate) fn leading_number(s: &str) -> Option<i64> {
    let digits: String = s
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// Year from "2021", "2021-03-04", "2021/03".
pub(crate) fn year(s: &str) -> Option<i64> {
    let y = leading_number(s)?;
    (1000..=9999).contains(&y).then_some(y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_keys_ignore_articles_case_and_accents() {
        assert_eq!(sort_key("The Beatles"), "beatles");
        assert_eq!(sort_key("Björk"), "bjork");
        assert_eq!(sort_key("The"), "the");
        assert_eq!(sort_key("  An Album "), "album");
    }

    #[test]
    fn identity_keeps_accents_but_folds_case_and_space() {
        assert_eq!(identity("Greatest  Hits"), identity("greatest hits"));
        assert_ne!(identity("Café"), identity("Cafe"));
    }

    #[test]
    fn numbers_and_years() {
        assert_eq!(leading_number("03/12"), Some(3));
        assert_eq!(year("2021-03-04"), Some(2021));
        assert_eq!(year("abc"), None);
    }
}
