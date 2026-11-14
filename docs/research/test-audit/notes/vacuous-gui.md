# TASK-29: GUI tests that can pass without testing what they claim

Read-only audit; nothing was built or run. Every statement below comes from reading the scripts, the runner
(`tools/test-runner/src/gui.rs`), the Makefile and the code that prints or draws the expected text.

## What each instruction proves (from gui.rs)

- `screen TEXT`: a whole row of the terminal grid equals TEXT (`r == text`, line 933), in a row below the last
  matched one. The grid read is the first window's body, or the body at `origin X Y`. `window`, `top` and `origin`
  reset the row to the top. So a typed command (`~> echo hi`) never satisfies its own output (`hi`), and text above
  the last match does not count; text left below it, or anywhere after a `window`, does.
- `prefix TEXT`: the same with `starts_with`.
- `serial TEXT`: `self.seen.contains(text)` over the whole serial line since boot (line 1013). No "after" notion:
  an identical earlier line satisfies it at once. Substring, not a whole line.
- `then TEXT`: substring of the serial line after the start of the last input step (type, key, chord, click, dclick,
  rclick, wheel, mclick, mdrag, drag; lines 983-998). `clickc` (AtColour) and `mark` are not in that list.
- `region`: waits until every pixel is the colour; passes at once if it already is. `ink`: waits until at least N
  pixels are not the colour (any non-matching pixels count).
- There is no negative step (no "text absent"). The only negatives are `region` with a ground colour and the
  Makefile's greps.
- `latency N [B [tcg B]]`: under TCG takes the `tcg` bound and polls every 150 ms; elsewhere polls every 10 ms. With
  no bound it only measures.
- `pause`: a fixed wall-clock wait. `shot`: a file copy, asserts nothing.
- `runs_under_tcg`: `--print-accel`; on CI (ubuntu-24.04, x86-64 host, aarch64 guest) `auto` resolves to tcg, so
  `X:auto:N` and `X:tcg:N` are the same configuration there.

## Confirmed findings

### G1. files.script:71-72 — the drop's read and refused write are satisfied by the chooser's earlier lines

- Claim (lines 55-59): "t_open gets the file to read", then `serial t_open: opened notes.test: 6 bytes: "hello"` and
  `serial t_open: write to notes.test refused`.
- Why it passes blind: both lines are byte-identical to lines 42-43, printed by the chooser's Open. `read_then_write`
  in `user/src/bin/t_open.rs:66-80` prints the same text for both paths (`shown` is `notes.test` in both), and
  `serial` matches anywhere in the log. A drop that hands t_open a writable handle (`write to notes.test allowed`),
  or one it cannot read (`reading notes.test failed`), still passes: only `files: dragging notes.test` and
  `t_open: dropped notes.test` are new.
- Confidence: CONFIRMED.
- Fix: lines 69-72 as `then` (the drag at line 68 is the input step). "Its frame the pill" (line 59) has a `shot`
  only; add a `region` on the pill if it is meant to be tested.

### G2. windows.script:20-24 and 27-31 — minimising is never observed

- Claim: "Minimised by the dash; F12 brings it back, and it takes keys again"; "Minimised again; Alt+Tab ... brings
  it back" (stage 7b's criterion: "minimises, restores").
- Why it passes blind: nothing is checked between `click 114 88` and the restoring key. If the dash does nothing,
  F12 with one window activates the same window (`next_window`, `crates/zoraya-compositor/src/lib.rs:2162-2169`:
  `order[1 % order.len()]`), the window was never hidden, and `echo restored` reads back. The same for Alt+Tab.
  Restoring is tested only if hiding happened.
- Confidence: CONFIRMED.
- Fix: after each `click 114 88`, a `region 130 76 200 4 <desktop colour>` (the title bar's place shows the
  desktop) before the key.
- Same pattern, SUSPECTED: rooms.script:35-41 ("Both hidden by their dashes; the dock's icon brings them back").
  No check that either window went away before `clickc`; I did not read what a dock click does to visible windows.

### G3. files-manage.script:120-124 — "Open terminal here" checks only the panel's status word

- Claim: "Open terminal here: a terminal in this folder, in the window's room."
- Why it passes blind: `then files: A terminal in ~/docs.` is printed when the panel answers status 0
  (`user/src/bin/files.rs:498-507`), and the panel answers 0 whenever `terminal_at` returns a launch record; the
  result of `launch(st, &l, None)` is not looked at (`user/src/bin/panel.rs:784-793`). A launch that fails, a
  terminal that opens in `~`, or one in the wrong room all pass. No window is read afterwards.
- Confidence: CONFIRMED.
- Fix: `origin` at the new terminal's body and `screen ~/docs> _` (and its tint or prompt for the room).

### G4. settings.script:25 and 27 — two `serial settings: Saved and applied.` that wait for nothing

- Claim: "Czech on, then in use", each click followed by a wait for the save.
- Why: line 20 already put the same text in the log, so lines 25 and 27 return at once. The second click (342, 535)
  goes 150 ms after the first, before the first save is applied; `shot czech` is taken with no wait. The outcome is
  still tested by `echo zy` -> `yz` (line 35), so this is a race and an unproved "Saved and applied" for the two
  layout changes, not a blind pass of the layout itself.
