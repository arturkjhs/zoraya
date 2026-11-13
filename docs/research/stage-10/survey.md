# Stage 10 survey: living in the system

The research behind proposed ADR-0331 to ADR-0342 (TASK-28): what a full-screen terminal, a curses library, `vi`,
`less`, a userland, pseudo-terminals, `git` and `ssh` need, and how systems without ambient authority let an
interactive program reach files a person names while it runs. It makes no decisions.

**How it was made.** Seven topics were researched in one session on 2026-11-13, each by a research subagent (Sonnet)
under the evidence rules of TASK-01: primary sources, every URL fetched in the session and the claim checked against
the fetched text, no citation from memory, paraphrase with short quotations only. The main session (which wrote the
ADRs) set the questions, read every section, assembled this file and wrote [Zoraya today](#zoraya-today) from the
repository. In the sections, "I" and "my count" are the subagent that wrote the section, and "the brief" is the
question list it was given, whose licence and evidence rules are TASK-28's and `CLAUDE.md`'s. A number marked as
counted or measured was produced by a command the section names (`wc -l`, `grep -c`, a script over a fetched file);
everything else is what the source says. A sentence marked `(inference)` is the researcher's, not the source's; one
marked `[unverified]` rests on no fetched page and no ADR relies on it.

**What could not be read** is listed per section in [Pages that could not be fetched](#pages-that-could-not-be-fetched).
The ones that matter for a decision: xterm's control-sequence document was read as its plain-text copy (the HTML
returned 403); the Plash site is gone and was read from web.archive.org copies of 2012; the Capsicum and SHILL papers
were read from text extracted from the PDFs, so their wording is approximate; the FreeBSD wiki's list of
Capsicum-converted programs, Ka-Ping Yee's design principles, HD Moore's terminal paper and the Fuchsia Starnix
devpts source were not read, and nothing rests on them.

**One expectation of the task that the sources did not bear out.** TASK-28 lists Plash among the systems that might
have a text-mode way to open a file by request. Nine pages of its wiki were read and none mentions an editor; its
only run-time grant is a graphical file chooser ([Pattern (a)](#pattern-a-grants-fixed-at-start)). The one shipped
text-mode request found anywhere is Deno's terminal prompt, with two advisories against it
([Pattern (b)](#pattern-b-a-trusted-intermediary-that-asks)).

**What the catalog already holds**, and this survey does not repeat: the kernel TTY layer and line disciplines
(DM-069), sessions and controlling terminals (DM-070), in-band escape sequences and their injection (DM-071),
`TIOCSTI` (DM-072), the Windows console and ConPTY (DM-073), ambient authority (DM-009), path races (DM-039),
retrofitted sandboxes (DM-014), object capabilities and where they hurt (GR-008), and the candidate principles CP-25
(terminal rights separated, control out of band, a VT translation for POSIX), CP-27 (a fixed and small POSIX scope)
and CP-28 (the network as capabilities). The graphical powerbox is in `../desktop/survey.md`, the capability shells
and their grant syntax in `../shell/survey.md`, the network grants in `../network/survey.md`.

## Zoraya today

Read from the repository at `a89755b` (2026-11-13) by two digest subagents and checked in part by the main session;
nothing was built or run for this task. A reference is an ADR item or a file and line. Where `docs/design.md` and the
code disagree (its "Known limitations" paragraph on the GUI still describes stage 5), this section follows the code.

### The terminal as built

Two arrangements exist. On the desktop the compositor's client `term` (`user/src/bin/term.rs`) owns the grid and the
scrollback and speaks `zoraya-proto-term` to `zosh`: `Output` (at most 4096 bytes), `Input` (at most 256 bytes, with a
count of dropped ones), `Size` and `Title`. `zosh` edits its own lines (ADR-0154) and relays every byte of its
programs' streams. A POSIX program started from `zosh` gets streams, not a terminal: its `isatty` is false (ADR-0153,
the note on ADR-0105 item 9; `user/describe/posix/dash.cmd` says so to the person). The other arrangement is the
POSIX session, started only when the volume has `/etc/zoraya/session`: `init` starts `ttyd` and the broker instead
of `zosh`, `ttyd --window` takes the shell's end of `term`, and the machine turns off when pid 2 ends. It is what
`make test-posix` and `make test-posix-gui` drive. Nothing opens a second POSIX terminal or starts a POSIX program
with a terminal from a desktop shell.

The emulator (`crates/zoraya-term/src/lib.rs`) parses the C0 controls `\n` (to column 0 of the next line, where xterm
keeps the column), `\r`, `\b`, `\t`; `ESC c`; and `CSI` with the finals `A B C D H f J K m` and nothing else. Every
`CSI` with a private marker (`?`, `>`, `=`, `<`) is dropped whole, so there is no alternate screen, scroll region,
insert or delete line or character, cursor visibility, application cursor keys, bracketed paste or mouse. `OSC`,
`DCS` and the other strings are skipped up to 4096 bytes. SGR knows 0, 1, 2, 7, 22, 27, 30 to 37, 39, 40 to 47, 49;
each parameter is read alone, so `ESC [ 38 ; 5 ; 1 m` sets bold. A cell is a character, two colour indexes and a
bold flag; every code point takes one cell (no wide or combining characters). No query is answered (INV-0096-2).
Scrollback is 500 lines; resizing the window resizes the grid without reflow and sends `Size`. The keys sent are
those of ADR-0096; there are no function keys, no Alt prefix, no modified arrows, Shift+Tab is Tab, and `term` drops
pointer, `Copy` and `Paste` events (no mouse, selection or clipboard). The fonts are JetBrains Mono and Inter
(ADR-0162): box drawing and block elements are complete, Cyrillic and Greek are there, and Braille, CJK and emoji
draw the hollow box.

### The tty service as built

The line discipline is the `no_std` crate `zoraya-tty`, hosted by `ttyd` (ADR-0105). It honours `ICRNL`, `INLCR`,
`IGNCR`, `OPOST` with `ONLCR`, `ISIG`, `ICANON` with `ERASE`, `KILL`, `EOF`, the echo flags and `NOFLSH`. With
`ICANON` off every byte is readable at once; `VMIN` and `VTIME` are defined and never read. A window size change is
`job_signal(foreground, Custom(28))`, which is `SIGWINCH` for a program that installed a handler, delivered at its
next wait point or library call (ADR-0106). `TIOCGWINSZ` gives rows and columns; there is no `TIOCSWINSZ`. In
`libposix`, `open("/dev/tty")` returns `ENXIO` and `ttyname` returns nothing although ADR-0105 items 4 and 6 decided
both; `pselect` reports a terminal ready at once (`PollReadable` exists in `ttyd` and the library never sends it);
`tcsetattr`, `cfmakeraw`, `tcflush`, `ctermid`, `select` and `poll` are not exported by `libc.a` (ADR-0141 hides every
name outside the tier lists; curl and wget link a shim). `TERM` is `dumb` in a session and unset from `zosh`; the
image has no terminfo or termcap file.

### What a POSIX program is given today

From `zosh` (ADR-0153, ADR-0200, ADR-0311): its three streams; the current directory to read; what its trusted
description designates; `/bin`, `/usr` and `/etc` to read, `/dev/null`, a `/tmp` of its command line; the random
service; `PATH` and `HOME`. Its children get the table `libposix` builds from its own (ADR-0107). A path no entry
covers fails with `ENOENT` and is reported once on the `REPORT` channel; after the job ends `zosh` prints the path
and the `+r` or `+w` line that would allow it (ADR-0153 rule 11). Nothing lets a running program be given more: the
broker never sees a path and holds no authority to hand out (ADR-0104 item 13). Trusted descriptions exist for
`dash`, `make`, `tcc`, `curl`, `wget` and three test programs; toybox's applets have none.

Process limits that a port meets: `fork` copies every page at the call and carries at most 64 connections; `exec`
reads the whole program each time (25 ms under HVF, 195 ms under TCG for a 1.4 MiB program; ADR-0212's image cache is
not built); programs are static; a file cannot be mapped (`ENODEV`); `mprotect` is `ENOSYS`; there are no hard links,
`st_nlink` is 1, the only identity is uid 1000 from `/etc/passwd` of the toolchain set, `chown` changes nothing;
`alarm` and `setitimer` are not offered (ADR-0106 item 10); `fcntl` record locks exclude only inside one process
(ADR-0319); `setsid` is not offered. Threads exist (ADR-0308).

### The network as built

A program reaches a host only through a scoped connection to `netd` (ADR-0220). `+net HOST[:PORT]` works before any
described program, and a literal `url` operand of an image description designates its host and port; the `host`
operand kind designates an echo only (proposed ADR-0330). Built in `libposix`: TCP over IPv4 with `connect`,
`getaddrinfo`, `select` by a shim. Not built although decided: `bind`, `listen`, `accept`, UDP, `socketpair`, `poll`,
`getnameinfo`, the inheritance of a socket or of the network connection by a child (ADR-0225 items 4, 5 and 9;
ADR-0287 item 3: a child of a POSIX program has no network and, when it was not started by `zosh`, no random
service), the `+listen` word, the rooms' network words, and the refusal report and hint of ADR-0224 item 5. A POSIX
program holds at most two connections. TLS for C programs is one static OpenSSL 3.5 (ADR-0312) that trusts
`/system/tls/roots` and nothing a file or a variable names (ADR-0310 item 6). There is no key agent and no client
certificate (ADR-0228 item 5).

### Ports as built

`ports/sources.lock` pins each archive by SHA-256; `make fetch-ports` is the only step with network. Built: dash,
GNU make, pdpmake, tcc, toybox 0.8.15 with twelve applets (a link per applet), busybox 1.38.0 for `tr` and `diff`,
zlib, OpenSSL, curl and wget (6.7 MiB each). The T2 applets do not link for want of about twenty libc names
([The userland today](#the-userland-today)). The exported surface of `libc.a` grows only by an ADR that names the
program behind each name (ADR-0005, INV-0005-1).

## Terminals

All measurements marked "my count" were made on files fetched on 2026-11-13 into the scratchpad (`wc`, `grep -c`, or a
small script that parses `terminfo.src` and resolves `use=` chains; the script is not part of the repository). The
terminfo counts include booleans, numbers, strings and user-defined (extended) names after `use=` resolution and `@`
cancellation, so they are comparable between entries but are not an official figure.

### How the terminal ecosystem is layered

ECMA-48 (5th edition, June 1991, also ISO/IEC 6429) defines control functions and their coded representations [T3]. In the
PDF the clause 8.3 lists 162 numbered control functions (my count of headings 8.3.1 to 8.3.162), and the file has 108
pages. ECMA-48 sets aside private use: if the first parameter byte of a CSI parameter string is in the range 03/12 to
03/15 (`<`, `=`, `>`, `?`), the format and meaning are "not defined by this Standard" [T3]. Every DEC private mode
(`CSI ? Pm h`) and every `CSI > ...` or `CSI = ...` query therefore sits outside the standard by construction (inference
from [T3], consistent with how [T13] labels the origin of each mode).

xterm's control sequence document is the de-facto reference above that. The plain-text copy, "updated for XTerm
Patch #411 (2026/08/23)", has 3,840 lines and 24,816 words (my count), and about 426 lines of the form "Ps = N -> meaning" (my
count of the private-mode, SGR and window-operation lists) [T2]. It documents the sequences this survey cares about:
DECCKM (mode 1), DECAWM (7), DECTCEM (25), alternate screen (47, 1047, 1048, 1049), mouse (1000, 1002, 1003, 1006),
focus events (1004), bracketed paste (2004), DECSTBM, DECKPAM/DECKPNM and SGR 38/48 [T2]. About 1049 it says to use it
"with terminfo-based applications rather than the 47 mode", and that it combines 1047 and 1048 [T2]. For colour it says
xterm accepts either colons (standard) or semicolons (legacy) to separate sub-parameters, and that after the first colon
only colons may be used [T2]. A search of the file finds no mention of synchronized output (mode 2026) or grapheme
clustering (mode 2027): the only hits for "2026" are the copyright and patch dates (my grep) [T2]. Those two modes are
defined elsewhere, see below.

Terminfo is the layer programs actually read. terminfo(5) lists the standard capability names and states that ncurses
also accepts user-defined ("extended") capabilities, which it treats as strings, numbers or booleans by name; names that
start with `k` are treated as function keys [T4]. It does not state a minimal set that a terminal description must have.
The only hard rules found in sources are in programs (next section) and in ncurses `initscr`, which refuses a "generic"
(`gn`) entry because it cannot be used for full-screen operation [T4][T31].

### What full-screen programs ask of a terminal

**less** (Mark Nudelman) is a termcap/terminfo program. In `screen.c` `get_term_info` reads `am`, `xenl`, `da`, `db`, `smkx`
and `rmkx`, `smcup` and `rmcup`, `el`, `ed`, `clear`, `cup`, `flash`, `home`, `ll`, `cr`, `il1` (as `al`) and `ri` (as `sr`),
`smso`/`rmso`, `smul`/`rmul`, `bold`, `blink`, the keypad key names (`kcuu1`, `kpp`, `khome`, `kdch1`, `kf1` and many more) and
`kbs` [T6]. It does not use scroll regions (no `csr` lookup was found). If neither `il1` nor `ri` exists, it sets
`no_back_scroll` and repaints the screen on any backward move (my reading of `screen.c`) [T6]. For mouse and bracketed
paste it does not use terminfo names: it falls back to hard-coded `ESC [ ? 1000 h`, `1002 h`, `1006 h` and
`ESC [ ? 2004 h` (with matching `l` strings) unless the user overrides them [T6]. For colour it emits `ESC [ 38 ; 5 ; N m`
and `ESC [ 48 ; 5 ; N m` directly [T6]. Licence: the file `LICENSE` is a two-clause BSD-style "Less License" and the
repository also has a GPL v3 text (`COPYING`) [T6].

**nvi / nvi2** put all terminal work behind curses. The `cl` directory is described as the "Vi interface to the curses(3)
library" [T7]. Its source calls `tigetstr`, `tigetnum("lines")`, `tigetnum("cols")`, `keypad(stdscr, TRUE)`, and reads
`cup`, `smso`, `rmso`, `el`, `cuu1` itself; the function-key table has about 20 `k*` names (`kcuu1`, `kdch1`, `knp`, `kpp`...)
[T7]. It passes the window size to curses by putting `LINES` and `COLUMNS` into the environment, and its comment says
this is "ugly" but the only portable way [T7]. Licence: three-clause BSD [T7].

**vim** keeps its own list of 'terminal options' (`t_xx`) that default to values from terminfo. `t_cm` (cursor motion)
is marked "required!" (E437) [T5]. The list covers `t_cs` (scroll region), `t_al`/`t_AL`, `t_dl`/`t_DL` (insert and delete
lines), `t_sr` (reverse scroll), `t_ti`/`t_te`, `t_vi`/`t_ve`/`t_vs`, `t_ks`/`t_ke`, `t_ts`/`t_fs` (title), `t_md`, `t_mr`, `t_us`, `t_ZH`
(italic), `t_Co`, `t_AF`/`t_AB`, and the "added by Vim" group: `t_8f`/`t_8b`/`t_8u` (RGB), `t_BE`/`t_BD`/`t_PS`/`t_PE`
(bracketed paste), `t_XM` (mouse), `t_fe`/`t_fd` (focus), `t_RV` (version request), `t_u7` (cursor position, used for
'ambiwidth'), `t_SH`/`t_RS` (cursor shape), `t_Cs` (undercurl), `t_BS`/`t_ES` (synchronized update) [T5]. `t_BS` and `t_ES`
are not read from terminfo but default to `ESC [ ? 2026 h` and `ESC [ ? 2026 l` [T5]. Vim says several terminfo names
(`RV`, `BE`, `BD`, `PS`, `PE`, `XM`, `FE`, `FD`) were added to ncurses' database at the start of 2023 so that terminals
need not claim an `xterm-` name to get Vim's built-in xterm behaviour [T5]. Vim's help also says the kitty terminal
"dictates how applications need to work" and that Vim ignores the `xterm-` prefix for `xterm-kitty` [T5]. Licence: the Vim
licence (not one of the permissive licences named in the brief) [T5].

**readline** (Python's `readline` module, bash) reads termcap names `ce`, `cl`, `cr`, `dc`/`DC`, `ic`/`IC`, `im`/`ei`,
`le`, `nd`, `up`, `ho`, `vb`, `vs`/`ve`, `ks`/`ke`, `kd kl kr ku kD kI kN kP kh @7`, `so`/`se`, and the bracketed-paste
names `BE`, `BD`, `PS`, `PE` [T10]. It only enables bracketed paste if the terminal looks ANSI: the source says there is
"no way to determine" support, so a non-ANSI terminal is assumed not to have it [T10]. `enable-keypad` is off by default
and, when on, sends `smkx` [T10]. st's FAQ confirms the practical effect: programs that never send `smkx` get the wrong
cursor-key sequences from a terminal whose terminfo says application mode [T24]. Licence: GPL v3 [T10].

**Python's REPL (pyrepl, `Lib/_pyrepl`)** on the CPython 3.13 branch imports a local `curses` wrapper and calls
`curses.setupterm`, `tigetstr` and `tparm`; the `main` branch has a pure-Python `terminfo.py` ("curses-like terminal
capability queries") that reads compiled terminfo files itself [T9]. Both read the same list: required `bel`, `clear`,
`cup`, `el`; optional `civis`, `cnorm`, `cub`, `cub1`, `cud`, `cud1`, `cuf`, `cuf1`, `cuu`, `cuu1`, `dch`, `dch1`, `hpa`,
`ich`, `ich1`, `ind`, `pad`, `ri`, `rmkx`, `smkx` [T9]. A missing required name raises `InvalidTerminal` ("terminal doesn't
have the required ... capability") [T9]. It also writes `ESC [ ? 7 l` / `ESC [ ? 7 h` (autowrap off and on) and
`ESC [ ? 2004 h` / `l` directly, installs a SIGWINCH handler, and gets the size from `LINES`/`COLUMNS` first, then `TIOCGWINSZ`, then 25x80 [T9]. Its licence was not checked beyond the project LICENSE title (see Sources, [T9]).

**htop** needs ncurses: its README says running it "requires `ncurses` libraries" and that it needs ncurses 6.0, ideally
the wide-character flavour; `configure.ac` searches package names `ncursesw6` ... `curses` [T8]. Licence: GPL v2 or later
(README badge, `COPYING` is GPL v2) [T8]. What it emits is therefore whatever ncurses emits for the entry in `$TERM`.

**tmux** is both a terminal client and a terminal emulator, so it states its needs most explicitly. Its man page has a
"TERMINFO EXTENSIONS" section with 23 entries (my count): `AX`, `Bidi`, `Cs`/`Cr`, `Cmg`/`Clmg`/`Dsmg`/`Enmg`, `Dsbp`/`Enbp`,
`Dseks`/`Eneks`, `Dsesc`/`Enesc`, `Dsfcs`/`Enfcs`, `Hls`, `Iscrl`, `Nobr`, `Rect`, `Smol`, `Smulx`, `Setulc`/`Setulc1`/`ol`, `Ss`/`Se`,
`Spb`, `Swd`, `Sxl`, `Sync`, `Tc` (same as `RGB`), `Ms`, `XT` [T11]. It says these are normally set through the option
`terminal-features`, which has 24 named features (my count): `256`, `appesc`, `clipboard`, `ccolour`, `cstyle`, `extkeys`,
`focus`, `hyperlinks`, `imagescroll`, `ignorefkeys`, `kittygraphics`, `margins`, `mouse`, `osc7`, `overline`, `progressbar`,
`rectfill`, `RGB`, `sixel`, `strikethrough`, `sync`, `title`, `usstyle`, `utf8` [T11]. `XT` is described as meaning the
terminal supports xterm title sequences and makes tmux set some of the others (bracketed paste, focus) automatically
[T11]. Inside tmux, `default-terminal` "must be set to `screen`, `tmux` or a derivative" [T11]. Licence: ISC-style
(the `COPYING` text is the "permission to use, copy, modify, and distribute ... with or without fee" form) [T11].

**ncurses applications** read the terminal description through terminfo and, for window size, through `TIOCGWINSZ` unless
`LINES`/`COLUMNS` are set (see "Window size" below) [T31][T32]. The sample programs shipped with ncurses are described in
the project README: `blue`, `bs`, `firework`, `gdc`, `hanoi`, `knight`, `lrtest` (access to the lower-right corner),
`ncurses` (a multi-test program), `newdemo`, `rain`, `tclock`, `testcurs`, `worm`, `xmas`; most use colour [T39]. Licence:
an MIT-style permission notice (X11 type) from Thomas Dickey and the Free Software Foundation [T41].

### Terminfo entries compared

The file `misc/terminfo.src` from the ncurses snapshot mirror is dated `2026/11/07` in its header and is 1,180,340 bytes
(bytes from `curl`) [T1]. It has about 2,900 distinct names including building-block fragments (my count). Measured
size of the resolved entries (my script):

| entry | caps | note |
|---|---|---|
| `dumb` | 6 | `am`, `cols#80`, `bel`, `cr`, `cud1`, `ind` |
| `ansi` | 82 | |
| `vt100` | 85 | |
| `vt220` | 107 | |
| `screen` / `screen-256color` | 116 | no italics, no title, no `Ss`/`Ms` |
| `linux` | 119 | 8 colours; no `smcup`/`rmcup` |
| `st` / `st-256color` | 214 / 216 | |
| `foot` | 264 | built from `xterm+256color2` plus `foot+base` |
| `tmux` / `tmux-256color` | 255 | |
| `alacritty` | 269 | |
| `xterm` / `xterm-256color` | 294 / 297 | |

All sizes from [T1]. The `foot` entry is not a small description: its base fragment pulls in some 30 shared fragments
(`ansi+csr`, `ansi+idc`, `ansi+idl`, `xterm+sm+1006`, `bracketed+paste`, `xterm+focus`, ...), and its comments record that
Dickey tested it with vttest and tack and found several gaps (no application keypad mode, no NRCS, no VT52, no SRM,
`SL/SR` not working, no focus events at version 1.8.2; the later 1.18.1 note lists rectangle operations with problems at
left/right margins) [T1].

The next table is the artefact asked for: feature, who needs it, terminfo name, and the sequence as found in the entries.
"Entries" shows where the capability is present in [T1] (x = xterm-256color, f = foot, a = alacritty, s = st-256color,
t = tmux-256color, L = linux, 100 = vt100, 220 = vt220).

| feature | who needs it | cap | sequence | entries |
|---|---|---|---|---|
| cursor address CUP | all (vim: "required"; pyrepl: required; less) | `cup` | `ESC [ r;c H` (1-based) | all |
| relative moves CUU/CUD/CUF/CUB | pyrepl, readline, ncurses | `cuu cud cuf cub`, `hpa`, `vpa` | `ESC [ n A/B/C/D`, `ESC [ n G`, `ESC [ n d` | all have `cuu..cub`; `hpa`/`vpa` not in vt100/vt220 |
| erase ED, EL, EL1 | all | `ed el el1` | `ESC [ J`, `ESC [ K`, `ESC [ 1 K` | all |
| erase chars ECH | ncurses | `ech` | `ESC [ n X` | all but vt100 |
| insert/delete line IL/DL | vim `t_al`/`t_dl`, less `al`, ncurses | `il1 dl1 il dl` | `ESC [ L`, `ESC [ M` | all but vt100 |
| delete char DCH | readline `dc`, pyrepl, ncurses | `dch1 dch` | `ESC [ P` | all but vt100 |
| insert char ICH | readline `ic`, pyrepl | `ich1 ich` | `ESC [ @` | none of the 9 entries I checked define `ich`/`ich1` |
| scroll region DECSTBM | vim `t_cs`, ncurses | `csr` | `ESC [ t;b r` | all |
| scroll SU/SD | ncurses | `indn rin` | `ESC [ n S`, `ESC [ n T` | x f a s t (not vt100, vt220, linux) |
| IND/RI | ncurses, less `sr`, pyrepl `ri` | `ind ri` | `ESC D`/`LF`, `ESC M` | all |
| save/restore cursor | ncurses | `sc rc` | `ESC 7`, `ESC 8` | all |
| alternate screen | vim, less, nvi, ncurses | `smcup rmcup` | `ESC [ ? 1049 h/l` (xterm, f, a also add `ESC [ 22;0;0 t` / `23;0;0 t` title push/pop) | x f a s t; not linux, vt100, vt220 |
| cursor visibility DECTCEM | vim `t_vi/ve`, pyrepl | `civis cnorm cvvis` | `ESC [ ? 25 l/h` (xterm adds `? 12 l`) | all but vt100 |
| autowrap DECAWM | pyrepl (direct), ncurses | `smam rmam`, `am`, `xenl` | `ESC [ ? 7 h/l` | all; `xenl` in x f a s t L |
| app cursor keys DECCKM + keypad | vim `t_ks`, less, readline | `smkx rmkx` | `ESC [ ? 1 h ESC =` / `ESC [ ? 1 l ESC >` | all but vt220, linux |
| SGR attributes | all | `bold dim smul rev blink invis sitm smxx sgr sgr0` | `ESC [ 1 m` etc. | italics `sitm` in x f a s t, not linux/vt220 |
| 8 colours | all colour users | `setaf setab op` | `ESC [ 3n m`, `ESC [ 4n m`, `ESC [ 39;49 m` | linux has `colors#8` |
| 256 colours | vim `t_Co`, less (direct), ncurses | `colors#0x100`, `setaf setab` | `ESC [ 38:5:n m` or `38;5;n` | x f a s t, not linux |
| direct colour | vim `t_8f`, tmux `Tc`/`RGB` | `RGB`, `setrgbf`, `Tc` (tmux ext.) | `ESC [ 38:2::r:g:b m` or `38;2;r;g;b m` | not in any of the 9 default entries; only the `-direct` variants |
| styled underline | vim, tmux `Smulx` | `Smulx` | `ESC [ 4:n m` | f a t only |
| bracketed paste | vim, readline, pyrepl, less | `BE BD PS PE` | `ESC [ ? 2004 h/l`, `ESC [ 200~ ... ESC [ 201~` | x f a s t, screen-256color |
| mouse 1000/1002/1006 | vim `t_XM`, less, tmux | `kmous`, `XM`, `xm` | `ESC [ ? 1006;1000 h`, reports `ESC [ < b;x;y M/m` | `XM` in x f a only; `kmous` is `ESC [ <` in x f a and `ESC [ M` in s t L |
| focus events 1004 | vim `t_fe/fd` | `fe fd` (user-defined, added to ncurses 2023 per [T5]) | `ESC [ ? 1004 h/l`, `ESC [ I`, `ESC [ O` | via `xterm+focus` fragment in foot |
| title | vim `t_ts/fs`, tmux `XT` | `tsl fsl dsl hs` | `ESC ] 0;` or `ESC ] 2;` ... `BEL` or `ESC \` | s t a f; not xterm-256color in this file |
| clipboard | tmux `set-clipboard` | `Ms` | `ESC ] 52 ; c ; data ST` | x f a s t |
| cursor style | vim `t_SH`, tmux | `Ss Se` | `ESC [ n SP q` | x f a s t |
| DA1, DSR, CPR | vim `t_u7`, `t_RV`; ncurses `u6..u9` | `u6 u7 u8 u9` | `ESC [ 6 n` -> `ESC [ r;c R`, `ESC [ c`, `ESC [ > c` | all define `u6`..`u9` (`u9` = `ESC Z` in vt100) |
| synchronized output 2026 | vim `t_BS` (hard-coded), tmux `Sync` | `Sync` (extension) | `ESC [ ? 2026 h/l` | none of the 9 entries define `Sync` (no match in [T1]) |
| line drawing | ncurses, all TUI box drawing | `acsc smacs rmacs enacs` | `ESC ( 0` ... `ESC ( B`, or SO/SI | all; the `acsc` pair map differs (xterm: `` ``aaffgg... ``, linux and screen/tmux: `++,,--..00` first) |
| repeat char | ncurses | `rep` | `c ESC [ n-1 b` | x f a s t, not tmux |

Sources for the table: capability values from [T1] (my parse); program needs from [T5], [T6], [T7], [T9], [T10], [T11]; the
meaning of the sequences from [T2] and [T13]. The entry `linux` has no `smcup` in [T1]; the same file carries an entry
`linux-alt1049` with the comment "Linux 7.0 (expected in April 2026) supports alternate-screen switching" [T1]. The kernel's
own `console_codes(4)` page lists no 1049 or 2004 mode and calls itself "a large subset of the VT102 and ECMA-48" [T27].

### What a small terminal implements

**Linux console** (the baseline) implements a large subset of VT102 and ECMA-48 plus private sequences for the palette and
mappings; the man page tells programs to use terminfo and a library such as ncurses instead of hard-coding [T27]. It
knows DECSTBM, DECOM, DECAWM, DECTCEM (`? 25`), X10 and normal mouse (`? 9`, `? 1000`), SGR including 38/48 mapped onto 8 or
16 colours ("shoehorned") [T27]. Its terminfo entry has 119 names [T1].

**st** (suckless): the home page says it is based on most VT10x sequences, UTF-8, wide characters, 256 and true colours,
line drawing and resize, and states the aim to avoid xterm's complexity by quoting xterm's README: more than 65K lines,
"emulates obscure and obsolete terminals" [T22]. The goals page says "Have a working graphical terminal for terminal
applications" and "Do not reimplement tmux and his comrades"; the listed non-goals include unlimited scrollback, session
servers and colour filters [T23]. Measured: `st.c` is 2,712 lines and 58,889 bytes per the repository viewer [T25]. From the
source: DECSET handles 1, 5, 6, 7, 25, 9, 47, 1000, 1002, 1003, 1004, 1006, 1034, 1047, 1048, 1049 and 2004; it
ignores 2, 3, 4, 8, 12, 18, 19, 42; CSI covers the full cursor, erase, IL/DL/ICH/DCH, ECH, SU/SD, DECSTBM, DECSCUSR set;
OSC covers 0, 1, 2, 4, 10, 11, 12, 52, 104, 110, 111, 112; there is no case for 2026 or 2027 (my reading of [T25]). It
calls libc `wcwidth` for character width and `TIOCSWINSZ` on resize [T25]. Its FAQ says that when a program complains the
terminal is unknown the cause is a missing terminfo entry, and that "Some programs just assume they're running in xterm"
[T24]. `st.info` defines `st-mono`, `st`, `st-256color`, `st-meta`, `st-meta-256color`, `st-bs`, `st-bs-256color` [T26]; the
README tells users to compile it with `tic -sx st.info` [T24]. Licence: MIT/X Consortium [T22].

**foot** documents every sequence it supports in `foot-ctlseqs(7)` (195 table rows, my count): C0 controls, ESC (DECSC,
DECRC, RIS, IND, NEL, HTS, RI, SS2/SS3, DECKPAM/DECKPNM, SCS where only `0` and `B` are supported for G0 to G3), SGR
including `38:2::`, `38;2;` and underline colour 58, private modes, window operations, DCS (sixel, DECRQSS, XTGETTCAP)
[T13]. Among the private modes it lists 1, 5, 6, 7, 12, 25, 45, 47, 66, 1000-1004, 1006, 1007, 1015, 1016, 1034-1036, 1042, 1047-1049,
1070, 2004, 2026, 2027, 2031, 2033, 2048, 8452 and its own 737769 [T13]. It does not support 8-bit C1 controls [T13]. It
answers DA1 as a VT220 with sixel and ANSI colour, DA2 as "a VT220 with a version number", DA3 as "FOOT", and XTVERSION
[T13][T14]. README features are fast, lightweight, Wayland-native, true colour, styled underlines, synchronized updates
and sixel; the README is explicit that foot sets no environment variable to identify itself and recommends DA2/DA3/XTVERSION
instead [T14]. Licence: MIT [T14]. Its test directory in the git tree holds only `meson.build` and `test-config.c` per the
Codeberg API listing (my check), so VT behaviour is not tested there by recorded sequences (inference: other test files
may live in unit tests inside source files; not checked) [T14].

**alacritty** publishes `alacritty-escapes(7)` with a status per sequence (Implemented, Partial, Rejected): 75 table rows
(my count) [T19]. It implements ESC `(` `)` `*` `+` for charsets `B` and `0` only, DECKPAM/DECKPNM, `ESC 7/8`, `ESC # 8`
(DECALN), IND/NEL/HTS/RI/DECID; CSI cursor and erase and edit functions, DECSTBM (`r`), SU/SD, `CSI u` kitty keyboard
protocol forms; `CSI ? h` modes 1, 3, 6, 7, 12, 25, 1000, 1002, 1004, 1005, 1006, 1007, 1042, 1049, 2004 and 2026; SGR 0-9,
21-49, 58, 59, 90-97, 100-107 (rejected: 11-19, 51-55); `CSI t` only 22 and 23; OSC 0, 2, 4, 8, 10, 11, 12, 50, 52, 104,
110-112 (rejected: OSC 1, icon names); DCS `= s` rejected because mode 2026 is used instead [T19]. It ships its own terminfo
`alacritty` and `alacritty-direct`; its install notes say "either the `alacritty` or `alacritty-direct` terminfo must be
used" [T20]. Licence: Apache-2.0 (file `LICENSE-APACHE`) [T20]. Alacritty tests itself with recorded sessions: the file
`alacritty_terminal/tests/ref.rs` feeds `alacritty.recording` into the parser and compares with `grid.json`; there are 45
`ref_tests!` cases, including recordings of `vim_simple_edit`, `vim_24bitcolors_bce`, `tmux_htop`, `tmux_git_log`,
`zsh_tab_completion`, and several named `vttest_*` [T21].

**kitty** goes the other way: it adds protocols rather than leaving things out. Its documented extensions include a graphics
protocol, a keyboard protocol, a text-sizing protocol, file transfer and hyperlinks [T15]. The keyboard protocol is opt-in
through `CSI = flags ; mode u`, with flag bits "Disambiguate escape codes" (1), "Report event types" (2), "Report alternate
keys" (4) and more; by default the terminal emits legacy codes "for compatibility" [T17]. Kitty ships its own terminfo
entry `xterm-kitty` (my count: about 260 comma-separated items in `kitty.terminfo`, 4,257 bytes) containing the extended
names `Su`, `Tc`, `XF`, `fullkbd`, `BD`/`BE`/`PE`/`PS`, `Cs`/`Cr`, `Ms`, `RV` ... [T18]. ncurses' own `terminfo.src` also has a
shorter entry named `kitty` (`use=xterm+256color, use=kitty+common`) [T1]. Licence of kitty: GPL v3 (the `LICENSE` file)
[T18]; so kitty is a protocol reference only for Zoraya, not code.

### Which TERM to claim

Arguments from the sources, in both directions:

- For an own name: foot's `INSTALL.md` says `xterm-256color` "will, in many cases, work" but recommends foot's own terminfo
  because foot's entry has non-standard capabilities used by tmux, because "new capabilities added to the `xterm-256color`
  terminfo could potentially break foot", and because its description may change [T14]. foot also states that reading
  `TERM` is "not reliable" to identify the terminal [T14]. Vim says terminals that call themselves `xterm-something` get
  Vim's builtin xterm termcap and then fail where they differ ("quite often these terminals are not 100% compatible with
  xterm") [T5]. tmux's FAQ: "Inside tmux TERM must be `screen`, `tmux` or similar", and "Outside, it should match your
  terminal" [T12].
- Against an own name: the problem is distribution of the entry. kitty's FAQ says the cause of "terminal unknown" and
  broken arrow keys is "the kitty terminfo files not being available", most commonly after SSH to a host without them;
  its fix is `kitten ssh`, which copies the files, and `sudo` can drop `TERMINFO` [T15]. st's FAQ gives the same advice
  (run `tic -sx st.info`) [T24]. foot installed to a private path sets `TERMINFO` in the child, which "simply does not
  work" in some situations (issue 695 in the repository), and installing under another name breaks programs that test
  `$TERM == foot` [T14]. Foot's partial answer is XTGETTCAP, which lets programs query capabilities from the terminal so
  that "you would no longer have to install foot's terminfo on remote hosts" [T14].
- Middle path seen in the data: ncurses itself carries entries for `st`, `foot`, `alacritty`, `kitty`, `wezterm`, so a
  terminal with a popular name is covered on any host with a recent ncurses [T1]; foot's `INSTALL.md` recommends using
  these ncurses entries when available [T14]. The terminfo(5) text says ncurses users override or extend capabilities with
  user-defined names; tmux confirms that `terminal-overrides` and `terminal-features` exist because entries often lack
  RGB, `Ms` and `Sync` [T11][T4].
- `tmux`'s FAQ explains why it uses `screen` inside: it "is already widely available", while `tmux` and `tmux-256color`
  exist in modern ncurses [T12].

### Window size

On Linux, `ioctl(fd, TIOCGWINSZ, struct winsize *)` and `TIOCSWINSZ` read and set the size; the structure has `ws_row`,
`ws_col`, `ws_xpixel`, `ws_ypixel` (the pixel fields are marked "unused"), the kernel keeps the window size but does not
use it (except for virtual consoles), and "when the window size changes, a SIGWINCH signal is sent to the foreground
process group" [T28]. POSIX.1-2024 (Issue 8) adds `tcgetwinsize(int fildes, struct winsize *)` and `tcsetwinsize` in
`<termios.h>`; for a pseudo-terminal whose size was never set, the size "shall be set to an appropriate default" at open
time [T29]. The page's rationale says SIGWINCH is delivered "conventionally" to the foreground process group; it warns that
touching a `winsize` structure in a signal handler is not atomic, that a process in the background gets no SIGWINCH and
should re-query on SIGCONT, and that threaded programs should use `sigwait` [T29]. The `<signal.h>` page for the same
issue lists SIGWINCH with default action "I" (ignore) and notes that Austin Group defect 1151 added it [T30].

How programs react: ncurses installs its own SIGWINCH handler if the application has none; on the next read the library
processes the event, resizes its windows, and `wgetch` returns `KEY_RESIZE`, which the application uses to relayout [T31].
If `LINES` or `COLUMNS` are set in the environment they override the size obtained from the operating system, "so even if
a SIGWINCH is received, no screen size change may be recorded" [T31]; `ncurses(3x)` says the same and mentions
`use_tioctl` to keep the variables in sync [T32]. nvi works around exactly this environment trap (see above) [T7], pyrepl
reads `LINES`/`COLUMNS` first and then the ioctl [T9], readline reads the same variables first (unless told to ignore
the environment), then the size from the ioctl, then termcap `co`/`li`, then 80x24 [T10], and st sends `TIOCSWINSZ` to the pty when its window changes [T25].

### UTF-8 and character width

POSIX `wcwidth()` returns the number of column positions for a wide character, 0 for NUL and -1 for non-printable; the
Issue 8 page says it is XSI-marked and depends on the current locale [T33]. Unicode UAX #11 defines the East_Asian_Width
property with six values (Ambiguous, Fullwidth, Halfwidth, Narrow, Wide, Neutral); Ambiguous characters need context to
resolve, and the annex itself notes the property "is not intended for use by modern terminal emulators without
appropriate tailoring" [T34]. The reissued revision for Unicode 18.0.0 is revision 46 [T34]. Markus Kuhn's `wcwidth.c`
(309 lines, my count) assigns width 0 to Mn, Me, Cf and U+200B, 2 to Wide and Fullwidth, 1 to everything else including
East Asian Ambiguous; its comment says no established standard exists for how many cells each character takes and
that the choice for Ambiguous "depends purely on a preference" [T35]. Combining marks are therefore width 0 and attach to the
previous cell in terminals that follow it; emoji and variation selectors are not handled by it (the file dates from Unicode
5.0, 2007) [T35].

The mismatch problem is stated most directly by kitty: client and terminal must "agree on how many cells a particular
string should be rendered in", the only shared database is Unicode which changes almost yearly, and getting the correct
width needs grapheme segmentation, so kitty's text-sizing protocol lets the client send the width explicitly [T16]. The
Contour project's "Terminal Unicode Core" specification (mode 2027) handles grapheme clusters, variation selectors VS15 and
VS16 and cursor placement, and explicitly not right-to-left text [T36]; foot lists mode 2027 as "Grapheme cluster
processing" with origin "contour" [T13]. Vim handles the ambiguity itself: with `t_u7` it asks the terminal for the cursor
position after printing an ambiguous-width character to set 'ambiwidth' [T5] (the option name appears in the `t_u7`
entry; the mechanism is the inference from the "see 'ambiwidth'" note). st uses libc `wcwidth` [T25], which is what
nearly all `less`/`vi` programs also use (inference).

### Testing a terminal

`vttest` tests "VT100-compatible" terminals; the author says the documentation is the source and the original VT100
manual, and that tests past version 1.7 cover VT220 to VT520 and xterm variants [T37]. It was written 1983 to 1985 by Per
Lindberg and has been maintained by Thomas Dickey since 1996 [T37]. Its original terms were "noncommercial"; the page says
the author later changed it to a BSD licence [T37]. Dickey also names tack (tests a terminal description), the ncurses
examples (which indirectly test line drawing and character encoding), and scripts in xterm's source for xterm-only
features [T37]. `esctest` (George Nachman and Thomas Dickey) is a unit-test suite for "similarity to a theoretical
ideal ... xterm, but without bugs in George's opinion"; it has flags `--expected-terminal={iTerm2,xterm}` and an action
`list-known-bugs` that lists the tests with known bugs for a given terminal [T38]. The freedesktop terminal-wg copy is GPL
v2 or later with 22 commits on one branch (per the GitLab project page) [T38]. The ncurses sample programs are built
both as part of ncurses and as "ncurses-examples", and the README admits they "do not comprise a complete set of tests"
and tabulates which library calls each program exercises [T39]. Alacritty's recorded-session reference tests are in [T21];
foot's repository test directory holds only a config test (checked via the Codeberg API listing) [T14].

## Curses libraries and terminfo

Scope: what a full-screen terminal program needs from a terminal-description library, and what `vi`-class editors and
`less`-class pagers do with files that the command line never names. All pages were fetched on 2026-11-13. Where a
number was measured by me, the method is stated. Statements marked `(inference)` are mine, not the source's.

### ncurses

The ncurses `COPYING` file is the MIT-style ("X11") permission notice: free use, copy, modify, merge, publish,
distribute and sell, with the notice kept and a ban on using the holders' names in advertising. The holders are Thomas
E. Dickey and the Free Software Foundation (the file lists 1998-2017/2018 for the FSF and 2018-2023 for Dickey) [C2].
So ncurses is permissive by the project's rule, although it is not a BSD/ISC text.

The `INSTALL` file lists the options that matter for an embedded port [C1]:

- `--disable-database`: use only built-in data. The libraries normally read terminfo and termcap from disk; with this
  option they use a built-in database ("fallback" entries). The file says embedded applications may have no need for
  an external database, and that `tset`/`tput` stay useful while `infocmp`/`tic` do not [C1].
- `--with-fallbacks=LIST`: a list of terminal descriptions compiled into the library. The build must run on a machine
  that has ncurses' `infocmp`, `tic` and a terminfo database, because the fallback source is generated before the
  build tree's own `infocmp` exists. Fallbacks are consulted by `setupterm()` only after the terminfo tree and the
  optional termcap fallback failed, so they never shadow an on-disk entry. By default the list is empty [C1].
- `--with-default-terminfo-dir=DIR` (default `DATADIR/terminfo`, for example `/usr/share/terminfo`) and
  `--with-terminfo-dirs=LIST` (a colon list compiled into the library) set where the database is looked for [C1].
- `--with-hashed-db[=XXX]`: store terminfo in one hashed file using the Berkeley DB 1.85 interface instead of a
  directory tree, one file per entry. `tic` then writes only the hashed form. A directory cannot hold both forms [C1].
- `--with-termlib[=NAME]`: split the build into `libncurses` and a low-level `libtinfo`, which "is about half the
  size of the total" [C1]. Programs that only need `tgetent`/`tputs`-level access can link only that part.
- `--enable-widec`: builds the wide-character variant (`libncursesw`). The file warns that applications built for it
  are not compatible with 8-bit-character builds [C1].
- `--without-cxx` (and `--without-cxx-binding`): do not build the C++ binding. Without a C++ compiler `--without-cxx`
  is required; the file warns that C++ applications may then fail to compile or run [C1].
- `--enable-termcap`: make curses and terminfo fall back to an existing termcap file [C1].

Size: I measured the release tarball `ncurses-6.6.tar.gz` on `ftp.gnu.org` with an HTTP HEAD request: Content-Length
3,791,150 bytes [C3]. The NetBSD-curses README states, for its own build comparison on one x86_64 sabotage system, an
ncurses extracted source of 15.8 MB, `libncursesw.so` of 346 KB, and 6.4 MB for the ncurses terminfo database [C9].
Those numbers are the competing project's, in a build tuned for size (`-Os`, `--gc-sections`) [C9].

### How ncurses finds a terminal description

The manual of ncurses 6.6 gives this search order [C4]. First `TERMINFO` is checked, as a database path. Then
`$HOME/.terminfo`, an optional feature that a build may omit or limit for privileged programs. Then each colon-separated
entry of `TERMINFO_DIRS`; an empty entry means the system directory `/usr/share/terminfo`. Last, the compiled-in
locations. The database may be a directory tree or a hashed database [C4].

One detail matters for a system without a `/usr/share`: `TERMINFO` "can contain a terminal description instead of the
pathname". If it begins with `hex:` or `b64:`, ncurses decodes the compiled description from the variable and uses it
if the name matches; the `-Q` option of `tic` and `infocmp` produces such text [C4]. So the terminal emulator could
hand every child its own description through the environment, with no file at all (inference).

The same manual page says the library "reads the first description which passes its consistency checks" [C4]. In a
directory tree the entry is a compiled binary file (the layout `x/xterm` is from my memory, `[unverified]`) [C4]. `tic` compiles source to this binary form,
`infocmp` prints or compares entries and can write the `hex:`/`b64:` form [C4]. `setupterm` returns an error value of
-1 when the database cannot be found [C5a].

### What ncurses needs from libc

From the ncurses 6.6 manual pages:

- Signals. When a function that initialises a SCREEN (`initscr`, `newterm`) first runs, ncurses installs handlers for
  `SIGINT`, `SIGTERM`, `SIGTSTP` and `SIGWINCH` unless the application set its own [C5]. The `SIGINT`/`SIGTERM`
  handler tries to restore the screen on exit and the manual lists limits of doing that from a handler. The `SIGTSTP`
  handler manages job-control stop; on resume it discards pending input with `flushinp` and repaints. The `SIGWINCH`
  handler sets a flag that `wgetch` later tests, returning `KEY_RESIZE`, after which ncurses calls `resizeterm` [C5][C6].
  An application can install its own `SIGWINCH` handler and call `resizeterm` itself [C6].
- Input waiting. By default a curses library tests the input file descriptor with `poll(2)` or `select(2)` while it
  updates the screen, and postpones output if input is pending (`typeahead`) [C7].
- Terminal modes. The terminal driver configuration (raw, cbreak, echo) goes through `termios(3)` on POSIX systems [C7].
- Wide characters. The `ncursesw` configuration provides the `wget_wch`-style interfaces [C8]; whether it calls
  `setlocale` itself, or requires the application to, was not stated in the pages I fetched (`[unverified]`; the
  usual rule that the application calls `setlocale` is from my memory, not a source).
- Environment variables read by the terminfo layer: `TERM`, `TERMINFO`, `TERMINFO_DIRS`, `HOME` (for `~/.terminfo`) [C4].

### NetBSD curses

NetBSD's `terminfo(5)` describes a different database layout: "hashed databases managed by cdb(5) instead of
directories", with `.cdb` appended to each file checked. It reads `TERMINFO` (if it begins with a slash, only that
file is searched), otherwise `$HOME/.terminfo`, then `/usr/share/misc/terminfo` unless `TERMINFO_DIRS` is set. If no
description is found, "a small embedded database" is searched; the page says it holds `ansi`, `dumb`, `vt100`,
`vt220`, `wsvt25` and `xterm` [C8]. The page also warns that `TERMINFO_DIRS` there points to files, not directories,
which differs from ncurses [C8]. An entry can also come from `TERMCAP` or from a `TERMINFO` value that is source text
[C8].

The portable fork `sabotage-linux/netbsd-curses` states [C9]:

- It is a port for Linux built on musl, and "should be possible to use on other POSIX platforms as well with minor
  changes". It supports wide characters "like the ncursesw build variant" and is "close to being a 100%
  feature-complete drop-in replacement" for ncurses (the project's own claim).
- Licence: libcurses, tset and tput are (C) 1980-1994 The Regents of the University of California under the 3-clause
  BSD licence; everything else is (C) 1999-2016 The NetBSD Foundation under the 2-clause BSD licence [C9]. (The
  separate `COPYING`/`LICENSE` file URL returned 404, so I rely on the README's statement; see the list below.)
- Size, from its Table 1 on one system: extracted source 3.3 MB (ncurses 15.8 MB), installed 1.3 MB (15.9 MB),
  `libncursesw.so` 150 KB (346 KB), statically linked `nano` 288 KB (334 KB), build 9 s against 59 s [C9].
- Differences: ncurses structure members are not exposed (programs that poke at them need patches); mouse functions
  are no-op macros; some rarely used functions such as `vidputs()` are missing; only 510 colour pairs; and
  `tgetent()` lives in a separate `libterminfo`, so programs need `-lcurses -lterminfo` [C9].
- Terminal database: compiled by `tic` into one CDB file. "support for about a dozen common TERMs is built-in already";
  more can be added in `libterminfo/genterms` or loaded from `$PREFIX/share/terminfo.cdb` or `$HOME/.terminfo.cdb`.
  The README lists "installation of global terminfo db" as a TODO [C9].
- Build: no configure script, GNU make, variables such as `CC`, `CFLAGS`, `PREFIX`; `all-static` builds static libs
  and programs only [C9].
- Known to build: "the majority of ncurses apps"; patches are cited for Midnight Commander (merged upstream) and for
  Python 2.7.x and 3.6; the README also names `nano` in its size table [C9].

### Programs and libraries without curses

**PDCurses.** The project page describes "A curses library for environments that don't fit the termcap/terminfo
model" and the README says it is public domain, for DOS, OS/2, Windows console, X11 and SDL [C10]. The sabotage README
calls it "unusable on UNIX terminals" (a competitor's claim) [C9]. PDCursesMod, the fork, says the core is in the
public domain with some parts under other licences, and adds flavours including a "VT-like console", a Linux/BSD
framebuffer flavour and a Plan 9 flavour [C11]. It is not a terminfo reader; it draws to a platform back end (inference
from the project descriptions).

**termbox2.** A single-file header library ("slim alternative" to ncurses) with "no dependencies beyond libc"; it has
built-in support for popular terminals when no terminfo database is present [C12]. The header (4,212 lines, measured with
`wc -l`) opens `/dev/tty` in `tb_init()` (it equals `tb_init_file("/dev/tty")`), installs a `SIGWINCH` handler with
`sigaction`, and looks up terminfo in `TERMINFO`, `~/.terminfo`, `TERMINFO_DIRS` and an optional compile-time
`TB_TERMINFO_DIR`; if that fails it uses a `builtin_terms[]` table matched against `TERM` [C13]. Its licence file starts
with the MIT permission text ("Copyright (c) 2021 termbox developers") [C13].

**toybox and busybox: no curses.** The toybox design document says: "we don't use curses when we can simply output ansi
escape sequences and trust all terminal programs written in the past 30 years to be able to support them", and that its
terminal control "outputs ANSI escape sequences" [C14]. `lib/tty.c` (296 lines by `wc -l`) opens `/dev/tty` only as the
last resort when none of stdin, stdout, stderr is a tty; it can query the size by ioctl, by `COLUMNS`/`LINES`, or by an
ANSI probe (`ESC[s ESC[999C ESC[999B ESC[6n ESC[u`) [C15]. Busybox `vi.c` uses the same technique: it sends
`ESC[999;999H ESC[6n` to ask the terminal for its size (when `FEATURE_VI_ASK_TERMINAL` is enabled), defines `ESC`
as `"\033"`, sets raw mode with `termios`, and handles `SIGWINCH`/`SIGTSTP` [C37]. busybox `libbb/lineedit.c` (3,080 lines
by `wc -l`) states its 1986-2003 portion "may safely be consumed by a BSD or GPL license" [C48]; the rest is GPL
v2 like busybox itself [C38].

**termcap API.** `less` can be built to use either the terminfo `setupterm`/`tigetstr`/`tparm` family or the
termcap `tgetent`/`tgetstr`/`tgetnum`/`tgetflag` family; its `screen.c` includes `<curses.h>` and `<term.h>` when
`USE_TERMINFO` is set, and `<termcap.h>` otherwise [C45]. Every capability lookup there takes both names (for example
`ltgetstr("smcup", "ti", ...)`), and the pager first checks environment overrides `LESS_TERMINFO_xx` and
`LESS_TERMCAP_xx` [C45]. If `setupterm`/`tgetent` fails, `less` sets `hardcopy` and runs without cursor addressing [C45].
Vim's `configure.ac` tests for `tgetent` in the libraries `tinfo ncurses termlib termcap curses`, in that order, and
`--with-tlib=library` picks one by hand [C31]. nvi2 links "curses" and "terminfo" libraries (`ncursesw`/`tinfow` with
wide-char support) [C26]. `mg` includes `<term.h>` and calls `setupterm` [C42]. So the smaller interface that `less`
and Vim use is `tgetent`-plus-`tputs`-plus-`tgoto`/`tparm`; nvi and `mg` call terminfo from a curses (or curses-like)
library; I did not find a source saying that nvi can run on termcap alone.

### Curses libraries compared

| Library | Licence | Wide chars | Terminal data | Used by (source) | Size |
|---|---|---|---|---|---|
| ncurses 6.6 | MIT-style X11 [C2] | `--enable-widec` [C1] | files in a tree, hashed DB, `hex:`/`b64:` in `TERMINFO`, or compiled-in fallbacks [C1][C4] | tmux depends on it [C16]; Vim, less, nvi2, mg can use it [C31][C45][C26][C42]; Python `curses` is designed to match its API [C18] | tarball 3.79 MB [C3]; `libncursesw.so` 346 KB in sabotage's build [C9] |
| netbsd-curses (portable) | BSD 3-clause (curses, tset, tput), BSD 2-clause (rest) [C9] | yes, "like ncursesw" [C9] | one CDB file; small embedded set; `~/.terminfo.cdb` [C8][C9] | "the majority of ncurses apps"; Midnight Commander, Python 2.7/3.6 need patches; `nano` [C9] | source 3.3 MB, `.so` 150 KB [C9] |
| PDCurses / PDCursesMod | public domain, some files otherwise [C10][C11] | per port | none (platform back ends) | Windows/DOS/X11/SDL programs [C10] | not stated |
| termbox2 | MIT text [C13] | UTF-8 and grapheme clusters [C12] | terminfo files or built-in table [C13] | its own demos [C12] | 4,212-line header [C13] |
| hard-coded ANSI (toybox, busybox vi, kilo) | 0BSD, GPLv2, BSD-2 | busybox vi: not Unicode-capable [C37] | none | toybox, busybox, kilo [C14][C37][C40] | n/a |
| GNU readline | GPL-3 [C25] | yes | termcap/tinfo (inference) | Python `readline` module (optional) [C22] | not measured |
| libedit | BSD ("Berkeley-style") [C24] | not stated | needs a curses library at link time [C24] | Python `readline` module (optional) [C22] | not measured |

### Python: curses, readline and the REPL

The `curses` module "is designed to match the API of ncurses"; availability is "not Android, not iOS, not WASI", it is an
optional module, and extended colour needs ncurses 6.1 or later [C18]. The `readline` module is "GNU readline interface"
but the underlying library "may be implemented by the editline (libedit) library"; it is not available on Android, iOS
or WASI [C22].

Python 3.13 made a new interactive interpreter the default, "based on code from the PyPy project"; setting
`PYTHON_BASIC_REPL` turns it off [C19]. In the 3.13 sources, `Lib/_pyrepl/curses.py` imports `_curses`, falls back to
the `curses` package, and finally to `_minimal_curses.py`, a ctypes-based minimal module [C20]. The terminal console
(`unix_console.py`) calls `curses.setupterm`, `curses.tigetstr` and `curses.tparm`, installs a `SIGWINCH` handler with
`signal.signal`, and says that its own Python function replaces `tputs` because the curses one "can't really be wrapped
in a sane manner" [C20]. `_pyrepl/main.py` disables the new REPL and uses the basic one when stdin is not a tty or its
`check()` reports a problem, printing "warning: can't use pyrepl" unless `PYTHON_BASIC_REPL` is set [C20].

In the 3.14 tree, `Lib/_pyrepl/curses.py` and `_minimal_curses.py` are gone and `terminfo.py` is new: its first line is
"Pure Python curses-like terminal capability queries", it parses the compiled terminfo binary format (both the 16-bit
and 32-bit magic numbers), and it follows ncurses' search order: `TERMINFO`, `~/.terminfo`, `TERMINFO_DIRS`, then fixed
directories (`/etc/terminfo`, `/lib/terminfo`, `/usr/lib/terminfo`, `/usr/share/terminfo`,
`/usr/share/lib/terminfo`, `/usr/share/misc/terminfo`, `/usr/local/lib/terminfo`, `/usr/local/share/terminfo`) [C21].
Its `unix_console.py` now calls `terminfo.TermInfo`, `terminfo.tparm` [C21]. So from 3.14 the REPL no longer needs the
`_curses` C module; it needs a compiled terminfo file in directory-tree form on disk (it does not handle `hex:`
`TERMINFO` values, from what the file's search code shows) [C21].

Python's `site.py` sets up readline history automatically: `gethistoryfile()` returns `PYTHON_HISTORY` if set, else
`~/.python_history` [C23]. That is a home-directory file the command line never names.

GNU readline is under "GNU General Public License, version 3", per the project's own page [C25]. libedit (Jess
Thrysoee's port of NetBSD's) is described as a "Berkeley-style licensed" command-line editor with "line editing, history,
and tokenization functions", and the page says programs "should link libcurses, as well as libedit" [C24].

## Editors and pagers

### Editors and pagers compared

| Program | Licence (file fetched) | Terminal interface | Other libc/system needs | Build options that shrink it | Size / activity |
|---|---|---|---|---|---|
| nvi2 2.2.2 | BSD 3-clause (UCB, Bostic, Verdoolaege, Yuan) [C26] | curses + terminfo libs [C26] | Berkeley DB 1 `dbopen` in libc, `iconv`, `/var/tmp/vi.recover/` mode 41777, sendmail for recovery mail [C26][C29] | `USE_WIDECHAR`, `USE_ICONV` CMake options [C26] | version 2.2.2 dated 2025-10-08; repository pushed 2026-10-08 (GitHub API) [C26] |
| nvi 1.81.6 | same BSD family [C27] | curses | bundles "a stripped-down, replacement db(3) library" [C27] | not examined | version dated 2007-11-18; still packaged in Debian [C27] |
| OpenBSD vi | BSD-style (OpenBSD tree) | curses | `dbopen`, `flock`, `/tmp/vi.XXXXXXXXXX`, sendmail [C29] | `secure` option [C28] | `exf.c` revision of 2026-06-19 [C29] |
| Vim 9.2 | Vim licence, charityware, GPL-compatible [C30] | termcap/terminfo or builtin [C31][C32] | many (see below) | `--with-features=tiny` [C31] | actively developed [C30] |
| busybox vi | GPL v2 only [C37][C38] | none: raw ANSI | `termios`, `system()` | `FEATURE_VI_*` config symbols; "vi (26 kb)" in its help text [C37] | part of busybox |
| toybox vi | 0BSD (toybox licence) [C39] | none: ANSI | `rename` for saves | `default n`, in `toys/pending/` [C39] | 1,871 lines (`wc -l`) |
| kilo | BSD 2-clause [C40] | none: VT100 escapes | `termios`, `ioctl(TIOCGWINSZ)`, `SIGWINCH` | n/a | 1,308 lines by `wc -l` (the file says under 1000 by `cloc`) [C40] |
| mg | public domain [C42] | terminfo (`setupterm`) | `libbsd` on Linux, mkstemp | not examined | active OpenBSD tree [C41][C42] |
| ed (OpenBSD) | BSD 2-clause (UCB-style, two clauses in file) [C43] | none | `system`, `popen`, `SIGHUP` | n/a | n/a |
| less 7xx | dual: GPL-3 or "Less License" (2-clause BSD text) [C44] | termcap or terminfo [C45] | `termios`, `ioctl` | `LESSSECURE` at run time [C46] | copyright to 2026 [C44] |
| toybox more | 0BSD [C47] | ANSI, `terminal_size` | opens `/dev/tty` | `default n`, `toys/pending/` [C47] | n/a |

### nvi

nvi 1.79 was the last version by Keith Bostic. After it, Sven Verdoolaege added iconv support and DB3 locking, and
Jun-ichiro Hagino added file-encoding detection in "nvi-m17n". Zhihao Yuan's nvi2 added multibyte encodings "onto DB1"
with Google Summer of Code money, and the project now has contributors from OpenBSD and FreeBSD [C26]. The licence file
is the three-clause BSD text with copyright of UC Berkeley, Keith Bostic, Sven Verdoolaege and Zhihao Yuan [C26]. The
README places the curses interface in a `cl` directory, calls it "Vi interface to the curses(3) library", and keeps a
modified regex library with wide-character support in `regex` [C26].

The DB dependency: nvi stores the edit buffer in a record-number Berkeley DB (`DB_RECNO`) opened on a temporary file.
In OpenBSD's `exf.c`, `dbopen(... DB_RECNO ...)` is called and the temp name is `/tmp/vi.XXXXXXXXXX` created with
`mkstemp` [C29]. nvi2 handles it by requiring "Berkeley DB1 in libc" (the `INSTALL.md` prerequisites) and its CMake file
links `db1` if `dbopen` is not in libc [C26]. nvi 1.81.6 (2007) instead ships its own "stripped-down, replacement db(3)
library" in a `db` directory [C27]. The `INSTALL.md` also demands `/var/tmp/vi.recover/` with mode 41777 (sticky,
world-writable) [C26], and nvi2's CMake sets the preserve path `/var/tmp/vi.recover/` [C26]. nvi2's CMake needs
`libbsd` where `strlcpy` or `getprogname` is missing [C26].

### Vim

Vim's licence text is in `uganda.txt` and the `LICENSE` file. It states that unmodified copies may be distributed freely
if the licence text is included; modified versions must meet four conditions (including making changes available and
contact information) [C30]. The manual says "Vim is Charityware" and asks for donations for children in Uganda via the
Kuwasha charity, and says Richard Stallman said the licence is GPL compatible, but linking with a GPL library limits
further distribution to the GPL [C30]. The documentation falls under the Open Publication License [C30]. This is not one
of the permissive licences on the project's list (inference).

Feature sets: `configure.ac` takes `--with-features=TYPE` with `tiny`, `normal` or `huge` (default huge); `small` and
`big` are aliases [C31]. The feature list in `various.txt` marks each feature T (tiny), N (normal), H (huge): `+multi_byte`
is always present (T), while `+eval`, `+syntax`, `+viminfo` and `+persistent_undo` are N (not in tiny) [C33]. `+terminal`
is "m" (manual) [C33]. Terminal handling: on Unix "the terminfo database or termcap file is used" and the choice is made
at configure time; builtin termcaps for several terminals exist, and "since patch 9.0.0280 there is no difference
between Vim versions" in which builtin terminals are included. The `ttybuiltin` option controls which source is tried
first [C32]. `--with-tlib=library` and the list `tinfo ncurses termlib termcap curses` select the library [C31].

### Small editors

busybox `vi.c` is "tiny vi.c: A small 'vi' clone" (2000, 2001 Sterling Huxley), GPLv2 or later in the header, but
busybox as a whole is "version 2 ... the only version" (its `LICENSE`) [C37][C38]. It has configuration symbols for
colon commands, yank/mark, search, regex search, undo, window resize and others; its Kconfig text says it is not
Unicode-capable and shows 8-bit characters only if `FEATURE_VI_8BIT` is on [C37]. A "TODO" list at the top still names
`./.exrc` and `:r !cmd` [C37].

toybox `toys/pending/vi.c` (Rob Landley 2015, Jarno Makipaa 2019) has `default n`, so it is off unless chosen, and sits in
`pending`; its help text lists a short command set (ex commands `q`, `q!`, `w [NAME]`, `wq`, motions `0 b e G H h j k L l M w $
f F`, and so on) [C39]. The toybox licence file is the ISC-style text "Permission to use, copy, modify, and/or distribute
this software for any purpose with or without fee is hereby granted" (0BSD) [C47].

kilo (Salvatore Sanfilippo, BSD 2-clause) says it "does not depend on libcurses, directly emits VT100 escapes". It
has about 1,300 lines by my `wc -l`; the file header says under 1,000 "as counted by cloc" [C40]. What it lacks (from
the source): it keeps `termios` raw mode and `SIGWINCH`, but the help line shows only "Ctrl-S = save | Ctrl-Q = quit |
Ctrl-F = find"; I found no undo, no UTF-8 handling, and no `grep` hits for either in the file (inference from absence)
[C40]. It is a modeless editor, not vi.

`mg` is "Public Domain EMACS style editor" per its README and the `pathnames.h` header says "This file is in the public
domain" [C42]; the OpenBSD manual says it is "intended to be a small, fast, and portable" editor [C41]. The Linux port needs
`libbsd` and an ncurses development package [C42]. `ed` from OpenBSD carries a two-clause BSD notice (Andrew Moore,
Talke Studio) [C43]; toybox has no `ed` and no `less` (I checked `toys/pending`, `toys/posix`, `toys/other` for
`ed.c`, `less.c`, `more.c`, `vi.c`: only `more.c` and `vi.c`, both in `pending`) [C47]. `nano` is GPL-3 (from memory, not
fetched; `[unverified]`).

### less and more

The `less` README says it may be distributed under either the GNU General Public License (version 3 or later, text in
`COPYING`) or "The Less License" in `LICENSE`; the file `LICENSE` is a two-clause BSD text ("Redistributions of source
code must retain...", "Redistributions in binary form must reproduce...") with "Copyright (C) 1984-2026 Mark Nudelman"
[C44]. It is therefore usable under a permissive licence (inference from the text). The manual says "Less uses termcap
(or terminfo on some systems)" and has limited support for hardcopy terminals [C46]. Capabilities used, from `screen.c`:
`smcup`/`rmcup` (alternate screen), `el`, `ed`, `clear`, `cup`, `home`, `ri`, the `sgr0` family for highlighting, the flags
`am`, `xenl`, `da`, `db`, `hc`, `nrrmc`, the sizes `lines` and `cols`, and a long list of key capabilities (`kcuf1`,
`kcub1`, `kcuu1`, `kcud1`, `kpp`, `knp`, `khome`, `kend`, `kf1`, `kdch1` and shifted/ctrl variants) [C45]. If TERM is
unknown it still works as a hardcopy device [C45].

toybox `more.c` (Bilal Qureshi 2013) is `default n`, in `pending`, opens `/dev/tty` with `fopen` when stdout is a tty,
and sets raw mode with `tcsetattr`; it calls `terminal_size` [C47].

### Files an editor or pager touches unasked

This list is the main output for the capability design. Each entry is from a fetched page.

**less** (command line names the files to view) [C46]:
- `:e [filename]` examines any file; `:n`, `:p`, `:x`, `:d` move through the list of names given on the command line,
  and `%`/`#` in a name stand for the current or previous file [C46].
- `!cmd` runs a shell command, `|` pipes text to a command, `s filename` saves piped input to a named file [C46].
- `v` starts an editor taken from `VISUAL`, then `EDITOR`, else `vi` (edit the current file) [C46].
- `-o file` / `-O file` and the in-program `-o` log standard-input data to a file (only for pipes) [C46].
- `LESSOPEN`/`LESSCLOSE`: an input preprocessor command run on each file name; it may create a replacement file [C46].
- History file: `LESSHISTFILE`, default on Linux and Unix `$XDG_STATE_HOME/lesshst`, or `$HOME/.local/state/lesshst`,
  or `$XDG_DATA_HOME/lesshst`, or `$HOME/.lesshst`; `-` or `/dev/null` disables it; it stores search and shell commands
  and marks [C46].
- Key files: `$XDG_CONFIG_HOME/lesskey`, `$HOME/.config/lesskey` or `$HOME/.lesskey`, and a system file
  `/usr/local/etc/syslesskey`; variables `LESSKEYIN`, `LESSKEYIN_SYSTEM` [C46].
- Tags: `-t` reads a tags file; `LESSGLOBALTAGS` names a `global(1)`-compatible command [C46].
- Environment: `LESS` options, `TERM`, `LESS_TERMCAP_xx` / `LESS_TERMINFO_xx` [C46][C45].
- `LESSSECURE=1` disables the edit, examine, glob, history, lesskey, lessopen, logfile, osc8, pipe, shell, stop and tags
  features; `LESSSECURE_ALLOW` re-enables selected ones [C46]. This is the one built-in switch among the programs here
  that matches a capability stance.

**vim** [C34][C35][C36]:
- Swap file: next to the edited file, same name with a leading `.` and the extension `.swp` on Unix; if it exists, `.swo`,
  `.swn` and so on; deleted when Vim stops editing. The `directory` option lists where swap files may go, default
  `.,~/tmp,/var/tmp,/tmp` on Unix; an empty value disables swap files [C36][C35].
- Backup: `backup` (off) and `writebackup` (on): before overwriting, Vim writes a backup; it is removed after success unless
  `backup` is on. `backupcopy` chooses "yes" (copy and overwrite the original), "no" (rename the original and write a
  new file) or "auto". `backupdir` default on Unix `.,~/tmp,~/` [C35].
- Undo files: `undofile` (off by default), `undodir` default `.` [C35].
- Info file: `$HOME/.viminfo` on Unix, set by `-i`, `viminfofile`, or the `n` flag; `-i NONE` disables it; absent in
  the tiny feature set [C34][C33].
- Startup files: `$HOME/.vimrc`, `$HOME/.vim/vimrc` or `$XDG_CONFIG_HOME/vim/vimrc`, then `$VIMINIT`/`$EXINIT`,
  then `.exrc` and `.vimrc` in the current directory when `exrc` is on (default off) [C34][C35]. `$VIMRUNTIME` files
  are also read (syntax, filetype) (inference).
- Other files by name: `:e`, `:n`, `:w file`, `:r file`, `:!cmd` (shell), `:r !cmd`, `:w !cmd`; filters with `shelltemp`
  (default on in Vim) use temporary files [C35][C33].
- Restricted mode `-Z` (also `rvim`) disables shell use, `system()`, filtering, `delete()`, `rename()`, `mkdir()`, interfaces;
  the manual notes "the user may still find a loophole" [C34].

**nvi / OpenBSD vi** [C28][C29]:
- Recovery files in `recdir`, default `/tmp/vi.recover` in the OpenBSD page (nvi2 uses `/var/tmp/vi.recover/`) [C28][C26].
- Temporary file `/tmp/vi.XXXXXXXXXX` for the record database; the manual lists `/tmp` under FILES [C28][C29].
- Startup files: `/etc/vi.exrc`, `$HOME/.nexrc` then `$HOME/.exrc`, `$NEXINIT`/`$EXINIT`, and local `.nexrc`/`.exrc`
  only if the `exrc` option is on (each must be owned by the user or root and not group-writable) [C28].
- `:e`, `:r`, `:w`, `:n`, `:!`; the `secure` option "turns off all access to external programs" and can't be unset;
  `lock` takes an exclusive `flock` on the edited file; `SIGALRM` drives periodic backups; recovery mail goes through
  `sendmail` [C28][C29].

**busybox vi**: writes by opening the file for writing without `O_TRUNC`, writing, then `ftruncate` (not by rename); it
runs `system()` for `:!` when exec is enabled; reads `$EXINIT`, else `$HOME/.exrc` (must belong to the user and not be
group/other-writable); expands `%` and `#` in colon commands [C37].

**toybox vi**: writes by creating `FILENAME.swp` next to the file and then `rename()` onto the target; reads `-s SCRIPT`
and `-c` command files [C39]. This means it needs create permission on the directory, not only on the file (inference).

**kilo**: opens only the single file named; saves in place with `open(O_RDWR|O_CREAT)`, `ftruncate`, one `write` [C40].
No other file is touched (from the source).

**mg**: backup `file~` made by copying (not renaming) the original, optionally moved to `~/.mg.d`; startup file `~/.mg` and
`~/.mg-TERM`; temporary file through `mkstemp`; backups in `/tmp` can be left with `leave-tmpdir-backups` [C41][C42].

**ed (OpenBSD)**: on `SIGHUP` writes `ed.hup` in the current directory or else `$HOME/ed.hup`; `!cmd` runs a shell; `r !cmd`
and `w !cmd` use `popen` [C43].

**Python REPL**: `~/.python_history` (or `PYTHON_HISTORY`), and `PYTHONSTARTUP` file [C23][C20].

## The userland tool set

Scope: which everyday command-line tools Zoraya has today, which it lacks, what blocks each, and which permissively
licensed sources could supply the rest. Part 1 is measured in the repository (commands listed under each claim).
Part 2 is read from upstream sources fetched on 2026-11-13. "Finished" and "pending" are toybox's own words: a
command in `toys/pending/` is, per its README, unfinished or in need of review [U18].

### The userland today

**What the tier file contains.** `tools/posix-surface/results/applets-by-tier.tsv` has six rows, tier, program,
applet list [U1]. I counted names per row with `awk -F'\t' '{n=split($3,a," "); print $1,$2,n}'`:

| Tier | toybox applets | busybox applets |
|---|---|---|
| T0 | 4 (`cat ls sleep touch`) | 1 (`tr`) |
| T1 | 9 (`cat head ls mv rm sleep touch true uname`) | 2 (`diff tr`) |
| T2 | 31 | 5 (`awk diff expr gzip tr`) |

The rows are cumulative: each higher row repeats the lower ones, so the file names 31 distinct toybox applets and
5 distinct busybox applets in all [U1]. The tiers come from TASK-02: T0 is dash plus toybox applets (an interactive
session), T1 adds make, tcc and busybox `diff` (self-hosting C), T2 adds running autoconf `configure` (workload W4),
T3 is `clang`+`lld` as a size reference only [U6, U7]. ADR-0005 accepted T1 and left T2 until the cost of
`snapshot_process` on W4 is measured [U4, U7].

**What is built and run today.** `ports/Makefile` line 321 sets `TOYBOX_APPLETS := cat head ls mv rm sleep touch true
uname sort wc yes` (12 names; `wc -w` on that line gives 14 words, i.e. the 12 plus the variable name and `:=`) and
line 348 sets `BUSYBOX_APPLETS := tr diff` [U2]. toybox is 0.8.15 (0BSD), configured with `allnoconfig` plus those
names switched on; busybox is 1.38.0 (GPL-2.0-only) [U2, U3]. The tier file lists nine toybox applets at T1; `sort`
and `wc` are T2 rows that ADR-0144 added to the build for workload W10, and `yes` is in no TSV row [U2, U3].
`design.md` records the whole toybox port as one patch to `lib/portability.[ch]` plus two extra libc names (`atoll`,
`getdelim`) [U4]. Tests: `tests/posix/rootfs/tests/` has `w1.sh` to `w10.sh` and `w9-check.sh`; a grep of those
scripts for the applet names finds `wc`, `ls`, `true`, `diff` and `grep`, and no other applet name by word match
(`grep -rlw` per name); `docs/tour.md` shows `wc -l notes.txt` working from toybox [U8]. So only a handful of the 14
built applets are exercised by name in committed scripts (inference from that grep; I did not run the tests).

**Zoraya's own commands.** `user/describe/commands/` has descriptions for `cat cp date fetch find head ls mkdir mv net
ping random rm search sleep stat sync` and `user/describe/builtins/` for `cd`, `kill`, `top`, `jobs`, `pwd` and others;
`user/describe/posix/` has only `cryptothreads curl dash make tcc tlsthreads wget zfetch` (`ls` of those directories).
No toybox applet has a description yet: ADR-0153 lists "which descriptions to ship for toybox applets" as open [U9].
The shell only turns operands into grants for programs that have a description (CLAUDE.md of the repository; brief).
(inference) A toybox `sort` run from `zosh` today therefore gets what the line itself grants and nothing from a
per-applet description.

**The gap, by applet.** 31 toybox T2 applets minus 11 built (the 9 T1 ones plus `sort`, `wc`) leaves 20 not built;
busybox T2 minus `tr`, `diff` leaves `awk`, `expr`, `gzip`. The probe `make -f ports/Makefile t2-probe` links the T2
applets against `libc.a` and records the undefined names [U2]. `design.md` and `docs/backlog.md` report 20 names
for toybox's T2 set (`regcomp regexec regerror getline mkdtemp getgrgid getgrnam getgroups getgrouplist link lchown
mknod dirname dprintf getentropy gethostbyname hstrerror __h_errno_location sethostname towlower`) and 9 for busybox
`awk expr gzip` (`regcomp regexec regerror regfree rand srand strncasecmp fileno_unlocked fputs_unlocked`) [U4, U5].
I did not re-run the probe (the brief forbids `make`). Since that report, ADR-0310 added `regcomp`, `regexec`,
`regerror`, `regfree`, `fnmatch`, `rand`, `srand` and `strncat` to `libc.a` for curl and wget, so those names are
likely no longer missing [U4, U8]; the others in the list are not in ADR-0310's additions (inference; unverified by
a link). The architect's order is: image cache, then the missing names, then W4 measured for real [U5].

The per-applet column below comes from my `grep -F` of each applet's own source file in the toybox 0.8.15 tarball
(sha256 `44e2bab4...574b375e`, the hash in `ports/sources.lock`) [U10]; names that live in `lib/` hit every applet
that links the shared code and are not attributed to one.

| Applet | Tier | Built today | Needs what libc.a lacked (own-source grep, inference) |
|---|---|---|---|
| arch, echo, env, mkdir, rmdir, chmod, basename | T2 | n | none found in own source; only shared `lib/` names |
| cp (and `install`, `mv` variants) | T2 | n (`mv` y) | `link`, `lchown`, `mknod`, `utimensat` |
| cpio | T2 | n | `getline`, `link`, `lchown`, `mknod`, `dprintf`; `fork` (the `xfork` applet, I-15 in stage 6 inputs) [U11] |
| dirname | T2 | n | `dirname` |
| find | T2 | n | `getgrgid`, `getpwuid`, `dirname`, `fnmatch`; `-exec` runs a child |
| grep | T2 | n | `regcomp`, `regexec`, `fnmatch` |
| sed | T2 | n | `regcomp`, `regexec`, `getline`, `dprintf` |
| hostname | T2 | n | `gethostbyname`, `sethostname` |
| id | T2 | n | `getgrgid`, `getgroups`, `getgrouplist`, `getpwnam` |
| ln | T2 | n | `link`, `dirname` |
| mktemp | T2 | n | `mkdtemp` |
| tar | T2 | n | `getpwnam`, `getgrgid`, `getgrnam`, `link`, `lchown`, `mknod`, `dprintf`, `fnmatch`, `utimensat`, a child for compression |
| xargs | T2 | n | none in own source; starts a child through `xexec` |
| busybox awk | T2 | n | `regcomp` family, `rand`, `srand`, `strncasecmp`, `*_unlocked` [U4] |
| busybox expr | T2 | n | same busybox list [U4] |
| busybox gzip | T2 | n | same busybox list [U4] |
| cat, head, ls, mv, rm, sleep, touch, true, uname, sort, wc; busybox tr, diff | T0-T2 | y | none |

In this grep, 13 toybox applets use `regcomp`, 18 use `getpwnam`, 13 `getpwuid`, 7 `getgrnam`, 5 `fnmatch`, 5
`utimensat`, 16 call `fork(` (the daemons and init among them), and none uses `glob(`, `nftw`, `fts_` or `posix_spawn`
(`grep -rlF` per name over `toys/` in the tarball) [U10]. toybox walks trees with its own `lib/dirtree.c`
(227 lines, `wc -l`), built on `openat` and `fstatat`, not on `fts` [U10]. Users and groups appear in 18 applets of
that count through `getpwnam`/`getpwuid`; Zoraya has no users, so what `ls -l`, `id`, `chown` and `tar` should print
is a design question this file does not answer (`getpwuid` is in the T2 list of ADR-0210 [U4]).

### toybox upstream

**Licence, size, aim.** toybox is licensed 0BSD, "a public domain equivalent"; the design page says this is why it
usually cannot take outside code, except public-domain-style code such as the xz decompressor [U12]. The LICENSE file
in the repository is the 0BSD text, copyright 2006 and 2019 Rob Landley [U13]. The FAQ gives the history: Landley
handed busybox maintenance on and restarted from scratch in 2006 after a licensing argument, and relaunched toybox in
2011 with the goal of making Android self-hosting [U14]. The status page describes release 0.8.15 as current [U15].

**What is finished, what is pending.** Directory listings from the GitHub API for `toys/<dir>` at the tag 0.8.15
and at `master` list the same `.c` files, so what follows applies to both [U16]. Counting `.c` files: `posix` 70,
`other` 95, `lsb` 14, `pending` 52 (one file can define several commands: `gzip.c` defines `gzip`, `gunzip`, `zcat`;
`ps.c` defines `ps`, `top`, `iotop`, `pgrep`, `pkill`) [U16, U17]. The status page's "Completed" block has 237 entries
(my line count of that block; it mixes markers) and "Partially implemented (in toys/pending)" 97 [U15]. The pending
README says everything there "defaults to n in defconfig" and that the directory should go away before 1.0 [U18].
The pending files are:

`arp arping awk bc bootchartd brctl chsh crond crontab csplit dhcp dhcp6 dhcpd diff dumpleases expr fdisk fsck
getfattr getty git groupadd groupdel hexdump init ip ipcrm ipcs klogd last lsof man mdev modprobe more route sh strace
stty sulogin syslogd tcpsvd telnet telnetd tftp tftpd tr traceroute useradd userdel vi xzcat` [U16].

The tools a user expects daily and that are only here: `awk` (4,579 lines, by Ray Gardner, written to the POSIX 2024
awk page, with regex `RS` and gawk-style bit functions listed among its deviations) [U19]; `vi` (1,871 lines; ex-mode commands, `-s` script) [U19]; `diff` (934 lines, unified output, `-r`); `tr` (239);
`expr` (251); `bc` (5,953); `sh` (5,466, a bash-compatible shell, itself pending); `man` (213); `more` (172);
`hexdump`; `xzcat` (2,651, Lasse Collin's decoder placed in the public domain) [U19]. Line counts are `wc -l` of the
fetched `master` files.

The roadmap states the dependency openly: to rebuild itself without busybox, toybox still needs busybox's `awk diff
expr fdisk gzip less route sh tr unxz vi xzcat`, and "all of those except awk and less" have partial implementations
in pending [U20]. The status page's own to-do list for POSIX adds `at awk bc csplit diff expr fuser join man more
newgrp sh stty tabs tput tr unexpand vi` [U15]. (That page also marks `gzip` "not started", but `toys/lsb/gzip.c` exists
at the 0.8.15 tag with `gzip`, `gunzip` and `zcat`, so the page lags the source [U15, U21].) `less` and `ed` are absent
from every directory and are on the page's "not started" list [U15, U16].

**git is not git.** `toys/pending/git.c` (656 lines) defines `gitclone`, `gitinit`, `gitremote`, `gitfetch` and
`gitcheckout`, each described as "A minimal git" operation; the config entry `GITCOMPAT` enables "git compatible
repos instead of minimal clone downloader" [U19]. It is not a git client in the daily sense (no `commit`, `log`,
`diff`, `push`).

**Compression in toybox.** `gzip.c` can use zlib (`CFG_TOYBOX_LIBZ`) or the built-in `lib/deflate.c`, a 540-line file
with an inflate decoder and a compressor [U21]. `tar` (1,221 lines) takes `-z -j -J -Z` (gzip, bzip2, xz, zstd) and
reaches them by running the matching external `zcat`/`bzcat`/`xzcat`/`zstdcat` or `gzip` child; it detects gzip and
bzip signatures when reading [U22]. `bzcat` is finished in `other/`; there is no bzip2 compressor, no `unzip` and no
`zip` in any directory [U16, U15 (`.bzip2.` and `unzip` on the not-started list)]. `xzcat` decodes only [U19].
`patch` (485 lines) says in its header that it applies only unified diffs [U22]. `find`, `grep`, `sed`, `xargs`,
`tar`, `cpio`, `ps` are finished and in `toys/posix` [U16].

**Portability claims.** The design page says toybox needs no libraries beyond libc, is tested with gcc and llvm on
glibc, musl and bionic plus occasional FreeBSD and macOS, and wants the `openat()` family [U12]. The roadmap lists
POSIX-2024 (SUSv5), the frozen Linux Standard Base 4.1, Android's toolbox and the Linux man pages as targets; the page
says LSB 4.1's list is mostly relevant where it names tools POSIX does not (`bc` and `seq` among them) [U20]. The same page notes
POSIX dropped `tar` and `cpio` while keeping `cksum` and `uucp` [U20].

### busybox

busybox's applet page lists about 310 names (my regex count of the comma list under "Currently available applets",
`awk`, `ash`, `hush`, `diff`, `ed`, `less`, `man`, `patch`, `tar`, `top`, `vi`, `bzip2`, `gzip`, `unzip`-family
tools) [U23]. It has everything toybox lacks in the daily list (`awk`, `vi`, `less`, `ash`, `bzip2`, `ed`).
Licence: the licence page says GPL version 2, and the `LICENSE` file at the repository root says version 2 is "the
only version" it may be distributed under [U24]. Zoraya's `docs/dependencies.md` already records it that way [U3].
The licence rule in the brief (permissive only for shipped code) excludes it as a general userland; it stays only
for `tr` and `diff` as a stopgap (inference from [U3] and the brief). toybox's FAQ presents the licence as the reason
the author left busybox [U14].

### BSD and other permissive sources

**Chimera Linux and chimerautils.** Chimera's about page says its userland is based on FreeBSD core tools replacing
coreutils, findutils, diffutils, sed and grep, chosen for "high quality code and solid feature set", and that the
project "is not anti-GNU/GPL" and the choice is "primarily technical" [U25]. `chimerautils` (a fork of
`dcantrell/bsdutils`) states it replaces coreutils, findutils, diffutils, grep, sed, ed, m4, patch, gzip and gawk, and
also ships `vi` (nvi), `sh`, `nc`, `telnet`, `fetch`, `compress`, `jot` and others [U26]. Its tree has
`src.freebsd/{coreutils,diffutils,findutils,grep,gzip,sed,awk,nvi,patch,sh,ed,m4,miscutils}`; coreutils alone has 70
entries (`ls` of the GitHub directory) including `ls cp mv rm dd df du sort tr tail tee test stat expr` [U26]. It
needs `meson`, `ninja`, `flex`, `byacc`/`bison`, and `libxo` as build dependencies, with `ncurses`, `libedit` and
OpenSSL's `libcrypto` optional [U26]. The licence statement: most code is FreeBSD's, "mostly BSD-2-Clause", the
custom tools the same terms [U27]. This is a port to Linux with a compat library (`src.compat`: `err.c reallocf.c
strlfuncs.c strtonum.c signames.c b64.c`); its tools use `fts`, regexes and `getpw*` as FreeBSD does [U26] (the use of
`fts` is my inference from FreeBSD's `ls`/`find`/`cp`; I did not grep the tree).

**sbase and ubase (suckless).** sbase is "a collection of unix tools that are inherently portable", MIT licensed
(`LICENSE`, copyright Connor Lane Smith, Dimitris Papastamos, Laslo Hunhold), builds with tcc among other compilers,
and can be built as one `sbase-box` binary [U28, U29]. Its README table lists about 94 utilities (my regex over the
table rows). It has `ls cp mv rm cat head tail sort uniq cut tr wc tee grep xargs od dd du date test expr printf
sleep tar` and more; `sed` carries no "finished" mark, `find` is not marked finished or audited, `ed` is not marked
finished, `tar` is marked non-POSIX; it has no `awk`, `diff`, `patch`, `vi`, `less`, `gzip` [U28]. ubase holds the
Linux-only half [U28].

**Other ports.** `oksh` is a self-contained portable OpenBSD `ksh`, building on Linux (glibc and musl), BSDs and
others [U30]; its `LEGAL` file says the vast majority of the pdksh code is public domain [U30]. A search of GitHub
found `Duncaen/lobase` and `ataraxialinux/lobase`, both described as "Port of the OpenBSD userland to Linux"; I did
not read their trees or licences (`weak`) [U31]. Plan 9 from User Space is under the MIT licence since the Plan 9
Foundation relicensed Plan 9 in March 2021; the exceptions are directories with their own LICENSE (bzip2, fonts) [U32].
9base (suckless) repackages the original Plan 9 commands including `awk`, `bc`, `dc`, `diff`, `ed`, `cmp`, `du`, `dd`
[U32]. The Heirloom Toolchest page says its tools derive from original Unix code released by Caldera and Sun, with
"more than 100" utilities including `awk`/`nawk`, `bc`, `cpio`, `diff`, `ed`, `grep`, `pax`, `sed`, `tar`; it does not
state a licence on the page I fetched, so its terms are `[unverified]` here [U33].

### awk

| Candidate | Licence | Size and build | Needs from libc | Source |
|---|---|---|---|---|
| one-true-awk (second edition) | Lucent Technologies permission notice: use, copy, modify, distribute "for any purpose and without fee"; copyright notice must be kept; bars the Lucent name in advertising without permission | about 7,500 lines of C and yacc (see note); `makefile` uses `bison -d` and builds `maketab` with the host compiler | has its own regex code in `b.c` (its includes are ctype, limits, stdio, string, stdlib, `awk.h`; no `regex.h`); `run.c` calls `popen`, `system`, `wctomb`, `towlower` | [U34, U35, U36] |
| toybox `awk` (pending) | 0BSD | 4,579 lines, one C file | POSIX regex (`regcomp`), toybox lib | [U19] |
| busybox `awk` | GPL-2.0-only | in the busybox tree | `regcomp` family, `rand`, `srand`, ... [U4] | [U23, U24] |
| mawk | GPL-2 (`COPYING` is GPL "Version 2, June 1991") | not examined | not examined | [U37] |
| gawk | GPL-3 `[unverified]` (fetch of its COPYING returned an HTML wrapper) | not examined | not examined | none |

Notes: the one-true-awk README says the second edition (Aho, Kernighan, Weinberger) handles UTF-8 and CSV input,
counts Unicode code points in string functions, and accepts UTF-8 in literals and regular expressions [U34]. The
repository has no pre-generated `awkgram.tab.c` in the root listing; the README says `yacc` or `bison` is needed and
the makefile defaults to `bison -d` [U34, U35]. The generated parser, not the licence, is the build obstacle: the
generated files could be committed once (inference). The line counts for the awk sources, from `wc -l` on `master`:
`b.c` 1,603, `lex.c` 630, `lib.c` 918, `main.c` 292, `parse.c` 285, `run.c` 2,621, `tran.c` 650, `awkgram.y` 497
(the order of my loop was `b.c lex.c lib.c main.c parse.c run.c tran.c awkgram.y`), about 7,500 lines in all [U34].
The Lucent notice is permissive but is not one of MIT, BSD, ISC, Zlib or Apache; it needs a decision on whether it
counts under the brief's licence rule (FreeBSD's awk ships the same text, `src.freebsd/awk/LICENSE` in chimerautils)
[U26, U35].

### Compression and archives

| What | Licence | Notes | Source |
|---|---|---|---|
| zlib 1.3.1 (in the tree for curl/wget) | zlib licence | zlib.net now lists 1.4 as current | [U3, U38] |
| toybox `lib/deflate.c` + `gzip.c` | 0BSD | 540 lines; gzip, gunzip, zcat; no zlib needed | [U21] |
| miniz | MIT | single-file deflate/inflate with optional zip API; inflater is a ~550-line coroutine file | [U39] |
| xz-utils: liblzma and `xz` | 0BSD (`COPYING`: liblzma and the command-line tools); scripts `xzgrep`, `xzdiff`, `xzless`, `xzmore` are GPLv2+; autotools files GPL; `getopt_long` fallback LGPLv2.1+ | full encoder and decoder | [U40] |
| XZ Embedded | 0BSD; "older versions are in the public domain" | decompressor only | [U41] |
| zstd | BSD or GPLv2 (dual, choose) | reference C library | [U42] |
| bzip2 1.0.8 | "BSD-style" | the page also calls it patent-free to the best of the author's knowledge | [U43] |
| libarchive: `bsdtar`, `bsdcpio`, `bsdcat`, `bsdunzip` | BSD-2-Clause for most files; some files 3-clause UC; public-domain `archive_parse_date.c`; build scripts "widely varying" | reads tar, pax, cpio, zip, ar, ISO and more; handles gzip, bzip2, lzip, xz, lzma, compress through libraries it finds | [U44] |

xz supply-chain note: the xz project's page states that XZ Utils 5.6.0 (2024-02-24) and 5.6.1 (2024-03-09) release
tarballs contain a backdoor inserted by a malicious co-maintainer, found by Andres Freund and made public on
2024-03-29 as CVE-2024-3094; the maintainer's page says tarballs created and signed by "Jia Tan" were the affected
ones [U45]. The attack worked through the release tarball's build scripts (autotools), not through the C sources in
the git tree (my reading of the project's statement that "tarballs" contain it; I did not read the technical
analysis). (inference) A ported xz built from a git checkout with a hand-written build avoids the tarball path.

Archive formats: POSIX.1-2024 has no `tar` page (the URL for `utilities/tar.html` returned 404) and no `cpio`; it
specifies `pax` with `-x` formats `cpio`, `pax` and `ustar` [U46]. toybox's own roadmap complains about this [U20].
(inference) Interchange with the outside world needs ustar plus pax extended headers (long names, large sizes) and
gzip; GNU tar's extra formats appear in real archives and toybox's `tar` handles sparse files and `--strip-components`
[U22].

### What POSIX has and what people rely on

I fetched the POSIX.1-2024 utilities index: it lists 154 utility names (155 tokens in my whitespace count, one
stray) [U47]. By my matching of those names against file names in toybox's four directories (plus manual fixes for
`chown`, `logname` and multi-command files), about 74 are finished in toybox, 14 pending (`awk bc crontab csplit diff
expr ipcrm ipcs man more sh stty tr vi`), 14 are shell builtins (`alias bg cd command fc fg getopts hash jobs read
type umask unalias wait`, dash's job) and about 52 are absent, of which the daily ones are `ed ex join pax pr unexpand
make ar nm strip ctags m4 lex yacc c17 tput at newgrp`. This is a rough name match, not a conformance test.

What POSIX.1-2024 contains for the "GNU extensions people rely on" [U48]:

| Feature | In POSIX.1-2024? | Evidence |
|---|---|---|
| `find -print0`, `find -exec ... {} +` | yes | find page lists `-print0` and the `+` form of `-exec` |
| `xargs -0` | yes | xargs page specifies NUL-delimited input |
| `diff -u` / `-U n` | yes | diff options include `-u`, `-U n` |
| `patch` | yes (all of -b -c -d -D -e -i -l -n -N -o -p -R -r ...) | patch page |
| `grep -r`, `-o`, `-H`, `-h`, `-w`, `-P` | no | grep synopsis: `[-E|-F] [-c|-l|-q] [-insvx]` |
| `sed -i`, `sed -z` | no | sed synopsis: `[-En]` only with `-e`/`-f` |
| `tar` | not present | utilities/tar.html is a 404; `pax` is |

So the `grep -r`, `sed -i` and `grep -o` that scripts use daily are outside the standard; toybox's `grep` and `sed`
implement them (they are in `toys/posix` with option strings I did not parse) (inference from [U16]; unverified per
option). OpenBSD's `diff` is ISC-licence text (Todd Miller, 2003) and its `patch` is Larry Wall's BSD-like condition
text; GNU diffutils' `COPYING` is GPL version 3 [U49]. toybox `diff` is pending with long options and unified output;
busybox `diff` is what Zoraya builds today [U19, U2].

### less, vi, ed and bc as sources

`less` upstream is dual: GPL version 3 or later, or the "Less License", a two-clause BSD-style text; its README lists
curses/terminfo/termcap libraries it links (`-ltinfo ... -ltermcap`) and a built-in terminal database fallback
(`termcap.dat`, line 190) [U50]. OpenBSD's copy states "either the GNU General Public License or the Less License"
[U50]. nvi (the BSD `vi`, in OpenBSD's tree and in chimerautils as `nvi`) is under the 3-clause UC Regents plus Keith
Bostic and Sven Verdoolaege notices [U51]; its README says it uses the `curses(3)` library through a `cl` directory
[U51]. OpenBSD's `bc` is ISC-style text (Otto Moerbeek, 2003) and is a yacc grammar (`bc.y`, 1,190 lines) [U49].

### What the missing tools need from libc

From the toybox 0.8.15 tarball [U10] (counts are number of files under `toys/` containing the name):

| Facility | Tools that use it (own-source grep) | Notes for Zoraya |
|---|---|---|
| `regcomp`/`regexec` | awk csplit cut diff expr grep man mdev nl ps sed test vi | now in `libc.a` per ADR-0310 [U8] |
| `fnmatch` | find grep ip modprobe tar | now in `libc.a` per ADR-0310 [U8] |
| `getpwnam` | 18 files incl. id, tar, ps, sh, su | Zoraya has no users |
| `getgrnam` | groupadd mdev ps tar tcpsvd useradd | idem |
| `getpwuid`, `getgrgid` | id, find, tar, ps, lib | `ls -l` through `lib` |
| `utimensat` | cp cpio gzip tar touch | `utimensat` is in the T1 list [U10] (`docs/adr/posix-surface-T1.txt` has it) |
| `mmap` | 9 files | private mapping exists in Zoraya (`mmap` private, plan 6c) [U52] |
| `fork`/`vfork` | `xfork` in lib; sh, cpio, init and daemons | `fork` is a snapshot (brief); `xargs`/`find -exec` use `xexec`/`xrun` |
| `getopt_long` | 1 file | in T1 list |
| `wcwidth` | 7 files (`ls`, `vi`, `less`-like) | in T1 list |
| `glob`, `nftw`, `fts_*`, `posix_spawn` | none | toybox needs none |

(`docs/adr/posix-surface-T1.txt` row counts: `getpwnam`, `utimensat`, `fork`, `posix_spawn`, `getopt_long` and
`wcwidth` each appear once; `regcomp`, `fnmatch`, `glob`, `nftw`, `fts_open`, `getgrnam`, `mmap`, `getline`,
`mkdtemp` do not (`grep -cw` over that file; the file is the T1 list, so a name's absence there only
means it was not T1) [U10].) On the host side the
FreeBSD tools of chimerautils also depend on `libxo`, `flex`, `byacc` and a compat library [U26]; that is a larger
port than toybox or sbase.

### Everyday tools compared

"toybox" column: F = finished (in `toys/posix`, `other` or `lsb` at 0.8.15 and `master`), P = in `toys/pending`, A = no
file in any directory (from the GitHub listings [U16] and `NEWTOY` lines [U17, U21]). "Zoraya" = built as a POSIX
program today [U2]; native `zosh` commands of the same name are noted where `user/describe/commands/` has one [U9].
Alternatives are the best candidates found in this survey; all licences as stated by the cited sources.

| Tool | toybox | Zoraya | Best permissive alternative source |
|---|---|---|---|
| cat | F | built (native too) | toybox 0BSD; sbase MIT [U28, U29] |
| cp | F | native only | toybox 0BSD; sbase MIT |
| mv | F | built (native too) | toybox |
| rm | F | built (native too) | toybox |
| ls | F | built (native too) | toybox; sbase (some flags missing) [U28] |
| mkdir | F | native only | toybox; sbase |
| ln | F | no | toybox (`link`, `dirname` needed) |
| chmod | F | no | toybox; sbase |
| touch | F | built | toybox |
| stat | F (`other`) | native only | toybox; sbase lacks it per README table I read (no `stat` row) [U28] |
| du | F | no | toybox; sbase |
| df | F | no | toybox |
| head | F | built (native too) | toybox |
| tail | F | no | toybox; sbase |
| sort | F | built | toybox |
| uniq | F | no | toybox; sbase |
| cut | F | no | toybox; sbase |
| tr | P | built from busybox (GPL-2.0-only) | toybox pending `tr` (239 lines, 0BSD); sbase `tr` MIT; FreeBSD `tr` BSD [U19, U28, U26] |
| wc | F | built | toybox |
| tee | F | no | toybox; sbase |
| grep | F | no (needs `regcomp`, now in libc) | toybox; FreeBSD grep BSD-2 [U26] |
| sed | F | no | toybox; FreeBSD sed; sbase `sed` not finished [U28] |
| awk | P | no | toybox pending `awk` (0BSD, 4,579 lines); one-true-awk (Lucent notice); FreeBSD awk (same notice) [U19, U34, U36] |
| find | F | native only | toybox; sbase `find` not marked finished [U28] |
| xargs | F | no | toybox; sbase |
| diff | P | built from busybox (GPL-2.0-only) | toybox pending `diff` (934 lines, 0BSD); OpenBSD `diff` ISC-style [U19, U49] |
| patch | F (unified only) | no | toybox; OpenBSD `patch` [U22, U49] |
| cmp | F | no | toybox; sbase |
| tar | F | no | toybox; bsdtar (BSD-2); sbase `tar` (non-POSIX mark) [U22, U44, U28] |
| gzip | F (`lsb`, own deflate) | no (busybox `gzip` failed to link) | toybox (deflate in `lib/deflate.c`); zlib; miniz (MIT) [U21, U38, U39] |
| xz (decode) | P (`xzcat`) | no | toybox `xzcat`; XZ Embedded (0BSD) [U19, U41] |
| xz (encode) | A | no | xz-utils (0BSD, take care over source provenance) [U40, U45] |
| bzip2 | A (`bzcat` F only) | no | bzip2 1.0.8 (BSD-style); libarchive [U43, U44] |
| unzip | A | no | libarchive `bsdunzip` (BSD-2); miniz zip API (MIT) [U39, U44] |
| dd | F | no | toybox; sbase |
| od | F | no | toybox; sbase |
| hexdump / xxd | hexdump P, xxd F | no | toybox `xxd`; FreeBSD hexdump [U17, U26] |
| date | F | native only | toybox; sbase |
| sleep | F | built (native too) | toybox |
| env | F | no | toybox; sbase |
| printf | F | no (dash has it as a builtin, `dash` is built) | toybox; sbase |
| test | F | no (dash builtin) | toybox; sbase |
| expr | P | no (busybox `expr` failed to link) | toybox pending `expr` (251 lines); sbase `expr` MIT (finished mark); FreeBSD `expr` [U19, U28, U26] |
| bc | P | no | toybox pending (5,953 lines); OpenBSD `bc` (ISC-style, yacc); 9base `bc` [U19, U49, U32] |
| seq | F (`lsb`) | no | toybox; sbase |
| basename / dirname | F | no | toybox; sbase |
| realpath | F (in `readlink.c`) | no | toybox; FreeBSD realpath [U17, U26] |
| which | F (`other`) | no | toybox; sbase |
| file | F | no | toybox |
| less | A | no | `less` itself (GPL-3+ or the Less License, BSD-style) [U50] |
| more | P | no | toybox pending (172 lines); FreeBSD/OpenBSD `more` not examined [U19] |
| vi | P | no | toybox pending (1,871 lines, 0BSD); nvi (BSD-3, curses) [U19, U51] |
| ed | A | no | sbase `ed` (MIT, not marked finished); FreeBSD `ed`; 9base `ed` [U28, U26, U32] |
| ps | F | no | toybox (also `top`, `pgrep`, `pkill` in the same file) [U17] |
| kill | F | shell builtin in `zosh` | toybox [U9, U16] |
| top | F (in `ps.c`) | `zosh` builtin | toybox [U17, U9] |
| git | pending stub (`gitclone` etc., not git) | no | none permissive examined here [U19] |
| sh | P (toybox sh); dash built | dash (BSD-3-Clause) | dash, already shipped [U3] |

## Pseudo-terminals

### What a pty is on Unix

A pseudoterminal is a pair of virtual character devices forming a bidirectional channel, called master and slave. The slave
behaves exactly like a classical terminal; a program that expects a terminal opens the slave and is driven by whatever holds
the master. Bytes written on the master reach the slave as if typed; writing the interrupt character (usually Ctrl-C) to the
master raises SIGINT in the foreground process group of the slave; bytes written on the slave can be read from the master
[P1]. The line discipline (echo, canonical editing, signal characters) therefore lives inside the pair, between the two
ends (inference from [P1]: the interrupt-to-signal conversion happens although neither end implements it).

The API that new programs should use is the System V one, standardised in SUSv1 [P1]. In POSIX Issue 8 (2024) the terms
are "manager" and "subsidiary" instead of master and slave [P3][P4]. `posix_openpt(oflag)` opens a manager device and
returns a file descriptor; `O_NOCTTY` keeps it from becoming the controlling terminal [P3]. `grantpt` changes mode and
ownership of the subsidiary: user ID is set to the real UID of the caller, group unspecified, mode readable and writable by
the owner and writable by the group [P4]. On Linux the sequence is: open `/dev/ptmx` (via `posix_openpt`), `grantpt`,
`unlockpt`, then open the name returned by `ptsname` [P1][P2]. Each open of `/dev/ptmx` returns an independent master with its
own slave under `/dev/pts` [P2]. The BSD style (precreated pairs `/dev/ptyXY` and `/dev/ttyXY`, found by trying each open
until one succeeds) is deprecated on Linux, and `CONFIG_LEGACY_PTYS` has been off by default since Linux 2.6.30 [P1].

BSD library helpers wrap this. `openpty` finds a free pair and returns both descriptors, optionally setting termios and
window size. `login_tty(fd)` creates a new session, makes `fd` the controlling terminal, installs it as stdin, stdout and
stderr and closes it. `forkpty` is `openpty` + `fork` + `login_tty` and returns the master in the parent [P5].

Controlling terminal and sessions: `TIOCSCTTY` makes a terminal the controlling terminal of the caller, which must be a
session leader without a controlling terminal; if the terminal already controls another session the call fails with EPERM
unless the caller has CAP_SYS_ADMIN and passes 1, in which case the terminal is stolen [P6]. `/dev/tty` is a synonym for the
caller's controlling terminal, if any [P9]. A daemon detaches with `TIOCNOTTY` on a descriptor opened from `/dev/tty`: if the
open fails it has no terminal [P9]. OpenSSH's sshd does exactly this sequence when it gives a login session its pty:
`TIOCNOTTY` on `/dev/tty`, `setsid`, check that `/dev/tty` can no longer be opened, then `TIOCSCTTY` on the slave [P21].
foot, a terminal emulator, does the child side as `setsid`, open the slave by name, `ioctl(pts, TIOCSCTTY, 0)` [P40].

Packet mode: `TIOCPKT` on the master (ENOTTY elsewhere) makes each read return either a single nonzero control byte or a zero
byte followed by data from the slave; control bits report flush-read, flush-write, stop and start of output [P7]. Linux has
`TIOCGPKT`, `TIOCSPTLCK`, `TIOCGPTLCK` and `TIOCGPTPEER`; the BSD ioctls TIOCSTOP, TIOCSTART, TIOCUCNTL and TIOCREMOTE are
not implemented on Linux [P1][P7].

Window size: `TIOCSWINSZ` stores rows, columns and pixel size in the kernel, which does not use them; when the size
changes, SIGWINCH is sent to the foreground process group [P8]. A terminal emulator or sshd sets the size on the master side:
tmux passes a `winsize` into `forkpty` when it spawns a pane [P18], and sshd calls `pty_change_window_size` on its master
descriptor when a "window-change" request arrives from the client [P21][P22].

### Pty security history

A pty slave is a device node that has to be owned by the user who runs on it. POSIX therefore defines `grantpt` as the
function that changes owner and mode of the subsidiary [P4]. Where the calling process cannot chown a device node, glibc
shipped a helper, `pt_chown`, which the source describes as "helper program for `grantpt`" that sets owner, group and mode of
the slave and "needs to be installed setuid `root`" [P10]. (glibc's licence per that file's header: LGPL 2.1 or later [P10].)
Whether and when glibc removed it is not covered by a page I fetched; do not rely on it.

The devpts filesystem removed the need for such a helper but left the namespace global. Linux's devpts documentation says
that each mount of devpts is now distinct: ptys and indices allocated in one mount are independent of those in every other
mount; every mount creates a `/dev/pts/ptmx` node with mode 0000; and the legacy `/dev/ptmx` node (`mknod c 5 2`) looks for
a devpts instance named `pts` in its own directory. The `ptmxmode=0666` mount option is needed if `/dev/ptmx` is a symlink or
bind mount of `/dev/pts/ptmx`, and a per-instance `max=` limit exists alongside global sysctls `kernel.pty.max`,
`kernel.pty.reserve` and `kernel.pty.nr` [P11]. This is what makes a container able to have its own ptys [P11]. A related
hazard is path-based access: pts(4)/ioctl docs warn that confusion can occur using `ptsname` with a path when a devpts is
mounted in a different mount namespace, and `TIOCGPTPEER` (Linux 4.13) opens the peer slave directly from the master
descriptor regardless of whether the slave path is visible [P7].

TIOCSTI: `ioctl(fd, TIOCSTI, &c)` inserts a byte into the terminal's input queue, as if typed [P12]. The kernel commit that
added the switch, "tty: Allow TIOCSTI to be disabled" (Kees Cook, 2022-10-22), says TIOCSTI "continues its long history of
being used in privilege escalation attacks", notes that OpenBSD had removed it entirely three years earlier and Android
filtered it, and adds a Kconfig option plus the sysctl `dev.tty.legacy_tiocsti` [P13]. The Kconfig help (`LEGACY_TIOCSTI`,
default y) calls it a "malicious privilege escalation mechanism" with no real-world utility any more, and says processes with
CAP_SYS_ADMIN, such as BRLTTY, can still use it when the option is N [P14]. A later commit (Samuel Thibault, 2023-07-10)
restored it for CAP_SYS_ADMIN because the first version broke BRLTTY's braille keyboards [P13]. man-pages states: since Linux
6.2 the operation may require CAP_SYS_ADMIN if the sysctl is false [P12]. The 6.2 version number comes from the man page [P12],
not from a commit I read.

Sandbox escape by TIOCSTI: Flatpak advisory CVE-2017-5226 says that if Flatpak is run from a terminal emulator containing an
interactive shell, a malicious app could inject input into that shell with TIOCSTI [P15]. The advisory records that the first
workaround in bubblewrap was reverted because it broke interactive shells inside Flatpak apps, and that the fix was a seccomp
filter in Flatpak [P15]. The first filter was incomplete on 64-bit architectures; Flatpak 1.3.1 fixed that as CVE-2019-10063
("only compare the low 32bit" of the ioctl argument) [P16]. Their other stated workaround: do not run apps with a controlling
terminal [P15]. (I could not fetch the oss-security thread; see the end.)

### Which programs need a pty

The deciding question is whether the program is *a terminal* (it sits between a person's display and a program that expects a
terminal) or *a user of a terminal*. Only the first kind creates ptys. man-pages lists the usual consumers: network login
services (ssh, rlogin, telnet), terminal emulators such as xterm, script, screen, tmux, unbuffer and expect [P1]; it adds that
a pty can also feed programs that refuse to read from pipes, such as `su` and `passwd` [P2].

| Program | Creates a pty? | Why | Source |
|---|---|---|---|
| tmux (ISC-style licence per file header) | yes, one per pane | Each pane is "a separate pseudo terminal"; spawn calls `fdforkpty` with the pane size as `winsize` | [P17][P18] |
| GNU screen | yes, one per window | Pseudo-ttys per window; man page documents the mode given to each newly allocated pseudo-tty; pty(7) names it | [P19][P1] |
| `script(1)` | yes | Runs a session inside a pty so it can log both directions; the `-E` option controls ECHO of the slave; input logging can capture passwords | [P20] |
| sshd (OpenSSH; Tatu Ylonen's free-use notice and BSD files) | yes, when the client asks for a pty | `pty_allocate` calls `openpty`; the child gets it as controlling tty; window-change requests call `TIOCSWINSZ`; sshd can forbid pty allocation (`no-pty` style restriction) | [P21][P22][P26] |
| ssh *client* | **no** | It does not allocate a pty; it asks the server for one (`tty_flag`), and when it does it only puts the *local* terminal into raw mode (`enter_raw_mode`, `tcsetattr`) and handles SIGWINCH to send window-change messages. If stdin is not a tty it does not request a pty unless forced with `-tt` | [P23][P24][P25][P27a] |
| expect (licence not checked) | yes | `spawn` uses a pty "initialized the same way as the user's tty" then set to sane settings | [P27] |
| Python `pty` module (Unix only) | yes | `pty.openpty`, `pty.fork`, `pty.spawn`; falls back to `os.forkpty`/`os.openpty`/`os.login_tty`, or opens the master by hand | [P28][P32] |
| CPython `subprocess` | **no** | The module source contains no pty use; `stdin=PIPE` etc. are pipes | [P30] (inference from absence in the text I searched) |
| CPython `getpass` | **no, it opens `/dev/tty`** | `os.open('/dev/tty', O_RDWR\|O_NOCTTY)` first; falls back to stdin | [P31] |
| CPython test suite | yes | `test_pty.py` tests `openpty`, forkpty and window-size propagation to the slave | [P29] |
| git (GPLv2 per COPYING) | **no** | Credential prompt first tries an askpass program (`GIT_ASKPASS`, config, `SSH_ASKPASS`), else `git_terminal_prompt` which opens `/dev/tty` for input and output. Pager and editor are children that inherit the terminal; I found no pty creation in the two files read | [P33][P34] |
| vim `:terminal` (Vim licence) | yes | Doc: "On Unix a pty is used to make it possible to run all kinds of commands"; source tries to open a master pty | [P35][P36] |
| less ("Less License", 2-clause BSD wording in LICENSE) | **no** | Opens `/dev/tty`, else `ttyname(2)`, else fd 2 for keys, so that it works when stdin is the piped text | [P37][P38] |
| sudo | optional | Allocates a new pty only when an I/O-logging plugin or the policy asks for terminal I/O logging (a monitor process then relays) | [P41] |
| xterm | yes | `get_pty` uses `openpty` or `posix_openpt` to create the pair and runs the shell on the slave | [P39] |
| foot (MIT) | yes | Opens the master, `ptsname`, child does `setsid` + `TIOCSCTTY` | [P40] |

Python 3.15's `pty` page says Availability: Unix [P32]; this is the doc page version I fetched.

A pattern from the table: every program that creates a pty is a terminal emulator, a remote-login server, a multiplexer or
a recorder. None of the programs Zoraya's stage 10 lists (vi, less, git, ssh *client*, tools) creates one. They consume the
terminal they inherit, or open `/dev/tty`. The one place where git, ssh, or the editor could need a *new* terminal is when
`vim :terminal` is used, and with the ssh *server*, which is not in the stage list.

### The name /dev/tty

`/dev/tty` is the controlling terminal of the caller, whatever fds 0 to 2 point at [P9]. Programs open it by name for these
reasons:

- OpenBSD's `readpassphrase(3)`, used by OpenSSH for password and passphrase prompts, tries `open("/dev/tty")` first; only if
  that fails does it read stdin and write stderr, and with `RPP_REQUIRE_TTY` it fails with ENOTTY instead [P42].
- Git's terminal credential prompt opens `/dev/tty` for input and for output; before that it tries an askpass program [P33][P34].
- less opens `/dev/tty` (on macOS it prefers `ttyname(2)` because poll does not work on `/dev/tty` there), then `ttyname(2)`
  of fd 2, then uses fd 2, so that keys are read from the person when stdin is a pipe [P37].
- Python's `getpass` reads and writes `/dev/tty` first [P31].
- gpg-agent's documentation tells users to set `GPG_TTY=$(tty)` in their shell start-up file, because the agent runs outside
  the session and needs the terminal's name for pinentry; the page says the variable must always reflect `tty`'s output [P43].
- sshd uses `/dev/tty` as a probe: open it to learn whether a controlling terminal exists, and as the handle for `TIOCNOTTY`
  [P21]. Vim's source comments that with pipes and no `setsid`, losing stdin as a tty would lose `/dev/tty` [P36].
- sudo's page I fetched has no `/dev/tty` statement; I did not verify sudo's own prompt code.

This is the ambient authority that an object-capability system has to replace: "the terminal of this session" reachable by a
fixed name from any process in the session.

### Ptys without a global name

| System | Mechanism | Who holds what | What it gives programs | Licence of the source read | Source |
|---|---|---|---|---|---|
| Fuchsia | `ptysvc` component serves `fuchsia.hardware.pty.Device`, "very similarly to the UNIX 98 /dev/ptmx"; each connection gets a new, unique pty server | A client holds a channel to the service (a capability routed by component manifests); the server side is the keyboard+monitor side; clients are numbered, client 0 is the controlling one and the only one that may create others or receive events | `OpenClient(id)` yields another Device channel; `SetWindowSize` only on the server side; `ClrSetFeature` on clients; raw feature disables `^C`, `^Z` events | docs CC-BY-4.0, code samples Apache-2.0 per page footer; `ptysvc.cml` BSD-style header | [P44][P45][P46][P47] |
| Redox | `ptyd` daemon serving the `pty:` scheme | Opening `pty:` creates a new pty and returns the control (master) handle; opening `pty:N` returns a slave for that id; `dup` of a handle with `"pgrp"`, `"termios"` or `"winsize"` yields sub-resources | Line discipline in the daemon: on the control side, VINTR and VQUIT bytes lead to `kill(-pgrp, SIGINT/SIGQUIT)`; the daemon enters the null namespace after opening its scheme | MIT | [P48][P49][P50][P52] |
| Plan 9 | No pty. `8½`/rio serves a distinct `/dev/cons`, `consctl`, `mouse` etc. per window, using per-process namespaces | Each client's namespace has its own `/dev/cons`; the window server holds the other end of a pipe; a `rawon`/`rawoff` write to `consctl` changes keyboard interpretation, reverting when closed | Programs write `/dev/cons`; for foreign systems `vt` emulates a VT-100/220 in a rio window and "is used only for communicating with foreign systems" | rio and 8½ docs, no licence read | [P59][P60][P61][P62] |
| Genode | `Terminal` session interface: a bidirectional channel with read, write, size query; `terminal_crosslink` connects exactly two Terminal clients (4 KiB buffer each by default) | Client and server share a communication buffer; the session is created by the parent and routed by the component tree | Libc's VFS plugin fakes `TIOCGWINSZ` from the node's rows/columns, answers `TIOCGETA` with fixed termios (only ECHO set, "needed by libreadline") and ignores `TIOCSETA`; no line discipline in the libc layer shown | AGPLv3 per file headers | [P53][P54][P55][P56] |
| Windows ConPTY | `CreatePseudoConsole(size, hInput, hOutput, flags, &HPCON)`; the *terminal* supplies two stream handles; Windows starts a ConHost instance on the other end | The terminal holds the pseudo-console handle and the pipe ends; the command-line app is attached to ConHost as usual | Input/output are UTF-8 text with VT sequences; `ResizePseudoConsole`, `ClosePseudoConsole` | Documentation | [P57][P58] |
| WASI (wasi-cli) | `terminal-input`, `terminal-output` resources and `get-terminal-stdin/stdout/stderr`, each returning `option<…>` ("link-time authority") | The host decides whether stdin is a terminal; the handles are empty resources today | The WIT comments list only possible future size and feature queries | Apache-2.0 per project (not read) | [P63] |

Fuchsia detail. The FIDL doc says the server side is obtained by opening `/dev/misc/ptmx` (a path in the legacy device
namespace), the client sides by calling `OpenClient`, and only client 0 can create more clients, receive events or select
which client PTY receives input [P44]. The `ptysvc.cml` manifest declares and exposes `fuchsia.hardware.pty.Device` as a
protocol capability, and its bootstrap shard routes it from `#ptysvc` to `self/toolbox`, so access is a routing decision in
the component tree, not a node in a global namespace [P46][P47]. I could not fetch which components other than `toolbox` get
it (console, sshd) or Starnix's devpts source; see the list at the end.

Redox detail. `ptyd` creates a `pty` scheme socket and then calls `setrens(0, 0)` ("enter null namespace") [P49]. The
control handle and slave handle share one `Pty` object inside the daemon [P48]. The terminal emulator `orbterm` has a
`getpty` function that opens `/dev/ptmx` with `O_NONBLOCK`, calls `grantpt`, `unlockpt`, sets the size with `TIOCSWINSZ`
and gets the slave name with `ptsname` [P51]; that is the POSIX API, which on Redox is presumably mapped to `pty:` by the C
library (inference; I did not read relibc). `ptyd`'s README has no design text [P52a].

Plan 9 detail. Rio's man page: mounting `$wsys` creates a window, and the attach specifier must carry a `-pid` so that "notes"
generated by DEL and hangups reach that process; the window files are mounted on `/mnt/wsys` and also `/dev` [P59]. The 8½
paper says the `/dev/cons` served in each window is a distinct file and "the per-process name spaces of Plan 9 allow 8½ to
provide a unique /dev/cons to each client" [P60]. cons(3) describes raw mode via `consctl` as lasting while the file is open
[P61]. Consequences (inference from [P59][P60][P62]): no device creation, no `grantpt`, no controlling-terminal concept, no
terminfo-style cursor addressing in native programs; `vt` exists for remote Unix hosts, with manual raw/cooked and CR/NL
menus.

Windows detail. The ConPTY announcement says Windows lacked a PTY infrastructure, that third-party terminals had to create
an off-screen console and scrape it, and that ConPTY gives "a mechanism similar to the POSIX PTY model, but in a
Windows-relevant manner" [P57]. The terminal "creates its own communication pipes" and calls the API; Windows then spins up
a ConHost on the other end [P57]. The pty is thus an object returned by an API call and owned by a handle, not a name [P58]
(inference: the quoted API takes handles and returns `HPCON`, no path is involved).

WASI: the wasi-cli `terminal.wit` I fetched defines the terminal-input and terminal-output resources empty except for
comments about future features, and `get-terminal-stdin` and friends return an option that is present only if the stream is
connected to a terminal [P63]. There is no pty creation call in that file.

QNX `devc-pty`: not researched; no primary document was fetched.

### A pty pair as capabilities

Two endpoints share one line-discipline object. The master-side holder has a lot of authority over the slave side, all
derived from the man pages above (marked as inference where I combine them):

- It sees every byte the slave program writes, and, because the slave's echo and input pass through the same pair, whatever
  the user types goes through it; `script(1)` documents that input logging "may record security-sensitive information"
  including passwords, regardless of the echo flag [P20].
- It can inject input and, through the line discipline, signals: writing the interrupt character to the master raises SIGINT
  in the slave's foreground process group [P1].
- It sets the window size, which delivers SIGWINCH to the foreground group [P8].
- It can flush and stop output as seen in packet mode [P7].

The slave-side holder has less: it can read and write, set termios on its side and, if it is a session leader, take the
terminal as controlling terminal [P6].

Authority leaks through naming, not through the pair itself:

- `/dev/tty` gives any process in a session the terminal; TIOCSTI on that descriptor pushes characters into the person's shell
  (the Flatpak case: a sandboxed app that had a controlling terminal could type into the parent shell) [P12][P15].
- A slave path can be reached by anyone who can open it when `grantpt`-style mode bits allow it (group-writable, so other
  users can write to your terminal) [P4].
- The shared `ptmx` device and the pty count are global resources, which is why devpts gained per-mount instances and limits
  [P11].

Capsicum treats a tty as an ordinary descriptor. `rights(4)` has `CAP_IOCTL` ("enormous scope, including potentially global
scope for some objects"), narrowed per command with `cap_ioctls_limit(2)`, and a `CAP_TTYHOOK` right for configuring TTY
hooks such as snp(4) [P64]. Neither `capsicum(4)` nor `rights(4)` text mentions `TIOCSTI` or opening `/dev/tty` [P64][P65];
(inference) a Capsicum program that keeps an ioctl-capable descriptor to a terminal can still inject input unless the ioctl
list is limited, and `/dev/tty` cannot be opened by name in capability mode because that needs a global namespace lookup, but
I did not verify this against the `cap_enter(2)` text.

I found no primary source that frames terminals as a confused-deputy problem. The nearest are the TIOCSTI commit message and
the Flatpak advisory, which describe the attack [P13][P15].

## git and ssh

Notes on method. All sources were fetched on 2026-11-13. Source files were read from the projects' `master`/`main`
branches, which are newer than any release the system would port. The man pages of OpenSSH were read as mdoc source
from the openssh-portable tree and rendered locally with `mandoc`; the text is the project's own. Counts marked
"measured" were made by me with `grep`/`wc` on the fetched files.

### git: licence and alternatives
Git's `COPYING` states that the only valid version of the GPL is version 2, not 2.2 or 3.x, unless stated otherwise
[G1]. So git itself is GPL-2.0-only; for Zoraya this is the licence of a ported program, nothing more is said here.

Permissive or linkable reimplementations:

| What | Licence | Language, shape | What it covers | What it needs | Source |
|---|---|---|---|---|---|
| libgit2 | GPL-2 with a linking exception: permission to link the compiled library into other programs and distribute the combination without restriction; GPL still covers changes to the library itself | C library | library, not a CLI | not examined | [G2] |
| gitoxide (`gix`) | `MIT OR Apache-2.0` (workspace `Cargo.toml`) | Rust, many crates | init yes; `ein clone` is unchecked; fetch implemented; **push not implemented**; ssh transport runs the external `ssh`; http via `curl` or `reqwest`, blocking only | std (not stated in the source; inference from the use of curl/reqwest and processes) | [G3][G4] |
| Game of Trees (`got`) | ISC; the project's goals say the whole code base is BSD-licensed | C, OpenBSD style; portable version `got-portable` for Linux, FreeBSD, NetBSD, DragonFly, macOS | works on the on-disk format of bare git repositories; own CLI (not a git porcelain); fetch over git, ssh, http(s); push over ssh only | libz, libbsd, libmd, libuuid, libncurses, bison (build), openssl for gotwebd/TLS (Linux list) | [G5][G6][G7] |
| dulwich | `Apache-2.0 OR GPL-2.0-or-later` | pure Python, optional Rust bindings | git implementation without calling git | a Python runtime | [G11] |

gitoxide's own status list is explicit about gaps: `push` is unchecked at the protocol level, `ssh://` without an external
`ssh` binary is unchecked, and the `gix-ssh` crate (a native SSH transport) is a plan with unchecked items [G4].
The list also notes a correctness gap relevant to a grant-by-destination system: credential-helper matching does not yet
use the URL that is actually contacted after `insteadOf` rewriting or an HTTP redirect to another host [G4].

Got is the one design here that already treats the host as hostile. Its stated goals are to use `pledge(2)` and
`unveil(2)` across the code base and to use privilege separation when parsing repository data from network or disk
[G6]. In the portable tree the repository code lives in separate helper programs under `libexec/` (`got-fetch-pack`,
`got-fetch-http`, `got-index-pack`, `got-read-pack`, `got-read-object`, `got-send-pack` and others) [G7]. The parent
creates a `socketpair`, `fork`s, and executes the helper with the socket as its only channel [G10]. The fetch helper
then restricts itself to the pledge `stdio recvfd` and drops file-system access (a `landlock_no_fs()` call in the
portable version) [G9]. The network connection is made by the parent and the descriptor is passed in. For ssh the parent
`unveil`s `/usr/bin/ssh` with execute permission only, then forks and `execv`s that path [G8]. This is the pattern of
"a parser with no authority, handed one descriptor" and it is about the same size as the problem Zoraya has with git
over ssh. The cost: it is a different tool (`got`, `tog`, `gotd`), not a `git` binary, so scripts and habits that call
`git` do not work with it [G5].

### What git needs
`INSTALL` says git is "reasonably self-sufficient" and lists its external needs: zlib (git will not build without it),
an `ssh` program for push/pull over the net, a POSIX shell for some scripts (it names `bisect` and `request-pull`),
Perl 5.26 or later for `send-email` and `svn`, libcurl 7.61.0 or later for http(s) transports (and `git-imap-send`),
expat for the DAV push helper, Tcl/Tk for `gitk`/`git-gui`, and a gettext library for translations [G12].

The Makefile header documents the switches that remove these [G13]:

| Knob | What the Makefile says | Source |
|---|---|---|
| `NO_CURL` | `git-http-fetch` and `git-http-push` are not built; no http(s) transports, smart or dumb | [G13] |
| `NO_EXPAT` | `git-http-push` is not built; no push over http(s) (dumb) | [G13] |
| `NO_OPENSSL` | for systems without OpenSSL. SHA-1 default is the built-in `sha1collisiondetection` (`sha1dc/`) unless `OPENSSL_SHA1`/`BLK_SHA1` is chosen; SHA-256 defaults to a built-in implementation | [G13] |
| `NO_PERL`, `NO_PYTHON`, `NO_TCLTK` | no Perl scripts or libraries, no Python, no Tcl/Tk GUI | [G13] |
| `NO_GETTEXT`, `NO_ICONV` | no translation; libc without a working `iconv` | [G13] |
| `NO_PTHREADS` | no pthreads (changes how async helpers are run, see below) | [G13][G14] |
| `NO_MMAP` | avoid `mmap` | [G13] |
| `NO_REGEX` | libc lacks regex with `REG_STARTEND` (git then uses its own) | [G13] |
| `NO_UNIX_SOCKETS`, `NO_SYS_POLL_H`, `NO_SETITIMER` | system lacks Unix sockets, `sys/poll.h`, `setitimer()` | [G13] |
| `NO_RUST` | Rust is an optional part of the build for now; the Makefile says that with Git 3.0 Rust will always be enabled | [G13] |

The last row is a trend to watch: a build of future git needs a Rust toolchain for the target (inference from [G13]).
Ten shell scripts (`git-submodule`, `git-mergetool`, `git-filter-branch`, `git-request-pull`, ...) and six Perl
scripts (`git-send-email`, `git-svn`, `git-cvsimport`, ...) are listed by the Makefile (measured: lines 727-736 and
742-747 of the fetched file) [G13]. With `NO_MMAP`, git's `git_mmap` allocates memory and fills it with `pread`; it only
supports private read-only mappings and dies on any other use [G27]. So packfiles and the index can be read without
`mmap`, at the cost of a copy.

### How git starts other programs
The command table in `git.c` lists 149 `cmd_*` builtins (measured with a regular expression on `{ "name", cmd_ ...`)
[G15], so most commands, including `index-pack` and `unpack-objects`, run inside the one `git` binary [G13]. Outside
the builtins there are the shell and Perl scripts above [G13] and the transport helper `git-remote-http`, which is
also installed under the names `git-remote-https`, `git-remote-ftp` and `git-remote-ftps` [G13].

`start_command()` in `run-command.c` is the one place that starts child processes: it creates pipes, calls `fork()`,
and in the child `execve()`s, with a notify pipe so the parent learns of an `exec` failure [G14]. Where git uses it:

- the pager (`start_command` on the `core.pager`/`$PAGER` program, which defaults to `less`; the output is a pipe) [G17][G22];
- the editor, chosen from `GIT_EDITOR`, then `core.editor`, `VISUAL`, `EDITOR`, with `vi` as fallback unless `TERM`
  is unset or `dumb` [G33][G33a];
- hooks, from `$GIT_DIR/hooks` or `core.hooksPath` [G22];
- `git-remote-https` for http(s) URLs, `ssh` (or `GIT_SSH`, `GIT_SSH_COMMAND`, `core.sshCommand`) for ssh URLs [G33];
- `index-pack` or `unpack-objects` during fetch [G20];
- `git gc --auto`, which detaches with `daemonize()` (a `fork` followed by `setsid`) and continues in the foreground if
  that fails [G18][G19];
- credential helpers and the askpass program [G29].

Without `fork`, the only fallback built into the source is the one `NO_PTHREADS` selects: the "async" helper, which
normally runs as a thread, runs as a forked child instead [G14]. The Windows port does not use `fork`; `compat/mingw.c`
implements `start_command` on `CreateProcessW` (spawn semantics) [G16]. That port shows that git runs with spawn-only
process creation, at the cost of an emulation layer for pipes, environment and signals (inference). A snapshot-`fork`
as Zoraya has it is the easy case for git; a pure spawn is the existing precedent.

### Files git touches unasked

Table 1. Files and paths git reads or writes without being told.

| Path | R/W | When | Source |
|---|---|---|---|
| the worktree (the directory tree being versioned) | R/W | status, add, checkout, commit | [G21] |
| `.git/` : `objects/`, `refs/`, `HEAD`, `index`, `config`, `hooks/`, `info/exclude`, `packed-refs` | R/W | every command | [G21] |
| `<file>.lock` (for example `index.lock`) | create exclusive, then `rename` | every update of a file | [G24] |
| parent directories of the cwd, looking for `.git` | R (stat) | repository discovery | [G19][G23] |
| `.git` as a file with `gitdir: <path>` | R | worktrees and submodules | [G21] |
| `objects/info/alternates` and its listed paths | R | object lookup | [G21] |
| `$(prefix)/etc/gitconfig` | R | every command | [G22] |
| `$XDG_CONFIG_HOME/git/config`, `~/.gitconfig` | R (W for `git config --global`) | every command | [G22] |
| files named by `include.path` / `includeIf.*.path` | R | config parsing | [G22] |
| `$XDG_CONFIG_HOME/git/ignore` (or `~/.config/git/ignore`), `core.excludesFile` | R | status, add | [G22] |
| `$XDG_CONFIG_HOME/git/attributes`, `core.attributesFile` | R | checkout, diff | [G22] |
| `~/.git-credentials`, `$XDG_CONFIG_HOME/git/credentials` (plaintext) | R, W | `credential.helper=store` | [G30] |
| cache-daemon Unix socket (current user only) | connect, create | `credential.helper=cache` | [G31] |
| `/dev/tty` | R/W | password prompt, when no askpass is set | [G32][G29] |
| `/etc/mailname`; the passwd entry of the uid; the host name | R | default commit identity | [G28] |
| the editor, pager, hook, ssh programs and anything `core.sshCommand` names | exec | see process model | [G22][G33] |
| submodule directories named by `.gitmodules` | R/W | `submodule update` | [G26] |
| the source repository of a local clone (hard links) | R, link | `git clone <path>` | [G25] |

Discovery walks up. The lookup for `.git` starts in the current directory and goes to its parents. `setup.c` implements
the stop conditions `GIT_CEILING_DIRECTORIES` and, by default, a file-system boundary unless
`GIT_DISCOVERY_ACROSS_FILESYSTEM` is set [G19]. Since CVE-2022-24765 git also checks the owner. The fixed release notes
describe the attack: on a multi-user machine a user may unexpectedly find themselves in a worktree that another user
created, for example in `C:\.git`, on a mounted drive or in a scratch space; merely running a git-aware shell prompt
(`git status`) in a directory that is supposedly not a repository can then run commands defined by that other user [G23].
The implementation, `ensure_valid_ownership`, verifies the owner of the worktree, the git directory and the gitfile
against the current user and consults `safe.directory` for exemptions [G19]. This is the best primary statement of why
"git finds its repository by looking upward" and "git runs programs named in the repository's config" must not meet
(inference: the ownership check is git's substitute for a capability).

Configuration is the executable surface. `core.hooksPath` selects the directory in which hooks are looked up, absolute
or relative to where hooks run [G22]. `core.editor`, `core.pager` and `core.sshCommand` name programs; `core.sshCommand`
is used by fetch and push instead of `ssh` [G22]. `includeIf` pulls arbitrary paths into the configuration [G22].
Repository-local `config` can set these, so a cloned repository's own config (written by the user's later commands, or
by tools) is trusted input. The `GIT_ASKPASS` / `core.askPass` / `SSH_ASKPASS` chain runs another program to get a
password, and with none set git prompts on the terminal [G29]; the terminal prompt opens `/dev/tty` for input and
output and uses `tcsetattr` to turn echo off [G32]. Submodules: CVE-2018-11235 was that a submodule name from the
untrusted `.gitmodules` was appended to `$GIT_DIR/modules` so `../` in a name could place a repository elsewhere; the
fix constrains names [G26]. Local clones hard-link `.git/objects/` files unless `--no-hardlinks` [G25]. Alternates are
documented as one path per line, and the HTTP fetcher also tries to use them remotely [G21].

Locking is by file creation, not `fcntl`. The `lockfile.h` comment says a lock is `<file>.lock` created with
`O_CREAT|O_EXCL`, written, then renamed over the destination; readers do not block but see the old or the new content,
"assuming that the filesystem implements `rename(2)` atomically" (quoted from [G24]); lock files are removed by an
`atexit` handler and a signal handler [G24]. So git needs: exclusive create, atomic `rename` over an existing name,
unlink, and directory creation in the directory that holds the file (the lock sits next to the target; inference).
Other filesystem behaviours are configurable: `core.fileMode` (executable bit honoured), `core.symlinks` (false
checks symlinks out as small files holding the link text), `core.ignoreCase`, `core.trustctime`, and `core.checkStat`
(`minimal` leaves out the sub-second times, uid, gid and inode number from the stat comparison); `clone` and `init`
probe the file system and set these [G22]. `core.fsyncMethod` and `core.fsync` choose the hardening of repository
writes [G22]. The index stores stat data, so a file system without stable inode numbers or ctime is handled with these
settings (inference from [G22]).

Identity. Without `user.name`/`user.email` git derives them from the passwd entry, the host name and `/etc/mailname`;
if there is no passwd entry for the uid it falls back to the name "unknown" and the gecos "Unknown" and marks the
identity bogus [G28]. Git therefore runs without a user database. Commit times need a wall clock (inference).
Signing programs were not examined.

Network. The smart-HTTP transport is run by `git-remote-http(s)` built on libcurl [G13]. The host contacted is the
URL's host. `http.followRedirects` defaults to `initial`: redirects are followed for the first request only, and the
redirected URL becomes the base of follow-up requests; `true` follows any redirect, `false` treats redirects as errors
[G22]. So a clone of `https://A/x` can end up talking to host B after an initial redirect (inference from [G22]).
Submodules and LFS use other URLs (not fetched; not claimed beyond `.gitmodules` in [G26]). Credentials: configured
helpers (`store` writes plaintext to `~/.git-credentials` [G30]; `cache` keeps them in a daemon's memory behind a Unix
socket for a timeout [G31]), then askpass, then terminal [G29]. `GIT_TERMINAL_PROMPT` appears in the environment list
[G33]. gitoxide documents the same mapping problem of rewritten and redirected URLs [G4].

### ssh implementations
OpenSSH's `LICENCE` says all components are under a BSD licence "or a licence more free than that" and that OpenSSH
contains no GPL code. It lists the mixed pieces: Tatu Ylonen's original text (use for any purpose, derived versions
must not be called "ssh" if they break the protocol), a few BSD-style files, public-domain parts, a 3-clause BSD part,
and additional licences for the portable tree [G48]. Dropbear: "MIT-style" licence, with `LICENSE` stating that all
components are "fairly non-restrictive" and the main text is the MIT text of Matt Johnston and Mihnea Stoenescu [G53][G54].
Dropbear's site says it can compile to a 110 kB statically linked binary with uClibc on x86 with minimal options
[G54]. No OpenSSH size is stated in the sources read.

OpenSSH `INSTALL`: required are a C89 compiler and privilege separation support (the sshd user, group and directory);
optional are zlib and libcrypto from LibreSSL (3.1.0+), OpenSSL (1.1.1+), AWS-LC or BoringSSL. "Building without
libcrypto is supported but severely restricts the available ciphers and algorithms"; `--without-openssl` builds
"only a subset" [G47]. The release notes mention that the libcrypto-less build uses an internal ed25519 (SUPERCOP)
[G46]; I did not verify which release made libcrypto optional. Entropy: OpenSSH relies on libcrypto reading
`/dev/random`, or failing that on prngd/egd (with `--without-openssl` the `--with-prngd-socket` option must match the
socket) [G47]. Dropbear bundles its own crypto and big-number code (libtomcrypt, libtommath: named by the SMALL notes
only as `LTM_CFLAGS` for libtommath [G55]; libtomcrypt not verified).

Table 3. OpenSSH and Dropbear.

| Aspect | OpenSSH portable | Dropbear | Source |
|---|---|---|---|
| Licence | BSD-style mix, no GPL | MIT-style | [G48][G53] |
| Size | not stated in the fetched sources | 110 kB static, minimal options (uClibc, x86) | [G54] |
| Crypto | libcrypto optional; reduced algorithm set without it | own bundled code | [G47][G55] |
| Programs | `ssh`, `sshd`, `ssh-agent`, `ssh-add`, `ssh-keygen`, `sftp`, `scp`, `sftp-server` | `dropbear`, `dbclient`, `dropbearkey`, `dropbearconvert`; can be one multi-call binary | [G56][G54] |
| Config file for the client | `~/.ssh/config`, `/etc/ssh/ssh_config` with `Host`/`Match`/`Include` | none; `-o` takes OpenSSH-format options (a subset such as `ProxyCommand`, `StrictHostKeyChecking`, `ForwardAgent`) | [G35][G58] |
| known_hosts | `~/.ssh/known_hosts`, `known_hosts2`, `/etc/ssh/ssh_known_hosts`, hashed names, markers | single `~/.ssh/known_hosts`; `-y` accepts unknown keys, `-y -y` disables checking | [G35][G58][G60] |
| Agent | `ssh-agent` + `ssh-add`; constraints | dbclient uses an OpenSSH-style agent; server forwards agent | [G44][G58][G54] |
| Jump hosts | `ProxyJump`, `-J`, `ProxyCommand` | `user1@hop1,user2@hop2,dest` multi-hop through TCP forwarding; `-J cmd` is a proxy command | [G35][G54][G57][G58] |
| sftp | client and server included | no sftp client; the server only starts an external `sftp-server` if `DROPBEAR_SFTPSERVER` and a path are set | [G59][G57][G58] |
| Server | needs passwd, privilege separation, ptys | needs ptys (`openpty`), passwd for multiuser; inetd mode `-i` | [G49][G61][G59][G57] |
| Client needs a passwd entry | yes: exits with "No user exists for uid" | no: uses `$HOME`, falls back to the passwd entry | [G36][G60] |

### What the ssh client needs
`ssh.c` calls `getpwuid(getuid())` and, if there is no entry, logs "No user exists for uid" and exits with status 255;
it makes a copy of the structure [G36] (the check sits right after the random generator is seeded [G36]). The user's
home directory comes from that entry. In Dropbear, the client reads `$HOME` first and falls back to the passwd entry
only if `HOME` is unset [G60]. OpenSSH uses Unix-domain sockets for the agent (`SSH_AUTH_SOCK`) and for connection
sharing (`ControlPath`) [G35][G44], and `fork`s for `ControlPersist` and `-f` backgrounding (inference; not read in
the source). The server needs more: privilege separation uses a `chroot` directory (`/var/empty` by default, owned by
root) during pre-authentication [G49], and file checks under `StrictModes` [G50].

### Files the ssh client touches unasked

Table 2. Files and paths the ssh client reads or writes without being told (client side first).

| Path | R/W | When | Source |
|---|---|---|---|
| `/etc/ssh/ssh_config`, `~/.ssh/config` | R | always; the user file must be writable only by its owner (readconf: owner root or the uid) | [G34][G38] |
| files named by `Include` (globs, tokens, `~`) | R | config parsing | [G35] |
| `~/.ssh/known_hosts`, `~/.ssh/known_hosts2` | R, W (adds new keys) | every connection | [G35][G34] |
| `/etc/ssh/ssh_known_hosts` | R | every connection | [G34] |
| `~/.ssh/id_rsa`, `id_ecdsa`, `id_ecdsa_sk`, `id_ed25519`, `id_ed25519_sk`, `id_mldsa44_ed25519` (+ `.pub`, certificates) | R | public-key authentication | [G34][G35] |
| agent socket `SSH_AUTH_SOCK` or `IdentityAgent` | connect | authentication; forwarding | [G35][G44] |
| `ControlPath` socket | create (master) or connect (client) | `ControlMaster` | [G35] |
| `/dev/tty`; `SSH_ASKPASS` program | R/W, exec | passphrase prompt | [G42][G34] |
| `PKCS11Provider`, `SecurityKeyProvider` shared library | load | smartcard or FIDO keys | [G35] |
| `~/.ssh/rc`, `~/.ssh/environment`, `~/.ssh/authorized_keys` | read by sshd on the server | login | [G34] |
| the programs named by `ProxyCommand`, `LocalCommand`, `KnownHostsCommand`, `Match exec` | exec | see below | [G35] |
| the local tty | R/W, `tcsetattr` | interactive session | [G41] |

Permission checks use Unix modes and owner. For a private key owned by the current uid whose mode has any group or
other bit (`st_mode & 077`), `authfile.c` prints a warning about an unprotected private key file, "Permissions ... are
too open", and the key is ignored [G37]; `ssh(1)` says the same ("ignore a private key file if it is accessible by
others") and also requires the user config to have strict permissions [G34]. The config file check uses owner and
write bit [G38]. A system with no users or modes would need either a different notion (the file was reached by a grant
that only this program holds) or these checks turned off (inference).

### Host keys
`StrictHostKeyChecking`: `yes` never adds keys and refuses changed keys; `accept-new` adds new keys automatically and
refuses changed keys; `no`/`off` adds new keys and lets connections to hosts with changed keys proceed "subject to some
restrictions"; `ask` (the default) adds new keys only after the user confirms, and refuses changed keys [G35]. A
changed key produces a banner warning that the identification "has changed", that someone could be eavesdropping, and
prints the new key's fingerprint; the client also reports the offending key with the file name and line number from
which it came, and refuses [G39]. `UpdateHostKeys` lets the server send replacement keys after authentication, which are
added to the user's `known_hosts` only if the authenticating key was already trusted [G35]. `VerifyHostKeyDNS` checks
SSHFP records (default no) [G35]. `HashKnownHosts` stores hashed host names so the file does not reveal them (default
no) [G35]. `CheckHostIP` also checks the IP address (default no) [G35]. `KnownHostsCommand` runs a command to obtain more
host keys; a non-zero exit ends the connection [G35]. The `@cert-authority` and `@revoked` markers are described in
`sshd(8)` (not read here). Dropbear: `StrictHostKeyChecking` accepts `yes`, `no`, `accept-new`, `ask`; `-y` accepts
unknown keys and aborts on a mismatch, a second `-y` disables the check [G58].

### The ssh agent
`ssh-agent` holds private keys and creates a Unix-domain socket whose path is exported as `SSH_AUTH_SOCK`; the manual
says it is accessible only to the current user and "easily abused by root or another instance of the same user" [G44].
The protocol is the IETF agent draft (cited by OpenSSH's `PROTOCOL.agent`) plus OpenSSH extensions [G43]. Holding the
socket confers the use of every loaded key to sign: `ssh -A` and `ForwardAgent` carry the warning that a user able to
bypass file permissions on the remote host can reach the agent through the forwarded connection, cannot obtain key
material, but can authenticate with the loaded identities; a jump host is named as the safer alternative [G34][G35].
Keys never leave the agent (the manual's own phrasing is that the attacker "cannot obtain key material") [G34]. Controls:
`ssh-add -c` makes each use require confirmation through `ssh-askpass` [G45]; `ssh-add -h` constrains a key to be usable
only from the agent's host to named destination hosts [G45]; this system was added in the release whose notes say it
adds "a system for restricting forwarding and use of keys" (the OpenSSH 8.9 release; the later bug note names
"destination constraints (`ssh-add -h ...`) added in OpenSSH 8.9") [G46]. The `session-bind` (an openssh.com extension) extension binds
an agent connection to a session id and the host key's signature of it, so the agent can test the constraints [G43].
FIDO keys needing a PIN can be added to the agent, with `$SSH_ASKPASS` used for the PIN [G46]. This is a capability-like
design: a process holds only a connection to the agent, which can be made to sign but not to reveal (inference).

### ssh configuration that runs programs or changes the host
- `HostName` replaces the name typed on the command line; the default is the typed name [G35]. With `Host` blocks in
  `~/.ssh/config`, `ssh work` may contact any host and port the config says. `Port` and `User` likewise [G35].
- `ProxyCommand`: a command run with the user's shell via `exec`, which "can be basically anything"; host-key
  management uses the `Hostname`, not the proxy's address; `CheckHostIP` is not available with it [G35].
- `ProxyJump`: opens an ssh connection to each jump host, then TCP forwarding to the target; the destination's config is
  generally not applied to the jump hosts, which read `~/.ssh/config` themselves [G35].
- `LocalCommand` (needs `PermitLocalCommand`, runs through the user's shell), `RemoteCommand`, `Match exec` (the
  command runs under `$SHELL`), `KnownHostsCommand`, `Include` (can be inside `Match`), `PKCS11Provider` and
  `SecurityKeyProvider` (load a shared library) [G35].
- `LocalForward`, `DynamicForward`, `RemoteForward`: the client listens on a local TCP port or Unix socket and relays
  to a destination; the bind address decides whether others can use it (`GatewayPorts`, default bound per that setting;
  `localhost` restricts to local use) [G35]. Others on the machine, or the network, thus get the client's reach.
- `ControlMaster`/`ControlPath`/`ControlPersist`: the first connection becomes a master listening on a Unix socket;
  later `ssh` invocations reuse it, and the master may stay in the background indefinitely [G35]. The manual
  recommends placing the socket in a directory not writable by others [G35].
- What `ssh host` contacts: the name resolved (`HostName` aliasing) through DNS, then that host's port 22 or the `Port`
  from the config, unless a proxy applies [G35][G34]. A system that derives the network grant from the command-line
  operand would grant the wrong name whenever `~/.ssh/config` rewrites it (inference).

### ssh and the terminal
When a pty is wanted the client puts the local tty in raw mode with `tcsetattr` (`VMIN = 1`) and restores it on exit
[G41]. It sends a `pty-req` containing the value of `TERM`, the window size (columns, rows, pixels from `TIOCGWINSZ`)
and the encoded terminal modes taken from the saved termios [G40][G36]. The RFC defines `pty-req` as the `TERM` string,
four dimensions and the encoded modes; zero dimensions must be ignored and the dimensions are informational [G52 6.2].
The client catches `SIGWINCH`, sets a flag, and then sends window-change messages [G40]; the RFC's `window-change`
request carries the new columns, rows and pixel sizes and expects no reply [G52 6.7]. The escape character `~` is
recognised after a newline when a pty exists; `~.` disconnects, `~^Z` backgrounds ssh; `-t`/`-T` force or disable
allocation, and the default is to ask for a pty only if the client has one [G34]. Passphrases are read from `/dev/tty`
with echo off, or from the `SSH_ASKPASS` program if there is no terminal and `DISPLAY` is set (or `SSH_ASKPASS_REQUIRE`
says so) [G42][G34]. The `TERM` value is passed on unchecked: the server does not translate it, so a terminfo name the
remote machine does not know is the user's problem (inference; the RFC says only that it is the environment variable's
value [G52 6.2]).

### scp, sftp and the server
The release notes state that OpenSSH switched `scp` from the legacy scp/rcp protocol to the SFTP protocol by default in
the release after 8.9 (the notes' heading is "Changes since OpenSSH 8.9"; the legacy protocol expands wildcards via the
remote shell) [G46]. Both programs run `ssh` as a subprocess (the release notes mention passing `ControlMaster no`
to it) [G46]; their local file access was not read from the man pages. The mux protocol for `ControlMaster` is a
length-prefixed packet protocol over the Unix socket [G51].

`sshd` options: `-i` says it is run from inetd [G49]; Dropbear's `-i` is "service program mode" for inetd, tcpsvd or
tcpserver, implies `-F`, and ignores `-p` [G59]. In both, a listener hands the accepted connection to the server as
stdin/stdout. Dropbear allocates ptys with `openpty()` [G61]. `sshd` checks file modes under `StrictModes` (default
yes) [G50].

### Other small SSH implementations

| What | Licence | Note | Source |
|---|---|---|---|
| libssh2 | BSD-3-Clause (GitHub licence detection of `COPYING`) | client library, as used by curl and libgit2 (not verified here) | [G62] |
| libssh | LGPL 2.1 (the `COPYING` text) | client and server library | [G63] |
| wolfSSH | GPLv3 or commercial | | [G64] |
| russh | Apache-2.0 | async Rust SSH 2.0 client and server library on Tokio | [G65] |
| tinysshd | `CC0-1.0 OR 0BSD OR MIT-0 OR MIT` | server only; no password auth, port or agent forwarding, no compression, subset of SSHv2 | [G66] |
| Plan 9 ssh | not fetched | | |

## Capability systems and interactive tools

Scope. Zoraya already surveys the GUI powerbox (CapDesk, Polaris, EROS SaveAs agent, macOS open panel, xdg portal;
`docs/research/desktop/survey.md`, "Capability-oriented desktops: the powerbox", and ADR-0167) and the capability
shell shapes (Plash `=>`/`+`, SHILL, Deno flags, `unveil`; `docs/research/shell/survey.md`, "Capability shells and
grant syntax"). This file does not repeat them ("already in shell/survey.md", "already in desktop/survey.md"). It
asks the narrower question: what do systems do when `vi` is given one file and the person types `:e other.txt`?
Source tag prefix `K`. Where the task text expected something that the sources do not show (an Emacs or vi
integration of Plash), that is said in the section where it matters.

Short version of the evidence. No system in this survey has a shipped text-mode powerbox that a program asks for a
named file in the middle of a run, except Deno's terminal prompt (and Deno's prompt was spoofed twice). Everything
else is one of: grant on the command line and fail otherwise; grant the whole directory tree; give the editor
ambient file access and confine something else; or hold the grant in the terminal or session, not the program.

### Pattern (a): grants fixed at start

**Plash.** Fetched from a web.archive.org copy (the live host fails TLS; see the list at the end) [K1]. The shell
rule is: files in an argument list are passed read-only; `=>` makes the objects to its right read-write "slots", so
the object need not exist in advance; `+` attaches an object into the callee's namespace without putting its name in
`argv`; a directory on the left of `=>` is recursively read-only [K1]. Arguments that are not file names must be
quoted so the shell can tell which words to grant [K1]. The shell gives every program a default read-only
"installation endowment" of `/bin`, `/lib`, `/usr`, `/etc` and read-write `/dev/null` and `/dev/tty`, not
configurable in the shell version fetched [K1]. `pola-run` is the programmatic form, grants nothing unless asked,
and takes `-f`/`-t` with options `w` (read-write to the slot), `a` (add to argv), `l` (follow symlinks) [K2]. Under
the hood Plash replaces glibc so that `open()` goes to a server over a socket; a program that bypasses glibc sees
only an empty chroot jail [K5], and the process finds the server through the environment variables `PLASH_COMM_FD`
and `PLASH_CAPS` [K6]. So for a program run as `vi notes.txt` under `pola-shell`, `:e other.txt` fails: the name
does not exist in the namespace (inference from [K1], [K5]; the fetched pages never mention vi or an editor).

I found no Emacs or vi integration in Plash. The nine Plash wiki pages I fetched (FrontPage, FilePowerbox, GtkFilePowerboxHook, pola-shell, PolaRun, PlashFAQs,
ProtocolEnvVars, PlashLimitations, ComparisonWithOtherSystems; [K7]) contain no "emacs" (checked with grep on the saved pages), and a web search for it found
none [K3]; an Emacs-devel thread on sandboxing is about capabilities inside Lisp, not the OS [K49]. Plash's only dynamic-grant mechanism is the file powerbox, a trusted separate process; the application
asks with a "powerbox_req_filename" object that `pola-run --powerbox` hands out, and the manager shows a chooser
titled with a pet name [K2]. That is a GUI chooser; `--powerbox` "only makes sense" when the caller runs with the
user's own namespace, because the chooser lists the caller's files [K2]. The task's idea of an environment-variable
text-mode powerbox request is not in the fetched pages; the environment variables found carry only the server socket
and object names [K6].

**WASI and Wasmtime.** `--dir=.` preopens a directory and makes it "available to the program as a capability"; the
runtime's libc maps path names to the preopened directories [K37]. A path outside fails even when the host user could
write it: `/tmp/../etc/passwd` gives "Operation not permitted", and the tutorial points out that this is the
capability system speaking, not Unix permissions; symlinks are covered the same way [K37]. `--dir=/var/tmp::/tmp`
remaps a host directory to a different guest name [K37]. The tutorial says the model can later grow "the ability for
applications to explicitly request capabilities" [K37]; that is future work, not shipped.

**CloudABI.** There is no global namespace and no open by absolute path; a process that lacks a resource "will not be
able to open any new resources", and opening a file needs a descriptor for its directory [K45]. The README page I
could read does not discuss interactive use, and the `cloudabi-run` YAML mechanism was not on the page [K45]. It is
the strictest form of this pattern.

**Genode.** A file-system server decides per client label: in `lx_fs`, "By default, no permissions are granted to any
session", and a `<policy label=...>` node gives a `root` (the viewport) and optionally `writeable="yes"` [K31]. A
component sees only the file-system sessions its parent routes to it [K33, partly: the page says only that RAM-fs
sessions "may be restricted in different ways"]. In Sculpt's deploy GUI the person picks, in the component's
configuration dialog, which file system satisfies each `File_system` request; the closest editor example is the
"system shell" hosting Vim [K32]. So the grant is decided when the component is deployed, by the person, in a GUI
that the component does not control; there is no mid-run request.

**Fuchsia.** A component starts with a table that maps path prefixes to object handles; "no ambient authority" [K34].
Servers block `..`, so a directory handle cannot climb out; `foo/../bar` is cleaned on the client side [K35]. The
pages fetched do not cover how rights narrow when opening children, so that claim is not made here.

**Shill.** Directory capabilities carry privileges such as `+lookup`, `+contents`, `+path`; `lookup` on a
capability cannot reach the parent (`lookup(cur, "..")` is refused) [K43]. An unmodified executable runs in a
sandbox that gets exactly the capabilities passed to `exec`, enforced by a MAC policy module [K43]. The paper says
outright that Shill "is not an interactive shell" [K43]. Already in shell/survey.md otherwise.

**Capsicum.** `cap_enter()` removes global namespaces; file-descriptor rights replace path access; `openat` with a
directory capability refuses absolute paths and `..` [K44]. The paper's `gzip` conversion keeps the argument-parsing
loop with ambient authority, opens the files, and sends pairs of descriptors into a sandbox [K44]. `libcapsicum`
also gives the sandbox an RPC channel that can receive "run-time delegated capabilities" [K44]: the mechanism for
later grants exists, the policy is the application's. The FreeBSD Capsicum wiki page (the list of converted
programs) was blocked by a bot wall, so I do not claim that any editor or pager has been converted.

**Landlock, bubblewrap, Node.** Landlock rulesets are fixed at `landlock_restrict_self()` and can only be narrowed
afterwards [K40]. `bwrap --ro-bind` / `--bind` builds a mount namespace whose contents are fixed at start [K41]. Node's
`--permission --allow-fs-read=...` is called a "seat belt", not a sandbox against malicious code; denial throws
`ERR_ACCESS_DENIED` with `permission` and `resource` fields; the page describes no prompts [K39]. Node also notes that
operations through already-open descriptors bypass the model [K39].

**Flatpak `@@`.** This is exactly "arguments on the command line become grants". `flatpak run --file-forwarding app
@@ file @@` scans the remaining arguments, treats those between `@@` markers as paths, exports them in the document
store and passes the resulting document path; `@@u` is for URIs; the exports are non-persistent and give the app read
and write [K22]. Note the default is read-write, the opposite of Plash's default.

**What the person sees.** With every system in this pattern the person who types `:e other.txt` sees an ordinary
error from the program, written by the program from `errno` (`ENOENT`, `EACCES`, "Operation not permitted") [K20,
K37, K39]. Nothing says why. Section (e) lists the messages.

### Pattern (b): a trusted intermediary that asks

**Deno's terminal prompt.** This is the one shipped text-mode instance. With no `--allow-*` flag, on a terminal,
Deno prompts with a line of the form "Deno requests net access to ..." and the choices `y/n/A` (A allows all of that
kind) [K8]. `--allow-read=./data` scopes a grant; a refused read throws `NotCapable`; `--no-prompt` makes later
reads "fail outright"; with a permission broker all flags and prompts are ignored [K8]. Subprocesses do not inherit
the restrictions, and `-A` "turns the sandbox off entirely" [K8]. The prompt is drawn by the runtime on the same
terminal that the program writes to, which is its weakness:

- CVE-2023-28446 (GHSA-vq67-rp93-65qf): the prompt for running subprocesses included text supplied by the program
  (program name, "Requested by" message) without removing ANSI escapes; the program could erase the real prompt with
  cursor-movement codes and print a fake one. The proof of concept showed `echo` while running `cat /etc/passwd`.
  Fixed in 1.31.2; affected 1.8.0 and later [K9].
- CVE-2024-27936 (GHSA-m4pq-fv2w-6hrw): the fix stripped escapes from the displayed text, but the permission was
  computed from the unstripped text. A read request for `/tmp/hello\u001b[/../../etc/hosts` displayed as
  `/tmp/hellotc/hosts` and normalised to `/etc/hosts`. Fixed in 1.41.0 [K10].

Both are one flaw: display and decision were computed from different strings, and the drawing surface was shared
with the program (inference). Neither advisory gives a rate of approvals or prompt fatigue; I found no primary source
with such a number.

**Qubes.** The nearest command-line analogue with a trusted destination chooser: `qvm-copy file...` names source
files on the command line; the file manager route opens a dom0 dialog where the person names the target qube [K47].
The page I fetched does not say how `qvm-copy` names the destination, so only the dialog route is claimed. The GUI
side: `qubes-guid` paints a coloured frame per qube, and the title always starts with the qube's name, drawn on the
GUI-domain side, so a compromised qube cannot claim to be another [K46]. The rest of the title comes from the
untrusted qube; the doc says the protection covers origin, not the whole title [K46].

**macOS panels.** Already in desktop/survey.md; new points from Apple's current page [K26]. The panel runs in a
separate process and extends the sandbox to the chosen URLs; the OS starts security-scoped access on URLs from open
panels, save panels or items dragged to the app's Dock icon [K26]. The page says a sandboxed app that wants access
across launches must store a security-scoped bookmark [K26]. For companion files there are two documented
mechanisms: "document-relative bookmarks" (an IDE project file that references source files, bookmarks stored
relative to the document) and *related items*: the app declares `NSIsRelatedItemType` document types and reads the
supporting file through an `NSFilePresenter` whose `primaryPresentedItemURL` is the document, via `NSFileCoordinator`
[K26]. The page does not say that command-line arguments are an implicit grant (my first assumption); I did not find
it stated, so I do not claim it.

**TCC prompts for the terminal.** Apple staff state that when a process touches an item restricted by the MAC
subsystem "the system may prompt the user to grant it access there and then"; the alert is shown once and the
answer remembered; the Files and Folders prompts appear only in a GUI login session, otherwise access is denied when
there is no stored decision [K28]. The system finds a "responsible code" for a helper tool [K28]. A Qt engineer
measured it: launched from a terminal, the prompt names the terminal (iTerm2 in the article), the decision is stored
against it, and "permissions are inherited by child processes"; `launchctl procinfo` shows the responsible path
[K29]. This is a real prompt that a text-mode user sees, but it grants a protected folder to everything run in that
terminal app, not one file to one program (inference, see pattern (d)). Apple also says some access (full disk
access) cannot be requested in code; the person must grant it in System Settings [K26].

**Flatpak portals.** The FileChooser portal has `multiple`, `directory` (since v3), `current_folder` ("the portal
implementation is free to ignore this option") and returns `uris` [K24]. Selected files "will be made accessible" and
may be added to the Documents portal [K24]. The Documents portal exposes files at
`/run/user/$UID/doc/$DOC_ID/filename` through FUSE; inside the sandbox only the files that app may access appear;
`Add()` takes an `O_PATH` descriptor, `AddNamed()` takes a parent directory descriptor and a file name "for writing a
new file"; `AddFull()` has flags including `as-needed-by-app` and (v4) `export-directory` [K23]. Permissions are
`read`, `write`, `grant-permissions`, `delete` [K23]. The portal page is silent on atomic save and sibling files
(I looked: no mention) [K23]. Flatpak's own guidance prefers the portal to broad `--filesystem=home` or `host` [K25].

**Android.** `ACTION_OPEN_DOCUMENT_TREE` (API 21+) "lets the user choose a directory for a client app to access", and
`ACTION_OPEN_DOCUMENT` gives long-term access to documents; the user can "add, edit, save, and delete files on the
provider" through it [K30]. Directory grant is a first-class option on a mobile capability-ish system; the page does
not discuss sibling or temp files.

**Plan 9's plumber.** A program writes a message (source, destination port, working directory, data type,
attributes, data) to `/mnt/plumb/send`; rules in `lib/plumbing` choose the port or start a program; with no matching
rule the write fails and the sender knows [K36]. The paper's security remarks are minimal: each user runs a plumber
and access follows file permissions [K36]. So the plumber routes a request for "open this" to another program; it
does not move authority (inference: Plan 9 has no ambient authority problem the same way, since namespaces are
per-process, but the plumber itself is not a grant mechanism).

**What a trusted prompt must have.** The sources agree on three items: a region the program cannot draw in; input the
program cannot synthesise; and a way for the person to tell where they are. Windows: Winlogon "registers" Ctrl+Alt+Del
"to keep other programs and processes from using it", and switches to a separate "secure desktop" invisible to
applications [K48]. Qubes: the frame and name prefix are painted on the GUI-domain side [K46]. EROS Window System:
already in desktop/survey.md. Terminals: bubblewrap's tracker shows the opposite, a sandboxed program that keeps the
controlling terminal can inject characters into its input with `TIOCSTI`, and a shell then runs them with the user's
rights (CVE-2017-5226); the fix is `setsid()`/`--new-session`, at the cost of job control [K42, K41]. Deno's two
advisories show the output side [K9, K10]. HD Moore's 2003 terminal-escape survey and CVE lists for title-report
injection were not fetched, so no claim about them.

### Pattern (c): a trusted helper with a fixed policy

**Casper `system.fileargs`.** `cap_fileargs(3)`: the files named in `argv` after option parsing are the allowed
set; a service opens them for the program after it has entered capability mode; `NULL` creates no service and blocks
all opens; the operations are `FA_OPEN`, `FA_LSTAT`, `FA_REALPATH`; flags and `cap_rights_t` limit the opened files;
the page calls the interface experimental and says it first appeared in FreeBSD 10.3 [K11]. libcasper lists it among
`system.dns`, `system.grp`, `system.net`, `system.pwd`, `system.sysctl`, `system.syslog` [K12]. In the FreeBSD source
the users that a GitHub search returned are `head`, `wc`, `paste`, `ident`, `tail`, `brandelf`, `md5`, `savecore`,
`cat` (`bin/cat`) and the elftoolchain `nm`, `size`, `strings`, `readelf` [K13]; I read `head.c`, `wc.c`, `tail.c` and
`cat.c`: each calls `fileargs_init(argc, argv, O_RDONLY, 0, &rights, FA_OPEN)` or equivalent and then
`caph_enter_casper()` [K14]. The search tool may not list every user. These are non-interactive filters: the name set
is exactly what the shell expanded. No editor or pager appears; `less` and `vi` are not in that list [K13]. The
pattern cannot serve `:e`, because the set is closed at `cap_enter()`.

**OpenBSD `unveil`.** `unveil` fixes a list of paths with permissions `r`, `w`, `x`, `c`; the first call hides the
rest of the filesystem; the list can be locked with `unveil(NULL, NULL)` [K20]. A path outside gives `ENOENT` if no
unveil matched, `EACCES` if one matched with the wrong permission [K20]. An `execve` starts the new program "without
any unveils" [K20]. Real programs that use it (from source, `got.c` in the portable mirror of OpenBSD's Game of
Trees [K18]; `gotd.c` [K19]):

- `got`'s `apply_unveil(repo, read_only, worktree)` unveils the repository path `"r"` or `"rwc"`, the worktree path
  `"rwc"`, a temporary directory `"rwc"`, the privsep helper executables, then calls `unveil(NULL, NULL)` [K18]. The
  pattern `unveil(` appears on 53 lines of `got.c` (`grep -c`). When a command needs an editor for a commit message,
  it additionally unveils the editor binary `"x"` and the message file `"r"` [K18]. Because an `execve` drops the
  unveils [K20], the editor that `got` starts is unrestricted (inference). So `got`'s answer for "the editor needs any
  file" is: the editor is not confined.
- `gotd` has `apply_unveil_repo_readonly` and `_readwrite`: one repository path `"r"` or `"rwc"` plus a tmp dir [K19].

For the tools in the question the three OpenBSD programs that I read call `pledge` but not `unveil`, measured by
grep on the fetched files [K15, K16, K17]:

| tool | `pledge` promises (source line) | `unveil` calls |
| --- | --- | --- |
| `vi` (`usr.bin/vi/common/main.c`) | `stdio rpath wpath cpath fattr flock getpw tty proc exec` at entry (line 72); with `-S` the later call drops `proc exec` (line 225) [K15] | none |
| `mg` (`usr.bin/mg/main.c`) | `stdio rpath wpath cpath fattr chown getpw tty proc exec` (line 74) [K16] | none |
| `less` (`usr.bin/less/main.c`) | `stdio rpath wpath cpath fattr proc exec tty` (line 98); with `LESSSECURE` set, `stdio rpath tty` (line 93 and 227) [K17] | none |

`pledge` promises are coarse: `rpath`/`wpath`/`cpath` allow opening, writing, creating, removing, with about three
dozen subsystems in all, and violations kill the process with `SIGABRT` [K21]. The man page describes no path limit in
`rpath`/`wpath`, so these editors keep every file the user could name (inference from [K21] and the table). The man
page for `unveil` gives the reason: "programs misbehave badly when their files unexpectedly disappear. In many cases
it is easier to unveil the directories in which an application makes use of files" [K20]. In other words, the
OpenBSD developers do not confine the *set of files* of vi, mg or less; they remove other powers (network, ioctls,
`proc`/`exec` in vi's `-S` and less's `LESSSECURE`). The `LESSSECURE` and `-S` modes also remove the commands that
need those powers (shell escape, `|`), so the cost of "refuse" is paid in features (inference from the pledge change
and the option names; I did not read less's command table).

### Pattern (d): the session holds the grant

**macOS TCC.** See (b): the terminal application is the responsible process, so everything the shell starts shares one
decision per protected folder [K29, K28]. The grant is large (all of Documents) and invisible once given.

**Directory grant to the session.** `bwrap --bind PROJECT /work` or Landlock with a rule on the project directory give
a session (an agent, an editor, a build) a read-write tree and nothing else [K41, K40]. Landlock separates the rights:
`MAKE_REG` creates files, `REMOVE_FILE` unlinks "or renaming one", and `REFER` is the only right denied by default and
is needed to rename or link between directories [K40]. The known weakness of "grant the whole project": everything in
the tree is writable, including version-control hook scripts and dotfiles (inference; I did not fetch a source for
git hooks).

**Qubes domains.** The unit of authority is the qube: file transfer between qubes is a deliberate act with a
dom0-drawn prompt, and the doc warns that copying from a less trusted to a more trusted qube can attack the receiving
parser [K47, K46]. Mid-run requests by a program for another qube's file do not exist.

**Android and Genode** (see (a) and (b)): the directory grant lives in the app's persisted permission or in the
deployment configuration, not in one run.

**Rooms (Zoraya).** Already in ADR-0172; the survey finds no other system with the same shape, except Genode's
deployed subsystems and Qubes qubes, both of which are chosen outside the program's run.

### Pattern (e): refusal and what the person sees

| system | what the program sees | source |
| --- | --- | --- |
| unveil | `ENOENT` (no matching path) or `EACCES` (wrong permission) | [K20] |
| pledge | uncatchable `SIGABRT` (or `ENOSYS` with the `error` promise) | [K21] |
| Wasmtime | "Operation not permitted" | [K37] |
| Deno | `NotCapable` error, catchable; or a prompt on a terminal | [K8] |
| Node | `ERR_ACCESS_DENIED` with `permission` and `resource` | [K39] |
| Capsicum | `ENOTCAPABLE` (paper's name for the error) | [K44] |
| Landlock | `EACCES`; `EXDEV` for a rename that would widen rights | [K40] |
| Plan 9 plumber | write to `send` fails when no rule matches | [K36] |
| TCC | operation denied by default outside a GUI session | [K28] |

A refusal is a number or a string chosen by the program. Only Node attaches the denied resource name in a structured
field [K39]. Nothing in the sources has the kernel or shell tell the *person* "this program asked for X and was
refused"; the closest is macOS's per-app list in Settings [K26] and Node's audit mode, which publishes violations on
a diagnostics channel without throwing [K39].

### Write-by-rename and derived files

An editor that is given one file and saves by writing a temporary name next to it and renaming over the original
needs more than that one file: creation and removal in the parent directory. Swap, backup (`file~`), lock and undo
files are the same. What each system does:

- **OpenBSD `unveil`.** Directories are remembered at the time of the call; a non-directory path is remembered "by
  name within [its] containing directory", so it can be created, removed or re-created later and still count [K20].
  That is the only system in the survey that names this case: an unveiled file name may be replaced under rename.
  A *sibling* name is not unveiled, so `file~` fails; the man page's advice is to unveil the directory [K20].
- **Landlock.** Rename inside a directory needs `REMOVE_FILE` and `MAKE_REG` on the directory; across directories
  `REFER` as well, and the moved file cannot gain rights (`EXDEV`) [K40]. A rule on a single file does not give any
  of these on the parent.
- **macOS.** The app obtains a scratch location from the system (`FileManager.SearchPathDirectory.itemReplacementDirectory`,
  "used to create a temporary directory" [K27]); I did not find the current page's statement of how the sandbox treats
  the replacement, so the claim that safe-save works in the sandbox through `replaceItemAt` is `[unverified]`. Related
  files go through `NSIsRelatedItemType` and `NSFilePresenter` [K26].
- **Flatpak.** `AddNamed()` takes a parent directory descriptor and a name to create a new document entry for
  writing [K23]; the page says nothing about sibling temp files [K23].
- **Plash.** The `=>` operator passes a "slot" (a directory entry) read-write, and the object need not exist [K1]. So
  writing the named file by creating or replacing the entry is within the grant; whether a rename of a temporary
  sibling works is not stated (the slot model suggests no, since the sibling is not a granted slot) (inference).
- **Polaris** (already in desktop/survey.md) fell back to read-only access to the directory when no designation existed.
- **Capsicum.** `openat` on a directory capability can create files in it if the rights allow; the paper's handling
  is by granting directory capabilities [K44].
- **Directory grant** (bwrap, Landlock tree, Genode `root=`, Android tree, Flatpak `export-directory`) makes the
  problem go away at the price of everything in that directory [K41, K40, K31, K30, K23].

No source gives a system that grants a file name *and* its derived names without granting the directory.

## What this suggests for Zoraya

Each list was written with its section and is the researcher's reading, not a decision; the ADRs decide.

### For the terminal

- The one mandatory contract for `vi`, `less`, pyrepl and ncurses programs is terminfo names, not xterm in general: `cup`,
  `clear`, `el`, `bel` are hard requirements in pyrepl, `cup` in vim, and a non-`gn` entry for ncurses [T5][T9][T4]. A first
  terminal must therefore ship a terminfo entry together with the emulator, not only the emulator (inference from
  [T5][T9]).
- The practical minimum for `vi` and `less` is small: CUP, ED, EL, IL/DL or reverse-index, SGR 0/1/4/7 and colours,
  alternate screen 1049, DECTCEM, DECAWM, DECCKM/keypad, CPR/DA1 replies [T6][T7][T5]. `foot+base` and `st` show that
  about 215 to 265 names cover real use; `linux` shows that a usable terminal exists without `smcup` (119 names) [T1].
- `ich`/`ich1` are absent from every entry that was measured, while `dch1`, `il1`, `dl1`, `csr`, `ech` are present in all
  xterm-like entries; implement ICH/DCH/IL/DL/ECH anyway because readline and pyrepl look at them [T1][T9][T10].
- Mouse, bracketed paste, focus events and 2026 are not in terminfo but are hard-coded by less and vim and detected by tmux
  with `terminal-features`; a terminal that ignores unknown private modes safely (as the sources say foot, st and alacritty
  do for modes they list as "ignored") is more important than implementing every mode [T6][T5][T11][T13][T25].
- Direct colour (`38:2` and `38;2`) is not described by `RGB` in the default xterm-256color/foot/alacritty entries, so a
  terminal that wants truecolor programs must either claim an `-direct` entry or tell tmux and vim by their own
  mechanisms; accept both the colon and semicolon forms as foot and xterm do [T1][T2][T13][T11].
- A new `TERM` name is only as useful as its entry's reach: kitty and st document breakage over SSH and `sudo`; the
  ncurses file already carries entries for st, foot, alacritty, wezterm and kitty (names, not xterm-compatible claims) [T1][T15].
  Claiming `xterm-256color` is covered everywhere, but foot's author lists concrete reasons against it [T14]; this is a
  decision for an ADR, not settled by these sources.
- The window-size path is `TIOCSWINSZ`/`tcsetwinsize` on the pty plus SIGWINCH to the foreground group; ncurses reads the ioctl unless `LINES`/`COLUMNS` are set, and pyrepl and readline read those variables before the ioctl,
  so a pty without a settable size breaks them and stale variables override it [T28][T29][T31][T9].
- Width: implement a `wcwidth` equal to the one in the libc that ported programs use (Kuhn-style: Mn/Me/Cf = 0,
  W/F = 2, Ambiguous = 1), because the mismatch problem is between program and terminal, and no standard fixes it [T35][T16].
- Test by recorded sessions as alacritty does (vim, tmux, htop, vttest recordings compared to a grid) plus `vttest` and
  `esctest` run by hand; these need no network in the repository once recorded [T21][T37][T38].

### For curses and terminfo

- ncurses is MIT-style and can be built with `--disable-database --with-fallbacks=xterm,...` so that no terminfo
  tree is read, or with `--with-termlib` to keep only `libtinfo`, which the file puts at about half the total size [C1][C2].
- ncurses' `TERMINFO=hex:`/`b64:` form lets a terminal hand its description to a child through the environment, so no
  `/usr/share/terminfo` has to exist in the program's world [C4]. Python 3.14's own reader does not appear to support
  this form, so Python would still need a file or a different `TERM` choice [C21].
- netbsd-curses is smaller (150 KB vs 346 KB library in the fork's own test), BSD-licensed, has a built-in set of six
  terminals and a single-file CDB database, but needs patches for programs that touch curses internals, and splits
  `libterminfo` out [C8][C9].
- Both ncurses and netbsd-curses expect `termios`, `SIGWINCH` (and `SIGTSTP`) and `poll`/`select` from libc [C5][C7]; Zoraya's
  signals "at wait points" are not mentioned in these sources and must be checked against `SIGWINCH` delivery (inference).
- The toybox/busybox/kilo family avoids the whole layer by emitting ANSI and asking the terminal for its size with
  `ESC[6n`; this needs the terminal to answer cursor-position reports, and toybox's `tty_fd()` opens `/dev/tty` as a last
  resort, which Zoraya does not have [C14][C15][C37][C40].
- Python 3.13 needs `_curses` (or ctypes) plus a terminfo entry for the new REPL; without them it falls back to the
  basic REPL with a printed warning, and the `readline` module is optional [C20][C22].
- If `TERM` names a type that the fallback list contains, `less` and Vim's builtin termcaps need no file at all
  (Vim's builtin list is the same in all feature sets since patch 9.0.0280) [C32].

### For the editor and the pager

- `less` is the best-fitting full-screen tool for a capability system among these: permissive Less License option,
  `LESSSECURE=1` removes the edit, examine, glob, history, lessopen, logfile, pipe and shell features, and it can use
  terminfo only through capabilities of the `setupterm` family [C44][C46][C45].
- Vim's behaviour that the command line never names (`.swp` in the file's directory, `~/.viminfo`, `~/.vimrc`, backup
  copies, undo files) maps to "write grant on the directory" plus several home-directory reads; switches exist to turn
  these off (`-i NONE`, `directory`, `nobackup`), but `uganda.txt` licence terms are not on the permissive list [C30][C34][C35][C36].
- Vim `tiny` has `+multi_byte` but no `eval`, `syntax`, `viminfo` or `persistent_undo`, which shrinks both features and
  file side effects [C33].
- nvi's record database and recovery directory need a writable `/tmp`-like location and a sendmail-style mailer (or code
  changes), plus a BSD `dbopen`; nvi 1.81.6's bundled `db` or nvi2's `db1` are the two ways to supply it [C26][C27][C29].
- Editors that write by rename (toybox vi's `.swp`, Vim's `backupcopy=no`) require a directory-level write grant; editors
  that write in place (kilo, busybox vi, Vim's `backupcopy=yes`) work with a single file grant [C35][C37][C39][C40].
- `:e`, `:r`, `:w other`, `:!` need either a chooser at run time or must be refused; vi's own `secure`, Vim's `-Z`, less's
  `LESSSECURE` are existing switches that remove these paths [C28][C34][C46].
- Programs with no curses (busybox vi, toybox vi, kilo) work on any terminal that answers `ESC[6n` and understands basic
  VT100 sequences; toybox helpers open `/dev/tty` as a fallback, which would have to be patched [C14][C15][C47].
- `ed` (OpenBSD) is a small BSD-licensed fallback editor without any terminal interface [C43].

### For the userland

- toybox already has finished, 0BSD versions of `grep sed find xargs tar cpio patch cmp dd od stat du df date tee
  tail uniq cut printf test seq ps top kill` and `gzip/gunzip/zcat` with its own deflate; none of those is built
  today beyond 14 applets [U1, U2, U16, U21]. The cheapest large step is to build more of toybox, with the missing
  libc names closed (`getline`, `mkdtemp`, `link`, `lchown`, `mknod`, `dirname`, `dprintf`, `towlower`, ...) [U4, U10].
- toybox lacks, in finished form, `awk`, `vi`, `diff`, `tr`, `expr`, `bc`, `sh`, `more`, `man` and `xzcat` (all in
  `pending`), and has no `less`, `ed`, `bzip2` compressor, `unzip`, `xz` compressor or real `git` [U16, U19, U20].
  Those are the places where another source is needed or pending code must be reviewed.
- For `vi`: toybox's pending `vi` (1,871 lines, 0BSD) is small; nvi is BSD-3-Clause with a curses dependency [U19, U51].
  Full-screen use needs terminal capabilities the graphical terminal emulates only partly (brief).
- For `less`: only `less` itself is a real option (BSD-style "Less License" alternative to GPL, curses-like
  dependency), or toybox `more` (pending); toybox has no `less` [U15, U50].
- For `awk`: one-true-awk brings its own regex engine, UTF-8 and CSV, and needs `bison` at build time; its licence
  text is permissive but not one of the brief's named licences, so it is a decision for the architect [U34, U35, U36].
  toybox's pending `awk` is 0BSD but unreviewed [U19].
- `diff`/`patch`: toybox `patch` (unified only, finished) with toybox pending `diff` or OpenBSD's ISC `diff` fits;
  GNU diffutils is GPL-3 [U22, U19, U49].
- Archives: gzip is covered by toybox or zlib; xz decoding by xz-embedded (0BSD); `xz` full tool is 0BSD but the
  2024 backdoor was in release tarballs, so build from reviewed source [U21, U40, U41, U45]. bsdtar/libarchive is the
  other broad option, BSD-2 with libraries it probes for [U44].
- The shell's description (grants) per applet is a gating item before most of these tools are useful: none of the
  toybox applets has one (ADR-0153 lists it open) [U9].
- Users and groups: `id`, `ls -l`, `chown` and `tar` all go through `getpw*`/`getgr*` in toybox; Zoraya has none, so
  the answer must come from the libc layer, not from patching 18 applets [U10].

### For pseudo-terminals

- Distinguish the three roles. Only terminal emulators, multiplexers, remote-login servers and recorders create ptys; vi,
  less, git, curl and the ssh client consume the terminal they inherit [P1][P23][P33][P37]. Stage 10's list needs a way to
  *pass a terminal as a grant*, and needs a pty-like object only for `ttyd` (and later for tmux or `script`-like tools).
- The ssh client needs a terminal in raw mode with window-size notification, not a pty: `enter_raw_mode` on stdin plus
  SIGWINCH-driven window-change messages [P24][P23]. It also asks for a pty only when stdin is a tty [P23].
- Prompts that bypass redirection (`ssh` and `git` passwords, `getpass`, `less` keys) all open `/dev/tty` by name [P42][P33]
  [P37][P31]. Each has a fallback (stdin/stderr, fd 2, askpass), so a libposix `/dev/tty` that opens *the terminal grant of
  the process* and fails with ENXIO or ENOTTY otherwise is compatible with these programs; ssh and git fall back, less uses
  fd 2 [P42][P34][P37].
- A pty in Zoraya is two handles to one object. Fuchsia and Windows give the model: the service returns a fresh pair per
  request, nothing is looked up by name, and access follows from who was handed the handle [P44][P45][P58].
- The master handle carries real authority (read all slave output, inject input and signals, resize) [P1][P8][P20], so a
  grant of "run this program in a new terminal" should give the program the slave only; the terminal client keeps the master.
  Fuchsia splits the same way, with `SetWindowSize` allowed only on the server side [P44].
- Do not provide `TIOCSTI` (OpenBSD removed it, Linux made it root-only, the Flatpak CVE and its incomplete first fix show
  the cost of leaving it available) [P13][P14][P15][P16]. `TIOCGPTPEER`-style "slave from master descriptor" avoids the
  path lookup that caused namespace confusion [P7].
- The signal characters need an owner: Redox's `ptyd` sends SIGINT to a process group id kept in the pty object [P50], Plan 9
  needs a `-pid` in the window attach specifier for the same reason [P59]. Zoraya's signals at wait points mean the pty
  object needs the process (group) to notify, set by whoever makes the slave a controlling terminal [P6][P59].
- Plan 9's `rio` and `vt` show the cost of having no pty: native programs use `consctl` raw mode, and foreign Unix
  programs need an emulator (`vt`) plus manual newline settings [P59][P62]. Zoraya's goal of running unmodified POSIX
  programs (vi, less) points towards a termios-visible pty, not a Plan 9 style device.
- Genode's libc fakes `TIOCGETA` (ECHO only) and `TIOCGWINSZ` and ignores `TIOCSETA`, enough for bash with readline but
  not for raw mode in vi (inference from the code shown; no line discipline is visible there) [P55]. A real termios
  implementation with the line discipline inside the pty object, as in Redox's `ptyd` [P50], is the safer reading for vi
  and less.
- Licences of candidates to read next: Redox `ptyd` and `orbterm` are MIT [P52]; the Fuchsia `ptysvc` source has a BSD-style
  header [P46]; Genode is AGPLv3 [P56]; tmux is ISC-style, foot is MIT [P18][P40].

### For git and ssh

- The ssh client exits if there is no passwd entry for its uid [G36], while git falls back to the name "unknown" and
  Dropbear's client takes `$HOME` first [G28][G60]. A system without users either supplies a synthetic passwd entry
  to OpenSSH or prefers Dropbear's `dbclient` (inference).
- Neither tool's file footprint is named by the command line: `~/.ssh/config`, `known_hosts`, keys, and `~/.gitconfig`
  [G34][G22]. The grant for `ssh HOST` and `git clone URL` would be a bundle (a "room"): the config, `known_hosts`
  (writable), one key or an agent connection, the network destination [G34][G35].
- Config files can change the host that is contacted (`HostName`, `Port`, `ProxyJump`) and the programs that are run
  (`ProxyCommand`, `LocalCommand`, `Match exec`, `core.sshCommand`, hooks, editor, pager) [G35][G22]. Deriving the
  destination from the typed name is unsafe unless the config is read by the trusted side or aliasing is switched off
  with `-F` and `-o` (inference).
- Dropbear has no config file, a single `known_hosts`, `-J`, `-y`, and a documented multi-hop mode, and a 110 kB
  statically linked size, but it has no sftp client [G58][G54][G59]. It is MIT-style [G53].
- An agent-style process that holds keys and answers sign requests matches the capability model and already has
  per-key confirmation and destination constraints [G44][G45][G43]. The permission checks on key files rely on
  uid and mode [G37], which would need replacing by "this program was granted the file".
- Git's tolerance for missing pieces is wide: `NO_CURL`, `NO_EXPAT`, `NO_PERL`, `NO_PYTHON`, `NO_TCLTK`, `NO_GETTEXT`,
  `NO_MMAP`, `NO_PTHREADS`; builtins cover most commands [G13][G15]. What remains outside is `git-remote-http(s)`
  (a child process using libcurl) plus `ssh`, pager and editor, which need `fork`+`exec` or a spawn equivalent [G13][G14].
- Git needs exclusive create, atomic `rename` over an existing file, and directory-walking upwards from the cwd
  (until it stops at `.git`, a ceiling or a file-system boundary) [G24][G19]. A directory grant that stops at the
  grant's root would make the upward walk end at the granted directory (inference).
- Got shows the same split Zoraya might use: a parent holding the network descriptor, helper processes that get
  `stdio` plus one received descriptor and no file system, ISC licence, push over ssh only [G9][G8][G6][G5]. It is not
  git, so `git` scripts do not carry over [G5].
- gitoxide (MIT or Apache-2.0) has no push and uses the external `ssh` and curl for transport [G3][G4]; neither of the
  permissive Rust or C options removes the need for an `ssh` program.

### For files named while a program runs

- A grant fixed on the command line is the norm in the capability systems that shipped (WASI, CloudABI, Plash, Genode,
  Shill), and each fails the in-program `:e` with a plain `errno` [K37, K45, K1, K31, K43]. Zoraya's `vi file` rule
  matches this and inherits that failure; the design question is only what the person sees.
- The only text-mode run-time request that exists is Deno's prompt, and both of its CVEs came from drawing on the
  program's own terminal and from deciding on a different string than the one shown [K8, K9, K10]. Any Zoraya text
  prompt should be drawn by the compositor or `ttyd` outside the program's cell grid, with the decision computed from
  the same bytes that are shown, and with name display from a fixed-font, non-escape path [K9, K10, K46].
- A prompt must be answerable only by input the program cannot synthesise: the Windows secure attention sequence and
  the separate secure desktop are the precedent for a key and a surface the program never sees [K48]; bubblewrap's
  `TIOCSTI` issue shows that a terminal input queue is not such a channel unless the session is detached [K42].
- Casper's `fileargs` is the closest precedent for "the shell's operands are the grant" for non-interactive tools
  (`head`, `wc`, `tail`, `cat`): the set is closed at `cap_enter()`, which matches Zoraya's grants for filters and
  fails for `:e` [K11, K13, K14].
- For editors and pagers, OpenBSD (the most systematic user of pledge and unveil) leaves the set of files open and
  confines other powers (`vi -S`, `LESSSECURE`) [K15, K17, K20]; its own `got` unveils the repository and worktree
  directories read-write and runs the editor unconfined [K18, K20]. Zoraya's directory grant (`+w ~/proj git ...`)
  has the same shape as `got`'s `apply_unveil`.
- Write-by-rename is the case where a single-file grant is smaller than what editors need; only `unveil` documents it
  for non-directories, and Landlock and the portal show that parent-directory rights are separate rights [K20, K40,
  K23]. The design needs either a directory-level right that does not read other entries, or a statement that
  `vi file` also grants the creation of named siblings.
- The macOS and Android examples show directory grants (`ACTION_OPEN_DOCUMENT_TREE`, document-relative bookmarks,
  related items) as an explicit second kind of grant next to a file grant, chosen by a separate act [K26, K30].
- TCC shows what happens when a session-level app (the terminal) holds the grant: one prompt, then everything run
  there inherits it, and the prompt names the terminal, not the program that touched the file [K29, K28]. That is
  the outcome a "rooms" design wants to avoid in text mode.
- Refusal messages differ and mostly say nothing about *why*; Node's structured `permission` and `resource` and audit
  mode are the one pattern that tells the author what a grant lacked [K39].

## Pages that could not be fetched

### Not fetched: terminals

- https://invisible-island.net/xterm/ctlseqs/ctlseqs.html: HTTP 403; replaced by `ctlseqs.txt` at the same site (fetched
  as plain text, same document, patch #411).
- https://invisible-island.net/ncurses/man/curs_util.3x.html: HTTP 403; `resizeterm(3x)`, `curs_getch(3x)` and `ncurses(3x)`
  replaced it.
- https://git.suckless.org/st/raw/... paths: HTTP 404; the `.../plain/...` viewer pages (`FAQ`, `README`, `st.c`, `st.info`,
  `LICENSE`, `TODO`) were fetched instead. The viewer returns HTML with line numbers, which I stripped before counting.
- https://raw.githubusercontent.com/alacritty/alacritty/master/docs/escape_support.md: 404 (file moved); replaced by
  `extra/man/alacritty-escapes.7.scd`.
- https://gitlab.freedesktop.org/terminal-wg/esctest `README.md` raw: 404; the project page and the GitHub copy
  `ThomasDickey/esctest2` (README text shown on the page) were read instead.
- https://man7.org/linux/man-pages/man2const/TIOCGWINSZ.2const.html: 404; `man2/TIOCGWINSZ.2const.html` worked.
- https://raw.githubusercontent.com/bminor/bash/master/lib/readline/terminal.c: 404; replaced by the Savannah cgit copy of
  `readline/terminal.c`.
- foot's `tests/` directory could not be listed beyond the two names the Codeberg API returned; I did not read
  foot's source for in-file unit tests.
- Not verified: Python/CPython licence of `Lib/_pyrepl` (the LICENSE file was read only to find its heading); vim's
  licence details beyond its first clauses; alacritty's possible dual licence (only `LICENSE-APACHE` was opened).
- Not examined: how `ssh`, `git` and `tmux` themselves choose escape sequences beyond what tmux's man page lists; the
  esctest GitHub page's licence line.

### Not fetched: curses libraries, editors and pagers

- `https://invisible-island.net/ncurses/man/ncurses.3x.html`: returned an 79-byte stub (no content); the signal and
  input facts were taken from `curs_initscr(3x)`, `resizeterm(3x)`, `curs_inopts(3x)` and `curs_getch(3x)` instead.
  The sentence in "How terminal descriptions are found" that `setupterm` returns -1 when no database is found comes from
  `curs_terminfo(3x)`, which I fetched (Section line 203), but I did not list it as a separate source: [C5a] = that page,
  `https://invisible-island.net/ncurses/man/curs_terminfo.3x.html`, primary.
- `https://raw.githubusercontent.com/sabotage-linux/netbsd-curses/master/LICENSE`: 404; licence taken from its README.
- `https://invisible-island.net/ncurses/ncurses-license.html`: not fetched; `COPYING` from the GitHub mirror used instead.
- `https://man.netbsd.org/curses.3`: fetched but gave no usable facts about terminfo or wide chars (only a cross reference
  to `curses_termcap(3)`); not cited.
- `https://vimhelp.org/...`: not used; the same documents were read from the Vim repository (`runtime/doc/*.txt`).
- `https://www.greenwoodsoftware.com/less/` and its FAQ: not fetched; the GitHub repository and man7.org page were used.
- `https://man.openbsd.org/dbopen.3`: fetched, but the text I extracted was a platform list only; not cited.
- nvi 1.79 original source and NetBSD's vi: not fetched.
- htop's use of ncurses, GNU `nano` licence, Python's `readline` module requirements beyond the docs: not verified.
- A statement that nvi cannot run on termcap alone: no source found.

### Not fetched: the userland tool set

- https://pubs.opengroup.org/onlinepubs/9799919799/utilities/tar.html: 404 (POSIX.1-2024 has no `tar` page); used as
  evidence that `tar` is not in the standard.
- https://github.com/onetrue-awk/awk: 404 (the task's URL); the repository is `onetrueawk/awk`, fetched instead.
- https://git.savannah.gnu.org/cgit/gawk.git/plain/COPYING: returned an HTML wrapper; gawk's licence is `[unverified]`.
- https://git.busybox.net/busybox/plain/LICENSE: empty; the GitHub mirror `mirror/busybox` raw LICENSE was used.
- https://github.com/ibara/oksh/raw/master/LICENSE: 404; `LEGAL` in the same repo was read.
- https://core.suckless.org/sbase/ content was only a short stub; the README at git.suckless.org was used.
- Heirloom Toolchest licence text: not found on the pages fetched; `[unverified]`.
- lobase repositories: only a GitHub search result seen (`weak`), not fetched.
- https://tukaani.org/xz/embedded.html fetched; no failures.

### Not fetched: pseudo-terminals

- https://fuchsia.googlesource.com/fuchsia/+/refs/heads/main/src/starnix/kernel/fs/devpts.rs : empty (path moved or wrong);
  Starnix's devpts implementation is not covered. Fuchsia's console and sshd pty acquisition also not covered: only the
  `toolbox` route in the bootstrap shard was read.
- https://gitlab.redox-os.org/redox-os/ptyd/-/raw/master/src/main.rs : Cloudflare challenge; replaced by raw.githubusercontent.com
  mirror (`redox-os/ptyd`).
- https://sourceware.org/git/?p=glibc.git;a=blob_plain;f=login/programs/pt_chown.c : bot wall; replaced by the bminor/glibc
  GitHub mirror.
- https://git.kernel.org/... commit and Kconfig pages: bot wall; the Kconfig came from raw.githubusercontent.com/torvalds/linux,
  the commit messages from the GitHub commit-search API.
- https://www.openwall.com/lists/oss-security/2016/02/21/2 : the message page returned "No such message"; no oss-security
  source is used.
- https://www.kernel.org/doc/html/latest/admin-guide/sysctl/dev.html : 404; `dev.tty.legacy_tiocsti` documented only via the
  man page and Kconfig.
- https://man7.org/linux/man-pages/man1/gpg.1.html, bubblewrap.xml, genode libc vfs_terminal.cc, fuchsia pty.fidl on a
  GitHub mirror, ssh `-t` text via man7: 404 or not parsed; replaced as listed in the sources.
- QNX `devc-pty` documentation: not searched.
- Not verified: sudo's use of `/dev/tty`; GNU screen's licence; expect's licence; Python's licence; whether glibc still ships
  `pt_chown`.

### Not fetched: git and ssh

- https://github.com/git/git/security/advisories/GHSA-vw2c-22j4-2fh2: returned an empty page; replaced by the release
  notes in the git tree [G23].
- https://man.openbsd.org/got.1 (and other `man.openbsd.org` pages): returned the search form; replaced by the manual
  source `got/got.1` in got-portable and the helper source files [G8][G9][G10].
- Plan 9 ssh, gitignore(5), gitattributes(5), githooks(5) text beyond the table of contents, `git-remote-https` page
  (a 541-byte stub): not read or not used; claims about them are omitted.
- gix's fetch/clone status for the CLI could not be confirmed beyond the crate status list [G4].

### Not fetched: capability systems and interactive tools

- http://plash.beasts.org/ and https://plash.beasts.org/: the TLS certificate names another host, and plain HTTP gave
  404; replaced by web.archive.org copies of the wiki dated 2012-12-21 (`/web/20121221113548/http://plash.beasts.org/wiki/...`).
- https://wiki.freebsd.org/Capsicum: an Anubis bot wall; the list of converted programs was not read.
- https://developer.apple.com/library/archive/.../AppSandboxInDepth.html and the Apple Support page on Files and
  Folders: JavaScript-only or no prompt detail; replaced by Apple's `.md` renderings of the documentation pages.
- Ka-Ping Yee's "User Interaction Design for Secure Systems" (people.ischool.berkeley.edu/~ping/sid/): redirects to a
  not-found page; web.archive.org copy refused by the fetch tool. The principle is cited only through [K3] and
  desktop/survey.md.
- https://www.gnu.org/software/hurd/hurd/ng/powerbox.html: HTTP 429.
- Genode `vfs` README (raw GitHub): 404. Fuchsia directory-rights pages were not tried.
- Bitfrost (OLPC), HD Moore's terminal-emulator paper, and CVE lists for title-report injection were not fetched.
- The Capsicum and Shill PDFs were not readable by the fetch tool; I extracted their text myself with a small
  decoder, so wording is approximate and no page numbers are given.

## Sources

Tags carry the letter of their section. Every entry was fetched on 2026-11-13 unless it says otherwise.

### Sources: terminals (T)

- [T1] ncurses project (T. Dickey), `misc/terminfo.src` from ThomasDickey/ncurses-snapshots,
  https://raw.githubusercontent.com/ThomasDickey/ncurses-snapshots/master/misc/terminfo.src, primary, fetched 2026-11-13.
  Read: entries `dumb`, `ansi`, `vt100`, `vt220`, `linux`, `linux-alt1049`, `xterm`, `xterm-256color`, `xterm+256color2`,
  `xterm+direct`, `st`, `foot`/`foot+base`, `alacritty`, `kitty`, `screen`, `tmux`; header date 2026/11/07; counts by my
  parser.
- [T2] Thomas E. Dickey / XTerm project, *XTerm Control Sequences* (patch #411, 2026/08/23),
  https://invisible-island.net/xterm/ctlseqs/ctlseqs.txt, primary, fetched 2026-11-13. Read: modes list, SGR 38/48 notes,
  line and word counts, absence of 2026/2027.
- [T3] Ecma International, *ECMA-48, 5th edition, June 1991*,
  https://ecma-international.org/wp-content/uploads/ECMA-48_5th_edition_june_1991.pdf and the landing page
  https://ecma-international.org/publications-and-standards/standards/ecma-48/, primary, fetched 2026-11-13. Read:
  clause 5.4.1 (private parameter strings), headings 8.3.1 to 8.3.162, page count.
- [T4] Linux man-pages / ncurses, `terminfo(5)`, https://man7.org/linux/man-pages/man5/terminfo.5.html, primary, fetched
  2026-11-13. Read: capability tables, user-defined capabilities, `smcup`/`rmcup`, `gn`; no minimal-entry statement found.
- [T5] Vim project, `runtime/doc/term.txt`, https://raw.githubusercontent.com/vim/vim/master/runtime/doc/term.txt, primary,
  fetched 2026-11-13 (header: Vim 9.2, last change 2026 Nov 02); and `LICENSE`,
  https://raw.githubusercontent.com/vim/vim/master/LICENSE (the Vim license, opened for its first clauses only).
  Read: t_xx list, builtin termcaps, xterm-terminfo-entries, kitty note, t_BS/t_ES.
- [T6] Mark Nudelman, `less` `screen.c` and `LICENSE`,
  https://raw.githubusercontent.com/gwsw/less/master/screen.c, `.../LICENSE`, primary, fetched 2026-11-13. Read:
  `get_term_info`, `tmodes`, fallbacks for mouse and bracketed paste, 256-colour output, licence text.
- [T7] nvi2 project, `cl/cl_term.c`, `cl/cl_screen.c`, `README`, `LICENSE`,
  https://raw.githubusercontent.com/lichray/nvi2/master/cl/cl_screen.c (and siblings), primary, fetched 2026-11-13. Read:
  curses interface, `tigetstr`/`tigetnum`, `LINES`/`COLUMNS` comment, BSD licence.
- [T8] htop-dev, `README.md` and `configure.ac`, https://raw.githubusercontent.com/htop-dev/htop/main/README.md, primary,
  fetched 2026-11-13. Read: ncurses requirement, GPL v2+ badge, curses package search.
- [T9] CPython, `Lib/_pyrepl/unix_console.py` and `terminfo.py` (main) and `unix_console.py` (3.13 branch),
  https://raw.githubusercontent.com/python/cpython/main/Lib/_pyrepl/unix_console.py, primary, fetched 2026-11-13. Read:
  required and optional capability list, SIGWINCH, `TIOCGWINSZ`, 2004 and `?7` writes; CPython `LICENSE` heading only.
- [T10] GNU Readline, `terminal.c`, https://git.savannah.gnu.org/cgit/readline.git/plain/terminal.c, and the Readline user
  manual, https://tiswww.case.edu/php/chet/readline/rluserman.html, primary, fetched 2026-11-13. Read: `tc_strings` table,
  non-ANSI bracketed-paste rule, `enable-keypad`; `COPYING` is GPL v3.
- [T11] tmux project, `tmux.1`, https://raw.githubusercontent.com/tmux/tmux/master/tmux.1, and `COPYING`, primary, fetched
  2026-11-13. Read: `terminal-features`, `terminal-overrides`, `default-terminal`, "TERMINFO EXTENSIONS".
- [T12] tmux project, wiki *FAQ*, https://raw.githubusercontent.com/wiki/tmux/tmux/FAQ.md, primary, fetched 2026-11-13.
  Read: TERM inside and outside tmux, why `screen`.
- [T13] foot project (D. Eklöf), `doc/foot-ctlseqs.7.scd`,
  https://codeberg.org/dnkl/foot/raw/branch/master/doc/foot-ctlseqs.7.scd, primary, fetched 2026-11-13. Read: ESC, SGR,
  colour forms, private-mode table with origins, DA replies, no C1.
- [T14] foot project, `README.md`, `INSTALL.md` (Codeberg raw, master), primary, fetched 2026-11-13; plus the Codeberg API
  listing of the `tests/` directory. Read: features, MIT licence, terminfo note, TERMINFO issue, identification via DA,
  XTGETTCAP.
- [T15] kitty project (K. Goyal), *FAQ*, https://sw.kovidgoyal.net/kitty/faq/ and *Terminal protocol extensions*,
  https://sw.kovidgoyal.net/kitty/protocol-extensions/, primary, fetched 2026-11-13. Read: terminfo-missing causes and
  `kitten ssh` fix; list of extensions.
- [T16] kitty project, *Text sizing protocol*, https://sw.kovidgoyal.net/kitty/text-sizing-protocol/, primary, fetched
  2026-11-13. Read: "Fixing the character width issue for the terminal ecosystem".
- [T17] kitty project, *Comprehensive keyboard handling in terminals*, https://sw.kovidgoyal.net/kitty/keyboard-protocol/,
  primary, fetched 2026-11-13. Read: progressive enhancement flags.
- [T18] kitty project, `terminfo/kitty.terminfo`,
  https://raw.githubusercontent.com/kovidgoyal/kitty/master/terminfo/kitty.terminfo, and `LICENSE` (GPL v3), primary, fetched
  2026-11-13. Read: entry `xterm-kitty`, extended names.
- [T19] Alacritty project, `extra/man/alacritty-escapes.7.scd`,
  https://raw.githubusercontent.com/alacritty/alacritty/master/extra/man/alacritty-escapes.7.scd, primary, fetched
  2026-11-13. Read: status per sequence.
- [T20] Alacritty project, `INSTALL.md`, `extra/alacritty.info`, `LICENSE-APACHE`, `README.md` (raw master), primary, fetched
  2026-11-13. Read: terminfo requirement, entry text, licence.
- [T21] Alacritty project, `alacritty_terminal/tests/ref.rs` and the GitHub API listing of `tests/ref`,
  https://raw.githubusercontent.com/alacritty/alacritty/master/alacritty_terminal/tests/ref.rs, primary, fetched 2026-11-13.
  Read: reference-test mechanism and case names.
- [T22] suckless.org, *st - simple terminal*, https://st.suckless.org/, and repository `LICENSE`
  (https://git.suckless.org/st/plain/LICENSE, MIT/X Consortium), primary, fetched 2026-11-13. Read: motivation, status list,
  version, licence.
- [T23] suckless.org, *st goals*, https://st.suckless.org/goals/, primary, fetched 2026-11-13. Read: goals and non-goals.
- [T24] suckless.org st repository, `FAQ` and `README`, https://git.suckless.org/st/plain/FAQ, primary, fetched 2026-11-13.
  Read: terminfo advice, `smkx` and keypad issue.
- [T25] suckless.org st repository, `st.c`, https://git.suckless.org/st/plain/st.c, primary, fetched 2026-11-13. Read: DECSET
  cases, CSI/OSC cases, `wcwidth`, `TIOCSWINSZ`, size.
- [T26] suckless.org st repository, `st.info`, https://git.suckless.org/st/plain/st.info, primary, fetched 2026-11-13. Read:
  entry names.
- [T27] Linux man-pages, `console_codes(4)`, https://man7.org/linux/man-pages/man4/console_codes.4.html, primary, fetched
  2026-11-13. Read: scope of the console's sequences, mouse, SGR 38/48, guidance to use terminfo.
- [T28] Linux man-pages, `TIOCGWINSZ(2const)`, https://man7.org/linux/man-pages/man2/TIOCGWINSZ.2const.html, primary,
  fetched 2026-11-13. Read: `winsize`, SIGWINCH.
- [T29] The Open Group, POSIX.1-2024, `tcgetwinsize`,
  https://pubs.opengroup.org/onlinepubs/9799919799/functions/tcgetwinsize.html, primary, fetched 2026-11-13. Read: semantics,
  default size, rationale, application usage.
- [T30] The Open Group, POSIX.1-2024, `<signal.h>`, https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/signal.h.html,
  primary, fetched 2026-11-13. Read: SIGWINCH entry, default action I, defect 1151.
- [T31] ncurses project, `resizeterm(3X)`, https://invisible-island.net/ncurses/man/resizeterm.3x.html, `curs_getch(3X)`,
  `curs_initscr(3X)`, primary, fetched 2026-11-13. Read: SIGWINCH handling, `KEY_RESIZE`, `LINES`/`COLUMNS` override,
  generic entries.
- [T32] ncurses project, `ncurses(3X)`, https://invisible-island.net/ncurses/man/ncurses.3x.html, primary, fetched 2026-11-13.
  Read: environment variables `LINES`, `COLUMNS`, `TERMINFO`.
- [T33] The Open Group, POSIX.1-2024, `wcwidth`, https://pubs.opengroup.org/onlinepubs/9799919799/functions/wcwidth.html,
  primary, fetched 2026-11-13. Read: description and return values.
- [T34] Unicode Consortium, UAX #11 *East Asian Width*, https://www.unicode.org/reports/tr11/, primary, fetched 2026-11-13.
  Read: six property values, ambiguous characters, terminal-emulator note, revision 46.
- [T35] Markus Kuhn, `wcwidth.c`, https://www.cl.cam.ac.uk/~mgk25/ucs/wcwidth.c, primary, fetched 2026-11-13. Read: rules and
  comments (version 2007-05-26, Unicode 5.0), 309 lines.
- [T36] Contour terminal project, `docs/vt-extensions/unicode-core.md` and `synchronized-output.md`,
  https://raw.githubusercontent.com/contour-terminal/contour/master/docs/vt-extensions/unicode-core.md, primary (for the
  extension it defines), fetched 2026-11-13. Read: scope of mode 2027 and DECRQM feature detection for 2026. The linked
  specification repository `terminal-unicode-core` returned 404 at the path I tried and was not read.
- [T37] Thomas E. Dickey, *VTTEST - VT100/VT220/XTerm test utility*, https://invisible-island.net/vttest/, primary, fetched
  2026-11-13. Read: synopsis, history, licensing, test-tools.
- [T38] George Nachman and Thomas Dickey, *esctest*, https://gitlab.freedesktop.org/terminal-wg/esctest and
  https://github.com/ThomasDickey/esctest2, primary, fetched 2026-11-13. Read: purpose, flags, licence line on GitLab.
- [T39] ncurses project, `test/README`, https://raw.githubusercontent.com/ThomasDickey/ncurses-snapshots/master/test/README,
  and *ncurses test/sample programs*, https://invisible-island.net/ncurses/ncurses-examples.html, primary, fetched
  2026-11-13. Read: program list and statement that they are not a complete test set.
- [T41] ncurses project, `COPYING`, https://raw.githubusercontent.com/ThomasDickey/ncurses-snapshots/master/COPYING,
  primary, fetched 2026-11-13. Read: permission notice (MIT/X11 style).

### Sources: curses libraries, editors and pagers (C)

- [C1] Thomas E. Dickey et al., ncurses `INSTALL`, https://raw.githubusercontent.com/mirror/ncurses/master/INSTALL, primary, fetched 2026-11-13. Read: configure options (`--disable-database`, `--with-fallbacks`, `--with-termlib`, `--enable-widec`, `--without-cxx`, `--with-hashed-db`, default terminfo directories) and the "CONFIGURING FALLBACK ENTRIES" section.
- [C2] ncurses `COPYING`, https://raw.githubusercontent.com/mirror/ncurses/master/COPYING, primary, fetched 2026-11-13. Read: whole file; MIT-style permission notice, holders Dickey and FSF.
- [C3] GNU FTP, ncurses 6.6 tarball header, https://ftp.gnu.org/gnu/ncurses/ncurses-6.6.tar.gz, primary, fetched 2026-11-13 (HTTP HEAD). Read: Content-Length 3,791,150 bytes.
- [C4] ncurses project, `terminfo(5)`, https://invisible-island.net/ncurses/man/terminfo.5.html, primary, fetched 2026-11-13. Read: "Fetching Compiled Descriptions": `TERMINFO`, `~/.terminfo`, `TERMINFO_DIRS`, `hex:`/`b64:`.
- [C5] ncurses project, `curs_initscr(3x)`, https://invisible-island.net/ncurses/man/curs_initscr.3x.html, primary, fetched 2026-11-13. Read: NOTES on signal handlers for `SIGINT`, `SIGTERM`, `SIGTSTP`, `SIGWINCH`.
- [C5a] ncurses project, `curs_terminfo(3x)`, https://invisible-island.net/ncurses/man/curs_terminfo.3x.html, primary, fetched 2026-11-13. Read: `setupterm` returns -1 if no database found.
- [C6] ncurses project, `resizeterm(3x)`, https://invisible-island.net/ncurses/man/resizeterm.3x.html, primary, fetched 2026-11-13. Read: `SIGWINCH` handling and application-supplied handlers.
- [C7] ncurses project, `curs_inopts(3x)`, https://invisible-island.net/ncurses/man/curs_inopts.3x.html, primary, fetched 2026-11-13. Read: `typeahead` use of `poll`/`select`; `termios(3)` references.
- [C8] NetBSD, `terminfo(5)`, https://man.netbsd.org/terminfo.5, primary, fetched 2026-11-13. Read: "Fetching Compiled Descriptions" (cdb, embedded database list, search order).
- [C9] sabotage-linux, netbsd-curses `README.md`, https://raw.githubusercontent.com/sabotage-linux/netbsd-curses/master/README.md, primary, fetched 2026-11-13. Read: whole file; size table, differences, licence, compiled-in database, build.
- [C10] William McBrine, PDCurses, https://pdcurses.org/, primary, fetched 2026-11-13. Read: description, targets, public domain statement.
- [C11] Bill Gray, PDCursesMod `README.md`, https://raw.githubusercontent.com/Bill-Gray/PDCursesMod/master/README.md, primary, fetched 2026-11-13. Read: licence note and flavours.
- [C12] termbox2 `README.md`, https://raw.githubusercontent.com/termbox/termbox2/master/README.md, primary, fetched 2026-11-13. Read: no dependencies beyond libc, built-in terminal support, single-file header.
- [C13] termbox2 `termbox2.h` and `LICENSE`, https://raw.githubusercontent.com/termbox/termbox2/master/termbox2.h, primary, fetched 2026-11-13. Read: `/dev/tty` open, `SIGWINCH`, terminfo path search, `builtin_terms`; `wc -l` gave 4212; licence text opens with MIT permission notice.
- [C14] Rob Landley, toybox design document, https://landley.net/toybox/design.html, primary, fetched 2026-11-13. Read: statements on curses and ANSI escape sequences.
- [C15] toybox `lib/tty.c`, https://raw.githubusercontent.com/landley/toybox/master/lib/tty.c, primary, fetched 2026-11-13. Read: `tty_fd` (`/dev/tty` fallback), `terminal_size`, ANSI size probe; `wc -l` gave 296.
- [C16] tmux `README`, https://raw.githubusercontent.com/tmux/tmux/master/README, primary, fetched 2026-11-13. Read: tmux depends on libevent 2.x and ncurses.
- [C18] Python Software Foundation, `curses` module docs, https://docs.python.org/3/library/curses.html, primary, fetched 2026-11-13. Read: matches ncurses API; availability list; optional module.
- [C19] Python, What's New in Python 3.13, https://docs.python.org/3/whatsnew/3.13.html, primary, fetched 2026-11-13. Read: new interactive interpreter based on PyPy code; `PYTHON_BASIC_REPL`.
- [C20] CPython 3.13 `Lib/_pyrepl/` (`curses.py`, `_minimal_curses.py`, `unix_console.py`, `main.py`), https://raw.githubusercontent.com/python/cpython/3.13/Lib/_pyrepl/curses.py (and sibling files), primary, fetched 2026-11-13. Read: curses import fallbacks, `setupterm`/`tigetstr`/`tparm`, `SIGWINCH`, pyrepl fallback to basic REPL.
- [C21] CPython 3.14 `Lib/_pyrepl/terminfo.py` and `unix_console.py`, https://raw.githubusercontent.com/python/cpython/3.14/Lib/_pyrepl/terminfo.py, primary, fetched 2026-11-13. Read: pure-Python terminfo reader, search directories, no `curses.py` in the 3.14 directory listing (GitHub API).
- [C22] Python, `readline` module docs, https://docs.python.org/3/library/readline.html, primary, fetched 2026-11-13 (page shows version 3.15.2). Read: availability and the note that libedit may be the underlying library.
- [C23] CPython 3.13 `Lib/site.py`, https://raw.githubusercontent.com/python/cpython/3.13/Lib/site.py, primary, fetched 2026-11-13. Read: `gethistoryfile()` and automatic readline configuration.
- [C24] Jess Thrysoee, Editline Library (libedit), https://www.thrysoee.dk/editline/, secondary (a port's page), fetched 2026-11-13. Read: description, Berkeley-style licence, linking with libcurses.
- [C25] Chet Ramey, GNU Readline page, https://tiswww.case.edu/php/chet/readline/rltop.html, primary, fetched 2026-11-13. Read: licence statement, GPL version 3.
- [C26] nvi2 (lichray), `README`, `LICENSE`, `INSTALL.md`, `CMakeLists.txt`, https://raw.githubusercontent.com/lichray/nvi2/master/README, primary, fetched 2026-11-13; plus GitHub API repository metadata. Read: version 2.2.2 (2025-10-08), history, BSD licence, DB1 and curses/terminfo/iconv prerequisites, recovery directory.
- [C27] Debian sources, nvi 1.81.6-24 `README`, https://sources.debian.org/data/main/n/nvi/1.81.6-24/README, secondary (a distribution copy of upstream's file), fetched 2026-11-13. Read: version 1.81.6 (2007-11-18), bundled "stripped-down, replacement db(3) library".
- [C28] OpenBSD, `vi(1)`, https://man.openbsd.org/vi.1, primary, fetched 2026-11-13. Read: `recdir`, `secure`, `lock`, startup files, FILES, `SIGALRM`.
- [C29] OpenBSD `usr.bin/vi/common/exf.c` and `recover.c`, https://raw.githubusercontent.com/openbsd/src/master/usr.bin/vi/common/exf.c, primary, fetched 2026-11-13. Read: `dbopen(DB_RECNO)`, `/tmp/vi.XXXXXXXXXX`, `flock`, sendmail.
- [C30] Vim, `uganda.txt` and `LICENSE`, https://raw.githubusercontent.com/vim/vim/master/runtime/doc/uganda.txt, primary, fetched 2026-11-13. Read: charityware summary and licence conditions.
- [C31] Vim `src/configure.ac`, https://raw.githubusercontent.com/vim/vim/master/src/configure.ac, primary, fetched 2026-11-13. Read: `--with-features`, `--with-tlib`, terminal library search list.
- [C32] Vim `runtime/doc/term.txt`, https://raw.githubusercontent.com/vim/vim/master/runtime/doc/term.txt, primary, fetched 2026-11-13. Read: termcap/terminfo choice, builtin terminals, `ttybuiltin`.
- [C33] Vim `runtime/doc/various.txt`, https://raw.githubusercontent.com/vim/vim/master/runtime/doc/various.txt, primary, fetched 2026-11-13. Read: feature list T/N/H column; `:!`.
- [C34] Vim `runtime/doc/starting.txt`, https://raw.githubusercontent.com/vim/vim/master/runtime/doc/starting.txt, primary, fetched 2026-11-13. Read: vimrc locations, viminfo file name, `-i`, `-Z`.
- [C35] Vim `runtime/doc/options.txt`, https://raw.githubusercontent.com/vim/vim/master/runtime/doc/options.txt, primary, fetched 2026-11-13. Read: `backup`, `backupcopy`, `backupdir`, `writebackup`, `directory`, `undofile`, `undodir`, `exrc`, `shelltemp`.
- [C36] Vim `runtime/doc/recover.txt`, https://raw.githubusercontent.com/vim/vim/master/runtime/doc/recover.txt, primary, fetched 2026-11-13. Read: swap file name and lifetime.
- [C37] BusyBox `editors/vi.c`, https://raw.githubusercontent.com/mirror/busybox/master/editors/vi.c, primary, fetched 2026-11-13. Read: header, Kconfig text, `file_write`, `system()`, `.exrc`/`EXINIT`, ANSI size query, signals; `wc -l` gave 5053.
- [C38] BusyBox `LICENSE`, https://raw.githubusercontent.com/mirror/busybox/master/LICENSE, primary, fetched 2026-11-13. Read: GPL version 2 only note.
- [C39] toybox `toys/pending/vi.c`, https://raw.githubusercontent.com/landley/toybox/master/toys/pending/vi.c, primary, fetched 2026-11-13. Read: header, `default n`, command list, `.swp` plus `rename` save; `wc -l` gave 1871.
- [C40] antirez, kilo `kilo.c` and `LICENSE`, https://raw.githubusercontent.com/antirez/kilo/master/kilo.c, primary, fetched 2026-11-13. Read: header, save routine, escapes, `SIGWINCH`; `wc -l` gave 1308; BSD 2-clause.
- [C41] OpenBSD, `mg(1)`, https://man.openbsd.org/mg.1, primary, fetched 2026-11-13. Read: description, backup files, `~/.mg`, `~/.mg.d`.
- [C42] hboetes/mg portable repository (`README`, `GNUmakefile`, `ttyio.c`, `tty.c`, `fileio.c`, `pathnames.h`), https://github.com/hboetes/mg, primary, fetched 2026-11-13 via raw.githubusercontent.com. Read: public domain statement, libbsd/ncurses dependencies, `setupterm`, backup via copy and `mkstemp`.
- [C43] OpenBSD `bin/ed/main.c` and `ed(1)`, https://raw.githubusercontent.com/openbsd/src/master/bin/ed/main.c, primary, fetched 2026-11-13. Read: licence block, `ed.hup` on `SIGHUP`, `system`, `popen`.
- [C44] Mark Nudelman, less `LICENSE`, `README`, `COPYING` head, https://raw.githubusercontent.com/gwsw/less/master/LICENSE, primary, fetched 2026-11-13. Read: Less License text; README's dual licensing; COPYING is GPL v3.
- [C45] less `screen.c`, https://raw.githubusercontent.com/gwsw/less/master/screen.c, primary, fetched 2026-11-13. Read: terminfo/termcap selection, capability names, env overrides, hardcopy fallback.
- [C46] less(1), https://man7.org/linux/man-pages/man1/less.1.html, primary, fetched 2026-11-13. Read: commands (`:e`, `:n`, `!`, `|`, `s`, `v`), `LESSOPEN`, `LESSHISTFILE`, lesskey locations, `LESSSECURE`.
- [C47] toybox `toys/pending/more.c` and `LICENSE`, https://raw.githubusercontent.com/landley/toybox/master/toys/pending/more.c, primary, fetched 2026-11-13. Read: `default n`, `/dev/tty`, `tcsetattr`; toybox `LICENSE` is the 0BSD-style permission text; directory listings via the GitHub API showed no `ed.c` or `less.c`.
- [C48] BusyBox `libbb/lineedit.c`, https://raw.githubusercontent.com/mirror/busybox/master/libbb/lineedit.c, primary, fetched 2026-11-13. Read: header licence note, HISTFILESIZE handling; `wc -l` gave 3080.

### Sources: the userland tool set (U)

- [U1] Zoraya repository, `tools/posix-surface/results/applets-by-tier.tsv`, local, primary, read 2026-11-13. Read: all six rows; supports the per-tier applet counts.
- [U2] Zoraya repository, `ports/Makefile` (lines 317-345, 348, 441-443, 655-700) and `ports/sources.lock`, local, primary, read 2026-11-13. Read: toybox and busybox applet lists, the T2 probe target and the toybox version/hash.
- [U3] Zoraya repository, `docs/dependencies.md`, local, primary, read 2026-11-13. Read: ported-programs table (toybox 0.8.15 0BSD, busybox 1.38.0 GPL-2.0-only, zlib 1.3.1).
- [U4] Zoraya repository, `docs/design.md` (scope of POSIX compatibility, ports, W4 and `libc.a` names, limitations), local, primary, read 2026-11-13. Read: tier summary, the 20+9 missing names, the 337-name `libc.a`.
- [U5] Zoraya repository, `docs/backlog.md` (T2 row of 2026-11-04), local, primary, read 2026-11-13. Read: the architect's order to T2 and the names list.
- [U6] Zoraya repository, `docs/research/TASK-02-posix-surface-measurement.md`, local, primary, read 2026-11-13. Read: the target-software tier table and workloads W1-W6.
- [U7] Zoraya repository, `docs/adr/0005-posix-scope.md`, local, primary, read 2026-11-13. Read: measured tiers, symbol counts, T1/T2 decision.
- [U8] Zoraya repository, `docs/adr/0310-surface-for-tls-curl-and-wget.md`, `docs/adr/0144-surface-for-sort-and-wc.md` and `docs/tour.md`, local, primary, read 2026-11-13. Read: the names added for curl/wget (`regcomp` family, `fnmatch`), why `sort`/`wc` were added, `wc` in the tour.
- [U9] Zoraya repository, `docs/adr/0153-what-a-program-gets.md` (open questions) and `ls user/describe/{commands,builtins,posix}`, local, primary, read 2026-11-13. Read: open item on toybox descriptions; list of description files.
- [U10] landley/toybox, release tarball 0.8.15, https://landley.net/toybox/downloads/toybox-0.8.15.tar.gz, primary, fetched 2026-11-13. Read: sha256 equals `ports/sources.lock`; `grep -rlF` of libc names over `toys/` and `lib/`; `lib/dirtree.c` line count; also `docs/adr/posix-surface-T1.txt` (local) for the T1 name rows.
- [U11] Zoraya repository, `docs/research/stage-6/inputs.md` (I-15, I-39), local, primary, read 2026-11-13. Read: open questions on `xfork` applets and the accepted applet set.
- [U12] Rob Landley, *Toybox design*, https://landley.net/toybox/design.html, primary, fetched 2026-11-13. Read: licence section (0BSD), portability section (libc only, tested with musl).
- [U13] landley/toybox, `LICENSE`, https://raw.githubusercontent.com/landley/toybox/master/LICENSE, primary, fetched 2026-11-13. Read: 0BSD text and copyright line.
- [U14] Rob Landley, *Toybox FAQ*, https://landley.net/toybox/faq.html, primary, fetched 2026-11-13. Read: the "Why is there toybox?" answer on the licence argument and Android.
- [U15] Rob Landley, *Toybox status*, https://landley.net/toybox/status.html, primary, fetched 2026-11-13. Read: current release line, completed/pending/not-started lists and the remaining-POSIX to-do list.
- [U16] GitHub API, `https://api.github.com/repos/landley/toybox/contents/toys/{pending,posix,other,lsb}` with and without `?ref=0.8.15`, primary, fetched 2026-11-13. Read: file names per directory; identical at the tag and `master`.
- [U17] landley/toybox `toys/other`, `toys/posix/ps.c`, `toys/other/readlink.c`, `toys/posix/cp.c`, `toys/posix/ls.c`, primary, fetched 2026-11-13. Read: `NEWTOY(` lines to see which commands one file defines.
- [U18] landley/toybox, `toys/pending/README`, https://raw.githubusercontent.com/landley/toybox/master/toys/pending/README, primary, fetched 2026-11-13. Read: meaning of pending, defaults to "n".
- [U19] landley/toybox `toys/pending/{awk,vi,git,xzcat,sh,diff,tr,expr,bc,man,more}.c` (master), primary, fetched 2026-11-13. Read: headers, `NEWTOY` lines, config help text, `wc -l` counts.
- [U20] Rob Landley, *Toybox roadmap*, https://landley.net/toybox/roadmap.html, primary, fetched 2026-11-13. Read: POSIX-2024 section, LSB list, "rebuild itself" busybox dependency list, Android toolbox section.
- [U21] landley/toybox `toys/lsb/gzip.c` and `lib/deflate.c`, (tag 0.8.15 and master), primary, fetched 2026-11-13. Read: `NEWTOY` lines for gzip/gunzip/zcat, the zlib-or-builtin switch, inflate/deflate in `lib/deflate.c`.
- [U22] landley/toybox `toys/posix/{tar,patch,find,sed,grep}.c`, primary, fetched 2026-11-13. Read: tar option string and compression children; patch's "only does -u" header.
- [U23] BusyBox project, *BusyBox command list*, https://busybox.net/downloads/BusyBox.html, primary, fetched 2026-11-13. Read: the "Currently available applets" list.
- [U24] BusyBox project, *License*, https://busybox.net/license.html, and `LICENSE` in `mirror/busybox` on GitHub (https://raw.githubusercontent.com/mirror/busybox/master/LICENSE), primary / secondary (mirror), fetched 2026-11-13. Read: GPL version 2 statement, "only version" note.
- [U25] Chimera Linux, *About*, https://chimera-linux.org/about/, primary, fetched 2026-11-13. Read: "Alternative userland" section.
- [U26] chimera-linux/chimerautils, `README.md` and `src.freebsd` directory listings via GitHub API, primary, fetched 2026-11-13. Read: replaced projects, build requirements, tool directories, compat sources.
- [U27] chimera-linux/chimerautils, `LICENSE`, primary, fetched 2026-11-13. Read: "mostly BSD-2-Clause".
- [U28] suckless.org, sbase `README`, https://git.suckless.org/sbase/plain/README, primary, fetched 2026-11-13. Read: portability statement, tool status table, sbase-box.
- [U29] suckless.org, sbase `LICENSE`, https://git.suckless.org/sbase/plain/LICENSE, primary, fetched 2026-11-13. Read: MIT licence heading and copyright lines.
- [U30] ibara/oksh, `README.md` and `LEGAL`, https://raw.githubusercontent.com/ibara/oksh/master/README.md, primary, fetched 2026-11-13. Read: supported systems; public-domain statement for pdksh code.
- [U31] GitHub repository search for "lobase openbsd linux", `weak`, fetched 2026-11-13. Read: names and one-line descriptions only.
- [U32] 9fans/plan9port `LICENSE` and suckless.org 9base page (https://tools.suckless.org/9base/), primary, fetched 2026-11-13. Read: MIT relicensing in 2021 and its exceptions; 9base command list.
- [U33] Heirloom Project, *Heirloom Toolchest*, https://heirloom.sourceforge.net/tools.html, primary, fetched 2026-11-13. Read: origin (Caldera, Sun), utility list.
- [U34] onetrueawk/awk, `README.md`, `FIXES`, and source files (`b.c lex.c lib.c main.c parse.c run.c tran.c awkgram.y`), https://github.com/onetrueawk/awk, primary, fetched 2026-11-13. Read: second-edition UTF-8/CSV notes, yacc/bison requirement, `wc -l` of files.
- [U35] onetrueawk/awk, `makefile` and repository listing (GitHub API), primary, fetched 2026-11-13. Read: `YACC = bison -d`, `maketab` host step, no generated parser in the listing.
- [U36] onetrueawk/awk, `LICENSE`, primary, fetched 2026-11-13. Read: Lucent permission notice.
- [U37] ThomasDickey/mawk-snapshots, `COPYING`, primary, fetched 2026-11-13. Read: GPL version 2 heading.
- [U38] Mark Adler et al., *zlib*, https://zlib.net/ and `LICENSE` in madler/zlib, primary, fetched 2026-11-13. Read: current release line and licence notice.
- [U39] richgel999/miniz, `README` and `LICENSE`, primary, fetched 2026-11-13. Read: "MIT licensed", single-file deflate/inflate, inflater size remark.
- [U40] tukaani-project/xz, `COPYING`, primary, fetched 2026-11-13. Read: which parts are 0BSD, GPLv2+ scripts, LGPL getopt_long.
- [U41] Tukaani, *XZ Embedded*, https://tukaani.org/xz/embedded.html, primary, fetched 2026-11-13. Read: decompressor-only description and licence.
- [U42] facebook/zstd, `README.md`, primary, fetched 2026-11-13. Read: "dual BSD OR GPLv2".
- [U43] bzip2 project, https://sourceware.org/bzip2/, primary, fetched 2026-11-13. Read: current version 1.0.8 and BSD-style licence statement.
- [U44] libarchive project, https://www.libarchive.org/ and `COPYING` and `README.md` in libarchive/libarchive, primary, fetched 2026-11-13. Read: bsdtar/bsdcpio/bsdcat/bsdunzip, formats, licence file summary.
- [U45] Tukaani, *XZ Utils* home page https://tukaani.org/xz/ and *XZ Utils backdoor* https://tukaani.org/xz-backdoor/, primary, fetched 2026-11-13. Read: CVE-2024-3094 facts, affected versions and dates.
- [U46] The Open Group, *POSIX.1-2024 pax*, https://pubs.opengroup.org/onlinepubs/9799919799/utilities/pax.html, primary, fetched 2026-11-13. Read: format names cpio, pax, ustar.
- [U47] The Open Group, *POSIX.1-2024 Utilities index*, https://pubs.opengroup.org/onlinepubs/9799919799/idx/utilities.html, primary, fetched 2026-11-13. Read: the utility name list.
- [U48] The Open Group, POSIX.1-2024 pages for `grep`, `sed`, `find`, `xargs`, `diff`, `patch` under https://pubs.opengroup.org/onlinepubs/9799919799/utilities/, primary, fetched 2026-11-13. Read: synopsis and option lists to check `-r`, `-i`, `-print0`, `-0`, `-u`.
- [U49] openbsd/src `usr.bin/diff/diff.c`, `usr.bin/patch/patch.c`, `usr.bin/bc/bc.y` (raw GitHub), and GNU diffutils `COPYING` (https://git.savannah.gnu.org/cgit/diffutils.git/plain/COPYING), primary, fetched 2026-11-13. Read: licence headers (ISC-style, Larry Wall condition, GPL v3 text).
- [U50] gwsw/less, `LICENSE` and `README`, and openbsd/src `usr.bin/less/main.c`, primary, fetched 2026-11-13. Read: GPL-3-or-later or Less License; curses/termcap link list; OpenBSD's header.
- [U51] openbsd/src `usr.bin/vi/LICENSE` and `README` (nvi 1.79), primary, fetched 2026-11-13. Read: UC Regents 3-clause plus Bostic/Verdoolaege notices; `cl` curses interface.
- [U52] Zoraya repository, `docs/research/stage-6/plan.md`, local, primary, read 2026-11-13. Read: stage 6c's `mmap` private and `fork` through the move list.

### Sources: pseudo-terminals (P)

- [P1] Linux man-pages, *pty(7)*, https://man7.org/linux/man-pages/man7/pty.7.html, primary, fetched 2026-11-13. Read: whole page; pair semantics, SysV vs BSD, consumers list.
- [P2] Linux man-pages, *pts(4)* (ptmx), https://man7.org/linux/man-pages/man4/pts.4.html, primary, fetched 2026-11-13. Read: whole page; /dev/ptmx behaviour, uses incl. su/passwd.
- [P3] The Open Group, *posix_openpt*, POSIX.1-2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/posix_openpt.html, primary, fetched 2026-11-13. Read: description and oflag values.
- [P4] The Open Group, *grantpt*, POSIX.1-2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/grantpt.html, primary, fetched 2026-11-13. Read: ownership and mode rules.
- [P5] Linux man-pages, *openpty(3)*, https://man7.org/linux/man-pages/man3/openpty.3.html, primary, fetched 2026-11-13. Read: openpty, login_tty, forkpty descriptions.
- [P6] Linux man-pages, *TIOCSCTTY(2const)*, https://man7.org/linux/man-pages/man2/TIOCSCTTY.2const.html, primary, fetched 2026-11-13. Read: controlling-terminal rules, CAP_SYS_ADMIN steal.
- [P7] Linux man-pages, *TIOCPKT(2const)* (also TIOCGPTPEER), https://man7.org/linux/man-pages/man2/TIOCPKT.2const.html, primary, fetched 2026-11-13. Read: packet mode, TIOCGPTPEER and the namespace warning.
- [P8] Linux man-pages, *TIOCSWINSZ(2const)*, https://man7.org/linux/man-pages/man2/TIOCSWINSZ.2const.html, primary, fetched 2026-11-13. Read: window size and SIGWINCH.
- [P9] Linux man-pages, *tty(4)*, https://man7.org/linux/man-pages/man4/tty.4.html, primary, fetched 2026-11-13. Read: /dev/tty as controlling terminal synonym, TIOCNOTTY.
- [P10] glibc, `login/programs/pt_chown.c`, https://raw.githubusercontent.com/bminor/glibc/master/login/programs/pt_chown.c, primary (GitHub mirror), fetched 2026-11-13. Read: header comment, setuid message, chown of slave.
- [P11] Linux kernel docs, *The Devpts Filesystem*, https://www.kernel.org/doc/html/latest/filesystems/devpts.html, primary, fetched 2026-11-13. Read: whole page.
- [P12] Linux man-pages, *TIOCSTI(2const)*, https://man7.org/linux/man-pages/man2/TIOCSTI.2const.html, primary, fetched 2026-11-13. Read: description, 6.2 CAP_SYS_ADMIN note.
- [P13] Linux kernel commits 83efeeeb3d04 and 690c8b804ad2 (messages), via https://api.github.com/search/commits?q=repo:torvalds/linux+%22Allow+TIOCSTI+to+be+disabled%22, primary (kernel history via GitHub API), fetched 2026-11-13. Read: commit messages and dates.
- [P14] Linux, `drivers/tty/Kconfig`, https://raw.githubusercontent.com/torvalds/linux/master/drivers/tty/Kconfig, primary, fetched 2026-11-13. Read: `LEGACY_TIOCSTI` help text.
- [P15] Flatpak, *CVE-2017-5226: Sandbox escape via TIOCSTI ioctl*, https://github.com/flatpak/flatpak/security/advisories/GHSA-7gfv-rvfx-h87x, primary, fetched 2026-11-13. Read: impact, patches, workarounds.
- [P16] Flatpak, `NEWS`, https://raw.githubusercontent.com/flatpak/flatpak/main/NEWS, primary, fetched 2026-11-13. Read: notes for 1.3.1 and 0.8.1 on TIOCSTI seccomp.
- [P17] tmux, *tmux(1)* man page, https://man7.org/linux/man-pages/man1/tmux.1.html, primary, fetched 2026-11-13. Read: description of panes as pseudo terminals.
- [P18] tmux, `spawn.c`, https://raw.githubusercontent.com/tmux/tmux/master/spawn.c, primary, fetched 2026-11-13. Read: `fdforkpty` call with window size, ISC-style header.
- [P19] GNU screen, *screen(1)*, https://man7.org/linux/man-pages/man1/screen.1.html, primary, fetched 2026-11-13. Read: passages on newly allocated pseudo-tty mode and the pty at a window's master side.
- [P20] util-linux, *script(1)*, https://man7.org/linux/man-pages/man1/script.1.html, primary, fetched 2026-11-13. Read: pty slave ECHO option and the input-logging warning.
- [P21] OpenSSH portable, `sshpty.c`, https://raw.githubusercontent.com/openssh/openssh-portable/master/sshpty.c, primary, fetched 2026-11-13. Read: `pty_allocate`, `pty_make_controlling_tty`, `pty_setowner`, `pty_change_window_size`.
- [P22] OpenSSH portable, `session.c`, https://raw.githubusercontent.com/openssh/openssh-portable/master/session.c, primary, fetched 2026-11-13. Read: `do_exec_pty`, window-change handling.
- [P23] OpenSSH portable, `ssh.c`, https://raw.githubusercontent.com/openssh/openssh-portable/master/ssh.c, primary, fetched 2026-11-13. Read: lines about `tty_flag` and "Do not allocate a tty if stdin is not a tty".
- [P24] OpenSSH portable, `sshtty.c` and `clientloop.c`, https://raw.githubusercontent.com/openssh/openssh-portable/master/sshtty.c, primary, fetched 2026-11-13. Read: `enter_raw_mode` with `tcsetattr`; clientloop's SIGWINCH handler.
- [P25] OpenBSD, *ssh(1)*, https://man.openbsd.org/ssh.1, primary, fetched 2026-11-13. Read: `-t`/`-T` text on pseudo-terminal allocation.
- [P26] OpenBSD, *sshd(8)*, https://man.openbsd.org/sshd.8, primary, fetched 2026-11-13. Read: authorized_keys options on tty allocation.
- [P27] Don Libes, *expect(1)*, https://man7.org/linux/man-pages/man1/expect.1.html, primary, fetched 2026-11-13. Read: `spawn` paragraph on the internal pty. (The table's [P27a] is [P25] for ssh `-t`.)
- [P28] CPython, `Lib/pty.py`, https://raw.githubusercontent.com/python/cpython/main/Lib/pty.py, primary, fetched 2026-11-13. Read: `openpty`, `fork`, `spawn`.
- [P29] CPython, `Lib/test/test_pty.py`, https://raw.githubusercontent.com/python/cpython/main/Lib/test/test_pty.py, primary, fetched 2026-11-13. Read: grep of `openpty` tests with termios and window size checks.
- [P30] CPython, `Lib/subprocess.py`, https://raw.githubusercontent.com/python/cpython/main/Lib/subprocess.py, primary, fetched 2026-11-13. Read: grep for "pty" (no pty use found).
- [P31] CPython, `Lib/getpass.py`, https://raw.githubusercontent.com/python/cpython/main/Lib/getpass.py, primary, fetched 2026-11-13. Read: `unix_getpass`.
- [P32] Python documentation, *pty*, https://docs.python.org/3/library/pty.html, primary, fetched 2026-11-13. Read: availability line.
- [P33] git, `compat/terminal.c`, https://raw.githubusercontent.com/git/git/master/compat/terminal.c, primary, fetched 2026-11-13. Read: `git_terminal_prompt` opening `/dev/tty`.
- [P34] git, `prompt.c`, https://raw.githubusercontent.com/git/git/master/prompt.c, primary, fetched 2026-11-13. Read: askpass lookup order before the terminal prompt.
- [P35] Vim, `runtime/doc/terminal.txt`, https://raw.githubusercontent.com/vim/vim/master/runtime/doc/terminal.txt, primary, fetched 2026-11-13. Read: "terminal-unix" paragraph.
- [P36] Vim, `src/os_unix.c`, https://raw.githubusercontent.com/vim/vim/master/src/os_unix.c, primary, fetched 2026-11-13. Read: comments at "Try to open a master pty" and the setsid/`/dev/tty` comment.
- [P37] less, `ttyin.c`, https://raw.githubusercontent.com/gwsw/less/master/ttyin.c, primary, fetched 2026-11-13. Read: `open_tty`.
- [P38] less, `LICENSE`, https://raw.githubusercontent.com/gwsw/less/master/LICENSE, primary, fetched 2026-11-13. Read: licence text.
- [P39] xterm, `main.c`, https://raw.githubusercontent.com/ThomasDickey/xterm-snapshots/master/main.c, primary, fetched 2026-11-13. Read: `get_pty` using `openpty`/`posix_openpt`.
- [P40] foot, `slave.c`, https://codeberg.org/dnkl/foot/raw/branch/master/slave.c, primary, fetched 2026-11-13. Read: setsid, open slave, TIOCSCTTY; and LICENSE head (MIT).
- [P41] sudo, *sudo(8)*, https://www.sudo.ws/docs/man/sudo.man/, primary, fetched 2026-11-13. Read: "Process model" paragraph on pty allocation.
- [P42] OpenBSD/OpenSSH compat, `readpassphrase.c`, https://raw.githubusercontent.com/openssh/openssh-portable/master/openbsd-compat/readpassphrase.c, primary, fetched 2026-11-13. Read: `/dev/tty` open and fallback.
- [P43] GnuPG, *Invoking GPG-AGENT*, https://www.gnupg.org/documentation/manuals/gnupg/Invoking-GPG_002dAGENT.html, primary, fetched 2026-11-13. Read: GPG_TTY paragraph.
- [P44] Fuchsia, *fuchsia.hardware.pty*, https://fuchsia.dev/reference/fidl/fuchsia.hardware.pty, primary, fetched 2026-11-13. Read: protocol overview, OpenClient, SetWindowSize, raw feature.
- [P45] Fuchsia, `src/bringup/bin/ptysvc/README.md`, https://fuchsia.googlesource.com/fuchsia/+/refs/heads/main/src/bringup/bin/ptysvc/README.md, primary, fetched 2026-11-13. Read: whole file.
- [P46] Fuchsia, `ptysvc/meta/ptysvc.cml`, https://fuchsia.googlesource.com/fuchsia/+/refs/heads/main/src/bringup/bin/ptysvc/meta/ptysvc.cml, primary, fetched 2026-11-13. Read: capabilities, expose, BSD-style header.
- [P47] Fuchsia, `ptysvc/meta/ptysvc.bootstrap_shard.cml`, https://fuchsia.googlesource.com/fuchsia/+/refs/heads/main/src/bringup/bin/ptysvc/meta/ptysvc.bootstrap_shard.cml, primary, fetched 2026-11-13. Read: offer to `self/toolbox`.
- [P48] Redox, `ptyd/src/scheme.rs`, https://raw.githubusercontent.com/redox-os/ptyd/master/src/scheme.rs, primary, fetched 2026-11-13. Read: `open` and `dup`.
- [P49] Redox, `ptyd/src/main.rs`, https://raw.githubusercontent.com/redox-os/ptyd/master/src/main.rs, primary, fetched 2026-11-13. Read: scheme socket creation and `setrens(0, 0)`.
- [P50] Redox, `ptyd/src/pty.rs`, https://raw.githubusercontent.com/redox-os/ptyd/master/src/pty.rs, primary, fetched 2026-11-13. Read: VINTR/VQUIT handling with `pgrp`.
- [P51] Redox, `orbterm/src/getpty.rs`, https://raw.githubusercontent.com/redox-os/orbterm/master/src/getpty.rs, primary, fetched 2026-11-13. Read: whole file.
- [P52] Redox, `ptyd` LICENSE (MIT, 2017 Redox OS) https://raw.githubusercontent.com/redox-os/ptyd/master/LICENSE and `orbterm` LICENSE (MIT, 2017 Jeremy Soller) https://raw.githubusercontent.com/redox-os/orbterm/master/LICENSE, primary, fetched 2026-11-13. Read: first lines of each. [P52a] ptyd README, https://raw.githubusercontent.com/redox-os/ptyd/master/README.md, read whole.
- [P53] Genode Labs, *Genode Foundations 23.05, Common session interfaces*, https://genode.org/documentation/genode-foundations/23.05/components/Common_session_interfaces.html, primary, fetched 2026-11-13. Read: "Terminal and UART" section.
- [P54] Genode, `terminal_crosslink` README, https://raw.githubusercontent.com/genodelabs/genode/master/repos/os/src/server/terminal_crosslink/README, primary, fetched 2026-11-13. Read: whole file.
- [P55] Genode, `libc/vfs_plugin.cc`, https://raw.githubusercontent.com/genodelabs/genode/master/repos/libports/src/lib/libc/vfs_plugin.cc, primary, fetched 2026-11-13. Read: ioctl handler for TIOCGWINSZ/TIOCGETA/TIOCSETA.
- [P56] Genode, `terminal_session/terminal_session.h`, https://raw.githubusercontent.com/genodelabs/genode/master/repos/os/include/terminal_session/terminal_session.h, primary, fetched 2026-11-13. Read: header (AGPLv3 notice).
- [P57] Microsoft, *Introducing the Windows Pseudo Console (ConPTY)*, https://devblogs.microsoft.com/commandline/windows-command-line-introducing-the-windows-pseudo-console-conpty/, primary, fetched 2026-11-13. Read: "From TTY to PTY" through the architecture section.
- [P58] Microsoft, *CreatePseudoConsole function*, https://learn.microsoft.com/en-us/windows/console/createpseudoconsole, primary, fetched 2026-11-13. Read: signature, parameters, remarks.
- [P59] Plan 9, *rio(4)*, https://9p.io/magic/man2html/4/rio, primary, fetched 2026-11-13. Read: window creation via mount of `$wsys`, `-pid`, cons/consctl.
- [P60] Rob Pike, *8½, the Plan 9 Window System*, https://9p.io/sys/doc/8%C2%BD/8%C2%BD.html, primary, fetched 2026-11-13. Read: the per-window `/dev/cons` passage.
- [P61] Plan 9, *cons(3)*, https://9p.io/magic/man2html/3/cons, primary, fetched 2026-11-13. Read: consctl raw mode.
- [P62] Plan 9, *vt(1)*, https://9p.io/magic/man2html/1/vt, primary, fetched 2026-11-13. Read: whole page.
- [P63] WebAssembly, wasi-cli `wit/terminal.wit`, https://raw.githubusercontent.com/WebAssembly/wasi-cli/main/wit/terminal.wit, primary, fetched 2026-11-13. Read: whole file.
- [P64] FreeBSD, *rights(4)*, https://man.freebsd.org/cgi/man.cgi?query=rights&sektion=4&format=html, primary, fetched 2026-11-13. Read: CAP_IOCTL and CAP_TTYHOOK entries.
- [P65] FreeBSD, *capsicum(4)*, https://man.freebsd.org/cgi/man.cgi?query=capsicum&sektion=4&format=html, primary, fetched 2026-11-13. Read: searched for tty/ioctl/`/dev` (only a cap_ioctls_limit cross-reference).

### Sources: git and ssh (G)

- [G1] git project, `COPYING`, https://raw.githubusercontent.com/git/git/master/COPYING, primary, fetched 2026-11-13. Read: head; GPL v2 only statement.
- [G2] libgit2 project, `COPYING`, https://raw.githubusercontent.com/libgit2/libgit2/main/COPYING, primary, fetched 2026-11-13. Read: head; GPL-2 plus linking exception.
- [G3] GitoxideLabs, workspace `Cargo.toml`, https://raw.githubusercontent.com/GitoxideLabs/gitoxide/main/Cargo.toml, primary, fetched 2026-11-13. Read: `license = "MIT OR Apache-2.0"`.
- [G4] GitoxideLabs, `crate-status.md`, https://raw.githubusercontent.com/GitoxideLabs/gitoxide/main/crate-status.md, primary, fetched 2026-11-13. Read: gix-transport, gix-protocol, gix-ssh, gix-credentials parts; push, ssh, curl/reqwest, redirects.
- [G5] Game of Trees, FAQ, https://gameoftrees.org/faq.html, primary, fetched 2026-11-13. Read: "Does Got contain GPL-licensed Git code?", what makes Got special.
- [G6] Game of Trees, Goals, https://gameoftrees.org/goals.html, primary, fetched 2026-11-13. Read: whole page; pledge/unveil, privilege separation, BSD licence, ssh push.
- [G7] got-portable, `README.portable`, https://raw.githubusercontent.com/ThomasAdam/got-portable/master/README.portable, primary, fetched 2026-11-13. Read: dependency list; plus the `libexec` directory listing from the GitHub API.
- [G8] got-portable, `lib/dial.c`, https://raw.githubusercontent.com/ThomasAdam/got-portable/master/lib/dial.c, primary, fetched 2026-11-13. Read: ssh dial: `unveil`, `fork`, `execv`.
- [G9] got-portable, `libexec/got-fetch-pack/got-fetch-pack.c`, https://raw.githubusercontent.com/ThomasAdam/got-portable/master/libexec/got-fetch-pack/got-fetch-pack.c, primary, fetched 2026-11-13. Read: pledge `stdio recvfd` and `landlock_no_fs`.
- [G10] got-portable, `lib/fetch.c`, https://raw.githubusercontent.com/ThomasAdam/got-portable/master/lib/fetch.c, primary, fetched 2026-11-13. Read: `socketpair`, `fork`, `got_privsep_exec_child`.
- [G11] dulwich, `COPYING` and `README.rst`, https://raw.githubusercontent.com/dulwich/dulwich/main/COPYING, primary, fetched 2026-11-13. Read: licence statement; pure Python, optional Rust.
- [G12] git project, `INSTALL`, https://raw.githubusercontent.com/git/git/master/INSTALL, primary, fetched 2026-11-13. Read: dependency list.
- [G13] git project, `Makefile`, https://raw.githubusercontent.com/git/git/master/Makefile, primary, fetched 2026-11-13. Read: header `Define NO_*` comments, SHA-1/SHA-256 backends, Rust, curl block, script lists.
- [G14] git project, `run-command.c`, https://raw.githubusercontent.com/git/git/master/run-command.c, primary, fetched 2026-11-13. Read: `start_command`, `NO_PTHREADS` async fork.
- [G15] git project, `git.c`, https://raw.githubusercontent.com/git/git/master/git.c, primary, fetched 2026-11-13. Read: builtin command table (counted with `grep -c`).
- [G16] git project, `compat/mingw.c`, https://raw.githubusercontent.com/git/git/master/compat/mingw.c, primary, fetched 2026-11-13. Read: `mingw_spawnve_fd`, `CreateProcessW`.
- [G17] git project, `pager.c`, https://raw.githubusercontent.com/git/git/master/pager.c, primary, fetched 2026-11-13. Read: `start_command(&pager_process)`, `isatty`.
- [G18] git project, `builtin/gc.c`, https://raw.githubusercontent.com/git/git/master/builtin/gc.c, primary, fetched 2026-11-13. Read: `daemonize` use and its failure comment.
- [G19] git project, `setup.c`, https://raw.githubusercontent.com/git/git/master/setup.c, primary, fetched 2026-11-13. Read: `daemonize`, ceiling and filesystem checks, `ensure_valid_ownership`, gitfile reading.
- [G20] git project, `fetch-pack.c`, https://raw.githubusercontent.com/git/git/master/fetch-pack.c, primary, fetched 2026-11-13. Read: the `index-pack`/`unpack-objects` command names.
- [G21] git, gitrepository-layout, https://git-scm.com/docs/gitrepository-layout, primary, fetched 2026-11-13. Read: gitfile, alternates.
- [G22] git, git-config, https://git-scm.com/docs/git-config, primary, fetched 2026-11-13. Read: FILES, SCOPES, `core.*` entries, `include`, `http.followRedirects`, `gc.autoDetach`.
- [G23] git project, release notes `Documentation/RelNotes/2.30.3.adoc`, https://raw.githubusercontent.com/git/git/master/Documentation/RelNotes/2.30.3.adoc, primary, fetched 2026-11-13. Read: CVE-2022-24765 text.
- [G24] git project, `lockfile.h`, https://raw.githubusercontent.com/git/git/master/lockfile.h, primary, fetched 2026-11-13. Read: header comment on lock files.
- [G25] git, git-clone, https://git-scm.com/docs/git-clone, primary, fetched 2026-11-13. Read: `--local` hard links, `--no-hardlinks`.
- [G26] git project, release notes `Documentation/RelNotes/2.13.7.adoc`, https://raw.githubusercontent.com/git/git/master/Documentation/RelNotes/2.13.7.adoc, primary, fetched 2026-11-13. Read: CVE-2018-11235 text.
- [G27] git project, `compat/mmap.c`, https://raw.githubusercontent.com/git/git/master/compat/mmap.c, primary, fetched 2026-11-13. Read: `NO_MMAP` emulation.
- [G28] git project, `ident.c`, https://raw.githubusercontent.com/git/git/master/ident.c, primary, fetched 2026-11-13. Read: `getpwuid` fallback, `/etc/mailname`.
- [G29] git, gitcredentials, https://git-scm.com/docs/gitcredentials, primary, fetched 2026-11-13. Read: askpass order.
- [G30] git, git-credential-store, https://git-scm.com/docs/git-credential-store, primary, fetched 2026-11-13. Read: file paths, plaintext warning.
- [G31] git, git-credential-cache, https://git-scm.com/docs/git-credential-cache, primary, fetched 2026-11-13. Read: daemon, Unix socket.
- [G32] git project, `compat/terminal.c`, https://raw.githubusercontent.com/git/git/master/compat/terminal.c, primary, fetched 2026-11-13. Read: `/dev/tty` path, `tcsetattr`.
- [G33] git, git(1) environment, https://git-scm.com/docs/git, primary, fetched 2026-11-13. Read: `GIT_EDITOR`, `GIT_SSH*`, `GIT_TERMINAL_PROMPT`. [G33a]: `editor.c` in the git tree, https://raw.githubusercontent.com/git/git/master/editor.c, primary, fetched 2026-11-13: editor selection and the `dumb` terminal rule.
- [G34] OpenSSH, `ssh.1` (mdoc source), https://raw.githubusercontent.com/openssh/openssh-portable/master/ssh.1, primary, fetched 2026-11-13. Read: FILES, `-A`, escape characters, ENVIRONMENT, pty rules.
- [G35] OpenSSH, `ssh_config.5`, https://raw.githubusercontent.com/openssh/openssh-portable/master/ssh_config.5, primary, fetched 2026-11-13. Read: the directives named in the text.
- [G36] OpenSSH, `ssh.c`, https://raw.githubusercontent.com/openssh/openssh-portable/master/ssh.c, primary, fetched 2026-11-13. Read: `getpwuid` and "No user exists", `TERM`.
- [G37] OpenSSH, `authfile.c`, https://raw.githubusercontent.com/openssh/openssh-portable/master/authfile.c, primary, fetched 2026-11-13. Read: private key permission check.
- [G38] OpenSSH, `readconf.c`, https://raw.githubusercontent.com/openssh/openssh-portable/master/readconf.c, primary, fetched 2026-11-13. Read: "Bad owner or permissions" check.
- [G39] OpenSSH, `sshconnect.c`, https://raw.githubusercontent.com/openssh/openssh-portable/master/sshconnect.c, primary, fetched 2026-11-13. Read: changed-host-key banner, "Offending key" lines.
- [G40] OpenSSH, `clientloop.c`, https://raw.githubusercontent.com/openssh/openssh-portable/master/clientloop.c, primary, fetched 2026-11-13. Read: `SIGWINCH` handling, `pty-req` construction.
- [G41] OpenSSH, `sshtty.c`, https://raw.githubusercontent.com/openssh/openssh-portable/master/sshtty.c, primary, fetched 2026-11-13. Read: raw-mode `tcsetattr`.
- [G42] OpenSSH, `readpass.c`, https://raw.githubusercontent.com/openssh/openssh-portable/master/readpass.c, primary, fetched 2026-11-13. Read: `/dev/tty`, askpass.
- [G43] OpenSSH, `PROTOCOL.agent`, https://raw.githubusercontent.com/openssh/openssh-portable/master/PROTOCOL.agent, primary, fetched 2026-11-13. Read: session-bind and destination-constraint extensions.
- [G44] OpenSSH, `ssh-agent.1`, https://raw.githubusercontent.com/openssh/openssh-portable/master/ssh-agent.1, primary, fetched 2026-11-13. Read: ENVIRONMENT, FILES.
- [G45] OpenSSH, `ssh-add.1`, https://raw.githubusercontent.com/openssh/openssh-portable/master/ssh-add.1, primary, fetched 2026-11-13. Read: `-c`, `-h`.
- [G46] OpenSSH, release notes, https://www.openssh.com/releasenotes.html, primary, fetched 2026-11-13. Read: "Changes since OpenSSH 8.8" (agent restriction system), "Changes since OpenSSH 8.9" (scp via SFTP), 8.9 bug note, libcrypto-less notes.
- [G47] OpenSSH portable, `INSTALL`, https://raw.githubusercontent.com/openssh/openssh-portable/master/INSTALL, primary, fetched 2026-11-13. Read: prerequisites, `--without-openssl`, entropy.
- [G48] OpenSSH portable, `LICENCE`, https://raw.githubusercontent.com/openssh/openssh-portable/master/LICENCE, primary, fetched 2026-11-13. Read: summary and numbered components.
- [G49] OpenSSH, `sshd.8`, https://raw.githubusercontent.com/openssh/openssh-portable/master/sshd.8, primary, fetched 2026-11-13. Read: `-i`, `/var/empty`.
- [G50] OpenSSH, `sshd_config.5`, https://raw.githubusercontent.com/openssh/openssh-portable/master/sshd_config.5, primary, fetched 2026-11-13. Read: `StrictModes`.
- [G51] OpenSSH, `PROTOCOL.mux`, https://raw.githubusercontent.com/openssh/openssh-portable/master/PROTOCOL.mux, primary, fetched 2026-11-13. Read: first paragraphs; Unix socket, packet framing.
- [G52] IETF, RFC 4254, https://www.rfc-editor.org/rfc/rfc4254.txt, primary, fetched 2026-11-13. Read: 6.2 `pty-req`, 6.7 `window-change`.
- [G53] Dropbear, `LICENSE`, https://raw.githubusercontent.com/mkj/dropbear/master/LICENSE, primary, fetched 2026-11-13. Read: head; MIT text.
- [G54] Dropbear, project page, https://matt.ucc.asn.au/dropbear/dropbear.html, primary, fetched 2026-11-13. Read: features, 110 kB, multi-hop, MIT-style.
- [G55] Dropbear, `SMALL.md`, https://raw.githubusercontent.com/mkj/dropbear/master/SMALL.md, primary, fetched 2026-11-13. Read: size tips, libtommath.
- [G56] Dropbear, `MULTI.md`, https://raw.githubusercontent.com/mkj/dropbear/master/MULTI.md, primary, fetched 2026-11-13. Read: multi-binary.
- [G57] Dropbear, `src/default_options.h`, https://raw.githubusercontent.com/mkj/dropbear/master/src/default_options.h, primary, fetched 2026-11-13. Read: forwarding, PROXYCMD, NETCAT, SFTPSERVER, MULTIUSER.
- [G58] Dropbear, `manpages/dbclient.1`, https://raw.githubusercontent.com/mkj/dropbear/master/manpages/dbclient.1, primary, fetched 2026-11-13. Read: `-y`, `-J`, `-o`, agent, sftp, StrictHostKeyChecking.
- [G59] Dropbear, `manpages/dropbear.8`, https://raw.githubusercontent.com/mkj/dropbear/master/manpages/dropbear.8, primary, fetched 2026-11-13. Read: `-i`.
- [G60] Dropbear, `src/cli-kex.c`, https://raw.githubusercontent.com/mkj/dropbear/master/src/cli-kex.c, primary, fetched 2026-11-13. Read: `known_hosts` lookup, `HOME`/`getpwuid`.
- [G61] Dropbear, `src/sshpty.c`, https://raw.githubusercontent.com/mkj/dropbear/master/src/sshpty.c, primary, fetched 2026-11-13. Read: `openpty`.
- [G62] libssh2, `COPYING`, https://raw.githubusercontent.com/libssh2/libssh2/master/COPYING, primary, fetched 2026-11-13. Read: head and GitHub licence detection (BSD-3-Clause).
- [G63] libssh, `COPYING`, https://gitlab.com/libssh/libssh-mirror/-/raw/master/COPYING, primary, fetched 2026-11-13. Read: head; LGPL 2.1.
- [G64] wolfSSL, wolfSSH `LICENSING`, https://raw.githubusercontent.com/wolfSSL/wolfssh/master/LICENSING, primary, fetched 2026-11-13. Read: head; GPLv3 or commercial.
- [G65] russh, `README.md` and `russh/Cargo.toml`, https://raw.githubusercontent.com/Eugeny/russh/main/russh/Cargo.toml, primary, fetched 2026-11-13. Read: licence Apache-2.0, description.
- [G66] tinyssh, `LICENSE.md` and `README.md`, https://raw.githubusercontent.com/janmojzis/tinyssh/master/LICENSE.md, primary, fetched 2026-11-13. Read: licence expression; feature list.

### Sources: capability systems and interactive tools (K)

- [K1] M. Seaborn, Plash wiki, *Plash's pola-shell*, http://web.archive.org/web/20121221113548/http://plash.beasts.org/wiki/pola-shell, primary (archive copy), fetched 2026-11-13. Read: `=>`, `+`, quoting, installation endowment, argument lists.
- [K2] M. Seaborn, Plash wiki, *pola-run*, http://web.archive.org/web/20121221113548/http://plash.beasts.org/wiki/PolaRun, primary (archive copy), fetched 2026-11-13. Read: `-f`/`-t` options, `--powerbox`, `--pet-name`.
- [K3] M. Seaborn, Plash wiki, *FilePowerbox*, http://web.archive.org/web/20121221113548/http://plash.beasts.org/wiki/FilePowerbox, primary (archive copy), fetched 2026-11-13. Read: definition, trusted-path argument, API arguments, history note (Yee and Walker, 2000). Also a web search for Plash and Emacs, which found nothing.
- [K4] M. Seaborn, Plash wiki, *GtkFilePowerboxHook*, http://web.archive.org/web/20121221113548/http://plash.beasts.org/wiki/GtkFilePowerboxHook, primary (archive copy), fetched 2026-11-13. Read: LD_PRELOAD replacement of `GtkFileChooserDialog`, limitations (no multi-select, filters).
- [K5] M. Seaborn, Plash wiki, *PlashFAQs*, http://web.archive.org/web/20121221113548/http://plash.beasts.org/wiki/PlashFAQs, primary (archive copy), fetched 2026-11-13. Read: chroot jail, replaced glibc, server socket.
- [K6] M. Seaborn, Plash wiki, *ProtocolEnvVars*, http://web.archive.org/web/20121221113548/http://plash.beasts.org/wiki/ProtocolEnvVars, primary (archive copy), fetched 2026-11-13. Read: `PLASH_COMM_FD`, `PLASH_CAPS`.
- [K7] M. Seaborn, Plash wiki, *ComparisonWithOtherSystems*, http://web.archive.org/web/20121221113548/http://plash.beasts.org/wiki/ComparisonWithOtherSystems, primary (archive copy), fetched 2026-11-13. Read: why static policy over-grants (e-mail attachment example); used only as background.
- [K8] Deno, *Security and permissions*, https://docs.deno.com/runtime/fundamentals/security/, primary, fetched 2026-11-13. Read: interactive prompt text and choices, `--no-prompt`, `NotCapable`, subprocess inheritance, broker mode.
- [K9] Deno, GitHub advisory GHSA-vq67-rp93-65qf (CVE-2023-28446), https://github.com/denoland/deno/security/advisories/GHSA-vq67-rp93-65qf, primary, fetched 2026-11-13. Read: ANSI-escape prompt spoofing, versions.
- [K10] Deno, GitHub advisory GHSA-m4pq-fv2w-6hrw (CVE-2024-27936), https://github.com/denoland/deno/security/advisories/GHSA-m4pq-fv2w-6hrw, primary, fetched 2026-11-13. Read: stripped display vs unstripped decision, example path, versions.
- [K11] FreeBSD, `cap_fileargs(3)`, https://man.freebsd.org/cgi/man.cgi?query=cap_fileargs&sektion=3, primary, fetched 2026-11-13. Read: API, flags, operations, experimental note, example.
- [K12] FreeBSD, `libcasper(3)`, https://man.freebsd.org/cgi/man.cgi?query=libcasper&sektion=3, primary, fetched 2026-11-13. Read: service list.
- [K13] GitHub code search API, query `fileargs_init repo:freebsd/freebsd-src`, run 2026-11-13 with `gh api`, secondary (a listing of paths from the project's source; may be incomplete). Read: the list of files that call `fileargs_init`.
- [K14] FreeBSD, `usr.bin/head/head.c`, `usr.bin/wc/wc.c`, `usr.bin/tail/tail.c`, `bin/cat/cat.c`, https://raw.githubusercontent.com/freebsd/freebsd-src/main/ (the four paths), primary, fetched 2026-11-13. Read: `fileargs_init` and `caph_enter_casper` calls (grep).
- [K15] OpenBSD, `usr.bin/vi/common/main.c`, https://raw.githubusercontent.com/openbsd/src/master/usr.bin/vi/common/main.c, primary, fetched 2026-11-13 (598 lines by `wc -l`). Read: pledge calls (lines 72, 225), `-S` secure option; no `unveil`.
- [K16] OpenBSD, `usr.bin/mg/main.c`, https://raw.githubusercontent.com/openbsd/src/master/usr.bin/mg/main.c, primary, fetched 2026-11-13. Read: pledge (line 74); no `unveil`.
- [K17] OpenBSD, `usr.bin/less/main.c`, https://raw.githubusercontent.com/openbsd/src/master/usr.bin/less/main.c, primary, fetched 2026-11-13. Read: `LESSSECURE` and pledge calls (lines 93, 98, 227); no `unveil`.
- [K18] Game of Trees, `got/got.c` (portable mirror of the OpenBSD code), https://raw.githubusercontent.com/ThomasAdam/got-portable/master/got/got.c, primary (project mirror; the OpenBSD tree copy is at a path I did not find), fetched 2026-11-13. Read: `apply_unveil`, editor unveil, 53 `unveil(` lines by `grep -c`.
- [K19] Game of Trees, `gotd/gotd.c`, https://raw.githubusercontent.com/ThomasAdam/got-portable/master/gotd/gotd.c, primary (mirror), fetched 2026-11-13. Read: `apply_unveil_repo_readonly/readwrite`.
- [K20] OpenBSD, `unveil(2)`, https://man.openbsd.org/unveil.2, primary, fetched 2026-11-13. Read: permissions, lock, `ENOENT`/`EACCES`, non-directory names, execve resets, advice to unveil directories.
- [K21] OpenBSD, `pledge(2)`, https://man.openbsd.org/pledge.2, primary, fetched 2026-11-13. Read: `rpath`/`wpath`/`cpath`, `SIGABRT`, `error` promise.
- [K22] Flatpak, command reference (`flatpak run --file-forwarding`), https://docs.flatpak.org/en/latest/flatpak-command-reference.html, primary, fetched 2026-11-13. Read: `@@` semantics, non-persistent read-write export.
- [K23] xdg-desktop-portal, `org.freedesktop.portal.Documents`, https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Documents.html, primary, fetched 2026-11-13. Read: FUSE path, Add/AddNamed/AddFull, flags, permissions.
- [K24] xdg-desktop-portal, FileChooser interface source, https://raw.githubusercontent.com/flatpak/xdg-desktop-portal/main/data/org.freedesktop.portal.FileChooser.xml, primary, fetched 2026-11-13. Read: options `multiple`, `directory`, `current_folder`; results.
- [K25] Flatpak, *Sandbox permissions*, https://docs.flatpak.org/en/latest/sandbox-permissions.html, primary, fetched 2026-11-13. Read: portals vs `--filesystem`, advice to stay narrow.
- [K26] Apple, *Accessing files from the macOS App Sandbox*, https://developer.apple.com/documentation/security/accessing-files-from-the-macos-app-sandbox (the `.md` rendering), primary, fetched 2026-11-13. Read: panels in a separate process, Dock-icon drags, bookmarks, document-relative bookmarks, related items, full disk access.
- [K27] Apple, `FileManager.SearchPathDirectory.itemReplacementDirectory`, https://developer.apple.com/documentation/foundation/filemanager/searchpathdirectory/itemreplacementdirectory, primary, fetched 2026-11-13. Read: one-line description only.
- [K28] Apple Developer Forums, thread 678819 (Apple engineer's explainer on TCC), https://developer.apple.com/forums/thread/678819, secondary (Apple staff post, not documentation), fetched 2026-11-13. Read: TCC, responsible code, prompt behaviour, no prompt outside a GUI session.
- [K29] Qt blog, *The curious case of the responsible process*, https://www.qt.io/blog/the-curious-case-of-the-responsible-process, weak, fetched 2026-11-13. Read: prompt names the terminal, child inheritance, `launchctl procinfo`.
- [K30] Android Developers, *Storage Access Framework* (document provider guide), https://developer.android.com/guide/topics/providers/document-provider, primary, fetched 2026-11-13. Read: `ACTION_OPEN_DOCUMENT(_TREE)`, persistent access.
- [K31] Genode, `lx_fs` README, https://raw.githubusercontent.com/genodelabs/genode/master/repos/os/src/server/lx_fs/README, primary, fetched 2026-11-13. Read: label-based policy with `root` and `writeable`, default no access.
- [K32] Genode, *Sculpt OS 23.10*, https://genode.org/documentation/articles/sculpt-23-10, primary, fetched 2026-11-13. Read: choosing a file system per `File_system` request in the deploy GUI; system shell with Vim.
- [K33] Genode Foundations, *Common session interfaces* (23.05), https://genode.org/documentation/genode-foundations/23.05/components/Common_session_interfaces.html, primary, fetched 2026-11-13. Read: file-system session description (limited detail on routing).
- [K34] Fuchsia, *Namespaces*, https://fuchsia.dev/fuchsia-src/concepts/process/namespaces, primary, fetched 2026-11-13. Read: path-prefix to handle table, no ambient authority.
- [K35] Fuchsia, *Dot dot considered harmful*, https://fuchsia.dev/fuchsia-src/concepts/filesystems/dotdot, primary, fetched 2026-11-13. Read: `..` blocked on servers, client-side cleanup.
- [K36] Pike, *Plumbing and Other Utilities* (Plan 9), https://9p.io/sys/doc/plumb.html, primary, fetched 2026-11-13. Read: message format, rules, failure when no rule, authority remarks.
- [K37] Wasmtime, *WASI tutorial*, https://raw.githubusercontent.com/bytecodealliance/wasmtime/main/docs/WASI-tutorial.md, primary, fetched 2026-11-13. Read: `--dir`, preopen, "Operation not permitted", `::` remap.
- [K38] Wasmtime, *CLI options*, https://docs.wasmtime.dev/cli-options.html, primary, fetched 2026-11-13. Read: `--dir` must precede the module (little else).
- [K39] Node.js, *Permissions*, https://nodejs.org/api/permissions.html, primary, fetched 2026-11-13. Read: seat-belt statement, flags, `ERR_ACCESS_DENIED`, audit mode, fd bypass, no prompts.
- [K40] Linux kernel, *Landlock*, https://docs.kernel.org/userspace-api/landlock.html, primary, fetched 2026-11-13. Read: access rights, `REFER`, layering, rename rules.
- [K41] bubblewrap, README, https://raw.githubusercontent.com/containers/bubblewrap/main/README.md, primary, fetched 2026-11-13. Read: mount namespace construction, `--new-session` note.
- [K42] bubblewrap, issue 142 (CVE-2017-5226), https://github.com/containers/bubblewrap/issues/142, primary (project tracker), fetched 2026-11-13. Read: `TIOCSTI` injection and `setsid()` fix.
- [K43] Moore, Dimoulas, King, Chong, *SHILL: A Secure Shell Scripting Language*, OSDI 2014, https://files.usenix.org/s3fs-public/cdn/osdi14-paper-moore.pdf, primary, fetched 2026-11-13 (text extracted by hand). Read: footnote "not an interactive shell", directory privileges, no `..`, sandbox with passed capabilities.
- [K44] Watson, Anderson, Laurie, Kennaway, *Capsicum: practical capabilities for UNIX*, USENIX Security 2010, https://www.usenix.org/legacy/event/sec10/tech/full_papers/Watson.pdf, primary, fetched 2026-11-13 (text extracted by hand). Read: delegation by directory capability, `..` and absolute path rules, `gzip` conversion, run-time delegation through the RPC channel.
- [K45] Nuxi, CloudABI, https://github.com/NuxiNL/cloudabi, primary, fetched 2026-11-13 (first 100,000 characters of the page). Read: no global namespace, descriptors from the parent.
- [K46] Qubes OS, *GUI virtualization* (developer docs), https://doc.qubes-os.org/en/latest/developer/system/gui.html, primary, fetched 2026-11-13. Read: coloured frames, name prefix in titles, untrusted rest of title.
- [K47] Qubes OS, *How to copy and move files*, https://doc.qubes-os.org/en/latest/user/how-to-guides/how-to-copy-and-move-files.html, primary, fetched 2026-11-13. Read: dom0 dialog names the target, `qvm-copy`, warnings.
- [K48] Microsoft, *How Interactive Logon Works* (Windows Server 2003), https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-server-2003/cc780332(v=ws.10), primary (archived), fetched 2026-11-13. Read: secure attention sequence registered by Winlogon, secure desktop.
- [K49] Emacs-devel, message of 2021-09 on object-capability security inside Emacs Lisp, https://lists.libreplanet.org/archive/html/emacs-devel/2021-09/msg01154.html, weak, fetched 2026-11-13. Read: it proposes capability-tagged name strings in Lisp, no OS sandbox; cited only to record that no editor-level design was found.
