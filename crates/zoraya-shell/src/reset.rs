//! A stop-gap until part 10c (review of 10a, finding 1): what the shell writes
//! to the terminal before a prompt that follows a foreground command.
//!
//! Until the shell hosts the line discipline and sends `Restore` (ADR-0331
//! item 9), whatever a program or a file leaves set in the terminal stays set:
//! the alternate screen, a hidden cursor, a scroll region, insert mode, the
//! line-drawing set, synchronized output, a colour. This string puts the
//! terminal back in-band, without erasing the primary screen. **Remove it when
//! 10c sends `Restore`** (`docs/design.md`, `docs/backlog.md`).

/// Leave the alternate screen (and restore the cursor 1049 saved), soft reset
/// (modes, region, sets, pen, saved cursors, the cursor shape), show the cursor,
/// ASCII in G0, shift in, default attributes, and reset the `OSC 4` palette (a
/// file that sets entries to the background colour would leave coloured output
/// invisible; the price is that a palette set from the shell does not last).
///
/// Its edges, until `Restore` replaces it: it comes before every prompt after the
/// first (an empty line, a builtin and a Ctrl-C included; it is idempotent and
/// keeps the screen); it clears a pending wrap (output that fills the last
/// column without a line feed no longer wraps the prompt); it undoes a cursor
/// shape a person set at the prompt; a job stopped with Ctrl-Z loses its modes
/// and `fg` does not give them back (INV-0331-4's second half, `reinstate`, is
/// 10c's); output from a background job while the editor is active is not
/// followed by a reset.
pub const BEFORE_PROMPT: &str = "\x1b[?1049l\x1b[!p\x1b[?25h\x1b(B\x0f\x1b[m\x1b]104\x07";

/// What goes before the prompt: nothing before the first of a shell, and the
/// reset after a command, when there is a terminal window (the console's
/// reader is the person's own terminal program, which is not ours to reset).
pub fn before_prompt(after_command: bool, window: bool) -> &'static str {
    if after_command && window {
        BEFORE_PROMPT
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zoraya_term::{Cell, Colour, Modes, Term};

    /// Whatever a program leaves set, the next prompt is typed and shown
    /// normally: the primary screen with its text, the cursor shown, the modes
    /// and sets and region at their defaults, the colour plain.
    #[test]
    fn the_prompt_after_a_hostile_program_is_normal() {
        let hostile = "\x1b[?1049h\x1b[?25l\x1b[8m\x1b(0\x0e\x1b[?2026h\x1b[2;3r\x1b[4h\x1b[?7l\x1b[?1h\x1b=\
                       \x1b[?2004h\x1b[?1000h\x1b[?5h\x1b[?6h\x1b[31;44;1m\x1b]4;3;#112233\x07\x1b[5 q";
        let mut t = Term::new(20, 5);
        t.write(b"earlier text\r\n");
        t.write(hostile.as_bytes());
        t.write(b"junk");
        t.write(before_prompt(true, true).as_bytes());
        t.write(b"~> ls");
        assert!(!t.on_alternate_screen());
        assert!(t.modes().cursor_visible);
        let mut want = Modes::DEFAULT;
        // What a soft reset leaves is only what the shell never sets.
        want.app_keypad = false;
        assert_eq!(*t.modes(), want);
        assert_eq!(t.scroll_region(), (0, 4));
        assert_eq!(t.line(0), "earlier text", "the primary screen is kept");
        let (x, y) = t.cursor();
        assert_eq!(
            t.line(y)
                .trim_start_matches(' ')
                .chars()
                .rev()
                .take(5)
                .collect::<String>(),
            "sl >~"
        );
        assert!(x >= 5);
        // The prompt's own cells are plain ASCII with default attributes.
        let at = t.line(y).find("~> ls").unwrap();
        for (i, ch) in "~> ls".chars().enumerate() {
            let c = t.cell(at + i, y);
            assert_eq!(
                (c.ch, c.fg(), c.bg(), c.attrs()),
                (ch, Colour::Default, Colour::Default, 0)
            );
        }
        assert_eq!(t.cell(19, 4), Cell::BLANK);
        assert_eq!(
            t.palette_override(3),
            None,
            "a palette set by a file does not outlive the command"
        );
    }

    #[test]
    fn nothing_before_the_first_prompt_or_on_the_console() {
        assert_eq!(before_prompt(false, true), "");
        assert_eq!(before_prompt(true, false), "");
        assert_eq!(before_prompt(true, true), BEFORE_PROMPT);
    }
}
