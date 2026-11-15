//! What a POSIX program of a line receives of the terminal (ADR-0331 items 2,
//! 3 and 15, ADR-0333 items 1, 4 and 5): which of its fds 0 to 2 are the
//! terminal, the `keys` line of `grants` that follows, and its environment.
//! Pure, so that the shell and `grants` use one rule and the host tests hold
//! it (INV-0331-1, INV-0333-2, INV-0333-3).

use alloc::string::String;
use alloc::vec::Vec;

use crate::syntax::Stream;

/// The `TERM` of a program of a window (ADR-0333 item 1): the pinned entry.
pub const WINDOW_TERM: &str = "xterm-256color";

/// What `grants` prints on its `keys` line (ADR-0331 item 3).
pub const KEYS_WORDS: &str = "the terminal, while this line is in front";

/// Which of fds 0, 1 and 2 of element `i` of `n` are the terminal (ADR-0331
/// item 2): the first one's input unless the line pipes or redirects it, or
/// runs in the background; the last one's output unless the line sends it
/// elsewhere; its error stream unless it is piped (`|&`, `err_too`, from a
/// non-last element), sent elsewhere, or redirected. `redirects` are the
/// element's own `<`, `>`, `2>` and `&>`, each of which takes its fd off the
/// terminal.
#[allow(clippy::too_many_arguments)] // one rule, each argument a fact of the line
pub fn terminal_fds(
    i: usize,
    n: usize,
    input_free: bool,
    background: bool,
    out_terminal: bool,
    err_terminal: bool,
    err_too: bool,
    redirects: &[Stream],
) -> [bool; 3] {
    let mut tty = [
        i == 0 && input_free && !background,
        i + 1 == n && out_terminal,
        !(err_too && i + 1 < n) && err_terminal,
    ];
    for s in redirects {
        match s {
            Stream::In => tty[0] = false,
            Stream::Out => tty[1] = false,
            Stream::Err => tty[2] = false,
            Stream::Both => {
                tty[1] = false;
                tty[2] = false;
            }
        }
    }
    tty
}

/// The words of `grants`' first row for a POSIX program: "the terminal" when
/// all three are, else each fd's own.
pub fn streams_words(tty: [bool; 3]) -> String {
    if tty == [true; 3] {
        return String::from("the terminal");
    }
    let w = |t: bool| if t { "the terminal" } else { "elsewhere" };
    alloc::format!("in: {}, out: {}, err: {}", w(tty[0]), w(tty[1]), w(tty[2]))
}

