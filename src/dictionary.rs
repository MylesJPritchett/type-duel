use std::collections::HashSet;
use std::sync::LazyLock;

/// The raw EFF large wordlist file, embedded at compile time.
const WORDLIST_RAW: &str = include_str!("../data/wordlist.txt");

/// Lazily parsed set of valid words from the EFF wordlist.
static WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    WORDLIST_RAW
        .lines()
        .filter_map(|line| line.split('\t').nth(1))
        .collect()
});

/// Check if a word is valid: present in the dictionary, at least 3 characters,
/// and contains only lowercase ASCII letters.
pub fn is_valid_word(word: &str) -> bool {
    word.len() >= 3
        && word.chars().all(|c| c.is_ascii_lowercase())
        && WORDS.contains(word)
}

/// Validate a word and return a specific rejection reason, or None if valid.
pub fn validate_word(word: &str) -> Option<&'static str> {
    if word.len() < 3 {
        return Some("too_short");
    }
    if !word.chars().all(|c| c.is_ascii_lowercase()) {
        return Some("invalid_chars");
    }
    if !WORDS.contains(word) {
        return Some("not_in_dictionary");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wordlist_loads() {
        // The EFF wordlist has 7776 words
        assert_eq!(WORDS.len(), 7776);
    }

    #[test]
    fn test_known_words_are_valid() {
        assert!(is_valid_word("abacus"));
        assert!(is_valid_word("zoom"));
        assert!(is_valid_word("abide"));
    }

    #[test]
    fn test_too_short() {
        assert!(!is_valid_word("ab"));
        assert!(!is_valid_word("a"));
        assert!(!is_valid_word(""));
    }

    #[test]
    fn test_invalid_chars() {
        assert!(!is_valid_word("Hello")); // uppercase
        assert!(!is_valid_word("test123")); // digits
        assert!(!is_valid_word("no-way")); // hyphen
        assert!(!is_valid_word("hi there")); // space
    }

    #[test]
    fn test_not_in_dictionary() {
        assert!(!is_valid_word("zzzzzzz"));
        assert!(!is_valid_word("xyzabc"));
    }

    #[test]
    fn test_validate_word_returns_reasons() {
        assert_eq!(validate_word("ab"), Some("too_short"));
        assert_eq!(validate_word("Hello"), Some("invalid_chars"));
        assert_eq!(validate_word("zzzzzzz"), Some("not_in_dictionary"));
        assert_eq!(validate_word("abacus"), None);
    }
}
