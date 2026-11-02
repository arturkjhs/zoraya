//! The mutation lists, `tools/mutants/*.toml`: a small subset of TOML, read
//! here so that the tool needs only `std` (docs/dependencies.md).
//!
//! ```text
//! # Defaults for every mutant in the file.
//! tests = ["cargo test -q -p zoraya-virtio"]
//! timeout = 600
//!
//! [[mutant]]
//! id = "V1"
//! file = "crates/zoraya-virtio/src/lib.rs"
//! find = 'if length > writable {'
//! replace = 'if false {'
//! note = "the used length is trusted"
//! ```
//!
//! Values are basic strings (`"..."`, with `\n \t \" \\`), literal strings
//! (`'...'`), multi-line literal strings (`'''...'''`), integers, and arrays
//! of strings on one line. `find` must occur exactly once in `file`, unless
//! `occurrence = n` picks the n-th (from 1). A mutant may set its own `tests`
//! and `timeout`.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Number(u64),
    List(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mutant {
    pub id: String,
    pub file: String,
    pub find: String,
    pub replace: String,
    pub occurrence: Option<u64>,
    pub tests: Vec<String>,
    pub timeout: u64,
    pub note: String,
}

/// The default time a test command may take, in seconds.
pub const TIMEOUT: u64 = 900;

/// Reads one list. `source` names it in errors.
pub fn parse(source: &str, text: &str) -> Result<Vec<Mutant>, String> {
    let mut top: BTreeMap<String, Value> = BTreeMap::new();
    let mut tables: Vec<(usize, BTreeMap<String, Value>)> = Vec::new();
    let mut lines = text.lines().enumerate();
    while let Some((index, line)) = lines.next() {
        let number = index + 1;
        let at = |e: String| format!("{source}:{number}: {e}");
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[mutant]]" {
            tables.push((number, BTreeMap::new()));
            continue;
        }
        if line.starts_with('[') {
            return Err(at(format!("only [[mutant]] tables: {line}")));
        }
        let (key, rest) = line
            .split_once('=')
            .ok_or_else(|| at("expected key = value".into()))?;
        let key = key.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(at(format!("bad key {key:?}")));
        }
        let rest = rest.trim();
        let value = if let Some(after) = rest.strip_prefix("'''") {
            // Multi-line literal: up to the closing quotes; a newline right
            // after the opening ones is dropped, as TOML does.
            let mut body = String::new();
            let mut current = after.to_string();
            let mut first = true;
            loop {
                if let Some(end) = current.find("'''") {
                    body.push_str(&current[..end]);
                    if !current[end + 3..].trim().is_empty()
                        && !current[end + 3..].trim().starts_with('#')
                    {
                        return Err(at("text after '''".into()));
                    }
                    break;
                }
                if !(first && current.is_empty()) {
                    body.push_str(&current);
                    body.push('\n');
                }
                first = false;
                match lines.next() {
                    Some((_, next)) => current = next.to_string(),
                    None => return Err(at("''' not closed".into())),
                }
            }
            Value::Text(body)
        } else {
            let (value, after) = value(rest).map_err(at)?;
            let after = after.trim();
            if !after.is_empty() && !after.starts_with('#') {
                return Err(at(format!("text after the value: {after}")));
            }
            value
        };
        let table = match tables.last_mut() {
            Some((_, table)) => table,
            None => &mut top,
        };
        if table.insert(key.to_string(), value).is_some() {
            return Err(at(format!("{key} given twice")));
        }
    }
    let default_tests = match top.remove("tests") {
        Some(Value::List(tests)) => tests,
        None => Vec::new(),
        Some(_) => return Err(format!("{source}: tests must be a list of strings")),
    };
    let default_timeout = match top.remove("timeout") {
        Some(Value::Number(n)) => n,
        None => TIMEOUT,
        Some(_) => return Err(format!("{source}: timeout must be a number")),
    };
    if let Some(key) = top.keys().next() {
        return Err(format!("{source}: unknown key {key} outside [[mutant]]"));
    }
    let mut out = Vec::new();
    for (number, mut table) in tables {
        let at = |e: &str| format!("{source}:{number}: {e}");
        let mut text = |key: &str, needed: bool| match table.remove(key) {
            Some(Value::Text(s)) => Ok(s),
            None if !needed => Ok(String::new()),
            None => Err(at(&format!("[[mutant]] needs {key}"))),
            Some(_) => Err(at(&format!("{key} must be a string"))),
        };
        let id = text("id", true)?;
        let file = text("file", true)?;
        let find = text("find", true)?;
        let replace = text("replace", true)?;
        let note = text("note", false)?;
        let occurrence = match table.remove("occurrence") {
            Some(Value::Number(0)) => return Err(at("occurrence counts from 1")),
            Some(Value::Number(n)) => Some(n),
            None => None,
            Some(_) => return Err(at("occurrence must be a number")),
        };
        let tests = match table.remove("tests") {
            Some(Value::List(tests)) => tests,
            None => default_tests.clone(),
            Some(_) => return Err(at("tests must be a list of strings")),
        };
        let timeout = match table.remove("timeout") {
            Some(Value::Number(n)) => n,
            None => default_timeout,
            Some(_) => return Err(at("timeout must be a number")),
        };
        if let Some(key) = table.keys().next() {
            return Err(at(&format!("unknown key {key}")));
        }
        if find.is_empty() || find == replace {
            return Err(at("find must be non-empty and differ from replace"));
        }
        if tests.is_empty() {
            return Err(at("no tests: set them for the file or the mutant"));
        }
        out.push(Mutant {
            id,
            file,
            find,
            replace,
            occurrence,
            tests,
            timeout,
            note,
        });
    }
    Ok(out)
}

