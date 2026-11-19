//! Host tests of the grant planner (ADR-0153; INV-0153-2, -3, -4, -8, -9).

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_command::{Outcome, Parsed};
use zoraya_describe::Description;
use zoraya_proto_file::rights;

use crate::eval::Value;
use crate::grants::{
    Access, Line, Planned, Refused, merge, plan, report, shown, summary, through_links,
    trusted_description,
};

const CP: &str = "name cp\nsummary copy a file\nusage cp [--force] FROM TO\noperand FROM read the file\noperand TO create the copy\noption -f --force replace\n";
const RM: &str = "name rm\nsummary remove files\nusage rm [--recursive] PATH...\noperand PATH... remove the files\noption -r --recursive all in it\nwhen --recursive PATH remove-tree\n";
const MV: &str =
    "name mv\nsummary move\nusage mv FROM TO\noperand FROM rename a\noperand TO rename b\n";
const TCC: &str = "name tcc\nsummary compile\nusage tcc FILE -o OUT\noperand FILE read the source\noption -o --output OUT create the program\n";
const ECHO: &str =
    "name echo-args\nsummary print\nusage echo-args [W...]\noperand [W...] text words\ncwd none\n";

fn d(text: &str) -> Description {
    zoraya_describe::parse(text.as_bytes()).unwrap()
}

/// Plans `args` (all literal unless named in `from_values`) from `cwd`.
fn go(
    desc: &str,
    args: &[&str],
    from_values: &[&str],
    grants: &[(bool, &str)],
    base: bool,
    cwd: &str,
) -> Result<Vec<Planned>, crate::grants::Refused> {
    let d = d(desc);
    let values: Vec<Value> = args
        .iter()
        .map(|a| Value {
            text: (*a).into(),
            literal: !from_values.contains(a),
        })
        .collect();
    let texts: Vec<&str> = values.iter().map(|v| v.text.as_str()).collect();
    let parsed: Parsed<'_> = match zoraya_command::parse(&d, &texts) {
        Ok(Outcome::Run(p)) => p,
        other => panic!("{other:?}"),
    };
    let grants: Vec<(bool, Value)> = grants
        .iter()
        .map(|(w, p)| (*w, Value::literal(p)))
        .collect();
    plan(&Line {
        description: &d,
        parsed: &parsed,
        args: &values,
        cwd,
        grants: &grants,
        nets: &[],
        listens: &[],
        socks: &[],
        base_system: base,
        room: None,
    })
}

fn rows(p: &[Planned]) -> Vec<(String, Access, String)> {
    p.iter()
        .map(|e| (e.path.clone(), e.access.clone(), e.reason.clone()))
        .collect()
}

#[test]
fn the_tours_example_14() {
    let p = go(
        CP,
        &["notes.txt", "project/notes.txt"],
        &[],
        &[],
        true,
        "/u",
    )
    .unwrap();
    assert_eq!(
        report("cp", &p, "/u", "the terminal", false, false),
        "cp gets:
  in, out, err         the terminal
  ~                    read (current directory)
  ~/notes.txt          read (operand FROM)
  ~/project            create or replace notes.txt (operand TO)
"
    );
}

#[test]
fn every_kind_gives_its_entries_and_rights() {
    let p = go(RM, &["a", "b/c"], &[], &[], true, "/w").unwrap();
    assert_eq!(
        rows(&p)[1..],
        [
            (
                "/w".into(),
                Access::Remove("a".into()),
                "operand PATH".into()
            ),
            (
                "/w/b".into(),
                Access::Remove("c".into()),
                "operand PATH".into()
            ),
        ]
    );
    let p = go(RM, &["-r", "build"], &[], &[], true, "/w").unwrap();
    assert_eq!(
        rows(&p)[1..],
        [
            (
                "/w".into(),
                Access::Remove("build".into()),
                "operand PATH".into()
            ),
            ("/w/build".into(), Access::Tree, "operand PATH".into()),
        ]
    );
    let p = go(MV, &["a", "../x/b"], &[], &[], true, "/w/s").unwrap();
    assert_eq!(p[1].access, Access::Rename("a".into()));
    assert_eq!(p[2].path, "/w/x", "joined and normalised (rule 6)");
    let p = go(TCC, &["main.c", "-o", "main"], &[], &[], true, "/w").unwrap();
    assert_eq!(p[2].reason, "option -o");
    assert_eq!(p[2].access, Access::Create("main".into()));
    // Rights: each kind exactly its row of rule 2's table.
    let r = |a: &Access| a.rights();
    assert_eq!(r(&Access::Read), rights::READ | rights::CLONE);
    assert_eq!(r(&Access::Remove("x".into())) & rights::WRITE, 0);
    assert_eq!(
        r(&Access::Create("x".into())) & rights::REMOVE,
        0,
        "no delete through create"
    );
    assert_eq!(r(&Access::Tree) & rights::PUBLISH, 0);
    // No cwd for a command that reads no files.
    assert!(go(ECHO, &["x"], &[], &[], false, "/w").unwrap().is_empty());
}

/// INV-0153-9: only base-system descriptions designate beyond `read`.
#[test]
fn other_descriptions_give_at_most_read() {
    let p = go(CP, &["notes.txt", "copy.txt"], &[], &[], false, "/w").unwrap();
    assert_eq!(p.len(), 2);
    assert!(p.iter().all(|e| e.access == Access::Read));
    let p = go(
        CP,
        &["notes.txt", "copy.txt"],
        &[],
        &[(true, "/w")],
        false,
        "/w",
    )
    .unwrap();
    let last = p.last().expect("the grant");
    assert_eq!(last.access, Access::Tree, "+w is how a person gives more");
}

/// INV-0153-3 and rule 4: an operand from a value designates only inside the
/// current directory or a grant on the line.
#[test]
fn values_reach_only_inside_the_directory_or_a_grant() {
    for path in ["/config/settings", "../x", "/x"] {
        let e = go(RM, &[path], &[path], &[], true, "/u/proj").unwrap_err();
        assert_eq!(e.problem, "from a value, outside the current directory");
        assert!(e.hint.unwrap().starts_with("if that is meant, write +w "));
    }
    assert!(go(RM, &["sub/x"], &["sub/x"], &[], true, "/u/proj").is_ok());
    assert!(
        go(
            RM,
            &["/config/x"],
            &["/config/x"],
            &[(true, "/config")],
            true,
            "/u/proj"
        )
        .is_ok()
    );
    // Written literally, it designates wherever it points.
    assert!(go(RM, &["/config/x"], &[], &[], true, "/u/proj").is_ok());
    let p = go(RM, &["/config/x"], &[], &[], true, "/u/proj").unwrap();
    assert_eq!(p[1].path, "/config");
}

/// Review S1: a value attached to its option (`-o$x`, `--output=$x`) is a
/// value, as one written apart is; S16: a value may not name the current
/// directory itself for a kind that narrows its parent; S10: the hint asks
/// `+r` for a read.
#[test]
fn values_inside_a_word_are_values() {
    for word in ["-o/config/settings", "--output=/config/settings"] {
        let e = go(TCC, &["a.c", word], &[word], &[], true, "/u/proj").unwrap_err();
        assert_eq!(
            e.problem, "from a value, outside the current directory",
            "{word}"
        );
        assert_eq!(
            e.hint.as_deref(),
            Some("if that is meant, write +w /config before the command")
        );
    }
    assert!(go(TCC, &["a.c", "-oout"], &["-oout"], &[], true, "/u/proj").is_ok());
    assert!(
        go(TCC, &["a.c", "-o/x"], &[], &[], true, "/u/proj").is_ok(),
        "typed: designates"
    );
    for v in [".", ""] {
        let e = go(RM, &[v], &[v], &[], true, "/u/proj");
        assert!(e.is_err(), "{v:?} names the current directory");
    }
    let cat = "name cat\nsummary c\nusage cat FILE...\noperand FILE... read f\n";
    let e = go(cat, &["/etc/x"], &["/etc/x"], &[], true, "/u/proj").unwrap_err();
    assert_eq!(
        e.hint.as_deref(),
        Some("if that is meant, write +r /etc/x before the command")
    );
}

#[test]
fn grants_are_entries_with_their_reason() {
    let p = go(
        TCC,
        &["main.c", "-o", "main"],
        &[],
        &[(false, "../include"), (true, "/tmp/w")],
        true,
        "/u/proj",
    )
    .unwrap();
    let reasons: Vec<(&str, &str)> = p
        .iter()
        .map(|e| (e.path.as_str(), e.reason.as_str()))
        .collect();
    assert_eq!(
        reasons,
        vec![
            ("/u/proj", "current directory"),
            ("/u/proj/main.c", "operand FILE"),
            ("/u/proj", "option -o"),
            ("/u/include", "+r"),
            ("/tmp/w", "+w"),
        ]
    );
}