- Confidence: CONFIRMED.
- Fix: `then` on lines 25 and 27 (and on 20).
- Related: the text typed into edit (line 44) is never read back; an empty `poznámky.txt` passes lines 69 and 72.
  Add `cat` of the file in the terminal.

### G5. spinners.script:95 and INV-0264-3 — the 50 ms bound is never checked on CI

- Claim (header, point 2): "The median with the spinners may be at most 50 ms over the baseline's (INV-0264-3)".
- Why: `latency 30 +50 tcg +450`; under TCG the runner takes `+450` (gui.rs:1138-1142). CI is TCG for both
  `spinners:auto:1` and `spinners:tcg:1`, so CI asserts only "the median key is under 3 extra polls", which the
  comment itself says separates only a starved terminal ("takes seconds"). The 50 ms bound runs only on a Mac under
  HVF. The invariant's text says "under HVF", so this is by statement, but its only checker is a local run.
- Also: the script runs 2 spinners in each of two trusted rooms at `--cpu 25%`; the invariant's text is two
  untrusted rooms with 16 spinning and 16 bursting processes each. The header hands that to `t_cpu`.
- Confidence: CONFIRMED.
- Fix: none on CI without acceleration; say in `docs/adr/invariants.md` that the GUI half is local only, or have the
  nightly record that it did not run.

### G6. spinners.script:71-95 — nothing shows that the spinners are running while the latency is measured

- Claim: "then with the spinners".
- Why: `jobs` is asked once (lines 76-78), for spin1's two only, before `room --leave`. `room --leave` closes the
  room's launch job handle (`zosh.rs:3221-3228`). Spin2's two are checked only by `[3] spin` / `[4] spin`, which
  zosh prints when it starts a background line (`zosh.rs:4140`), whether or not the program lives. If the spinners
  end with the job handle, or `spin` fails to start in spin2, the second `latency` measures an idle machine and
  passes.
- Confidence: CONFIRMED that it is unchecked; whether closing the handle ends them I did not establish.
- Fix: after the last `latency`, `type jobs` and four `prefix [n]  running   spin` rows (or `top` with the rooms'
  CPU above zero).

### G7. netsurf.script:33-38 — the form's text is not checked

- Claim: "The form: type into the field, submit with Return; the result page is yellow."
- Why: `/ns/result` is yellow for any query (`tools/test-httpd/src/main.rs:210-223`); the page's `Result: {q}` is
  not read and the Makefile does not grep the request. Lost keys, or Return in an empty field, pass.
- Confidence: CONFIRMED.
- Fix: in test-netsurf's log check, `grep -q '^http: GET /ns/result?q=zoraya '` (the server logs every `/ns/`
  request line).

### G8. Shots where the comment claims a property (no assertion at all)

All CONFIRMED by the scripts; each has only `pause` and `shot`:

