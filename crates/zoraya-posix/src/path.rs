//! The resolver (ADR-0107 items 6 and 7, ADR-0153 rule 6): a path to an
//! entry of the namespace table and the relative path within it, lexically.
//! Nothing here talks to a service, so all of it is tested on the host.

#![forbid(unsafe_code)]

use alloc::string::String;
use alloc::vec::Vec;

use crate::errno::{EILSEQ, ELOOP, ENAMETOOLONG, ENOENT};

/// The longest path, with its NUL (POSIX `PATH_MAX`, musl's 4096).
pub const PATH_MAX: usize = 4096;
/// Links followed in one resolution before `ELOOP` (ADR-0107 item 7).
pub const MAX_LINKS: u32 = 40;

/// How `..` and relative paths behave (`zoraya_abi::posix::ABSOLUTE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// ADR-0107 item 6, INV-0002-4: a relative path is resolved in cwd's
    /// entry, and `..` at that entry's root stays there.
    EntryRelative,
    /// ADR-0153 rule 6, INV-0153-8: a relative path is joined to cwd's
    /// absolute path, and `..` stops only at `/`.
    Absolute,
}

/// A place in the table: an entry and a normal relative path in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub entry: usize,
    pub path: String,
}

/// Where a path led.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub place: Place,
    /// The absolute normal path, for `getcwd`, reports and links.
    pub absolute: String,
    /// The path ended in `/`: it must name a directory.
    pub directory: bool,
}

/// Why a path led nowhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No entry covers the absolute path: `ENOENT`, and a report (ADR-0153
    /// rule 11).
    Outside(String),
    /// Any other error, as an `errno`.
    Error(i32),
}

impl Refusal {
    pub fn errno(&self) -> i32 {
        match self {
            Refusal::Outside(_) => ENOENT,
            Refusal::Error(e) => *e,
        }
    }
}

/// A path as a program names it: bytes, without the NUL.
pub fn check(path: &[u8]) -> Result<&str, i32> {
    if path.is_empty() {
        return Err(ENOENT);
    }
    if path.len() >= PATH_MAX {
        return Err(ENAMETOOLONG);
    }
    // ADR-0013 resolves X-8 so: a name that is not UTF-8 is `EILSEQ`.
    core::str::from_utf8(path).map_err(|_| EILSEQ)
}

/// The components of an absolute normal path.
fn components(absolute: &str) -> impl Iterator<Item = &str> {
    absolute.split('/').filter(|c| !c.is_empty())
}

/// `path` applied to the absolute normal `base`: `.` and empty components
/// dropped, `..` removing the component before it, but none of the first
/// `floor` components of `base`.
pub fn join(base: &str, floor: usize, path: &str) -> String {
    let mut parts: Vec<&str> = if path.starts_with('/') {
        Vec::new()
    } else {
        components(base).collect()
    };
    let floor = if path.starts_with('/') {
        0
    } else {
        floor.min(parts.len())
    };
    for c in path.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                if parts.len() > floor {
                    parts.pop();
                }
            }
            name => parts.push(name),
        }
    }
    let mut out = String::with_capacity(path.len() + base.len() + 1);
    if parts.is_empty() {
        out.push('/');
    }
    for p in parts {
        out.push('/');
        out.push_str(p);
    }
    out
}

/// The absolute path of `place`, under its entry's `prefix`.
pub fn absolute_of(prefix: &str, path: &str) -> String {
    let mut out = String::from(prefix);
    if !path.is_empty() {
        if !out.ends_with('/') {
            out.push('/');
        }
        out.push_str(path);
    }
    out
}

/// True if `prefix` covers the absolute normal `path`, on a component
/// boundary.
pub fn covers(prefix: &str, path: &str) -> bool {
    prefix == "/"
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The entry with the longest prefix that covers the absolute normal `path`,
/// and the relative path after it.
pub fn longest<'p, 'a>(
    prefixes: impl Iterator<Item = &'p str>,
    path: &'a str,
) -> Option<(usize, &'a str)> {
    let mut best: Option<(usize, usize)> = None;
    for (i, prefix) in prefixes.enumerate() {
        if covers(prefix, path) && best.is_none_or(|(_, length)| prefix.len() > length) {
            best = Some((i, prefix.len()));
        }
    }
    let (i, length) = best?;
    let rest = path[length..].trim_start_matches('/');
    Some((i, rest))
}

/// Resolves the absolute normal `path` against the table.
pub fn place<'p>(
    prefixes: impl Iterator<Item = &'p str>,
    path: String,
    directory: bool,
) -> Result<Resolved, Refusal> {
    match longest(prefixes, &path) {
        Some((entry, rest)) => Ok(Resolved {
            place: Place {
                entry,
                path: String::from(rest),
            },
            absolute: path,
            directory,
        }),
        None => Err(Refusal::Outside(path)),
    }
}

