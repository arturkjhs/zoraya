//! The evaluator (ADR-0150, ADR-0151): expands words and runs the tree over
//! a [`Host`], which runs programs and the builtins that act on the system.
//! The language's own builtins (`set`, `unset`, `export`, `count`, `echo`,
//! `test`, `math`, `status`, `source`) and functions are here.
//!
//! The stop rule (ADR-0151 rule 4): a failure nothing checks stops the
//! statement list it is in, and every one around it, up to the line, the
//! script or a check. A check is the condition of `if` or `while`, `!`, the
//! left side of `&&` or `||`, or `try`.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use zoraya_abi::ExitRecord;
use zoraya_command::{Outcome, Parsed};
use zoraya_describe::{Arity, Description, Kind, Operand, What};

mod language;

pub use language::{
    DESCRIPTIONS as LANGUAGE_DESCRIPTIONS, description as language_description, math,
};

use crate::glob::{self, Lister};
use crate::record::{self, exited, is_success};
use crate::syntax::{
    self, Block, Chain, Command, Element, Function, Join, Part, Pipeline, Simple, Statement,
    Stream, Word,
};

/// Deepest nesting of function calls and `source`.
pub const MAX_CALLS: usize = 64;
/// Most elements a variable holds.
pub const MAX_VALUES: usize = 100_000;

/// One element of a list, with whether it came from text the person wrote
/// on the line (ADR-0153 rule 4): a function's parameters keep the mark of
/// the caller's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    pub text: String,
    pub literal: bool,
}

impl Value {
    pub fn literal(text: &str) -> Value {
        Value {
            text: text.into(),
            literal: true,
        }
    }

    pub fn data(text: &str) -> Value {
        Value {
            text: text.into(),
            literal: false,
        }
    }
}

/// What a name is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// A builtin the host runs.
    Builtin,
    /// One of the language's builtins, run here.
    Language,
    Function,
    Program,
    Nothing,
}

/// A program of a pipeline, for the host to run.
#[derive(Debug)]
pub struct Exec<'a> {
    pub name: &'a str,
    pub args: &'a [Value],
    /// `NAME=value` for the child's environment (ADR-0150 rule 6).
    pub env: &'a [String],
    pub grants: &'a [(bool, Value)],
    /// Its own redirections, which take the place of what the pipeline
    /// gives that stream (ADR-0152 rule 3).
    pub redirects: &'a [(Stream, bool, String)],
    /// `&|` after it: its `err` goes into the pipe too (ADR-0152 rule 2).
    pub err_too: bool,
}

/// A pipeline of programs (ADR-0152 rules 1 and 2): one stream per `|`,
/// all started as one job.
#[derive(Debug)]
pub struct Pipe<'a> {
    pub programs: &'a [Exec<'a>],
    /// What the first program reads: the output of the builtin or function
    /// that started the pipeline (rule 5). `None`: the terminal, for a
    /// foreground job; nothing (end of file) for a background one.
    pub input: Option<&'a str>,
    /// Where the last program's `out` and every `err` go, unless a
    /// redirection says otherwise.
    pub out: Sink,
    pub err: Sink,
    pub background: bool,
    /// The line as written, for job lists.
    pub line: &'a str,
}

/// How a pipeline ended, or why it did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ran {
    Ended {
        /// One per program, in order.
        records: Vec<ExitRecord>,
        /// The terminal's Ctrl-C sent its interrupt.
        ctrl_c: bool,
        /// What went to [`Sink::Capture`].
        captured: String,
    },
    /// Started in the background as job `n`.
    Background(u32),
    /// Stopped in front by Ctrl-Z, and kept as job `n` (ADR-0158 rule 3);
    /// the host said so.
    Stopped(u32),
    /// Not started; the host said why, already printed.
    Refused(i32),
}

/// Output a builtin produced, for the shell to route.
#[derive(Debug, Default)]
pub struct Io {
    pub out: String,
    pub err: String,
    /// Output goes to the terminal: a builtin may page it.
    pub terminal: bool,
}

/// Where an output stream goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sink {
    Terminal,
    /// A file the host opened.
    File(u32),
    /// The innermost `$(...)`.
    Capture,
}

/// What the shell needs from the system.
pub trait Host {
    /// What `name` is, functions and the language's builtins aside.
    fn lookup(&mut self, name: &str) -> Found;
    /// The description of a builtin or program.
    fn description(&mut self, name: &str) -> Option<Rc<Description>>;
    /// Every command name, for suggestions.
    fn names(&mut self) -> Vec<String>;
    /// Runs a host builtin. Its arguments are parsed already.
    fn builtin(
        &mut self,
        shell: &mut State,
        name: &str,
        d: &Description,
        parsed: &Parsed<'_>,
        io: &mut Io,
    ) -> i32;
    fn program(&mut self, pipe: &Pipe<'_>) -> Ran;
    /// `dir` is `""` for the current directory.
    fn list(&mut self, dir: &str) -> Option<Vec<(String, bool)>>;
    /// A whole file, as text.
    fn read(&mut self, path: &str) -> Result<String, String>;
    /// `Some(true)` for a directory, `Some(false)` for a file.
    fn kind(&mut self, path: &str) -> Option<bool>;
    fn open_output(&mut self, path: &str, append: bool) -> Result<u32, String>;
    fn write(&mut self, sink: Sink, text: &str);
    fn close_output(&mut self, token: u32);
    /// The home directory (`shell.home`).
    fn home(&mut self) -> String;
    /// The current directory, absolute.
    fn cwd(&mut self) -> String;
    /// `grants`: what running `exec` would give, printed into `io`.
    fn grants(&mut self, state: &mut State, exec: &Exec<'_>, io: &mut Io) -> i32;
    /// True once if Ctrl-C was pressed while the shell itself was busy in
    /// a loop. Asked before each round of `while` and `for`; other keys
    /// must be kept for the prompt.
    fn interrupted(&mut self) -> bool;
}

struct HostLister<'a, H: Host>(&'a mut H);

impl<H: Host> Lister for HostLister<'_, H> {
    fn list(&mut self, dir: &str) -> Option<Vec<(String, bool)>> {
        self.0.list(dir)
    }
}

#[derive(Debug, Clone, Default)]
struct Var {
    values: Vec<Value>,
    exported: bool,
}

/// A failure that stops what contains it.
#[derive(Debug, Clone)]
pub struct Failure {
    pub record: ExitRecord,
    pub name: String,
    pub ctrl_c: bool,
    /// Already reported where it stopped a script.
    pub reported: bool,
}

/// How evaluation left a statement list.
#[derive(Debug)]
pub enum Flow {
    Stop(Failure),
    Break,
    Continue,
    Return(ExitRecord),
}

type Run = Result<Result_, Flow>;

/// A command's result: its record, and whether it was a question's "no".
#[derive(Debug, Clone, Copy)]
pub struct Result_ {
    pub record: ExitRecord,
    pub no: bool,
    pub ctrl_c: bool,
    /// Its failure was reported already (by the script it stopped).
    pub reported: bool,
}