/// One value at the start of `s`, and what follows it.
fn value(s: &str) -> Result<(Value, &str), String> {
    if s.starts_with('"') || s.starts_with('\'') {
        let (text, rest) = string(s)?;
        return Ok((Value::Text(text), rest));
    }
    if let Some(mut rest) = s.strip_prefix('[') {
        let mut items = Vec::new();
        loop {
            rest = rest.trim_start();
            if let Some(after) = rest.strip_prefix(']') {
                return Ok((Value::List(items), after));
            }
            let (item, after) = string(rest)?;
            items.push(item);
            rest = after.trim_start();
            if let Some(after) = rest.strip_prefix(',') {
                rest = after;
            } else if !rest.starts_with(']') {
                return Err("expected , or ] in a list".into());
            }
        }
    }
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if end == 0 {
        return Err(format!("not a value: {s}"));
    }
    let n = s[..end]
        .parse()
        .map_err(|_| format!("bad number {}", &s[..end]))?;
    Ok((Value::Number(n), &s[end..]))
}

/// A basic or literal string on one line.
fn string(s: &str) -> Result<(String, &str), String> {
    let mut chars = s.char_indices();
    let quote = match chars.next() {
        Some((_, q @ ('"' | '\''))) => q,
        _ => return Err(format!("expected a string: {s}")),
    };
    let mut out = String::new();
    while let Some((at, c)) = chars.next() {
        if c == quote {
            return Ok((out, &s[at + 1..]));
        }
        if c == '\\' && quote == '"' {
            match chars.next() {
                Some((_, 'n')) => out.push('\n'),
                Some((_, 't')) => out.push('\t'),
                Some((_, '"')) => out.push('"'),
                Some((_, '\\')) => out.push('\\'),
                other => return Err(format!("unknown escape {other:?}")),
            }
            continue;
        }
        out.push(c);
    }
    Err("string not closed".into())
}