/// Resolves `path`, named from `from` (cwd or a directory fd): `prefixes` are
/// the table's, and `from.entry` indexes them.
pub fn resolve(
    prefixes: &[&str],
    form: Form,
    from: &Place,
    path: &[u8],
) -> Result<Resolved, Refusal> {
    let text = check(path).map_err(Refusal::Error)?;
    let prefix = prefixes.get(from.entry).copied().unwrap_or("/");
    let base = absolute_of(prefix, &from.path);
    let floor = match form {
        Form::EntryRelative => components(prefix).count(),
        Form::Absolute => 0,
    };
    let joined = join(&base, floor, text);
    let directory = text.ends_with('/') || text.ends_with("/.") || text.ends_with("/..");
    place(prefixes.iter().copied(), joined, directory)
}

/// Where resolution goes on after the service met a link (ADR-0107 item 7).
/// `at` is the place that was opened, `remaining` the part of its path after
/// the link and `target` the link's contents. A relative target is read from
/// the link's directory, with `..` held as `form` says; an absolute one from
/// the table.
pub fn follow(
    prefixes: &[&str],
    form: Form,
    at: &Place,
    remaining: &str,
    target: &str,
    directory: bool,
) -> Result<Resolved, Refusal> {
    if target.is_empty() {
        return Err(Refusal::Error(ENOENT));
    }
    // The link is the component before `remaining`.
    let consumed = at
        .path
        .strip_suffix(remaining)
        .unwrap_or(&at.path)
        .trim_end_matches('/');
    let link_directory = match consumed.rfind('/') {
        Some(cut) => &consumed[..cut],
        None => "",
    };
    let prefix = prefixes.get(at.entry).copied().unwrap_or("/");
    let base = absolute_of(prefix, link_directory);
    let floor = match form {
        Form::EntryRelative => components(prefix).count(),
        Form::Absolute => 0,
    };
    let mut path = String::from(target);
    if !remaining.is_empty() {
        path.push('/');
        path.push_str(remaining);
    }
    if path.len() >= PATH_MAX {
        return Err(Refusal::Error(ENAMETOOLONG));
    }
    let joined = join(&base, floor, &path);
    place(prefixes.iter().copied(), joined, directory)
}

/// Counts links in one resolution: `ELOOP` past `MAX_LINKS`.
#[derive(Debug, Default)]
pub struct Links(u32);

impl Links {
    pub fn take(&mut self) -> Result<(), i32> {
        self.0 += 1;
        if self.0 > MAX_LINKS {
            Err(ELOOP)
        } else {
            Ok(())
        }
    }
}