impl Result_ {
    fn of(record: ExitRecord) -> Result_ {
        Result_ {
            record,
            no: false,
            ctrl_c: false,
            reported: false,
        }
    }

    fn code(code: i32) -> Result_ {
        Result_::of(exited(code))
    }

    pub fn success(&self) -> bool {
        is_success(&self.record)
    }

    pub fn failed(&self) -> bool {
        !self.success() && !self.no
    }
}

/// The shell's state that builtins see.
pub struct State {
    globals: BTreeMap<String, Var>,
    frames: Vec<BTreeMap<String, Var>>,
    functions: BTreeMap<String, Rc<Function>>,
    /// Where each function came from, for `help`.
    origins: BTreeMap<String, String>,
    last: ExitRecord,
    calls: usize,
    sinks: Vec<(Sink, Sink)>,
    captures: Vec<String>,
    /// The script being run, if any, for messages.
    script: Option<String>,
    loops: usize,
    /// The text each function was parsed from: its spans point into it.
    sources: BTreeMap<String, Rc<str>>,
}

impl State {
    pub fn new() -> State {
        State {
            globals: BTreeMap::new(),
            frames: Vec::new(),
            functions: BTreeMap::new(),
            origins: BTreeMap::new(),
            last: exited(0),
            calls: 0,
            sinks: Vec::new(),
            captures: Vec::new(),
            script: None,
            loops: 0,
            sources: BTreeMap::new(),
        }
    }

    /// The last command's record (`status`).
    pub fn last(&self) -> ExitRecord {
        self.last
    }

    pub fn get(&self, name: &str) -> Option<&[Value]> {
        if let Some(frame) = self.frames.last()
            && let Some(v) = frame.get(name)
        {
            return Some(&v.values);
        }
        self.globals.get(name).map(|v| v.values.as_slice())
    }

    fn set(&mut self, name: &str, values: Vec<Value>, global: bool) -> Result<(), String> {
        if name == "status" {
            return Err("status is set by the shell".into());
        }
        let target = match self.frames.last_mut() {
            Some(frame) if !global => frame,
            _ => &mut self.globals,
        };
        let var = target.entry(name.into()).or_default();
        if var.exported && values.len() != 1 {
            return Err(format!("{name} is exported, so it holds exactly one value"));
        }
        // Rule 4 (ADR-0153): a variable's values are data, whatever marks
        // the words that gave them had; only a call's parameters and
        // `$args` keep the caller's marks, and they are not set here.
        var.values = values.iter().map(|v| Value::data(&v.text)).collect();
        Ok(())
    }

    fn unset(&mut self, name: &str) {
        if let Some(frame) = self.frames.last_mut()
            && frame.remove(name).is_some()
        {
            return;
        }
        self.globals.remove(name);
    }

    /// Every variable name visible here, sorted.
    pub fn variable_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.globals.keys().cloned().collect();
        if let Some(frame) = self.frames.last() {
            names.extend(frame.keys().cloned());
        }
        names.push("status".into());
        names.sort();
        names.dedup();
        names
    }

    /// The environment vector of a child (ADR-0150 rule 6, INV-0150-4):
    /// exported variables, then this command's own `NAME=value`, which wins.
    pub fn environment(&self, own: &[(String, String)]) -> Vec<String> {
        let mut env: BTreeMap<&str, &str> = BTreeMap::new();
        for (name, var) in &self.globals {
            if var.exported
                && let [one] = var.values.as_slice()
            {
                env.insert(name, &one.text);
            }
        }
        for (name, value) in own {
            env.insert(name, value);
        }
        env.into_iter().map(|(n, v)| format!("{n}={v}")).collect()
    }

    pub fn function(&self, name: &str) -> Option<Rc<Function>> {
        self.functions.get(name).cloned()
    }

    pub fn function_names(&self) -> Vec<String> {
        self.functions.keys().cloned().collect()
    }

    /// Where function `name` was defined: `/config/start.zosh`, or `the prompt`.
    pub fn function_origin(&self, name: &str) -> Option<&str> {
        self.origins.get(name).map(String::as_str)
    }
}

impl Default for State {
    fn default() -> State {
        State::new()
    }
}

/// A function's description, from its comment lines (ADR-0156 item 3).
pub fn function_description(f: &Function) -> Description {
    let summary = f
        .doc
        .first()
        .cloned()
        .unwrap_or_else(|| "a function".into());
    let mut d = Description::new(What::Command, &f.name, &summary);
    let mut usage = f.name.clone();
    for p in &f.params {
        let upper = p.to_uppercase();
        usage.push(' ');
        usage.push_str(&upper);
        d.operands.push(Operand {
            name: upper,
            arity: Arity::One,
            kind: Kind::Text,
            text: String::from("an argument"),
        });
    }
    d.usage.push(usage);
    if f.doc.len() > 1 {
        d.sections.push(zoraya_describe::Section {
            title: None,
            lines: f.doc[1..].to_vec(),
        });
    }
    d
}

/// Familiar names from other systems, with what to type here (ADR-0154
/// rule 6, as decided).
pub const UNIX_HINTS: [(&str, &str); 6] = [
    ("grep", "on Zoraya this is search: search TEXT FILE..."),
    ("wc", "to count lines: count --lines FILE..."),
    ("man", "on Zoraya this is help: help NAME"),
    (
        "less",
        "to print a file: cat FILE (help pages page by themselves)",
    ),
    (
        "more",
        "to print a file: cat FILE (help pages page by themselves)",
    ),
    (
        "which",
        "help NAME says what NAME is and where it comes from",
    ),
];

/// Edit distance with adjacent transpositions counted as one (`sl` and `ls`
/// are 1 apart), bounded: anything over `limit` is `limit + 1`.
pub fn distance(a: &str, b: &str, limit: usize) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > limit || a.len() > 64 || b.len() > 64 {
        return limit + 1;
    }
    let w = b.len() + 1;
    let mut d = alloc::vec![0usize; (a.len() + 1) * w];
    for i in 0..=a.len() {
        d[i * w] = i;
    }
    for (j, cell) in d.iter_mut().enumerate().take(w) {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[(i - 1) * w + j] + 1)
                .min(d[i * w + j - 1] + 1)
                .min(d[(i - 1) * w + j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[(i - 2) * w + j - 2] + 1);
            }
            d[i * w + j] = v;
        }
    }
    d[a.len() * w + b.len()].min(limit + 1)
}

/// The known names nearest to `name`, at distance 2 at most: only those at
/// the smallest distance found, at most three, sorted.
pub fn nearest(name: &str, names: &[String]) -> Vec<String> {
    let mut found: Vec<(usize, &String)> = names
        .iter()
        .map(|n| (distance(name, n, 2), n))
        .filter(|(d, n)| *d <= 2 && n.as_str() != name)
        .collect();
    found.sort();
    found.dedup();
    let best = found.first().map_or(0, |(d, _)| *d);
    found
        .into_iter()
        .take_while(|(d, _)| *d == best)
        .take(3)
        .map(|(_, n)| n.clone())
        .collect()
}

/// The language's builtins, run here.
pub const LANGUAGE: [&str; 9] = [
    "set", "unset", "export", "count", "echo", "test", "math", "status", "source",
];