/// Rule 2: one entry per path; one name stays narrowed, two names or a
/// narrowed name beside a whole entry fall back to the whole directory, and
/// `grants` says so (INV-0153-4).
/// `jobs`' words for what a job got (ADR-0158 rule 5): the entries of
/// `grants`, without the reasons, each once.
#[test]
fn a_jobs_summary_words_its_entries() {
    let p = go(
        CP,
        &["notes.txt", "project/notes.txt"],
        &[],
        &[],
        true,
        "/u",
    )
    .unwrap();
    assert_eq!(
        summary(&p, "/u"),
        "~: read, ~/project: create or replace notes.txt"
    );
    let p = go(CP, &["a.txt", "b.txt"], &[], &[], true, "/u").unwrap();
    assert_eq!(
        summary(&p, "/u"),
        "~: read, ~: create and change anything in"
    );
    assert_eq!(summary(&[], "/u"), "");
}

#[test]
fn entries_of_one_path_merge() {
    let p = go(CP, &["a.txt", "b.txt"], &[], &[], true, "/u").unwrap();
    let m = merge(&p);
    assert_eq!(m.len(), 1, "/u; /u/a.txt is inside it with no more rights");
    assert_eq!(m[0].name, None);
    assert_eq!(
        m[0].rights,
        Access::Read.rights() | Access::Create("b.txt".into()).rights()
    );
    assert_eq!(
        report("cp", &p, "/u", "the terminal", false, false),
        "cp gets:
  in, out, err         the terminal
  ~                    read (current directory)
  ~/a.txt              read (operand FROM)
  ~                    create and change anything in (operand TO)
"
    );
    let rm_d = "name rm\nsummary r\nusage rm PATH...\noperand PATH... remove x\ncwd none\n";
    let one = go(rm_d, &["x/a"], &[], &[], true, "/w").unwrap();
    assert_eq!(merge(&one)[0].name.as_deref(), Some("a"));
    let two = go(rm_d, &["x/a", "x/b"], &[], &[], true, "/w").unwrap();
    let m = merge(&two);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].name, None);
    assert!(m[0].lines.iter().all(|(w, _)| w == "remove anything in"));
}

/// Rule 5: a designated path through a link gets the target only inside
/// the current directory or a grant; otherwise the line is refused with
/// the path and the target; grants are followed; the current directory's
/// entry is left as it is.
#[test]
fn links_lead_only_inside_the_directory_or_a_grant() {
    let links = |p: &str| -> Option<String> {
        match p {
            "/u/w/in-link" => Some("/u/w/inside.txt".into()),
            "/u/w/out-link" => Some("/u/secret.txt".into()),
            "/u/w/sub" => Some("/u/w/real".into()),
            "/u/w/up" => Some("/u".into()),
            "/u/w/g" => Some("/u/shared".into()),
            "/u/w" => Some("/elsewhere".into()),
            _ => None,
        }
    };
    let cat = "name cat\nsummary c\nusage cat FILE...\noperand FILE... read f\n";
    let p = go(cat, &["in-link", "plain"], &[], &[], true, "/u/w").unwrap();
    let p = through_links(p, "/u/w", "/u", links).unwrap();
    assert_eq!(
        rows(&p),
        [
            ("/u/w".into(), Access::Read, "current directory".into()),
            (
                "/u/w/inside.txt".into(),
                Access::Read,
                "operand FILE, through a link at ~/w/in-link".into()
            ),
            ("/u/w/plain".into(), Access::Read, "operand FILE".into()),
        ]
    );
    let p = go(cat, &["out-link"], &[], &[], true, "/u/w").unwrap();
    assert_eq!(
        through_links(p, "/u/w", "/u", links),
        Err(Refused {
            subject: "~/w/out-link".into(),
            problem: "leads through a link to ~/secret.txt, outside the current directory and \
                      this line's grants"
                .into(),
            hint: Some("to give it, put +r ~/secret.txt before the command".into()),
        })
    );
    // A grant that covers the target allows it; a grant through a link
    // is its target.
    let p = go(cat, &["out-link"], &[], &[(false, "/u")], true, "/u/w").unwrap();
    let p = through_links(p, "/u/w", "/u", links).unwrap();
    assert!(p.iter().any(|e| e.path == "/u/secret.txt"), "{p:?}");
    let p = go(cat, &["/u/shared/x"], &[], &[(false, "g")], true, "/u/w").unwrap();
    let p = through_links(p, "/u/w", "/u", links).unwrap();
    assert!(
        p.iter().any(|e| e.path == "/u/shared" && e.reason == "+r"),
        "{p:?}"
    );
    // A narrowed entry's directory through a link; a write kind asks +w.
    let rm = "name rm\nsummary r\nusage rm PATH...\noperand PATH... remove x\n";
    let p = go(rm, &["sub/a"], &[], &[], true, "/u/w").unwrap();
    let p = through_links(p, "/u/w", "/u", links).unwrap();
    assert!(
        p.iter()
            .any(|e| e.path == "/u/w/real" && e.access == Access::Remove("a".into())),
        "{p:?}"
    );
    let p = go(rm, &["up/a"], &[], &[], true, "/u/w").unwrap();
    let r = through_links(p, "/u/w", "/u", links).unwrap_err();
    assert_eq!(
        r.hint.as_deref(),
        Some("to give it, put +w ~ before the command")
    );
    // The current directory's own entry is not followed.
    let p = go(cat, &["plain"], &[], &[], true, "/u/w").unwrap();
    let p = through_links(p, "/u/w", "/u", links).unwrap();
    assert_eq!(p[0].path, "/u/w");
}

/// INV-0153-6: only a description of the system image's, for the path it
/// names, applies to a POSIX program; `./tcc` or a copy gets none, and a
/// description that names no program never applies.
#[test]
fn only_the_images_description_for_its_path_applies() {
    let tcc =
        d("name tcc\nsummary c\nusage tcc FILE...\nprogram /bin/tcc\noperand FILE... read f\n");
    let make = d("name make\nsummary m\nusage make\nprogram /bin/make\n");
    let native = d("name cat\nsummary c\nusage cat FILE...\noperand FILE... read f\n");
    let trusted = [tcc, make, native];
    assert_eq!(
        trusted_description(&trusted, "/bin/tcc").map(|d| d.name.as_str()),
        Some("tcc")
    );
    for path in [
        "/u/w/tcc",
        "/bin/tcc2",
        "/usr/bin/tcc",
        "tcc",
        "/bin/cat",
        "",
    ] {
        assert!(trusted_description(&trusted, path).is_none(), "{path}");
    }
}

