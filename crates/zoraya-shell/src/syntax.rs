//! The language's syntax (ADR-0150, ADR-0151, ADR-0152): source text to a
//! tree. The parser is total and bounded (INV-0150-3): any text gives a tree
//! or an [`Error`] with a position, never a panic; nesting is limited to
//! [`MAX_DEPTH`] and the tree is never larger than a fixed multiple of the
//! input.
//!
//! A line is statements separated by `;` or a newline. A statement is a chain
//! of pipelines joined by `&&` and `||`, optionally ending in `&`. A pipeline
//! is elements joined by `|` or `&|`, optionally after `!`. An element is a
//! block construct (`if`, `for`, `while`, `fn`, `try`, `break`, `continue`,
//! `return`) or a simple command: grants (`+r PATH`, `+w PATH`) and
//! assignments (`NAME=value`) before the command's words, with redirections
//! among them.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Deepest nesting of blocks and substitutions.
pub const MAX_DEPTH: usize = 64;
/// Largest source the parser takes, in bytes.
pub const MAX_SOURCE: usize = 1 << 20;

/// Bytes `start..end` of the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// Unquoted text: `*`, `?` and a leading `~` are active in it.
    Literal(String),
    /// Quoted or escaped text: nothing in it is active.
    Quoted(String),
    /// `$name` or `$name[i]`; `quoted` inside `"..."`.
    Var {
        name: String,
        index: Option<i64>,
        quoted: bool,
    },
    /// `$(...)`; `quoted` inside `"..."`.
    Subst { body: Block, quoted: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub parts: Vec<Part>,
    pub span: Span,
}

impl Word {
    /// The word's text if it is plain unquoted text alone (a keyword, a grant
    /// marker, an assignment).
    pub fn literal(&self) -> Option<&str> {
        match self.parts.as_slice() {
            [Part::Literal(s)] => Some(s),
            _ => None,
        }
    }

    /// True if no part of it comes from a value (ADR-0153 rule 4).
    pub fn is_literal_source(&self) -> bool {
        self.parts
            .iter()
            .all(|p| matches!(p, Part::Literal(_) | Part::Quoted(_)))
    }
}

/// Which stream a redirection is for (ADR-0152 rule 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    In,
    Out,
    Err,
    /// `&>`: out and err.
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    pub stream: Stream,
    pub append: bool,
    pub target: Word,
    pub span: Span,
}

/// `+r PATH` or `+w PATH` (ADR-0153 rule 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub write: bool,
    pub path: Word,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub name: String,
    pub value: Word,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Simple {
    pub grants: Vec<Grant>,
    pub assignments: Vec<Assignment>,
    pub words: Vec<Word>,
    pub redirects: Vec<Redirect>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Simple(Simple),
    If {
        branches: Vec<(Chain, Block)>,
        otherwise: Option<Block>,
    },
    For {
        name: String,
        values: Vec<Word>,
        body: Block,
    },
    While {
        condition: Chain,
        body: Block,
    },
    Fn(Box<Function>),
    Try {
        body: Block,
        catch: Option<Block>,
    },
    Break,
    Continue,
    Return(Option<Word>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub params: Vec<String>,
    pub body: Block,
    /// The comment lines directly above `fn`, without `#` (ADR-0156 item 3).
    pub doc: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub command: Command,
    /// `&|` after it: its err goes into the pipe too.
    pub err_too: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pipeline {
    pub negated: bool,
    pub elements: Vec<Element>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    pub first: Pipeline,
    pub rest: Vec<(Join, Pipeline)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub chain: Chain,
    pub background: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Block {
    pub statements: Vec<Statement>,
}

/// Why the parser stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The text ends inside a quote, a block, a substitution or after an
    /// operator: more lines would complete it (`...> `).
    Incomplete,
    /// A mistake at byte `at`.
    Invalid {
        at: usize,
        problem: String,
        hint: Option<String>,
    },
}

fn invalid<T>(at: usize, problem: &str) -> Result<T, Error> {
    Err(Error::Invalid {
        at,
        problem: problem.into(),
        hint: None,
    })
}

const KEYWORDS: [&str; 11] = [
    "if", "else", "for", "in", "while", "fn", "try", "catch", "break", "continue", "return",
];

pub fn is_name(s: &str) -> bool {
    let mut cs = s.chars();
    cs.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && s.len() <= 64
}

/// Splits `NAME=value` into the name and the value's start.
fn assignment(word: &Word) -> Option<(String, usize)> {
    let Some(Part::Literal(first)) = word.parts.first() else {
        return None;
    };
    let (name, _) = first.split_once('=')?;
    is_name(name).then(|| (name.into(), name.len() + 1))
}

/// Characters that end a word.
fn is_break(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | ';' | '&' | '|' | '<' | '>')
}

struct Parser<'a> {
    src: &'a str,
    at: usize,
    depth: usize,
    /// Open `$(`s around the current position.
    parens: usize,
    /// Comment lines just read, for a following `fn`.
    doc: Vec<String>,
}