/// The result of a line at the prompt.
#[derive(Debug)]
pub struct LineEnd {
    pub status: Result_,
}

pub struct Shell<H: Host> {
    pub host: H,
    pub state: State,
    /// The name of the last command run, for failure lines.
    last_name: String,
    /// The failure that stopped the script `source` ran.
    sourced: Option<Result_>,
}

/// An error the shell reports itself: `zosh: SUBJECT: PROBLEM`.
struct ShellError {
    subject: String,
    problem: String,
    hints: Vec<String>,
    code: i32,
}

fn shell_error(subject: &str, problem: &str) -> ShellError {
    ShellError {
        subject: subject.into(),
        problem: problem.into(),
        hints: Vec::new(),
        code: 1,
    }
}

/// How the values of a word may expand.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Command,
    /// `for` values and `set`: a pattern that matches nothing is empty.
    EmptyOk,
    /// The path after `+r` or `+w`: no pattern (rule 3).
    Grant,
}

impl<H: Host> Shell<H> {
    pub fn new(host: H) -> Shell<H> {
        Shell {
            host,
            state: State::new(),
            last_name: String::new(),
            sourced: None,
        }
    }

    fn sink_out(&self) -> Sink {
        self.state.sinks.last().map_or(Sink::Terminal, |s| s.0)
    }

    fn sink_err(&self) -> Sink {
        self.state.sinks.last().map_or(Sink::Terminal, |s| s.1)
    }

    fn emit(&mut self, sink: Sink, text: &str) {
        if text.is_empty() {
            return;
        }
        match sink {
            Sink::Capture => {
                if let Some(c) = self.state.captures.last_mut()
                    && c.len() + text.len() <= syntax::MAX_SOURCE
                {
                    c.push_str(text);
                }
            }
            other => self.host.write(other, text),
        }
    }

    /// Writes to the current output.
    pub fn out(&mut self, text: &str) {
        let s = self.sink_out();
        self.emit(s, text);
    }

    /// Writes to the current error output.
    pub fn err(&mut self, text: &str) {
        let s = self.sink_err();
        self.emit(s, text);
    }

    fn report(&mut self, e: &ShellError) -> Result_ {
        let hints: Vec<&str> = e.hints.iter().map(String::as_str).collect();
        let line = zoraya_command::error("zosh", Some(&e.subject), &e.problem, &hints);
        self.err(&line);
        Result_::code(e.code)
    }

    /// Defines a function from outside a line (tests, the start file).
    pub fn set_global(&mut self, name: &str, values: &[&str]) {
        let values = values.iter().map(|v| Value::literal(v)).collect();
        let _ = self.state.set(name, values, true);
    }

    /// What `name` names: function, builtin, program or nothing.
    pub fn find(&mut self, name: &str) -> Found {
        if self.state.functions.contains_key(name) {
            Found::Function
        } else if LANGUAGE.contains(&name) {
            Found::Language
        } else {
            self.host.lookup(name)
        }
    }

    /// Runs a line typed at the prompt. Parse errors are the caller's: it
    /// has the line to show them on.
    pub fn run_line(&mut self, src: &str, block: &Block) -> LineEnd {
        let count = block.statements.len();
        for (i, statement) in block.statements.iter().enumerate() {
            match self.statement(src, statement) {
                Ok(r) => {
                    if i + 1 == count {
                        return LineEnd { status: r };
                    }
                }
                Err(Flow::Stop(f)) => {
                    // A loop's interrupt comes here without a statement
                    // that recorded it (review S21).
                    self.state.last = f.record;
                    if !f.reported
                        && let Some(line) = record::screen_line(&f.name, &f.record, f.ctrl_c)
                    {
                        self.err(&format!("{line}\n"));
                    }
                    if i + 1 < count {
                        self.err("(the rest of the line was not run)\n");
                    }
                    return LineEnd {
                        status: Result_ {
                            record: f.record,
                            no: false,
                            ctrl_c: f.ctrl_c,
                            reported: f.reported,
                        },
                    };
                }
                Err(Flow::Break | Flow::Continue) => {
                    let r = self.report(&shell_error("break", "not inside a loop"));
                    self.state.last = r.record;
                    return LineEnd { status: r };
                }
                Err(Flow::Return(record)) => {
                    let r = self.report(&shell_error("return", "not inside a function"));
                    let _ = record;
                    self.state.last = r.record;
                    return LineEnd { status: r };
                }
            }
        }
        LineEnd {
            status: Result_::of(self.state.last),
        }
    }

    /// Runs a script (`source`, the start file): its failure stops it and is
    /// reported with its line (ADR-0151 rule 6).
    pub fn run_script(&mut self, name: &str, src: &str, args: Vec<Value>) -> Result_ {
        let block = match syntax::parse(src) {
            Ok(b) => b,
            Err(e) => {
                let (at, problem) = match e {
                    syntax::Error::Incomplete => {
                        (src.len(), String::from("the script ends too soon"))
                    }
                    syntax::Error::Invalid { at, problem, .. } => (at, problem),
                };
                let (line, _) = syntax::position(src, at);
                let msg = format!("zosh: {name} line {line}: {problem}\n");
                self.err(&msg);
                return Result_::code(1);
            }
        };
        if self.state.calls >= MAX_CALLS {
            return self.report(&shell_error(name, "scripts call each other too deeply"));
        }
        self.state.calls += 1;
        let saved_script = self.state.script.replace(name.into());
        let mut frame = BTreeMap::new();
        frame.insert(
            "args".into(),
            Var {
                values: args,
                exported: false,
            },
        );
        let in_function = !self.state.frames.is_empty();
        if in_function {
            self.state.frames.push(frame);
        } else {
            self.state
                .globals
                .insert("args".into(), frame.remove("args").unwrap_or_default());
        }
        let count = block.statements.len();
        let mut result = Result_::of(exited(0));
        for (i, statement) in block.statements.iter().enumerate() {
            match self.statement(src, statement) {
                Ok(r) => result = r,
                Err(Flow::Stop(mut f)) => {
                    let (line, _) = syntax::position(src, statement.span.start);
                    if !f.reported {
                        let msg = format!(
                            "zosh: {name} line {line}: {}: {}\n",
                            f.name,
                            record::what(&f.record)
                        );
                        self.err(&msg);
                        f.reported = true;
                    }
                    if i + 1 < count {
                        self.err(&format!("(the rest of {name} was not run)\n"));
                    }
                    result = Result_ {
                        record: f.record,
                        no: false,
                        ctrl_c: f.ctrl_c,
                        reported: true,
                    };
                    break;
                }
                Err(Flow::Return(record)) => {
                    result = Result_::of(record);
                    break;
                }
                Err(Flow::Break | Flow::Continue) => {
                    result = self.report(&shell_error("break", "not inside a loop"));
                    break;
                }
            }
        }
        if in_function {
            self.state.frames.pop();
        }
        self.state.script = saved_script;
        self.state.calls -= 1;
        result
    }

