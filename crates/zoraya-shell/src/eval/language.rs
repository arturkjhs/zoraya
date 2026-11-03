//! The language's own builtins (ADR-0150, ADR-0151, ADR-0155 rule 8): the
//! ones that only touch the shell's variables, or read files as the
//! language says (`source`, `count --lines`).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use zoraya_command::Parsed;
use zoraya_describe::Description;

use super::{Host, Result_, Shell, Value, shell_error};
use crate::record;
use crate::syntax;

/// The descriptions of the language's builtins: `(name, text)`.
pub const DESCRIPTIONS: [(&str, &str); 9] = [
    ("count", include_str!("../../describe/count.cmd")),
    ("echo", include_str!("../../describe/echo.cmd")),
    ("export", include_str!("../../describe/export.cmd")),
    ("math", include_str!("../../describe/math.cmd")),
    ("set", include_str!("../../describe/set.cmd")),
    ("source", include_str!("../../describe/source.cmd")),
    ("status", include_str!("../../describe/status.cmd")),
    ("test", include_str!("../../describe/test.cmd")),
    ("unset", include_str!("../../describe/unset.cmd")),
];

/// The parsed description of language builtin `name`.
pub fn description(name: &str) -> Option<Description> {
    let (_, text) = DESCRIPTIONS.iter().find(|(n, _)| *n == name)?;
    zoraya_describe::parse(text.as_bytes()).ok()
}

impl<H: Host> Shell<H> {
    fn fail(&mut self, subject: &str, problem: &str) -> i32 {
        let line = zoraya_command::error("zosh", Some(subject), problem, &[]);
        self.err(&line);
        1
    }

    /// Runs language builtin `name`; gives its exit code.
    pub(super) fn language(
        &mut self,
        name: &str,
        d: &Description,
        p: &Parsed<'_>,
        args: &[Value],
    ) -> i32 {
        // Operands as values, keeping their marks: each parsed operand is
        // a slice of one argument's text.
        let operands: Vec<Value> = p
            .operands
            .iter()
            .map(|(_, t)| {
                args.iter()
                    .find(|a| core::ptr::eq(a.text.as_str(), *t))
                    .cloned()
                    .unwrap_or_else(|| Value::data(t))
            })
            .collect();
        match name {
            "echo" => {
                let words: Vec<&str> = operands.iter().map(|v| v.text.as_str()).collect();
                let line = format!("{}\n", words.join(" "));
                self.out(&line);
                0
            }
            "set" => self.set_builtin(d, p, operands),
            "unset" => {
                for v in &operands {
                    self.state.unset(&v.text);
                }
                0
            }
            "export" => self.export_builtin(operands),
            "count" => {
                if p.flag(d, "lines") {
                    let mut total = 0usize;
                    for v in &operands {
                        match self.host.read(&v.text) {
                            Ok(text) => total += text.lines().count(),
                            Err(problem) => {
                                let line =
                                    zoraya_command::error("count", Some(&v.text), &problem, &[]);
                                self.err(&line);
                                return 1;
                            }
                        }
                    }
                    self.out(&format!("{total}\n"));
                } else {
                    self.out(&format!("{}\n", operands.len()));
                }
                0
            }
            "test" => self.test_builtin(d, p, &operands),
            "math" => {
                let text: Vec<&str> = operands.iter().map(|v| v.text.as_str()).collect();
                match math(&text.join(" ")) {
                    Ok(n) => {
                        self.out(&format!("{n}\n"));
                        0
                    }
                    Err(problem) => {
                        let line = zoraya_command::error("math", None, problem, &[]);
                        self.err(&line);
                        1
                    }
                }
            }
            "status" => {
                let r = self.state.last;
                let text = if p.flag(d, "code") {
                    r.code.to_string()
                } else if p.flag(d, "reason") {
                    record::reason_word(&r).into()
                } else if p.flag(d, "detail") {
                    r.detail.to_string()
                } else {
                    record::describe(&r)
                };
                self.out(&format!("{text}\n"));
                0
            }
            "source" => {
                let Some((file, rest)) = operands.split_first() else {
                    return 2;
                };
                let text = match self.host.read(&file.text) {
                    Ok(t) => t,
                    Err(problem) => {
                        let line = zoraya_command::error("source", Some(&file.text), &problem, &[]);
                        self.err(&line);
                        return 1;
                    }
                };
                let r: Result_ = self.run_script(&file.text, &text, rest.to_vec());
                self.sourced = Some(r);
                if r.failed() { 1 } else { 0 }
            }
            _ => 2,
        }
    }

    fn set_builtin(&mut self, d: &Description, p: &Parsed<'_>, operands: Vec<Value>) -> i32 {
        let global = p.flag(d, "global");
        let Some((name, values)) = operands.split_first() else {
            // `set` alone: every variable.
            let mut text = String::new();
            for n in self.state.variable_names() {
                let values: Vec<String> = if n == "status" {
                    alloc::vec![record::status_word(&self.state.last)]
                } else {
                    self.state
                        .get(&n)
                        .unwrap_or(&[])
                        .iter()
                        .map(|v| zoraya_describe::quote_value(&v.text))
                        .collect()
                };
                text.push_str(&n);
                for v in values {
                    text.push(' ');
                    text.push_str(&v);
                }
                text.push('\n');
            }
            self.out(&text);
            return 0;
        };
        if p.flag(d, "query") {
            if !values.is_empty() {
                return self.fail("set", "--query takes only a name");
            }
            let set = self
                .state
                .get(&name.text)
                .is_some_and(|v| v.iter().any(|v| !v.text.is_empty()));
            return i32::from(!set);
        }
        if !syntax::is_name(&name.text) {
            return self.fail(&name.text, "not a variable name: letters, digits and _");
        }
        if values.len() > super::MAX_VALUES {
            return self.fail(&name.text, "too many values");
        }
        match self.state.set(&name.text, values.to_vec(), global) {
            Ok(()) => 0,
            Err(problem) => self.fail(&name.text, &problem),
        }
    }