/// The tour's example 17: a POSIX program's report, in the line's order
/// (grants last), with the toolchain set.
#[test]
fn a_posix_programs_report_shows_its_toolchain_set() {
    let tcc = "name tcc\nsummary c\nusage tcc [-o FILE] FILE...\nprogram /bin/tcc\n\
               option -o --output FILE create  out\noperand FILE... read src\n";
    let p = go(
        tcc,
        &["main.c", "-o", "main"],
        &[],
        &[(false, "/u/include")],
        true,
        "/u/project",
    )
    .unwrap();
    assert_eq!(
        report("tcc", &p, "/u", "the terminal", true, true),
        "tcc gets (a POSIX program):
  in, out, err         the terminal
  keys                 the terminal, while this line is in front
  ~/project            read (current directory)
  ~/project/main.c     read (operand FILE)
  ~/project            create and change anything in (option -o)
  ~/include            read (+r)
  /bin /usr /etc       read (system)
  /proc/self/exe       read (system)
  /tmp                 change anything in (this line's own)
  /dev/null
"
    );
}

/// Review S3 and S9: an entry inside an undivided one gets its rights too,
/// a narrowed one inside a wider grant is undivided, and an entry that adds
/// nothing to one covering it is left out.
#[test]
fn entries_inside_others_take_their_rights() {
    let cat = "name cat\nsummary c\nusage cat FILE...\noperand FILE... read f\n";
    // `~/project> +w ~ cat x`: the current directory is writable too.
    let p = go(cat, &["x"], &[], &[(true, "/u")], true, "/u/project").unwrap();
    let m = merge(&p);
    assert_eq!(m.len(), 1, "{m:?}");
    assert_eq!(
        (m[0].path.as_str(), m[0].rights),
        ("/u", Access::Tree.rights())
    );
    // A narrowed remove inside `+w .` does not keep its sibling from it.
    let rm = "name rm\nsummary r\nusage rm PATH...\noperand PATH... remove x\n";
    let p = go(rm, &["sub/a"], &[], &[(true, ".")], true, "/u").unwrap();
    let m = merge(&p);
    assert!(m.iter().all(|e| e.name.is_none()), "{m:?}");
    assert!(
        m.iter()
            .all(|e| e.rights & Access::Tree.rights() == Access::Tree.rights()),
        "{m:?}"
    );
    // Seventy files read in the current directory: one entry.
    let names: Vec<String> = (0..70).map(|i| alloc::format!("f{i}.txt")).collect();
    let args: Vec<&str> = names.iter().map(String::as_str).collect();
    let p = go(cat, &args, &[], &[], true, "/u").unwrap();
    assert_eq!(merge(&p).len(), 1);
    // A narrowed create in another directory stays narrowed.
    let p = go(CP, &["a", "d/b"], &[], &[], true, "/u").unwrap();
    assert!(merge(&p).iter().any(|e| e.name.as_deref() == Some("b")));
}

/// Mutation testing: each kind's rights, exactly, and a path shown under
/// the home only when it is inside it.
#[test]
fn each_access_has_exactly_its_rights() {
    use rights::*;
    assert_eq!(Access::Read.rights(), READ | CLONE);
    assert_eq!(Access::Change.rights(), READ | WRITE | SYNC | CLONE);
    assert_eq!(
        Access::Create("x".into()).rights(),
        READ | WRITE | CREATE | SYNC | CLONE
    );
    assert_eq!(Access::Remove("x".into()).rights(), READ | REMOVE | CLONE);
    assert_eq!(
        Access::Rename("x".into()).rights(),
        READ | RENAME | CREATE | REMOVE | CLONE
    );
    assert_eq!(shown("/u/a", "/u"), "~/a");
    assert_eq!(shown("/uxy", "/u"), "/uxy");
    assert_eq!(shown("/abc", "/"), "/abc");
}

/// Mutation testing: an operand is an argument's when its bytes are inside
/// that argument's, by address; slices of one string fix the layout.
#[test]
fn within_is_by_address() {
    use crate::grants::within;
    let s = "a".repeat(1001);
    assert!(within(&s, &s[1..]));
    assert!(within(&s[..1], &s[..1]));
    assert!(within(&s[1..], &s[1..]));
    assert!(!within(&s[..1], &s[1..]), "the next bytes are not inside");
    assert!(!within(&s[1..], &s[..1]), "the bytes before are not inside");
    assert!(!within(&s[1..2], &s[1..3]));
    let copy = s.clone();
    assert!(!within(&copy, &s), "the same text elsewhere is not");
}

/// Mutation testing: a value of one character is an argument's too.
#[test]
fn a_one_character_argument_is_found() {
    assert!(go(RM, &["/"], &[], &[], true, "/u/proj").is_ok());
}

/// Mutation testing: how `merge` folds an entry into one that covers it.
#[test]
fn merge_folds_only_what_is_covered() {
    // A narrowed outer does not lend its rights to a plain inner entry.
    let p = go(CP, &["x/y/z", "x/y"], &[], &[], true, "/u").unwrap();
    assert_eq!(merge(&p).len(), 2, "{:?}", merge(&p));
    // An unrelated entry keeps its own rights.
    let p = go(CP, &["/w/f", "x"], &[], &[(true, "/u")], true, "/u").unwrap();
    let m = merge(&p);
    let f = m.iter().find(|e| e.path == "/w/f").expect("/w/f");
    assert_eq!(f.rights, Access::Read.rights(), "{m:?}");
    // Rights are joined, and only a covered entry with no more rights goes.
    let cat = "name cat\nsummary c\nusage cat FILE...\noperand FILE... read f\n";
    let p = go(
        cat,
        &["x"],
        &[],
        &[(false, "/u"), (true, "/u/d")],
        true,
        "/u/d",
    )
    .unwrap();
    let m = merge(&p);
    assert_eq!(m.len(), 2, "{m:?}");
    let d = m.iter().find(|e| e.path == "/u/d").expect("/u/d");
    assert_eq!(d.rights, Access::Tree.rights(), "{m:?}");
}

// `names OPTION OPERAND` (TASK-27 9a, proposed ADR-0311; INV-0311-1): curl's
// -O creates the file the URL's last segment names, in the current
// directory, and nothing else.
const CURL: &str = "name curl\nsummary fetch\nusage curl [-O] URL...\noperand URL... url the address\noption -o --output FILE create where\noption -O --remote-name save under the name of the address\nnames --remote-name URL\n";

#[test]
fn curl_o_creates_the_name_of_the_url_in_the_current_directory() {
    let p = go(
        CURL,
        &["-O", "https://x.example/a/b.txt"],
        &[],
        &[],
        true,
        "/u/p",
    )
    .unwrap();
    assert_eq!(
        rows(&p),
        vec![
            ("/u/p".into(), Access::Read, "current directory".into()),
            (
                "/u/p".into(),
                Access::Create("b.txt".into()),
                "option -O".into()
            ),
        ]
    );
}

#[test]
fn curl_o_with_two_urls_creates_two_names_and_without_o_none() {
    let p = go(
        CURL,
        &["-O", "http://h/one", "http://h/dir/two.html?x=1#f"],
        &[],
        &[],
        true,
        "/u",
    )
    .unwrap();
    let creates: Vec<&str> = p.iter().filter_map(|e| e.access.name()).collect();
    assert_eq!(creates, vec!["one", "two.html"]);
    let p = go(CURL, &["http://h/one"], &[], &[], true, "/u").unwrap();
    assert!(p.iter().all(|e| e.access.name().is_none()));
}

#[test]
fn curl_o_refuses_a_name_the_shell_cannot_be_sure_of() {
    for url in [
        "https://h/",
        "https://h",
        "https://h/a/",
        "https://h/a%2Fb",
        "https://h/a b",
        "https://h/..",
        "https://h/.",
        "https://h/a\u{e9}",
        "https://h/a;b",
    ] {
        let r = go(CURL, &["-O", url], &[], &[], true, "/u").unwrap_err();
        assert_eq!(r.subject, url, "{url}");
        assert!(r.problem.contains("cannot be sure"), "{url}: {}", r.problem);
        assert_eq!(
            r.hint.as_deref(),
            Some("to say where it goes, write -o FILE")
        );
    }
}

#[test]
fn curl_o_in_a_description_that_is_not_the_images_designates_nothing() {
    let p = go(CURL, &["-O", "https://h/b.txt"], &[], &[], false, "/u").unwrap();
    assert!(p.iter().all(|e| e.access == Access::Read));
}

// With `cwd none` (curl's and wget's descriptions) the one name is all the
// program is given of the current directory: the merge keeps the entry
// narrowed to it, and `-o`/`-O` together with the current directory's read
// is what would have folded into "create and change anything in" (ADR-0311
// item 5, INV-0311-1).
const CURL_NO_CWD: &str = "name curl\nsummary fetch\nusage curl [-O] URL...\noperand URL... url the address\noption -o --output FILE create where\noption -O --remote-name save under the name of the address\nnames --remote-name URL\ncwd none\n";

#[test]
fn curl_o_with_no_cwd_is_one_narrowed_entry_and_nothing_else_of_the_directory() {
    let p = go(
        CURL_NO_CWD,
        &["-O", "https://x.example/a/b.txt"],
        &[],
        &[],
        true,
        "/u/p",
    )
    .unwrap();
    assert_eq!(
        rows(&p),
        vec![(
            "/u/p".into(),
            Access::Create("b.txt".into()),
            "option -O".into()
        )]
    );
    let m = merge(&p);
    assert_eq!(m.len(), 1, "{m:?}");
    assert_eq!(m[0].path, "/u/p");
    assert_eq!(m[0].name.as_deref(), Some("b.txt"));
    assert_eq!(
        m[0].lines,
        vec![(
            "create or replace b.txt".to_string(),
            "option -O".to_string()
        )]
    );
    // `-o FILE` is the same.
    let p = go(
        CURL_NO_CWD,
        &["-o", "f.txt", "http://h/"],
        &[],
        &[],
        true,
        "/u/p",
    )
    .unwrap();
    let m = merge(&p);
    assert_eq!((m.len(), m[0].name.as_deref()), (1, Some("f.txt")));
}

const WGET_NO_CWD: &str = "name wget\nsummary get\nusage wget [-O FILE] URL...\noperand URL... url the address\noption -O --output-document FILE create where\ncwd none\n";

#[test]
fn wget_o_dash_is_the_terminal_and_creates_nothing() {
    // `-O -` is the standard output (review of 9a, N5).
    let p = go(
        WGET_NO_CWD,
        &["-O", "-", "http://h/"],
        &[],
        &[],
        true,
        "/u/p",
    )
    .unwrap();
    assert!(p.is_empty(), "{p:?}");
    // A file named otherwise is still one narrowed entry.
    let p = go(
        WGET_NO_CWD,
        &["-O", "x", "http://h/"],
        &[],
        &[],
        true,
        "/u/p",
    )
    .unwrap();
    assert_eq!(p.len(), 1);
}

#[test]
fn curl_o_below_the_directory_and_data_from_a_file_needs_a_grant() {
    // W1: `-o sub/f` gives the path below the directory, nothing at it; a
    // `-d @x` file is not given by the plan, so zosh refuses it unless a
    // `+r` of the line covers it. The plan holds one narrowed entry.
    let p = go(
        CURL_NO_CWD,
        &["-o", "sub/f", "http://h/"],
        &[],
        &[],
        true,
        "/u/p",
    )
    .unwrap();
    assert_eq!(
        rows(&p),
        vec![(
            "/u/p/sub".into(),
            Access::Create("f".into()),
            "option -o".into()
        )]
    );
    assert!(
        p.iter()
            .all(|e| e.access != Access::Read && e.path != "/u/p")
    );
}

fn read_of(path: &str) -> Planned {
    Planned {
        path: path.into(),
        access: Access::Read,
        reason: String::from("test"),
    }
}

/// INV-0317-1, the host half: the window's pills say every grant, the
/// network's first.
#[test]
fn window_pills_list_the_network_first_then_the_files() {
    let plan = vec![read_of("/a"), read_of("/b")];
    let net = vec![String::from("+net example.org:443")];
    assert_eq!(
        crate::grants::window_pills(&plan, &net).unwrap(),
        vec!["+net example.org:443", "+r /a", "+r /b"]
    );
    // The same path twice is one pill.
    let twice = vec![read_of("/a"), read_of("/a")];
    assert_eq!(
        crate::grants::window_pills(&twice, &[]).unwrap(),
        vec!["+r /a"]
    );
}

/// INV-0317-1: more pills than the window shows is a refusal, never a window
/// that understates its grants (the net pills do not fall off the end).
#[test]
fn window_pills_refuse_more_than_sixteen() {
    let max = crate::grants::MAX_WINDOW_PILLS;
    let files: Vec<Planned> = (0..max).map(|i| read_of(&format!("/d{i}"))).collect();
    assert_eq!(crate::grants::window_pills(&files, &[]).unwrap().len(), max);
    let net = vec![String::from("+net h")];
    assert_eq!(crate::grants::window_pills(&files, &net), Err(max + 1));
    let fewer: Vec<Planned> = files[..max - 1].to_vec();
    let pills = crate::grants::window_pills(&fewer, &net).unwrap();
    assert_eq!((pills.len(), pills[0].as_str()), (max, "+net h"));
}

/// INV-0326-1, the shell's half: a description with `window` gives
/// `/system/fonts` read only, and nothing else does.
#[test]
fn the_fonts_come_with_a_window_read_only_and_only_with_it() {
    let netsurf = d(include_str!("../../../user/describe/posix/netsurf.cmd"));
    let fonts = crate::grants::with_window(&netsurf).expect("netsurf has a window");
    assert_eq!(fonts.path, "/system/fonts");
    assert_eq!(fonts.access, Access::Read);
    assert_eq!(fonts.reason, "with the window");
    // The pill says read, never write.
    assert_eq!(
        crate::grants::window_pills(&[fonts], &[]).unwrap(),
        vec!["+r /system/fonts"]
    );
    for text in [
        include_str!("../../../user/describe/posix/curl.cmd"),
        include_str!("../../../user/describe/posix/tcc.cmd"),
        include_str!("../../../user/describe/posix/python3.cmd"),
    ] {
        let plain = d(text);
        assert!(plain.window.is_none());
        assert_eq!(crate::grants::with_window(&plain), None, "{}", plain.name);
    }
}

// ADR-0336 item 6, INV-0336-6: a file operand of a `cwd none` description
// arrives as its directory narrowed to the name, for `sed -i` (write) and for
// `sed` without it (read); the directory's other names are not given.
const SED: &str = "name sed\nsummary edit\nusage sed [-i] SCRIPT [FILE...]\noperand SCRIPT text the script\noperand [FILE...] read the files\noption -i --in-place change the files in place\nwhen -i FILE write\ncwd none\n";

#[test]
fn a_file_of_a_cwd_none_program_is_given_as_one_narrowed_name() {
    let p = go(SED, &["-i", "s/a/b/", "notes.txt"], &[], &[], true, "/w").unwrap();
    let m = merge(&p);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].path, "/w");
    assert_eq!(m[0].name.as_deref(), Some("notes.txt"));
    assert_eq!(m[0].rights & zoraya_proto_file::rights::CREATE, 0);
    assert_ne!(m[0].rights & zoraya_proto_file::rights::WRITE, 0);
    assert_eq!(m[0].lines[0].0, "change notes.txt");

    let r = go(SED, &["s/a/b/", "notes.txt"], &[], &[], true, "/w").unwrap();
    let m = merge(&r);
    assert_eq!(m[0].name.as_deref(), Some("notes.txt"));
    assert_eq!(m[0].rights & zoraya_proto_file::rights::WRITE, 0);
    assert_eq!(m[0].lines[0].0, "read notes.txt");
    // Not the current directory.
    assert!(r.iter().all(|e| e.reason != "current directory"));
}