    fn statement(&mut self, src: &str, s: &Statement) -> Run {
        let line = src
            .get(s.span.start..s.span.end)
            .unwrap_or("")
            .trim_end_matches([' ', '\t', '&', ';', '\n']);
        let (r, checked) = self.chain(src, &s.chain, false, s.background, line)?;
        self.state.last = r.record;
        if r.failed() && !checked {
            return Err(Flow::Stop(Failure {
                record: r.record,
                name: self.last_name.clone(),
                ctrl_c: r.ctrl_c,
                reported: r.reported,
            }));
        }
        Ok(r)
    }

    /// Gives the chain's result and whether its last run part was checked.
    fn chain(
        &mut self,
        src: &str,
        c: &Chain,
        checked: bool,
        background: bool,
        line: &str,
    ) -> Result<(Result_, bool), Flow> {
        let left_checked = checked || !c.rest.is_empty();
        let mut r = self.pipeline(src, &c.first, left_checked, background, line)?;
        self.state.last = r.record;
        let mut was_checked = left_checked;
        for (i, (join, p)) in c.rest.iter().enumerate() {
            let run = match join {
                Join::And => r.success(),
                Join::Or => !r.success(),
            };
            if !run {
                continue;
            }
            let last = i + 1 == c.rest.len();
            was_checked = checked || !last;
            r = self.pipeline(src, p, was_checked, background, line)?;
            self.state.last = r.record;
        }
        Ok((r, was_checked || checked))
    }

    fn pipeline(
        &mut self,
        src: &str,
        p: &Pipeline,
        checked: bool,
        background: bool,
        line: &str,
    ) -> Run {
        let checked = checked || p.negated;
        let r = if p.elements.len() > 1 {
            self.several(src, p, background, line)?
        } else {
            self.element(src, &p.elements[0], checked, background, line)?
        };
        if p.negated {
            let ok = !r.success();
            return Ok(Result_::code(if ok { 0 } else { 1 }));
        }
        Ok(r)
    }

    /// A pipeline of more than one element (ADR-0152 rules 2, 5 and 6).
    fn several(&mut self, src: &str, p: &Pipeline, background: bool, line: &str) -> Run {
        for e in &p.elements[1..] {
            if let Some(name) = self.runs_in_shell(&e.command) {
                self.last_name = name.clone();
                return Ok(self.report(&only_first(&name)));
            }
        }
        let first = &p.elements[0];
        let in_shell = self.runs_in_shell(&first.command);
        let mut members = Vec::new();
        let mut names = Vec::new();
        let mut input = None;
        if let Some(name) = &in_shell {
            if background {
                self.last_name = name.clone();
                return Ok(self.report(&shell_error(
                    name,
                    "a builtin or function cannot run in the background",
                )));
            }
            // Rule 5: its output is the first program's input.
            self.state.captures.push(String::new());
            let err = if first.err_too {
                Sink::Capture
            } else {
                self.sink_err()
            };
            self.state.sinks.push((Sink::Capture, err));
            let r = self.element(src, first, true, false, line);
            self.state.sinks.pop();
            let text = self.state.captures.pop().unwrap_or_default();
            let r = r?;
            members.push(Member {
                record: r.record,
                question: r.no,
            });
            names.push(name.clone());
            input = Some(text);
        }
        let rest = if in_shell.is_some() {
            &p.elements[1..]
        } else {
            &p.elements[..]
        };
        let mut prepared = Vec::new();
        for e in rest {
            let Command::Simple(s) = &e.command else {
                return Ok(self.report(&shell_error("|", "only commands can be piped")));
            };
            match self.program_of(src, s) {
                Ok(mut x) => {
                    x.err_too = e.err_too;
                    prepared.push(x);
                }
                Err(r) => return Ok(r),
            }
        }
        let execs: Vec<Exec<'_>> = prepared.iter().map(Prepared::exec).collect();
        let pipe = Pipe {
            programs: &execs,
            input: input.as_deref(),
            out: self.sink_out(),
            err: self.sink_err(),
            background,
            line,
        };
        let ran = self.host.program(&pipe);
        let questions: Vec<bool> = prepared
            .iter()
            .map(|x| self.host.description(&x.name).is_some_and(|d| d.question))
            .collect();
        match ran {
            Ran::Ended {
                records,
                ctrl_c,
                captured,
            } => {
                self.emit(Sink::Capture, &captured);
                for (record, question) in records.into_iter().zip(questions) {
                    members.push(Member { record, question });
                }
                names.extend(prepared.iter().map(|x| x.name.clone()));
                // A stop reports the element that failed, not the last.
                if let Some(i) = pipeline_failure(&members) {
                    self.last_name = names[i].clone();
                }
                let mut r = pipeline_status(&members);
                r.ctrl_c = ctrl_c;
                Ok(r)
            }
            Ran::Background(_) => Ok(Result_::code(0)),
            Ran::Stopped(_) => Ok(stopped()),
            Ran::Refused(code) => Ok(Result_::code(code)),
        }
    }

    /// A pipeline element after the first: a program, prepared to run.
    fn program_of(&mut self, src: &str, s: &Simple) -> Result<Prepared, Result_> {
        let args = self.arguments(src, s)?;
        let name = args[0].text.clone();
        self.last_name = name.clone();
        match self.find(&name) {
            Found::Program => self.prepare(src, s, &args),
            Found::Nothing => Err(self.not_found(&name)),
            _ => Err(self.report(&only_first(&name))),
        }
    }

    /// The name of a builtin or function the element would run in the shell.
    fn runs_in_shell(&mut self, c: &Command) -> Option<String> {
        match c {
            Command::Simple(s) => {
                let name = s.words.first()?.literal()?.to_string();
                matches!(
                    self.find(&name),
                    Found::Builtin | Found::Language | Found::Function
                )
                .then_some(name)
            }
            Command::If { .. } => Some("if".into()),
            Command::For { .. } => Some("for".into()),
            Command::While { .. } => Some("while".into()),
            Command::Try { .. } => Some("try".into()),
            Command::Fn(_) => Some("fn".into()),
            _ => None,
        }
    }

