use super::*;

const CP: &str = "name     cp
summary  copy a file
usage    cp [--force] FROM TO
operand  FROM  read    the file to copy
operand  TO    create  the name of the copy
option   -f --force    replace TO if it exists
exit     0  copied
exit     1  not copied
---
Copies the file FROM to a new file TO. TO must not exist unless --force is given.

EXAMPLES
  cp notes.txt notes.bak
  cp --force draft.txt final.txt

SEE ALSO
  mv, rm
";

const RM: &str = "name rm
summary remove files
usage rm [--recursive] [--verbose] PATH...
operand PATH... remove the files to remove
option -r --recursive also directories
option -v --verbose say what was removed
option -n --lines N text how many
option -1 --one just one
when --recursive PATH remove-tree
";

const SEARCH: &str = "name search
summary find lines that contain text
usage search TEXT [PATH...]
operand TEXT text what to look for
operand [PATH...] read where to look
question
";

fn d(text: &str) -> Description {
    zoraya_describe::parse(text.as_bytes()).unwrap()
}

fn run<'a>(d: &Description, args: &[&'a str]) -> Parsed<'a> {
    match parse(d, args) {
        Ok(Outcome::Run(p)) => p,
        other => panic!("{args:?}: {other:?}"),
    }
}

fn usage(d: &Description, args: &[&str]) -> String {
    match parse(d, args) {
        Err(e) => e.0,
        other => panic!("{args:?}: {other:?}"),
    }
}

#[test]
fn help_is_the_example_of_adr_0155() {
    assert_eq!(
        help(&d(CP)),
        "cp: copy a file
usage: cp [--force] FROM TO
  -f, --force   replace TO if it exists
  --help        show this help
more: help cp
"
    );
}

#[test]
fn a_usage_mistake_is_the_example_of_adr_0155() {
    let cp = d(CP);
    let e = parse(&cp, &[]).unwrap_err();
    assert_eq!(
        usage_error(&cp, &e),
        "cp: missing operand FROM
usage: cp [--force] FROM TO
more: help cp
"
    );
    assert_eq!(usage(&cp, &["a"]), "missing operand TO");
    assert_eq!(usage(&cp, &["a", "b", "c"]), "c: unexpected operand");
}

#[test]
fn the_page_is_the_example_of_adr_0156() {
    assert_eq!(
        page(&d(CP), "cp is a program, /zoraya/bin/cp"),
        "cp is a program, /zoraya/bin/cp

CP

copy a file

usage: cp [--force] FROM TO

  FROM   the file to copy (read)
  TO     the name of the copy (created)
  -f, --force   replace TO if it exists

Copies the file FROM to a new file TO. TO must not exist unless --force is given.

EXAMPLES
  cp notes.txt notes.bak
  cp --force draft.txt final.txt

EXIT STATUS
  0  copied
  1  not copied

SEE ALSO
  mv, rm
"
    );
}

#[test]
fn completion_is_the_example_of_adr_0154() {
    assert_eq!(
        columns(&completion_options(&d(CP)), "", 4),
        "--force    replace TO if it exists
--help     show this help
"
    );
}

#[test]
fn options_in_every_place_and_spelling() {
    let rm = d(RM);
    let force = |args: &[&str]| run(&rm, args).flag(&rm, "recursive");
    assert!(force(&["-r", "x"]));
    assert!(force(&["x", "--recursive"]));
    assert!(force(&["-vr", "x"]));
    assert!(!force(&["--", "-r"]));
    assert_eq!(run(&rm, &["--", "-r"]).operand_values(&rm, "PATH"), ["-r"]);
    assert_eq!(run(&rm, &["./-r"]).operand_values(&rm, "PATH"), ["./-r"]);
    assert_eq!(run(&rm, &["-"]).operand_values(&rm, "PATH"), ["-"]);
    for args in [
        &["--lines", "5", "x"][..],
        &["--lines=5", "x"],
        &["-n", "5", "x"],
        &["-n5", "x"],
        &["-vn", "5", "x"],
        &["x", "-n", "5"],
    ] {
        let p = run(&rm, args);
        assert_eq!(p.value(&rm, "lines"), Some("5"), "{args:?}");
        assert_eq!(p.operand_values(&rm, "PATH"), ["x"], "{args:?}");
    }
    // The last of a repeated option wins.
    assert_eq!(
        run(&rm, &["-n", "1", "-n", "2", "x"]).value(&rm, "lines"),
        Some("2")
    );
    assert!(run(&rm, &["-1", "x"]).flag(&rm, "one"));
    assert_eq!(usage(&rm, &["--nope", "x"]), "--nope: no such option");
    assert_eq!(usage(&rm, &["-z", "x"]), "-z: no such option");
    assert_eq!(usage(&rm, &["-rz", "x"]), "-z: no such option");
    assert_eq!(usage(&rm, &["--lines"]), "--lines: needs a value");
    assert_eq!(usage(&rm, &["x", "-n"]), "-n: needs a value");
    assert_eq!(usage(&rm, &["-n", "--", "x"]), "-n: needs a value");
    assert_eq!(
        usage(&rm, &["--verbose=yes", "x"]),
        "--verbose: takes no value"
    );
    assert_eq!(usage(&rm, &[]), "missing operand PATH");
    assert_eq!(parse(&rm, &["--nope", "--help"]), Ok(Outcome::Help));
    assert_eq!(
        run(&rm, &["--", "--help"]).operand_values(&rm, "PATH"),
        ["--help"]
    );
}