#[test]
fn two_files_of_one_directory_fall_back_to_the_directory_in_words() {
    use zoraya_proto_file::rights;
    let p = go(SED, &["-i", "x", "a", "b"], &[], &[], true, "/w").unwrap();
    let m = merge(&p);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].name, None);
    assert_eq!(m[0].lines.len(), 2, "{m:?}");
    assert!(
        m[0].lines
            .iter()
            .all(|(w, _)| w == "read and change anything in")
    );
    // The fallback is the union of the parts' rights: no more.
    assert_eq!(
        m[0].rights,
        Access::ChangeName("a".into()).rights(),
        "{m:?}"
    );
    assert_eq!(
        m[0].rights & (rights::CREATE | rights::REMOVE | rights::RENAME),
        0
    );
}

// INV-0336-6's other half: a file outside the current directory is its own
// directory narrowed to the name, and the current directory is not given.
#[test]
fn a_file_outside_the_current_directory_is_its_directory_narrowed() {
    let p = go(SED, &["-i", "x", "/x/notes.txt"], &[], &[], true, "/w").unwrap();
    let m = merge(&p);
    assert_eq!(m.len(), 1, "{m:?}");
    assert_eq!(m[0].path, "/x");
    assert_eq!(m[0].name.as_deref(), Some("notes.txt"));
    assert!(p.iter().all(|e| e.path != "/w"), "{p:?}");
    let p = go(SED, &["x", "../y/a.txt"], &[], &[], true, "/w/sub").unwrap();
    assert_eq!(
        rows(&p),
        [(
            "/w/y".into(),
            Access::ReadName("a.txt".into()),
            "operand FILE".into()
        )]
    );
}

// Review of 10e, finding 6 (S16's rule for `cwd none` files): a value naming
// the current directory itself would give its parent narrowed to its name,
// so it is refused; a value naming a file in it is allowed.
#[test]
fn a_cwd_none_value_naming_the_current_directory_is_refused() {
    let r = go(SED, &["-i", "x", "."], &["."], &[], true, "/u/proj").unwrap_err();
    assert_eq!(r.problem, "from a value, outside the current directory");
    let r = go(SED, &["x", "/u/proj"], &["/u/proj"], &[], true, "/u/proj").unwrap_err();
    assert_eq!(r.subject, "/u/proj");
    let p = go(SED, &["-i", "x", "f.txt"], &["f.txt"], &[], true, "/u/proj").unwrap();
    assert_eq!(
        rows(&p),
        [(
            "/u/proj".into(),
            Access::ChangeName("f.txt".into()),
            "operand FILE".into()
        )]
    );
    // Written literally, `.` is what the person asked for: the parent
    // narrowed to the directory's name, which the service opens read-only.
    let p = go(SED, &["x", "."], &[], &[], true, "/u/proj").unwrap();
    assert_eq!(p[0].path, "/u");
}

