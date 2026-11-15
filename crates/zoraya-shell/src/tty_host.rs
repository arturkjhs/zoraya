//! The shell as the terminal service of its window (ADR-0331, ADR-0386): the
//! decisions `zosh` takes about the discipline, apart from the machine, so
//! that they are host-tested. `zosh` owns the handles and the channels; this
//! module owns the rules:
//!
//! - which line is in front, and that only its programs read keys or change
//!   modes (INV-0331-2, INV-0331-5);
//! - that a stop or the end of the line in front gives the terminal its
//!   default attributes and asks `term` to restore the screen, and that `fg`
//!   gives both back (INV-0331-4);
//! - that a change of the window's size is one `SIGWINCH` to the line in front
//!   (INV-0331-6);
//! - that a `tcsetpgrp` the broker passes on is accepted only for a group
//!   made inside the line in front: the broker names the line by the group
//!   the shell started (its root), which the shell knows (ADR-0331 item 6,
//!   ADR-0388);
//! - that once a group is chosen, the signal characters and `SIGWINCH` go to
//!   that group alone, and before, `SIGWINCH` only to the groups the shell
//!   started, never to a native element (the review of 10c, findings 2, 3);
//! - the carriage return of a program's output (INV-0331-7), and the paste to
//!   a program: brackets when it set 2004, a confirmation for a newline when
//!   it did not, and nothing that can end the bracket (ADR-0332 item 8,
//!   INV-0332-5).
//!
//! It tests no termios flag itself (INV-0331-8): `zoraya-tty` answers.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_tty::{Action, Id, Terminal, Termios};

use crate::crlf::Crlf;
use crate::grantline::{GrantLine, Mode, Step};
use crate::refusal::given_notice;
use zoraya_proto_term::{STATUS_FIELD, STATUS_NOTICE, STATUS_PLAIN};

/// What the shell must do after a call.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect<H> {
    /// An action of the discipline: replies to connections, echo, signals.
    Tty(Action<H>),
    /// `Restore` to `term` (`keep`: a stop, to be set again by `Reinstate`).
    Restore { keep: bool },
    /// `Reinstate` to `term`: a stopped line is back in front.
    Reinstate,
    /// The window's title, out of band: `Some(question)` while a paste waits
    /// for its confirmation, `None` to show the directory again.
    Question(Option<String>),
    /// `kill(-group, signal)` through the broker: a signal character or
    /// `SIGWINCH` for one POSIX group, not the line's whole job.
    GroupSignal { group: u32, signal: u32 },
    /// Tell the broker that the line of these roots is in the background
    /// (`back`) or in front, so that its `tcsetpgrp` is refused with `EPERM`
    /// at once while it is not in front (the architect, 2026-11-15).
    Background { roots: Vec<u32>, back: bool },
    /// The status row's text, out of band (ADR-0337 items 3 and 5): `kind`
    /// is `STATUS_NOTICE`, `STATUS_PLAIN` or `STATUS_FIELD`; empty text clears
    /// the row.
    Status { kind: u32, text: String },
    /// Enter on a grant line, after the guard: `text` is what the person
    /// typed for element `element` of `line`. The shell parses it with
    /// `grantline::parse_give` against its own table and answers with
    /// [`Host::gave`].
    Give {
        line: u32,
        element: usize,
        text: String,
    },
    /// Enter on the kill offer, after the guard: end `line`'s group
    /// (ADR-0387 item 4, ADR-0158 rule 3).
    Kill { line: u32 },
    /// Tab on a non-empty grant line: the shell lists the names of its own
    /// table that begin with `word` and answers with [`Host::completed`].
    Complete { word: String },
}

/// Where the shell runs, for the attention key (ADR-0337 item 10, INV-0337-7).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The first terminal, a room's own terminal, a shell that entered a room.
    #[default]
    Normal,
    /// A program of an untrusted room: the grant line gives nothing.
    Untrusted,
    /// The serial console: no status row and no reserved chord.
    Console,
}

/// `SIGINT`, `SIGQUIT` and `SIGWINCH` (ADR-0106's numbers).
const SIGINT: u32 = 2;
const SIGQUIT: u32 = 3;
const SIGWINCH: u32 = 28;

/// Where the keys after a held paste's dismissal are: the rest of the key
/// that dismissed it is not the program's (the review of 10c, finding 9).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Swallow {
    #[default]
    No,
    /// After `ESC`.
    Esc,
    /// Inside `CSI`: parameters and intermediates until the final byte.
    Csi,
    /// After `ESC O`: one more byte.
    Ss3,
}

/// Why a request of the broker was not taken.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// No line is in front, or the group was not created inside it.
    NotInFront,
}

/// Mode 2004 (bracketed paste) as the program set it, read from the bytes it
/// writes (ADR-0386 item 3). A sequence cut by a chunk is carried over; any
/// byte that does not belong to `CSI ? Ps ; Ps ... h` or `l` abandons it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Bracket {
    scan: Scan,
    param: u32,
    saw: bool,
    on: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Scan {
    #[default]
    Ground,
    Esc,
    Csi,
    Private,
}

impl Bracket {
    /// Whether the program has bracketed paste on.
    pub const fn on(&self) -> bool {
        self.on
    }

    /// Back to the default, with nothing carried over.
    pub fn reset(&mut self) {
        *self = Bracket::default();
    }

    /// Reads a chunk of the program's output.
    pub fn scan(&mut self, bytes: &[u8]) {
        for &b in bytes {
            match (self.scan, b) {
                (_, 0x1b) => self.scan = Scan::Esc,
                (Scan::Esc, b'[') => self.scan = Scan::Csi,
                (Scan::Csi, b'?') => {
                    self.scan = Scan::Private;
                    self.param = 0;
                    self.saw = false;
                }
                (Scan::Private, b'0'..=b'9') => {
                    self.param = self
                        .param
                        .saturating_mul(10)
                        .saturating_add(u32::from(b - b'0'));
                }
                (Scan::Private, b';') => {
                    self.saw |= self.param == 2004;
                    self.param = 0;
                }
                (Scan::Private, b'h' | b'l') => {
                    if self.saw || self.param == 2004 {
                        self.on = b == b'h';
                    }
                    self.scan = Scan::Ground;
                }
                _ => self.scan = Scan::Ground,
            }
        }
    }
}

/// One line of the shell that holds, or may hold, a terminal.
struct Line<H> {
    id: u32,
    job: H,
    /// Whether any of its programs has the terminal (INV-0331-1).
    terminal: bool,
    /// The groups the shell started its POSIX programs in: the roots by which
    /// the broker names the line (ADR-0388). Groups made inside it since are
    /// the broker's to know, so nothing here grows with the line's jobs.
    roots: Vec<u32>,
    /// The group a `tcsetpgrp` chose; until one does the whole line reads
    /// (`Terminal::set_foreground_line`).
    chosen: Option<u32>,
    /// A stopped line's attributes, for `fg`.
    saved: Option<Termios>,
    bracket: Bracket,
    /// The names of the line's elements, for the grant line.
    elements: Vec<String>,
    /// The element the status row's refusal came from.
    refused: Option<usize>,
}

/// The grant line or the kill offer, open on one line.
struct Open {
    line: u32,
    gl: GrantLine,
}

/// A paste held until the person's Enter or Escape.
struct Held {
    text: String,
}

/// The shell's host of the discipline for one window.
pub struct Host<H> {
    pub tty: Terminal<H>,
    lines: Vec<Line<H>>,
    front: Option<u32>,
    held: Option<Held>,
    swallow: Swallow,
    grant: Option<Open>,
    /// What the status row shows when no field is open.
    notice: Option<(u32, String)>,
    scope: Scope,
    /// The clock, in milliseconds, for the guard.
    now: u64,
}