/// The environment of a POSIX program (ADR-0153 rule 10, ADR-0333): what the
/// line gave, then `PATH` and `HOME` unless it gave them, then, for a program
/// with the terminal on at least one of fds 0 to 2, `TERM` unless it gave it:
/// the pinned entry in a window, `console_term` (the setting `console.term`,
/// `dumb` when empty) on the serial console. Never `TERMINFO`,
/// `TERMINFO_DIRS`, `LINES`, `COLUMNS` or `COLORTERM`: a program of the
/// line's own giving still gets them, the shell never adds them.
pub fn posix_env(
    line: &[String],
    home: &str,
    has_terminal: bool,
    window: bool,
    console_term: &str,
) -> Vec<String> {
    let mut env: Vec<String> = line.to_vec();
    let path = String::from("PATH=/bin:/usr/bin");
    let home = alloc::format!("HOME={home}");
    for (name, value) in [("PATH=", &path), ("HOME=", &home)] {
        if !env.iter().any(|v| v.starts_with(name)) {
            env.push(value.clone());
        }
    }
    if has_terminal && !env.iter().any(|v| v.starts_with("TERM=")) {
        let term = if window {
            WINDOW_TERM
        } else if console_term.is_empty() {
            "dumb"
        } else {
            console_term
        };
        env.push(alloc::format!("TERM={term}"));
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    fn fds(redirects: &[Stream]) -> [bool; 3] {
        terminal_fds(0, 1, true, false, true, true, false, redirects)
    }

    #[test]
    fn a_lone_program_has_the_terminal_on_each_fd_the_line_leaves_alone() {
        // INV-0331-1: every combination of the three redirections.
        assert_eq!(fds(&[]), [true, true, true]);
        assert_eq!(fds(&[Stream::In]), [false, true, true]);
        assert_eq!(fds(&[Stream::Out]), [true, false, true]);
        assert_eq!(fds(&[Stream::Err]), [true, true, false]);
        assert_eq!(fds(&[Stream::Both]), [true, false, false]);
        assert_eq!(fds(&[Stream::In, Stream::Out]), [false, false, true]);
        assert_eq!(fds(&[Stream::In, Stream::Both]), [false, false, false]);
        assert_eq!(
            fds(&[Stream::In, Stream::Out, Stream::Err]),
            [false, false, false]
        );
    }

    #[test]
    fn a_pipe_and_the_background_take_fds_off_the_terminal() {
        // `echo x | sh`: the second element's input is the pipe.
        assert_eq!(
            terminal_fds(1, 2, false, false, true, true, false, &[]),
            [false, true, true]
        );
        // `sh | cat`: the first one's output is the pipe, its input is not.
        assert_eq!(
            terminal_fds(0, 2, true, false, true, true, false, &[]),
            [true, false, true]
        );
        // `sh |& cat`: its error stream goes into the pipe too.
        assert_eq!(
            terminal_fds(0, 2, true, false, true, true, true, &[]),
            [true, false, false]
        );
        // `a |& sh`: the last element's error stream is not piped by `|&`
        // (that is the one before it), so it is the terminal.
        assert_eq!(
            terminal_fds(1, 2, false, false, true, true, true, &[]),
            [false, true, true]
        );
        // A middle element has neither end.
        assert_eq!(
            terminal_fds(1, 3, false, false, true, true, false, &[]),
            [false, false, true]
        );
        // `sh &`: no keys; its output and error are still shown.
        assert_eq!(
            terminal_fds(0, 1, true, true, true, true, false, &[]),
            [false, true, true]
        );
        // The line's output and error sent to a file or a capture.
        assert_eq!(
            terminal_fds(0, 1, true, false, false, false, false, &[]),
            [true, false, false]
        );
    }

    #[test]
    fn the_words_say_which_fds_are_the_terminal() {
        assert_eq!(streams_words([true; 3]), "the terminal");
        assert_eq!(
            streams_words([false, true, false]),
            "in: elsewhere, out: the terminal, err: elsewhere"
        );
    }

    fn names(env: &[String]) -> Vec<&str> {
        env.iter()
            .map(|v| v.split('=').next().unwrap_or(""))
            .collect()
    }

    #[test]
    fn term_is_set_exactly_for_a_program_with_a_terminal() {
        // INV-0333-2: with and without a terminal, window and console.
        let with = posix_env(&[], "/home", true, true, "dumb");
        assert!(with.contains(&"TERM=xterm-256color".to_string()));
        let without = posix_env(&[], "/home", false, true, "dumb");
        assert!(!names(&without).contains(&"TERM"));
        assert_eq!(names(&without), ["PATH", "HOME"]);
        // On the serial console it is the setting; empty is `dumb`.
        let console = posix_env(&[], "/home", true, false, "vt100");
        assert!(console.contains(&"TERM=vt100".to_string()));
        let default = posix_env(&[], "/home", true, false, "");
        assert!(default.contains(&"TERM=dumb".to_string()));
        // A TERM the line gives is kept, once.
        let own = posix_env(&["TERM=vt100".to_string()], "/home", true, true, "dumb");
        assert_eq!(own.iter().filter(|v| v.starts_with("TERM=")).count(), 1);
        assert!(own.contains(&"TERM=vt100".to_string()));
    }

    #[test]
    fn the_shell_never_adds_the_variables_adr_0333_item_4_names() {
        // INV-0333-3.
        for (terminal, window) in [(true, true), (true, false), (false, true)] {
            let env = posix_env(&[], "/home", terminal, window, "vt100");
            for banned in ["TERMINFO", "TERMINFO_DIRS", "LINES", "COLUMNS", "COLORTERM"] {
                assert!(!names(&env).contains(&banned), "{banned}");
            }
        }
        // What the line gave is the line's, and the defaults come after it.
        let env = posix_env(
            &["A=1".to_string(), "PATH=/x".to_string()],
            "/h",
            false,
            true,
            "",
        );
        assert_eq!(env, ["A=1", "PATH=/x", "HOME=/h"]);
    }
}
