//! Host tests of the language (ADR-0150, ADR-0151) over a fake host.

use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::{ExitReason, ExitRecord, RequestKind};
use zoraya_command::Parsed;
use zoraya_describe::Description;

use crate::eval::{Exec, Found, Host, Io, Pipe, Ran, Shell, Sink, State, Value};
use crate::record::{self, exited};
use crate::syntax::Stream;
use crate::syntax::{self, Error};

/// A program the fake host knows: what it does with its arguments and
/// input, and what it prints.
type Program = fn(&[Value], &str) -> (ExitRecord, String);

#[derive(Default)]
struct Fake {
    /// Absolute paths without the leading `/`; directories end in `/`.
    files: BTreeMap<String, String>,
    cwd: String,
    programs: BTreeMap<&'static str, Program>,
    /// Every program run: its name and arguments.
    runs: Vec<(String, Vec<String>, Vec<String>)>,
    /// The grants of every program run: `+w`, the path, its mark.
    granted: Vec<(bool, String, bool)>,
    /// The marks of every program run's arguments (ADR-0153 rule 4).
    marks: Vec<Vec<bool>>,
    screen: String,
    outputs: BTreeMap<u32, (String, bool)>,
    next: u32,
    interrupts: u32,
    /// `io.terminal` of each run of the builtin `pager`.
    terminals: Vec<bool>,
}

const QUESTION: &str =
    "name ask\nsummary a question\nusage ask [X...]\noperand [X...] text x\nquestion\n";

impl Fake {
    fn new() -> Fake {
        let mut f = Fake::default();
        f.programs.insert("ok", |_, _| (exited(0), String::new()));
        f.programs.insert("fail", |_, _| (exited(1), String::new()));
        f.programs.insert("ask", |args, _| {
            let yes = args.first().is_some_and(|a| a.text == "yes");
            (exited(if yes { 0 } else { 1 }), String::new())
        });
        f.programs
            .insert("spin", |_, _| (record::interrupted(), String::new()));
        f.programs.insert("crash", |_, _| {
            let r = ExitRecord {
                code: 0,
                reason: ExitReason::Fault,
                detail: 0x24,
                request_kind: None,
            };
            (r, String::new())
        });
        f.programs.insert("prog", |_, _| (exited(0), String::new()));
        // One write as large as a line may be, for the capture's limit.
        f.programs
            .insert("big", |_, _| (exited(0), "x".repeat(syntax::MAX_SOURCE)));
        f.programs.insert("zee", |_, _| (exited(0), String::new()));
        // Prints its arguments, one per line.
        f.programs.insert("say", |args, _| {
            let mut out = String::new();
            for a in args {
                out.push_str(&a.text);
                out.push('\n');
            }
            (exited(0), out)
        });
        f.programs
            .insert("upper", |_, input| (exited(0), input.to_uppercase()));
        // A POSIX writer whose reader closed: SIGPIPE.
        f.programs.insert("piped", |_, _| {
            let r = ExitRecord {
                code: 0,
                reason: ExitReason::Terminated,
                detail: crate::eval::SIGPIPE,
                request_kind: None,
            };
            (r, String::new())
        });
        for name in [
            "a.txt",
            "b.txt",
            "-n",
            ".hidden",
            "sub/",
            "sub/c.txt",
            "sub/deep/",
            "sub/deep/d.txt",
        ] {
            f.files.insert(name.into(), String::new());
        }
        f
    }

    fn path(&self, p: &str) -> String {
        let p = match p.strip_prefix('/') {
            Some(abs) => abs.to_string(),
            None if self.cwd.is_empty() => p.to_string(),
            None => alloc::format!("{}/{p}", self.cwd),
        };
        p.trim_end_matches('/').to_string()
    }
}

impl Host for Fake {
    fn cwd(&mut self) -> String {
        alloc::format!("/{}", self.cwd)
    }

    /// The grants and arguments it was asked about, with their marks.
    fn grants(&mut self, _: &mut State, exec: &Exec<'_>, io: &mut Io) -> i32 {
        for (w, v) in exec.grants {
            io.out
                .push_str(&alloc::format!("grant {w} {} {}\n", v.text, v.literal));
        }
        io.out.push_str(&alloc::format!("command {}\n", exec.name));
        for a in exec.args {
            io.out
                .push_str(&alloc::format!("arg {} {}\n", a.text, a.literal));
        }
        0
    }

    fn lookup(&mut self, name: &str) -> Found {
        if self.programs.contains_key(name) {
            Found::Program
        } else if name == "grants" || name == "pager" || name == "settings" {
            Found::Builtin
        } else {
            Found::Nothing
        }
    }

    fn description(&mut self, name: &str) -> Option<Rc<Description>> {
        let text = match name {
            "ask" => QUESTION,
            "grants" => {
                "name grants\nsummary show\nusage grants LINE...\noperand LINE... text l\ncwd none\n"
            }
            "pager" => "name pager\nsummary page\nusage pager\ncwd none\n",
            "settings" => include_str!("../../../user/describe/builtins/settings.cmd"),
            _ => return None,
        };
        Some(Rc::new(zoraya_describe::parse(text.as_bytes()).unwrap()))
    }

    fn names(&mut self) -> Vec<String> {
        let mut n: Vec<String> = self.programs.keys().map(|s| s.to_string()).collect();
        n.push("ls".into());
        n
    }

    fn builtin(
        &mut self,
        _: &mut State,
        name: &str,
        d: &Description,
        parsed: &Parsed<'_>,
        io: &mut Io,
    ) -> i32 {
        if name == "pager" {
            self.terminals.push(io.terminal);
            io.out.push_str("paged\n");
        }
        // `settings` prints what it was given, as parsed: its options by
        // long name and its operands by name.
        if name == "settings" {
            for (o, arg) in &parsed.options {
                let arg = arg.unwrap_or("");
                io.out
                    .push_str(&alloc::format!("option {} {arg}\n", d.options[*o].long));
            }
            for (o, text) in &parsed.operands {
                io.out
                    .push_str(&alloc::format!("operand {} {text}\n", d.operands[*o].name));
            }
        }
        0
    }

    /// Runs the programs in turn, each reading what the one before printed.
    fn program(&mut self, pipe: &Pipe<'_>) -> Ran {
        for exec in pipe.programs {
            for (w, v) in exec.grants {
                self.granted.push((*w, v.text.clone(), v.literal));
            }
            let args: Vec<String> = exec.args.iter().map(|a| a.text.clone()).collect();
            self.marks
                .push(exec.args.iter().map(|a| a.literal).collect());
            self.runs.push((exec.name.into(), args, exec.env.to_vec()));
        }
        if pipe.background {
            return Ran::Background(1);
        }
        // Stopped by Ctrl-Z in front.
        if pipe.programs.iter().any(|e| e.name == "zee") {
            return Ran::Stopped(1);
        }
        let mut input = String::from(pipe.input.unwrap_or(""));
        let mut records = Vec::new();
        for exec in pipe.programs {
            let (record, out) = self.programs[exec.name](exec.args, &input);
            records.push(record);
            input = out;
        }
        let last = pipe.programs.last().expect("a program");
        let mut out = pipe.out;
        let mut opened = None;
        for (stream, append, path) in last.redirects {
            if *stream == Stream::Out {
                let t = self.open_output(path, *append).expect("opened");
                out = Sink::File(t);
                opened = Some(t);
            }
        }
        let mut captured = String::new();
        match out {
            Sink::Capture => captured = input,
            other => self.write(other, &input),
        }
        if let Some(t) = opened {
            self.close_output(t);
        }
        let ctrl_c = records
            .iter()
            .any(|r| r.request_kind == Some(RequestKind::Interrupt));
        Ran::Ended {
            records,
            ctrl_c,
            captured,
        }
    }

    fn list(&mut self, dir: &str) -> Option<Vec<(String, bool)>> {
        let base = self.path(dir);
        let prefix = if base.is_empty() {
            String::new()
        } else {
            alloc::format!("{base}/")
        };
        if !base.is_empty() && !self.files.contains_key(&alloc::format!("{base}/")) {
            return None;
        }
        let mut out = Vec::new();
        for name in self.files.keys() {
            let Some(rest) = name.strip_prefix(&prefix) else {
                continue;
            };
            let rest_trim = rest.trim_end_matches('/');
            if rest_trim.is_empty() || rest_trim.contains('/') {
                continue;
            }
            out.push((rest_trim.to_string(), rest.ends_with('/')));
        }
        Some(out)
    }