    fn element(
        &mut self,
        src: &str,
        e: &Element,
        checked: bool,
        background: bool,
        line: &str,
    ) -> Run {
        if background && !matches!(e.command, Command::Simple(_)) {
            let name = self.runs_in_shell(&e.command).unwrap_or_default();
            self.last_name = name.clone();
            return Ok(self.report(&shell_error(
                &name,
                "a builtin or function cannot run in the background",
            )));
        }
        let result = match &e.command {
            Command::Simple(s) => self.simple(src, s, background, line),
            Command::If {
                branches,
                otherwise,
            } => {
                for (condition, body) in branches {
                    let (c, _) = self.chain(src, condition, true, false, line)?;
                    self.state.last = c.record;
                    if c.success() {
                        return self.block(src, body);
                    }
                }
                match otherwise {
                    Some(b) => self.block(src, b),
                    None => Ok(Result_::code(0)),
                }
            }
            Command::For { name, values, body } => {
                let mut items = Vec::new();
                for w in values {
                    match self.expand(src, w, Context::EmptyOk) {
                        Ok(v) => items.extend(v),
                        Err(e) => {
                            self.last_name = "for".into();
                            return Ok(self.report_owned(e));
                        }
                    }
                }
                let mut r = Result_::code(0);
                self.state.loops += 1;
                for item in items {
                    if self.host.interrupted() {
                        self.state.loops -= 1;
                        return self.interrupted_stop(checked);
                    }
                    if let Err(problem) = self.state.set(name, alloc::vec![item], false) {
                        r = self.report(&shell_error(name, &problem));
                        break;
                    }
                    match self.block(src, body) {
                        Ok(x) => r = x,
                        Err(Flow::Break) => break,
                        Err(Flow::Continue) => continue,
                        Err(other) => {
                            self.state.loops -= 1;
                            return Err(other);
                        }
                    }
                }
                self.state.loops -= 1;
                Ok(r)
            }
            Command::While { condition, body } => {
                let mut r = Result_::code(0);
                self.state.loops += 1;
                loop {
                    if self.host.interrupted() {
                        self.state.loops -= 1;
                        return self.interrupted_stop(checked);
                    }
                    let (c, _) = match self.chain(src, condition, true, false, line) {
                        Ok(c) => c,
                        Err(f) => {
                            self.state.loops -= 1;
                            return Err(f);
                        }
                    };
                    if !c.success() {
                        break;
                    }
                    match self.block(src, body) {
                        Ok(x) => r = x,
                        Err(Flow::Break) => break,
                        Err(Flow::Continue) => continue,
                        Err(other) => {
                            self.state.loops -= 1;
                            return Err(other);
                        }
                    }
                }
                self.state.loops -= 1;
                Ok(r)
            }
            Command::Fn(f) => {
                if LANGUAGE.contains(&f.name.as_str())
                    || matches!(self.host.lookup(&f.name), Found::Builtin)
                {
                    self.last_name = "fn".into();
                    return Ok(self.report(&shell_error(
                        &f.name,
                        "is a builtin; a function cannot take its name",
                    )));
                }
                let origin = self
                    .state
                    .script
                    .clone()
                    .unwrap_or_else(|| "the prompt".into());
                self.state.origins.insert(f.name.clone(), origin);
                self.state.sources.insert(f.name.clone(), Rc::from(src));
                self.state
                    .functions
                    .insert(f.name.clone(), Rc::new((**f).clone()));
                Ok(Result_::code(0))
            }
            Command::Try { body, catch } => match self.block(src, body) {
                Err(Flow::Stop(f)) => {
                    self.state.last = f.record;
                    match catch {
                        Some(c) => self.block(src, c),
                        None => Ok(Result_::code(0)),
                    }
                }
                other => other,
            },
            Command::Break | Command::Continue if self.state.loops == 0 => {
                let word = if matches!(e.command, Command::Break) {
                    "break"
                } else {
                    "continue"
                };
                self.last_name = word.into();
                Ok(self.report(&shell_error(word, "not inside a loop")))
            }
            Command::Break => Err(Flow::Break),
            Command::Continue => Err(Flow::Continue),
            Command::Return(value) => {
                let code = match value {
                    None => 0,
                    Some(w) => {
                        let v = match self.expand(src, w, Context::Command) {
                            Ok(v) => v,
                            Err(e) => return Ok(self.report_owned(e)),
                        };
                        let text = v.first().map(|v| v.text.as_str()).unwrap_or("");
                        match zoraya_describe::parse_number(text) {
                            Some(n @ 0..=255) if v.len() == 1 => n as i32,
                            _ => {
                                self.last_name = "return".into();
                                let mut err = shell_error(
                                    "return",
                                    &format!("{text}: not a status from 0 to 255"),
                                );
                                err.code = 2;
                                return Ok(self.report(&err));
                            }
                        }
                    }
                };
                if self.state.frames.is_empty() && self.state.script.is_none() {
                    self.last_name = "return".into();
                    return Ok(self.report(&shell_error("return", "not inside a function")));
                }
                Err(Flow::Return(exited(code)))
            }
        };
        match result {
            Err(Flow::Stop(f)) if checked => Ok(Result_ {
                record: f.record,
                no: false,
                ctrl_c: f.ctrl_c,
                reported: f.reported,
            }),
            other => other,
        }
    }

    fn interrupted_stop(&mut self, checked: bool) -> Run {
        let r = Result_ {
            record: record::interrupted(),
            no: false,
            ctrl_c: true,
            reported: false,
        };
        self.last_name = "zosh".into();
        if checked {
            return Ok(r);
        }
        Err(Flow::Stop(Failure {
            record: r.record,
            name: "zosh".into(),
            ctrl_c: true,
            reported: false,
        }))
    }

    fn report_owned(&mut self, e: ShellError) -> Result_ {
        self.report(&e)
    }

    fn block(&mut self, src: &str, b: &Block) -> Run {
        let mut r = Result_::code(0);
        for s in &b.statements {
            r = self.statement(src, s)?;
        }
        Ok(r)
    }

    /// Expands one word to its arguments (ADR-0150 rules 2 to 8).
    fn expand(&mut self, src: &str, w: &Word, context: Context) -> Result<Vec<Value>, ShellError> {
        enum Piece {
            Text(String, bool),
            List(Vec<Value>, String),
        }
        let source = src.get(w.span.start..w.span.end).unwrap_or("").to_string();
        let mut pieces: Vec<Piece> = Vec::new();
        for (i, part) in w.parts.iter().enumerate() {
            match part {
                Part::Literal(t) => {
                    let home = i == 0 && (t == "~" || t.starts_with("~/"));
                    if home {
                        pieces.push(Piece::Text(self.host.home(), false));
                        pieces.push(Piece::Text(t[1..].into(), true));
                    } else {
                        pieces.push(Piece::Text(t.clone(), true));
                    }
                }
                Part::Quoted(t) => pieces.push(Piece::Text(t.clone(), false)),
                Part::Var {
                    name,
                    index,
                    quoted,
                } => {
                    let values = self.variable(name, *index)?;
                    if *quoted {
                        let joined: Vec<&str> = values.iter().map(|v| v.text.as_str()).collect();
                        pieces.push(Piece::Text(joined.join(" "), false));
                    } else {
                        pieces.push(Piece::List(values, format!("${name}")));
                    }
                }
                Part::Subst { body, quoted } => {
                    let text = self.substitute(src, body)?;
                    if *quoted {
                        let t = text.strip_suffix('\n').unwrap_or(&text);
                        pieces.push(Piece::Text(t.into(), false));
                    } else {
                        let lines: Vec<Value> = text.lines().map(Value::data).collect();
                        pieces.push(Piece::List(lines, "$(...)".into()));
                    }
                }
            }
        }
        // Rule 4: one list may have several elements; an empty one with
        // other text is an error.
        let lists: Vec<usize> = pieces
            .iter()
            .enumerate()
            .filter_map(|(i, p)| matches!(p, Piece::List(..)).then_some(i))
            .collect();
        let alone = pieces.len() == 1;
        let mut several = None;
        for &i in &lists {
            let Piece::List(values, name) = &pieces[i] else {
                continue;
            };
            if values.is_empty() {
                if alone {
                    return Ok(Vec::new());
                }
                return Err(shell_error(&source, &format!("{name} is empty")));
            }
            if values.len() > 1 {
                if several.is_some() {
                    return Err(shell_error(
                        &source,
                        "two lists of several values in one word",
                    ));
                }
                several = Some(i);
            }
        }
        let count = several.map_or(1, |i| match &pieces[i] {
            Piece::List(v, _) => v.len(),
            Piece::Text(..) => 1,
        });
        let mut out = Vec::new();
        let literal = w.is_literal_source();
        for n in 0..count {
            let mut chars: Vec<(char, bool)> = Vec::new();
            // A word that is one variable alone has its value's mark (a
            // parameter keeps the caller's, ADR-0153 rule 4); any other
            // word with a variable in it is data.
            let mut value_literal = literal || (pieces.len() == 1 && !lists.is_empty());
            for piece in &pieces {
                match piece {
                    Piece::Text(t, active) => chars.extend(t.chars().map(|c| (c, *active))),
                    Piece::List(values, _) => {
                        let v = if values.len() == 1 {
                            &values[0]
                        } else {
                            &values[n]
                        };
                        value_literal &= v.literal && pieces.len() == 1;
                        chars.extend(v.text.chars().map(|c| (c, false)));
                    }
                }
            }
            match glob::pattern(&chars) {
                // Rule 3: a grant is one path as written (review S8).
                Some(_) if context == Context::Grant => {
                    return Err(shell_error(
                        &source,
                        "a grant names one path: a pattern is not one",
                    ));
                }
                None => out.push(Value {
                    text: chars.iter().map(|(c, _)| c).collect(),
                    literal: value_literal,
                }),
                Some(p) => {
                    let found = glob::expand(&p, &mut HostLister(&mut self.host));
                    if found.is_empty() && context == Context::Command {
                        return Err(shell_error(&source, "no file matches"));
                    }
                    out.extend(found.into_iter().map(|t| Value { text: t, literal }));
                }
            }
            if out.len() > MAX_VALUES {
                return Err(shell_error(&source, "too many values"));
            }
        }
        Ok(out)
    }