#[test]
fn a_negative_number_is_an_operand_unless_a_digit_option_exists() {
    let search = d(SEARCH);
    assert_eq!(run(&search, &["-5"]).operand(&search, "TEXT"), Some("-5"));
    let rm = d(RM);
    assert!(run(&rm, &["-1", "x"]).flag(&rm, "one"));
    assert_eq!(usage(&rm, &["-5", "x"]), "-5: no such option");
}

#[test]
fn operands_by_arity() {
    let s = d(SEARCH);
    assert_eq!(usage(&s, &[]), "missing operand TEXT");
    let p = run(&s, &["milk"]);
    assert!(p.operand_values(&s, "PATH").is_empty());
    let p = run(&s, &["milk", "a", "b"]);
    assert_eq!(p.operand(&s, "TEXT"), Some("milk"));
    assert_eq!(p.operand_values(&s, "PATH"), ["a", "b"]);
    let opt = d("name ls\nsummary s\nusage ls [A] [B]\noperand [A] read a\noperand [B] read b\n");
    assert_eq!(run(&opt, &["x"]).operand(&opt, "A"), Some("x"));
    assert_eq!(run(&opt, &["x"]).operand(&opt, "B"), None);
    assert_eq!(run(&opt, &["x", "y"]).operand(&opt, "B"), Some("y"));
    assert_eq!(usage(&opt, &["x", "y", "z"]), "z: unexpected operand");
    let mid = d("name mv\nsummary s\nusage mv FROM... TO\noperand FROM... read a\n");
    assert_eq!(
        run(&mid, &["a", "b"]).operand_values(&mid, "FROM"),
        ["a", "b"]
    );
    let none = d("name x\nsummary s\nusage x\n");
    assert_eq!(usage(&none, &["a"]), "a: unexpected operand");
}

#[test]
fn when_changes_an_operands_kind() {
    let rm = d(RM);
    assert_eq!(run(&rm, &["x"]).kind_of(&rm, 0), Kind::Remove);
    assert_eq!(run(&rm, &["-r", "x"]).kind_of(&rm, 0), Kind::RemoveTree);
}

#[test]
fn questions_fail_with_3() {
    assert_eq!(failure_code(&d(SEARCH)), exit::QUESTION_FAILED);
    assert_eq!(failure_code(&d(CP)), exit::FAILURE);
}

/// `NAME: SUBJECT: PROBLEM` or `NAME: PROBLEM`, lower case, no final period,
/// no CamelCase status name (INV-0155-3).
fn well_formed(line: &str) -> bool {
    let Some((name, rest)) = line.split_once(": ") else {
        return false;
    };
    let camel = rest.split(|c: char| !c.is_ascii_alphabetic()).any(|w| {
        w.chars().any(|c| c.is_ascii_lowercase())
            && w.chars().skip(1).any(|c| c.is_ascii_uppercase())
    });
    let problem = rest.rsplit(": ").next().unwrap_or("");
    !name.is_empty()
        && !name.contains(' ')
        && !problem.is_empty()
        && !problem.ends_with('.')
        && problem
            .chars()
            .next()
            .is_some_and(|c| !c.is_ascii_uppercase())
        && !camel
        && !problem.chars().all(|c| c.is_ascii_digit())
}

