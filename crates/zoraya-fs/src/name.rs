//! Names and relative paths (ADR-0013 rules 1, 2, 4 and 6).
//!
//! The `abi` crate that ADR-0013 names as the home of `Name` does not exist
//! yet; until it does, this module is the one validator, used by the request
//! API and by the on-disk decoder alike.

use core::fmt;

/// Longest name component in bytes (ADR-0013 rule 4).
pub const MAX_NAME: usize = 255;
/// Longest relative path in a request, in bytes (ADR-0013 rule 4).
pub const MAX_PATH: usize = 4096;

/// Why a name or path was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    /// `.` or `..`.
    Dot,
    /// Contains `/`.
    Slash,
    /// Contains a control character (U+0000–U+001F, U+007F, U+0080–U+009F).
    Control,
    /// Not well-formed UTF-8 (overlong forms, surrogates, above U+10FFFF,
    /// truncated sequences).
    NotUtf8,
    /// A path starting with `/`.
    Absolute,
}

/// Checks one name component against ADR-0013 rules 1, 2 and 4.
///
/// Takes bytes so that the on-disk decoder can use it on untrusted input.
pub fn validate_name(bytes: &[u8]) -> Result<&str, NameError> {
    if bytes.is_empty() {
        return Err(NameError::Empty);
    }
    if bytes.len() > MAX_NAME {
        return Err(NameError::TooLong);
    }
    if bytes == b"." || bytes == b".." {
        return Err(NameError::Dot);
    }
    // core's UTF-8 validation rejects overlong forms, surrogates and values
    // above U+10FFFF, which is exactly rule 1.
    let s = core::str::from_utf8(bytes).map_err(|_| NameError::NotUtf8)?;
    for c in s.chars() {
        if c == '/' {
            return Err(NameError::Slash);
        }
        if is_forbidden_control(c) {
            return Err(NameError::Control);
        }
    }
    Ok(s)
}

/// U+0000–U+001F, U+007F and U+0080–U+009F (ADR-0013 rule 2).
pub fn is_forbidden_control(c: char) -> bool {
    let v = u32::from(c);
    v <= 0x1F || (0x7F..=0x9F).contains(&v)
}

/// A validated name component.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name<'a>(&'a str);

impl<'a> Name<'a> {
    pub fn new(s: &'a str) -> Result<Name<'a>, NameError> {
        validate_name(s.as_bytes()).map(Name)
    }

    pub fn as_str(&self) -> &'a str {
        self.0
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.0.as_bytes()
    }
}

impl fmt::Debug for Name<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.0, f)
    }
}

/// A validated relative path: 1 to 4,096 bytes, components separated by
/// single `/`, each a valid [`Name`]. No leading or trailing `/`, no empty
/// component, no `.` or `..` (ADR-0013 rule 6, INV-0013-5).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RelPath<'a>(&'a str);

impl<'a> RelPath<'a> {
    pub fn new(s: &'a str) -> Result<RelPath<'a>, NameError> {
        if s.is_empty() {
            return Err(NameError::Empty);
        }
        if s.len() > MAX_PATH {
            return Err(NameError::TooLong);
        }
        if s.starts_with('/') {
            return Err(NameError::Absolute);
        }
        for comp in s.split('/') {
            validate_name(comp.as_bytes())?;
        }
        Ok(RelPath(s))
    }

    pub fn as_str(&self) -> &'a str {
        self.0
    }

    /// The components in order.
    pub fn components(&self) -> impl Iterator<Item = Name<'a>> + 'a {
        self.0.split('/').map(Name)
    }

    /// The parent part (if any) and the last component.
    pub fn split_last(&self) -> (Option<RelPath<'a>>, Name<'a>) {
        match self.0.rfind('/') {
            Some(i) => (Some(RelPath(&self.0[..i])), Name(&self.0[i + 1..])),
            None => (None, Name(self.0)),
        }
    }
}

impl fmt::Debug for RelPath<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.0, f)
    }
}