    fn variable(&mut self, name: &str, index: Option<i64>) -> Result<Vec<Value>, ShellError> {
        let values: Vec<Value> = if name == "status" {
            alloc::vec![Value::data(&record::status_word(&self.state.last))]
        } else {
            match self.state.get(name) {
                Some(v) => v.to_vec(),
                None => {
                    let mut e = shell_error(&format!("${name}"), "not set");
                    e.hints.push(format!(
                        "set it first: set {name} VALUE, or ask: set --query {name}"
                    ));
                    return Err(e);
                }
            }
        };
        match index {
            None => Ok(values),
            Some(i) => {
                let len = values.len() as i64;
                let at = if i > 0 { i - 1 } else { len + i };
                if at < 0 || at >= len {
                    return Err(shell_error(
                        &format!("${name}[{i}]"),
                        &format!("out of range: it has {len}"),
                    ));
                }
                Ok(alloc::vec![values[at as usize].clone()])
            }
        }
    }

    /// `$(...)`: the output of the block, as text.
    fn substitute(&mut self, src: &str, body: &Block) -> Result<String, ShellError> {
        self.state.captures.push(String::new());
        let err = self.sink_err();
        self.state.sinks.push((Sink::Capture, err));
        let r = self.block(src, body);
        self.state.sinks.pop();
        let text = self.state.captures.pop().unwrap_or_default();
        match r {
            Ok(r) if !r.failed() => Ok(text),
            Err(Flow::Return(record)) if is_success(&record) => Ok(text),
            _ => Err(shell_error(
                "$(...)",
                "its command failed, so this one did not run",
            )),
        }
    }

