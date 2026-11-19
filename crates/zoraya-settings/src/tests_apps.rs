use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::apps::*;
use crate::sha256;

fn toks(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

fn ctx_ok(
    home: &str,
) -> (
    impl Fn(&str) -> bool + use<>,
    impl Fn(&str) -> Result<(), String> + use<>,
    String,
) {
    (
        |p: &str| p.ends_with(".txt"),
        |t: &str| {
            if t.starts_with("any") {
                Err("any is refused".into())
            } else {
                Ok(())
            }
        },
        home.to_string(),
    )
}

fn check(w: &Word, home: &str) -> Result<(), String> {
    let (f, n, h) = ctx_ok(home);
    check_word(
        w,
        &Ctx {
            home: &h,
            is_file: &f,
            net: &n,
        },
    )
}

// --- SHA-256 (FIPS 180-4 and NIST examples) ---

#[test]
fn sha256_known_answers() {
    let hex = |d: &[u8]| sha256::hex(&sha256::hash(d));
    assert_eq!(
        hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

#[test]
fn sha256_of_a_million_a() {
    let mut h = sha256::Sha256::new();
    let chunk = [b'a'; 1000];
    for _ in 0..1000 {
        h.update(&chunk);
    }
    assert_eq!(
        sha256::hex(&h.finish()),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

#[test]
fn sha256_does_not_depend_on_how_the_bytes_are_split() {
    let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    let whole = sha256::hash(&data);
    for cut in [0, 1, 55, 56, 63, 64, 65, 119, 120, 128, 999, 1000] {
        let mut h = sha256::Sha256::new();
        h.update(&data[..cut]);
        h.update(&data[cut..]);
        assert_eq!(h.finish(), whole, "split at {cut}");
    }
    // Every length around the padding's edge, against a one-shot hash of a
    // copy that is fed a byte at a time.
    for n in 0..200 {
        let mut h = sha256::Sha256::new();
        for b in &data[..n] {
            h.update(&[*b]);
        }
        assert_eq!(h.finish(), sha256::hash(&data[..n]), "length {n}");
    }
}

#[test]
fn the_identity_moves_with_either_part_and_with_the_boundary() {
    let a = sha256::identity(b"prog", b"desc");
    assert_ne!(a, sha256::identity(b"prog!", b"desc"));
    assert_ne!(a, sha256::identity(b"prog", b"desc!"));
    // The same bytes with the boundary moved are another identity.
    assert_ne!(
        sha256::identity(b"progd", b"esc"),
        sha256::identity(b"prog", b"desc")
    );
    assert_eq!(sha256::from_hex(&sha256::hex(&a)), Some(a));
    assert_eq!(sha256::from_hex("zz"), None);
}

// --- Words, and what may be saved (ADR-0397 items 1 to 3; INV-0397-4) ---

#[test]
fn the_five_words_parse_and_print_as_typed() {
    let w = parse_words(&toks(
        "+own ~/.config/ed +w ~/notes +r ~/docs +net sync.example.org:443 +web",
    ))
    .unwrap();
    let text: Vec<String> = w.iter().map(Word::words).collect();
    assert_eq!(
        text,
        [
            "+own ~/.config/ed",
            "+w ~/notes",
            "+r ~/docs",
            "+net sync.example.org:443",
            "+web"
        ]
    );
}

#[test]
fn nothing_else_is_saved() {
    for bad in [
        "+listen 80",
        "-w ~/x",
        "COMMAND",
        "FOO=bar",
        "+w",
        "+net",
        "+w relative/path",
        "+w ~/../up",
        "+net ''",
    ] {
        let t: Vec<String> = if bad == "+net ''" {
            vec!["+net".into(), String::new()]
        } else {
            toks(bad)
        };
        assert!(parse_words(&t).is_err(), "{bad}");
    }
}

#[test]
fn one_case_per_refusal_of_item_3() {
    let home = "/home";
    for (word, why) in [
        (Word::Write("/".into()), "the whole volume"),
        (Word::Read("/".into()), "the whole volume"),
        (Word::Write("/config".into()), "configuration"),
        (Word::Read("/config/apps".into()), "configuration"),
        (Word::Write("/apps".into()), "installed programs"),
        (Word::Write("/apps/ed".into()), "installed programs"),
        (Word::Write("/data".into()), "own directories"),
        (Word::Read("/data/home".into()), "own directories"),
        (Word::Write("/data/work".into()), "own directories"),
        (Word::Write("/data/home/ssh/keys".into()), "own directories"),
        (Word::Read("/home/notes.txt".into()), "a file"),
        (Word::Net("any".into()), "any is refused"),
        (Word::Own("/x".into()), "plain names"),
        (Word::Own("~/../x".into()), "plain names"),
        (Word::Own("~".into()), "plain names"),
    ] {
        let e = check(&word, home).expect_err(&word.words());
        assert!(e.contains(why), "{}: {e}", word.words());
        assert!(
            e.contains(&word.words().split(' ').next().unwrap().to_string()) || e.contains("+")
        );
    }
    // The volume root as the shell's `~`.
    let e = check(&Word::Write("~".into()), "/").unwrap_err();
    assert!(e.contains("volume root"), "{e}");
    // And what is allowed.
    for ok in [
        Word::Write("~".into()),
        Word::Write("~/notes".into()),
        Word::Read("/system/fonts".into()),
        Word::Read("/datastore".into()),
        Word::Read("/configs".into()),
        Word::Net("sync.example.org:443".into()),
        Word::Web,
        Word::Own("~/.config/ed".into()),
    ] {
        check(&ok, home).unwrap_or_else(|e| panic!("{}: {e}", ok.words()));
    }
    assert!(is_everything(&Word::Write("~".into()), home));
    assert!(is_everything(&Word::Write("/home".into()), home));
    assert!(!is_everything(&Word::Write("~/notes".into()), home));
    assert!(!is_everything(&Word::Read("~".into()), home));
    assert!(meaning(&Word::Write("~".into()), home).contains("everything"));
    assert!(!meaning(&Word::Write("~/n".into()), home).contains("everything"));
}

// --- The record ---

fn app(name: &str, words: &str) -> App {
    App {
        name: name.into(),
        hash: sha256::hash(name.as_bytes()),
        installed: "2026-11-15".into(),
        words: parse_words(&toks(words)).unwrap(),
    }
}

#[test]
fn a_line_reads_back_as_written() {
    let a = app(
        "ed",
        "+own ~/.config/ed +w ~/notes +net sync.example.org:443 +web",
    );
    let bytes = alloc::format!("# mine\n{}\n", a.line());
    let r = read(bytes.as_bytes());
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.get("ed"), Some(&a));
    assert_eq!(a.short_hash().len(), 8);
}

#[test]
fn the_reader_is_total_and_says_each_bad_line_once() {
    let good = app("ed", "+w ~/n").line();
    let text = alloc::format!(
        "{good}\nBad sha256:00\ned sha256:{h}\nnoname\n\"\n{good2}\nzed sha256:{h} --installed 2026 +w ~/x\nzed sha256:{h} +listen 5\n",
        h = sha256::hex(&sha256::hash(b"x")),
        good2 = app("ed", "+r ~/other").line()
    );
    let r = read(text.as_bytes());
    assert_eq!(r.apps.len(), 1, "{:?}", r.apps);
    assert_eq!(r.apps[0].words, vec![Word::Write("~/n".into())]);
    // Bad name, a second line, a short hash...: one problem each.
    assert_eq!(r.problems.len(), 7, "{:?}", r.problems);
    let lines: Vec<usize> = r.problems.iter().map(|p| p.line).collect();
    assert_eq!(lines, [2, 3, 4, 5, 6, 7, 8]);
    // Any bytes at all.
    for junk in [&b"\xff\xfe\n"[..], b"\0\0\0", b"a\rb\n\n\n", &[b'x'; 5000]] {
        let _ = read(junk);
    }
}

#[test]
fn the_reader_stops_at_the_cap_and_at_the_file_size() {
    let mut text = String::new();
    for i in 0..(MAX_APPS + 3) {
        let n = alloc::format!("p{}", (b'a' + (i % 26) as u8) as char);
        let n = alloc::format!("{n}{}", i / 26);
        text.push_str(&app(&n, "").line());
        text.push('\n');
    }
    let r = read(text.as_bytes());
    assert_eq!(r.apps.len(), MAX_APPS);
    assert_eq!(r.problems.len(), 3);
    let big = alloc::format!("{}\n{}", app("ed", "").line(), "# x\n".repeat(70_000));
    let r = read(big.as_bytes());
    assert_eq!(r.apps.len(), 1);
    assert!(r.problems.iter().any(|p| p.problem.contains("256 KiB")));
}

#[test]
fn put_and_remove_keep_every_other_byte() {
    let ed = app("ed", "+w ~/notes");
    let vi = app("vi", "");
    let base = alloc::format!("# header\n\nmystery line\n{}\n", ed.line());
    let with_vi = put(base.as_bytes(), &vi).unwrap();
    assert_eq!(
        String::from_utf8(with_vi.clone()).unwrap(),
        alloc::format!("{base}{}\n", vi.line())
    );
    // A file with no final newline gets one before the new line.
    let cut = base.trim_end().as_bytes();
    assert!(
        put(cut, &vi)
            .unwrap()
            .ends_with(alloc::format!("\n{}\n", vi.line()).as_bytes())
    );
    // Replacing keeps the rest.
    let mut ed2 = ed.clone();
    ed2.words.clear();
    let replaced = put(&with_vi, &ed2).unwrap();
    let r = read(&replaced);
    assert_eq!(r.get("ed").unwrap().words.len(), 0);
    assert!(
        String::from_utf8(replaced.clone())
            .unwrap()
            .starts_with("# header\n\nmystery line\n")
    );
    assert!(r.get("vi").is_some());
    // Removing drops that line only.
    let gone = remove(&replaced, "ed").unwrap();
    let r = read(&gone);
    assert!(r.get("ed").is_none() && r.get("vi").is_some());
    assert!(String::from_utf8(gone).unwrap().contains("mystery line"));
    assert_eq!(remove(&replaced, "nope"), Err(Refusal::NoSuchApp));
    assert_eq!(put(b"", &app("Ed", "")), Err(Refusal::BadName));
}

#[test]
fn a_record_has_at_most_64_programs_and_16_words() {
    let mut bytes = Vec::new();
    for i in 0..MAX_APPS {
        let n = alloc::format!("p{}{}", (b'a' + (i % 26) as u8) as char, i / 26);
        bytes = put(&bytes, &app(&n, "")).unwrap();
    }
    assert_eq!(put(&bytes, &app("extra", "")), Err(Refusal::TooMany));
    let mut many = app("ed", "");
    for i in 0..=MAX_WORDS {
        many.words.push(Word::Read(alloc::format!("~/d{i}")));
    }
    assert_eq!(put(b"", &many), Err(Refusal::TooMany));
}

// --- INV-0399-1: revoke removes exactly the words named ---

#[test]
fn a_revoke_removes_exactly_the_words_named() {
    let a = app("ed", "+w ~/notes +r ~/docs +net sync.example.org:443");
    let other = app("vi", "+w ~/notes");
    let bytes = put(&put(b"# top\n", &a).unwrap(), &other).unwrap();
    let (after, gone) = revoke(&bytes, "ed", &[Word::Write("~/notes".into())]).unwrap();
    assert_eq!(gone, vec![Word::Write("~/notes".into())]);
    let r = read(&after);
    assert_eq!(
        r.get("ed").unwrap().words,
        vec![
            Word::Read("~/docs".into()),
            Word::Net("sync.example.org:443".into())
        ]
    );
    // Another program's identical word stays, and so do the other lines.
    assert_eq!(
        r.get("vi").unwrap().words,
        vec![Word::Write("~/notes".into())]
    );
    assert!(
        String::from_utf8(after.clone())
            .unwrap()
            .starts_with("# top\n")
    );
    assert_eq!(r.get("ed").unwrap().hash, a.hash);
    // A word it does not hold refuses everything and changes nothing.
    assert_eq!(
        revoke(&bytes, "ed", &[Word::Read("~/docs".into()), Word::Web]),
        Err(Refusal::NotSaved("+web".into()))
    );
    assert_eq!(
        revoke(&bytes, "nobody", &[Word::Web]),
        Err(Refusal::NoSuchApp)
    );
    // `+w ~/notes` is not `+r ~/notes`.
    assert!(revoke(&bytes, "ed", &[Word::Read("~/notes".into())]).is_err());
    let (all, gone) = revoke_all(&bytes, "ed").unwrap();
    assert_eq!(gone.len(), 3);
    assert!(read(&all).get("ed").unwrap().words.is_empty());
    assert_eq!(read(&all).get("vi").unwrap().words.len(), 1);
}

// --- The package check (INV-0396-3, INV-0396-2) ---

const CMD: &str = "name  ed\nsummary  the standard editor\nusage  ed [FILE]\noperand  [FILE]  read  the file\noption  -s --silent  no counts\nexit  0  fine\n---\nBody text.\n";
const ELF: &[u8] = b"\x7fELF\x02\x01";

fn pkg<'a>(name: &'a str, cmd: Option<&'a [u8]>, head: Option<&'a [u8]>) -> Package<'a> {
    Package {
        name,
        program_head: head,
        description: cmd,
        request: None,
    }
}

fn free(_: &str) -> Option<&'static str> {
    None
}

#[test]
fn a_good_package_installs_with_every_kind_text() {
    let c = check_package(&pkg("ed", Some(CMD.as_bytes()), Some(ELF)), &free, false).unwrap();
    assert_eq!(c.description.name, "ed");
    assert!(
        c.description
            .operands
            .iter()
            .all(|o| o.kind == zoraya_describe::Kind::Text)
    );
    // A script with #! is a program too.
    check_package(
        &pkg("ed", Some(CMD.as_bytes()), Some(b"#!/bin/sh\n")),
        &free,
        false,
    )
    .unwrap();
}

#[test]
fn one_case_per_refusal_of_install() {
    let r = |p: &Package<'_>, taken: &dyn Fn(&str) -> Option<&'static str>, stray| {
        check_package(p, taken, stray).unwrap_err()
    };
    let good = |cmd: &str| -> Result<Checked, Reject> {
        check_package(&pkg("ed", Some(cmd.as_bytes()), Some(ELF)), &free, false)
    };
    // A name that is not one.
    assert_eq!(
        r(&pkg("Ed", Some(CMD.as_bytes()), Some(ELF)), &free, false),
        Reject::BadName
    );
    assert_eq!(
        r(&pkg("", Some(CMD.as_bytes()), Some(ELF)), &free, false),
        Reject::BadName
    );
    assert_eq!(
        r(
            &pkg(&"a".repeat(MAX_NAME + 1), Some(CMD.as_bytes()), Some(ELF)),
            &free,
            false
        ),
        Reject::BadName
    );
    // Taken by an image command, a builtin, a function, a room: the caller's list.
    let taken = |n: &str| match n {
        "ls" => Some("an image command"),
        "cd" => Some("a builtin"),
        _ => None,
    };
    assert_eq!(
        r(&pkg("ls", Some(CMD.as_bytes()), Some(ELF)), &taken, false),
        Reject::NameTaken("an image command")
    );
    assert_eq!(
        r(&pkg("cd", Some(CMD.as_bytes()), Some(ELF)), &taken, false),
        Reject::NameTaken("a builtin")
    );
    // A directory under /apps that no install made.
    assert_eq!(
        r(&pkg("ed", Some(CMD.as_bytes()), Some(ELF)), &free, true),
        Reject::NameTaken("another package")
    );
    // No program, not a program, no description.
    assert_eq!(
        r(&pkg("ed", Some(CMD.as_bytes()), None), &free, false),
        Reject::NoProgram
    );
    assert_eq!(
        r(
            &pkg("ed", Some(CMD.as_bytes()), Some(b"MZ\0\0")),
            &free,
            false
        ),
        Reject::NotAProgram
    );
    assert_eq!(
        r(&pkg("ed", None, Some(ELF)), &free, false),
        Reject::NoDescription
    );
    // A name that differs.
    assert_eq!(
        good(&CMD.replace("name  ed", "name  vi")).unwrap_err(),
        Reject::NameDiffers("vi".into())
    );
    assert_eq!(
        good(&CMD.replace("name  ed", "topic  ed")).unwrap_err(),
        Reject::Key(1, "topic".into())
    );
    // The parser's own refusal, with its line.
    let e = good("name  ed\nsummary\n---\n").unwrap_err();
    assert!(matches!(e, Reject::Description(2, _)), "{e:?}");
    // A key outside the set, one per key of ADR-0396 item 5.
    for key in [
        "data  ed as ~/.ed",
        "needs  net-status",
        "program  /bin/ed",
        "cwd  none",
        "setting  ed.x  text  a  b",
        "when  a B read",
        "net  web",
        "reach  HOST tcp 22",
        "window  Title",
        "names  a b",
    ] {
        let k = key.split_whitespace().next().unwrap();
        let cmd = alloc::format!("name  ed\nsummary  s\n{key}\n---\n");
        assert_eq!(good(&cmd).unwrap_err(), Reject::Key(3, k.into()), "{key}");
    }
    // A body line that looks like a key is text.
    good("name  ed\nsummary  s\nusage  ed\n---\ndata  ed as ~/.x\nneeds  net-status\n").unwrap();
    // The messages say the key and the line.
    assert!(Reject::Key(3, "data".into()).text("ed").contains("`data`"));
    assert!(Reject::NameDiffers("vi".into()).text("ed").contains("`vi`"));
}

#[test]
fn a_description_that_names_authority_designates_nothing() {
    // INV-0396-2: the parser would read these; `for_volume` makes them text.
    let text = "name  ed\nsummary  s\nusage  ed FILE\noperand  FILE  create  the file\noperand  [DIR...]  tree  dirs\noption  -o --out FILE create  where\n---\n";
    let parsed = zoraya_describe::parse(text.as_bytes()).unwrap();
    assert_eq!(parsed.operands[0].kind, zoraya_describe::Kind::Create);
    let v = zoraya_describe::for_volume(parsed);
    assert!(
        v.operands
            .iter()
            .all(|o| o.kind == zoraya_describe::Kind::Text)
    );
    assert!(v.options.iter().all(|o| {
        o.argument
            .as_ref()
            .is_none_or(|a| a.kind == zoraya_describe::Kind::Text)
    }));
    // Even a description built with the authority keys set loses them.
    let mut d = zoraya_describe::Description::new(zoraya_describe::What::Command, "ed", "s");
    d.data = Some(zoraya_describe::Data {
        name: "ed".into(),
        path: "~/.ed".into(),
        read: false,
    });
    d.net_web = true;
    d.needs_net_status = true;
    d.program = Some("/bin/ed".into());
    d.window = Some("ed".into());
    d.cwd = zoraya_describe::Cwd::None;
    let v = zoraya_describe::for_volume(d);
    assert!(
        v.data.is_none()
            && !v.net_web
            && !v.needs_net_status
            && v.program.is_none()
            && v.window.is_none()
    );
    assert_eq!(v.cwd, zoraya_describe::Cwd::Read);
}

#[test]
fn a_request_is_shown_as_the_words_that_parse_and_applied_never() {
    let req = b"+own ~/.config/ed\n+w ~/notes\nrm -rf /\n\n# note\n+listen 80\n+w ~/notes\n\xff\n";
    let words = request_words(req);
    assert_eq!(
        words,
        vec![
            Word::Own("~/.config/ed".into()),
            Word::Write("~/notes".into())
        ]
    );
    // A request changes nothing in a check: only `asked` carries it.
    let mut p = pkg("ed", Some(CMD.as_bytes()), Some(ELF));
    p.request = Some(req);
    let c = check_package(&p, &free, false).unwrap();
    assert_eq!(c.asked, words);
    p.request = None;
    assert!(check_package(&p, &free, false).unwrap().asked.is_empty());
}

// --- What `programs` prints ---

#[test]
fn the_table_and_the_page_head() {
    let apps = read(
        alloc::format!(
            "{}\n{}\n",
            app("ed", "+w ~/n").line(),
            app("brws", "+web +own ~/.x").line()
        )
        .as_bytes(),
    );
    let t = table(&apps, &|n| usize::from(n == "brws"), &|n| n == "brws");
    let rows: Vec<&str> = t.lines().collect();
    assert!(rows[0].starts_with("NAME") && rows[0].contains("RUNNING"));
    assert!(rows[1].starts_with("ed ") && rows[1].contains("2026-11-15"));
    assert!(
        rows[2].starts_with("brws") && rows[2].ends_with("changed"),
        "{t}"
    );
    assert_eq!(
        table(&Apps::default(), &|_| 0, &|_| false),
        "no installed programs\n"
    );
    let a = apps.get("ed").unwrap();
    let head = help_head(a);
    assert!(head.starts_with("ed is an installed program, /apps/ed/ed (sha256 "));
    assert!(head.ends_with(", installed 2026-11-15)"));
}

// --- The log (INV-0399-5) ---

#[test]
fn the_log_has_a_line_a_change_and_stays_under_its_cap() {
    let mut log = Vec::new();
    log = log_append(
        &log,
        &log_line(
            "2026-11-15T10:02:11",
            "install",
            "ed",
            "sha256:1a2b3c4d +own +w",
        ),
    );
    log = log_append(
        &log,
        &log_line("2026-11-15T10:03:00", "revoke", "ed", "+w ~/notes"),
    );
    assert_eq!(log.iter().filter(|b| **b == b'\n').count(), 2);
    assert_eq!(
        log_view(&log, None),
        "2026-11-15T10:02:11 install ed sha256:1a2b3c4d +own +w\n2026-11-15T10:03:00 revoke ed +w ~/notes\n"
    );
    assert!(log_view(&log, Some("vi")).is_empty());
    assert_eq!(log_view(&log, Some("ed")).lines().count(), 2);
    for i in 0..5000 {
        log = log_append(
            &log,
            &log_line(
                "2026-11-15T10:03:00",
                "grant",
                "ed",
                &alloc::format!("+r ~/dir{i}"),
            ),
        );
        assert!(log.len() <= LOG_CAP);
    }
    // The oldest went, whole lines only, the newest is last.
    assert!(log.len() > LOG_CAP - 200);
    assert!(log.starts_with(b"2026-11-15T10:03:00 grant ed"));
    assert!(log.ends_with(b"+r ~/dir4999\n"));
    assert!(
        log.split(|b| *b == b'\n')
            .all(|l| l.is_empty() || l.starts_with(b"2026-"))
    );
    // A hostile detail is cut at a line's length.
    let huge = "x".repeat(5000);
    let l = log_append(b"", &log_line("t", "grant", "ed", &huge));
    assert!(l.len() <= 1025);
}

// --- Instances (INV-0399-2) ---

#[test]
fn the_instances_of_a_name_in_this_tree_and_elsewhere() {
    let all = vec![
        Instance {
            job: 1,
            program: "ed".into(),
            here: true,
        },
        Instance {
            job: 1,
            program: "ed".into(),
            here: true,
        },
        Instance {
            job: 2,
            program: "vi".into(),
            here: true,
        },
        Instance {
            job: 3,
            program: "ed".into(),
            here: true,
        },
        Instance {
            job: 4,
            program: "ed".into(),
            here: false,
        },
        Instance {
            job: 5,
            program: "edx".into(),
            here: true,
        },
    ];
    let (here, elsewhere) = instances_of(&all, "ed");
    assert_eq!(
        here,
        vec![1, 3],
        "exactly the instances of ed in the tree, once a job"
    );
    assert_eq!(elsewhere, 1);
    let (here, elsewhere) = instances_of(&all, "vi");
    assert_eq!((here, elsewhere), (vec![2], 0));
    assert_eq!(instances_of(&all, "nobody"), (vec![], 0));
}

// Holes the mutation run found (10j).

#[test]
fn tokens_hex_and_the_names_of_words() {
    assert_eq!(Word::Read("~".into()).token(), "+r");
    assert_eq!(Word::Write("~".into()).token(), "+w");
    assert_eq!(Word::Net("h".into()).token(), "+net");
    assert_eq!(Word::Web.token(), "+web");
    assert_eq!(Word::Own("~/x".into()).token(), "+own");
    let d = sha256::hash(b"abc");
    let hex = sha256::hex(&d);
    assert_eq!(sha256::from_hex(&hex.to_uppercase()), Some(d));
    let mixed: String = hex
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if i % 2 == 0 {
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect();
    assert_eq!(sha256::from_hex(&mixed), Some(d));
    assert_eq!(sha256::from_hex(&"g".repeat(64)), None);
    assert_eq!(sha256::from_hex(&"0".repeat(64)), Some([0; 32]));
    assert_eq!(sha256::from_hex(&"f".repeat(64)), Some([255; 32]));
    assert_eq!(sha256::from_hex(&"A".repeat(64)), Some([0xAA; 32]));
    assert!(is_app_name("a") && is_app_name("a-1") && is_app_name(&"a".repeat(MAX_NAME)));
    assert!(!is_app_name("-a") && !is_app_name("a_b") && !is_app_name("1a"));
    assert!(meaning(&Word::Read("/home".into()), "/home").starts_with("~  read"));
    assert!(meaning(&Word::Read("/home/x".into()), "/home").starts_with("~/x  read"));
}

// The caps, exactly (the mutation run's triage).

fn line_with(n_web: usize) -> String {
    let mut s = alloc::format!("ed sha256:{}", sha256::hex(&sha256::hash(b"x")));
    for _ in 0..n_web {
        s.push_str(" +web");
    }
    s.push('\n');
    s
}

#[test]
fn the_log_keeps_exactly_its_cap() {
    assert_eq!(LOG_CAP, 65536);
    let mut log = Vec::new();
    let detail = "d".repeat(1023 - "t grant ed ".len());
    for _ in 0..64 {
        log = log_append(&log, &log_line("t", "grant", "ed", &detail));
    }
    assert_eq!(log.len(), 65536);
    assert!(log.starts_with(b"t grant ed d"));
    assert_eq!(log.iter().filter(|b| **b == b'\n').count(), 64);
    log = log_append(&log, &log_line("t", "grant", "ed", &detail));
    assert!(log.len() <= 65536 && log.iter().filter(|b| **b == b'\n').count() == 64);
}

#[test]
fn the_limits_of_a_line_and_of_a_record() {
    // The 37th token of a line is the last: the name, the hash, `--installed DATE`
    // and 16 words of two tokens (a record of 16 words with its date reads).
    assert!(read(line_with(1).as_bytes()).problems.is_empty());
    // (+web repeats are one word; the token limit is what is tested.)
    let long = line_with(35);
    assert!(read(long.as_bytes()).problems.is_empty(), "37 tokens");
    assert_eq!(
        read(line_with(36).as_bytes()).problems.len(),
        1,
        "38 tokens"
    );
    // 16 distinct words parse, 17 do not.
    let words = |n: usize| -> Vec<String> {
        (0..n)
            .flat_map(|i| [String::from("+r"), alloc::format!("~/d{i}")])
            .collect()
    };
    assert_eq!(parse_words(&words(16)).unwrap().len(), 16);
    assert!(parse_words(&words(17)).is_err());
    // A record of 16 words is stored, new or replaced, and 17 is not.
    let mut a = app("ed", "");
    a.words = parse_words(&words(16)).unwrap();
    let bytes = put(b"", &a).unwrap();
    assert_eq!(read(&bytes).get("ed").unwrap().words.len(), 16);
    assert!(put(&bytes, &a).is_ok());
    a.words.push(Word::Read("~/more".into()));
    assert_eq!(put(&bytes, &a), Err(Refusal::TooMany));
    assert_eq!(put(b"", &a), Err(Refusal::TooMany));
    // A request keeps 16 words.
    let req: String = (0..17).map(|i| alloc::format!("+r ~/d{i}\n")).collect();
    assert_eq!(request_words(req.as_bytes()).len(), 16);
}

#[test]
fn the_file_is_cut_at_the_last_whole_line() {
    let one = line_with(1);
    // Exactly the size, ending in a newline: no problem.
    let mut exact = String::new();
    while exact.len() + one.len() < crate::MAX_SIZE {
        exact.push_str(&one.replace("ed ", "ed2 "));
    }
    let pad = crate::MAX_SIZE - exact.len();
    exact.push_str(&"#".repeat(pad - 1));
    exact.push('\n');
    assert_eq!(exact.len(), crate::MAX_SIZE);
    assert!(
        read(exact.as_bytes())
            .problems
            .iter()
            .all(|p| !p.problem.contains("256 KiB"))
    );
    // Over it: the partial last line is dropped, the whole ones before stay.
    let kept = one.len() * 2;
    let mut big = String::new();
    big.push_str(&one);
    big.push_str(&"#".repeat(crate::MAX_SIZE - kept - 1));
    big.push('\n');
    big.push_str(&one.replace("ed ", "zed "));
    big.push_str(&"#".repeat(10));
    let r = read(big.as_bytes());
    assert!(r.get("ed").is_some_and(|a| a.words == vec![Word::Web]));
    assert!(r.get("zed").is_none());
    let cut = r
        .problems
        .iter()
        .find(|p| p.problem.contains("256 KiB"))
        .unwrap();
    assert_eq!(cut.line, 3);
}

// --- `+sock` (ADR-0465): the one saved word that names a file ---

#[test]
fn a_sock_word_is_saved_only_for_a_file_in_a_program_directory() {
    let w = parse_words(&toks("+sock /data/home/srv/s")).unwrap();
    assert_eq!(w, [Word::Sock("/data/home/srv/s".into())]);
    assert_eq!(w[0].words(), "+sock /data/home/srv/s");
    assert_eq!(w[0].token(), "+sock");
    assert_eq!(check(&w[0], "/home"), Ok(()));
    assert_eq!(
        sock_place("/data/home/srv/run/s"),
        Some(("/data/home/srv/run", "s"))
    );
    for bad in [
        "/data/home/srv",
        "/data/home",
        "/data",
        "/data/home/srv/",
        "/data/home/../srv/s",
        "/data/home/srv//s",
        "/tmp/s",
        "/config/s/x",
    ] {
        assert_eq!(sock_place(bad), None, "{bad}");
        let r = check(&Word::Sock(bad.into()), "/home");
        assert!(r.is_err(), "{bad}");
    }
    // A word needs its path, and a relative or `~` one is not a socket's.
    assert!(parse_words(&toks("+sock")).is_err());
    assert!(check(&Word::Sock("~/s".into()), "/home").is_err());
    // It round-trips through a record line.
    let app = App {
        name: "cl".into(),
        hash: sha256::identity(b"p", b"d"),
        installed: String::new(),
        words: w,
    };
    assert!(app.line().contains("+sock /data/home/srv/s"));
}