    fn read(&mut self, path: &str) -> Result<String, String> {
        self.files
            .get(&self.path(path))
            .cloned()
            .ok_or_else(|| "not found".into())
    }

    fn kind(&mut self, path: &str) -> Option<bool> {
        let p = self.path(path);
        if self.files.contains_key(&alloc::format!("{p}/")) {
            Some(true)
        } else {
            self.files.contains_key(&p).then_some(false)
        }
    }

    fn open_output(&mut self, path: &str, append: bool) -> Result<u32, String> {
        self.next += 1;
        self.outputs.insert(self.next, (self.path(path), append));
        if !append {
            self.files.insert(self.path(path), String::new());
        }
        Ok(self.next)
    }

    fn write(&mut self, sink: Sink, text: &str) {
        match sink {
            Sink::File(t) => {
                let (p, _) = self.outputs[&t].clone();
                self.files.entry(p).or_default().push_str(text);
            }
            _ => self.screen.push_str(text),
        }
    }

    fn close_output(&mut self, token: u32) {
        self.outputs.remove(&token);
    }

    fn home(&mut self) -> String {
        "/house".into()
    }

    fn interrupted(&mut self) -> bool {
        if self.interrupts > 0 {
            self.interrupts -= 1;
            return self.interrupts == 0;
        }
        false
    }
}

fn shell() -> Shell<Fake> {
    Shell::new(Fake::new())
}

/// Runs `line` and gives what the screen showed.
fn run(sh: &mut Shell<Fake>, line: &str) -> String {
    sh.host.screen.clear();
    let block = syntax::parse(line).unwrap_or_else(|e| panic!("{line:?}: {e:?}"));
    sh.run_line(line, &block);
    core::mem::take(&mut sh.host.screen)
}

fn ran(sh: &Shell<Fake>) -> Vec<String> {
    sh.host
        .runs
        .iter()
        .map(|(n, a, _)| {
            let mut s = n.clone();
            for x in a {
                s.push(' ');
                s.push_str(x);
            }
            s
        })
        .collect()
}

#[test]
fn the_examples_of_adr_0150() {
    let mut sh = shell();
    assert_eq!(run(&mut sh, "set name 'my notes.txt'"), "");
    assert_eq!(run(&mut sh, "echo $name"), "my notes.txt\n");
    run(&mut sh, "prog $name");
    assert_eq!(sh.host.runs[0].1, ["my notes.txt"]);
    assert_eq!(run(&mut sh, "set files *.txt"), "");
    assert_eq!(run(&mut sh, "echo $files"), "a.txt b.txt\n");
    assert_eq!(run(&mut sh, "count $files"), "2\n");
    assert_eq!(run(&mut sh, "echo $files.bak"), "a.txt.bak b.txt.bak\n");
    assert_eq!(run(&mut sh, "prog *.tmp"), "zosh: *.tmp: no file matches\n");
    assert_eq!(sh.host.runs.len(), 1, "nothing ran");
    assert_eq!(
        run(&mut sh, "x=1"),
        "zosh: x=1: not a command\n  hint: to set a variable, type: set x 1\n"
    );
    assert_eq!(run(&mut sh, "echo \"it's\" 'a*b'"), "it's a*b\n");
    assert_eq!(run(&mut sh, "echo ~ ~/x a~"), "/house /house/x a~\n");
    assert_eq!(
        run(&mut sh, "echo \"you are in $(echo here)\""),
        "you are in here\n"
    );
    assert_eq!(
        run(&mut sh, "set e; echo $e.x"),
        "zosh: $e.x: $e is empty\n"
    );
    assert_eq!(run(&mut sh, "set e; prog $e; echo ok"), "ok\n");
    assert_eq!(
        sh.host.runs.last().unwrap().1.len(),
        0,
        "the empty list gives no argument"
    );
    assert_eq!(run(&mut sh, "set e ''; count $e"), "1\n");
    assert_eq!(run(&mut sh, "echo $files[2] $files[-1]"), "b.txt b.txt\n");
    assert_eq!(
        run(&mut sh, "echo $files[3]"),
        "zosh: $files[3]: out of range: it has 2\n"
    );
    assert_eq!(
        run(&mut sh, "set a 1 2; set b 3 4; echo $a$b"),
        "zosh: $a$b: two lists of several values in one word\n"
    );
    assert_eq!(run(&mut sh, "echo my\\ notes"), "my notes\n");
    assert_eq!(run(&mut sh, "echo \"tab\\there\""), "tab\there\n");
    assert_eq!(run(&mut sh, "set l $(echo one; echo two); count $l"), "2\n");
    assert_eq!(
        run(&mut sh, "echo $nope").lines().next(),
        Some("zosh: $nope: not set")
    );
    assert!(
        run(&mut sh, "export X=1")
            .starts_with("zosh: X=1: not a variable name\n  hint: to export it, type: export X 1")
    );
    assert_eq!(
        run(&mut sh, "wc -l x"),
        "zosh: wc: no such command\n  hint: to count lines: count --lines FILE...\n"
    );
    assert_eq!(
        run(&mut sh, "grep x"),
        "zosh: grep: no such command\n  hint: on Zoraya this is search: search TEXT FILE...\n"
    );
    assert_eq!(
        run(&mut sh, "sl"),
        "zosh: sl: no such command\n  hint: did you mean ls?\n"
    );
    assert_eq!(
        run(&mut sh, "math 6 x 7; math '(1 + 2) * 3'; math -1 + 2"),
        "42\n9\n1\n"
    );
}

/// INV-0150-1: `$name` gives exactly one argument per element, whatever the
/// elements hold.
#[test]
fn a_variable_is_never_split_or_globbed() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let alphabet = [
        " ", "*", "?", "'", "\"", "-", "a", "\u{e9}", "$", "~", "/", "\t", "[",
    ];
    for _ in 0..500 {
        let n = rng.below(6);
        let values: Vec<String> = (0..n)
            .map(|_| {
                (0..rng.below(6))
                    .map(|_| alphabet[rng.below(alphabet.len())])
                    .collect()
            })
            .collect();
        let mut sh = shell();
        let refs: Vec<&str> = values.iter().map(String::as_str).collect();
        sh.set_global("v", &refs);
        sh.host.runs.clear();
        run(&mut sh, "prog $v");
        assert_eq!(sh.host.runs.len(), 1, "{values:?}");
        assert_eq!(sh.host.runs[0].1, values);
        run(&mut sh, "prog \"$v\"");
        assert_eq!(sh.host.runs[1].1.len(), 1);
    }
}

/// INV-0150-2: no glob gives an argument starting with `-`, and one that
/// matches nothing runs nothing, except in `for` and `set`.
#[test]
fn globs_are_safe() {
    let mut sh = shell();
    assert_eq!(run(&mut sh, "echo *"), "./-n a.txt b.txt sub\n");
    assert_eq!(run(&mut sh, "echo .*"), ".hidden\n");
    assert_eq!(run(&mut sh, "echo */*.txt"), "sub/c.txt\n");
    assert_eq!(
        run(&mut sh, "echo **/*.txt"),
        "a.txt b.txt sub/c.txt sub/deep/d.txt\n"
    );
    assert_eq!(run(&mut sh, "echo ?.txt"), "a.txt b.txt\n");
    assert_eq!(run(&mut sh, "echo '*'.txt"), "*.txt\n");
    assert_eq!(run(&mut sh, "set p '*'; echo $p"), "*\n");
    run(&mut sh, "prog *");
    for arg in &sh.host.runs[0].1 {
        assert!(!arg.starts_with('-'), "{arg}");
    }
    sh.host.runs.clear();
    assert_eq!(
        run(&mut sh, "prog *.none; prog after"),
        "zosh: *.none: no file matches\n(the rest of the line was not run)\n"
    );
    assert!(sh.host.runs.is_empty());
    assert_eq!(
        run(&mut sh, "for f in *.none { prog $f }; echo done"),
        "done\n"
    );
    assert!(sh.host.runs.is_empty());
    assert_eq!(run(&mut sh, "set x *.none; count $x"), "0\n");
    sh.host.cwd = "sub".into();
    assert_eq!(run(&mut sh, "echo *"), "c.txt deep\n");
}