    fn simple(&mut self, src: &str, s: &Simple, background: bool, line: &str) -> Run {
        // `x=1` alone.
        if s.words.is_empty() {
            let a = &s.assignments[0];
            let shown = src
                .get(a.value.span.start.saturating_sub(a.name.len() + 1)..a.value.span.end)
                .unwrap_or("");
            let value = src.get(a.value.span.start..a.value.span.end).unwrap_or("");
            self.last_name = "zosh".into();
            let mut e = shell_error(shown, "not a command");
            e.hints
                .push(format!("to set a variable, type: set {} {value}", a.name));
            return Ok(self.report(&e));
        }
        let args = match self.arguments(src, s) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };
        let name = args[0].text.clone();
        self.last_name = name.clone();
        let found = self.find(&name);
        if found != Found::Program && !s.grants.is_empty() {
            return Ok(self.report(&shell_error(&name, "only a program can be given grants")));
        }
        if background && found != Found::Program && found != Found::Nothing {
            return Ok(self.report(&shell_error(
                &name,
                "a builtin or function cannot run in the background",
            )));
        }
        match found {
            Found::Nothing => Ok(self.not_found(&name)),
            Found::Program => self.program(src, s, &args, background, line),
            Found::Function => {
                let f = self.state.function(&name).expect("found");
                self.with_redirects(src, s, |shell| shell.call(src, &f, &args[1..]))
            }
            Found::Language | Found::Builtin => {
                self.with_redirects(src, s, |shell| Ok(shell.builtin(&name, &args[1..], found)))
            }
        }
    }

    /// A simple command's words, expanded; at least the name.
    fn arguments(&mut self, src: &str, s: &Simple) -> Result<Vec<Value>, Result_> {
        let mut args: Vec<Value> = Vec::new();
        for (i, w) in s.words.iter().enumerate() {
            let context = if i > 0 && args.first().is_some_and(|a| a.text == "set") {
                Context::EmptyOk
            } else {
                Context::Command
            };
            match self.expand(src, w, context) {
                Ok(v) => args.extend(v),
                Err(e) => {
                    self.last_name = "zosh".into();
                    return Err(self.report(&e));
                }
            }
        }
        if args.is_empty() {
            self.last_name = "zosh".into();
            let e = shell_error(
                src.get(s.words[0].span.start..s.words[0].span.end)
                    .unwrap_or(""),
                "gives no command name",
            );
            return Err(self.report(&e));
        }
        Ok(args)
    }

    /// `grants LINE` (ADR-0153 rule 7, review S6): the line's words with
    /// their marks, `+r` and `+w` only as written, so that the host plans
    /// exactly what running the line would give.
    fn grants(&mut self, state: &mut State, args: &[Value], io: &mut Io) -> i32 {
        let mut rest = args;
        let mut given: Vec<(bool, Value)> = Vec::new();
        while let [g, path, after @ ..] = rest {
            if !(g.literal && (g.text == "+r" || g.text == "+w")) {
                break;
            }
            given.push((g.text == "+w", path.clone()));
            rest = after;
        }
        let Some((first, args)) = rest.split_first() else {
            io.err.push_str(
                "grants: missing operand LINE\nusage: grants LINE...\nmore: help grants\n",
            );
            return 2;
        };
        let exec = Exec {
            name: &first.text,
            args,
            env: &[],
            grants: &given,
            redirects: &[],
            err_too: false,
        };
        self.host.grants(state, &exec, io)
    }

    /// Rule 4 for a redirection (review S5): a target from a value reaches
    /// only inside the current directory or a grant of the line, as an
    /// operand from a value does; one written out goes where it says.
    fn redirect_target(&mut self, v: &Value, grants: &[(bool, Value)]) -> Result<(), ShellError> {
        if v.literal {
            return Ok(());
        }
        let cwd = self.host.cwd();
        let abs = zoraya_resolve::join(&cwd, 0, &v.text);
        let inside = |base: &str| zoraya_resolve::covers(base, &abs);
        if inside(&cwd)
            || grants
                .iter()
                .any(|(_, g)| inside(&zoraya_resolve::join(&cwd, 0, &g.text)))
        {
            return Ok(());
        }
        let mut e = shell_error(&v.text, "from a value, outside the current directory");
        e.hints
            .push("if that is meant, write the path out on the line".into());
        Err(e)
    }

    /// `zosh: NAME: no such command`, with a hint when one is near.
    fn not_found(&mut self, name: &str) -> Result_ {
        let mut e = shell_error(name, "no such command");
        if let Some((_, hint)) = UNIX_HINTS.iter().find(|(n, _)| *n == name) {
            e.hints.push((*hint).into());
        } else if name.contains('=') && name.split('=').next().is_some_and(syntax::is_name) {
            let (n, v) = name.split_once('=').unwrap_or_default();
            e.hints
                .push(format!("to set a variable, type: set {n} {v}"));
        } else {
            let mut names = self.host.names();
            names.extend(self.state.function_names());
            names.extend(LANGUAGE.iter().map(|n| n.to_string()));
            let near = nearest(name, &names);
            if !near.is_empty() {
                e.hints.push(format!("did you mean {}?", near.join(" or ")));
            }
        }
        self.report(&e)
    }

    /// Runs `f` with the statement's redirections as its output.
    fn with_redirects(
        &mut self,
        src: &str,
        s: &Simple,
        f: impl FnOnce(&mut Shell<H>) -> Run,
    ) -> Run {
        if s.redirects.is_empty() {
            return f(self);
        }
        let (mut out, mut err) = (self.sink_out(), self.sink_err());
        let mut opened = Vec::new();
        for r in &s.redirects {
            let target = match self.expand(src, &r.target, Context::Command) {
                Ok(v) if v.len() == 1 => match self.redirect_target(&v[0], &[]) {
                    Ok(()) => v[0].text.clone(),
                    Err(e) => {
                        for t in opened {
                            self.host.close_output(t);
                        }
                        return Ok(self.report(&e));
                    }
                },
                Ok(_) => {
                    for t in opened {
                        self.host.close_output(t);
                    }
                    return Ok(self.report(&shell_error(
                        src.get(r.target.span.start..r.target.span.end)
                            .unwrap_or(""),
                        "a redirection needs exactly one file",
                    )));
                }
                Err(e) => {
                    for t in opened {
                        self.host.close_output(t);
                    }
                    return Ok(self.report(&e));
                }
            };
            if r.stream == Stream::In {
                for t in opened {
                    self.host.close_output(t);
                }
                return Ok(self.report(&shell_error("<", "builtins and functions read no input")));
            }
            match self.host.open_output(&target, r.append) {
                Ok(token) => {
                    opened.push(token);
                    match r.stream {
                        Stream::Out => out = Sink::File(token),
                        Stream::Err => err = Sink::File(token),
                        Stream::Both => {
                            out = Sink::File(token);
                            err = Sink::File(token);
                        }
                        Stream::In => {}
                    }
                }
                Err(problem) => {
                    for t in opened {
                        self.host.close_output(t);
                    }
                    let line = zoraya_command::error("zosh", Some(&target), &problem, &[]);
                    self.err(&line);
                    return Ok(Result_::code(1));
                }
            }
        }
        self.state.sinks.push((out, err));
        let r = f(self);
        self.state.sinks.pop();
        for t in opened {
            self.host.close_output(t);
        }
        r
    }

    /// A program's environment, grants and redirections, expanded.
    fn prepare(&mut self, src: &str, s: &Simple, args: &[Value]) -> Result<Prepared, Result_> {
        let mut own = Vec::new();
        for a in &s.assignments {
            let v = match self.expand(src, &a.value, Context::Command) {
                Ok(v) => v,
                Err(e) => return Err(self.report(&e)),
            };
            if v.len() != 1 {
                return Err(self.report(&shell_error(
                    &a.name,
                    "an environment value is exactly one value",
                )));
            }
            own.push((a.name.clone(), v[0].text.clone()));
        }
        let mut grants = Vec::new();
        for g in &s.grants {
            let v = match self.expand(src, &g.path, Context::Grant) {
                Ok(v) => v,
                Err(e) => return Err(self.report(&e)),
            };
            if v.len() != 1 {
                let marker = if g.write { "+w" } else { "+r" };
                return Err(self.report(&shell_error(marker, "needs exactly one path")));
            }
            grants.push((g.write, v[0].clone()));
        }
        let mut redirects = Vec::new();
        for r in &s.redirects {
            match self.expand(src, &r.target, Context::Command) {
                Ok(v) if v.len() == 1 => {
                    if let Err(e) = self.redirect_target(&v[0], &grants) {
                        return Err(self.report(&e));
                    }
                    redirects.push((r.stream, r.append, v[0].text.clone()));
                }
                Ok(_) => {
                    return Err(self.report(&shell_error(
                        src.get(r.target.span.start..r.target.span.end)
                            .unwrap_or(""),
                        "a redirection needs exactly one file",
                    )));
                }
                Err(e) => return Err(self.report(&e)),
            }
        }
        let env = self.state.environment(&own);
        Ok(Prepared {
            name: args[0].text.clone(),
            args: args[1..].to_vec(),
            env,
            grants,
            redirects,
            err_too: false,
        })
    }

    fn program(
        &mut self,
        src: &str,
        s: &Simple,
        args: &[Value],
        background: bool,
        line: &str,
    ) -> Run {
        let prepared = match self.prepare(src, s, args) {
            Ok(p) => p,
            Err(r) => return Ok(r),
        };
        let exec = [prepared.exec()];
        let pipe = Pipe {
            programs: &exec,
            input: None,
            out: self.sink_out(),
            err: self.sink_err(),
            background,
            line,
        };
        match self.host.program(&pipe) {
            Ran::Ended {
                records,
                ctrl_c,
                captured,
            } => {
                self.emit(Sink::Capture, &captured);
                let record = records.first().copied().unwrap_or_else(|| exited(0));
                let question = self
                    .host
                    .description(&prepared.name)
                    .is_some_and(|d| d.question);
                Ok(Result_ {
                    record,
                    no: question && is_success_no(&record),
                    ctrl_c,
                    reported: false,
                })
            }
            Ran::Background(_) => Ok(Result_::code(0)),
            Ran::Stopped(_) => Ok(stopped()),
            Ran::Refused(code) => Ok(Result_::code(code)),
        }
    }

    fn call(&mut self, src: &str, f: &Function, args: &[Value]) -> Run {
        let _ = src;
        if args.len() < f.params.len() {
            let d = function_description(f);
            let e = zoraya_command::UsageError(format!(
                "missing operand {}",
                f.params[args.len()].to_uppercase()
            ));
            let text = zoraya_command::usage_error(&d, &e);
            self.err(&text);
            return Ok(Result_::code(2));
        }
        if self.state.calls >= MAX_CALLS {
            return Ok(self.report(&shell_error(
                &f.name,
                "functions call each other too deeply",
            )));
        }
        let mut frame = BTreeMap::new();
        for (p, a) in f.params.iter().zip(args) {
            frame.insert(
                p.clone(),
                Var {
                    values: alloc::vec![a.clone()],
                    exported: false,
                },
            );
        }
        frame.insert(
            "args".into(),
            Var {
                values: args.to_vec(),
                exported: false,
            },
        );
        self.state.calls += 1;
        self.state.frames.push(frame);
        let saved_loops = core::mem::take(&mut self.state.loops);
        // The body's own source is the function's text, kept in its spans:
        // a function defined at the prompt keeps the prompt's line.
        let body_src = self
            .function_source(&f.name)
            .unwrap_or_else(|| Rc::from(src));
        let r = self.block(&body_src, &f.body);
        self.state.loops = saved_loops;
        self.state.frames.pop();
        self.state.calls -= 1;
        match r {
            Ok(r) => Ok(r),
            Err(Flow::Return(record)) => Ok(Result_::of(record)),
            Err(Flow::Break | Flow::Continue) => {
                Ok(self.report(&shell_error(&f.name, "break or continue outside a loop")))
            }
            Err(stop) => Err(stop),
        }
    }

    fn function_source(&self, name: &str) -> Option<Rc<str>> {
        self.state.sources.get(name).cloned()
    }

    /// The description of a builtin, the language's or the host's.
    pub fn builtin_description(&mut self, name: &str) -> Option<Rc<Description>> {
        if LANGUAGE.contains(&name) {
            return language::description(name).map(Rc::new);
        }
        self.host.description(name)
    }

    fn builtin(&mut self, name: &str, args: &[Value], found: Found) -> Result_ {
        let Some(d) = self.builtin_description(name) else {
            return self.report(&shell_error(name, "has no description"));
        };
        let mut texts: Vec<&str> = args.iter().map(|a| a.text.as_str()).collect();
        // `grants LINE`: the line is the command's, options and all
        // (`grants tcc main.c -o main`, ADR-0153 rule 7).
        if name == "grants" && texts.first().is_some_and(|t| !t.starts_with('-')) {
            texts.insert(0, "--");
        }
        // `settings NAME -ITEM` (ADR-0171): after a setting's name every
        // word is a value, `-ua` too; `--at N` stays an option.
        if name == "settings"
            && texts.first().is_some_and(|t| !t.starts_with('-'))
            && texts.len() > 1
        {
            let mut at = Vec::new();
            if let Some(i) = texts.iter().position(|t| *t == "--at")
                && i + 1 < texts.len()
            {
                at = texts.drain(i..i + 2).collect();
            }
            if texts.len() > 1 && texts[1] != "--" {
                texts.insert(1, "--");
            }
            for (k, t) in at.into_iter().enumerate() {
                texts.insert(k, t);
            }
        }
        let parsed = match zoraya_command::parse(&d, &texts) {
            Ok(Outcome::Help) => {
                let help = zoraya_command::help(&d);
                self.out(&help);
                return Result_::code(0);
            }
            Ok(Outcome::Run(p)) => p,
            Err(e) => {
                let text = zoraya_command::usage_error(&d, &e);
                self.err(&text);
                return Result_::code(2);
            }
        };
        let code = if found == Found::Language {
            let code = self.language(name, &d, &parsed, args);
            if let Some(r) = self.sourced.take() {
                return r;
            }
            code
        } else {
            let mut io = Io {
                terminal: self.sink_out() == Sink::Terminal,
                ..Io::default()
            };
            let mut state = core::mem::take(&mut self.state);
            let code = if name == "grants" {
                self.grants(&mut state, args, &mut io)
            } else {
                self.host.builtin(&mut state, name, &d, &parsed, &mut io)
            };
            self.state = state;
            self.out(&io.out);
            self.err(&io.err);
            code
        };
        Result_ {
            record: exited(code),
            no: d.question && code == 1,
            ctrl_c: false,
            reported: false,
        }
    }
}