// Review of 10e, finding 7 (ADR-0153 rule 5): a narrowed name that is itself
// a link is followed to its target, which must lie strictly below the
// current directory or a grant; a link outside is refused with the hint; a
// dangling one inside is the target's name; `remove` keeps the link.
#[test]
fn a_narrowed_name_that_is_a_link_is_followed() {
    let links = |p: &str| -> Option<String> {
        match p {
            "/w/notes.txt" => Some("/w/drafts/notes.txt".into()),
            "/w/out.txt" => Some("/secret/out.txt".into()),
            "/w/gone.txt" => Some("/w/new/gone.txt".into()),
            "/w/self" => Some("/w".into()),
            _ => None,
        }
    };
    let p = go(SED, &["-i", "x", "notes.txt"], &[], &[], true, "/w").unwrap();
    let p = through_links(p, "/w", "/u", links).unwrap();
    assert_eq!(
        rows(&p),
        [
            (
                "/w/drafts".into(),
                Access::ChangeName("notes.txt".into()),
                "operand FILE, through a link at /w/notes.txt".into()
            ),
            // The link's own name, so the open meets the link (check of the
            // fix pass, N1).
            (
                "/w".into(),
                Access::ChangeName("notes.txt".into()),
                "operand FILE, the link itself".into()
            ),
        ]
    );
    let p = go(SED, &["x", "out.txt"], &[], &[], true, "/w").unwrap();
    let r = through_links(p, "/w", "/u", links).unwrap_err();
    assert_eq!(r.subject, "/w/out.txt");
    assert_eq!(
        r.hint.as_deref(),
        Some("to give it, put +r /secret/out.txt before the command")
    );
    let p = go(SED, &["-i", "x", "out.txt"], &[], &[], true, "/w").unwrap();
    let r = through_links(p, "/w", "/u", links).unwrap_err();
    assert_eq!(
        r.hint.as_deref(),
        Some("to give it, put +w /secret/out.txt before the command")
    );
    // Allowed when a grant covers the target.
    let p = go(
        SED,
        &["x", "out.txt"],
        &[],
        &[(false, "/secret")],
        true,
        "/w",
    )
    .unwrap();
    let p = through_links(p, "/w", "/u", links).unwrap();
    assert!(
        p.iter()
            .any(|e| e.path == "/secret" && e.access == Access::ReadName("out.txt".into())),
        "{p:?}"
    );
    // A `create` name acts on the link object (`ln -sf`, `mv` onto a
    // link): it is not followed, inside or outside, and a dangling one is
    // kept as written (check of the fix pass, N2).
    let ln = "name ln\nsummary l\nusage ln [-sf] TARGET LINK\noperand TARGET text t\noperand LINK create l\noption -s --symbolic s\noption -f --force f\n";
    for link in ["notes.txt", "out.txt", "gone.txt"] {
        let p = go(ln, &["-sf", "new", link], &[], &[], true, "/w").unwrap();
        let p = through_links(p, "/w", "/u", links).unwrap();
        assert!(
            p.iter()
                .any(|e| e.path == "/w" && e.access == Access::Create(link.into())),
            "{p:?}"
        );
    }
    let p = go(MV, &["a", "out.txt"], &[], &[], true, "/w").unwrap();
    assert!(through_links(p, "/w", "/u", links).is_ok());
    let p = go(CP, &["a", "gone.txt"], &[], &[], true, "/w").unwrap();
    let p = through_links(p, "/w", "/u", links).unwrap();
    assert!(
        p.iter()
            .any(|e| e.path == "/w" && e.access == Access::Create("gone.txt".into())),
        "{p:?}"
    );
    // A link to the current directory itself is not a name below it.
    let p = go(SED, &["x", "self"], &[], &[], true, "/w").unwrap();
    assert!(through_links(p, "/w", "/u", links).is_err());
    // `rm` removes the link, not its target.
    let rm = "name rm\nsummary r\nusage rm PATH...\noperand PATH... remove x\n";
    let p = go(rm, &["notes.txt"], &[], &[], true, "/w").unwrap();
    let p = through_links(p, "/w", "/u", links).unwrap();
    assert!(
        p.iter()
            .any(|e| e.path == "/w" && e.access == Access::Remove("notes.txt".into())),
        "{p:?}"
    );
}

/// INV-0331-1: `grants` prints the `keys` line exactly when any of the three
/// fds is the terminal, for every combination of redirections.
#[test]
fn the_keys_line_is_there_exactly_when_a_stream_is_the_terminal() {
    use crate::syntax::Stream;
    let words = crate::terminal::KEYS_WORDS;
    let all = [Stream::In, Stream::Out, Stream::Err];
    for mask in 0..8u32 {
        let redirects: Vec<Stream> = all
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, s)| *s)
            .collect();
        let tty = crate::terminal::terminal_fds(0, 1, true, false, true, true, false, &redirects);
        let keys = tty.contains(&true);
        let text = report(
            "sh",
            &[],
            "/u",
            &crate::terminal::streams_words(tty),
            true,
            keys,
        );
        assert_eq!(text.contains("keys "), keys, "{redirects:?}");
        assert_eq!(text.contains(words), keys, "{redirects:?}");
        // The terminal's entry is inside the system's `/usr`.
        assert!(text.contains("/bin /usr /etc"));
    }
    // A program whose three streams are all elsewhere has no keys.
    assert!(
        !report("sh", &[], "/u", "none", true, false).contains("keys"),
        "no terminal, no keys"
    );
}

// ------------------------------------------------------------ ADR-0341

const SSH_CMD: &str = include_str!("../../../user/describe/posix/ssh.cmd");
const KEYGEN_CMD: &str = include_str!("../../../user/describe/posix/ssh-keygen.cmd");
const CAT_CMD: &str = include_str!("../../../user/describe/commands/cat.cmd");
const LS_CMD: &str = include_str!("../../../user/describe/commands/ls.cmd");

/// INV-0341-1: only an image description's `data` line, for a program the
/// list gives that name, makes an entry.
#[test]
fn only_an_image_description_with_a_listed_program_has_an_own_directory() {
    use crate::grants::own_directory;
    let ssh = d(SSH_CMD);
    let own = own_directory(&ssh, true, None).expect("an own directory");
    assert_eq!(own.real, "/data/home/ssh");
    assert_eq!(own.view, "/home/.ssh");
    assert!(!own.read);
    // A description from a volume (not the image's): none.
    assert!(own_directory(&ssh, false, None).is_none());
    // A copy of the program under another name: the description names a
    // path, and `trusted_description` gives none for another one.
    let trusted = [ssh.clone()];
    assert!(trusted_description(&trusted, "/u/ssh").is_none());
    assert!(trusted_description(&trusted, "/bin/ssh").is_some());
    // A description that claims the name but whose program the list does
    // not give it to: none.
    let thief = d("name evil\nsummary s\nusage evil\nprogram /bin/evil\ndata ssh as ~/.ssh\n");
    assert!(own_directory(&thief, true, None).is_none());
    let unlisted = d("name ssh\nsummary s\nusage ssh\nprogram /bin/ssh\ndata other as ~/.other\n");
    assert!(own_directory(&unlisted, true, None).is_none());
    // No data line, none.
    assert!(own_directory(&d(CAT_CMD), true, None).is_none());
}

/// INV-0341-2: the directory is `/data/ROOM/NAME` of the line's room and the
/// description's name, and the same name in two rooms gives two directories.
#[test]
fn the_own_directory_is_per_room_and_name() {
    use crate::grants::own_directory;
    let ssh = d(SSH_CMD);
    let keygen = d(KEYGEN_CMD);
    let mut seen = Vec::new();
    for room in [None, Some("proj"), Some("ops"), Some("home2")] {
        let a = own_directory(&ssh, true, room).unwrap();
        let b = own_directory(&keygen, true, room).unwrap();
        // ssh and ssh-keygen share a directory in a room, by name.
        assert_eq!(a.real, b.real);
        assert_eq!(
            a.real,
            alloc::format!("/data/{}/ssh", room.unwrap_or("home"))
        );
        assert_eq!(a.view, "/home/.ssh");
        assert!(!seen.contains(&a.real), "{} twice", a.real);
        seen.push(a.real);
    }
    // `read` is the description's.
    let reader = d("name scp\nsummary s\nusage scp\nprogram /bin/scp\ndata ssh as ~/.ssh read\n");
    assert!(reader.data.as_ref().unwrap().read);
}