/// INV-0150-3: the parser is total.
#[test]
fn the_parser_is_total() {
    let pieces = [
        "if ", "else ", "for ", " in ", "while ", "fn ", "try ", "catch ", "{", "}", " ", "\n",
        ";", "&&", "||", "|", "&|", "&", "!", "'", "\"", "\\", "$", "$(", ")", "$x", "[1]", "*",
        "#", ">", ">>", "2>", "&>", "<", "+r ", "+w ", "x=1", "echo", "\u{e9}", "return ", "break",
    ];
    let mut rng = Rng(1);
    for round in 0..30_000 {
        let mut s = String::new();
        if round % 2 == 0 {
            for _ in 0..rng.below(30) {
                s.push_str(pieces[rng.below(pieces.len())]);
            }
        } else {
            for _ in 0..rng.below(40) {
                let c = char::from_u32(rng.below(0x80) as u32).unwrap_or('x');
                s.push(c);
            }
        }
        match syntax::parse(&s) {
            Ok(_) | Err(Error::Incomplete) => {}
            Err(Error::Invalid { at, .. }) => assert!(at <= s.len(), "{s:?}"),
        }
    }
    let deep = "$(".repeat(1000);
    assert!(syntax::parse(&deep).is_err());
    let blocks = "if x {".repeat(1000);
    assert!(syntax::parse(&blocks).is_err());
}

/// INV-0150-4: a child's environment is exactly the exported variables and
/// its own `NAME=value` words.
#[test]
fn the_environment_is_what_was_exported() {
    let mut sh = shell();
    run(&mut sh, "set secret 1; export shown 2; export kept; ");
    run(&mut sh, "set kept 3; export kept");
    run(&mut sh, "B=4 shown=5 prog");
    assert_eq!(sh.host.runs[0].2, ["B=4", "kept=3", "shown=5"]);
    run(&mut sh, "prog");
    assert_eq!(sh.host.runs[1].2, ["kept=3", "shown=2"]);
    assert!(run(&mut sh, "set kept 1 2").contains("exported, so it holds exactly one value"));
    assert!(run(&mut sh, "set many 1 2; export many").contains("holds 2 values"));
}

/// INV-0151-1: an unchecked failure runs nothing after it; a checked one
/// runs exactly its branch.
#[test]
fn the_stop_rule() {
    let cases: [(&str, &[&str]); 18] = [
        ("fail; ok", &["fail"]),
        ("ok; fail; ok", &["ok", "fail"]),
        ("fail && ok; ok 2", &["fail", "ok 2"]),
        ("fail || ok 1; ok 2", &["fail", "ok 1", "ok 2"]),
        ("ok || fail; ok 2", &["ok", "ok 2"]),
        ("ok && fail; ok 2", &["ok", "fail"]),
        ("! fail; ok 2", &["fail", "ok 2"]),
        ("! ok; ok 2", &["ok"]),
        (
            "if fail { ok 1 } else { ok 2 }; ok 3",
            &["fail", "ok 2", "ok 3"],
        ),
        ("if ok { fail; ok 1 }; ok 3", &["ok", "fail"]),
        ("for x in 1 2 { fail; ok $x }; ok 3", &["fail"]),
        (
            "set n 1; while test $n = 1 { set n 2; fail; ok }; ok 3",
            &["fail"],
        ),
        (
            "try { fail; ok 1 } catch { ok 2 }; ok 3",
            &["fail", "ok 2", "ok 3"],
        ),
        ("try { fail }; ok 3", &["fail", "ok 3"]),
        ("fn f { fail; ok 1 }; f; ok 2", &["fail"]),
        (
            "fn f { fail; ok 1 }; f || ok 2; ok 3",
            &["fail", "ok 2", "ok 3"],
        ),
        (
            "fn f { fail; ok 1 }; if f { ok 2 } else { ok 3 }",
            &["fail", "ok 3"],
        ),
        ("ask no; ok 2", &["ask no", "ok 2"]),
    ];
    for (line, expected) in cases {
        let mut sh = shell();
        run(&mut sh, line);
        assert_eq!(ran(&sh), expected, "{line}");
    }
    let mut sh = shell();
    assert_eq!(run(&mut sh, "ask no; echo $status"), "1\n");
    assert_eq!(run(&mut sh, "if ask no { echo y } else { echo n }"), "n\n");
}

#[test]
fn the_examples_of_adr_0151() {
    let mut sh = shell();
    assert_eq!(
        run(&mut sh, "if test -d sub { echo yes } else { echo no }"),
        "yes\n"
    );
    assert_eq!(
        run(&mut sh, "if test -d build { echo yes } else { echo no }"),
        "no\n"
    );
    assert_eq!(
        run(&mut sh, "fail; echo made"),
        "(the rest of the line was not run)\n"
    );
    assert_eq!(
        run(&mut sh, "fail || echo 'it was there'"),
        "it was there\n"
    );
    let src = "fn greet who {\n  echo \"hello, $who\"\n}";
    assert_eq!(run(&mut sh, src), "");
    assert_eq!(
        run(&mut sh, "greet"),
        "greet: missing operand WHO\nusage: greet WHO\nmore: help greet\n"
    );
    assert_eq!(run(&mut sh, "greet Ada"), "hello, Ada\n");
    assert_eq!(run(&mut sh, "spin"), "spin: interrupted (Ctrl-C)\n");
    assert_eq!(run(&mut sh, "echo $status"), "interrupted\n");
    assert_eq!(
        run(
            &mut sh,
            "try { fail } catch { echo \"status was $status\" }"
        ),
        "status was 1\n"
    );
    assert_eq!(
        run(&mut sh, "crash"),
        "crash: crashed (data abort, class 0x24)\n"
    );
    assert_eq!(run(&mut sh, "status --reason"), "fault\n");
    run(&mut sh, "crash");
    assert_eq!(run(&mut sh, "status"), "crashed (data abort, class 0x24)\n");
    assert_eq!(run(&mut sh, "status"), "succeeded (exit status 0)\n");
    assert_eq!(run(&mut sh, "fn r { return 3 }; r || echo $status"), "3\n");
    assert_eq!(run(&mut sh, "set --query nope || echo unset"), "unset\n");
    assert_eq!(
        run(
            &mut sh,
            "fn g { set inner 1; set --global outer 2 }; g; echo $outer"
        ),
        "2\n"
    );
    assert!(run(&mut sh, "echo $inner").starts_with("zosh: $inner: not set"));
    assert_eq!(
        run(
            &mut sh,
            "for f in a b c { if test $f = b { break }; echo $f }"
        ),
        "a\n"
    );
    assert_eq!(
        run(
            &mut sh,
            "for f in a b c { if test $f = b { continue }; echo $f }"
        ),
        "a\nc\n"
    );
    assert_eq!(run(&mut sh, "fn args { count $args }; args 1 2 3"), "3\n");
}

#[test]
fn scripts_say_where_they_stopped() {
    let mut sh = shell();
    sh.host
        .files
        .insert("build.zosh".into(), "fail\necho built\n".into());
    assert_eq!(
        run(&mut sh, "source build.zosh"),
        "zosh: build.zosh line 1: fail: exit status 1\n(the rest of build.zosh was not run)\n"
    );
    assert_eq!(run(&mut sh, "echo $status"), "1\n");
    sh.host
        .files
        .insert("s.zosh".into(), "echo one\nspin\necho two\n".into());
    assert_eq!(
        run(&mut sh, "source s.zosh"),
        "one\nzosh: s.zosh line 2: spin: interrupted\n(the rest of s.zosh was not run)\n"
    );
    sh.host.files.insert(
        "tidy.zosh".into(),
        "for f in *.bak {\n    echo $f\n}\n".into(),
    );
    assert_eq!(run(&mut sh, "source tidy.zosh"), "");
    sh.host
        .files
        .insert("bad.zosh".into(), "echo a\nif {\n".into());
    assert!(run(&mut sh, "source bad.zosh").starts_with("zosh: bad.zosh line 2:"));
}

