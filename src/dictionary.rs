use std::collections::HashSet;
use std::sync::LazyLock;

/// The raw EFF large wordlist file, embedded at compile time.
const WORDLIST_RAW: &str = include_str!("../data/wordlist.txt");

/// Lazily parsed set of valid words from the wordlist.
static WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    WORDLIST_RAW
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
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
        // The wordlist should have a substantial number of words
        assert!(WORDS.len() > 100_000, "Dictionary should have >100k words, got {}", WORDS.len());
    }

    #[test]
    fn test_known_words_are_valid() {
        assert!(is_valid_word("abacus"));
        assert!(is_valid_word("zoom"));
        assert!(is_valid_word("abide"));
    }

    #[test]
    fn test_common_english_words_are_valid() {
        // These are extremely common English words that any typing game dictionary must include
        let common_words = [
            "where", "this", "that", "have", "with", "from", "they", "been",
            "some", "what", "when", "your", "which", "their", "will", "each",
            "make", "like", "just", "over", "such", "take", "than", "them",
            "very", "after", "also", "made", "well", "back", "much", "then",
            "could", "people", "into", "year", "most", "good", "know", "come",
            "think", "about", "would", "still", "going", "never", "under",
            "same", "last", "long", "great", "little", "world", "house",
            "between", "before", "should", "through", "right", "small",
            "place", "around", "every", "being", "work", "three", "water",
            "balls", "hello", "keyboard", "typing", "game", "play", "fast",
            "slow", "quick", "jump", "runs", "walk", "read", "write",
        ];
        for word in &common_words {
            assert!(is_valid_word(word), "Common word '{word}' should be in dictionary");
        }
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