/// INV-0341-7: with the current directory at `/`, `/data`, a room's
/// directory or inside a program's own directory, no current-directory entry
/// is given, with or without an operand; a literal operand still designates.
#[test]
fn the_current_directory_does_not_reach_into_data() {
    use crate::grants::cwd_reaches_data;
    let cat = |cwd: &str, args: &[&str]| go(CAT_CMD, args, &[], &[], true, cwd).unwrap();
    for cwd in [
        "/",
        "/data",
        "/data/home",
        "/data/proj",
        "/data/home/ssh",
        "/data/home/ssh/sub",
        "/data/proj/ssh/x/y",
    ] {
        assert!(cwd_reaches_data(cwd), "{cwd}");
        assert!(cat(cwd, &[]).is_empty(), "{cwd}: no operand");
        assert!(
            cat(cwd, &["x"])
                .iter()
                .all(|e| e.reason != "current directory"),
            "{cwd}: an operand"
        );
        let ls = go(LS_CMD, &[], &[], &[], true, cwd).unwrap();
        assert!(ls.iter().all(|e| e.reason != "current directory"), "{cwd}");
    }
    // Anywhere else it is given, as before.
    for cwd in [
        "/u",
        "/home",
        "/data2",
        "/database",
        "/data/home/other",
        "/tmp/data",
    ] {
        assert!(!cwd_reaches_data(cwd), "{cwd}");
        let p = cat(cwd, &[]);
        assert_eq!(p.len(), 1, "{cwd}");
        assert_eq!(p[0].reason, "current directory");
    }
    // A literal operand names it: the one way in.
    let p = cat("/data/home/ssh", &["/data/home/ssh/known_hosts"]);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].path, "/data/home/ssh/known_hosts");
    assert_eq!(p[0].access, Access::Read);
    // ... but a value inside the current directory is not covered by it.
    let r = go(
        CAT_CMD,
        &["known_hosts"],
        &["known_hosts"],
        &[],
        true,
        "/data/home/ssh",
    );
    assert!(r.unwrap_err().problem.contains("from a value"));
    // A `+r` word names it too.
    let p = go(
        CAT_CMD,
        &["known_hosts"],
        &["known_hosts"],
        &[(false, "/data/home/ssh")],
        true,
        "/data/home/ssh",
    )
    .unwrap();
    assert!(
        p.iter()
            .any(|e| e.reason == "+r" && e.path == "/data/home/ssh")
    );
    // An undescribed program (`cwd read`, not the image's): none either.
    let p = go(ECHO, &[], &[], &[], false, "/data/home/ssh").unwrap();
    assert!(p.is_empty());
}

/// ADR-0341 item 3: a grant on the program's own directory is refused.
#[test]
fn a_grant_cannot_name_the_own_directory() {
    use crate::grants::{own_conflict, own_directory};
    let own = own_directory(&d(SSH_CMD), true, None).unwrap();
    let w = |p: &str| vec![(true, Value::literal(p))];
    assert!(own_conflict(&own, "/u", &w("/u/other"), &[]).is_ok());
    assert!(own_conflict(&own, "/u", &w("/home"), &[]).is_ok());
    for p in [
        "/home/.ssh",
        "/home/.ssh/known_hosts",
        "../home/.ssh",
        ".ssh/x",
    ] {
        let cwd = if p.starts_with("..") {
            "/u"
        } else if p.starts_with(".ssh") {
            "/home"
        } else {
            "/u"
        };
        let r = own_conflict(&own, cwd, &w(p), &[]).unwrap_err();
        assert!(r.problem.contains("own directory"), "{p}: {}", r.problem);
    }
}

/// INV-0341-3: `grants` prints the entry with the real path and the path
/// the program sees, in the line's order before the toolchain set.
#[test]
fn grants_shows_the_own_directory_in_both_names() {
    use crate::grants::{own_directory, report_with};
    let own = own_directory(&d(SSH_CMD), true, Some("proj")).unwrap();
    let p = go(SSH_CMD, &["10.0.2.50"], &[], &[], true, "/u").unwrap();
    let text = report_with("ssh", &p, "/u", "the terminal", true, true, Some(&own));
    assert!(
        text.contains("  /data/proj/ssh       its own files (it sees them as ~/.ssh)\n"),
        "{text}"
    );
    let own_at = text.find("/data/proj/ssh").unwrap();
    assert!(own_at < text.find("/bin /usr /etc").unwrap(), "{text}");
    // Without one, as before.
    let plain = report_with("ssh", &p, "/u", "the terminal", true, true, None);
    assert!(!plain.contains("its own files"));
    assert_eq!(plain, report("ssh", &p, "/u", "the terminal", true, true));
    let read = d("name scp\nsummary s\nusage scp\nprogram /bin/scp\ndata ssh as ~/.ssh read\n");
    let mut own = own;
    own.read = read.data.unwrap().read;
    assert!(crate::grants::own_words(&own).contains("to read"));
}

/// ADR-0340 item 4: `--` goes before HOST, whatever options come first, so
/// that ssh's own option reading ends there; the far side's words follow.
#[test]
fn the_shell_ends_ssh_s_options_before_the_host() {
    use crate::grants::end_of_options_at;
    let ssh = d(SSH_CMD);
    let at = |args: &[&str]| end_of_options_at(&ssh, args);
    assert_eq!(at(&["host"]), Some(0));
    assert_eq!(at(&["host", "ls", "-l"]), Some(0));
    assert_eq!(at(&["-l", "ada", "host", "-o", "x"]), Some(2));
    assert_eq!(
        at(&["-p", "22", "-l", "ada", "-v", "host", "-J", "x"]),
        Some(5)
    );
    assert_eq!(at(&["-vl", "ada", "ada@host", "id"]), Some(2));
    // Already there: not doubled.
    assert_eq!(at(&["-l", "ada", "--", "host", "ls"]), None);
    // A line that does not parse, or has no host, or another description.
    assert_eq!(at(&["-J", "x", "host"]), None);
    assert_eq!(at(&[]), None);
    assert_eq!(end_of_options_at(&d(CAT_CMD), &["x"]), None);
    // A same-looking word elsewhere is not the host: the host is the first
    // operand, by position.
    let words: Vec<String> = ["-l", "host", "host", "host"]
        .iter()
        .map(|w| String::from(*w))
        .collect();
    let refs: Vec<&str> = words.iter().map(String::as_str).collect();
    assert_eq!(at(&refs), Some(2));
}

// INV-0335-1 (the plan's half) and INV-0335-4: the real descriptions of the pager and the editor, ADR-0335 item 3.
const VI: &str = include_str!("../../../user/describe/posix/vi.cmd");
const EX: &str = include_str!("../../../user/describe/posix/ex.cmd");
const VIEW: &str = include_str!("../../../user/describe/posix/view.cmd");
const LESS: &str = include_str!("../../../user/describe/posix/less.cmd");

#[test]
fn vi_is_given_its_one_name_and_not_the_directory() {
    use zoraya_proto_file::rights;
    for desc in [VI, EX] {
        let p = go(desc, &["notes.txt"], &[], &[], true, "/w").unwrap();
        let m = merge(&p);
        assert_eq!(m.len(), 1, "{m:?}");
        assert_eq!(m[0].path, "/w");
        assert_eq!(m[0].name.as_deref(), Some("notes.txt"), "{m:?}");
        // To create or replace that name, and to read it first (ADR-0335 item 3).
        assert_ne!(m[0].rights & rights::READ, 0);
        assert_ne!(m[0].rights & rights::WRITE, 0);
        assert_ne!(m[0].rights & rights::CREATE, 0);
        // Nothing of the directory itself, whether or not it is the current one.
        assert!(p.iter().all(|e| e.reason != "current directory"), "{p:?}");
        // A file elsewhere is its own directory narrowed to the name.
        let q = go(desc, &["/x/other.txt"], &[], &[], true, "/w").unwrap();
        let m = merge(&q);
        assert_eq!(m.len(), 1, "{m:?}");
        assert_eq!(m[0].path, "/x");
        assert_eq!(m[0].name.as_deref(), Some("other.txt"));
        assert!(q.iter().all(|e| e.path != "/w"), "{q:?}");
    }
}

