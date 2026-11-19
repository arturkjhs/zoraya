//! What the shell does for an installed program (ADR-0396, ADR-0397,
//! ADR-0399): the order a name is looked up in, the check of a launch, the
//! refusal of a `+w` over `/apps`, and the saved words as plan entries. Pure,
//! so each rule is tested on the host; `zosh` supplies what the shell holds.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_settings::apps::{self, Apps, Word, expand};
use zoraya_settings::sha256::Digest;

use crate::eval::Value;
use crate::grants::{Access, Own, Planned, Refused, USER_HOME};
use crate::netgrant::{self, SavedNet};

/// Where a command name is found (ADR-0396 item 6): builtins and functions
/// are the evaluator's, before the host; then the image, then the volume's
/// `shell.path`, then the installed names. An installed name never shadows
/// anything (INV-0396-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Image,
    Path,
    Installed,
    Nothing,
}

/// The first place that has the name, in ADR-0396 item 6's order.
pub fn pick(in_image: bool, on_path: bool, installed: bool) -> Source {
    if in_image {
        Source::Image
    } else if on_path {
        Source::Path
    } else if installed {
        Source::Installed
    } else {
        Source::Nothing
    }
}

/// What a launch of an installed name comes to (ADR-0396 item 8, ADR-0397
/// item 4, INV-0396-4, INV-0397-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch<'a> {
    /// The loaded bytes are what was installed: run it, with these words.
    Run(&'a [Word]),
    /// Not what was installed: `zosh: NAME: not what was installed (install
    /// again)`; no word applies.
    Changed,
    /// No record for the name.
    Unknown,
}

/// `loaded` is [`zoraya_settings::sha256::identity`] of the bytes the loader
/// received and of the description file read for this launch.
pub fn launch<'a>(record: &'a Apps, name: &str, loaded: &Digest) -> Launch<'a> {
    match record.get(name) {
        None => Launch::Unknown,
        // A constant-time compare is not needed: nothing here is secret.
        Some(a) if a.hash == *loaded => Launch::Run(&a.words),
        Some(_) => Launch::Changed,
    }
}

/// INV-0396-1: no line gives a program a writable connection over `/apps`, `/`
/// or anything inside a package. Only a typed `+w` and a room's are refused, and it is the
/// one case where the shell refuses a typed word: installed programs are
/// changed with `install`. `plan` is the line's plan; its `+w` entries have
/// the reason `+w` (possibly with a link's note after it).
pub fn refuse_apps_write(plan: &[Planned]) -> Result<(), Refused> {
    for e in plan {
        if !(e.reason.starts_with("+w") || e.reason.starts_with("room "))
            || e.access != Access::Tree
        {
            continue;
        }
        let path = e.path.trim_end_matches('/');
        let inside = path == apps::APPS_DIR
            || path
                .strip_prefix(apps::APPS_DIR)
                .is_some_and(|r| r.starts_with('/'));
        // `/` holds `/apps` below it.
        let above = path.is_empty();
        if inside || above {
            let shown = if above { "/" } else { path };
            return Err(Refused {
                subject: format!("+w {shown}"),
                problem: String::from("installed programs are changed with install"),
                hint: Some(if above {
                    String::from("+w / would reach /apps too: name the directory you mean")
                } else {
                    String::from(
                        "install PACKAGE replaces a program; programs remove NAME removes one",
                    )
                }),
            });
        }
    }
    Ok(())
}

/// Why `install` or `programs` is refused here (ADR-0396 item 3, INV-0396-5):
/// an untrusted room, or a shell that holds `/apps` or `/config` without the
/// right to change it. `None`: allowed. A program cannot reach a builtin: only
/// the shell's own terminal line runs one.
pub fn gate(untrusted_room: bool, holds_apps: bool, holds_config: bool) -> Option<&'static str> {
    if untrusted_room {
        Some("not in an untrusted room")
    } else if !holds_apps || !holds_config {
        Some("this shell does not hold /apps and /config (open a terminal that does)")
    } else {
        None
    }
}