impl<H: Copy + Eq + core::fmt::Debug> Default for Host<H> {
    fn default() -> Self {
        Host::new()
    }
}

fn effects<H>(actions: Vec<Action<H>>, out: &mut Vec<Effect<H>>) {
    out.extend(actions.into_iter().map(Effect::Tty));
}

/// The number of lines a paste holds, for the question.
fn lines_of(text: &str) -> usize {
    let n = text.bytes().filter(|&b| b == b'\n').count();
    n + usize::from(!text.ends_with('\n'))
}

impl<H: Copy + Eq + core::fmt::Debug> Host<H> {
    pub fn new() -> Host<H> {
        let mut tty = Terminal::new();
        // Lines are told apart from the start: with none in front nobody reads.
        let _ = tty.set_front_line(None);
        Host {
            tty,
            lines: Vec::new(),
            front: None,
            held: None,
            swallow: Swallow::No,
            grant: None,
            notice: None,
            scope: Scope::Normal,
            now: 0,
        }
    }

    fn line(&self, id: u32) -> Option<&Line<H>> {
        self.lines.iter().find(|l| l.id == id)
    }

    fn line_mut(&mut self, id: u32) -> Option<&mut Line<H>> {
        self.lines.iter_mut().find(|l| l.id == id)
    }

    /// The line in front.
    pub fn front(&self) -> Option<u32> {
        self.front
    }

    /// A line starts: `job` receives its signal characters, `group` is the
    /// group of its first program, `terminal` whether any program has the
    /// terminal. The shell hands each program's connection out with
    /// `Terminal::add_terminal_in(group, line)`.
    pub fn start_line(&mut self, id: u32, job: H, group: u32, terminal: bool) {
        self.lines.retain(|l| l.id != id);
        self.lines.push(Line {
            id,
            job,
            terminal,
            roots: if group == 0 {
                Vec::new()
            } else {
                alloc::vec![group]
            },
            chosen: None,
            saved: None,
            bracket: Bracket::default(),
            elements: Vec::new(),
            refused: None,
        });
    }

    /// Whether line `id` has a record.
    pub fn has_line(&self, id: u32) -> bool {
        self.line(id).is_some()
    }

    /// The shell started a POSIX program of line `id` in `group` (one per
    /// element: a pipeline's elements are groups of their own).
    pub fn add_root(&mut self, id: u32, group: u32) {
        if let Some(l) = self.line_mut(id)
            && group != 0
            && !l.roots.contains(&group)
        {
            l.roots.push(group);
        }
    }

    /// The broker passed on a `tcsetpgrp(group)` of a process whose line's
    /// root is `root` (ADR-0388 item 4; the broker checked that `group` was
    /// made inside that same line): taken only when that line is in front.
    pub fn foreground_asked(&mut self, group: u32, root: u32) -> Result<Vec<Effect<H>>, Refused> {
        let front = self.front.ok_or(Refused::NotInFront)?;
        let l = self.line(front).ok_or(Refused::NotInFront)?;
        if root == 0 || group == 0 || !l.roots.contains(&root) {
            return Err(Refused::NotInFront);
        }
        let job = l.job;
        if let Some(l) = self.line_mut(front) {
            l.chosen = Some(group);
            // 2004 is the reader's: a new reader has not set it.
            l.bracket.reset();
        }
        let mut out = Vec::new();
        effects(self.tty.set_foreground(group, job), &mut out);
        Ok(out)
    }

    /// Whether the keys go to the discipline: a line with a terminal is in
    /// front. Otherwise they are the shell's.
    pub fn takes_keys(&self) -> bool {
        self.front
            .and_then(|f| self.line(f))
            .is_some_and(|l| l.terminal)
    }

    /// Line `id` comes to the front: new, continued with `fg`, or brought
    /// back from the background. What was typed for another line is dropped
    /// (INV-0331-5); a stopped line gets its attributes and its screen back
    /// and a `SIGWINCH` so that it repaints (ADR-0331 item 9).
    pub fn bring_front(&mut self, id: u32) -> Vec<Effect<H>> {
        let mut out = Vec::new();
        let Some(l) = self.line(id) else {
            return out;
        };
        let (job, chosen, saved, terminal) = (l.job, l.chosen, l.saved, l.terminal);
        if !l.roots.is_empty() {
            out.push(Effect::Background {
                roots: l.roots.clone(),
                back: false,
            });
        }
        self.swallow = Swallow::No;
        self.drop_held(&mut out);
        self.clear_row(&mut out);
        self.tty.discipline.flush();
        self.front = Some(id);
        effects(self.tty.set_front_line(Some(id)), &mut out);
        match chosen {
            Some(group) => effects(self.tty.set_foreground(group, job), &mut out),
            None => effects(self.tty.set_foreground_line(job), &mut out),
        }
        if let Some(attr) = saved {
            effects(self.tty.continue_line(attr), &mut out);
            if let Some(l) = self.line_mut(id) {
                l.saved = None;
            }
            out.push(Effect::Reinstate);
            if terminal {
                out.extend(self.winch());
            }
        } else if terminal {
            // A new line starts from the defaults the end of the last gave.
            self.tty.discipline.flush();
        }
        out
    }

    /// `SIGWINCH` for the line in front: to the group a `tcsetpgrp` chose,
    /// or to each group the shell started; never to the line's job, which
    /// holds its native elements too (they have no handler and would be
    /// ended, the review of 10c, finding 3).
    fn winch(&self) -> Vec<Effect<H>> {
        let Some(l) = self.front.and_then(|f| self.line(f)) else {
            return Vec::new();
        };
        let groups: Vec<u32> = match l.chosen {
            Some(g) => alloc::vec![g],
            None => l.roots.clone(),
        };
        groups
            .into_iter()
            .map(|group| Effect::GroupSignal {
                group,
                signal: SIGWINCH,
            })
            .collect()
    }

    /// The discipline's actions for the keys of the line in front: once a
    /// group is chosen, an interrupt or a quit is for that group alone (as
    /// the suspend character is), not for the line's job, which holds every
    /// job a POSIX shell made (the review of 10c, finding 2).
    fn keyed(&self, actions: Vec<Action<H>>, out: &mut Vec<Effect<H>>) {
        let chosen = self.front.and_then(|f| self.line(f)).and_then(|l| l.chosen);
        for a in actions {
            match (a, chosen) {
                (
                    Action::Signal {
                        kind:
                            kind @ (zoraya_abi::RequestKind::Interrupt | zoraya_abi::RequestKind::Quit),
                        ..
                    },
                    Some(group),
                ) => out.push(Effect::GroupSignal {
                    group,
                    signal: if kind == zoraya_abi::RequestKind::Interrupt {
                        SIGINT
                    } else {
                        SIGQUIT
                    },
                }),
                (a, _) => out.push(Effect::Tty(a)),
            }
        }
    }

    /// The line in front stops or goes to the background: its attributes are
    /// kept for `fg`, the terminal's go back to the defaults, and `term` is
    /// asked to restore the screen, keeping what it clears.
    pub fn stop_front(&mut self) -> Vec<Effect<H>> {
        let mut out = Vec::new();
        let Some(id) = self.front.take() else {
            return out;
        };
        self.drop_held(&mut out);
        self.clear_row(&mut out);
        let (was, actions) = self.tty.end_line();
        effects(actions, &mut out);
        self.leave(&mut out);
        if let Some(l) = self.line_mut(id) {
            l.saved = Some(was);
            if !l.roots.is_empty() {
                out.push(Effect::Background {
                    roots: l.roots.clone(),
                    back: true,
                });
            }
        }
        out.push(Effect::Restore { keep: true });
        out
    }