#[test]
fn vi_with_no_file_is_given_no_directory_and_a_read_only_name_is_read_only() {
    use zoraya_proto_file::rights;
    let p = go(VI, &[], &[], &[], true, "/w").unwrap();
    assert!(
        p.iter().all(|e| e.path != "/w"),
        "no file: the current directory is not given: {p:?}"
    );
    for (desc, args) in [
        (VI, &["-R", "notes.txt"][..]),
        (VIEW, &["notes.txt"][..]),
        (LESS, &["notes.txt"][..]),
    ] {
        let p = go(desc, args, &[], &[], true, "/w").unwrap();
        let m = merge(&p);
        assert_eq!(m.len(), 1, "{m:?}");
        assert_eq!(m[0].name.as_deref(), Some("notes.txt"));
        assert_ne!(m[0].rights & rights::READ, 0);
        assert_eq!(
            m[0].rights & (rights::WRITE | rights::CREATE | rights::REMOVE | rights::RENAME),
            0,
            "{m:?}"
        );
    }
}

#[test]
fn the_editor_and_the_pager_let_through_only_what_their_descriptions_list() {
    let d = |t| zoraya_describe::parse(t).unwrap();
    for (text, shorts) in [(VI, "Rc"), (EX, "Rsc"), (VIEW, "c"), (LESS, "FiINRSsX")] {
        let desc = d(text.as_bytes());
        let got: Vec<char> = desc.options.iter().filter_map(|o| o.short).collect();
        let mut want: Vec<char> = shorts.chars().collect();
        want.sort_unstable();
        let mut got = got;
        got.sort_unstable();
        assert_eq!(got, want, "{}", desc.name);
        assert_eq!(desc.cwd, zoraya_describe::Cwd::None, "{}", desc.name);
    }
}

// Review of 10d, findings 1, 2 and 11: a `+` word is never a file, two names of one directory are
// refused for the editor and the pager (with the way to say it), and `-R` in a cluster still reads.
#[test]
fn a_plus_word_is_refused_and_never_gives_the_directory() {
    for desc in [VI, EX, VIEW, LESS] {
        for words in [
            &["+10", "notes.txt"][..],
            &["notes.txt", "+G"][..],
            &["+", "notes.txt"][..],
        ] {
            let r = go(desc, words, &[], &[], true, "/w");
            let e = r.expect_err("a + word is refused");
            assert!(e.problem.contains("starts with +"), "{e:?}");
        }
    }
    // A file of that name is written as a path.
    let p = go(VI, &["./+10"], &[], &[], true, "/w").unwrap();
    assert_eq!(merge(&p)[0].name.as_deref(), Some("+10"));
}

#[test]
fn two_files_of_one_directory_are_refused_for_the_editor_and_the_pager() {
    for (desc, marker) in [(VI, "+w"), (EX, "+w"), (VIEW, "+r"), (LESS, "+r")] {
        let e = go(desc, &["a.txt", "b.txt"], &[], &[], true, "/w").expect_err("refused");
        assert!(e.problem.contains("two files of one directory"), "{e:?}");
        assert!(
            e.hint.as_deref().unwrap().contains(&format!("{marker} /w")),
            "{e:?}"
        );
    }
    // Two directories are two connections; one file is one.
    assert!(go(VI, &["a.txt", "/x/b.txt"], &[], &[], true, "/w").is_ok());
    assert!(go(VI, &["a.txt"], &[], &[], true, "/w").is_ok());
    // Not the other `cwd none` programs: sed's two files keep ADR-0344's rule.
    assert!(go(SED, &["-i", "x", "a", "b"], &[], &[], true, "/w").is_ok());
}

#[test]
fn a_cluster_with_r_still_reads_only() {
    use zoraya_proto_file::rights;
    let p = go(EX, &["-Rs", "notes.txt"], &[], &[], true, "/w").unwrap();
    let m = merge(&p);
    assert_eq!(m.len(), 1, "{m:?}");
    assert_eq!(m[0].rights & (rights::WRITE | rights::CREATE), 0, "{m:?}");
}

/// A room's grant on the view path is refused as a line's is (review of 10g,
/// finding 20).
#[test]
fn a_room_grant_cannot_name_the_own_directory_either() {
    use crate::grants::{own_conflict, own_directory};
    let own = own_directory(&d(SSH_CMD), true, Some("r")).unwrap();
    let room = |p: &str| vec![(true, String::from(p))];
    assert!(own_conflict(&own, "/u", &[], &room("/u/work")).is_ok());
    for p in ["/home/.ssh", "/home/.ssh/keys"] {
        assert!(own_conflict(&own, "/u", &[], &room(p)).is_err(), "{p}");
    }
    // A write that covers the real directory (`/data/r/ssh` here) is refused too (finding 5);
    // a read is not.
    for p in ["/data", "/data/r", "/"] {
        let r = own_conflict(&own, "/u", &[], &room(p)).unwrap_err();
        assert!(r.problem.contains("write handle"), "{p}: {}", r.problem);
        assert!(
            own_conflict(&own, "/u", &[(true, Value::literal(p))], &[]).is_err(),
            "{p}"
        );
        assert!(
            own_conflict(&own, "/u", &[(false, Value::literal(p))], &[]).is_ok(),
            "{p} read"
        );
    }
    assert!(own_conflict(&own, "/u", &[(true, Value::literal("/data/other"))], &[]).is_ok());
}

/// The program reads short options only: the shell names the short form
/// (review of 10g, finding 7).
#[test]
fn a_long_option_of_ssh_is_a_usage_mistake_with_the_short_form_named() {
    use crate::grants::long_option_used;
    let ssh = d(SSH_CMD);
    let keygen = d(KEYGEN_CMD);
    let used = |d: &zoraya_describe::Description, a: &[&str]| long_option_used(d, a);
    assert_eq!(
        used(&ssh, &["--port", "2222", "host"]),
        Some(("--port".into(), "-p".into()))
    );
    assert_eq!(
        used(&ssh, &["-l", "ada", "--login=ada", "host"]),
        Some(("--login=ada".into(), "-l".into()))
    );
    assert_eq!(
        used(&keygen, &["--type", "ed25519"]),
        Some(("--type".into(), "-t".into()))
    );
    // Short forms, `--help`, and words after the host (the far side's) pass.
    assert_eq!(used(&ssh, &["-p", "22", "host", "--port", "x"]), None);
    assert_eq!(used(&ssh, &["--help"]), None);
    assert_eq!(used(&ssh, &["-l", "ada", "host", "ls", "--color"]), None);
    assert_eq!(used(&keygen, &["-t", "ed25519", "--", "--x"]), None);
    // An unknown long option has no short form to name.
    assert_eq!(
        used(&ssh, &["--nope", "host"]),
        Some(("--nope".into(), String::new()))
    );
}

// ------------------------------------------------------------ ADR-0339 (git)

const GIT_CMD: &str = include_str!("../../../user/describe/posix/git.cmd");
const GIT_CLONE_CMD: &str = include_str!("../../../user/describe/posix/git-clone.cmd");
const GIT_INIT_CMD: &str = include_str!("../../../user/describe/posix/git-init.cmd");
const GIT_LS_REMOTE_CMD: &str = include_str!("../../../user/describe/posix/git-ls-remote.cmd");

/// The descriptions of git, as the shell's list holds them.
fn git_descriptions() -> Vec<Description> {
    [GIT_CMD, GIT_CLONE_CMD, GIT_INIT_CMD, GIT_LS_REMOTE_CMD]
        .iter()
        .map(|t| d(t))
        .collect()
}

#[test]
fn the_form_of_a_command_is_chosen_by_its_first_word() {
    use crate::grants::{trusted_description_args, words_of};
    let all = git_descriptions();
    let by = |words: &[&str]| {
        trusted_description_args(&all, "/bin/git", words).map(|d| d.name.as_str().to_string())
    };
    assert_eq!(
        by(&["clone", "https://e.org/a.git"]).as_deref(),
        Some("git-clone")
    );
    assert_eq!(by(&["init", "p"]).as_deref(), Some("git-init"));
    assert_eq!(
        by(&["ls-remote", "https://e.org/a"]).as_deref(),
        Some("git-ls-remote")
    );
    // Any other command, `-C` first, a bare `git`, and a copy of the program: the program's own or none.
    assert_eq!(by(&["status"]).as_deref(), Some("git"));
    assert_eq!(by(&["-C", "x", "clone", "u"]).as_deref(), Some("git"));
    assert_eq!(by(&[]).as_deref(), Some("git"));
    assert!(trusted_description_args(&all, "/tmp/git", &["clone"]).is_none());
    // The plain lookup never gives a form.
    assert_eq!(
        trusted_description(&all, "/bin/git").map(|d| d.name.as_str()),
        Some("git")
    );
    // A form parses the words after its command word.
    let clone = &all[1];
    assert_eq!(words_of(clone, &["clone", "u"]), ["u"]);
    assert_eq!(words_of(&all[0], &["status"]), ["status"]);
}

