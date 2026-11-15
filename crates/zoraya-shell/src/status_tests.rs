//! Host tests of the status row's words and the grant line (ADR-0337):
//! INV-0337-3 (what is given is what the line parses to), INV-0337-5 (every
//! path shown passed the filter) and INV-0337-6 (the three cases).

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::grantline::{Give, GrantLine, Held, Mode, Step, parse_give, words};
use crate::refusal::{
    CANNOT_SHOW, Case, MAX_PENDING, Pending, View, classify, hint_path, notice, program_name,
    shown_path,
};

const HOME: &str = "/home/u";

// ---------------------------------------------------------------- the filter

#[test]
fn plain_paths_are_shown_normalised() {
    assert_eq!(
        shown_path("/home/u/other.txt").as_deref(),
        Some("/home/u/other.txt")
    );
    assert_eq!(shown_path("/a/b/../c/./d").as_deref(), Some("/a/c/d"));
    assert_eq!(shown_path("/a/../..").as_deref(), Some("/"));
    assert_eq!(
        shown_path("/x/a-b_c+d~e@f,g=h.i").as_deref(),
        Some("/x/a-b_c+d~e@f,g=h.i")
    );
}

#[test]
fn hostile_reports_are_not_shown() {
    // INV-0337-5: ESC, a space, U+202E, a newline, `;`, a Hebrew name, a
    // name mixing Latin and Cyrillic letters, and 4096 bytes.
    let long = alloc::format!("/{}", "a".repeat(4095));
    let cases: Vec<String> = vec![
        "/home/u/\x1b[2Jx".to_string(),
        "/home/u/a b".to_string(),
        "/home/u/\u{202e}txt.exe".to_string(),
        "/home/u/a\nb".to_string(),
        "/home/u/a;rm".to_string(),
        "/home/u/\u{5e9}\u{5dc}\u{5d5}\u{5dd}".to_string(),
        "/home/u/p\u{430}ypal".to_string(),
        long,
        "relative/path".to_string(),
        String::new(),
        "/a/.../b".to_string(),
        "/a/'b'".to_string(),
        "/a/$HOME".to_string(),
        "/a/`id`".to_string(),
        "/a/b\0c".to_string(),
    ];
    for c in &cases {
        assert_eq!(shown_path(c), None, "{c:?}");
        assert_eq!(hint_path(c, HOME), CANNOT_SHOW, "{c:?}");
    }
    // A path that fits only just is shown; one byte more is not.
    let fits = alloc::format!("/{}", "a".repeat(crate::refusal::MAX_SHOWN - 1));
    assert!(shown_path(&fits).is_some());
    assert!(shown_path(&alloc::format!("{fits}a")).is_none());
}

#[test]
fn whatever_the_filter_lets_through_is_made_of_allowed_characters() {
    // A deterministic walk over many strings of printable and hostile bytes.
    let alphabet: Vec<char> = "/.a-~ \t\n\x1b;$`'\"\u{202e}\u{430}\u{5d0}\\*?b0"
        .chars()
        .collect();
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    for _ in 0..20_000 {
        let mut s = String::from("/");
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let n = (seed >> 60) as usize + 1;
        for _ in 0..n {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            s.push(alphabet[(seed >> 33) as usize % alphabet.len()]);
        }
        if let Some(p) = shown_path(&s) {
            assert!(
                p.starts_with('/') && p.len() <= crate::refusal::MAX_SHOWN,
                "{s:?}"
            );
            assert!(
                p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "./-_+~@,=".contains(c)),
                "{s:?} -> {p:?}"
            );
            assert!(
                !p.split('/')
                    .any(|c| !c.is_empty() && c.chars().all(|d| d == '.'))
            );
        }
    }
}

#[test]
fn the_hint_prints_the_home_as_a_tilde_and_quotes_nothing_it_does_not_need() {
    assert_eq!(hint_path("/home/u/other.txt", HOME), "~/other.txt");
    assert_eq!(hint_path("/home/u", HOME), "~");
    assert_eq!(hint_path("/etc/x", HOME), "/etc/x");
    assert_eq!(hint_path("/home/u/a b", HOME), CANNOT_SHOW);
}

// ----------------------------------------------------------------- the words

struct World {
    held: Vec<&'static str>,
    present: Vec<&'static str>,
}