/// The byte offset of the place `m` changes in `text`, if it is there.
pub fn locate(m: &Mutant, text: &str) -> Result<usize, String> {
    let places: Vec<usize> = text.match_indices(&m.find).map(|(at, _)| at).collect();
    match (m.occurrence, places.len()) {
        (None, 1) => Ok(places[0]),
        (None, 0) => Err("find is not in the file".into()),
        (None, n) => Err(format!("find occurs {n} times: set occurrence")),
        (Some(k), n) => places
            .get(k as usize - 1)
            .copied()
            .ok_or_else(|| format!("occurrence {k}, but find occurs {n} times")),
    }
}

/// `text` with the mutation applied.
pub fn apply(m: &Mutant, text: &str) -> Result<String, String> {
    let at = locate(m, text)?;
    let mut out = String::with_capacity(text.len() + m.replace.len());
    out.push_str(&text[..at]);
    out.push_str(&m.replace);
    out.push_str(&text[at + m.find.len()..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"
# defaults
tests = ["cargo test -p a", 'make b']
timeout = 60

[[mutant]]
id = "A1"
file = "src/lib.rs"
find = 'if x > y {'   # a comment
replace = "if \"q\" {"

[[mutant]]
id = "A2"
file = "src/lib.rs"
find = '''
one
two'''
replace = ''
occurrence = 2
tests = ["only"]
timeout = 5
note = "a note"
"#;

    #[test]
    fn a_list_is_read() {
        let m = parse("t.toml", LIST).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].tests, ["cargo test -p a", "make b"]);
        assert_eq!(m[0].timeout, 60);
        assert_eq!(m[0].find, "if x > y {");
        assert_eq!(m[0].replace, "if \"q\" {");
        assert_eq!(m[1].find, "one\ntwo");
        assert_eq!(m[1].replace, "");
        assert_eq!(m[1].occurrence, Some(2));
        assert_eq!(m[1].tests, ["only"]);
        assert_eq!(m[1].timeout, 5);
        assert_eq!(m[1].note, "a note");
    }

    #[test]
    fn mistakes_are_refused_with_their_line() {
        for (text, wanted) in [
            ("tests = [\"t\"]\n[[mutant]]\nid = \"a\"\n", "needs file"),
            ("[table]\n", "only [[mutant]]"),
            (
                "tests = [\"t\"]\n[[mutant]]\nid = \"a\"\nid = \"b\"\n",
                "given twice",
            ),
            ("x = 1\n", "unknown key x"),
            (
                "[[mutant]]\nid='a'\nfile='f'\nfind='a'\nreplace='b'\n",
                "no tests",
            ),
            (
                "tests = [\"t\"]\n[[mutant]]\nid='a'\nfile='f'\nfind='a'\nreplace='a'\n",
                "differ",
            ),
            ("a = \"open\n", "not closed"),
            ("a = '''\nb\n", "not closed"),
            ("tests = [\"t\" \"u\"]\n", "expected , or ]"),
        ] {
            let e = parse("t.toml", text).unwrap_err();
            assert!(e.contains(wanted), "{text:?}: {e}");
            assert!(e.starts_with("t.toml"), "{e}");
        }
    }

    fn mutant(find: &str, occurrence: Option<u64>) -> Mutant {
        Mutant {
            id: "m".into(),
            file: "f".into(),
            find: find.into(),
            replace: "X".into(),
            occurrence,
            tests: vec!["t".into()],
            timeout: 1,
            note: String::new(),
        }
    }

    #[test]
    fn a_mutation_needs_one_place_or_an_occurrence() {
        assert_eq!(apply(&mutant("b", None), "abc").unwrap(), "aXc");
        assert!(apply(&mutant("z", None), "abc").is_err());
        assert!(
            apply(&mutant("a", None), "aa")
                .unwrap_err()
                .contains("2 times")
        );
        assert_eq!(apply(&mutant("a", Some(2)), "aa").unwrap(), "aX");
        assert!(apply(&mutant("a", Some(3)), "aa").is_err());
    }
}