/// Parses a whole source: a line typed at the prompt, or a script.
pub fn parse(src: &str) -> Result<Block, Error> {
    if src.len() > MAX_SOURCE {
        return invalid(0, "the text is too long");
    }
    let mut p = Parser {
        src,
        at: 0,
        depth: 0,
        parens: 0,
        doc: Vec::new(),
    };
    let block = p.statements(None)?;
    Ok(block)
}

/// Where a list of statements ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    Brace,
    Paren,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.src[self.at..].chars().next()
    }

    fn peek2(&self) -> Option<char> {
        let mut cs = self.src[self.at..].chars();
        cs.next();
        cs.next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += c.len_utf8();
        Some(c)
    }

    fn rest(&self) -> &'a str {
        &self.src[self.at..]
    }

    /// Spaces, tabs and escaped newlines.
    fn blanks(&mut self) {
        loop {
            match self.peek() {
                Some(' ' | '\t') => {
                    self.at += 1;
                }
                Some('\\') if self.peek2() == Some('\n') => {
                    self.at += 2;
                }
                _ => return,
            }
        }
    }

    /// Blanks, newlines and comments; comments are kept for `fn`.
    fn blank_lines(&mut self) {
        loop {
            self.blanks();
            match self.peek() {
                Some('\n') => {
                    self.at += 1;
                    if self.rest().starts_with('\n') {
                        self.doc.clear();
                    }
                }
                Some('#') => self.comment(),
                _ => return,
            }
        }
    }

    /// A comment to the end of the line. One alone on its line is kept as
    /// help for a following `fn`.
    fn comment(&mut self) {
        let line_end = self
            .rest()
            .find('\n')
            .map_or(self.src.len(), |n| self.at + n);
        let line_start = self.src[..self.at].rfind('\n').map_or(0, |n| n + 1);
        let alone = self.src[line_start..self.at].trim().is_empty();
        let text = self.src[self.at + 1..line_end].trim();
        if !alone {
            self.doc.clear();
        } else if self.doc.len() < 32 {
            self.doc.push(text.into());
        }
        self.at = line_end;
    }

    /// The next word, if a word comes next, without consuming it.
    fn peek_literal(&self) -> Option<&'a str> {
        let rest = self.rest();
        let end = rest.find(|c: char| is_break(c)).unwrap_or(rest.len());
        let w = &rest[..end];
        let plain = !w.is_empty() && !w.contains(['\'', '"', '\\', '$', '#']);
        plain.then_some(w)
    }

    fn at_word_end(&self, len: usize) -> bool {
        self.src[self.at + len..]
            .chars()
            .next()
            .is_none_or(is_break)
    }

    /// A `{` or `}` standing as a word.
    fn at_brace(&self, brace: char) -> bool {
        self.peek() == Some(brace) && self.at_word_end(1)
    }

    fn enter(&mut self) -> Result<(), Error> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return invalid(self.at, "nested too deeply");
        }
        Ok(())
    }

    fn statements(&mut self, end: Option<End>) -> Result<Block, Error> {
        self.enter()?;
        let mut block = Block::default();
        loop {
            self.blank_lines();
            match self.peek() {
                None => {
                    if end.is_some() {
                        return Err(Error::Incomplete);
                    }
                    break;
                }
                Some(';') => {
                    return invalid(self.at, "a ; with no command before it");
                }
                Some(')') if end == Some(End::Paren) => {
                    self.at += 1;
                    break;
                }
                Some('}') if end == Some(End::Brace) && self.at_brace('}') => {
                    self.at += 1;
                    break;
                }
                _ => {}
            }
            if self.at_brace('}') {
                return invalid(self.at, "a } with no { before it");
            }
            let start = self.at;
            let chain = self.chain(end)?;
            self.blanks();
            let mut background = false;
            if self.peek() == Some('&') {
                self.at += 1;
                background = true;
                self.blanks();
            }
            block.statements.push(Statement {
                chain,
                background,
                span: Span {
                    start,
                    end: self.at,
                },
            });
            match self.peek() {
                None | Some('\n') => {}
                Some(';') => self.at += 1,
                Some(')') if end == Some(End::Paren) => {}
                Some('}') if end == Some(End::Brace) && self.at_brace('}') => {}
                Some('#') => {}
                Some(_) if background => {}
                Some(_) => return invalid(self.at, "unexpected text"),
            }
        }
        self.depth -= 1;
        Ok(block)
    }

    fn chain(&mut self, end: Option<End>) -> Result<Chain, Error> {
        let first = self.pipeline(end)?;
        let mut rest = Vec::new();
        loop {
            self.blanks();
            let join = if self.rest().starts_with("&&") {
                Join::And
            } else if self.rest().starts_with("||") {
                Join::Or
            } else {
                break;
            };
            self.at += 2;
            self.blank_lines();
            if self.peek().is_none() {
                return Err(Error::Incomplete);
            }
            rest.push((join, self.pipeline(end)?));
        }
        Ok(Chain { first, rest })
    }

    fn pipeline(&mut self, end: Option<End>) -> Result<Pipeline, Error> {
        self.blanks();
        let start = self.at;
        let mut negated = false;
        if self.peek() == Some('!') && self.at_word_end(1) {
            self.at += 1;
            negated = true;
            self.blanks();
        }
        let mut elements = Vec::new();
        loop {
            let el_start = self.at;
            let command = self.element(end)?;
            self.blanks();
            let err_too = self.rest().starts_with("&|");
            let piped = err_too || (self.peek() == Some('|') && self.peek2() != Some('|'));
            elements.push(Element {
                command,
                err_too,
                span: Span {
                    start: el_start,
                    end: self.at,
                },
            });
            if !piped {
                break;
            }
            self.at += if err_too { 2 } else { 1 };
            self.blank_lines();
            if self.peek().is_none() {
                return Err(Error::Incomplete);
            }
        }
        Ok(Pipeline {
            negated,
            elements,
            span: Span {
                start,
                end: self.at,
            },
        })
    }

    fn keyword(&self) -> Option<&'static str> {
        let w = self.peek_literal()?;
        KEYWORDS.into_iter().find(|k| *k == w)
    }

    fn eat_keyword(&mut self, k: &str) -> bool {
        if self.peek_literal() == Some(k) {
            self.at += k.len();
            self.blanks();
            true
        } else {
            false
        }
    }

    fn block(&mut self, after: &str) -> Result<Block, Error> {
        self.blanks();
        if self.peek().is_none() {
            return Err(Error::Incomplete);
        }
        if !self.at_brace('{') {
            let start = self.at;
            let line_end = self
                .rest()
                .find('\n')
                .map_or(self.src.len(), |n| self.at + n);
            let what = self.src[start..line_end].trim();
            let body = what.strip_suffix('}').unwrap_or(what).trim();
            return Err(Error::Invalid {
                at: start,
                problem: alloc::format!("'{after}' needs a block"),
                hint: Some(alloc::format!("write: {after} {{ {body} }}")),
            });
        }
        self.at += 1;
        self.statements(Some(End::Brace))
    }

    fn element(&mut self, end: Option<End>) -> Result<Command, Error> {
        self.blanks();
        let start = self.at;
        let doc = core::mem::take(&mut self.doc);
        let Some(k) = self.keyword() else {
            return Ok(Command::Simple(self.simple(end)?));
        };
        self.at += k.len();
        self.blanks();
        match k {
            "if" => {
                let mut branches = Vec::new();
                let mut otherwise = None;
                loop {
                    let condition = self.condition("if")?;
                    let body = self.block("if")?;
                    branches.push((condition, body));
                    self.blanks();
                    if !self.eat_keyword("else") {
                        break;
                    }
                    if self.eat_keyword("if") {
                        continue;
                    }
                    otherwise = Some(self.block("else")?);
                    break;
                }
                Ok(Command::If {
                    branches,
                    otherwise,
                })
            }
            "for" => {
                let name = self.name_word("for needs a variable name")?;
                self.blanks();
                if !self.eat_keyword("in") {
                    if self.peek().is_none() {
                        return Err(Error::Incomplete);
                    }
                    return invalid(self.at, "for needs in after its variable");
                }
                let mut values = Vec::new();
                loop {
                    self.blanks();
                    match self.peek() {
                        None | Some('\n') => return Err(Error::Incomplete),
                        _ if self.at_brace('{') => break,
                        Some(c) if is_break(c) => {
                            return invalid(self.at, "for needs a block");
                        }
                        _ => values.push(self.word()?),
                    }
                }
                let body = self.block("for")?;
                Ok(Command::For { name, values, body })
            }
            "while" => {
                let condition = self.condition("while")?;
                let body = self.block("while")?;
                Ok(Command::While { condition, body })
            }
            "fn" => {
                let name_at = self.at;
                let Some(name) = self.peek_literal() else {
                    if self.peek().is_none() {
                        return Err(Error::Incomplete);
                    }
                    return invalid(self.at, "fn needs a name");
                };
                let ok = name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                    && name.len() <= 24
                    && !KEYWORDS.contains(&name);
                if !ok {
                    return invalid(name_at, "a function's name is letters, digits, - and _");
                }
                self.at += name.len();
                let mut params = Vec::new();
                loop {
                    self.blanks();
                    match self.peek() {
                        None | Some('\n') => return Err(Error::Incomplete),
                        _ if self.at_brace('{') => break,
                        _ => {
                            let p = self.name_word("a parameter is a variable name")?;
                            if params.contains(&p) {
                                return invalid(self.at, "two parameters have this name");
                            }
                            params.push(p);
                        }
                    }
                }
                let body = self.block("fn")?;
                Ok(Command::Fn(Box::new(Function {
                    name: name.into(),
                    params,
                    body,
                    doc,
                })))
            }
            "try" => {
                let body = self.block("try")?;
                self.blanks();
                let catch = if self.eat_keyword("catch") {
                    Some(self.block("catch")?)
                } else {
                    None
                };
                Ok(Command::Try { body, catch })
            }
            "break" => Ok(Command::Break),
            "continue" => Ok(Command::Continue),
            "return" => {
                self.blanks();
                let value = match self.peek() {
                    None => None,
                    Some(c) if is_break(c) || c == '#' => None,
                    _ if self.at_brace('}') => None,
                    _ => Some(self.word()?),
                };
                Ok(Command::Return(value))
            }
            "else" | "catch" => invalid(start, &alloc::format!("'{k}' with no block before it")),
            _ => invalid(start, &alloc::format!("'{k}' cannot start a command")),
        }
    }

    fn name_word(&mut self, problem: &str) -> Result<String, Error> {
        self.blanks();
        match self.peek_literal() {
            Some(w) if is_name(w) => {
                self.at += w.len();
                Ok(w.into())
            }
            _ if self.peek().is_none() => Err(Error::Incomplete),
            _ => invalid(self.at, problem),
        }
    }

    /// The condition of `if` or `while`: a chain up to the `{`.
    fn condition(&mut self, after: &str) -> Result<Chain, Error> {
        self.blanks();
        if self.at_brace('{') {
            return invalid(
                self.at,
                &alloc::format!("'{after}' needs a command before its block"),
            );
        }
        if self.peek().is_none() {
            return Err(Error::Incomplete);
        }
        self.chain(None)
    }

    fn simple(&mut self, end: Option<End>) -> Result<Simple, Error> {
        let mut s = Simple {
            grants: Vec::new(),
            assignments: Vec::new(),
            words: Vec::new(),
            redirects: Vec::new(),
        };
        loop {
            self.blanks();
            let Some(c) = self.peek() else { break };
            if c == '\n' || c == ';' || c == '#' {
                break;
            }
            if c == '&' && !matches!(self.peek2(), Some('>')) {
                break;
            }
            if c == '|' {
                break;
            }
            if c == ')' && (end == Some(End::Paren) || self.parens > 0) {
                break;
            }
            if self.at_brace('{') || self.at_brace('}') {
                break;
            }
            if let Some(r) = self.redirect()? {
                s.redirects.push(r);
                continue;
            }
            let start = self.at;
            let prefix = s.words.is_empty();
            if prefix && let Some(marker @ ("+r" | "+w")) = self.peek_literal() {
                self.at += 2;
                self.blanks();
                match self.peek() {
                    None => return invalid(self.at, &alloc::format!("{marker} needs a path")),
                    Some(c) if is_break(c) => {
                        return invalid(self.at, &alloc::format!("{marker} needs a path"));
                    }
                    _ => {}
                }
                let path = self.word()?;
                s.grants.push(Grant {
                    write: marker == "+w",
                    path,
                    span: Span {
                        start,
                        end: self.at,
                    },
                });
                continue;
            }
            let word = self.word()?;
            if prefix && let Some((name, cut)) = assignment(&word) {
                let mut value = word.clone();
                if let Some(Part::Literal(first)) = value.parts.first_mut() {
                    first.drain(..cut);
                    if first.is_empty() {
                        value.parts.remove(0);
                    }
                }
                value.span.start += cut;
                s.assignments.push(Assignment { name, value });
                continue;
            }
            s.words.push(word);
        }
        if s.words.is_empty()
            && s.assignments.is_empty()
            && s.grants.is_empty()
            && s.redirects.is_empty()
        {
            return match self.peek() {
                None => Err(Error::Incomplete),
                Some('&') if self.rest().starts_with("&&") => {
                    invalid(self.at, "&& with no command before it")
                }
                Some('|') => invalid(self.at, "| with no command before it"),
                Some('&') => invalid(self.at, "& with no command before it"),
                Some('{') => invalid(self.at, "a block needs if, for, while, fn or try before it"),
                _ => invalid(self.at, "a command is missing"),
            };
        }
        if s.words.is_empty() && !s.grants.is_empty() {
            return invalid(self.at, "a grant needs a command after it");
        }
        Ok(s)
    }

    fn redirect(&mut self) -> Result<Option<Redirect>, Error> {
        let start = self.at;
        let rest = self.rest();
        let (stream, append, len) = if rest.starts_with("2>>") {
            (Stream::Err, true, 3)
        } else if rest.starts_with("2>") {
            (Stream::Err, false, 2)
        } else if rest.starts_with("&>") {
            (Stream::Both, false, 2)
        } else if rest.starts_with(">>") {
            (Stream::Out, true, 2)
        } else if rest.starts_with('>') {
            (Stream::Out, false, 1)
        } else if rest.starts_with('<') {
            (Stream::In, false, 1)
        } else {
            return Ok(None);
        };
        self.at += len;
        self.blanks();
        match self.peek() {
            None | Some('\n') => return invalid(self.at, "a redirection needs a file name"),
            Some(c) if is_break(c) || c == '#' => {
                return invalid(self.at, "a redirection needs a file name");
            }
            _ => {}
        }
        let target = self.word()?;
        Ok(Some(Redirect {
            stream,
            append,
            target,
            span: Span {
                start,
                end: self.at,
            },
        }))
    }

    fn word(&mut self) -> Result<Word, Error> {
        let start = self.at;
        let mut parts: Vec<Part> = Vec::new();
        let mut text = String::new();
        let mut quoted = String::new();
        // Flushes pending text of one kind before a part of another.
        fn flush(parts: &mut Vec<Part>, text: &mut String, quoted: &mut String) {
            if !text.is_empty() {
                parts.push(Part::Literal(core::mem::take(text)));
            }
            if !quoted.is_empty() {
                parts.push(Part::Quoted(core::mem::take(quoted)));
            }
        }
        while let Some(c) = self.peek() {
            if is_break(c) {
                break;
            }
            match c {
                '\'' => {
                    if !text.is_empty() {
                        flush(&mut parts, &mut text, &mut quoted);
                    }
                    self.at += 1;
                    let Some(n) = self.rest().find('\'') else {
                        return Err(Error::Incomplete);
                    };
                    let inside = &self.src[self.at..self.at + n];
                    // An empty '' still makes a word.
                    if inside.is_empty() && quoted.is_empty() {
                        parts.push(Part::Quoted(String::new()));
                    }
                    quoted.push_str(inside);
                    self.at += n + 1;
                }
                '"' => {
                    if !text.is_empty() {
                        flush(&mut parts, &mut text, &mut quoted);
                    }
                    self.at += 1;
                    let before = parts.len();
                    self.double_quoted(&mut parts, &mut quoted)?;
                    if parts.len() == before && quoted.is_empty() {
                        parts.push(Part::Quoted(String::new()));
                    }
                }
                '\\' => {
                    self.at += 1;
                    match self.bump() {
                        None => return Err(Error::Incomplete),
                        Some('\n') => {}
                        Some(e) => {
                            if !text.is_empty() {
                                flush(&mut parts, &mut text, &mut quoted);
                            }
                            quoted.push(e);
                        }
                    }
                }
                '$' => {
                    flush(&mut parts, &mut text, &mut quoted);
                    let part = self.dollar(false)?;
                    parts.push(part);
                }
                ')' if self.parens > 0 => break,
                _ => {
                    if !quoted.is_empty() {
                        flush(&mut parts, &mut text, &mut quoted);
                    }
                    text.push(c);
                    self.at += c.len_utf8();
                }
            }
        }
        flush(&mut parts, &mut text, &mut quoted);
        if parts.is_empty() {
            return invalid(start, "a word is missing");
        }
        Ok(Word {
            parts,
            span: Span {
                start,
                end: self.at,
            },
        })
    }

    fn double_quoted(&mut self, parts: &mut Vec<Part>, quoted: &mut String) -> Result<(), Error> {
        loop {
            let Some(c) = self.bump() else {
                return Err(Error::Incomplete);
            };
            match c {
                '"' => return Ok(()),
                '\\' => match self.bump() {
                    None => return Err(Error::Incomplete),
                    Some('"') => quoted.push('"'),
                    Some('\\') => quoted.push('\\'),
                    Some('$') => quoted.push('$'),
                    Some('n') => quoted.push('\n'),
                    Some('t') => quoted.push('\t'),
                    Some(other) => {
                        quoted.push('\\');
                        quoted.push(other);
                    }
                },
                '$' => {
                    if !quoted.is_empty() {
                        parts.push(Part::Quoted(core::mem::take(quoted)));
                    }
                    self.at -= 1;
                    let part = self.dollar(true)?;
                    parts.push(part);
                }
                _ => quoted.push(c),
            }
        }
    }

    fn dollar(&mut self, quoted: bool) -> Result<Part, Error> {
        let start = self.at;
        self.at += 1; // `$`
        if self.peek() == Some('(') {
            self.at += 1;
            self.parens += 1;
            let saved = core::mem::take(&mut self.doc);
            let body = self.statements(Some(End::Paren));
            self.doc = saved;
            self.parens -= 1;
            return Ok(Part::Subst {
                body: body?,
                quoted,
            });
        }
        let rest = self.rest();
        let len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        let name = &rest[..len];
        if !is_name(name) {
            return Err(Error::Invalid {
                at: start,
                problem: "$ needs a variable name".into(),
                hint: Some("for a $ itself, write \\$ or put it in '...'".into()),
            });
        }
        self.at += len;
        let mut index = None;
        if self.peek() == Some('[') {
            let open = self.at;
            let Some(close) = self.rest().find(']') else {
                return invalid(open, "an index needs ]");
            };
            let inside = &self.src[open + 1..open + close];
            let n = match zoraya_describe::parse_number(inside) {
                Some(n) if n != 0 => n,
                _ => return invalid(open, "an index is a whole number other than 0"),
            };
            index = Some(n);
            self.at = open + close + 1;
        }
        Ok(Part::Var {
            name: name.into(),
            index,
            quoted,
        })
    }
}

/// The line and column (both from 1, the column in characters) of byte `at`.
pub fn position(src: &str, at: usize) -> (usize, usize) {
    let at = at.min(src.len());
    let before = &src[..at];
    let line = 1 + before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |n| n + 1);
    (line, src[line_start..at].chars().count() + 1)
}

/// The text of line `line` (from 1).
pub fn line_text(src: &str, line: usize) -> &str {
    src.split('\n').nth(line.saturating_sub(1)).unwrap_or("")
}
