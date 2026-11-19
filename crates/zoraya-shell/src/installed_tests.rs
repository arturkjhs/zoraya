//! Host tests of what the shell does for an installed program (ADR-0396,
//! ADR-0397, ADR-0399): INV-0396-1, -4, -5, INV-0397-2, -3, -5.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_command::{Outcome, Parsed};
use zoraya_settings::apps::{self, App, Apps, Word};
use zoraya_settings::sha256::{self, identity};

use crate::eval::Value;
use crate::grants::{Access, Line, Planned, plan};
use crate::installed::*;

const UNDESCRIBED: &str =
    "name undescribed\nsummary a program the person built\nusage undescribed\ncwd read\n";

fn planned(grants: &[(bool, &str)], cwd: &str) -> Vec<Planned> {
    let d = zoraya_describe::parse(UNDESCRIBED.as_bytes()).unwrap();
    let parsed: Parsed<'_> = match zoraya_command::parse(&d, &[]) {
        Ok(Outcome::Run(p)) => p,
        other => panic!("{other:?}"),
    };
    let g: Vec<(bool, Value)> = grants
        .iter()
        .map(|(w, p)| (*w, Value::literal(p)))
        .collect();
    plan(&Line {
        description: &d,
        parsed: &parsed,
        args: &[],
        cwd,
        grants: &g,
        nets: &[],
        listens: &[],
        base_system: false,
        room: None,
    })
    .unwrap()
}

// --- INV-0396-4: the lookup order and the launch check ---

#[test]
fn an_installed_name_never_shadows_anything() {
    assert_eq!(pick(true, true, true), Source::Image);
    assert_eq!(pick(true, false, true), Source::Image);
    assert_eq!(pick(false, true, true), Source::Path);
    assert_eq!(pick(false, false, true), Source::Installed);
    assert_eq!(pick(false, false, false), Source::Nothing);
}

fn record(words: &str) -> (Apps, App, Vec<u8>, Vec<u8>) {
    let program = b"\x7fELF the program".to_vec();
    let cmd = b"name ed\nsummary s\nusage ed\n---\n".to_vec();
    let toks: Vec<String> = words.split_whitespace().map(String::from).collect();
    let app = App {
        name: "ed".into(),
        hash: identity(&program, &cmd),
        installed: String::new(),
        words: apps::parse_words(&toks).unwrap(),
    };
    let bytes = apps::put(b"", &app).unwrap();
    (apps::read(&bytes), app, program, cmd)
}

#[test]
fn a_program_changed_by_one_byte_does_not_start_and_gets_no_word() {
    let (rec, app, program, cmd) = record("+w ~/notes +net h:443");
    // As installed: the words are the record's.
    assert_eq!(
        launch(&rec, "ed", &identity(&program, &cmd)),
        Launch::Run(&app.words)
    );
    // One byte of the program, any byte.
    for i in 0..program.len() {
        let mut p = program.clone();
        p[i] ^= 1;
        assert_eq!(
            launch(&rec, "ed", &identity(&p, &cmd)),
            Launch::Changed,
            "byte {i}"
        );
    }
    // One byte of the description, or of what is appended to either.
    let mut c = cmd.clone();
    c.push(b' ');
    assert_eq!(launch(&rec, "ed", &identity(&program, &c)), Launch::Changed);
    let mut p = program.clone();
    p.push(0);
    assert_eq!(launch(&rec, "ed", &identity(&p, &cmd)), Launch::Changed);
    // Another name has no record, and the same bytes under it get nothing.
    assert_eq!(
        launch(&rec, "vi", &identity(&program, &cmd)),
        Launch::Unknown
    );
    // The record is the hash of both parts.
    assert_eq!(app.hash, identity(&program, &cmd));
    assert_ne!(app.hash, sha256::hash(&program));
}

// --- INV-0396-1: no `+w` over /apps ---

