use core::fmt::{self, Write};

use alloc::{borrow::Cow, format};

/// A struct represents a string contains or does not contains escaped
/// characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscapeStr<'a> {
    str: &'a str,
    needs_escape: bool,
}

impl<'a> EscapeStr<'a> {
    #[must_use]
    pub fn new(str: &'a str, needs_escape: bool) -> Self {
        Self { str, needs_escape }
    }

    pub fn to_escaped_string(&self) -> Cow<'a, str> {
        if !self.needs_escape {
            Cow::Borrowed(self.str)
        } else {
            Cow::Owned(format!("{self}"))
        }
    }

    pub fn raw_str(&self) -> &'a str {
        self.str
    }
}

impl fmt::Display for EscapeStr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.needs_escape {
            return write!(f, "{}", self.str);
        }

        fn needs_escape(ch: char) -> Option<char> {
            if ch == '\'' || ch == '\"' || ch == '\\' {
                Some(ch)
            } else {
                None
            }
        }

        let mut chars = self.str.chars().peekable();

        while let Some(ch) = chars.peek().copied() {
            chars.next();

            if ch != '\\' {
                f.write_char(ch)?;
                continue;
            }

            if let Some(ch) = chars.peek() {
                if let Some(escaped_ch) = needs_escape(*ch) {
                    chars.next();
                    f.write_char(escaped_ch)?;
                } else {
                    f.write_char('\\')?;
                }
            }
        }
        Ok(())
    }
}