    /// Line `id` ended: its record goes, and if it was in front the terminal
    /// is as a new shell expects it.
    pub fn end_line(&mut self, id: u32) -> Vec<Effect<H>> {
        let mut out = Vec::new();
        if self.front == Some(id) {
            self.front = None;
            self.drop_held(&mut out);
            self.clear_row(&mut out);
            let (_, actions) = self.tty.end_line();
            effects(actions, &mut out);
            self.leave(&mut out);
            out.push(Effect::Restore { keep: false });
        }
        self.lines.retain(|l| l.id != id);
        out
    }

    fn leave(&mut self, out: &mut Vec<Effect<H>>) {
        self.tty.clear_foreground_group();
        effects(self.tty.set_front_line(None), out);
    }

    fn drop_held(&mut self, out: &mut Vec<Effect<H>>) {
        if self.held.take().is_some() {
            out.push(Effect::Question(None));
        }
    }

    /// `term` says the window has this size: the discipline's size changes
    /// and the line in front gets one `SIGWINCH` (INV-0331-6). A program
    /// cannot set the size; no request does.
    pub fn size(&mut self, rows: u32, columns: u32) -> Vec<Effect<H>> {
        // The discipline's size; its signal to the line's job is not used
        // (`winch`).
        let _ = self.tty.resize(rows, columns);
        // A native line has no handler for the request: only a line with
        // the terminal is told (ADR-0331 item 7 is about POSIX programs).
        if self.takes_keys() {
            self.winch()
        } else {
            Vec::new()
        }
    }

    /// Keys typed while the line in front has a terminal. The discipline
    /// edits, echoes, signals and holds them; a paste waiting for its
    /// confirmation takes the first key (Enter sends it, anything else drops
    /// it, ADR-0386 item 4) and nothing else does anything meanwhile.
    pub fn keys(&mut self, bytes: &[u8]) -> Vec<Effect<H>> {
        if self.grant.is_some() {
            return self.grant_keys(bytes);
        }
        let mut out = Vec::new();
        if !self.takes_keys() {
            return out;
        }
        let mut rest = bytes;
        while !rest.is_empty() {
            if self.swallow != Swallow::No {
                let b = rest[0];
                self.swallow = match (self.swallow, b) {
                    (Swallow::Esc, b'[') => Swallow::Csi,
                    (Swallow::Esc, b'O') => Swallow::Ss3,
                    (Swallow::Csi, 0x20..=0x3f) => Swallow::Csi,
                    // Alt and a key, the final byte of CSI, SS3's byte.
                    (Swallow::Esc | Swallow::Csi | Swallow::Ss3, 0x20..=0x7e) => Swallow::No,
                    // Anything else is a key of its own.
                    _ => {
                        self.swallow = Swallow::No;
                        continue;
                    }
                };
                rest = &rest[1..];
                continue;
            }
            if let Some(held) = self.held.take() {
                out.push(Effect::Question(None));
                match rest[0] {
                    b'\r' | b'\n' => self.send_paste(&held.text, &mut out),
                    0x1b => self.swallow = Swallow::Esc,
                    _ => {}
                }
                rest = &rest[1..];
                continue;
            }
            let actions = self.tty.keys(rest);
            self.keyed(actions, &mut out);
            break;
        }
        out
    }

    /// The keys of one read from `term` are all given: a key that dismissed
    /// a held paste cannot continue into the next read.
    pub fn end_of_keys(&mut self) {
        self.swallow = Swallow::No;
    }

    /// Whether the line in front set 2004 and reads as an editor does: a
    /// program in canonical mode cannot take the marks for what they are, so
    /// a newline still asks (the review of 10c, finding 4: `cat` of a file
    /// holding the sequence would otherwise turn the question off).
    fn bracketed(&self) -> bool {
        !self.tty.discipline.canonical()
            && self
                .front
                .and_then(|f| self.line(f))
                .is_some_and(|l| l.bracket.on())
    }

    /// A cleaned paste (`paste::Assembler`) for the program in front.
    /// Returns false when no program with a terminal is in front: the paste
    /// is not for it.
    pub fn paste(&mut self, text: &str, out: &mut Vec<Effect<H>>) -> bool {
        if self.grant.is_some() {
            out.extend(self.grant_paste(text));
            return true;
        }
        if !self.takes_keys() {
            return false;
        }
        let on = self.bracketed();
        if self.held.is_some() {
            // One question at a time: the new paste replaces the old.
            self.held = None;
        }
        if !on && text.contains('\n') {
            out.push(Effect::Question(Some(format!(
                "paste {} lines? Enter sends, any other key drops",
                lines_of(text)
            ))));
            self.held = Some(Held { text: text.into() });
        } else {
            self.send_paste(text, out);
        }
        true
    }

    /// Text through the discipline: newlines as carriage returns, as a
    /// terminal sends them, between the marks when 2004 is set. The text was
    /// cleaned: no `ESC`, so it cannot end the bracket, and no control
    /// character, so no signal character is in it.
    fn send_paste(&mut self, text: &str, out: &mut Vec<Effect<H>>) {
        let on = self.bracketed();
        let mut bytes = Vec::with_capacity(text.len() + 12);
        if on {
            bytes.extend_from_slice(b"\x1b[200~");
        }
        bytes.extend(text.bytes().map(|b| if b == b'\n' { b'\r' } else { b }));
        if on {
            bytes.extend_from_slice(b"\x1b[201~");
        }
        // The marks are the shell's, not typed: ISIG cannot see them as keys
        // because ESC is no signal character.
        let actions = self.tty.keys(&bytes);
        self.keyed(actions, out);
    }

    /// A program's output on its way to `term` (INV-0331-7): the carriage
    /// return as its `OPOST` and `ONLCR` say, now. A line's program also
    /// tells the shell whether it has set bracketed paste.
    pub fn relay(&mut self, line: u32, bytes: &[u8], crlf: &mut Crlf, out: &mut Vec<u8>) {
        let onlcr = self.tty.discipline.onlcr();
        crlf.convert(bytes, onlcr, out);
        if let Some(l) = self.line_mut(line) {
            l.bracket.scan(bytes);
        }
    }

    // --- The status row, the grant line and the kill offer (ADR-0337) ---

    /// The clock, for the guard of the grant line; `zosh` sets it where it
    /// sets the discipline's.
    pub fn set_clock(&mut self, now_ms: u64) {
        self.now = now_ms;
    }

    /// Where this shell runs (INV-0337-7).
    pub fn set_scope(&mut self, scope: Scope) {
        self.scope = scope;
    }

    /// The names of the elements of line `id`, in the line's order.
    pub fn set_elements(&mut self, id: u32, names: Vec<String>) {
        if let Some(l) = self.line_mut(id) {
            l.elements = names;
        }
    }

    /// Whether a grant line or kill offer is open: then every key and paste
    /// is its, none the program's (INV-0337-2).
    pub fn grant_open(&self) -> bool {
        self.grant.is_some()
    }

    /// The text of the open field, for tests and the shell's completion.
    pub fn grant_text(&self) -> Option<&str> {
        self.grant.as_ref().map(|o| o.gl.text())
    }

    fn status(kind: u32, text: &str) -> Effect<H> {
        Effect::Status {
            kind,
            text: String::from(text),
        }
    }

    /// The row as it is when no field is open.
    fn rest_row(&self) -> Effect<H> {
        match &self.notice {
            Some((kind, text)) => Self::status(*kind, text),
            None => Self::status(STATUS_PLAIN, ""),
        }
    }