impl View for World {
    fn holds(&self, p: &str) -> bool {
        self.held.iter().any(|h| zoraya_resolve::covers(h, p))
    }
    fn exists(&self, p: &str) -> bool {
        self.present.contains(&p)
    }
}

#[test]
fn the_three_cases_are_worded_by_the_shells_own_lookup() {
    // INV-0337-6.
    let w = World {
        held: vec!["/home/u"],
        present: vec!["/home/u/other.txt"],
    };
    let mut p = Pending::new();
    assert!(p.is_empty());
    assert!(p.note("/home/u/other.txt", false, 0));
    let r = p.latest().unwrap().clone();
    let case = classify(&w, r.path.as_deref().unwrap());
    assert_eq!(case, Case::NotGiven);
    assert_eq!(
        notice("vi", &r, case, HOME, 0),
        "vi was not given ~/other.txt (to read)"
    );
    p.note("/home/u/other.txt", true, 0);
    let r = p.latest().unwrap().clone();
    assert_eq!(
        notice("vi", &r, Case::NotGiven, HOME, p.more()),
        "vi was not given ~/other.txt (to change)  [1 more]"
    );
    p.note("/home/u/othr.txt", false, 0);
    let r = p.latest().unwrap().clone();
    let case = classify(&w, r.path.as_deref().unwrap());
    assert_eq!(case, Case::DoesNotExist);
    assert_eq!(
        notice("vi", &r, case, HOME, 0),
        "vi asked for ~/othr.txt, which does not exist"
    );
    p.note("/x", false, 0);
    let r = p.latest().unwrap().clone();
    let case = classify(&w, "/x");
    assert_eq!(case, Case::NotHeld);
    assert_eq!(
        notice("vi", &r, case, HOME, 0),
        "vi asked for /x, which this shell does not hold"
    );
    assert_eq!(p.more(), 3);
    // The program's name is the person's text, shown as printable ASCII only.
    assert_eq!(program_name("v\u{202e}i\x1b"), "v?i?");
}

#[test]
fn a_hostile_report_is_worded_as_a_path_that_cannot_be_shown() {
    let mut p = Pending::new();
    p.note("/home/u/\x1b]0;pwned\x07", false, 1);
    let r = p.latest().unwrap().clone();
    assert_eq!(r.path, None);
    assert_eq!(r.element, 1);
    for case in [Case::NotGiven, Case::DoesNotExist, Case::NotHeld] {
        assert_eq!(
            notice("vi", &r, case, HOME, 0),
            "vi was refused a path that cannot be shown"
        );
    }
}

#[test]
fn refusals_are_counted_per_path_and_access_and_restart_after_a_grant() {
    // ADR-0337 item 1: once per path and access; the limit restarts.
    let mut p = Pending::new();
    assert!(p.note("/a", false, 0));
    assert!(!p.note("/a", false, 0), "the same again is nothing new");
    assert!(p.note("/a", true, 0), "a write is another report");
    assert!(p.note("/a", false, 1), "so is another element's");
    assert_eq!(p.more(), 2);
    for i in 0..(MAX_PENDING + 10) {
        p.note(&alloc::format!("/f{i}"), false, 0);
    }
    assert!(p.more() >= MAX_PENDING, "overflow is still counted");
    p.clear();
    assert!(p.is_empty() && p.latest().is_none() && p.more() == 0);
}

// ------------------------------------------------------------------ the line