#[test]
fn every_status_reads_as_words() {
    for status in Status::ALL {
        let p = status_problem(*status);
        let line = error("cat", Some("notes.txt"), &p, &[]);
        assert!(well_formed(line.trim_end()), "{line:?}");
        assert!(!line.contains(&format!("{status:?}")) || p.split(' ').count() == 1);
        assert!(well_formed(error("sync", None, &p, &[]).trim_end()));
    }
    for raw in (0..=20).chain([FILE_GONE, u32::MAX]) {
        let p = file_problem(raw);
        let line = error("cat", Some("x"), &p, &[]);
        assert!(well_formed(line.trim_end()), "{line:?}");
    }
    assert_eq!(file_problem(file::NOT_FOUND), "not found");
    assert_eq!(file_problem(file::ACCESS_DENIED), "not granted");
    assert_eq!(status_problem(Status::AccessDenied), "not granted");
    assert_eq!(status_problem(Status::BadState), "bad state");
    assert_eq!(
        error(
            "cp",
            Some("b.txt"),
            "already exists",
            &["cp --force a.txt b.txt replaces it"]
        ),
        "cp: b.txt: already exists\n  hint: cp --force a.txt b.txt replaces it\n"
    );
}

#[test]
fn usage_problems_are_well_formed() {
    let rm = d(RM);
    for args in [
        &[][..],
        &["--nope"],
        &["-z"],
        &["--lines"],
        &["--verbose=1", "x"],
    ] {
        let e = parse(&rm, args).unwrap_err();
        let first = usage_error(&rm, &e);
        let first = first.lines().next().unwrap();
        assert!(well_formed(first), "{first}");
    }
}

/// INV-0156-3, the rendering half: `--help`, the page and completion list
/// the same options.
#[test]
fn help_page_and_completion_agree() {
    for text in [CP, RM, SEARCH] {
        let d = d(text);
        let longs: Vec<String> = d.options.iter().map(|o| format!("--{}", o.long)).collect();
        let help = help(&d);
        let page = page(&d, "");
        let completed = completion_options(&d);
        for long in &longs {
            assert!(help.contains(long.as_str()), "{long}");
            assert!(page.contains(long.as_str()), "{long}");
            assert!(completed.iter().any(|(l, _)| l == long), "{long}");
        }
        // Every option once, and --help once, in --help and in completion.
        assert_eq!(
            help.lines().filter(|l| l.starts_with("  -")).count(),
            longs.len() + 1
        );
        assert_eq!(completed.len(), longs.len() + 1);
    }
}

#[test]
fn closed_output_ends_quietly() {
    assert_eq!(output_failed(Status::PeerClosed), Some(0));
    assert_eq!(output_failed(Status::NoMemory), None);
}

/// Mutation testing: a letter option with no digit option beside it, and
/// a long option's value that would be `--`.
#[test]
fn letters_are_options_and_dash_dash_is_no_value() {
    let cp = d(CP);
    let p = run(&cp, &["-f", "a", "b"]);
    assert!(p.flag(&cp, "force"));
    assert_eq!(p.operand(&cp, "FROM"), Some("a"));
    assert_eq!(
        usage(&d(RM), &["--lines", "--", "x"]),
        "--lines: needs a value"
    );
}

/// Mutation testing: a page with operands only, with options only, and
/// with settings.
#[test]
fn a_page_has_what_its_description_has() {
    let search = page(&d(SEARCH), "");
    assert!(
        search.contains("\n  TEXT   what to look for\n  PATH   where to look (read)\n"),
        "{search}"
    );
    let v = page(
        &d("name v\nsummary s\nusage v\noption -v --verbose say more\n"),
        "",
    );
    assert_eq!(v, "V\n\ns\n\nusage: v\n\n  -v, --verbose   say more\n");
    let none = page(&d("name n\nsummary s\nusage n\n"), "");
    assert_eq!(none, "N\n\ns\n\nusage: n\n", "no blank line for nothing");
    let x = page(
        &d("name x\nsummary s\nusage x\nsetting shell.size number 0 100000 1000 lines kept\n"),
        "",
    );
    assert!(
        x.ends_with(
            "usage: x\n\nSETTINGS\n  shell.size  number 0 100000, default 1000: lines kept\n"
        ),
        "{x}"
    );
}

/// Mutation testing: the statuses with wording of their own.
#[test]
fn the_fixed_status_wordings_are_exact() {
    for (status, words) in [
        (Status::NotFound, "not found"),
        (Status::AccessDenied, "not granted"),
        (Status::NoMemory, "out of memory"),
        (Status::LimitReached, "a limit was reached"),
        (Status::DeadlineExceeded, "took too long"),
        (Status::PeerClosed, "the other end is gone"),
        (Status::BadHandle, "not a usable handle"),
        (Status::InvalidArgument, "not a valid argument"),
        (Status::NotSupported, "not supported by this machine"),
        (Status::OverQuota, "over quota"),
    ] {
        assert_eq!(status_problem(status), words);
    }
}