/// The parent of a place and the last name in it, for the requests that
/// name one component (`make_directory`, `remove`, `rename`, `make_link`).
/// `None` for an entry's root, which has no name in it.
pub fn split(place: &Place) -> Option<(Place, &str)> {
    if place.path.is_empty() {
        return None;
    }
    let (parent, name) = match place.path.rfind('/') {
        Some(cut) => (&place.path[..cut], &place.path[cut + 1..]),
        None => ("", place.path.as_str()),
    };
    Some((
        Place {
            entry: place.entry,
            path: String::from(parent),
        },
        name,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;

    const TABLE: [&str; 5] = ["/work/src/deep", "/work", "/tmp", "/dev/null", "/"];

    fn at(entry: usize, path: &str) -> Place {
        Place {
            entry,
            path: path.to_string(),
        }
    }

    fn go(form: Form, from: &Place, path: &str) -> Result<(usize, String), Refusal> {
        resolve(&TABLE, form, from, path.as_bytes()).map(|r| (r.place.entry, r.place.path))
    }

    #[test]
    fn absolute_paths_take_the_longest_prefix_on_a_boundary() {
        let cwd = at(4, "");
        for form in [Form::EntryRelative, Form::Absolute] {
            let ok = |p: &str, e: usize, rest: &str| {
                assert_eq!(go(form, &cwd, p), Ok((e, rest.to_string())), "{p}")
            };
            ok("/work/a", 1, "a");
            ok("/work", 1, "");
            ok("/work/", 1, "");
            ok("/workshop/a", 4, "workshop/a");
            ok("/work/src/deep/x/y", 0, "x/y");
            ok("/work/src/deeper", 1, "src/deeper");
            ok("/tmp/GMfifo7", 2, "GMfifo7");
            ok("/dev/null", 3, "");
            ok("/dev/nullx", 4, "dev/nullx");
            ok("//work///a/./b/", 1, "a/b");
            ok("/../../work/a", 1, "a");
            ok("/work/../tmp/x", 2, "x");
            ok("/Київ/Plzeň", 4, "Київ/Plzeň");
        }
    }

    #[test]
    fn an_absolute_path_no_entry_covers_is_refused_as_outside() {
        let table = ["/work", "/bin"];
        let cwd = at(0, "");
        let r = resolve(&table, Form::EntryRelative, &cwd, b"/etc/passwd");
        assert_eq!(r, Err(Refusal::Outside("/etc/passwd".to_string())));
        assert_eq!(r.unwrap_err().errno(), ENOENT);
        let r = resolve(&table, Form::EntryRelative, &cwd, b"/workshop");
        assert_eq!(r, Err(Refusal::Outside("/workshop".to_string())));
        let r = resolve(&table, Form::Absolute, &at(0, "a"), b"../../etc/./x/");
        assert_eq!(r, Err(Refusal::Outside("/etc/x".to_string())));
    }

    #[test]
    fn relative_paths_and_dot_dot_in_each_form() {
        let cwd = at(1, "a/b");
        // Entry-relative: `..` stops at the root of cwd's entry, /work.
        let er = Form::EntryRelative;
        assert_eq!(go(er, &cwd, "c"), Ok((1, "a/b/c".into())));
        assert_eq!(go(er, &cwd, ".."), Ok((1, "a".into())));
        assert_eq!(go(er, &cwd, "../../../../etc"), Ok((1, "etc".into())));
        assert_eq!(go(er, &cwd, "../../src/deep/z"), Ok((0, "z".into())));
        assert_eq!(go(er, &at(2, ""), "../work/x"), Ok((2, "work/x".into())));
        // Absolute: `..` leaves the entry, and only `/` holds it.
        let ab = Form::Absolute;
        assert_eq!(go(ab, &cwd, "../../../../etc"), Ok((4, "etc".into())));
        assert_eq!(go(ab, &at(2, ""), "../work/x"), Ok((1, "x".into())));
        assert_eq!(go(ab, &cwd, "../../../tmp"), Ok((2, "".into())));
    }

    #[test]
    fn bad_paths_are_refused_with_their_errno() {
        let cwd = at(4, "");
        let e = |p: &[u8]| {
            resolve(&TABLE, Form::EntryRelative, &cwd, p)
                .unwrap_err()
                .errno()
        };
        assert_eq!(e(b""), ENOENT);
        assert_eq!(e(b"/a/\xff"), EILSEQ);
        assert_eq!(e(&[b'a'; PATH_MAX]), ENAMETOOLONG);
        assert!(resolve(&TABLE, Form::EntryRelative, &cwd, &[b'a'; PATH_MAX - 1]).is_ok());
        let r = resolve(&TABLE, Form::EntryRelative, &cwd, b"x/").unwrap();
        assert!(r.directory, "a trailing slash names a directory");
        let r = resolve(&TABLE, Form::EntryRelative, &cwd, b"x/..").unwrap();
        assert!(r.directory);
        assert!(
            !resolve(&TABLE, Form::EntryRelative, &cwd, b"x")
                .unwrap()
                .directory
        );
        // Control characters are names for the service to judge, not here.
        let r = resolve(&TABLE, Form::EntryRelative, &cwd, b"a\x01b").unwrap();
        assert_eq!(r.place.path, "a\u{1}b");
    }

    #[test]
    fn links_resolve_from_their_directory_or_the_table() {
        let er = Form::EntryRelative;
        // /work/a/b/link/c with link -> ../x: /work/a/x/c.
        let opened = at(1, "a/b/link/c");
        let r = follow(&TABLE, er, &opened, "c", "../x", false).unwrap();
        assert_eq!((r.place.entry, r.place.path.as_str()), (1, "a/x/c"));
        // A relative target that climbs past the entry stays in it...
        let r = follow(&TABLE, er, &opened, "c", "../../../../../etc", false).unwrap();
        assert_eq!((r.place.entry, r.place.path.as_str()), (1, "etc/c"));
        // ...unless the table is absolute.
        let r = follow(
            &TABLE,
            Form::Absolute,
            &opened,
            "c",
            "../../../../../etc",
            false,
        );
        assert_eq!(r.unwrap().absolute, "/etc/c");
        // An absolute target is read from the table.
        let r = follow(&TABLE, er, &opened, "c", "/tmp/t", false).unwrap();
        assert_eq!((r.place.entry, r.place.path.as_str()), (2, "t/c"));
        // A link as the last component.
        let r = follow(&TABLE, er, &at(1, "l"), "", "f", false).unwrap();
        assert_eq!(r.place.path, "f");
        let r = follow(&TABLE, er, &at(1, "d/l"), "", "f", true).unwrap();
        assert_eq!((r.place.path.as_str(), r.directory), ("d/f", true));
        assert_eq!(
            follow(&TABLE, er, &opened, "c", "", false),
            Err(Refusal::Error(ENOENT))
        );
        let long = "a/".repeat(PATH_MAX / 2);
        assert_eq!(
            follow(&TABLE, er, &opened, "c", &long, false),
            Err(Refusal::Error(ENAMETOOLONG))
        );
        let mut links = Links::default();
        for _ in 0..MAX_LINKS {
            assert_eq!(links.take(), Ok(()));
        }
        assert_eq!(links.take(), Err(ELOOP));
    }

    #[test]
    fn split_gives_the_parent_and_the_name() {
        let abc = at(1, "a/b/c");
        let (p, n) = split(&abc).unwrap();
        assert_eq!((p, n), (at(1, "a/b"), "c"));
        let c = at(1, "c");
        let (p, n) = split(&c).unwrap();
        assert_eq!((p, n), (at(1, ""), "c"));
        assert!(split(&at(1, "")).is_none());
        assert_eq!(absolute_of("/", ""), "/");
        assert_eq!(absolute_of("/", "a"), "/a");
        assert_eq!(absolute_of("/w", ""), "/w");
        assert_eq!(absolute_of("/w", "a/b"), "/w/a/b");
    }

    /// A component generator for the property tests: names, `.`, `..`,
    /// empty components and names that look like prefixes.
    fn random_path(next: &mut impl FnMut(u64) -> u64) -> String {
        const PARTS: [&str; 10] = [
            "..", ".", "", "work", "src", "deep", "tmp", "a", "dev", "null",
        ];
        let mut out = String::new();
        if next(2) == 0 {
            out.push('/');
        }
        for i in 0..next(8) {
            if i > 0 {
                out.push('/');
            }
            out.push_str(PARTS[next(PARTS.len() as u64) as usize]);
        }
        if out.is_empty() {
            out.push('.');
        }
        out
    }

    /// INV-0107-2, INV-0002-4 and INV-0153-8, in both forms: whatever a path
    /// is, the entry chosen is the longest prefix covering the absolute
    /// result, the rest has no `.`, `..` or empty component, the result is
    /// `join`'s normal form, and in the entry-relative form a relative path
    /// never leaves cwd's entry.
    #[test]
    fn no_path_escapes_an_entry_in_either_form() {
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let mut next = move |n: u64| {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            x.wrapping_mul(0x2545_f491_4f6c_dd1d) % n
        };
        let tables: [&[&str]; 3] = [&TABLE, &["/work", "/tmp"], &["/"]];
        for _ in 0..20_000 {
            let table = tables[next(3) as usize];
            let form = if next(2) == 0 {
                Form::EntryRelative
            } else {
                Form::Absolute
            };
            let from = at(
                next(table.len() as u64) as usize,
                ["", "a", "a/b"][next(3) as usize],
            );
            let path = random_path(&mut next);
            match resolve(table, form, &from, path.as_bytes()) {
                Ok(r) => {
                    let rest = &r.place.path;
                    assert!(
                        rest.is_empty()
                            || rest
                                .split('/')
                                .all(|c| !c.is_empty() && c != "." && c != ".."),
                        "{path} gave {rest}"
                    );
                    assert_eq!(absolute_of(table[r.place.entry], rest), r.absolute);
                    assert!(zoraya_abi::posix::is_absolute_normal(&r.absolute));
                    let best = table
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| covers(p, &r.absolute))
                        .max_by_key(|(_, p)| p.len())
                        .map(|(i, _)| i);
                    assert_eq!(best, Some(r.place.entry), "longest for {}", r.absolute);
                    if form == Form::EntryRelative && !path.starts_with('/') {
                        let own = table[from.entry];
                        assert!(
                            covers(own, &r.absolute),
                            "{path} from {own} left it: {}",
                            r.absolute
                        );
                    }
                }
                Err(Refusal::Outside(abs)) => {
                    assert!(table.iter().all(|p| !covers(p, &abs)), "{abs}");
                    assert!(zoraya_abi::posix::is_absolute_normal(&abs));
                    assert!(
                        form == Form::Absolute || path.starts_with('/'),
                        "entry-relative {path} from inside"
                    );
                }
                Err(e) => panic!("{path}: {e:?}"),
            }
        }
    }

    /// `..` at an entry's root resolves to that root (INV-0002-4's words),
    /// for every entry and depth.
    #[test]
    fn dot_dot_at_an_entry_root_stays_there() {
        for (i, prefix) in TABLE.iter().enumerate() {
            for depth in 1..5 {
                let path = "../".repeat(depth);
                let r = resolve(&TABLE, Form::EntryRelative, &at(i, ""), path.as_bytes()).unwrap();
                assert_eq!(r.absolute, *prefix, "{} from {prefix}", path);
                let r = resolve(
                    &TABLE,
                    Form::EntryRelative,
                    &at(i, ""),
                    format!("{path}x").as_bytes(),
                )
                .unwrap();
                assert_eq!(r.place.entry, i);
            }
        }
    }
}