#[test]
fn redirections_of_builtins() {
    let mut sh = shell();
    assert_eq!(run(&mut sh, "echo 'buy milk' > notes.txt"), "");
    assert_eq!(run(&mut sh, "echo 'call Ada' >> notes.txt"), "");
    assert_eq!(sh.host.files["notes.txt"], "buy milk\ncall Ada\n");
    assert_eq!(run(&mut sh, "count --lines notes.txt"), "2\n");
    assert_eq!(
        run(&mut sh, "echo x > $nope"),
        "zosh: $nope: not set\n  hint: set it first: set nope VALUE, or ask: set --query nope\n"
    );
}

#[test]
fn builtins_and_functions_stay_in_the_shell() {
    let mut sh = shell();
    assert_eq!(
        run(&mut sh, "echo a | echo b"),
        "zosh: echo: a builtin or function can only start a pipeline\n  hint: run echo on its own line\n"
    );
    assert_eq!(
        run(&mut sh, "echo a &"),
        "zosh: echo: a builtin or function cannot run in the background\n"
    );
    run(&mut sh, "prog x &");
    assert!(sh.host.runs[0].0 == "prog");
    assert_eq!(
        run(&mut sh, "+r . echo x"),
        "zosh: echo: only a program can be given grants\n"
    );
}

#[test]
fn usage_comes_from_descriptions() {
    let mut sh = shell();
    assert_eq!(
        run(&mut sh, "echo --nope"),
        "echo: --nope: no such option\nusage: echo [WORD...]\nmore: help echo\n"
    );
    assert!(
        run(&mut sh, "set --help")
            .starts_with("set: set a variable\nusage: set [--global] NAME [VALUE...]\n")
    );
    assert_eq!(run(&mut sh, "echo $status"), "0\n");
}

#[test]
fn a_function_has_its_comments_as_help() {
    let src =
        "# back up a file\n# Copies FILE to FILE.bak.\nfn backup file {\n  echo $file.bak\n}\n";
    let block = syntax::parse(src).unwrap();
    let syntax::Command::Fn(f) = &block.statements[0].chain.first.elements[0].command else {
        panic!();
    };
    assert_eq!(f.doc, ["back up a file", "Copies FILE to FILE.bak."]);
    let d = crate::eval::function_description(f);
    assert_eq!(d.summary, "back up a file");
    assert_eq!(d.usage, ["backup FILE"]);
    let block = syntax::parse("echo x # not help\nfn f {\n}\n").unwrap();
    let syntax::Command::Fn(f) = &block.statements[1].chain.first.elements[0].command else {
        panic!();
    };
    assert!(f.doc.is_empty());
}

#[test]
fn parse_errors_have_positions_and_hints() {
    let line = "if test -f a.txt { echo yes } else echo no }";
    match syntax::parse(line) {
        Err(Error::Invalid { at, problem, hint }) => {
            assert_eq!(problem, "'else' needs a block");
            assert_eq!(&line[at..at + 4], "echo");
            assert_eq!(hint.as_deref(), Some("write: else { echo no }"));
        }
        other => panic!("{other:?}"),
    }
    for incomplete in [
        "echo 'open",
        "if x {",
        "echo a &&",
        "echo \"x",
        "echo $(x",
        "for f in a b",
        "fn g {\n echo",
        "echo a \\",
    ] {
        assert_eq!(
            syntax::parse(incomplete),
            Err(Error::Incomplete),
            "{incomplete}"
        );
    }
    for invalid in [
        "; echo",
        "echo }",
        "}",
        "echo $",
        "&& x",
        "| x",
        "echo a | | b",
        "echo >",
        "fn 1x {}",
        "else { }",
        "+r",
        "for 1 in a { }",
    ] {
        assert!(
            matches!(syntax::parse(invalid), Err(Error::Invalid { .. })),
            "{invalid}"
        );
    }
    assert_eq!(syntax::position("a\nbc d", 5), (2, 4));
}

#[test]
fn the_interrupt_stops_a_loop() {
    let mut sh = shell();
    sh.host.interrupts = 3;
    let out = run(&mut sh, "while test 1 = 1 { echo x }");
    assert!(out.ends_with("zosh: interrupted (Ctrl-C)\n"), "{out}");
}

/// INV-0151-2, INV-0151-3: `$status` and the screen line are functions of
/// the record alone, and only `Exited` 0 is success.
#[test]
fn records_in_words() {
    let kinds = [
        None,
        Some(RequestKind::Interrupt),
        Some(RequestKind::Terminate),
        Some(RequestKind::Hangup),
        Some(RequestKind::Quit),
        Some(RequestKind::Custom(7)),
    ];
    let reasons = [
        ExitReason::Exited,
        ExitReason::Terminated,
        ExitReason::Killed,
        ExitReason::Fault,
        ExitReason::Limit,
    ];
    for reason in reasons {
        for kind in kinds {
            for code in [0, 1, 255] {
                for detail in [0, 13, u32::MAX] {
                    let r = ExitRecord {
                        code,
                        reason,
                        detail,
                        request_kind: kind,
                    };
                    let word = record::status_word(&r);
                    assert!(!word.is_empty() && !word.contains(' '), "{r:?}");
                    assert_eq!(word, record::status_word(&r.clone()));
                    let success = record::is_success(&r);
                    assert_eq!(success, reason == ExitReason::Exited && code == 0, "{r:?}");
                    let line = record::screen_line("p", &r, false);
                    assert_eq!(line.is_none(), reason == ExitReason::Exited, "{r:?}");
                }
            }
        }
    }
    let term = |detail, kind| ExitRecord {
        code: 0,
        reason: ExitReason::Terminated,
        detail,
        request_kind: kind,
    };
    assert_eq!(record::status_word(&term(13, None)), "signal-13");
    assert_eq!(record::status_word(&term(0, None)), "terminated");
    assert_eq!(record::status_word(&term(u32::MAX, None)), "terminated");
    assert_eq!(record::what(&term(6, None)), "ended by signal 6");
    assert_eq!(
        record::status_word(&term(0, Some(RequestKind::Hangup))),
        "hung-up"
    );
    assert_eq!(
        record::status_word(&term(0, Some(RequestKind::Custom(9)))),
        "request-9"
    );
    assert_eq!(
        record::what(&term(0, Some(RequestKind::Custom(9)))),
        "ended by request 9"
    );
    assert_eq!(record::status_word(&exited(255)), "255");
}

