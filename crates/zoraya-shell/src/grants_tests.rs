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
        base_system: base,
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
        report("cp", &p, "/u", "the terminal", false),
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
        report("cp", &p, "/u", "the terminal", false),
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
        report("tcc", &p, "/u", "the terminal", true),
        "tcc gets (a POSIX program):
  in, out, err         the terminal
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