/// A program's expanded words and settings, owned while its pipeline runs.
struct Prepared {
    name: String,
    args: Vec<Value>,
    env: Vec<String>,
    grants: Vec<(bool, Value)>,
    redirects: Vec<(Stream, bool, String)>,
    err_too: bool,
}

impl Prepared {
    fn exec(&self) -> Exec<'_> {
        Exec {
            name: &self.name,
            args: &self.args,
            env: &self.env,
            grants: &self.grants,
            redirects: &self.redirects,
            err_too: self.err_too,
        }
    }
}

fn only_first(name: &str) -> ShellError {
    let mut err = shell_error(name, "a builtin or function can only start a pipeline");
    err.hints.push(format!("run {name} on its own line"));
    err
}

/// A line whose job in front was stopped stops there, as at Ctrl-C: the
/// rest of it does not run, and `$status` is an interrupted command's. The
/// host has already said `[N] stopped: LINE`.
fn stopped() -> Result_ {
    Result_ {
        record: record::interrupted(),
        no: false,
        ctrl_c: false,
        reported: true,
    }
}

/// One element of a pipeline as it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Member {
    pub record: ExitRecord,
    /// A question command (ADR-0151 rule 3): exit 1 is its "no".
    pub question: bool,
}

/// `SIGPIPE`'s number, in a POSIX program's `Terminated` record (ADR-0101).
pub const SIGPIPE: u32 = 13;

/// A pipeline's status (ADR-0152 rule 6, INV-0152-3). An element fails
/// unless it succeeded, answered "no", or (not being the last) ended by
/// `SIGPIPE` because its reader closed. With a failure the status is the
/// rightmost failure's; else "no" when the last element answered "no";
/// else success.
pub fn pipeline_status(members: &[Member]) -> Result_ {
    if let Some(i) = pipeline_failure(members) {
        return Result_::of(members[i].record);
    }
    match members.last() {
        Some(m) if answered_no(m) => Result_ {
            record: m.record,
            no: true,
            ctrl_c: false,
            reported: false,
        },
        _ => Result_::code(0),
    }
}

/// The rightmost element that failed, by rule 6: the one whose record is
/// the pipeline's status and whose name its report gives.
pub fn pipeline_failure(members: &[Member]) -> Option<usize> {
    let last = members.len().saturating_sub(1);
    members.iter().enumerate().rposition(|(i, m)| {
        let sigpipe = m.record.reason == zoraya_abi::ExitReason::Terminated
            && m.record.detail == SIGPIPE
            && i != last;
        !is_success(&m.record) && !answered_no(m) && !sigpipe
    })
}

fn answered_no(m: &Member) -> bool {
    m.question && is_success_no(&m.record)
}

/// A question's "no": exit code 1.
fn is_success_no(r: &ExitRecord) -> bool {
    r.reason == zoraya_abi::ExitReason::Exited && r.code == 1
}