#[test]
fn a_typed_write_over_apps_is_refused_and_nothing_else_is() {
    for refused in ["/apps", "/apps/ed", "/apps/ed/share", "/"] {
        let plan = planned(&[(true, refused)], "/home");
        let e = refuse_apps_write(&plan).expect_err(refused);
        assert!(
            e.problem.contains("changed with install"),
            "{refused}: {}",
            e.problem
        );
        assert!(e.subject.starts_with("+w "), "{}", e.subject);
    }
    // A read is not a write; a neighbour of the name is not inside it; /config is
    // the person's to give when they type it.
    for ok in [
        (false, "/apps"),
        (false, "/"),
        (true, "/appstore"),
        (true, "/home/apps"),
        (true, "/config"),
        (true, "/home"),
    ] {
        let plan = planned(&[ok], "/home");
        refuse_apps_write(&plan).unwrap_or_else(|e| panic!("{ok:?}: {e:?}"));
    }
    // No grant at all: the plan holds the current directory and nothing of /apps or /config.
    let plan = planned(&[], "/home");
    refuse_apps_write(&plan).unwrap();
    assert!(
        plan.iter()
            .all(|e| !e.path.starts_with("/apps") && !e.path.starts_with("/config"))
    );
    // A grant through a link keeps its `+w` reason and is looked at as well.
    let linked = vec![Planned {
        path: "/apps/ed".into(),
        access: Access::Tree,
        reason: "+w, through a link at ~/x".into(),
    }];
    assert!(refuse_apps_write(&linked).is_err());
}

// --- INV-0397-2 and INV-0397-5: the saved words are never more than the shell holds ---

fn held<'a>(
    path: &'a dyn Fn(&str, bool) -> bool,
    room: Option<&'a str>,
    untrusted: bool,
    room_nets: Option<&'a [String]>,
    network: bool,
) -> Held<'a> {
    Held {
        home: "/home",
        room,
        untrusted,
        room_nets,
        path,
        network,
    }
}

fn words(s: &str) -> Vec<Word> {
    let t: Vec<String> = s.split_whitespace().map(String::from).collect();
    apps::parse_words(&t).unwrap()
}

#[test]
fn every_word_that_is_held_applies_as_if_typed_before_the_command() {
    let everything = |_: &str, _: bool| true;
    let a = apply(
        "ed",
        &words("+own ~/.config/ed +w ~/notes +r ~/docs +net sync.example.org:443 +web"),
        &held(&everything, None, false, None, true),
    );
    assert!(a.notes.is_empty(), "{:?}", a.notes);
    assert_eq!(
        a.planned,
        vec![
            Planned {
                path: "/home/notes".into(),
                access: Access::Tree,
                reason: "saved".into()
            },
            Planned {
                path: "/home/docs".into(),
                access: Access::Read,
                reason: "saved".into()
            },
        ]
    );
    let own = a.own.unwrap();
    assert_eq!(
        (own.real.as_str(), own.view.as_str(), own.read),
        ("/data/home/ed", "/home/.config/ed", false)
    );
    assert!(a.net.scope.is_some() && a.net.dropped.is_empty());
    // In a room the own directory is the room's.
    let a = apply(
        "ed",
        &words("+own ~/.x"),
        &held(&everything, Some("work"), false, None, false),
    );
    assert_eq!(a.own.unwrap().real, "/data/work/ed");
}

#[test]
fn a_shell_that_holds_less_drops_the_words_and_says_so() {
    let only_notes =
        |p: &str, write: bool| write && p.starts_with("/home/notes") || !write && p == "/home/docs";
    let a = apply(
        "ed",
        &words("+w ~/notes +w ~/other +r ~/docs +r ~/secret +own ~/.config/ed +net h:443 +web"),
        &held(&only_notes, None, false, None, false),
    );
    let said: Vec<&str> = a.notes.iter().map(String::as_str).collect();
    assert_eq!(
        said,
        [
            "zosh: ed: saved +w ~/other is not held here\n",
            "zosh: ed: saved +r ~/secret is not held here\n",
            "zosh: ed: saved +own ~/.config/ed is not held here\n",
            "zosh: ed: saved +net h:443 is not held here\n",
            "zosh: ed: saved +web is not held here\n",
        ]
    );
    let paths: Vec<&str> = a.planned.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(paths, ["/home/notes", "/home/docs"]);
    assert!(a.own.is_none() && a.net.scope.is_none());
}

#[test]
fn the_record_itself_and_the_places_of_item_3_are_in_no_table() {
    let everything = |_: &str, _: bool| true;
    // A hand-edited record with words the writers refuse.
    let w = vec![
        Word::Read("/config/apps".into()),
        Word::Write("/config".into()),
        Word::Write("/apps/ed".into()),
        Word::Read("/".into()),
        Word::Write("/data/home".into()),
        Word::Write("~/ok".into()),
    ];
    let a = apply("ed", &w, &held(&everything, None, false, None, true));
    let paths: Vec<&str> = a.planned.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(paths, ["/home/ok"]);
    assert_eq!(a.notes.len(), 5);
    assert!(a.notes.iter().all(|n| n.contains("is not applied")));
}