#[test]
fn git_init_creates_the_directory_it_names_and_everything_in_it() {
    // INV-0339 item 3: the parent narrowed to the name, and what is in it.
    let p = go(GIT_INIT_CMD, &["proj"], &[], &[], true, "/u").unwrap();
    assert_eq!(
        rows(&p),
        [
            ("/u".into(), Access::Read, "current directory".into()),
            (
                "/u".into(),
                Access::CreateTree("proj".into()),
                "operand DIR".into()
            ),
        ]
    );
    assert_eq!(
        Access::CreateTree("proj".into()).words(),
        "create proj, and change anything in it"
    );
    // Everything but `publish`, as for a `+w` tree; the name is the one narrowed to.
    assert_eq!(
        Access::CreateTree("proj".into()).rights(),
        Access::Tree.rights()
    );
    assert_eq!(Access::CreateTree("proj".into()).name(), Some("proj"));
    // Without a directory git makes `.git` in the current directory: nothing is given but the read.
    let p = go(GIT_INIT_CMD, &[], &[], &[], true, "/u").unwrap();
    assert_eq!(rows(&p).len(), 1);
    // A directory that is a value from outside the current directory is refused, as a `create` is.
    let r = go(
        GIT_INIT_CMD,
        &["/other/place"],
        &["/other/place"],
        &[],
        true,
        "/u",
    )
    .unwrap_err();
    assert!(r.problem.contains("from a value"), "{}", r.problem);
    // An undescribed copy of the description gets no create-tree (only the image's designate).
    let p = go(GIT_INIT_CMD, &["proj"], &[], &[], false, "/u").unwrap();
    assert_eq!(rows(&p).len(), 1);
}

#[test]
fn git_clone_without_a_directory_makes_the_one_the_address_names() {
    let url = "https://example.org/ada/proj.git";
    // Written: that directory.
    let p = go(GIT_CLONE_CMD, &[url, "work"], &[], &[], true, "/u").unwrap();
    assert!(rows(&p).contains(&(
        "/u".into(),
        Access::CreateTree("work".into()),
        "operand DIR".into()
    )));
    // Left out: the last part without `.git`, as git names it, designated by the shell itself.
    let p = go(GIT_CLONE_CMD, &[url], &[], &[], true, "/u").unwrap();
    assert!(rows(&p).contains(&(
        "/u".into(),
        Access::CreateTree("proj".into()),
        "operand DIR".into()
    )));
    let p = go(
        GIT_CLONE_CMD,
        &["https://example.org/ada/tool"],
        &[],
        &[],
        true,
        "/u",
    )
    .unwrap();
    assert!(rows(&p).contains(&(
        "/u".into(),
        Access::CreateTree("tool".into()),
        "operand DIR".into()
    )));
    // A name the shell cannot be sure of refuses the line, with the way out.
    for bad in [
        "https://example.org/",
        "https://example.org/a/",
        "https://example.org/a/%2e",
        "https://example.org/.git",
        "https://example.org/a/.hidden",
    ] {
        let r = go(GIT_CLONE_CMD, &[bad], &[], &[], true, "/u").unwrap_err();
        assert!(r.problem.contains("cannot be sure"), "{bad}: {}", r.problem);
        assert!(
            r.hint
                .as_deref()
                .unwrap_or("")
                .contains("write a directory"),
            "{bad}"
        );
    }
}

#[test]
fn the_url_of_git_clone_is_not_a_path_and_the_option_values_are_text() {
    let p = go(
        GIT_CLONE_CMD,
        &[
            "--depth",
            "1",
            "-b",
            "main",
            "https://example.org/a/p.git",
            "d",
        ],
        &[],
        &[],
        true,
        "/u",
    )
    .unwrap();
    // Only the current directory and the new directory: no entry for the URL, the depth or the branch.
    assert_eq!(rows(&p).len(), 2, "{p:?}");
}

#[test]
fn git_dash_c_designates_its_directory_to_read() {
    let p = go(GIT_CMD, &["-C", "proj/src", "status"], &[], &[], true, "/u").unwrap();
    assert!(
        rows(&p).contains(&("/u/proj/src".into(), Access::Read, "option -C".into())),
        "{p:?}"
    );
    // From a value outside the current directory: refused as any path is.
    let r = go(
        GIT_CMD,
        &["-C", "/etc", "status"],
        &["/etc"],
        &[],
        true,
        "/u",
    )
    .unwrap_err();
    assert!(r.problem.contains("from a value"), "{}", r.problem);
    // The words after the command are the program's: a path there designates nothing.
    let p = go(GIT_CMD, &["add", "-f", "/etc/passwd"], &[], &[], true, "/u").unwrap();
    assert_eq!(rows(&p).len(), 1, "{p:?}");
}

#[test]
fn the_git_descriptions_carry_the_repository_key_and_the_own_directory() {
    let all = git_descriptions();
    assert!(all[0].repository && all[0].sub.is_none());
    for form in &all[1..] {
        assert!(
            !form.repository,
            "{}: a form that makes a repository does not search",
            form.name
        );
        assert!(form.sub.is_some());
    }
    for d in &all {
        let own = crate::grants::own_directory(d, true, Some("proj")).expect("git's directory");
        assert_eq!(own.real, "/data/proj/git", "{}", d.name);
        assert_eq!(own.view, "/home/.config/git");
    }
}

#[test]
fn git_gets_no_double_dash_before_its_command() {
    use crate::grants::end_of_options_at;
    // ssh has one before the host (ADR-0340 item 4); git would read it as an option before the command.
    let git = d(GIT_CMD);
    assert_eq!(end_of_options_at(&git, &["status"]), None);
    assert_eq!(
        end_of_options_at(&git, &["-C", "p", "log", "-n", "3"]),
        None
    );
    assert_eq!(
        end_of_options_at(&d(SSH_CMD), &["-l", "ada", "h", "ls"]),
        Some(2)
    );
}

/// ADR-0465: a socket entry is never merged with another of its directory.
#[test]
fn a_socket_entry_is_refused_beside_another_of_its_directory() {
    let c = |n: &str| Planned {
        path: "/data/home/srv".into(),
        access: Access::Connect(n.into()),
        reason: "+sock".into(),
    };
    let read = Planned {
        path: "/data/home/srv".into(),
        access: Access::Read,
        reason: "current directory".into(),
    };
    let other = Planned {
        path: "/data/home/other".into(),
        access: Access::Read,
        reason: "+r".into(),
    };
    assert!(crate::grants::connect_conflict(&[c("a"), other.clone()]).is_ok());
    assert!(crate::grants::connect_conflict(&[c("a"), c("a")]).is_ok());
    assert!(crate::grants::connect_conflict(&[c("a"), c("b")]).is_err());
    assert!(crate::grants::connect_conflict(&[read.clone(), c("a")]).is_err());
    assert!(crate::grants::connect_conflict(&[c("a"), read]).is_err());
}

/// ADR-0465: `+sock PATH` gives the directory narrowed to the name, to
/// change (connect to), and nothing about its siblings; `grants` says so and
/// the window's pill names the file, not the directory.
#[test]
fn a_sock_word_is_a_directory_narrowed_to_one_name() {
    let d = d(ECHO);
    let parsed = Parsed::default();
    let socks = [Value::literal("/data/home/srv/s")];
    let line = Line {
        description: &d,
        parsed: &parsed,
        args: &[],
        cwd: "/w",
        grants: &[],
        nets: &[],
        listens: &[],
        socks: &socks,
        base_system: true,
        room: None,
    };
    let p = plan(&line).unwrap();
    assert_eq!(
        rows(&p),
        [(
            "/data/home/srv".into(),
            Access::Connect("s".into()),
            "+sock".into()
        )]
    );
    let r = report("echo-args", &p, "/home", "the terminal", false, false);
    assert!(r.contains("connect to s (+sock)"), "{r}");
    assert!(!r.contains("change"), "{r}");
    assert_eq!(
        crate::grants::window_pills(&p, &[]),
        Ok(vec!["+sock /data/home/srv/s".into()])
    );
    // A relative word joins the current directory; `/` has no name.
    let rel = [Value::literal("x/sock")];
    let p = plan(&Line {
        socks: &rel,
        ..line
    })
    .unwrap();
    assert_eq!(p[0].path, "/w/x");
    let bad = [Value::literal("/")];
    assert!(
        plan(&Line {
            socks: &bad,
            ..line
        })
        .is_err()
    );
}
