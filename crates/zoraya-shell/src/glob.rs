//! Patterns (ADR-0150 rule 7): `*` any characters but `/`, `?` one
//! character, `**` as a whole component any depth of directories. Only
//! characters from unquoted source text are active. Matches are sorted byte
//! by byte; a component matches a name starting with `.` only if the
//! component itself starts with a literal `.`; a match that would start with
//! `-` is given as `./-name` (DM-095, INV-0150-2).

use alloc::string::String;
use alloc::vec::Vec;

/// Most matches one pattern may give.
pub const MAX_MATCHES: usize = 10_000;
/// Deepest `**` descends.
pub const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Char(char),
    Star,
    Quest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Component {
    Tokens(Vec<Token>),
    /// `**`
    Any,
}

impl Component {
    fn is_literal(&self) -> Option<String> {
        match self {
            Component::Any => None,
            Component::Tokens(t) => t
                .iter()
                .map(|t| match t {
                    Token::Char(c) => Some(*c),
                    _ => None,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pub absolute: bool,
    pub components: Vec<Component>,
}

/// Characters of a word with whether each is active; `None` if no
/// character is an active `*` or `?`, so the word is no pattern.
pub fn pattern(chars: &[(char, bool)]) -> Option<Pattern> {
    if !chars
        .iter()
        .any(|(c, active)| *active && (*c == '*' || *c == '?'))
    {
        return None;
    }
    let absolute = chars.first().is_some_and(|(c, _)| *c == '/');
    let mut components = Vec::new();
    for piece in chars.split(|(c, _)| *c == '/') {
        if piece.is_empty() {
            continue; // `//` or the leading `/`
        }
        if piece.len() == 2 && piece.iter().all(|(c, a)| *a && *c == '*') {
            components.push(Component::Any);
            continue;
        }
        let tokens = piece
            .iter()
            .map(|(c, active)| match (c, active) {
                ('*', true) => Token::Star,
                ('?', true) => Token::Quest,
                (c, _) => Token::Char(*c),
            })
            .collect();
        components.push(Component::Tokens(tokens));
    }
    Some(Pattern {
        absolute,
        components,
    })
}

/// Whether `name` matches `tokens`. Linear in both, without recursion.
pub fn matches(tokens: &[Token], name: &str) -> bool {
    let hidden_ok = matches!(tokens.first(), Some(Token::Char('.')));
    if name.starts_with('.') && !hidden_ok {
        return false;
    }
    let name: Vec<char> = name.chars().collect();
    let (mut t, mut n) = (0, 0);
    let mut back: Option<(usize, usize)> = None;
    while n < name.len() {
        match tokens.get(t) {
            Some(Token::Star) => {
                back = Some((t, n));
                t += 1;
            }
            Some(Token::Quest) => {
                t += 1;
                n += 1;
            }
            Some(Token::Char(c)) if *c == name[n] => {
                t += 1;
                n += 1;
            }
            _ => match back {
                Some((bt, bn)) => {
                    t = bt + 1;
                    n = bn + 1;
                    back = Some((bt, bn + 1));
                }
                None => return false,
            },
        }
    }
    tokens[t.min(tokens.len())..]
        .iter()
        .all(|t| *t == Token::Star)
}

/// Lists a directory for matching: `(name, is directory)`. `dir` is `""` for
/// the current directory, else a path relative to it, or absolute.
pub trait Lister {
    fn list(&mut self, dir: &str) -> Option<Vec<(String, bool)>>;
}

fn join(base: &str, name: &str) -> String {
    let mut s = String::from(base);
    if !s.is_empty() && !s.ends_with('/') {
        s.push('/');
    }
    s.push_str(name);
    s
}

struct Walk<'a, L: Lister> {
    lister: &'a mut L,
    out: Vec<String>,
    full: bool,
}

impl<L: Lister> Walk<'_, L> {
    /// `path` is a directory (or the start); match `rest` below it.
    fn walk(&mut self, path: &str, rest: &[Component], depth: usize) {
        if self.full || depth > MAX_DEPTH {
            return;
        }
        let Some((first, after)) = rest.split_first() else {
            self.push(path);
            return;
        };
        if let Component::Any = first {
            // Zero directories, then each directory one deeper.
            self.walk(path, after, depth + 1);
            let Some(entries) = self.lister.list(path) else {
                return;
            };
            for (name, dir) in entries {
                if dir && !name.starts_with('.') {
                    self.walk(&join(path, &name), rest, depth + 1);
                }
            }
            return;
        }
        if let Some(name) = first.is_literal() {
            // A literal component is checked against the listing too, so
            // that only what exists is given.
            let Some(entries) = self.lister.list(path) else {
                return;
            };
            if let Some((_, dir)) = entries.iter().find(|(n, _)| *n == name)
                && (after.is_empty() || *dir)
            {
                self.walk(&join(path, &name), after, depth + 1);
            }
            return;
        }
        let Component::Tokens(tokens) = first else {
            return;
        };
        let Some(entries) = self.lister.list(path) else {
            return;
        };
        for (name, dir) in entries {
            if (after.is_empty() || dir) && matches(tokens, &name) {
                self.walk(&join(path, &name), after, depth + 1);
            }
        }
    }

    fn push(&mut self, path: &str) {
        if self.out.len() >= MAX_MATCHES {
            self.full = true;
            return;
        }
        self.out.push(path.into());
    }
}

/// Every name the pattern matches, sorted byte by byte, without
/// duplicates, each safe from being read as an option.
pub fn expand<L: Lister>(p: &Pattern, lister: &mut L) -> Vec<String> {
    let start = if p.absolute { "/" } else { "" };
    let mut w = Walk {
        lister,
        out: Vec::new(),
        full: false,
    };
    w.walk(start, &p.components, 0);
    let mut out = w.out;
    out.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    out.dedup();
    for name in &mut out {
        if name.starts_with('-') {
            name.insert_str(0, "./");
        }
    }
    out
}