#[test]
fn in_an_untrusted_room_no_saved_word_applies() {
    let everything = |_: &str, _: bool| true;
    let nets = vec![String::from("*")];
    let a = apply(
        "ed",
        &words("+w ~/notes +own ~/.x +net h:443 +web"),
        &held(&everything, Some("agent"), true, Some(&nets), true),
    );
    assert!(a.planned.is_empty() && a.own.is_none() && a.net.scope.is_none());
    assert_eq!(
        a.notes,
        vec![String::from(
            "zosh: ed: saved grants are not applied in an untrusted room\n"
        )]
    );
    // Trusted room, same words: they apply (a trusted one).
    let a = apply(
        "ed",
        &words("+w ~/notes"),
        &held(&everything, Some("work"), false, None, false),
    );
    assert_eq!(a.planned.len(), 1);
    // Nothing saved: nothing said, even in an untrusted room.
    let a = apply(
        "ed",
        &[],
        &held(&everything, Some("agent"), true, None, true),
    );
    assert_eq!(a, Applied::default());
}

#[test]
fn in_a_room_a_net_word_lies_inside_the_rooms_words() {
    let everything = |_: &str, _: bool| true;
    let nets = vec![String::from("*.example.org")];
    let a = apply(
        "ed",
        &words("+net api.example.org:443 +net other.test +web"),
        &held(&everything, Some("work"), false, Some(&nets), true),
    );
    assert!(a.net.scope.is_some());
    assert_eq!(a.net.dropped, ["+net other.test", "+web"]);
    // Outside a room the shell's own connection bounds it; `any` is never kept.
    let a = apply(
        "ed",
        &words("+net any"),
        &held(&everything, None, false, None, true),
    );
    assert!(a.net.scope.is_none());
    assert_eq!(a.net.dropped, ["+net any"]);
}

#[test]
fn the_network_words_make_one_scope() {
    use crate::netgrant::{describe, saved_scope};
    let n = saved_scope(&[String::from("sync.example.org:443")], true, None);
    let (rights, scope) = n.scope.unwrap();
    assert_ne!(rights, 0);
    let lines = describe(&scope);
    assert!(
        lines.iter().any(|l| l.contains("sync.example.org")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("web") || l.contains("public")),
        "{lines:?}"
    );
    assert_eq!(saved_scope(&[], false, None), Default::default());
    // A word that does not parse is left out and the good ones stay.
    let n = saved_scope(
        &[String::from("not a host"), String::from("h.test")],
        false,
        None,
    );
    assert_eq!(n.dropped, ["+net not a host"]);
    assert!(n.scope.is_some());
}

// --- INV-0396-5 and INV-0397-1: who may run the builtins and what may be saved ---

#[test]
fn install_and_programs_need_a_shell_that_holds_both_and_no_untrusted_room() {
    assert_eq!(gate(false, true, true), None);
    assert!(gate(true, true, true).unwrap().contains("untrusted"));
    for (apps, config) in [(false, true), (true, false), (false, false)] {
        assert!(
            gate(false, apps, config)
                .unwrap()
                .contains("/apps and /config")
        );
    }
    assert!(gate(true, false, false).unwrap().contains("untrusted"));
}

#[test]
fn only_words_typed_on_the_line_are_saved() {
    let typed = vec![Value::literal("+w"), Value::literal("~/notes")];
    assert_eq!(literal_tokens(&typed).unwrap(), ["+w", "~/notes"]);
    // A variable's value, a substitution's and a pipe's are data.
    for data in ["+w", "~/notes", "+w ~/notes"] {
        let mut v = typed.clone();
        v[1] = Value::data(data);
        let e = literal_tokens(&v).unwrap_err();
        assert!(e.contains("variable, a substitution or a pipe"), "{e}");
        let mut v = typed.clone();
        v[0] = Value::data(data);
        assert!(literal_tokens(&v).is_err());
    }
    assert_eq!(literal_tokens(&[]).unwrap(), Vec::<String>::new());
}

#[test]
fn a_rooms_write_over_apps_is_refused_too() {
    for path in ["/", "/apps", "/apps/ed"] {
        let plan = vec![Planned {
            path: path.into(),
            access: Access::Tree,
            reason: "room work".into(),
        }];
        assert!(refuse_apps_write(&plan).is_err(), "{path}");
    }
    let ok = vec![Planned {
        path: "/home/p".into(),
        access: Access::Tree,
        reason: "room work".into(),
    }];
    assert!(refuse_apps_write(&ok).is_ok());
}
