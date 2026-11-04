//! Host tests of the language (ADR-0150, ADR-0151) over a fake host.

use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::{ExitReason, ExitRecord, RequestKind};
use zoraya_command::Parsed;
use zoraya_describe::Description;

use crate::eval::{Found, Host, Io, Pipe, Ran, Shell, Sink, State, Value};
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
    screen: String,
    outputs: BTreeMap<u32, (String, bool)>,
    next: u32,
    interrupts: u32,
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
    fn lookup(&mut self, name: &str) -> Found {
        if self.programs.contains_key(name) {
            Found::Program
        } else {
            Found::Nothing
        }
    }

    fn description(&mut self, name: &str) -> Option<Rc<Description>> {
        (name == "ask").then(|| Rc::new(zoraya_describe::parse(QUESTION.as_bytes()).unwrap()))
    }

    fn names(&mut self) -> Vec<String> {
        let mut n: Vec<String> = self.programs.keys().map(|s| s.to_string()).collect();
        n.push("ls".into());
        n
    }

    fn builtin(
        &mut self,
        _: &mut State,
        _: &str,
        _: &Description,
        _: &Parsed<'_>,
        _: &mut Io,
    ) -> i32 {
        0
    }

    /// Runs the programs in turn, each reading what the one before printed.
    fn program(&mut self, pipe: &Pipe<'_>) -> Ran {
        for exec in pipe.programs {
            let args: Vec<String> = exec.args.iter().map(|a| a.text.clone()).collect();
            self.runs.push((exec.name.into(), args, exec.env.to_vec()));
        }
        if pipe.background {
            return Ran::Background(1);
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