    fn export_builtin(&mut self, operands: Vec<Value>) -> i32 {
        let Some((name, value)) = operands.split_first() else {
            return 2;
        };
        if let Some((n, v)) = name.text.split_once('=')
            && syntax::is_name(n)
        {
            let mut e = shell_error(&name.text, "not a variable name");
            e.hints.push(format!("to export it, type: export {n} {v}"));
            self.report(&e);
            return 1;
        }
        if !syntax::is_name(&name.text) {
            return self.fail(&name.text, "not a variable name: letters, digits and _");
        }
        let values = match value.first() {
            Some(v) => alloc::vec![v.clone()],
            None => match self.state.get(&name.text) {
                Some(v) => v.to_vec(),
                None => return self.fail(&name.text, "not set"),
            },
        };
        if values.len() != 1 {
            let problem = format!(
                "holds {} values; an exported variable holds one",
                values.len()
            );
            return self.fail(&name.text, &problem);
        }
        let var = self.state.globals.entry(name.text.clone()).or_default();
        var.values = values;
        var.exported = true;
        0
    }

    fn test_builtin(&mut self, d: &Description, p: &Parsed<'_>, operands: &[Value]) -> i32 {
        let flags = ["directory", "file", "exists"];
        let given: Vec<&str> = flags.iter().copied().filter(|f| p.flag(d, f)).collect();
        let usage = |shell: &mut Shell<H>, problem: &str| -> i32 {
            let e = zoraya_command::UsageError(problem.into());
            let text = zoraya_command::usage_error(d, &e);
            shell.err(&text);
            2
        };
        match (given.as_slice(), operands) {
            ([flag], [path]) => {
                let kind = self.host.kind(&path.text);
                let yes = match *flag {
                    "directory" => kind == Some(true),
                    "file" => kind == Some(false),
                    _ => kind.is_some(),
                };
                i32::from(!yes)
            }
            ([_], _) => usage(self, "one PATH is needed"),
            ([], [a, op, b]) if op.text == "=" => i32::from(a.text != b.text),
            ([], [a, op, b]) if op.text == "!=" => i32::from(a.text == b.text),
            ([], _) => usage(self, "not a check test knows"),
            _ => usage(self, "one check at a time"),
        }
    }
}

/// Evaluates whole-number arithmetic: `+ - x * / %`, unary `-`, parentheses.
pub fn math(text: &str) -> Result<i64, &'static str> {
    #[derive(Clone, Copy, PartialEq)]
    enum T {
        N(i64),
        Op(char),
    }
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_whitespace() {
            chars.next();
        } else if c.is_ascii_digit() {
            let mut n: i64 = 0;
            while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                n = n
                    .checked_mul(10)
                    .and_then(|n| n.checked_add(i64::from(d as u8 - b'0')))
                    .ok_or("the number is too large")?;
                chars.next();
            }
            tokens.push(T::N(n));
        } else if "+-*x/%()".contains(c) {
            tokens.push(T::Op(if c == 'x' { '*' } else { c }));
            chars.next();
        } else {
            return Err("not a valid expression");
        }
        if tokens.len() > 4096 {
            return Err("the expression is too long");
        }
    }
    struct P {
        t: Vec<T>,
        at: usize,
        depth: usize,
    }
    impl P {
        fn peek(&self) -> Option<T> {
            self.t.get(self.at).copied()
        }
        fn sum(&mut self) -> Result<i64, &'static str> {
            let mut v = self.product()?;
            while let Some(T::Op(op @ ('+' | '-'))) = self.peek() {
                self.at += 1;
                let r = self.product()?;
                v = if op == '+' {
                    v.checked_add(r)
                } else {
                    v.checked_sub(r)
                }
                .ok_or("the result is too large")?;
            }
            Ok(v)
        }
        fn product(&mut self) -> Result<i64, &'static str> {
            let mut v = self.unary()?;
            while let Some(T::Op(op @ ('*' | '/' | '%'))) = self.peek() {
                self.at += 1;
                let r = self.unary()?;
                v = match op {
                    '*' => v.checked_mul(r).ok_or("the result is too large")?,
                    _ if r == 0 => return Err("division by zero"),
                    '/' => v.checked_div(r).ok_or("the result is too large")?,
                    _ => v.checked_rem(r).ok_or("the result is too large")?,
                };
            }
            Ok(v)
        }
        fn unary(&mut self) -> Result<i64, &'static str> {
            self.depth += 1;
            if self.depth > 256 {
                return Err("the expression is nested too deeply");
            }
            let v = match self.peek() {
                Some(T::Op('-')) => {
                    self.at += 1;
                    self.unary()?
                        .checked_neg()
                        .ok_or("the result is too large")?
                }
                Some(T::Op('(')) => {
                    self.at += 1;
                    let v = self.sum()?;
                    if self.peek() != Some(T::Op(')')) {
                        return Err("a ( is not closed");
                    }
                    self.at += 1;
                    v
                }
                Some(T::N(n)) => {
                    self.at += 1;
                    n
                }
                _ => return Err("not a valid expression"),
            };
            self.depth -= 1;
            Ok(v)
        }
    }
    let mut p = P {
        t: tokens,
        at: 0,
        depth: 0,
    };
    let v = p.sum()?;
    if p.at != p.t.len() {
        return Err("not a valid expression");
    }
    Ok(v)
}
