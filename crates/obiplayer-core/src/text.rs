//! Text normalisation shared by grouping, matching and sorting.

use std::cmp::Ordering;

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Lower-case, strip diacritics, NFC. "Barón Rojo" and "BARON ROJO" fold to
/// the same key.
pub fn fold(s: &str) -> String {
    s.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .nfc()
        .collect()
}

/// Sort key: folded, ignoring leading punctuation (`"Heroes"`, `...Llego`).
pub fn sort_key(s: &str) -> String {
    let f = fold(s);
    let trimmed = f.trim_start_matches(|c: char| !c.is_alphanumeric());
    if trimmed.is_empty() {
        f
    } else {
        trimmed.to_owned()
    }
}

/// Case- and accent-insensitive ordering that ignores leading punctuation,
/// stable for equal keys.
pub fn compare(a: &str, b: &str) -> Ordering {
    sort_key(a).cmp(&sort_key(b)).then_with(|| a.cmp(b))
}

/// Upper-case the first letter of each word, except short all-caps words
/// such as "UK" or "DJ".
pub fn capitalize_words(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            if w.chars().count() <= 2 && w.chars().all(|c| !c.is_lowercase()) {
                return w.to_owned();
            }
            let mut cs = w.chars();
            match cs.next() {
                Some(f) => f.to_uppercase().chain(cs).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding() {
        assert_eq!(fold("Barón Rojo"), "baron rojo");
        assert_eq!(fold("BARON ROJO"), "baron rojo");
        assert_eq!(fold("Mötley Crüe"), "motley crue");
        assert!(compare("Ángel", "angel").is_ne());
        assert_eq!(compare("Ángel", "Bravo"), Ordering::Less);
        assert_eq!(compare("\"Heroes\"", "Highway"), Ordering::Less);
        assert_eq!(compare("...Llego El Dia", "Abbey Road"), Ordering::Greater);
        assert_eq!(sort_key("!!!"), "!!!");
    }

    #[test]
    fn capitalization() {
        assert_eq!(capitalize_words("progressive rock"), "Progressive Rock");
        assert_eq!(capitalize_words("UK garage"), "UK Garage");
        assert_eq!(capitalize_words("j-pop"), "J-pop");
    }
}