- rooms.script:76-80: "Show-commands: every icon and chip with its line."
- rooms.script:14-16: the terminal's "tooltip under the pointer" (`pointc` finds the dot's colour only).
- limits.script:44-48: "the hover card, above or below the window" (header: "draws the hover card, from the panel's
  readings only").
- files-manage.script:110-118: "In show-commands mode the menu writes each item's zosh line" (`menu open`, `menu
  closed` only).
- netsurf.script:83-88: "with no window" after the refused lines (the shell's message is checked; no `region`).
- windows.script:100-101: "A terminal of the panel's has no power to end the machine" is a remark; nothing types
  `poweroff` there. rooms.script:31 covers slots 4 and 5 (zosh lists slots in order, `zosh.rs:4192-4200`).

Fix: a `region`/`ink` on what the mode draws, or drop the claim from the comment.

### G9. Makefile: a failed probe is a silent skip (test-netsurf, test-hand-run's net part)

- `Makefile:727` and `:799`: `$(RUN_QEMU) --probe-net >/dev/null 2>&1 || { echo "... skipped ..."; exit 0; }`.
  Any failure of the probe (no slirp, but also a broken `run-qemu.sh` or QEMU missing) makes the whole of
  test-netsurf, with every INV-0228-1/0317/0326 check, exit 0. `tools/test-summary.sh` counts skipped lines but
  nothing fails on them, on CI either.
- Confidence: CONFIRMED.
- Fix: with `CI` set, a skip is an error (`[ -z "$$CI" ] || exit 1`).
- Not a finding: the loops of test-gui, test-posix-gui and test-tour-gui end with `|| exit 1` inside the loop; an
  empty `GUI_RUNS` or `POSIX_ACCELS` would run nothing, and `check-ci-jobs` compares CI's list to `GUI_RUNS_ALL`.

## test-netsurf's log check after 057f3a4 / 616fe29 / cdabf8d (class 6)

Sound now: a missing log or mark fails (`! grep -q "$mark"`), `before` must be exactly 1, `after` at least 1, an
empty `tlsexpired` port gives `before` = 0. What is left:

- G10 (SUSPECTED, small). `tls expired: ended after 0 request bytes` is printed for any connection that ends or
  times out before a request head, whether the handshake was refused or completed
  (`tools/test-httpd/src/main.rs:400-408`). If Proceed did turn verification off and the window is closed
  (`pause 5000`, then `click 122 119`) before NetSurf sent its request, which is possible under TCG, the log reads
  the same as a refusal. Fix: log whether the handshake completed (`conn.is_handshaking()`) and require
  "refused" on both sides of the mark.
- G11 (CONFIRMED, by design). netsurf.script:115 `region 110 175 40 20 9e9e9e` after Proceed is the same region as
  line 105 and already true: the screen proves nothing about Proceed; only the log does. Worth a comment there. I did
  not find where 9e9e9e is drawn (not in `ports/`), so I could not check that only the certificate error page
  shows it; the `before` = 1 count covers a DNS or connect failure.
- `tls valid: request of` is satisfied by any request to the valid server; the blue and green regions carry it.

## Timing assumptions (class 5)

- `pause 3000` before `net status` (net.script:10, hand-run-gui.script:15): the lease must be in within 3 s; the
  `prefix` then waits to the deadline but the command is not repeated, so a slow DHCP under TCG is a flake.
- tour-gui.script (generator `tools/tour/src/lib.rs:275`): `pause 1000` before Ctrl-C and Ctrl-Z, "pressed once
  the program is running". The evidence after it is real (`spin: interrupted (Ctrl-C)`, `[1] stopped: sleep 5`),
  so a late start is a flake, not a blind pass.
- netsurf.script: `pause 15000`/`20000` before `region` are delays only (region waits itself); before `shot` they
  are assumptions. `pause 5000` after Proceed bounds the time in which the second handshake must be logged
  (`after` >= 1): slow TCG fails, does not pass blind.
- `latency 10` (terminal.script:115) and the first `latency 30` (spinners.script:64) have no bound: measurements.
- windows.script:48-62 `pause 1500` between nine launches: followed by a real wait (`region 1004 300 ...`).

## Suspected, not verified

- S1. posix/gui.script:27-33, "Ctrl-C of a foreground sleep: 130". `chord ctrl c` follows `type sleep 30\r` with
  no wait. The tty flushes its input on VINTR (`crates/zoraya-tty/src/discipline.rs:150-170`); if the key arrives
  while dash is still the foreground group, dash itself takes SIGINT and (in current dash) sets status 130, with no
  sleep interrupted. Fix: `type echo go; sleep 30\r`, `screen go`, then the chord.
- S2. limits.script:43 `region 366 115 2 8 f6f9f9`: said to be a fill "which the calm title bar does not have",
  but the calm state is not asserted. Add `region 366 115 2 8 d3e1e5` before `type memhog`.
- S3. limits.script:26-28: "top shows the room against its limit" is `prefix project`: the 64M is not read (line 31,
  `grants echo`, does read it). Line 38 `prefix memhog: refused after` reads no amount.
- S4. spinners.script:43,53 `prefix memhog: refused after 9`: memhog prints KiB (`memhog.rs:30`), so the prefix
  takes 9000-9999 KiB but also 9x, 9xx and 9xxxx KiB. The stated break (a job of its own, about 15 MiB) fails.
- S5. terminal.script:87-94: the wheel check relies on `head -n 40 /system/tls/roots` printing 40 lines (the file
  has 2855 now). If head fails, the mark never leaves the view and `wheel up` is not needed. "Three lines a notch"
  is not bounded: 2 to any number pass. Fix: `top` on a known line of the output before the wheel.
- S6. files.script:21 and files-manage.script:28 `files: holds +r / +w ~`: substring; a further hold that sorts
  after the home (`files.rs:703-715` sorts by path) would pass. `/` and `/config` to write would not.
- S7. wheel.script:116,120 (the editor): at least three lines a notch, no upper bound (the file manager's part is
  exact).
- S8. Runner, latent: `clickc` and `mark` do not move `then`'s mark. No script has `then` after `clickc` today.

## Coverage

Read fully and found sound apart from the items above: terminal, windows, layouts, rooms, limits, spinners, files,
files-manage, settings, wheel, net, netsurf, netsurf-text, `tests/posix/gui.script`, `tests/posix/room-gui.script`
(tr and uname are not native programs, so they come from the volume), `tests/shell/hand-run-gui.script`, gui.rs
whole, the five Makefile recipes. No finding in layouts, net, netsurf-text, room-gui, hand-run-gui beyond the
timing notes.

Not read fully: `tests/posix/tour-gui.script` (926 generated lines). I read its generator's GUI half
(`tools/tour/src/lib.rs:210-290`), the first 60 lines, the Ctrl-C/Ctrl-Z steps and the end. Its pattern (Ctrl-L and
`top` before each line, `window`, then whole-row matches) cannot be satisfied by the typed line or by the old
screen; continuation prompts (`...>`) are not cleared and I did not check each of them.

Not established: what closing a room's job handle does to its processes (G6), what a dock click does to visible
windows (G2's rooms case), where 9e9e9e comes from (G11), which dash behaviour the port has (S1).