struct Holds {
    paths: Vec<(&'static str, bool)>,
    nets: Vec<&'static str>,
}

impl Held for Holds {
    fn path(&self, p: &str, write: bool) -> bool {
        self.paths
            .iter()
            .any(|(h, w)| zoraya_resolve::covers(h, p) && (*w || !write))
    }
    fn net(&self, word: &str) -> bool {
        self.nets.contains(&word)
    }
}

fn holds() -> Holds {
    Holds {
        paths: vec![("/home/u", true), ("/ro", false)],
        nets: vec!["example.org:443"],
    }
}

#[test]
fn a_grant_line_gives_what_its_literal_words_say() {
    // INV-0337-3, through the entry point, as INV-0153-2 and INV-0224-1.
    let g = parse_give(
        "+r ~/other.txt  +w sub +net example.org:443",
        "/home/u/proj",
        HOME,
        &holds(),
    )
    .unwrap();
    assert_eq!(
        g,
        vec![
            Give::Path {
                path: "/home/u/other.txt".into(),
                write: false
            },
            Give::Path {
                path: "/home/u/proj/sub".into(),
                write: true
            },
            Give::Net("example.org:443".into()),
        ]
    );
    assert_eq!(
        words(&g, HOME),
        "+r ~/other.txt, +w ~/proj/sub, +net example.org:443"
    );
    assert_eq!(
        parse_give("+r ../x", "/home/u/proj", HOME, &holds()).unwrap(),
        vec![Give::Path {
            path: "/home/u/x".into(),
            write: false
        }]
    );
}

#[test]
fn a_grant_line_reads_no_variable_no_substitution_and_no_pattern() {
    for bad in [
        "+r $HOME/x",
        "+r $(cat x)",
        "+r `id`",
        "+r ~/*.txt",
        "+r ~/a?",
        "+r ~/[ab]",
        "+r '~/x'",
        "+r \"~/x\"",
        "+r ~/a;+w /",
        "+r ~/a|b",
        "+r ~/a&",
        "+r ~/a>b",
        "+r ~/a\\ b",
        "+r ~/{a,b}",
        "+r ~/a\x1bb",
        "+r ~bob/x",
        "+r +w",
        "+r",
        "+w",
        "",
        "   ",
        "ls /",
        "+x ~/a",
        "r ~/a",
        "+net",
        "+net not a host",
        "+net $H",
        "+r ~/a ~/b",
    ] {
        assert!(
            parse_give(bad, "/home/u", HOME, &holds()).is_err(),
            "{bad:?} must give nothing"
        );
    }
}

#[test]
fn a_grant_line_cannot_give_more_than_the_shell_holds() {
    // INV-0337-3, CP-02: rights only narrow.
    let h = holds();
    for (text, ok) in [
        ("+r /etc/passwd", false),
        ("+w /ro/x", false),
        ("+r /ro/x", true),
        ("+r ~/../../etc", false),
        ("+r /", false),
        ("+w ~/x", true),
        ("+net other.example:443", false),
        ("+net example.org:443", true),
        ("+r ~/ok +r /etc", false), // all or nothing
    ] {
        assert_eq!(
            parse_give(text, "/home/u", HOME, &h).is_ok(),
            ok,
            "{text:?}"
        );
    }
    // A shell that holds nothing new (a room's own terminal) gives nothing.
    let none = Holds {
        paths: vec![],
        nets: vec![],
    };
    assert!(parse_give("+r ~/x", "/home/u", HOME, &none).is_err());
}

fn open(mode: Mode, names: &[&str], from: Option<usize>, now: u64) -> GrantLine {
    GrantLine::open(
        mode,
        names.iter().map(|s| s.to_string()).collect(),
        from,
        now,
    )
}

#[test]
fn enter_within_the_guard_gives_nothing() {
    // INV-0337-2: an Enter at 100 ms is ignored, one at 500 ms is taken.
    let mut g = open(Mode::Give, &["vi"], None, 1_000);
    g.keys(b"+r ~/a", 1_050);
    assert!(g.keys(b"\r", 1_100).is_empty());
    assert!(g.guarded(1_499) && !g.guarded(1_500));
    assert_eq!(g.keys(b"\r", 1_500), vec![Step::Give("+r ~/a".into())]);
    // The kill offer has the same guard.
    let mut k = open(Mode::Kill, &["vi"], None, 0);
    assert!(k.keys(b"\r\n", 499).is_empty());
    assert_eq!(k.keys(b"\r", 500), vec![Step::Kill]);
}

#[test]
fn the_field_is_edited_by_the_person_only() {
    let mut g = open(Mode::Give, &["vi"], None, 0);
    assert_eq!(g.field(), "give vi: ");
    g.keys(b"+r ~/o", 10);
    assert_eq!(g.field(), "give vi: +r ~/o");
    g.keys(b"\x7f\x7f", 10);
    assert_eq!(g.text(), "+r ~");
    // Arrow keys and other escape sequences do nothing; a lone ESC closes.
    assert!(g.keys(b"\x1b[A\x1b[1;5C\x1bOP\x1bx", 10).is_empty());
    assert_eq!(g.text(), "+r ~");
    assert_eq!(g.keys(b"\x1b", 10), vec![Step::Close]);
    // Control characters are not text.
    g.keys(b"\x01\x02\x03\x04\x0b\x0c\x1a", 10);
    assert_eq!(g.text(), "+r ~");
    // Ctrl-U clears; a UTF-8 character is one character.
    g.keys(b"\x15", 10);
    assert_eq!(g.text(), "");
    g.keys("\u{43f}\u{440}".as_bytes(), 10);
    assert_eq!(g.text(), "\u{43f}\u{440}");
}

#[test]
fn a_paste_goes_into_the_field_and_its_newline_is_not_enter() {
    // ADR-0332 item 8 for the grant line.
    let mut g = open(Mode::Give, &["vi"], None, 0);
    let steps = g.paste("+r ~/a\n+w /\n");
    assert_eq!(steps.len(), 1);
    assert!(!steps.iter().any(|s| matches!(s, Step::Give(_))));
    assert_eq!(g.text(), "+r ~/a +w / ");
    // Even a long one stays in bounds.
    g.paste(&"x".repeat(1000));
    assert!(g.text().len() <= crate::grantline::MAX_TEXT);
    // A kill offer takes none.
    let mut k = open(Mode::Kill, &["vi"], None, 0);
    assert!(k.paste("yes\n").is_empty());
}

#[test]
fn a_pipelines_field_names_the_refused_element_and_tab_moves_it() {
    // ADR-0337 item 5.
    let mut g = open(Mode::Give, &["cat", "vi", "wc"], Some(1), 0);
    assert_eq!(g.program(), "vi");
    assert_eq!(g.element(), 1);
    // Tab on the empty field moves; on a typed field it completes instead.
    assert_eq!(g.keys(b"\t", 0), vec![Step::Redraw]);
    assert_eq!(g.program(), "wc");
    g.keys(b"\t\t", 0);
    assert_eq!(g.program(), "vi");
    g.keys(b"+r ~/ot", 0);
    assert_eq!(g.keys(b"\t", 0), vec![Step::Complete("~/ot".into())]);
    assert_eq!(
        g.element(),
        1,
        "the element does not change once text is typed"
    );
    // With no refusal the last element.
    assert_eq!(open(Mode::Give, &["cat", "vi"], None, 0).element(), 1);
    assert_eq!(open(Mode::Give, &["cat", "vi"], Some(9), 0).element(), 1);
}

#[test]
fn completion_extends_to_what_the_candidates_share() {
    let mut g = open(Mode::Give, &["vi"], None, 0);
    g.keys(b"+r ~/ot", 0);
    let c = ["~/other.txt".to_string(), "~/others/".to_string()];
    assert_eq!(g.complete(&c), vec![Step::Redraw]);
    assert_eq!(g.text(), "+r ~/other");
    assert!(g.complete(&c).is_empty(), "nothing more in common");
    assert!(g.complete(&[]).is_empty());
    g.complete(&["~/other.txt".to_string()]);
    assert_eq!(g.text(), "+r ~/other.txt");
}

#[test]
fn the_kill_offer_asks_and_takes_only_enter_and_escape() {
    let mut k = open(Mode::Kill, &["vi"], None, 0);
    assert_eq!(k.field(), "kill vi? Enter kills, Escape keeps");
    assert!(k.keys(b"y\x7f\x15\tq", 9_999).is_empty());
    assert_eq!(k.keys(b"\x1b", 9_999), vec![Step::Close]);
    assert_eq!(k.keys(b"\r", 9_999), vec![Step::Kill]);
}

#[test]
fn the_grant_line_and_its_parser_are_total_on_any_bytes() {
    // INV-0337-1 and INV-0337-3 for hostile input: no panic, the text stays
    // bounded, and what parses is only ever what the shell holds.
    let h = holds();
    let mut seed = 0x0123_4567_89ab_cdefu64;
    let mut next = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };
    for round in 0..3000 {
        let mode = if round % 5 == 0 {
            Mode::Kill
        } else {
            Mode::Give
        };
        let mut g = open(mode, &["a", "b", "c"], Some(next() % 5), 0);
        let mut now = 0u64;
        for _ in 0..40 {
            let n = next() % 12;
            let bytes: Vec<u8> = (0..n).map(|_| next() as u8).collect();
            now += (next() % 300) as u64;
            for step in g.keys(&bytes, now) {
                if let Step::Complete(w) = step {
                    g.complete(&[w.clone(), alloc::format!("{w}x")]);
                }
            }
            assert!(g.text().len() <= crate::grantline::MAX_TEXT);
            assert!(g.element() < 3);
            assert!(!g.field().contains('\n'));
            let text: String = g.text().into();
            if let Ok(gives) = parse_give(&text, "/home/u", HOME, &h) {
                for give in gives {
                    match give {
                        Give::Path { path, write } => assert!(h.path(&path, write), "{text:?}"),
                        Give::Net(word) => assert!(h.net(&word)),
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- delivery

mod delivery_tests {
    use crate::delivery::*;
    use alloc::string::ToString;
    use alloc::vec;

    fn vi_notes() -> Held {
        // `vi ~/notes.txt`: the home narrowed to one name, and the system's
        // directories read whole.
        Held {
            wholes: vec![("/etc".into(), READ), ("/usr".into(), READ)],
            names: vec![("/home/u".into(), "notes.txt".into(), CHANGE)],
        }
    }

    fn file(path: &str, write: bool) -> Wanted {
        Wanted {
            path: path.into(),
            dir: false,
            write,
        }
    }

    #[test]
    fn a_second_file_of_a_directory_is_a_list_of_both() {
        let mut h = vi_notes();
        let specs = h.give(&[file("/home/u/other.txt", false)]).unwrap();
        assert_eq!(
            specs,
            vec![Spec::Names {
                prefix: "/home/u".into(),
                names: vec![
                    ("notes.txt".to_string(), CHANGE),
                    ("other.txt".to_string(), READ)
                ],
                rights: CHANGE | READ,
            }]
        );
        // Changing it later adds to that name's rights only.
        let specs = h.give(&[file("/home/u/other.txt", true)]).unwrap();
        let Spec::Names { names, .. } = &specs[0] else {
            panic!()
        };
        assert_eq!(names[1], ("other.txt".to_string(), READ | CHANGE));
        assert_eq!(names[0], ("notes.txt".to_string(), CHANGE));
        // Nothing to give twice.
        assert!(h.give(&[file("/home/u/other.txt", true)]).is_err());
    }

    #[test]
    fn a_directory_is_whole_and_takes_the_names_given_in_it() {
        let mut h = vi_notes();
        let specs = h
            .give(&[Wanted {
                path: "/home/u".into(),
                dir: true,
                write: false,
            }])
            .unwrap();
        assert_eq!(
            specs,
            vec![Spec::Whole {
                prefix: "/home/u".into(),
                rights: CHANGE | READ
            }]
        );
        assert!(h.names.is_empty());
        assert!(h.wholes.contains(&("/home/u".to_string(), CHANGE | READ)));
    }

    #[test]
    fn a_grant_never_shadows_or_replaces_what_the_program_holds_whole() {
        let mut h = vi_notes();
        let before = h.clone();
        // Below a directory it holds whole, and a system directory itself.
        for w in [
            file("/usr/lib/x", false),
            file("/etc/passwd", true),
            Wanted {
                path: "/usr/lib".into(),
                dir: true,
                write: true,
            },
            Wanted {
                path: "/etc".into(),
                dir: true,
                write: false,
            },
        ] {
            assert!(h.give(&[w.clone()]).is_err(), "{w:?}");
            assert_eq!(h, before, "nothing changed");
        }
        // All or nothing.
        assert!(
            h.give(&[file("/home/u/ok.txt", false), file("/usr/x", false)])
                .is_err()
        );
        assert_eq!(h, before);
        // Rights of a directory held whole grow only at that directory.
        let mut g = Held {
            wholes: vec![("/w".into(), READ)],
            names: vec![],
        };
        let specs = g
            .give(&[Wanted {
                path: "/w".into(),
                dir: true,
                write: true,
            }])
            .unwrap();
        assert_eq!(
            specs,
            vec![Spec::Whole {
                prefix: "/w".into(),
                rights: TREE | READ
            }]
        );
    }

    #[test]
    fn the_list_is_bounded() {
        let mut h = Held::default();
        for i in 0..zoraya_proto_file::narrow_names::MAX_NAMES {
            h.give(&[file(&alloc::format!("/d/f{i}"), false)]).unwrap();
        }
        assert!(h.give(&[file("/d/more", false)]).is_err());
        // Another directory is another list.
        assert!(h.give(&[file("/e/more", false)]).is_ok());
    }
}
