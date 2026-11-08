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
/// Most directory listings one pattern may ask (review S14): each is a
/// round trip to a file service.
pub const MAX_LISTS: usize = 10_000;

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
    lists: usize,
}

impl<L: Lister> Walk<'_, L> {
    /// A listing, counted against [`MAX_LISTS`].
    fn list(&mut self, path: &str) -> Option<Vec<(String, bool)>> {
        if self.lists >= MAX_LISTS {
            self.full = true;
            return None;
        }
        self.lists += 1;
        self.lister.list(path)
    }

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
            // `**/**` is `**` (review S14).
            if let Some(Component::Any) = after.first() {
                self.walk(path, after, depth);
                return;
            }
            // Zero directories, then each directory one deeper.
            self.walk(path, after, depth + 1);
            let Some(entries) = self.list(path) else {
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
            // `.` and `..` are steps, which no listing holds (review S13).
            if name == "." || name == ".." {
                self.walk(&join(path, &name), after, depth + 1);
                return;
            }
            // A literal component is checked against the listing too, so
            // that only what exists is given.
            let Some(entries) = self.list(path) else {
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
        let Some(entries) = self.list(path) else {
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
        lists: 0,
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;
    use alloc::format;
    use alloc::string::String;
    use alloc::vec;

    /// A tree, listed by normal path; counts its listings.
    struct Tree {
        dirs: BTreeMap<String, Vec<(String, bool)>>,
        lists: usize,
    }

    impl Lister for Tree {
        fn list(&mut self, dir: &str) -> Option<Vec<(String, bool)>> {
            self.lists += 1;
            let key = zoraya_resolve::join("/w", 0, dir);
            self.dirs.get(&key).cloned()
        }
    }

    fn tree() -> Tree {
        let mut dirs = BTreeMap::new();
        dirs.insert(
            "/".into(),
            vec![("w".into(), true), ("top.txt".into(), false)],
        );
        dirs.insert(
            "/w".into(),
            vec![("a.txt".into(), false), ("d".into(), true)],
        );
        dirs.insert("/w/d".into(), vec![("e".into(), true)]);
        dirs.insert("/w/d/e".into(), vec![("f.txt".into(), false)]);
        Tree { dirs, lists: 0 }
    }

    fn chars(s: &str) -> Vec<(char, bool)> {
        s.chars().map(|c| (c, true)).collect()
    }

    /// Review S13: `.` and `..` are steps, as written.
    #[test]
    fn dot_and_dot_dot_are_steps() {
        let mut t = tree();
        let p = pattern(&chars("../*.txt")).expect("a pattern");
        assert_eq!(expand(&p, &mut t), ["../top.txt"]);
        let p = pattern(&chars("./*.txt")).expect("a pattern");
        assert_eq!(expand(&p, &mut t), ["./a.txt"]);
        let p = pattern(&chars("d/../*.txt")).expect("a pattern");
        assert_eq!(expand(&p, &mut t), ["d/../a.txt"]);
    }

    /// `n` directories `d` in a chain below `/w`, with `a.txt` at the
    /// bottom.
    fn chain(n: usize) -> Tree {
        let mut dirs = BTreeMap::new();
        let mut at = String::from("/w");
        for _ in 0..n {
            dirs.insert(at.clone(), vec![("d".into(), true)]);
            at.push_str("/d");
        }
        dirs.insert(at, vec![("a.txt".into(), false)]);
        Tree { dirs, lists: 0 }
    }

    fn found(text: &str, t: &mut Tree) -> usize {
        expand(&pattern(&chars(text)).expect("a pattern"), t).len()
    }

    /// Mutation testing: every kind of step counts toward `MAX_DEPTH`, the
    /// deepest match is found and one level more is not.
    #[test]
    fn every_step_counts_toward_the_depth_bound() {
        let deepest = (0..40)
            .take_while(|n| found("**/a.txt", &mut chain(*n)) == 1)
            .count();
        assert_eq!(
            deepest,
            MAX_DEPTH - 1,
            "a.txt below MAX_DEPTH - 2 directories"
        );
        for n in [MAX_DEPTH - 1, MAX_DEPTH] {
            let literal = format!("{}*.txt", "d/".repeat(n));
            let any = format!("{}a.txt", "*/".repeat(n));
            let expect = usize::from(n < MAX_DEPTH);
            assert_eq!(found(&literal, &mut chain(n)), expect, "{literal}");
            assert_eq!(found(&any, &mut chain(n)), expect, "{any}");
            let dots = format!("{}*.txt", "./".repeat(n));
            assert_eq!(found(&dots, &mut chain(0)), expect, "{dots}");
        }
    }

    /// Mutation testing: `**` skips hidden directories, and a literal step
    /// is followed only to an entry of its name and kind.
    #[test]
    fn literal_steps_and_hidden_directories() {
        let mut t = tree();
        t.dirs.insert(
            "/w".into(),
            vec![
                ("-n".into(), false),
                (".hid".into(), true),
                ("a.txt".into(), false),
                ("sub".into(), true),
            ],
        );
        t.dirs
            .insert("/w/.hid".into(), vec![("a.txt".into(), false)]);
        t.dirs
            .insert("/w/sub".into(), vec![("c.txt".into(), false)]);
        let mut e = |p: &str| expand(&pattern(&chars(p)).expect("a pattern"), &mut t);
        assert_eq!(e("**/a.txt"), ["a.txt"]);
        assert_eq!(e("sub/*.txt"), ["sub/c.txt"]);
        assert_eq!(e("*/c.txt"), ["sub/c.txt"]);
        assert!(e("a.txt/*").is_empty(), "a file is no step");
    }

    /// Review S14: repeated `**` is one, and the listings are bounded.
    #[test]
    fn repeated_any_is_one_and_listings_are_bounded() {
        let mut t = tree();
        let p = pattern(&chars("**/*.txt")).expect("a pattern");
        let once = expand(&p, &mut t);
        let lists = t.lists;
        let mut t = tree();
        let p = pattern(&chars("**/**/**/**/**/**/*.txt")).expect("a pattern");
        assert_eq!(expand(&p, &mut t), once);
        assert_eq!(t.lists, lists, "no more listings than one **");
        // A tree wider than the budget stops at it.
        let mut wide = Tree {
            dirs: BTreeMap::new(),
            lists: 0,
        };
        let names: Vec<(String, bool)> = (0..200).map(|i| (alloc::format!("d{i}"), true)).collect();
        wide.dirs.insert("/w".into(), names.clone());
        for i in 0..200 {
            wide.dirs.insert(alloc::format!("/w/d{i}"), names.clone());
        }
        let p = pattern(&chars("**/nomatch")).expect("a pattern");
        let _ = expand(&p, &mut wide);
        assert!(wide.lists <= MAX_LISTS, "{}", wide.lists);
    }
}