    /// A refusal of element `element` of the line in front (the shell wrote
    /// the words with `refusal::notice`): shown at once unless a field is
    /// open, and remembered as the element a grant line names (ADR-0337 items
    /// 3 and 5). Nothing a program writes comes here but through the shell's
    /// own reading of its report channel (INV-0337-5). Not for a line that is
    /// not in front, nor on the console.
    pub fn refusal(&mut self, line: u32, element: usize, text: &str) -> Vec<Effect<H>> {
        let mut out = Vec::new();
        if self.front != Some(line) || self.scope == Scope::Console {
            return out;
        }
        if let Some(l) = self.line_mut(line) {
            l.refused = Some(element);
        }
        self.notice = Some((STATUS_NOTICE, String::from(text)));
        if self.grant.is_none() {
            out.push(self.rest_row());
        }
        out
    }

    /// The attention message of the terminal: System+G (`give`) or System+K
    /// (`kill`). Opens the field for the line in front; the key again closes
    /// it. With no line in front, on the console, or (for a grant) in an
    /// untrusted room it gives nothing (ADR-0337 items 4, 5 and 10).
    pub fn attention(&mut self, give: bool) -> Vec<Effect<H>> {
        let mut out = Vec::new();
        let Some(front) = self.front else {
            return out;
        };
        if self.scope == Scope::Console {
            return out;
        }
        let mode = if give { Mode::Give } else { Mode::Kill };
        if let Some(open) = self.grant.take() {
            let same = open.gl.mode() == mode;
            out.push(self.rest_row());
            if same {
                return out;
            }
        }
        if give && self.scope == Scope::Untrusted {
            out.push(Self::status(STATUS_PLAIN, "not for an untrusted room"));
            return out;
        }
        let Some(l) = self.line(front) else {
            return out;
        };
        let mut names = l.elements.clone();
        if names.is_empty() {
            names.push(String::new());
        }
        let gl = GrantLine::open(mode, names, l.refused, self.now);
        out.push(Self::status(STATUS_FIELD, &gl.field()));
        self.grant = Some(Open { line: front, gl });
        out
    }

    fn steps(&mut self, steps: Vec<Step>, out: &mut Vec<Effect<H>>) {
        for step in steps {
            let Some(open) = self.grant.as_ref() else {
                return;
            };
            let line = open.line;
            match step {
                Step::Redraw => out.push(Self::status(STATUS_FIELD, &open.gl.field())),
                Step::Close => {
                    self.grant = None;
                    out.push(self.rest_row());
                }
                Step::Give(text) => out.push(Effect::Give {
                    line,
                    element: open.gl.element(),
                    text,
                }),
                Step::Kill => {
                    self.grant = None;
                    out.push(self.rest_row());
                    out.push(Effect::Kill { line });
                }
                Step::Complete(word) => out.push(Effect::Complete { word }),
            }
        }
    }

    /// Keys while a field is open: its alone (INV-0337-2).
    pub fn grant_keys(&mut self, bytes: &[u8]) -> Vec<Effect<H>> {
        let mut out = Vec::new();
        let now = self.now;
        if let Some(open) = self.grant.as_mut() {
            let steps = open.gl.keys(bytes, now);
            self.steps(steps, &mut out);
        }
        out
    }

    /// Pasted text while a field is open: into it, never to a program, its
    /// newlines not Enter (ADR-0332 item 8).
    pub fn grant_paste(&mut self, text: &str) -> Vec<Effect<H>> {
        let mut out = Vec::new();
        if let Some(open) = self.grant.as_mut() {
            let steps = open.gl.paste(text);
            self.steps(steps, &mut out);
        }
        out
    }

    /// The shell's candidates for the word [`Effect::Complete`] named.
    pub fn completed(&mut self, candidates: &[String]) -> Vec<Effect<H>> {
        let mut out = Vec::new();
        if let Some(open) = self.grant.as_mut() {
            let steps = open.gl.complete(candidates);
            self.steps(steps, &mut out);
        }
        out
    }

    /// The shell's answer to [`Effect::Give`]: `Ok(words)` closes the field
    /// and says what is given until the program ends and its refusals start
    /// to be counted again (ADR-0337 items 1 and 6); `Err(why)` closes it and
    /// says why nothing was given.
    pub fn gave(&mut self, result: Result<&str, &str>) -> Vec<Effect<H>> {
        let mut out = Vec::new();
        let Some(open) = self.grant.take() else {
            return out;
        };
        match result {
            Ok(words) => {
                let text = given_notice(&open.gl.program(), words);
                if let Some(l) = self.line_mut(open.line) {
                    l.refused = None;
                }
                self.notice = Some((STATUS_PLAIN, text));
            }
            Err(why) => {
                self.notice = Some((STATUS_PLAIN, String::from(why)));
            }
        }
        out.push(self.rest_row());
        out
    }

    /// The row and the field go with the line in front (it ended, stopped or
    /// went to the background): nothing of its refusals is shown for the next
    /// one.
    fn clear_row(&mut self, out: &mut Vec<Effect<H>>) {
        let field = self.grant.take().is_some();
        let notice = self.notice.take().is_some();
        if field || notice {
            out.push(Self::status(STATUS_PLAIN, ""));
        }
    }

