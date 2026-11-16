use std::fmt;

/// One `key: value` line of an MPD response.
pub type Pair = (String, String);

/// Quote an argument for the MPD protocol: wrap in `"` and escape `"` and `\`.
pub fn quote(arg: &str) -> String {
    let mut s = String::with_capacity(arg.len() + 2);
    s.push('"');
    for c in arg.chars() {
        if c == '"' || c == '\\' {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('"');
    s
}

/// A single protocol command. Every argument is quoted and escaped, which MPD
/// accepts for all argument types (numbers included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    line: String,
    invalid: Option<String>,
}

impl Command {
    pub fn new(name: &str) -> Self {
        Command {
            line: name.to_owned(),
            invalid: None,
        }
    }

    pub fn arg(mut self, arg: impl fmt::Display) -> Self {
        let arg = arg.to_string();
        if arg.contains('\n') && self.invalid.is_none() {
            self.invalid = Some(arg.clone());
        }
        self.line.push(' ');
        self.line.push_str(&quote(&arg));
        self
    }

    pub fn args<I, T>(self, args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: fmt::Display,
    {
        args.into_iter().fold(self, Command::arg)
    }

    /// The wire form, without the trailing newline.
    pub fn line(&self) -> &str {
        &self.line
    }

    pub(crate) fn validate(&self) -> super::Result<()> {
        match &self.invalid {
            Some(a) => Err(super::Error::InvalidArgument(a.clone())),
            None => Ok(()),
        }
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.line)
    }
}

/// Split a response line into `(key, value)`.
pub(crate) fn parse_pair(line: &str) -> Option<Pair> {
    let (k, v) = line.split_once(": ")?;
    Some((k.to_owned(), v.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_escapes_quotes_and_backslashes() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote(r#"AC/DC "Live""#), r#""AC/DC \"Live\"""#);
        assert_eq!(quote(r"a\b"), r#""a\\b""#);
        assert_eq!(quote(""), "\"\"");
    }

    #[test]
    fn command_builds_quoted_line() {
        let c = Command::new("sticker")
            .args(["set", "song"])
            .arg("Rock/It's \"x\".flac")
            .arg("rating")
            .arg(5);
        assert_eq!(
            c.line(),
            r#"sticker "set" "song" "Rock/It's \"x\".flac" "rating" "5""#
        );
        assert!(c.validate().is_ok());
    }

    #[test]
    fn command_rejects_newlines() {
        let c = Command::new("add").arg("evil\nclear");
        assert!(c.validate().is_err());
    }

    #[test]
    fn pairs_split_on_first_colon_space() {
        assert_eq!(
            parse_pair("Title: Time: The Conqueror"),
            Some(("Title".into(), "Time: The Conqueror".into()))
        );
        assert_eq!(parse_pair("garbage"), None);
    }
}
