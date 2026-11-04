//! `/etc/zoraya/session` (ADR-0104 item 12, ADR-0110 item 1): the session's
//! command and what its environment adds. Plain lines (ADR-0190):
//!
//! ```text
//! # a comment
//! command sh -i
//! env PS1="PSURF> "
//! env ENV=/dev/null
//! ```
//!
//! `command` takes words split at spaces, with no quoting; `env` takes a
//! `NAME=VALUE` to the end of the line, the value as written, or between
//! double quotes when it starts with one (for a value with a space at its
//! end, which editors drop).

#![forbid(unsafe_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// The session's base environment (ADR-0104 item 12).
pub const BASE: [&str; 4] = [
    "PATH=/bin:/usr/bin",
    "HOME=/home",
    "TERM=dumb",
    "SHELL=/bin/sh",
];

/// The longest file read.
pub const MAX_SIZE: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// `argv`; its first word names the program under `/bin` unless it has
    /// a `/`.
    pub command: Vec<String>,
    /// `NAME=VALUE`, in the file's order.
    pub env: Vec<String>,
}

impl Default for Session {
    /// No file: the shell, interactive.
    fn default() -> Session {
        Session {
            command: alloc::vec![String::from("sh"), String::from("-i")],
            env: Vec::new(),
        }
    }
}

/// A line the file cannot have, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error {
    pub line: usize,
    pub reason: &'static str,
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

pub fn parse(text: &str) -> Result<Session, Error> {
    let mut command: Option<Vec<String>> = None;
    let mut env = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let err = |reason| Error {
            line: i + 1,
            reason,
        };
        let (keyword, rest) = line.split_once(' ').unwrap_or((line, ""));
        match keyword {
            "command" => {
                if command.is_some() {
                    return Err(err("a second command"));
                }
                let words: Vec<String> = rest
                    .split(' ')
                    .filter(|w| !w.is_empty())
                    .map(ToString::to_string)
                    .collect();
                if words.is_empty() {
                    return Err(err("an empty command"));
                }
                command = Some(words);
            }
            "env" => {
                let Some((name, _)) = rest.split_once('=') else {
                    return Err(err("env without NAME=VALUE"));
                };
                if !valid_name(name) {
                    return Err(err("not a variable name"));
                }
                let value = &rest[name.len() + 1..];
                let value = match value.strip_prefix('"') {
                    Some(quoted) => quoted.strip_suffix('"').ok_or(err("an unclosed quote"))?,
                    None => value,
                };
                env.push(alloc::format!("{name}={value}"));
            }
            _ => return Err(err("unknown keyword")),
        }
    }
    Ok(Session {
        command: command.unwrap_or_else(|| Session::default().command),
        env,
    })
}

/// The environment the session's first process starts with: the base,
/// with what the file adds after it, a name the file gives again replacing
/// the base's.
pub fn environment(s: &Session) -> Vec<String> {
    let mut out: Vec<String> = BASE.iter().map(|v| v.to_string()).collect();
    for v in &s.env {
        let name = v.split_once('=').map_or(v.as_str(), |(n, _)| n);
        out.retain(|old| old.split_once('=').map(|(n, _)| n) != Some(name));
        out.push(v.clone());
    }
    out
}

/// The program's path: `/bin/` and the first word, unless it has a `/`.
pub fn program(s: &Session) -> String {
    let first = s.command.first().map_or("sh", String::as_str);
    if first.contains('/') {
        first.to_string()
    } else {
        alloc::format!("/bin/{first}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_harness_session_parses() {
        let text =
            "# ADR-0110's session\ncommand sh -i\nenv PS1=\"PSURF> \"\nenv ENV=/dev/null\n\n";
        let s = parse(text).unwrap();
        assert_eq!(s.command, ["sh", "-i"]);
        assert_eq!(s.env, ["PS1=PSURF> ", "ENV=/dev/null"]);
        assert_eq!(program(&s), "/bin/sh");
        let env = environment(&s);
        assert_eq!(&env[..4], &BASE);
        assert_eq!(&env[4..], ["PS1=PSURF> ", "ENV=/dev/null"]);
    }

    #[test]
    fn a_name_given_again_replaces_the_base() {
        let s = parse("env TERM=vt100\nenv HOME=/tmp/x\n").unwrap();
        let env = environment(&s);
        assert!(env.contains(&String::from("TERM=vt100")));
        assert!(!env.contains(&String::from("TERM=dumb")));
        assert_eq!(env.len(), 4);
        assert_eq!(parse("").unwrap(), Session::default());
        assert_eq!(program(&parse("command /usr/bin/x").unwrap()), "/usr/bin/x");
    }

    #[test]
    fn bad_lines_are_named() {
        for (text, line, reason) in [
            ("command\n", 1, "an empty command"),
            ("command a\ncommand b\n", 2, "a second command"),
            ("\nenv X\n", 2, "env without NAME=VALUE"),
            ("env 1X=y\n", 1, "not a variable name"),
            ("env =y\n", 1, "not a variable name"),
            ("env A-B=y\n", 1, "not a variable name"),
            ("shell sh\n", 1, "unknown keyword"),
            ("env A=\"x\n", 1, "an unclosed quote"),
        ] {
            assert_eq!(parse(text), Err(Error { line, reason }), "{text:?}");
        }
    }
}