/// The words of an `install` or `programs` line that may be saved: each one
/// typed literally on the line (ADR-0397 item 2, INV-0397-1). The value of a
/// variable, a substitution or a pipe is data, never a word to save.
pub fn literal_tokens(values: &[Value]) -> Result<Vec<String>, String> {
    if let Some(v) = values.iter().find(|v| !v.literal) {
        return Err(format!(
            "{}: from a variable, a substitution or a pipe: a saved word is typed on the line",
            v.text
        ));
    }
    Ok(values.iter().map(|v| v.text.clone()).collect())
}

/// What the shell holds and where it is, for [`apply`].
pub struct Held<'a> {
    pub home: &'a str,
    /// `None` outside a room; else the room's name.
    pub room: Option<&'a str>,
    pub untrusted: bool,
    /// The room's `+net` words (a trusted room's bound), when in a room.
    pub room_nets: Option<&'a [String]>,
    /// Whether the shell holds `abs`, to change when `write`.
    pub path: &'a dyn Fn(&str, bool) -> bool,
    /// Whether the shell has a network connection to narrow from.
    pub network: bool,
}

/// What a record's saved words come to for one launch (ADR-0397 item 5).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Applied {
    /// `+r` and `+w` words that are held, with the reason `saved`.
    pub planned: Vec<Planned>,
    /// `+own`: the program's own directory.
    pub own: Option<Own>,
    /// The network words that hold, as the scope `narrow` takes.
    pub net: SavedNet,
    /// The lines for the person, each ending in a newline and shown before
    /// the program starts.
    pub notes: Vec<String>,
}

/// Adds the record's words to a line as if the person had typed them before
/// the command, never more than the shell holds: a word it does not hold is
/// dropped with `zosh: ed: saved +w ~/notes is not held here`; in an untrusted
/// room no word applies at all (INV-0397-2, INV-0397-5).
pub fn apply(name: &str, words: &[Word], held: &Held<'_>) -> Applied {
    let mut out = Applied::default();
    if words.is_empty() {
        return out;
    }
    if held.untrusted {
        out.notes.push(format!(
            "zosh: {name}: saved grants are not applied in an untrusted room\n"
        ));
        return out;
    }
    let not_held = |w: &Word| format!("zosh: {name}: saved {} is not held here\n", w.words());
    let mut nets: Vec<String> = Vec::new();
    let mut web = false;
    let mut net_words: Vec<&Word> = Vec::new();
    for w in words {
        match w {
            Word::Read(p) | Word::Write(p) => {
                let write = matches!(w, Word::Write(_));
                let abs = expand(p, held.home);
                // The writers refuse these places; a hand-edited record is
                // refused here too, so the record is in no table (INV-0397-2).
                if let Some(what) = apps::refused_place(&abs) {
                    out.notes.push(format!(
                        "zosh: {name}: saved {} is not applied: {what} cannot be given\n",
                        w.words()
                    ));
                } else if (held.path)(&abs, write) {
                    out.planned.push(Planned {
                        path: abs,
                        access: if write { Access::Tree } else { Access::Read },
                        reason: String::from("saved"),
                    });
                } else {
                    out.notes.push(not_held(w));
                }
            }
            Word::Own(p) => {
                let room = held.room.unwrap_or("home");
                let real = format!("/data/{room}/{name}");
                match p.strip_prefix("~/") {
                    Some(rest) if (held.path)(&real, true) && out.own.is_none() => {
                        out.own = Some(Own {
                            name: name.into(),
                            real,
                            view: format!("{USER_HOME}/{rest}"),
                            read: false,
                        });
                    }
                    _ => out.notes.push(not_held(w)),
                }
            }
            Word::Net(t) => {
                nets.push(t.clone());
                net_words.push(w);
            }
            Word::Web => {
                web = true;
                net_words.push(w);
            }
        }
    }
    if !net_words.is_empty() {
        if !held.network {
            for w in net_words {
                out.notes.push(not_held(w));
            }
        } else {
            out.net = netgrant::saved_scope(&nets, web, held.room_nets);
            for d in &out.net.dropped {
                out.notes
                    .push(format!("zosh: {name}: saved {d} is not held here\n"));
            }
        }
    }
    out
}
