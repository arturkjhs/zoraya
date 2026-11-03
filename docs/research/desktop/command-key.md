# Does the Mac's Command key reach the guest? (ADR-0160)

Measured on 2026-11-03 on the architect's Mac (Apple Silicon, HVF), QEMU 11.1.2 from Homebrew, the stage 5 kernel
built from this branch with one throwaway change not committed: the US keymap mapped `KEY_LEFTMETA` (125) to `@`
and `KEY_RIGHTMETA` (126) to `#`, so that a Meta key reaching `compd` shows up as a character in the terminal.
QEMU was started through `tools/run-qemu.sh --gui --mem 256M --disk build/disk.img --qmp … --display-opts …`;
keys were posted as HID-level events through CoreGraphics (`CGEventPost`, the events a keyboard produces, so that
modifier keys give real `flagsChanged` events; System Events' `key code` does not, which an earlier run showed), and
the screen was read back with QMP `screendump`. As a control, `meta_l` was injected over QMP's `send-key`, which
bypasses the cocoa display: it produced `@` in every run, so the guest side (virtio-input, `inputd`, the compositor,
the keymap) delivers Meta correctly.

Typed in each run: `echo `, left Command alone, `l`, right Command alone, `r `, Option alone, `o `, Cmd-A, ` e`,
Enter; then Cmd-H, Cmd-M, Cmd-Tab, Cmd-W, Cmd-Q with a second of waiting after each, then Enter.

| Display | What the terminal showed | Host |
|---|---|---|
| `cocoa,left-command-key=on` | `echo @l#r o @a e`, then `@h@m@@w@q` | QEMU stayed up and in front after Cmd-H, Cmd-M, Cmd-W and Cmd-Q; Cmd-Tab switched to Terminal (the system's switcher) and left one `@` (the Meta press) in the guest |
| `cocoa,left-command-key=off` | `echo l#r o a e`, then `hmwq` | the same: nothing closed or hid, and the letters reached the guest without Meta |
| `cocoa` (the default, no suboption) | `echo @l#r o @a e`, then `@h@m@@w@q` | as with `left-command-key=on`: QEMU 11.1.2's default already delivers both Command keys |

Reading: with `left-command-key=on` both Command keys arrive as Meta, a Command chord arrives as Meta plus the key,
and in no run, with or without the option, did a Command chord act on the host (so the option cannot be credited with that; it pins the delivery of Meta). With `off`, QEMU swallows the left Command key
(the right one is delivered either way) and strips Meta from the chord. In no run did Cmd-Q quit QEMU, Cmd-W close
the window, Cmd-H hide it or Cmd-M minimise it. Cmd-Tab is taken by macOS before any application in every run, so
it cannot be a chord of the desktop; ADR-0160's switcher chord is settled below (Alt+Tab, measured; F12 as the backup). The Option key produced no character, so it is not Meta; its code was not measured (it should be `KEY_LEFTALT`,
which the first stage 7 task checks before relying on AltGr, ADR-0163).

## The switcher chord: System+` and Option+Tab

Measured the same way on 2026-11-03 with `cocoa,left-command-key=on`, with the throwaway keymap also mapping
`KEY_LEFTALT` (56) to `%` and the terminal mapping the Tab key to `^` so that both are visible. Typed: `echo `,
Cmd-` (the key code of ` on an ANSI keyboard, 50), a space, Option+Tab, a space, Tab alone, ` e`, Enter; the QMP
control; then Cmd-`, Option+Tab and Cmd-Tab again.

| Chord | What the terminal showed | Host |
|---|---|---|
| Cmd-` | nothing at all, not even the Meta press | macOS takes Cmd-` (its "move focus to next window") before QEMU; QEMU stayed in front |
| Option+Tab | `%^`: Alt and Tab both delivered | nothing on the host |
| Tab alone | `^` | |
| Cmd-Tab | the Meta press only (`@` in the earlier runs) | macOS's application switcher |

Reading: a switcher chord with the Command key cannot be Cmd-Tab or Cmd-`, since macOS consumes both; Option+Tab
reaches the guest whole as Alt+Tab. ADR-0160 therefore reserves Alt+Tab for the switcher (the only reserved chord
without the system key) and keeps F12 as the backup.

Decision taken from this: `tools/run-qemu.sh` opens the interactive GUI window on macOS with
`-display cocoa,left-command-key=on`, which pins the behaviour the default showed, against a later QEMU changing its
default; `--display-opts` replaces it for experiments. `full-grab=on`, which would
also take Cmd-Tab, was not needed and is not used, since it requires the Accessibility permission for QEMU. The
screenshots of the runs were kept outside the repository (they contain nothing but the lines above).