    /// The connection ids of a new program of `line` are the shell's to hand
    /// out; this is the call, so that the line tag is never left to the
    /// program (ADR-0331 item 4).
    pub fn connect(&mut self, line: u32, group: u32) -> Option<Id> {
        self.line(line)?;
        self.tty.add_terminal_in(group, line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paste::Assembler;
    use alloc::vec;
    use zoraya_abi::RequestKind;
    use zoraya_proto_tty::{
        Attr, GetAttr, PollReadable, Read, ReadReply, WindowSize, WindowSizeReply, status,
    };
    use zoraya_protocol::{Header, Message, decode, encode};
    use zoraya_tty::reply_status;

    type E = Effect<u32>;

    fn message<'m, M: Message<'m>>(m: &M, t: u32) -> Vec<u8> {
        let mut out = vec![0; 8192];
        let n = encode(m, t, &mut out).unwrap();
        out.truncate(n);
        out
    }

    fn replies(es: &[E]) -> Vec<(Id, Vec<u8>)> {
        es.iter()
            .filter_map(|e| match e {
                Effect::Tty(Action::Reply { to, bytes }) => Some((*to, bytes.clone())),
                _ => None,
            })
            .collect()
    }

    fn read(h: &mut Host<u32>, id: Id, t: u32) -> Result<Vec<u8>, u32> {
        let es = h.tty.request(id, &message(&Read { max: 4096 }, t), vec![]);
        let es: Vec<E> = es.into_iter().map(Effect::Tty).collect();
        let r = replies(&es);
        let (head, _) = Header::decode(&r[0].1).unwrap();
        if head.ordinal == zoraya_protocol::ERROR_ORDINAL {
            return Err(reply_status(&r[0].1));
        }
        Ok(decode::<ReadReply<'_>>(&r[0].1, 0)
            .unwrap()
            .1
            .bytes
            .0
            .to_vec())
    }

    fn set_raw(h: &mut Host<u32>, id: Id) -> u32 {
        let es = h.tty.request(
            id,
            &zoraya_tty::set_attr_request(&Termios::raw(), 50),
            vec![],
        );
        reply_status(&replies(&es.into_iter().map(Effect::Tty).collect::<Vec<E>>())[0].1)
    }

    /// Two lines with a terminal: 1 (job 101, group 11), 2 (job 102, group
    /// 12), each with a program's connection; line 1 in front.
    fn two() -> (Host<u32>, Id, Id) {
        let mut h = Host::new();
        h.start_line(1, 101, 11, true);
        h.start_line(2, 102, 12, true);
        let a = h.connect(1, 11).unwrap();
        let b = h.connect(2, 12).unwrap();
        h.bring_front(1);
        (h, a, b)
    }

    #[test]
    fn nobody_reads_with_no_line_in_front() {
        // INV-0331-2, INV-0331-5: keys typed with no line in front are not
        // the discipline's, and a program that asks gets nothing.
        let mut h = Host::new();
        h.start_line(1, 101, 11, true);
        let a = h.connect(1, 11).unwrap();
        assert!(h.keys(b"typed for the prompt\n").is_empty());
        assert_eq!(read(&mut h, a, 1), Err(status::NOT_FOREGROUND));
    }

    #[test]
    fn a_program_of_another_line_cannot_read_or_change_modes() {
        // INV-0331-2: whatever group it declares, and a clone too.
        let (mut h, a, b) = two();
        h.keys(b"hunter2\n");
        assert_eq!(read(&mut h, b, 2), Err(status::NOT_FOREGROUND));
        assert_eq!(set_raw(&mut h, b), status::IO);
        assert_eq!(h.tty.discipline.attr, Termios::default());
        let es = h.tty.request(b, &message(&PollReadable {}, 3), vec![]);
        let es: Vec<E> = es.into_iter().map(Effect::Tty).collect();
        assert_eq!(reply_status(&replies(&es)[0].1), status::IO);
        // Declaring the front line's group changes nothing.
        let c = h.connect(2, 11).unwrap();
        assert_eq!(read(&mut h, c, 4), Err(status::NOT_FOREGROUND));
        assert_eq!(read(&mut h, a, 5).unwrap(), b"hunter2\n");
        // Another line in front: the roles swap, and what line 1 typed is
        // gone (INV-0331-5).
        h.keys(b"typed for line one\n");
        h.bring_front(2);
        assert_eq!(read(&mut h, a, 6), Err(status::NOT_FOREGROUND));
        h.keys(b"for two\n");
        assert_eq!(read(&mut h, b, 7).unwrap(), b"for two\n");
    }

    #[test]
    fn keys_around_a_change_of_the_front_line_reach_only_the_line_then_in_front() {
        // INV-0331-5.
        let (mut h, a, b) = two();
        h.keys(b"one\n");
        h.bring_front(2);
        h.keys(b"two\n");
        assert_eq!(read(&mut h, b, 1).unwrap(), b"two\n");
        h.bring_front(1);
        h.keys(b"three\n");
        assert_eq!(read(&mut h, a, 2).unwrap(), b"three\n");
        h.stop_front();
        assert!(h.keys(b"for the prompt\n").is_empty());
    }

    #[test]
    fn a_line_without_a_terminal_has_no_keys() {
        // INV-0331-1's shell half: keys are the shell's.
        let mut h = Host::new();
        h.start_line(1, 101, 11, false);
        h.bring_front(1);
        assert!(!h.takes_keys());
        assert!(h.keys(b"x").is_empty());
        assert_eq!(h.connect(9, 1), None, "no connection for an unknown line");
    }

    #[test]
    fn a_size_is_one_sigwinch_to_the_line_in_front_and_tiocgwinsz_follows() {
        // INV-0331-6.
        let (mut h, a, _) = two();
        let es = h.size(40, 100);
        assert_eq!(
            es,
            [Effect::GroupSignal {
                group: 11,
                signal: 28
            }],
            "one SIGWINCH, to the group the shell started, not the line's job"
        );
        let es = h.tty.request(a, &message(&WindowSize {}, 1), vec![]);
        let es: Vec<E> = es.into_iter().map(Effect::Tty).collect();
        let (_, s) = decode::<WindowSizeReply>(&replies(&es)[0].1, 0).unwrap();
        assert_eq!((s.rows, s.columns), (40, 100));
        // With no line in front, nobody gets a signal.
        h.stop_front();
        assert!(h.size(30, 90).is_empty());
        // No request of a program sets the size: there is none to send.
    }

    #[test]
    fn a_stop_restores_the_screen_and_the_defaults_and_fg_gives_them_back() {
        // INV-0331-4, the shell's half.
        let (mut h, a, _) = two();
        assert_eq!(set_raw(&mut h, a), status::OK);
        let raw = h.tty.discipline.attr;
        h.tty.keys(b"stale");
        let es = h.stop_front();
        assert!(es.contains(&Effect::Restore { keep: true }));
        assert_eq!(h.tty.discipline.attr, Termios::default());
        assert_eq!(h.front(), None);
        let es = h.bring_front(1);
        assert!(es.contains(&Effect::Reinstate));
        assert_eq!(h.tty.discipline.attr, raw, "the attributes it had");
        assert_eq!(
            es.iter()
                .filter(|e| matches!(e, Effect::GroupSignal { signal: 28, .. }))
                .count(),
            1,
            "one SIGWINCH to repaint"
        );
        // Nothing typed before the stop reaches it.
        let es = h.tty.request(a, &message(&Read { max: 10 }, 9), vec![]);
        assert!(replies(&es.into_iter().map(Effect::Tty).collect::<Vec<E>>()).is_empty());
    }

    #[test]
    fn the_end_of_the_line_in_front_gives_a_new_shell_its_terminal() {
        let (mut h, a, _) = two();
        set_raw(&mut h, a);
        let es = h.end_line(1);
        assert!(es.contains(&Effect::Restore { keep: false }));
        assert_eq!(h.tty.discipline.attr, Termios::default());
        // The next line starts from the defaults.
        h.bring_front(2);
        let es = h.tty.request(a, &message(&GetAttr {}, 1), vec![]);
        let es: Vec<E> = es.into_iter().map(Effect::Tty).collect();
        let r = replies(&es);
        assert!(decode::<Attr<'_>>(&r[0].1, 0).is_ok());
        assert_eq!(h.tty.discipline.attr, Termios::default());
        // A line that ends in the background restores nothing.
        let mut h = Host::new();
        h.start_line(1, 101, 11, true);
        assert!(h.end_line(1).is_empty());
    }

    #[test]
    fn tcsetpgrp_is_for_a_group_made_inside_the_front_line() {
        let (mut h, _, b) = two();
        // Line 2's (root 12) is refused while line 1 is in front, whatever
        // group it names, line 1's own included.
        assert_eq!(h.foreground_asked(77, 12), Err(Refused::NotInFront));
        assert_eq!(h.foreground_asked(11, 12), Err(Refused::NotInFront));
        assert_eq!(h.foreground_asked(55, 0), Err(Refused::NotInFront));
        assert_eq!(h.foreground_asked(0, 11), Err(Refused::NotInFront));
        assert_eq!(h.tty.foreground(), None);
        assert!(h.foreground_asked(55, 11).is_ok());
        assert_eq!(h.tty.foreground(), Some(55));
        // A read from line 1's first group is now NotForeground (item 6).
        let c = h.connect(1, 11).unwrap();
        assert_eq!(read(&mut h, c, 1), Err(status::NOT_FOREGROUND));
        let _ = b;
        h.stop_front();
        assert_eq!(h.foreground_asked(55, 11), Err(Refused::NotInFront));
    }

    #[test]
    fn the_broker_is_told_when_a_line_goes_to_the_back_and_comes_to_the_front() {
        // So that a `tcsetpgrp` of a line not in front is EPERM at once
        // (the architect, 2026-11-15).
        let (mut h, _, _) = two();
        let es = h.stop_front();
        assert!(es.contains(&Effect::Background {
            roots: vec![11],
            back: true
        }));
        let es = h.bring_front(2);
        assert!(es.contains(&Effect::Background {
            roots: vec![12],
            back: false
        }));
        // A native line (no root) tells the broker nothing.
        let mut n: Host<u32> = Host::new();
        n.start_line(1, 101, 0, false);
        assert!(
            !n.bring_front(1)
                .iter()
                .any(|e| matches!(e, Effect::Background { .. }))
        );
        assert!(
            !n.stop_front()
                .iter()
                .any(|e| matches!(e, Effect::Background { .. }))
        );
    }

    #[test]
    fn a_shell_that_made_seventy_jobs_still_gives_the_next_one_the_terminal() {
        // The review of 10c, finding 1: nothing is recorded per group made,
        // so no count of jobs ends job control.
        let (mut h, a, _) = two();
        for g in 1000..1070 {
            assert!(h.foreground_asked(g, 11).is_ok(), "job {g}");
            assert_eq!(h.tty.foreground(), Some(g));
        }
        let c = h.connect(1, 1069).unwrap();
        h.keys(b"x\n");
        assert_eq!(read(&mut h, c, 1).unwrap(), b"x\n");
        assert_eq!(read(&mut h, a, 2), Err(status::NOT_FOREGROUND));
    }

    #[test]
    fn signal_characters_go_to_the_chosen_group_once_there_is_one() {
        // The review of 10c, finding 2: `sh`'s background jobs are in the
        // line's job; Ctrl-C for its foreground job must not reach them.
        let (mut h, _, _) = two();
        let es = h.keys(b"\x03");
        assert!(
            es.contains(&Effect::Tty(Action::Signal {
                job: 101,
                kind: RequestKind::Interrupt,
                detail: 2
            })),
            "no group chosen: the line is the job: {es:?}"
        );
        assert!(h.foreground_asked(55, 11).is_ok());
        let es = h.keys(b"\x03");
        assert!(es.contains(&Effect::GroupSignal {
            group: 55,
            signal: 2
        }));
        assert!(
            !es.iter()
                .any(|e| matches!(e, Effect::Tty(Action::Signal { .. })))
        );
        let es = h.keys(b"\x1c");
        assert!(es.contains(&Effect::GroupSignal {
            group: 55,
            signal: 3
        }));
        // The chosen group gets the size too, and only it.
        assert_eq!(
            h.size(30, 90),
            [Effect::GroupSignal {
                group: 55,
                signal: 28
            }]
        );
    }

    #[test]
    fn sigwinch_goes_to_each_group_the_shell_started_and_never_to_the_job() {
        // The review of 10c, finding 3: a native element of the line (no
        // group) would be ended by the request.
        let mut h: Host<u32> = Host::new();
        h.start_line(1, 101, 11, true);
        h.connect(1, 11).unwrap();
        h.add_root(1, 12);
        h.add_root(1, 0);
        h.bring_front(1);
        assert_eq!(
            h.size(30, 90),
            [
                Effect::GroupSignal {
                    group: 11,
                    signal: 28
                },
                Effect::GroupSignal {
                    group: 12,
                    signal: 28
                }
            ]
        );
    }

    #[test]
    fn a_stopped_or_ended_line_leaves_the_front_of_the_discipline() {
        // INV-0331-2 after a stop (the `leave` survivor): a stopped line
        // continued in the background changes no mode.
        let (mut h, a, b) = two();
        assert_eq!(h.front(), Some(1));
        h.stop_front();
        assert_eq!(h.tty.front_line(), None);
        assert_eq!(set_raw(&mut h, a), status::IO);
        h.bring_front(2);
        assert_eq!(h.front(), Some(2));
        h.end_line(2);
        assert_eq!(h.tty.front_line(), None);
        assert_eq!(set_raw(&mut h, b), status::IO);
        // Ending line 2 kept line 1's record (the `retain` survivor).
        assert!(h.has_line(1));
        assert!(!h.has_line(2));
        h.bring_front(1);
        assert_eq!(h.front(), Some(1));
        h.keys(b"back\n");
        assert_eq!(read(&mut h, a, 3).unwrap(), b"back\n");
    }

    #[test]
    fn the_relay_adds_the_carriage_return_as_the_attributes_say() {
        // INV-0331-7, the relay.
        let (mut h, a, _) = two();
        let mut out = Vec::new();
        h.relay(1, b"a\nb\n", &mut Crlf::new(), &mut out);
        assert_eq!(out, b"a\r\nb\r\n");
        set_raw(&mut h, a);
        out.clear();
        h.relay(1, b"a\nb\n", &mut Crlf::new(), &mut out);
        assert_eq!(out, b"a\nb\n", "output processing off");
    }

    #[test]
    fn bracketed_paste_mode_is_read_from_the_output_across_chunks() {
        let mut b = Bracket::default();
        b.scan(b"x\x1b[?20");
        assert!(!b.on());
        b.scan(b"04h");
        assert!(b.on());
        b.scan(b"\x1b[?1;2004l");
        assert!(!b.on());
        b.scan(b"\x1b[?1;2004h\x1b[?25l");
        assert!(b.on(), "another mode's reset is not this one's");
        b.scan(b"\x1b[2004l\x1b[?2005l\x1b[?20041l");
        assert!(b.on(), "not private, another mode, a longer number");
        b.scan(b"\x1b[?2004;1l");
        assert!(!b.on(), "2004 first of several");
        b.scan(b"\x1b[?2004;1h");
        assert!(b.on());
        b.scan(b"\x1b[?1;25l");
        assert!(b.on(), "neither parameter is 2004");
        b.reset();
        assert!(!b.on());
    }

    /// What the program could read now, in raw mode, without a request.
    fn typed(h: &mut Host<u32>, _id: Id) -> Vec<u8> {
        h.tty.discipline.read_raw(100_000)
    }

    fn pasted(h: &mut Host<u32>, text: &str) -> Vec<E> {
        let mut a = Assembler::default();
        let clean = a.part(text.as_bytes(), true, true).unwrap();
        let mut out = Vec::new();
        assert!(h.paste(&clean, &mut out));
        out
    }

    #[test]
    fn a_paste_to_a_program_with_2004_is_bracketed_and_cannot_end_the_bracket() {
        // INV-0332-5, the program's half.
        let (mut h, a, _) = two();
        set_raw(&mut h, a);
        h.relay(1, b"\x1b[?2004h", &mut Crlf::new(), &mut Vec::new());
        let es = pasted(&mut h, "ls\nrm x\x1b[201~ \x03\x1a;echo\x07 end");
        assert!(!es.iter().any(|e| matches!(e, Effect::Question(_))));
        let got = typed(&mut h, a);
        assert_eq!(got, b"\x1b[200~ls\rrm x[201~ ;echo end\x1b[201~");
        assert_eq!(
            got.iter().filter(|&&b| b == 0x1b).count(),
            2,
            "the shell's two marks only"
        );
        assert!(
            !es.iter()
                .any(|e| matches!(e, Effect::Tty(Action::Signal { .. }))),
            "no signal character in it"
        );
    }

    #[test]
    fn a_paste_with_a_newline_to_a_program_without_2004_waits_for_enter() {
        // INV-0332-5: held, shown on the title, sent on Enter, dropped on
        // anything else; while it waits no other key does anything.
        let (mut h, a, _) = two();
        set_raw(&mut h, a);
        let es = pasted(&mut h, "one\ntwo\n");
        assert_eq!(
            es,
            [Effect::Question(Some(
                "paste 2 lines? Enter sends, any other key drops".into()
            ))]
        );
        assert!(typed(&mut h, a).is_empty(), "nothing sent yet");
        let es = h.keys(b"\r");
        assert!(es.contains(&Effect::Question(None)));
        assert_eq!(typed(&mut h, a), b"one\rtwo\r");
        // Escape drops it, and the keys after it are the program's again.
        let es = pasted(&mut h, "x\ny");
        assert_eq!(
            es,
            [Effect::Question(Some(
                "paste 2 lines? Enter sends, any other key drops".into()
            ))]
        );
        h.keys(b"\x1b");
        h.end_of_keys();
        assert!(typed(&mut h, a).is_empty());
        h.keys(b"k");
        assert_eq!(typed(&mut h, a), b"k");
        // An arrow key (CSI, then SS3) drops it whole, one byte at a time as
        // zosh gives them (the review of 10c, finding 9).
        for key in [&b"\x1b[A"[..], b"\x1b[1;5C", b"\x1bOA"] {
            pasted(&mut h, "x\ny");
            for b in key {
                h.keys(&[*b]);
            }
            h.end_of_keys();
            assert!(typed(&mut h, a).is_empty(), "{key:?}");
            h.keys(b"k");
            assert_eq!(typed(&mut h, a), b"k");
        }
        // A key while it waits is not typed.
        pasted(&mut h, "x\ny");
        h.keys(b"q");
        assert!(typed(&mut h, a).is_empty());
        // One line, no newline: no question.
        let es = pasted(&mut h, "plain");
        assert!(es.is_empty());
        assert_eq!(typed(&mut h, a), b"plain");
    }

    #[test]
    fn a_held_paste_goes_with_its_line() {
        let (mut h, a, _) = two();
        set_raw(&mut h, a);
        pasted(&mut h, "x\ny");
        let es = h.stop_front();
        assert!(es.contains(&Effect::Question(None)));
        h.bring_front(1);
        h.keys(b"\r");
        assert_eq!(
            typed(&mut h, a),
            b"\r",
            "the Enter is a key, not a confirmation"
        );
    }

    #[test]
    fn a_paste_with_no_program_in_front_is_not_the_programs() {
        let mut h: Host<u32> = Host::new();
        let mut out = Vec::new();
        assert!(!h.paste("x", &mut out));
        assert!(out.is_empty());
    }
    #[test]
    fn a_native_line_is_restored_but_never_told_a_size() {
        // ADR-0331 item 9 for every line in front; item 7 only for a line
        // with the terminal (a native program has no handler for it).
        let mut h: Host<u32> = Host::new();
        h.start_line(1, 101, 0, false);
        h.bring_front(1);
        assert!(h.size(30, 90).is_empty());
        let es = h.stop_front();
        assert!(es.contains(&Effect::Restore { keep: true }));
        let es = h.bring_front(1);
        assert!(es.contains(&Effect::Reinstate));
        assert!(
            !es.iter().any(|e| matches!(
                e,
                Effect::Tty(Action::Signal { .. }) | Effect::GroupSignal { .. }
            )),
            "no SIGWINCH to a native line"
        );
        assert_eq!(h.end_line(1), [Effect::Restore { keep: false }]);
    }

    #[test]
    fn every_program_of_the_line_in_front_reads_until_tcsetpgrp_chooses() {
        // `less` at the end of `ls | less` has a group of its own.
        let mut h: Host<u32> = Host::new();
        h.start_line(1, 101, 11, true);
        let first = h.connect(1, 11).unwrap();
        let last = h.connect(1, 12).unwrap();
        h.add_root(1, 12);
        h.bring_front(1);
        h.keys(b"q\n");
        assert_eq!(read(&mut h, last, 1).unwrap(), b"q\n");
        h.keys(b"r\n");
        assert_eq!(read(&mut h, first, 2).unwrap(), b"r\n");
        assert!(h.foreground_asked(12, 12).is_ok());
        assert_eq!(read(&mut h, first, 3), Err(status::NOT_FOREGROUND));
        // The choice outlives a stop.
        h.stop_front();
        h.bring_front(1);
        assert_eq!(h.tty.foreground(), Some(12));
    }

    #[test]
    fn a_paste_to_a_program_is_cleaned_as_at_the_prompt() {
        // The 10b review's finding 2: what cannot be seen does not reach a
        // program either (zero-width and bidirectional format characters),
        // through the shell's one paste path (`paste::Feed`).
        let (mut h, a, _) = two();
        set_raw(&mut h, a);
        h.relay(1, b"\x1b[?2004h", &mut Crlf::new(), &mut Vec::new());
        let mut feed = crate::paste::Feed::default();
        let mut keys = Vec::new();
        let text = "ls \u{200b}\u{202e}\u{2066}\u{feff}ok\x1b\x03";
        assert!(feed.part(text.as_bytes(), true, true, &mut keys));
        let crate::paste::Routed::Insert(clean) =
            feed.route(keys[0], crate::paste::Reading::Program)
        else {
            panic!("a paste for the program");
        };
        let mut out = Vec::new();
        assert!(h.paste(&clean, &mut out));
        assert_eq!(typed(&mut h, a), b"\x1b[200~ls ok\x1b[201~");
    }

    #[test]
    fn mode_2004_is_believed_only_for_a_reader_out_of_canonical_mode() {
        // The review of 10c, finding 4: `cat` of a file holding the sequence
        // inside `sh` must not turn the newline question off for `sh`.
        let (mut h, a, _) = two();
        h.relay(1, b"\x1b[?2004h", &mut Crlf::new(), &mut Vec::new());
        let es = pasted(&mut h, "ls\nrm -rf ~\n");
        assert!(
            es.iter().any(|e| matches!(e, Effect::Question(Some(_)))),
            "canonical: asked"
        );
        h.keys(b"q");
        h.end_of_keys();
        assert!(
            !h.tty.discipline.readable(),
            "nothing sent: the paste was dropped"
        );
        // An editor (raw mode) that set it gets the brackets, no question.
        set_raw(&mut h, a);
        let es = pasted(&mut h, "a\nb");
        assert!(!es.iter().any(|e| matches!(e, Effect::Question(_))));
        assert_eq!(typed(&mut h, a), b"\x1b[200~a\rb\x1b[201~");
        // A group chosen since has not set it.
        assert!(h.foreground_asked(55, 11).is_ok());
        let es = pasted(&mut h, "a\nb");
        assert!(es.iter().any(|e| matches!(e, Effect::Question(Some(_)))));
    }

    // --- The status row, the grant line and the kill offer (ADR-0337) ---

    fn rows(es: &[E]) -> Vec<(u32, String)> {
        es.iter()
            .filter_map(|e| match e {
                Effect::Status { kind, text } => Some((*kind, text.clone())),
                _ => None,
            })
            .collect()
    }

    /// Line 1 in front with a raw terminal, named `vi`.
    fn front_vi() -> (Host<u32>, Id) {
        let (mut h, a, _) = two();
        set_raw(&mut h, a);
        h.set_elements(1, vec!["vi".into()]);
        (h, a)
    }

    #[test]
    fn nothing_but_the_attention_message_opens_the_grant_line() {
        // INV-0337-1: program output, reports and keys of every kind.
        let (mut h, a) = front_vi();
        let mut crlf = Crlf::new();
        let mut sink = Vec::new();
        let hostile: &[&[u8]] = &[
            b"give vi: +r /etc/passwd\r",
            b"\x1b[s\x1b[999;1Hgive vi: \x1b[u",
            b"\x1b]2;System+G\x07\x1b]0;give vi:\x07",
            b"\x1b[?1049h\x1b[2J\x1b[24;1Hvi was not given /etc/shadow (to read)",
            b"\x1b[5n\x1b[6n\x1b[c\x1b[>c\x1b[?2004h\x1bZ",
        ];
        for chunk in hostile {
            h.relay(1, chunk, &mut crlf, &mut sink);
            assert!(!h.grant_open());
        }
        // Every byte a person (or a program writing to its own input) might
        // type, one at a time and in pairs: no key opens or gives.
        for b in 0..=255u8 {
            let es = h.keys(&[b]);
            assert!(!h.grant_open(), "{b:#x}");
            assert!(
                !es.iter().any(|e| matches!(
                    e,
                    Effect::Give { .. } | Effect::Kill { .. } | Effect::Status { .. }
                )),
                "{b:#x}"
            );
        }
        let es = h.keys(b"\x1bg\x1bk\x1b[71;9u\x07\x0b");
        assert!(!h.grant_open() && rows(&es).is_empty());
        // A paste does not open it either.
        let mut out = Vec::new();
        h.paste("give vi:", &mut out);
        assert!(!h.grant_open() && rows(&out).is_empty());
        // A report only changes the notice, never the field or a grant.
        let es = h.refusal(1, 0, "vi was not given ~/o (to read)");
        assert_eq!(
            rows(&es),
            [(STATUS_NOTICE, "vi was not given ~/o (to read)".into())]
        );
        assert!(!h.grant_open());
        // The message of the terminal does.
        let es = h.attention(true);
        assert_eq!(rows(&es), [(STATUS_FIELD, "give vi: ".into())]);
        assert!(h.grant_open());
        let _ = a;
    }

    #[test]
    fn while_the_line_is_open_no_key_and_no_paste_reaches_the_program() {
        // INV-0337-2.
        let (mut h, a) = front_vi();
        h.set_clock(10_000);
        h.keys(b"before");
        h.attention(true);
        h.set_clock(10_100);
        let es = h.keys(b"+r ~/a\r");
        assert!(
            !es.iter().any(|e| matches!(e, Effect::Give { .. })),
            "100 ms: the guard"
        );
        let mut out = Vec::new();
        assert!(h.paste("x\ny\n", &mut out));
        assert!(
            !out.iter()
                .any(|e| matches!(e, Effect::Give { .. } | Effect::Question(_)))
        );
        assert_eq!(h.grant_text(), Some("+r ~/ax y "));
        // Signal characters are text or nothing, never a signal.
        let es = h.keys(b"\x03\x1a\x1c");
        assert!(
            !es.iter()
                .any(|e| matches!(e, Effect::Tty(_) | Effect::GroupSignal { .. }))
        );
        assert_eq!(
            typed(&mut h, a),
            b"before",
            "only what came before the line"
        );
        // The guard over, Enter gives the typed text for element 0.
        h.set_clock(10_600);
        let es = h.keys(b"\r");
        assert!(es.contains(&Effect::Give {
            line: 1,
            element: 0,
            text: "+r ~/ax y ".into()
        }));
        // The answer closes the field and says what was given.
        let es = h.gave(Ok("+r ~/a"));
        assert_eq!(
            rows(&es),
            [(STATUS_PLAIN, "given until vi ends: +r ~/a".into())]
        );
        assert!(!h.grant_open());
        // After it the keys are the program's again.
        h.keys(b"after");
        assert_eq!(typed(&mut h, a), b"after");
        // Nothing of it ever reached the program.
        h.attention(true);
        h.set_clock(20_000);
        h.keys(b"+r /x\x1b");
        assert!(!h.grant_open(), "a lone Escape closes");
        assert!(typed(&mut h, a).is_empty());
    }

    #[test]
    fn the_key_again_closes_and_the_other_key_changes_the_field() {
        let (mut h, _) = front_vi();
        h.attention(true);
        let es = h.attention(true);
        assert_eq!(rows(&es), [(STATUS_PLAIN, String::new())]);
        assert!(!h.grant_open());
        h.attention(true);
        let es = h.attention(false);
        assert_eq!(
            rows(&es),
            [
                (STATUS_PLAIN, String::new()),
                (STATUS_FIELD, "kill vi? Enter kills, Escape keeps".into())
            ]
        );
    }

    #[test]
    fn with_no_line_in_front_on_the_console_or_in_an_untrusted_room_the_key_gives_nothing() {
        // INV-0337-7 and ADR-0337 items 5 and 10.
        let mut h: Host<u32> = Host::new();
        assert!(h.attention(true).is_empty() && !h.grant_open());
        let (mut h, _) = front_vi();
        h.set_scope(Scope::Console);
        assert!(h.attention(true).is_empty() && h.attention(false).is_empty());
        assert!(
            h.refusal(1, 0, "x").is_empty(),
            "no status row on the console"
        );
        assert!(!h.grant_open());
        h.set_scope(Scope::Untrusted);
        let es = h.attention(true);
        assert_eq!(
            rows(&es),
            [(STATUS_PLAIN, "not for an untrusted room".into())]
        );
        assert!(!h.grant_open());
        // The kill offer is not a grant: it is there in every room.
        assert!(h.attention(false).iter().any(|e| {
            rows(core::slice::from_ref(e))
                .iter()
                .any(|(k, _)| *k == STATUS_FIELD)
        }));
    }

    #[test]
    fn a_refusal_of_a_line_not_in_front_is_not_shown() {
        let (mut h, _) = front_vi();
        assert!(h.refusal(2, 0, "vi2 was not given /x (to read)").is_empty());
        assert!(h.attention(true).iter().any(|e| matches!(
            e,
            Effect::Status { text, .. } if text == "give vi: "
        )));
    }

    #[test]
    fn the_grant_goes_to_the_element_the_row_named_and_the_field_does_not_move() {
        // ADR-0337 item 5, INV-0337-4: a report written by the other element
        // while the line is open changes the notice, not the element.
        let (mut h, _) = front_vi();
        h.set_elements(1, vec!["cat".into(), "vi".into(), "wc".into()]);
        h.refusal(1, 1, "vi was not given ~/o (to read)");
        h.attention(true);
        h.refusal(1, 2, "wc was not given ~/p (to read)");
        h.set_clock(1_000);
        h.keys(b"+r ~/o");
        let es = h.keys(b"\r");
        assert!(es.contains(&Effect::Give {
            line: 1,
            element: 1,
            text: "+r ~/o".into()
        }));
    }

    #[test]
    fn the_kill_offer_kills_the_line_on_enter_after_the_guard() {
        // ADR-0387 item 4, ADR-0158 rule 3.
        let (mut h, a) = front_vi();
        h.set_clock(5_000);
        let es = h.attention(false);
        assert_eq!(
            rows(&es),
            [(STATUS_FIELD, "kill vi? Enter kills, Escape keeps".into())]
        );
        h.set_clock(5_200);
        assert!(
            !h.keys(b"\r")
                .iter()
                .any(|e| matches!(e, Effect::Kill { .. }))
        );
        h.set_clock(5_600);
        let es = h.keys(b"\r");
        assert!(es.contains(&Effect::Kill { line: 1 }));
        assert!(!h.grant_open());
        assert!(typed(&mut h, a).is_empty());
        // Escape keeps.
        h.attention(false);
        let es = h.keys(b"\x1b");
        assert!(!es.iter().any(|e| matches!(e, Effect::Kill { .. })));
        assert!(!h.grant_open());
    }

    #[test]
    fn the_row_and_the_field_go_with_the_line() {
        // INV-0337-4: nothing is kept past the line.
        let (mut h, _) = front_vi();
        h.refusal(1, 0, "vi was not given ~/o (to read)");
        h.attention(true);
        let es = h.end_line(1);
        assert_eq!(rows(&es), [(STATUS_PLAIN, String::new())]);
        assert!(!h.grant_open());
        // The next line starts with a clear row and no remembered element.
        h.set_elements(2, vec!["less".into()]);
        let es = h.bring_front(2);
        assert!(rows(&es).is_empty());
        let es = h.attention(true);
        assert_eq!(rows(&es), [(STATUS_FIELD, "give less: ".into())]);
        // A stop clears too.
        let es = h.stop_front();
        assert_eq!(rows(&es), [(STATUS_PLAIN, String::new())]);
    }

    #[test]
    fn a_refused_give_closes_the_field_and_says_why() {
        let (mut h, _) = front_vi();
        h.attention(true);
        h.set_clock(1_000);
        h.keys(b"+r /etc");
        h.keys(b"\r");
        let es = h.gave(Err("/etc: this shell does not hold it"));
        assert_eq!(
            rows(&es),
            [(STATUS_PLAIN, "/etc: this shell does not hold it".into())]
        );
        assert!(!h.grant_open());
        // An answer with no field open does nothing.
        assert!(h.gave(Ok("+r /x")).is_empty());
    }
}
