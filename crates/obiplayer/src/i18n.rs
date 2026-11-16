//! UI translations.
//!
//! Strings are written in English in the code and looked up in a gettext
//! `.po` catalog embedded at build time (`po/<lang>.po`). Parsing it
//! ourselves avoids a dependency on the C gettext runtime.

use std::collections::HashMap;
use std::sync::OnceLock;

static CATALOG: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Embedded catalogs by language code.
const CATALOGS: &[(&str, &str)] = &[("es", include_str!("../../../po/es.po"))];

/// `setting`: "system" (from LANGUAGE/LC_ALL/LC_MESSAGES/LANG), "en", "es"…
pub fn init(setting: &str) {
    let lang = if setting == "system" || setting.is_empty() {
        ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|v| std::env::var(v).ok())
            .find(|v| !v.is_empty())
            .unwrap_or_default()
    } else {
        setting.to_owned()
    };
    let code: String = lang
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    let map = CATALOGS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, po)| parse_po(po))
        .unwrap_or_default();
    let _ = CATALOG.set(map);
}

pub fn tr(s: &str) -> String {
    CATALOG
        .get()
        .and_then(|m| m.get(s))
        .cloned()
        .unwrap_or_else(|| s.to_owned())
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Minimal `.po` parser: msgid/msgstr with continuation lines.
pub fn parse_po(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let (mut id, mut st) = (String::new(), String::new());
    let mut in_str = false;
    let quoted = |l: &str| {
        l.trim()
            .strip_prefix('"')
            .and_then(|x| x.strip_suffix('"'))
            .map(unescape)
    };
    let mut flush = |id: &mut String, st: &mut String| {
        if !id.is_empty() && !st.is_empty() {
            map.insert(std::mem::take(id), std::mem::take(st));
        }
        id.clear();
        st.clear();
    };
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("msgid ") {
            flush(&mut id, &mut st);
            id = quoted(rest).unwrap_or_default();
            in_str = false;
        } else if let Some(rest) = l.strip_prefix("msgstr ") {
            st = quoted(rest).unwrap_or_default();
            in_str = true;
        } else if l.starts_with('"')
            && let Some(q) = quoted(l)
        {
            if in_str {
                st.push_str(&q)
            } else {
                id.push_str(&q)
            }
        }
    }
    flush(&mut id, &mut st);
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_po() {
        let m = parse_po(
            "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain\\n\"\n\n#: x.rs\nmsgid \"Play\"\nmsgstr \"Reproducir\"\n\nmsgid \"A \\\"b\\\"\"\nmsgstr \"\"\n\"Un \"\n\"\\\"b\\\"\"\n\nmsgid \"Untranslated\"\nmsgstr \"\"\n",
        );
        assert_eq!(m.get("Play").map(String::as_str), Some("Reproducir"));
        assert_eq!(m.get("A \"b\"").map(String::as_str), Some("Un \"b\""));
        assert!(!m.contains_key("Untranslated"));
        assert!(!m.contains_key(""));
    }
}
