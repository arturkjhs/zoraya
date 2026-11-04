//! Host tests of the grant planner (ADR-0153; INV-0153-2, -3, -4, -8, -9).

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_command::{Outcome, Parsed};
use zoraya_describe::Description;
use zoraya_proto_file::rights;

use crate::eval::Value;
use crate::grants::{
    Access, Line, Planned, Refused, merge, plan, report, summary, through_links,
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
        report("cp", &p, "/u", "the terminal"),
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
    assert_eq!(p[1].access, Access::Tree, "+w is how a person gives more");
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
            ("/u/include", "+r"),
            ("/tmp/w", "+w"),
            ("/u/proj/main.c", "operand FILE"),
            ("/u/proj", "option -o"),
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
        "~: read, ~/notes.txt: read, ~/project: create or replace notes.txt"
    );
    let p = go(CP, &["a.txt", "b.txt"], &[], &[], true, "/u").unwrap();
    assert_eq!(
        summary(&p, "/u"),
        "~: read, ~: create and change anything in, ~/a.txt: read"
    );
    assert_eq!(summary(&[], "/u"), "");
}

#[test]
fn entries_of_one_path_merge() {
    let p = go(CP, &["a.txt", "b.txt"], &[], &[], true, "/u").unwrap();
    let m = merge(&p);
    assert_eq!(m.len(), 2, "/u and /u/a.txt");
    assert_eq!(m[0].name, None);
    assert_eq!(
        m[0].rights,
        Access::Read.rights() | Access::Create("b.txt".into()).rights()
    );
    assert_eq!(
        report("cp", &p, "/u", "the terminal"),
        "cp gets:
  in, out, err         the terminal
  ~                    read (current directory)
  ~                    create and change anything in (operand TO)
  ~/a.txt              read (operand FROM)
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