#[test]
fn math_is_checked() {
    use crate::eval::math;
    assert_eq!(math("1 + 2 * 3"), Ok(7));
    assert_eq!(math("(1 + 2) x 3"), Ok(9));
    assert_eq!(math("-7 / 2"), Ok(-3));
    assert_eq!(math("7 % 3"), Ok(1));
    assert_eq!(math("1 / 0"), Err("division by zero"));
    assert_eq!(
        math("9223372036854775807 + 1"),
        Err("the result is too large")
    );
    assert!(math("1 +").is_err());
    assert!(math("(1").is_err());
    assert!(math("a").is_err());
    assert!(math(&"(".repeat(10_000)).is_err());
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn every_language_description_parses() {
    for (name, text) in crate::eval::LANGUAGE_DESCRIPTIONS {
        let d = zoraya_describe::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(d.name, name);
    }
    let _ = vec![0];
}

/// INV-0152-3: the status rule over every combination of three elements'
/// records: success, a failure, another failure, a question's "no" and a
/// writer ended by `SIGPIPE`.
#[test]
fn a_pipelines_status_follows_rule_6_for_every_combination() {
    use crate::eval::{Member, SIGPIPE, pipeline_status};
    let sigpipe = ExitRecord {
        code: 0,
        reason: ExitReason::Terminated,
        detail: SIGPIPE,
        request_kind: None,
    };
    let kinds = [
        ("ok", exited(0), false),
        ("fail", exited(1), false),
        ("fail3", exited(3), false),
        ("no", exited(1), true),
        ("sigpipe", sigpipe, false),
        ("yes-question", exited(0), true),
        ("fail2-question", exited(2), true),
    ];
    for a in &kinds {
        for b in &kinds {
            for c in &kinds {
                let elements = [a, b, c];
                let members: Vec<Member> = elements
                    .iter()
                    .map(|(_, record, question)| Member {
                        record: *record,
                        question: *question,
                    })
                    .collect();
                let r = pipeline_status(&members);
                // The rule, as ADR-0152 says it, element by element.
                let mut failure = None;
                for (i, (name, record, _)) in elements.iter().enumerate() {
                    let fine = match *name {
                        "ok" | "yes-question" | "no" => true,
                        "sigpipe" => i < 2,
                        _ => false,
                    };
                    if !fine {
                        failure = Some(*record);
                    }
                }
                let names: Vec<&str> = elements.iter().map(|e| e.0).collect();
                match failure {
                    Some(record) => {
                        assert_eq!(r.record, record, "{names:?}");
                        assert!(r.failed(), "{names:?}");
                    }
                    None if c.0 == "no" => {
                        assert!(r.no && !r.failed(), "{names:?}");
                        assert_eq!(r.record, exited(1), "{names:?}");
                    }
                    None => assert!(r.success() && !r.no, "{names:?}"),
                }
            }
        }
    }
}

/// ADR-0152 rules 2, 4 and 5: bytes pass along a pipeline; a builtin or
/// function may start one; `$(...)` takes a pipeline's output.
#[test]
fn pipelines_pass_output_along() {
    let mut sh = shell();
    assert_eq!(run(&mut sh, "say a b | upper"), "A\nB\n");
    assert_eq!(ran(&sh), ["say a b", "upper"]);
    assert_eq!(run(&mut sh, "echo hi | upper"), "HI\n");
    assert_eq!(
        run(&mut sh, "fn two { echo x; echo y }; two | upper | upper"),
        "X\nY\n"
    );
    assert_eq!(run(&mut sh, "set l $(say a b c | upper); count $l"), "3\n");
    assert_eq!(run(&mut sh, "echo $(say q)"), "q\n");
    assert_eq!(run(&mut sh, "say q | upper > out.txt"), "");
    assert_eq!(sh.host.files["out.txt"], "Q\n");
    // `&|` sends err into the pipe; `|` leaves it on the terminal.
    let both = run(&mut sh, "count --nope &| upper");
    assert!(both.starts_with("COUNT: --NOPE: NO SUCH OPTION"), "{both}");
    let apart = run(&mut sh, "count --nope | upper");
    assert!(
        apart.starts_with("count: --nope: no such option"),
        "{apart}"
    );
    sh.host.runs.clear();
    assert_eq!(
        run(&mut sh, "say a | echo b"),
        "zosh: echo: a builtin or function can only start a pipeline\n  hint: run echo on its own line\n"
    );
    assert!(
        run(&mut sh, "say a | nope").starts_with("zosh: nope: no such command"),
        "a missing program"
    );
    assert!(sh.host.runs.is_empty(), "nothing of either ran");
    assert_eq!(
        run(&mut sh, "echo a | upper &"),
        "zosh: echo: a builtin or function cannot run in the background\n"
    );
    run(&mut sh, "say a | upper &");
    assert_eq!(ran(&sh), ["say a", "upper"]);
}

/// ADR-0152 rule 6 in the language: failures stop the line, a question's
/// "no" does not, and `SIGPIPE` before the last element is not a failure.
#[test]
fn pipeline_failures_stop_the_line_and_answers_do_not() {
    let mut sh = shell();
    assert_eq!(run(&mut sh, "say x | ask; echo after $status"), "after 1\n");
    assert_eq!(run(&mut sh, "piped | ok; echo after $status"), "after 0\n");
    assert!(!run(&mut sh, "ok | piped; echo after").contains("after"));
    assert!(!run(&mut sh, "fail | ok; echo after").contains("after"));
    assert_eq!(
        run(&mut sh, "try { fail | ok } catch { echo $status }"),
        "1\n"
    );
    assert_eq!(
        run(&mut sh, "if say x | ask { echo yes } else { echo no }"),
        "no\n"
    );
}

/// A job stopped in front stops its line as Ctrl-C does, with nothing more
/// said: the host has said `[N] stopped: LINE` (ADR-0158 rule 3).
#[test]
fn a_stopped_job_stops_its_line() {
    let mut sh = shell();
    assert_eq!(
        run(&mut sh, "zee; echo after"),
        "(the rest of the line was not run)\n"
    );
    assert_eq!(run(&mut sh, "status"), "interrupted\n");
    assert_eq!(run(&mut sh, "say a | zee && echo after"), "");
    assert_eq!(run(&mut sh, "zee"), "");
}

/// The report of a stopped line names the element whose record is the
/// pipeline's status (ADR-0151 rule 6, ADR-0152 rule 6), not the last.
#[test]
fn a_pipelines_report_names_the_element_that_failed() {
    let mut sh = shell();
    let crash = "crash: crashed (data abort, class 0x24)\n";
    assert_eq!(run(&mut sh, "crash | ok"), crash);
    assert_eq!(run(&mut sh, "crash | piped | upper"), crash);
    assert_eq!(run(&mut sh, "say x | crash | ok"), crash);
    assert_eq!(
        run(&mut sh, "crash | ok | piped"),
        "piped: ended by signal 13\n"
    );
    assert_eq!(
        run(&mut sh, "echo x | crash; echo after"),
        alloc::format!("{crash}(the rest of the line was not run)\n")
    );
}

/// INV-0153-3's cases (review S7, S8): a grant is `+r` or `+w` written on
/// the line and one path; from a value it is no grant, an empty or longer
/// list is refused, and so is a pattern, even one that matches one name.
#[test]
fn a_grant_is_written_and_is_one_path() {
    let mut sh = shell();
    let out = run(&mut sh, "set none; +w $none ok");
    assert_eq!(out, "zosh: +w: needs exactly one path\n");
    let out = run(&mut sh, "set two a b; +w $two ok");
    assert_eq!(out, "zosh: +w: needs exactly one path\n");
    let out = run(&mut sh, "+r a* ok");
    assert!(out.starts_with("zosh: a*: a grant names one path"), "{out}");
    assert!(
        sh.host.granted.is_empty(),
        "none of those ran: {:?}",
        sh.host.granted
    );
    // A marker from a value is a word, not a grant: the command is `+w /`.
    let out = run(&mut sh, "set g '+w /'; $g ok");
    assert!(out.starts_with("zosh: +w /: no such command"), "{out}");
    assert!(sh.host.granted.is_empty());
    // Written, with a path from a value: a grant, the value marked.
    run(&mut sh, "set d sub; +w $d ok");
    assert_eq!(sh.host.granted, [(true, "sub".into(), false)]);
    run(&mut sh, "+r a.txt ok");
    assert_eq!(sh.host.granted[1], (false, "a.txt".into(), true));
}

/// Review S5: a redirection's target from a value reaches only inside the
/// current directory (or a grant of a program's line); written out, it
/// goes where it says.
#[test]
fn a_redirection_from_a_value_stays_inside() {
    let mut sh = shell();
    sh.host.cwd = "sub".into();
    let out = run(&mut sh, "set f /config/settings; echo x > $f");
    assert!(
        out.starts_with("zosh: /config/settings: from a value, outside"),
        "{out}"
    );
    assert!(!sh.host.files.contains_key("config/settings"));
    let out = run(&mut sh, "set f /config/settings; say x > $f");
    assert!(
        out.starts_with("zosh: /config/settings: from a value, outside"),
        "{out}"
    );
    assert_eq!(run(&mut sh, "set f out.txt; echo x > $f"), "");
    assert_eq!(sh.host.files["sub/out.txt"], "x\n");
    assert_eq!(run(&mut sh, "echo y > /written.txt"), "");
    assert!(sh.host.files.contains_key("written.txt"));
}

/// Review S6: `grants` gets the line as a run would: marks kept, `+r` and
/// `+w` grants only as written.
#[test]
fn grants_sees_the_line_as_a_run_would() {
    let mut sh = shell();
    assert_eq!(
        run(&mut sh, "set d sub; grants +w $d cp $d x"),
        "grant true sub false\ncommand cp\narg sub false\narg x true\n"
    );
    assert_eq!(
        run(&mut sh, "set g +w; grants $g / rm x"),
        "command +w\narg / true\narg rm true\narg x true\n"
    );
    // Mutation testing: an option after the command's name is the
    // command's, not grants'.
    assert_eq!(
        run(&mut sh, "grants cp -f x"),
        "command cp\narg -f true\narg x true\n"
    );
}

fn invalid(line: &str) -> (usize, String) {
    match syntax::parse(line) {
        Err(Error::Invalid { at, problem, .. }) => (at, problem),
        other => panic!("{line:?}: {other:?}"),
    }
}

fn says(sh: &mut Shell<Fake>, line: &str) -> String {
    run(sh, line)
}

/// Mutation testing: `unset`, `set` alone and `set --query`, and the size
/// of a list a variable holds.
#[test]
fn variables_are_set_listed_queried_and_unset() {
    let mut sh = shell();
    run(&mut sh, "set x 1");
    assert_eq!(says(&mut sh, "set"), "status 0\nx 1\n");
    assert_eq!(says(&mut sh, "set --query x || echo unset"), "");
    run(&mut sh, "set e ''");
    assert_eq!(says(&mut sh, "set --query e || echo unset"), "unset\n");
    run(&mut sh, "unset x");
    assert!(says(&mut sh, "echo $x").contains("$x: not set"));
    run(&mut sh, "set my_name 1");
    assert_eq!(says(&mut sh, "echo $my_name"), "1\n");
    assert!(says(&mut sh, "set a.b 1").contains("not a variable name"));
    // The bound counts expanded values: a variable of 1000 words, used
    // MAX_VALUES / 1000 times, reaches it without a long line to parse.
    const PART: usize = 1000;
    assert_eq!(crate::eval::MAX_VALUES % PART, 0);
    run(&mut sh, &alloc::format!("set a{}", " w".repeat(PART)));
    let words = |n: usize| alloc::format!("set v{}", " $a".repeat(n));
    assert_eq!(says(&mut sh, &words(crate::eval::MAX_VALUES / PART)), "");
    assert!(says(&mut sh, &words(crate::eval::MAX_VALUES / PART + 1)).contains("too many values"));
}

/// Mutation testing: what `test` checks and says.
#[test]
fn test_checks_kinds_and_text() {
    let mut sh = shell();
    let yn = |sh: &mut Shell<Fake>, check: &str| {
        says(
            sh,
            &alloc::format!("if test {check} {{ echo y }} else {{ echo n }}"),
        )
    };
    assert_eq!(yn(&mut sh, "-d a.txt"), "n\n");
    assert_eq!(yn(&mut sh, "-f sub"), "n\n");
    assert_eq!(yn(&mut sh, "-f a.txt"), "y\n");
    assert_eq!(yn(&mut sh, "-d sub"), "y\n");
    assert_eq!(yn(&mut sh, "a '!=' b"), "y\n");
    assert_eq!(yn(&mut sh, "a '!=' a"), "n\n");
    assert_eq!(yn(&mut sh, "a = a"), "y\n");
    let unknown = yn(&mut sh, "a foo b");
    assert!(
        unknown.ends_with("n\n") && !unknown.lines().any(|l| l == "y"),
        "{unknown:?}"
    );
    assert!(says(&mut sh, "test -d").contains("one PATH is needed"));
    assert!(says(&mut sh, "test a").contains("not a check test knows"));
}

/// Mutation testing: `math`'s bounds are exact.
#[test]
fn maths_bounds_are_exact() {
    use crate::eval::math;
    // 4096 tokens: 2048 numbers and 2047 operators, then one more pair.
    let sum = |terms: usize| {
        let mut s = String::from("1");
        for _ in 1..terms {
            s.push_str("+1");
        }
        s
    };
    assert_eq!(math(&sum(257)), Ok(257));
    assert_eq!(math(&sum(2048)), Ok(2048));
    assert!(math(&sum(2049)).is_err());
    // Exactly 4096 tokens: a sign, 2048 numbers and 2047 operators.
    assert_eq!(math(&alloc::format!("-1{}", "+1".repeat(2047))), Ok(2046));
    assert_eq!(math(&alloc::format!("{}1", "-".repeat(255))), Ok(-1));
    assert!(math(&alloc::format!("{}1", "-".repeat(257))).is_err());
}

/// Mutation testing: every fault class in words, a signal number only
/// in range, and the Ctrl-C note only for an interrupt from it.
#[test]
fn records_name_their_classes_and_causes() {
    let fault = |detail| ExitRecord {
        code: 0,
        reason: ExitReason::Fault,
        detail,
        request_kind: None,
    };
    for (class, name) in [
        (0x00, "unknown instruction"),
        (0x15, "system call"),
        (0x20, "instruction abort"),
        (0x21, "instruction abort"),
        (0x22, "pc alignment"),
        (0x26, "stack alignment"),
        (0x3c, "breakpoint"),
    ] {
        assert_eq!(
            record::what(&fault(class)),
            alloc::format!("crashed ({name}, class {class:#x})")
        );
    }
    let ended = |detail, kind| ExitRecord {
        code: 0,
        reason: ExitReason::Terminated,
        detail,
        request_kind: kind,
    };
    assert_eq!(record::what(&ended(0, None)), "terminated");
    assert_eq!(record::what(&ended(u32::MAX, None)), "terminated");
    let t = ended(0, Some(RequestKind::Terminate));
    assert_eq!(
        record::screen_line("p", &t, true).as_deref(),
        Some("p: terminated")
    );
    let i = ended(0, Some(RequestKind::Interrupt));
    assert_eq!(
        record::screen_line("p", &i, false).as_deref(),
        Some("p: interrupted")
    );
    assert_eq!(record::describe(&exited(2)), "exit status 2");
    assert_eq!(record::describe(&exited(0)), "succeeded (exit status 0)");
}

/// Mutation testing: what the parser calls incomplete, and the problem
/// each mistake names.
#[test]
fn each_syntax_mistake_names_its_own_problem() {
    for incomplete in ["fn", "fn f\n", "for", "!"] {
        assert_eq!(
            syntax::parse(incomplete),
            Err(Error::Incomplete),
            "{incomplete:?}"
        );
    }
    for (line, problem) in [
        ("; echo", "a ; with no command before it"),
        ("else { }", "'else' with no block before it"),
        ("for f in a; { echo $f }", "for needs a block"),
        ("+r", "+r needs a path"),
        ("+r;", "+r needs a path"),
        ("| x", "| with no command before it"),
        ("& x", "& with no command before it"),
        ("&& x", "&& with no command before it"),
        (
            "{ echo }",
            "a block needs if, for, while, fn or try before it",
        ),
        ("echo >", "a redirection needs a file name"),
        ("echo > # x", "a redirection needs a file name"),
        ("echo a }", "unexpected text"),
        ("echo a {", "unexpected text"),
        ("echo $(if ok { echo a) })", "unexpected text"),
        ("if ok { if ok { echo } }x }", "unexpected text"),
        ("fn if { echo }", ""),
        ("fn 1x { echo }", ""),
        ("fn a.b { echo }", ""),
    ] {
        let (_, got) = invalid(line);
        if !problem.is_empty() {
            assert_eq!(got, problem, "{line:?}");
        }
    }
    let (at, _) = invalid("echo $x[0]");
    assert_eq!(at, 7, "at the index");
    let long = "x".repeat(syntax::MAX_SOURCE + 1);
    assert_eq!(invalid(&long).0, 0);
    assert!(syntax::parse(&long[1..]).is_ok(), "MAX_SOURCE bytes parse");
}

/// Mutation testing: lines the parser takes as they are written.
#[test]
fn the_parser_takes_what_is_well_formed() {
    for line in [
        "return",
        "return;",
        "fn r { return }",
        "fn back-up file { echo }",
        "fn back_up file { echo }",
        "echo a & echo b",
    ] {
        assert!(syntax::parse(line).is_ok(), "{line:?}");
    }
    let nested = alloc::format!("{}ok{}", "if ok { ".repeat(63), " }".repeat(63));
    assert!(syntax::parse(&nested).is_ok());
    assert!(syntax::parse(&"echo $(ok) ".repeat(70)).is_ok());
    let block = syntax::parse("echo a; ) echo b").map(|b| b.statements.len());
    assert!(block != Ok(1), "nothing after a ) is dropped: {block:?}");
    let mut sh = shell();
    run(&mut sh, "set x V");
    assert_eq!(says(&mut sh, "echo )"), ")\n");
    assert_eq!(says(&mut sh, "echo a)"), "a)\n");
    assert_eq!(says(&mut sh, "echo $(echo x) a)"), "x a)\n");
    assert_eq!(says(&mut sh, "echo ~''"), "/house\n");
    assert_eq!(says(&mut sh, "echo ab\"$x\""), "abV\n");
    assert_eq!(says(&mut sh, "if ok \\\n{ echo yes }"), "yes\n");
    assert_eq!(says(&mut sh, "echo }x"), "}x\n");
    let doc = alloc::format!("{}fn f {{\n}}\n", "# n\n".repeat(33));
    run(&mut sh, &doc);
    assert!(says(&mut sh, "!x").contains("x"));
}

/// ADR-0153 rule 4 (mutation testing found it broken): a function's
/// parameters and `$args` keep the marks of the words the caller wrote;
/// a variable that `set` or `for` gave is data, and so is a word that
/// mixes a variable with text.
#[test]
fn parameters_keep_the_callers_marks_and_variables_are_data() {
    let mut sh = shell();
    run(&mut sh, "fn view f { prog $f; prog $args }");
    run(&mut sh, "view ../x");
    let marks = &sh.host.marks;
    assert_eq!(
        marks[marks.len() - 2..],
        [vec![true], vec![true]],
        "typed at the call"
    );
    run(&mut sh, "set p ../x; view $p");
    let marks = &sh.host.marks;
    assert_eq!(
        marks[marks.len() - 2..],
        [vec![false], vec![false]],
        "a value at the call"
    );
    run(&mut sh, "set q ../x; prog $q");
    assert_eq!(sh.host.marks.last(), Some(&vec![false]), "set gives data");
    run(&mut sh, "for v in ../x { prog $v }");
    assert_eq!(sh.host.marks.last(), Some(&vec![false]), "for gives data");
    run(&mut sh, "fn pre f { prog x$f }; pre ../y");
    assert_eq!(sh.host.marks.last(), Some(&vec![false]), "mixed with text");
    // Mutation testing: a quoted variable is one text piece, and data.
    run(&mut sh, "set v x; prog \"$v\"");
    assert_eq!(sh.host.marks.last(), Some(&vec![false]), "a quoted value");
}

/// Mutation testing: the edit distance behind "did you mean", exactly.
#[test]
fn the_distance_is_exact() {
    use crate::eval::distance;
    let a64 = "a".repeat(64);
    let b64 = alloc::format!("{}b", "a".repeat(63));
    for (a, b, limit, d) in [
        ("ab", "abc", 1, 1),
        ("ab", "a", 2, 1),
        ("xyz", "q", 5, 3),
        ("a", "xy", 2, 2),
        ("zab", "zba", 2, 1),
        ("abcd", "xy", 2, 3),
        ("abcdef", "a", 2, 3),
        (a64.as_str(), b64.as_str(), 2, 1),
        (b64.as_str(), a64.as_str(), 2, 1),
    ] {
        assert_eq!(distance(a, b, limit), d, "{a} {b} {limit}");
    }
    assert_eq!(distance(&"a".repeat(65), &a64, 1), 2, "too long to compare");
    assert_eq!(distance(&a64, &"a".repeat(65), 1), 2);
    let mut sh = shell();
    assert_eq!(run(&mut sh, "zzzzzz"), "zosh: zzzzzz: no such command\n");
}

/// Mutation testing: indexes, chains, scripts and calls.
#[test]
fn indexes_chains_scripts_and_calls() {
    let mut sh = shell();
    run(&mut sh, "set a x; set b y; set files a.txt b.txt");
    assert_eq!(run(&mut sh, "echo $a$b"), "xy\n");
    assert_eq!(run(&mut sh, "echo $files[1] $files[-2]"), "a.txt a.txt\n");
    assert!(run(&mut sh, "echo $files[-3]").contains("$files[-3]: out of range: it has 2"));
    sh.host.runs.clear();
    run(&mut sh, "ok && fail && ok; ok 2");
    assert_eq!(ran(&sh), ["ok", "fail", "ok 2"]);
    sh.host
        .files
        .insert("last.zosh".into(), "echo one\nfail\n".into());
    let out = run(&mut sh, "source last.zosh");
    assert!(
        out.contains("last.zosh line 2") && !out.contains("the rest of"),
        "{out}"
    );
    sh.host
        .files
        .insert("set.zosh".into(), "set made 1\n".into());
    run(&mut sh, "source set.zosh");
    assert_eq!(run(&mut sh, "echo $made"), "1\n");
    sh.host.files.insert("hi.zosh".into(), "echo hi\n".into());
    assert_eq!(
        run(&mut sh, &"source hi.zosh; ".repeat(70)),
        "hi\n".repeat(70)
    );
    run(&mut sh, "fn f { ok }");
    assert_eq!(run(&mut sh, &"f; ".repeat(70)), "");
    assert_eq!(sh.state.function_origin("f"), Some("the prompt"));
    sh.host
        .files
        .insert("lib.zosh".into(), "fn g { ok }\n".into());
    run(&mut sh, "source lib.zosh");
    assert_eq!(sh.state.function_origin("g"), Some("lib.zosh"));
    run(&mut sh, "fn h { echo $e.x }; set e");
    assert!(
        run(&mut sh, "h").contains("$e.x"),
        "the function's own text"
    );
    assert!(run(&mut sh, "fn set { ok }").contains("is a builtin"));
    run(&mut sh, "set two 1 2");
    assert!(run(&mut sh, "fn r { return $two }; r").contains("return"));
}

/// Mutation testing: `continue` outside a loop, after every way a loop
/// can end.
#[test]
fn continue_outside_a_loop_after_any_loop() {
    for setup in [
        "for x in 1 { echo $x }",
        "while fail { echo x }",
        "for x in 1 { fail }",
        "while test 1 = 1 { fail }",
        "while ok { break }",
        "for x in 1 2 { break }",
    ] {
        let mut sh = shell();
        run(&mut sh, setup);
        let out = run(&mut sh, "continue; echo after");
        assert!(
            out.contains("continue: not inside a loop"),
            "{setup}: {out}"
        );
        assert!(!out.contains("after"), "{setup}: {out}");
    }
    for (n, setup) in [
        (1, "for x in 1 2 { echo $x }"),
        (3, "while test 1 = 1 { echo x }"),
    ] {
        let mut sh = shell();
        sh.host.interrupts = n;
        run(&mut sh, setup);
        assert!(
            run(&mut sh, "continue").contains("not inside a loop"),
            "{setup}"
        );
    }
}

/// Mutation testing: what runs in the shell in a pipeline, substitutions,
/// redirections and grants of one path, and options of a builtin.
#[test]
fn compound_commands_substitutions_and_redirections() {
    let mut sh = shell();
    for c in [
        "if ok { echo b }",
        "for x in 1 { echo b }",
        "while fail { echo b }",
        "try { echo b }",
        "fn q { echo b }",
    ] {
        let out = run(&mut sh, &alloc::format!("say a | {c}"));
        assert!(out.contains("can only start a pipeline"), "{c}: {out}");
    }
    assert_eq!(run(&mut sh, "if ok { echo a } | upper"), "A\n");
    assert_eq!(run(&mut sh, "for x in a { echo $x } | upper"), "A\n");
    assert_eq!(run(&mut sh, "try { echo a } | upper"), "A\n");
    assert!(run(&mut sh, "echo $(fail && ok)").contains("its command failed"));
    assert!(run(&mut sh, "fn f3 { echo $(echo x; return 3) }; f3").contains("its command failed"));
    assert_eq!(
        run(&mut sh, "fn g0 { echo $(echo x; return 0) }; g0"),
        "x\n"
    );
    run(&mut sh, "set files *.txt");
    sh.host.runs.clear();
    assert!(run(&mut sh, "echo x > $files").contains("exactly one file"));
    assert!(run(&mut sh, "prog > $files").contains("exactly one file"));
    assert!(ran(&sh).is_empty());
    assert!(!run(&mut sh, "+r a.txt prog").contains("zosh"));
    assert!(!run(&mut sh, "set p a.txt; +r $p prog").contains("zosh"));
    assert!(run(&mut sh, "+r *.txt prog").contains("a pattern is not one"));
    let out = run(&mut sh, "echo hi --nope");
    assert!(out.contains("--nope: no such option"), "{out}");
    assert_eq!(run(&mut sh, "test -d nope; echo after"), "after\n");
    assert!(!run(&mut sh, "math 1 / 0; echo after").contains("after"));
    run(&mut sh, "set v $(echo ''; big)");
    assert_eq!(run(&mut sh, "count $v"), "1\n");
}

/// Mutation testing: a line's end says the last answer, and a function's
/// one-line help has no description section.
#[test]
fn a_lines_end_and_a_short_help() {
    let mut sh = shell();
    let b = syntax::parse("ask no").unwrap();
    assert!(sh.run_line("ask no", &b).status.no);
    let b = syntax::parse("spin").unwrap();
    assert!(sh.run_line("spin", &b).status.ctrl_c);
    for (src, sections) in [("# one\nfn f {\n}\n", 0), ("# one\n# two\nfn f {\n}\n", 1)] {
        let block = syntax::parse(src).unwrap();
        let syntax::Command::Fn(f) = &block.statements[0].chain.first.elements[0].command else {
            panic!();
        };
        let d = crate::eval::function_description(f);
        assert_eq!(d.sections.len(), sections, "{src:?}");
    }
    let doc = alloc::format!("{}fn f {{\n}}\n", "# n\n".repeat(33));
    let block = syntax::parse(&doc).unwrap();
    let syntax::Command::Fn(f) = &block.statements[0].chain.first.elements[0].command else {
        panic!();
    };
    assert_eq!(f.doc.len(), 32);
}

/// Mutation testing: an interrupted or broken loop leaves no loop open, so
/// a later `continue` is outside one and stops its line.
#[test]
fn an_interrupted_loop_leaves_no_loop_open() {
    let note = "zosh: continue: not inside a loop\n(the rest of the line was not run)\n";
    for (n, setup) in [
        (1, "for x in 1 2 { echo $x }"),
        (3, "while test 1 = 1 { echo x }"),
        (0, "while break { echo x }"),
    ] {
        let mut sh = shell();
        sh.host.interrupts = n;
        run(&mut sh, setup);
        let out = run(&mut sh, "continue; echo after");
        assert!(out.ends_with(note), "{setup}: {out:?}");
        assert!(!out.contains("after\n"), "{setup}: {out:?}");
    }
}

/// Mutation testing and review S21: a loop interrupted inside a function
/// stops the function's line, and `$status` is an interrupted command's.
#[test]
fn an_interrupt_inside_a_function_stops_its_line() {
    let mut sh = shell();
    run(&mut sh, "fn f { while test 1 = 1 { echo x } }");
    sh.host.interrupts = 3;
    let out = run(&mut sh, "f; echo after");
    assert!(!out.contains("after"), "{out:?}");
    assert_eq!(run(&mut sh, "echo $status"), "interrupted\n");
    // Review S21: at the prompt too, as for a program at Ctrl-C.
    sh.host.interrupts = 3;
    run(&mut sh, "while test 1 = 1 { echo x }");
    assert_eq!(run(&mut sh, "echo $status"), "interrupted\n");
    sh.host.interrupts = 1;
    run(&mut sh, "for x in 1 2 { echo $x }");
    assert_eq!(run(&mut sh, "echo $status"), "interrupted\n");
}

/// Mutation testing: one word may expand to exactly MAX_VALUES values.
#[test]
fn one_word_of_max_values() {
    let mut sh = shell();
    const PART: usize = 1000;
    run(&mut sh, &alloc::format!("set a{}", " w".repeat(PART)));
    let all = alloc::format!("set v{}", " $a".repeat(crate::eval::MAX_VALUES / PART));
    assert_eq!(run(&mut sh, &all), "");
    assert_eq!(
        run(&mut sh, "count $v"),
        alloc::format!("{}\n", crate::eval::MAX_VALUES)
    );
}

/// Mutation testing: `""` is one empty word.
#[test]
fn an_empty_double_quoted_word_is_a_word() {
    let mut sh = shell();
    assert_eq!(run(&mut sh, "count \"\""), "1\n");
    assert_eq!(run(&mut sh, "count '' \"\""), "2\n");
}

/// Mutation testing: a builtin is told whether its output goes to the
/// terminal (it may page it).
#[test]
fn a_builtin_knows_when_its_output_is_the_terminal() {
    let mut sh = shell();
    run(&mut sh, "pager");
    run(&mut sh, "echo $(pager)");
    assert_eq!(sh.host.terminals, [true, false]);
}

/// The parts of the second word of `src`, a simple command.
fn second_word_parts(src: &str) -> Vec<crate::syntax::Part> {
    let block = crate::syntax::parse(src).unwrap();
    match &block.statements[0].chain.first.elements[0].command {
        crate::syntax::Command::Simple(s) => s.words[1].parts.clone(),
        other => panic!("{other:?}"),
    }
}

/// Mutation testing: quoted text is one part, with no empty part before
/// it, and runs of quoted and escaped text join.
#[test]
fn quoted_text_is_one_part() {
    use crate::syntax::Part::Quoted;
    assert_eq!(second_word_parts("echo 'abc'"), [Quoted("abc".into())]);
    assert_eq!(second_word_parts("echo \"abc\""), [Quoted("abc".into())]);
    assert_eq!(second_word_parts("echo 'x'\\y"), [Quoted("xy".into())]);
    assert_eq!(second_word_parts("echo \"\""), [Quoted(String::new())]);
}

/// A function that fails at the start of a pipeline stops its own body;
/// the pipeline, which fails when any element fails (ADR-0151 rule 4),
/// runs its programs and then stops the line.
#[test]
fn a_failing_function_stops_its_pipeline() {
    let mut sh = shell();
    run(&mut sh, "fn f { fail; echo x }");
    let before = sh.host.runs.len();
    let out = run(&mut sh, "f | ok; echo after");
    let names: Vec<&str> = sh.host.runs[before..]
        .iter()
        .map(|r| r.0.as_str())
        .collect();
    assert_eq!(names, ["fail", "ok"], "{out:?}");
    assert!(!out.contains("x\n"), "the body stopped: {out:?}");
    assert!(!out.contains("after"), "{out:?}");
}

#[test]
fn settings_reset_takes_its_name_as_the_options_argument() {
    let mut sh = shell();
    assert_eq!(
        run(&mut sh, "settings --reset shell.colour"),
        "option reset shell.colour\n"
    );
}

#[test]
fn a_dash_item_after_a_settings_name_is_a_value() {
    let mut sh = shell();
    let want = "operand NAME gui.keyboard-layouts\noperand VALUE -cz\n";
    assert_eq!(run(&mut sh, "settings gui.keyboard-layouts -cz"), want);
    assert_eq!(run(&mut sh, "settings gui.keyboard-layouts -- -cz"), want);
}

#[test]
fn settings_at_stays_an_option_wherever_it_stands() {
    let mut sh = shell();
    assert_eq!(
        run(&mut sh, "settings gui.keyboard-layouts +ru --at 1"),
        "option at 1\noperand NAME gui.keyboard-layouts\noperand VALUE +ru\n"
    );
    assert_eq!(
        run(&mut sh, "settings gui.keyboard-layouts us ua +ru --at 1"),
        "option at 1\noperand NAME gui.keyboard-layouts\noperand VALUE us\n\
         operand VALUE ua\noperand VALUE +ru\n"
    );
    assert_eq!(
        run(&mut sh, "settings gui.keyboard-layouts --at 1"),
        "option at 1\noperand NAME gui.keyboard-layouts\n"
    );
}

#[test]
fn a_settings_at_with_nothing_after_it_is_a_value() {
    let mut sh = shell();
    assert_eq!(
        run(&mut sh, "settings gui.keyboard-layouts +ru --at"),
        "operand NAME gui.keyboard-layouts\noperand VALUE +ru\noperand VALUE --at\n"
    );
}
