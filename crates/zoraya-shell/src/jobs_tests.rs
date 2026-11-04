//! Host tests of the job table and its notices (ADR-0158).

use alloc::string::String;
use alloc::vec::Vec;

use zoraya_abi::{ExitReason, ExitRecord, RequestKind};

use crate::editor::{Assist, Completion, Editor, Options};
use crate::editor_tests::{screen_lines, without_sgr};
use crate::jobs::{At, Exit, Jobs, State, notice};
use crate::record::exited;

struct NoAssist;

impl Assist for NoAssist {
    fn known(&mut self, _: &str) -> bool {
        true
    }
    fn incomplete(&mut self, _: &str) -> bool {
        false
    }
    fn complete(&mut self, _: &str, _: usize) -> Completion {
        Completion::default()
    }
}

fn terminated() -> ExitRecord {
    ExitRecord {
        code: 0,
        reason: ExitReason::Terminated,
        detail: 0,
        request_kind: Some(RequestKind::Terminate),
    }
}

/// Rule 1: the smallest free number from 1; rule 5: `%N`, the newest by
/// default, `jobs`' listing with the state and what was given.
#[test]
fn numbers_words_and_the_listing() {
    let mut j: Jobs<()> = Jobs::default();
    assert_eq!(j.which(None), Err("no jobs".into()));
    assert_eq!(
        j.add("make -C big &", "~/big: change anything in".into(), ()),
        1
    );
    assert_eq!(j.add("spin", String::new(), ()), 2);
    assert_eq!(j.add("sleep 9 &", String::new(), ()), 3);
    assert!(j.remove(2).is_some());
    assert_eq!(j.add("again", String::new(), ()), 2, "the smallest free");
    assert_eq!(j.which(None), Ok(3), "the newest");
    assert_eq!(j.which(Some("%1")), Ok(1));
    assert_eq!(j.which(Some("%7")), Err("%7: no such job".into()));
    for bad in ["1", "%", "%0", "%-1", "%x", "%99999999999"] {
        assert_eq!(
            j.which(Some(bad)),
            Err(alloc::format!("{bad}: not a job; write %N")),
            "{bad}"
        );
    }
    j.get_mut(3).unwrap().state = State::Stopped;
    assert_eq!(j.newest_stopped(), Some(3));
    assert_eq!(
        j.list(),
        "[1]  running   make -C big   (~/big: change anything in)\n\
         [3]  stopped   sleep 9\n\
         [2]  running   again\n"
    );
    j.get_mut(3).unwrap().state = State::Running;
    assert_eq!(j.newest_stopped(), None);
}

/// INV-0158-3's table half: `exit` with live jobs only warns the first
/// time; a second `exit` in a row leaves; anything between counts again.
#[test]
fn exit_warns_once_then_leaves() {
    let mut j: Jobs<()> = Jobs::default();
    assert_eq!(j.exit(), Exit::Leave, "no jobs: leave at once");
    j.add("sleep 60 &", String::new(), ());
    assert_eq!(
        j.exit(),
        Exit::Warn("1 job is still running; exit again to end it\n".into())
    );
    assert_eq!(j.len(), 1, "the warning ends nothing");
    j.other_line();
    assert!(matches!(j.exit(), Exit::Warn(_)), "counted again");
    assert_eq!(j.exit(), Exit::Leave);
    let mut j: Jobs<()> = Jobs::default();
    j.add("a &", String::new(), ());
    j.add("b", String::new(), ());
    j.get_mut(2).unwrap().state = State::Stopped;
    assert_eq!(
        j.exit(),
        Exit::Warn("2 jobs are still running; exit again to end them\n".into()),
        "a stopped job counts"
    );
}

/// INV-0158-4: every job's end is reported exactly once, by `kill`, by
/// `wait` or before a prompt, and never inside a line being edited. Job
/// ends arrive at each point: during `kill` and `wait` (shown there),
/// while a line is edited (shown above it, the line drawn again whole),
/// and while a line runs (shown before the next prompt).
#[test]
fn every_end_is_reported_once_and_never_inside_an_edited_line() {
    let mut j: Jobs<u32> = Jobs::default();
    for (i, line) in ["one &", "two &", "three &", "four &", "five &"]
        .iter()
        .enumerate()
    {
        assert_eq!(j.add(line, String::new(), i as u32), i as u32 + 1);
    }
    let mut screen = String::new();
    let mut e = Editor::new(Options::default());
    let type_line = |e: &mut Editor, screen: &mut String, text: &[u8]| {
        for k in text {
            e.key(*k, screen, &mut NoAssist);
        }
    };
    // A prompt, nothing queued; `kill %1` runs, and the end shows at once.
    screen.push_str(&j.take_notices());
    e.start("~> ", &mut screen);
    type_line(&mut e, &mut screen, b"kill %1\r");
    let (entry, now) = j.ended(1, &terminated(), At::Reporting).unwrap();
    assert_eq!(entry.payload, 0);
    screen.push_str(&now.expect("kill reports it"));
    // `wait`: jobs 2 and 3 end, each shown as it comes.
    e.start("~> ", &mut screen);
    type_line(&mut e, &mut screen, b"wait\r");
    for n in [2, 3] {
        let (_, now) = j.ended(n, &exited(0), At::Reporting).unwrap();
        screen.push_str(&now.unwrap());
    }
    // A new prompt, and a line being edited when job 4 ends: queued, then
    // shown above the line, which is drawn again whole.
    screen.push_str(&j.take_notices());
    e.start("~> ", &mut screen);
    type_line(&mut e, &mut screen, b"echo half");
    let (_, now) = j.ended(4, &exited(2), At::Elsewhere).unwrap();
    assert_eq!(now, None, "never in the middle of the line");
    assert!(j.has_notices());
    let queued = j.take_notices();
    e.above(&queued, &mut screen);
    assert!(!j.has_notices());
    // The line goes on and runs; job 5 ends while it runs.
    type_line(&mut e, &mut screen, b"way\r");
    let (_, now) = j.ended(5, &exited(0), At::Elsewhere).unwrap();
    assert_eq!(now, None);
    // Shown before the next prompt.
    screen.push_str(&j.take_notices());
    e.start("~> ", &mut screen);
    // A second end of one job is no second notice.
    assert!(j.ended(5, &exited(0), At::Reporting).is_none());
    assert!(j.ended(4, &exited(0), At::Elsewhere).is_none());
    assert_eq!(j.take_notices(), "");

    let lines = screen_lines(&without_sgr(&screen));
    let shown: Vec<&str> = lines.iter().map(|l| l.trim_end()).collect();
    assert_eq!(
        shown,
        [
            "~> kill %1",
            "[1] one: terminated",
            "~> wait",
            "[2] done: two",
            "[3] done: three",
            "[4] four: exit status 2",
            "~> echo halfway",
            "[5] done: five",
            "~>",
        ],
        "each end once, on rows of its own, the edited line whole"
    );
}

/// Rule 6's words.
#[test]
fn notices_say_how_a_job_ended() {
    assert_eq!(notice(3, "make", &exited(0)), "[3] done: make\n");
    assert_eq!(notice(2, "spin", &terminated()), "[2] spin: terminated\n");
    assert_eq!(
        notice(1, "false-ish", &exited(4)),
        "[1] false-ish: exit status 4\n"
    );
}
