//! Plays a GUI script against a kernel booted with `--gui --qmp` (TASK-14,
//! ADR-0097): keys go in through QMP, and the screen comes back as QEMU
//! screendumps, which are read as text.
//!
//! **How the screen is read.** The compositor puts the first window at a
//! fixed place, and the terminal draws every character with `zoraya-text`,
//! deterministically (INV-0162-2). A screendump is lossless (PPM), so each
//! cell of the terminal's grid is matched exactly against every character
//! drawn by the same crate with the same fonts, size and colours
//! (`zoraya_term::read`): the tolerance is zero, and a cell that matches
//! no character reads as `?`. The block cursor, a cell of one colour,
//! reads as `_`. Letters every font draws alike (Latin `A` and Cyrillic
//! `А`) read as the Latin one, and a script's text is compared in the same
//! classes. Regions outside the window are checked pixel by pixel against
//! a stated colour.
//!
//! A script is lines, run in order:
//!
//! ```text
//! # a comment
//! serial TEXT       wait until TEXT appears on the serial line
//! then TEXT         wait until TEXT appears on the serial line after the
//!                   last input step (a click, key, chord, type or drag): an
//!                   earlier identical line does not count
//! screen TEXT       wait until a row below the last matched one reads TEXT
//! prefix TEXT       the same for a row that begins with TEXT (a line being
//!                   edited, whose faint suggestion follows what is typed)
//! pause MS          wait MS milliseconds, reading the serial line
//! type KEYS         press and release each key (escapes \r \e \\)
//! key QCODE         press and release one QEMU key code, such as f12
//! chord MOD QCODE   the same with MOD (ctrl, shift, alt, meta) held, such
//!                   as chord ctrl c; chord meta spc switches the layout
//! layout NAME       type the following text as keys of the layout NAME
//!                   (us, ua, ru, cz; ADR-0163), a letter with no key of its
//!                   own through a dead key: it presses nothing itself
//! region X Y W H RGB  wait until every pixel of the rectangle is the colour
//!                   RGB (hex)
//! click X Y         move the pointer to (X, Y) and press and release the
//!                   left button
//! drag X Y X2 Y2 [HOLD]  press the left button at (X, Y), hold it HOLD ms,
//!                   move to (X2, Y2) in steps, release
//! dclick X Y        two left clicks at (X, Y) at once: a double click
//! rclick X Y        a right click at (X, Y)
//! mclick MOD X Y    a left click with MOD (shift, ctrl, alt, meta) held
//! mdrag MOD X Y X2 Y2 [HOLD]  a drag with MOD held at its release
//!
//! Coordinates are logical pixels: the runner multiplies them by the
//! display's scale (ADR-0164), which it takes from the screendump's width
//! as the compositor does (`zoraya_compositor::scale_for`), so one script
//! runs at both scales. The terminal is read at the first window's body
//! (`zoraya_compositor::first_body`).
//! window            a new window: the next `screen` looks from its top row
//! top TEXT          wait until the top row reads TEXT (a cleared screen),
//!                   then a new window as `window`
//! latency N [MS]    type `x` N times at the prompt, timing each until it
//!                   shows, then erase them; with MS, fail if the median
//!                   is more than MS over one screendump read
//! shot NAME         keep the screen as `build/shots/NAME-LABEL.ppm` (the screens
//!                   of a task's report)
//! exit              wait for QEMU to end with status 0
//! ```
//!
//! Every run fails on `ZORAYA-PANIC`, `ZORAYA-FAULT` or `panicked` on the
//! serial line.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::output;
use zoraya_term::Look;
use zoraya_term::read::Reader;
use zoraya_tokens::{Appearance, Scale};

/// The reader of the terminal as `term` draws it at `scale`, the light set.
pub fn reader_at(scale: Scale) -> Reader {
    Reader::new(Look::new(scale, Appearance::Light))
}

/// The reader at scale 1.
pub fn reader() -> Reader {
    reader_at(Scale::ONE)
}

/// How long a modifier key is held before and after a click or a drag's
/// release, at least.
const MODIFIER_SETTLE: Duration = Duration::from_millis(400);

/// How long a drag's button is held after its last move, at least, so that
/// a slow guest has answered the press before the release (a drag is taken
/// only while its press is held, ADR-0170).
const DRAG_SETTLE: Duration = Duration::from_millis(800);

const FORBIDDEN: [&str; 3] = ["ZORAYA-PANIC", "ZORAYA-FAULT", "panicked"];
/// Where the screendumps go, relative to QEMU's working directory: one file
/// per run's label, so that two runs at once do not read each other's.
fn shot_path(label: &str) -> String {
    format!("build/gui-screen-{}.ppm", file_safe(label))
}

/// A label as part of a file name: anything but letters, digits, `-` and
/// `_` becomes `_`, so that a label never leaves `build/` or breaks the
/// QMP command it is quoted in.
fn file_safe(label: &str) -> String {
    label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
/// The time between two keys a `type` step presses: a fast keyboard's rate
/// (USB full speed polls every 8 ms). QEMU's virtio-input event queue holds
/// 64 events, 16 keys (a press and a release, each with its SYN), and QEMU
/// drops events when the guest has not handed buffers back; QMP alone can
/// inject a key in well under a millisecond, which a guest under TCG does
/// not keep up with while it composes (traced as `virtio_input_queue_full`,
/// TASK-24 7a). At this rate the queue holds 128 ms of typing. At scale 2
/// each key costs the guest four times the drawing: the interval is four
/// times longer there (`key_interval`; keys were lost on CI's TCG at
/// scale 2 at 8 ms, 2026-11-09).
const KEY_INTERVAL: Duration = Duration::from_millis(8);
/// The time after a click before the next step: the compositor reads its
/// keyboard and its pointer as two devices, and a key typed with no pause
/// may be read in the same round before the click (TASK-24 7d).
const AFTER_CLICK: Duration = Duration::from_millis(150);
/// The least time between two screendumps while a step waits for the
/// screen: a dump holds QEMU, and back to back (once reading became fast,
/// 2026-11-09) they kept a TCG guest from draining the keys just typed, and
/// keys were lost.
const SCREEN_POLL: Duration = Duration::from_millis(150);
/// Keys typed before `type` waits for the guest to catch up (`settle`): eight
/// keys with Shift are 64 events, virtio-input's whole queue. A fixed interval
/// suits a guest of one speed only: on CI's runner, and locally with QEMU on
/// the efficiency cores, the ends of long lines were lost (2026-11-09).
const KEYS_PER_SETTLE: usize = 8;
/// The longest `settle` waits: a screen that keeps changing (a clock, a
/// program's output) must not hold the typing for good.
const SETTLE_MOST: Duration = Duration::from_secs(10);
/// The longest wait for one QMP reply: QEMU answers in milliseconds, so a
/// silent QEMU fails the run instead of blocking it past its deadline.
const QMP_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Serial(String),
    Then(String),
    Screen(String),
    Prefix(String),
    Pause(u64),
    Type(Vec<u8>),
    Key(String),
    Chord(String, String),
    Region(i64, i64, i64, i64, u32),
    /// The count, and the most the median may exceed one idle screendump read, in ms.
    Latency(usize, Option<u32>),
    Click(i64, i64),
    DoubleClick(i64, i64),
    RightClick(i64, i64),
    /// A click with a modifier's QEMU key held.
    ModClick(String, i64, i64),
    /// A drag with a modifier held from before the release.
    ModDrag(String, i64, i64, i64, i64, u64),
    Layout(usize),
    /// From, to, and how long the press is held before the move, in ms.
    Drag(i64, i64, i64, i64, u64),
    /// Reads the terminal whose body is at this logical point (a second
    /// window), instead of the first.
    Origin(i64, i64),
    /// Points at (`false`) or clicks (`true`) the first pixel of a colour
    /// in a logical rectangle: a part of the dock found by its colour.
    AtColour(bool, u32, i64, i64, i64, i64),
    Window,
    Top(String),
    Shot(String),
    Exit,
}

/// QEMU's key code for a script's modifier name.
fn modifier_qcode(m: &str) -> &str {
    if m == "meta" { "meta_l" } else { m }
}

pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kind, rest) = line.split_once(' ').unwrap_or((line, ""));
        let bad = |what: &str| format!("line {n}: {what}");
        steps.push(match kind {
            "serial" => Step::Serial(rest.to_string()),
            "then" => Step::Then(rest.to_string()),
            "screen" => Step::Screen(rest.to_string()),
            "prefix" => Step::Prefix(rest.to_string()),
            "pause" => Step::Pause(
                rest.parse()
                    .map_err(|_| bad("not a number of milliseconds"))?,
            ),
            "type" => Step::Type(crate::script::unescape(rest).map_err(|e| bad(&e))?),
            "key" => Step::Key(rest.to_string()),
            "chord" => {
                let (m, code) = rest.split_once(' ').ok_or_else(|| bad("chord MOD QCODE"))?;
                if !m
                    .split('+')
                    .all(|m| matches!(m, "ctrl" | "shift" | "alt" | "meta"))
                {
                    return Err(bad(
                        "chord's modifiers are ctrl, shift, alt or meta, joined by +",
                    ));
                }
                Step::Chord(m.to_string(), code.to_string())
            }
            "region" => {
                let v: Vec<&str> = rest.split_whitespace().collect();
                if v.len() != 5 {
                    return Err(bad("region X Y W H RGB"));
                }
                let num = |s: &str| s.parse::<i64>().map_err(|_| bad("not a number"));
                let colour = u32::from_str_radix(v[4], 16).map_err(|_| bad("not a colour"))?;
                Step::Region(num(v[0])?, num(v[1])?, num(v[2])?, num(v[3])?, colour)
            }
            "layout" => Step::Layout(
                zoraya_keymap::by_name(rest).ok_or_else(|| bad("layout us, ua, ru or cz"))?,
            ),
            "click" => {
                let v: Vec<i64> = rest
                    .split_whitespace()
                    .map(|n| n.parse().map_err(|_| bad("not a number")))
                    .collect::<Result<_, _>>()?;
                let [x, y] = v[..] else {
                    return Err(bad("click X Y"));
                };
                Step::Click(x, y)
            }
            "dclick" | "rclick" => {
                let v: Vec<i64> = rest
                    .split_whitespace()
                    .map(|n| n.parse().map_err(|_| bad("not a number")))
                    .collect::<Result<_, _>>()?;
                let [x, y] = v[..] else {
                    return Err(bad("dclick X Y or rclick X Y"));
                };
                if kind == "dclick" {
                    Step::DoubleClick(x, y)
                } else {
                    Step::RightClick(x, y)
                }
            }
            "mclick" | "mdrag" => {
                let (m, rest) = rest.split_once(' ').ok_or_else(|| bad("MOD and a place"))?;
                if !matches!(m, "ctrl" | "shift" | "alt" | "meta") {
                    return Err(bad("the modifier is ctrl, shift, alt or meta"));
                }
                let v: Vec<i64> = rest
                    .split_whitespace()
                    .map(|n| n.parse().map_err(|_| bad("not a number")))
                    .collect::<Result<_, _>>()?;
                match (kind, &v[..]) {
                    ("mclick", [x, y]) => Step::ModClick(m.to_string(), *x, *y),
                    ("mdrag", [x, y, x2, y2]) => Step::ModDrag(m.to_string(), *x, *y, *x2, *y2, 0),
                    ("mdrag", [x, y, x2, y2, h]) if *h >= 0 => {
                        Step::ModDrag(m.to_string(), *x, *y, *x2, *y2, *h as u64)
                    }
                    _ => return Err(bad("mclick MOD X Y or mdrag MOD X Y X2 Y2 [HOLD_MS]")),
                }
            }
            "drag" => {
                let v: Vec<i64> = rest
                    .split_whitespace()
                    .map(|n| n.parse().map_err(|_| bad("not a number")))
                    .collect::<Result<_, _>>()?;
                let (x, y, x2, y2, hold) = match v[..] {
                    [x, y, x2, y2] => (x, y, x2, y2, 0),
                    [x, y, x2, y2, h] if h >= 0 => (x, y, x2, y2, h as u64),
                    _ => return Err(bad("drag X Y X2 Y2 [HOLD_MS]")),
                };
                Step::Drag(x, y, x2, y2, hold)
            }
            "latency" => {
                let mut words = rest.split_whitespace();
                let n = words
                    .next()
                    .and_then(|w| w.parse().ok())
                    .ok_or_else(|| bad("latency N [MS]"))?;
                let bound = match words.next() {
                    None => None,
                    Some(w) => Some(w.parse().map_err(|_| bad("not a number of ms"))?),
                };
                Step::Latency(n, bound)
            }
            "origin" => {
                let v: Vec<i64> = rest
                    .split_whitespace()
                    .map(|n| n.parse().map_err(|_| bad("not a number")))
                    .collect::<Result<_, _>>()?;
                let [x, y] = v[..] else {
                    return Err(bad("origin X Y"));
                };
                Step::Origin(x, y)
            }
            "clickc" | "pointc" => {
                let v: Vec<&str> = rest.split_whitespace().collect();
                if v.len() != 5 {
                    return Err(bad("clickc RGB X Y W H"));
                }
                let num = |s: &str| s.parse::<i64>().map_err(|_| bad("not a number"));
                let colour = u32::from_str_radix(v[0], 16).map_err(|_| bad("not a colour"))?;
                Step::AtColour(
                    kind == "clickc",
                    colour,
                    num(v[1])?,
                    num(v[2])?,
                    num(v[3])?,
                    num(v[4])?,
                )
            }
            "window" => Step::Window,
            "top" => Step::Top(rest.to_string()),
            "shot" => {
                let ok = !rest.is_empty()
                    && rest
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
                if !ok {
                    return Err(bad("shot NAME, of letters, digits, - and _"));
                }
                Step::Shot(rest.to_string())
            }
            "exit" => Step::Exit,
            _ => return Err(bad("unknown step")),
        });
    }
    Ok(steps)
}

/// A screendump: width, height and `0x00RRGGBB` pixels.
pub struct Screen {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

/// A binary PPM (P6) with a maximum of 255.
pub fn parse_ppm(bytes: &[u8]) -> Result<Screen, String> {
    let mut fields = Vec::new();
    let mut at = 0;
    while fields.len() < 4 {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if bytes.get(at) == Some(&b'#') {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        let start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if start == at {
            return Err("a short PPM header".into());
        }
        fields.push(String::from_utf8_lossy(&bytes[start..at]).into_owned());
    }
    at += 1;
    let number = |s: &String| s.parse::<usize>().map_err(|_| format!("PPM field {s:?}"));
    if fields[0] != "P6" || number(&fields[3])? != 255 {
        return Err("not a P6 PPM of depth 255".into());
    }
    let (width, height) = (number(&fields[1])?, number(&fields[2])?);
    let data = bytes
        .get(at..at + width * height * 3)
        .ok_or("a short PPM body")?;
    let pixels = data
        .chunks(3)
        .map(|p| u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]))
        .collect();
    Ok(Screen {
        width,
        height,
        pixels,
    })
}

impl Screen {
    pub fn at(&self, x: usize, y: usize) -> u32 {
        self.pixels[y * self.width + x]
    }

    /// The display's scale, as the compositor chooses it.
    pub fn scale(&self) -> Scale {
        // No window in a test: QEMU states no physical size (ADR-0247).
        zoraya_compositor::scale_for(self.width as u32, 0)
    }

    /// The terminal's rows as text: the first window's body, where the
    /// compositor puts it, with the terminal's padding and grid.
    pub fn rows(&self, reader: &mut Reader) -> Vec<String> {
        self.rows_at(reader, zoraya_compositor::first_body(self.scale()))
    }

    /// The rows of a terminal whose body is at `(ox, oy)`, physical.
    pub fn rows_at(&self, reader: &mut Reader, (ox, oy): (i64, i64)) -> Vec<String> {
        let (columns, rows) = reader
            .look()
            .grid_for(self.width as u32, self.height as u32);
        reader.rows(
            columns,
            rows,
            (ox.max(0) as usize, oy.max(0) as usize),
            (self.width, self.height),
            |x, y| self.at(x, y),
        )
    }
}

/// QEMU's key code name for an evdev key code of the typing area.
pub fn qcode_of(code: u32) -> Option<&'static str> {
    const ROW1: [&str; 12] = [
        "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "minus", "equal",
    ];
    const ROW2: [&str; 12] = [
        "q",
        "w",
        "e",
        "r",
        "t",
        "y",
        "u",
        "i",
        "o",
        "p",
        "bracket_left",
        "bracket_right",
    ];
    const ROW3: [&str; 11] = [
        "a",
        "s",
        "d",
        "f",
        "g",
        "h",
        "j",
        "k",
        "l",
        "semicolon",
        "apostrophe",
    ];
    const ROW4: [&str; 10] = ["z", "x", "c", "v", "b", "n", "m", "comma", "dot", "slash"];
    let c = code as usize;
    Some(match code {
        2..=13 => ROW1[c - 2],
        16..=27 => ROW2[c - 16],
        30..=40 => ROW3[c - 30],
        44..=53 => ROW4[c - 44],
        41 => "grave_accent",
        43 => "backslash",
        57 => "spc",
        86 => "less",
        _ => return None,
    })
}

/// A key to press: its evdev code, and Shift and AltGr held.
pub type Press = (u32, bool, bool);

/// The keys that type `c` in a layout: its own key at some level, or a dead
/// key and a base the accent composes it with (ADR-0163).
pub fn presses(layout: &zoraya_keymap::Layout, c: char) -> Option<Vec<Press>> {
    use zoraya_keymap::{Accent, Out};
    // The lowest level first: a plain key over one with Shift or AltGr.
    let find = |want: Out| -> Option<Press> {
        (0..4).find_map(|l| {
            layout
                .keys
                .iter()
                .find(|(_, levels)| levels[l] == want)
                .map(|(code, _)| (*code, l % 2 == 1, l >= 2))
        })
    };
    if let Some(p) = find(Out::Char(c)) {
        return Some(vec![p]);
    }
    for accent in Accent::ALL {
        let Some(dead) = find(Out::Dead(accent)) else {
            continue;
        };
        for (_, levels) in layout.keys {
            for o in levels {
                if let Out::Char(base) = o
                    && accent.compose(*base) == Some(c)
                    && let Some(b) = find(*o)
                {
                    return Some(vec![dead, b]);
                }
            }
        }
    }
    None
}

/// QMP's key code for a character, and whether Shift is needed.
pub fn qcode(c: char) -> Option<(&'static str, bool)> {
    const LETTERS: [&str; 26] = [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r",
        "s", "t", "u", "v", "w", "x", "y", "z",
    ];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    Some(match c {
        'a'..='z' => (LETTERS[c as usize - 'a' as usize], false),
        'A'..='Z' => (LETTERS[c as usize - 'A' as usize], true),
        '0'..='9' => (DIGITS[c as usize - '0' as usize], false),
        ' ' => ("spc", false),
        '\r' => ("ret", false),
        '\x1b' => ("esc", false),
        '.' => ("dot", false),
        ',' => ("comma", false),
        '/' => ("slash", false),
        '-' => ("minus", false),
        '_' => ("minus", true),
        '=' => ("equal", false),
        ';' => ("semicolon", false),
        ':' => ("semicolon", true),
        '\'' => ("apostrophe", false),
        '"' => ("apostrophe", true),
        '!' => ("1", true),
        '$' => ("4", true),
        '%' => ("5", true),
        '&' => ("7", true),
        '*' => ("8", true),
        '(' => ("9", true),
        ')' => ("0", true),
        '@' => ("2", true),
        '#' => ("3", true),
        '^' => ("6", true),
        '+' => ("equal", true),
        '\\' => ("backslash", false),
        '|' => ("backslash", true),
        '<' => ("comma", true),
        '>' => ("dot", true),
        '?' => ("slash", true),
        '[' => ("bracket_left", false),
        ']' => ("bracket_right", false),
        '{' => ("bracket_left", true),
        '}' => ("bracket_right", true),
        '`' => ("grave_accent", false),
        '~' => ("grave_accent", true),
        '\t' => ("tab", false),
        _ => return None,
    })
}

struct Qmp {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    shot: String,
}

impl Qmp {
    fn connect(path: &str, until: Instant) -> Result<Qmp, String> {
        let stream = loop {
            match UnixStream::connect(path) {
                Ok(s) => break s,
                Err(e) if Instant::now() > until => return Err(format!("QMP {path}: {e}")),
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        };
        stream
            .set_read_timeout(Some(QMP_TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(QMP_TIMEOUT)))
            .map_err(|e| e.to_string())?;
        let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        let mut q = Qmp {
            stream,
            reader,
            shot: shot_path("gui"),
        };
        q.reply()?;
        q.execute(r#"{"execute":"qmp_capabilities"}"#)?;
        Ok(q)
    }

    /// The next line that is not an event.
    fn reply(&mut self) -> Result<String, String> {
        loop {
            let mut line = String::new();
            if self
                .reader
                .read_line(&mut line)
                .map_err(|e| format!("QMP: no reply within {} s ({e})", QMP_TIMEOUT.as_secs()))?
                == 0
            {
                return Err("QMP closed".into());
            }
            if !line.contains("\"event\"") {
                return Ok(line);
            }
        }
    }

    fn execute(&mut self, command: &str) -> Result<String, String> {
        self.stream
            .write_all(format!("{command}\n").as_bytes())
            .map_err(|e| e.to_string())?;
        let reply = self.reply()?;
        if reply.contains("\"error\"") {
            return Err(format!("QMP: {}", reply.trim()));
        }
        Ok(reply)
    }

    fn keys(&mut self, events: &[(&str, bool)]) -> Result<(), String> {
        let list: Vec<String> = events
            .iter()
            .map(|(code, down)| {
                format!(
                    r#"{{"type":"key","data":{{"down":{down},"key":{{"type":"qcode","data":"{code}"}}}}}}"#
                )
            })
            .collect();
        let command = format!(
            r#"{{"execute":"input-send-event","arguments":{{"events":[{}]}}}}"#,
            list.join(",")
        );
        self.execute(&command).map(|_| ())
    }

    fn press(&mut self, code: &str, shift: bool) -> Result<(), String> {
        if shift {
            self.keys(&[
                ("shift", true),
                (code, true),
                (code, false),
                ("shift", false),
            ])
        } else {
            self.keys(&[(code, true), (code, false)])
        }
    }

    fn dump(&mut self) -> Result<(), String> {
        let command = format!(
            r#"{{"execute":"screendump","arguments":{{"filename":"{}"}}}}"#,
            self.shot
        );
        self.execute(&command).map(|_| ())
    }

    /// The pointer to `(x, y)` of a `width × height` display (QMP's
    /// absolute axes run from 0 to 32767), with the left button's change.
    fn pointer(
        &mut self,
        at: (i64, i64),
        size: (usize, usize),
        button: Option<bool>,
    ) -> Result<(), String> {
        self.pointer_with(at, size, button.map(|d| ("left", d)))
    }

    /// The pointer to `(x, y)`, with one button's change, or with each of
    /// `clicks` (button, down) in the same batch.
    fn pointer_with(
        &mut self,
        at: (i64, i64),
        size: (usize, usize),
        button: Option<(&str, bool)>,
    ) -> Result<(), String> {
        self.pointer_batch(at, size, &button.into_iter().collect::<Vec<_>>())
    }

    fn pointer_batch(
        &mut self,
        (x, y): (i64, i64),
        (width, height): (usize, usize),
        clicks: &[(&str, bool)],
    ) -> Result<(), String> {
        let axis = |v: i64, side: usize| {
            (v.clamp(0, side as i64 - 1) * 32767 / (side as i64 - 1).max(1)) as u32
        };
        let mut events = format!(
            r#"{{"type":"abs","data":{{"axis":"x","value":{}}}}},{{"type":"abs","data":{{"axis":"y","value":{}}}}}"#,
            axis(x, width),
            axis(y, height)
        );
        for (b, down) in clicks {
            events.push_str(&format!(
                r#",{{"type":"btn","data":{{"down":{down},"button":"{b}"}}}}"#
            ));
        }
        let command =
            format!(r#"{{"execute":"input-send-event","arguments":{{"events":[{events}]}}}}"#);
        self.execute(&command).map(|_| ())
    }

    fn screen(&mut self) -> Result<Screen, String> {
        self.dump()?;
        let bytes = std::fs::read(&self.shot).map_err(|e| format!("{}: {e}", self.shot))?;
        parse_ppm(&bytes)
    }
}

struct Run {
    child: Child,
    qmp: Qmp,
    serial: mpsc::Receiver<(String, output::Read)>,
    /// The serial line so far, at most `output::LOG_CAP` bytes.
    seen: String,
    /// Where `seen` ended when the last input step began: `then` looks
    /// from there.
    mark: usize,
    out: output::Output,
    label: String,
    deadline: Instant,
    /// The row of the last `screen` match.
    row: Option<usize>,
    reader: Reader,
    /// The scale `reader` reads at.
    scale: Scale,
    /// The layout `type` types in (`layout`): an index of
    /// `zoraya_keymap::LAYOUTS`, 0 being us.
    layout: usize,
    /// The body of the terminal read (`origin`), logical; the first's when
    /// none.
    origin: Option<(i64, i64)>,
}

impl Run {
    fn pump(&mut self) -> Result<(), String> {
        while let Ok((line, read)) = self.serial.try_recv() {
            self.out.counted_line(&line, read.cut, read.bytes)?;
            if let Some(bad) = FORBIDDEN.iter().find(|f| line.contains(**f)) {
                return Err(format!("{bad} on the serial line: {line}"));
            }
            output::keep(&mut self.seen, &line)?;
            output::keep(&mut self.seen, "\n")?;
        }
        if Instant::now() > self.deadline {
            return Err("timeout".into());
        }
        Ok(())
    }

    /// Waits until two screendumps a poll apart are the same: the guest has
    /// drawn every key typed so far, so virtio-input's queue is empty again.
    fn settle(&mut self) -> Result<(), String> {
        let until = Instant::now() + SETTLE_MOST;
        let mut last = self.qmp.screen()?.pixels;
        while Instant::now() < until {
            self.pump()?;
            std::thread::sleep(SCREEN_POLL);
            let now = self.qmp.screen()?.pixels;
            if now == last {
                return Ok(());
            }
            last = now;
        }
        Ok(())
    }

    fn wait_screen(&mut self, text: &str) -> Result<Duration, String> {
        self.wait_row(text, false)
    }

    /// A row below the last matched one that reads `text`, or with
    /// `prefix` begins with it.
    fn wait_row(&mut self, text: &str, prefix: bool) -> Result<Duration, String> {
        let start = Instant::now();
        let want = self.reader.canonical(text);
        let text = want.as_str();
        loop {
            // The screen's own timeout comes first, so that it says what
            // the screen shows.
            if Instant::now() <= self.deadline {
                self.pump()?;
            }
            let screen = self.qmp.screen()?;
            self.rescale(&screen);
            let rows = match self.origin {
                Some((x, y)) => {
                    let k = i64::from(screen.scale().factor());
                    screen.rows_at(&mut self.reader, (x * k, y * k))
                }
                None => screen.rows(&mut self.reader),
            };
            let from = self.row.map_or(0, |r| r + 1);
            let hit = |r: &String| {
                if prefix {
                    r.starts_with(text)
                } else {
                    r == text
                }
            };
            if let Some(at) = rows.iter().skip(from).position(hit) {
                self.row = Some(from + at);
                return Ok(start.elapsed());
            }
            if Instant::now() > self.deadline {
                return Err(format!(
                    "timeout waiting for the screen to show {text:?}; it shows:\n{}",
                    rows.join("\n")
                ));
            }
            std::thread::sleep(SCREEN_POLL);
        }
    }

    /// Reads at the screen's scale from now on.
    fn rescale(&mut self, screen: &Screen) {
        let scale = screen.scale();
        if scale != self.scale {
            self.scale = scale;
            self.reader = reader_at(scale);
        }
    }

    /// The display's size and scale, from a screendump.
    /// The time between two typed keys at the screen's scale.
    /// Between a modifier key and the pointer's event (and back): the
    /// keyboard and the tablet are two devices, read in no fixed order by
    /// a slow guest, so the key must have been seen before the click.
    fn modifier_settle(&self) -> Duration {
        self.key_interval().max(MODIFIER_SETTLE)
    }

    fn key_interval(&self) -> Duration {
        KEY_INTERVAL * self.scale.factor().pow(2)
    }

    fn display(&mut self) -> Result<((usize, usize), i64), String> {
        let screen = self.qmp.screen()?;
        self.rescale(&screen);
        Ok((
            (screen.width, screen.height),
            i64::from(screen.scale().factor()),
        ))
    }

    fn step(&mut self, step: &Step) -> Result<(), String> {
        // An input step: what the guest prints from now on is its answer.
        if matches!(
            step,
            Step::Type(_)
                | Step::Key(_)
                | Step::Chord(..)
                | Step::Click(..)
                | Step::DoubleClick(..)
                | Step::RightClick(..)
                | Step::ModClick(..)
                | Step::ModDrag(..)
                | Step::Drag(..)
        ) {
            self.pump()?;
            self.mark = self.seen.len();
        }
        match step {
            Step::Then(text) => loop {
                self.pump()?;
                if self
                    .seen
                    .get(self.mark..)
                    .is_some_and(|t| t.contains(text.as_str()))
                {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(20));
            },
            Step::Serial(text) => loop {
                self.pump()?;
                if self.seen.contains(text.as_str()) {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(20));
            },
            Step::Screen(text) => self.wait_screen(text).map(|_| ()),
            Step::Prefix(text) => self.wait_row(text, true).map(|_| ()),
            Step::Pause(ms) => {
                let until = Instant::now() + Duration::from_millis(*ms);
                while Instant::now() < until {
                    self.pump()?;
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok(())
            }
            Step::Type(keys) => {
                let layout = &zoraya_keymap::LAYOUTS[self.layout];
                let text = String::from_utf8_lossy(keys);
                for (i, c) in text.chars().enumerate() {
                    // Before the next eight, when the guest has drawn these.
                    if i > 0 && i % KEYS_PER_SETTLE == 0 {
                        self.settle()?;
                    }
                    // Under us, and for Enter, Tab and Space in any layout,
                    // the plain table; else the layout's keys.
                    let plain = qcode(c).filter(|_| {
                        self.layout == 0 || !c.is_alphanumeric() && !c.is_ascii_punctuation()
                    });
                    if let Some((code, shift)) = plain {
                        self.qmp.press(code, shift)?;
                        std::thread::sleep(self.key_interval());
                        continue;
                    }
                    let keys =
                        presses(layout, c).ok_or(format!("no key for {c:?} in {}", layout.name))?;
                    for (code, shift, altgr) in keys {
                        let q = qcode_of(code).ok_or(format!("no QEMU key for code {code}"))?;
                        let mut events = Vec::new();
                        if altgr {
                            events.push(("alt_r", true));
                        }
                        if shift {
                            events.push(("shift", true));
                        }
                        events.push((q, true));
                        events.push((q, false));
                        if shift {
                            events.push(("shift", false));
                        }
                        if altgr {
                            events.push(("alt_r", false));
                        }
                        self.qmp.keys(&events)?;
                        std::thread::sleep(self.key_interval());
                    }
                }
                Ok(())
            }
            Step::Layout(index) => {
                self.layout = *index;
                Ok(())
            }
            Step::Key(code) => self.qmp.press(code, false),
            Step::Chord(m, code) => {
                // Every modifier down, the key, then up in reverse.
                let mods: Vec<&str> = m.split('+').map(modifier_qcode).collect();
                let mut events: Vec<(&str, bool)> = mods.iter().map(|m| (*m, true)).collect();
                events.push((code.as_str(), true));
                events.push((code.as_str(), false));
                events.extend(mods.iter().rev().map(|m| (*m, false)));
                self.qmp.keys(&events)
            }
            // Waits for the colour, as `screen` waits for text: under TCG
            // the compositor may not have drawn yet what a pointer step did.
            Step::Region(x, y, w, h, colour) => loop {
                if Instant::now() <= self.deadline {
                    self.pump()?;
                }
                let screen = self.qmp.screen()?;
                let k = i64::from(screen.scale().factor());
                let (x, y, w, h) = (x * k, y * k, w * k, h * k);
                let mut wrong = None;
                'scan: for py in y..y + h {
                    for px in x..x + w {
                        let p = screen.at(px as usize, py as usize);
                        if p != *colour {
                            wrong = Some((px, py, p));
                            break 'scan;
                        }
                    }
                }
                let Some((px, py, p)) = wrong else {
                    return Ok(());
                };
                if Instant::now() > self.deadline {
                    return Err(format!("pixel {px},{py} is {p:06x}, not {colour:06x}"));
                }
                std::thread::sleep(SCREEN_POLL);
            },
            Step::Latency(n, bound) => {
                let mut times = Vec::new();
                let row = self.row;
                let prompt = self.qmp.screen()?.rows(&mut self.reader)[row.unwrap_or(0)].clone();
                let base = prompt.trim_end_matches('_').to_string();
                for i in 1..=*n {
                    self.row = row.map(|r| r.wrapping_sub(1));
                    self.qmp.press("x", false)?;
                    let want = format!("{base}{}_", "x".repeat(i));
                    times.push(self.wait_screen(&want)?);
                }
                for _ in 0..*n {
                    self.qmp.press("backspace", false)?;
                }
                self.row = row.map(|r| r.wrapping_sub(1));
                self.wait_screen(&prompt)?;
                times.sort();
                // What one screendump and its reading cost, with nothing
                // changing: the floor of every time above.
                let mut idle = Vec::new();
                for _ in 0..5 {
                    let start = Instant::now();
                    let _ = self.qmp.screen()?.rows(&mut self.reader);
                    idle.push(start.elapsed());
                }
                idle.sort();
                let ms = |d: &Duration| d.as_secs_f64() * 1000.0;
                say!(
                    "[{}] measure: key to screendump {n} keys: min {:.1} ms, median {:.1} ms, max {:.1} ms; one screendump read alone {:.1} ms",
                    self.label,
                    ms(&times[0]),
                    ms(&times[times.len() / 2]),
                    ms(&times[times.len() - 1]),
                    ms(&idle[2])
                );
                // A bound is on the median over the floor, so that the cost of
                // reading a screendump is not counted (a starved terminal
                // takes seconds, not tens of milliseconds more).
                if let Some(limit) = bound {
                    let over = ms(&times[times.len() / 2]) - ms(&idle[2]);
                    if over > f64::from(*limit) {
                        return Err(format!(
                            "latency: the median key took {over:.1} ms over one screendump read, more than {limit} ms"
                        ));
                    }
                }
                Ok(())
            }
            Step::Window => {
                self.row = None;
                Ok(())
            }
            Step::Top(text) => loop {
                if Instant::now() <= self.deadline {
                    self.pump()?;
                }
                let rows = self.qmp.screen()?.rows(&mut self.reader);
                if rows.first() == Some(&self.reader.canonical(text)) {
                    self.row = None;
                    return Ok(());
                }
                if Instant::now() > self.deadline {
                    return Err(format!(
                        "timeout waiting for the top row to read {text:?}; the screen shows:\n{}",
                        rows.join("\n")
                    ));
                }
                std::thread::sleep(SCREEN_POLL);
            },
            Step::Origin(x, y) => {
                self.origin = Some((*x, *y));
                self.row = None;
                Ok(())
            }
            Step::AtColour(click, colour, x, y, w, h) => loop {
                if Instant::now() <= self.deadline {
                    self.pump()?;
                }
                let screen = self.qmp.screen()?;
                self.rescale(&screen);
                let k = i64::from(screen.scale().factor());
                let size = (screen.width, screen.height);
                let found = (y * k..(y + h) * k)
                    .flat_map(|py| (x * k..(x + w) * k).map(move |px| (px, py)))
                    .find(|&(px, py)| {
                        px >= 0
                            && py >= 0
                            && (px as usize) < screen.width
                            && (py as usize) < screen.height
                            && screen.at(px as usize, py as usize) == *colour
                    });
                if let Some(at) = found {
                    self.qmp.pointer(at, size, None)?;
                    if *click {
                        self.qmp.pointer(at, size, Some(true))?;
                        self.qmp.pointer(at, size, Some(false))?;
                        std::thread::sleep(AFTER_CLICK);
                    }
                    return Ok(());
                }
                if Instant::now() > self.deadline {
                    return Err(format!(
                        "timeout waiting for {colour:06x} in {x} {y} {w} {h}"
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            },
            Step::Click(x, y) => {
                let (size, k) = self.display()?;
                let at = (x * k, y * k);
                self.qmp.pointer(at, size, None)?;
                self.qmp.pointer(at, size, Some(true))?;
                self.qmp.pointer(at, size, Some(false))?;
                // The keyboard and the pointer are two devices: a key typed
                // at once could be read before the click and go to the window
                // focused before it. A hand takes longer.
                std::thread::sleep(AFTER_CLICK);
                Ok(())
            }
            Step::DoubleClick(x, y) => {
                let (size, k) = self.display()?;
                let at = (x * k, y * k);
                self.qmp.pointer(at, size, None)?;
                // Four events of their own, at once: one batch is one frame
                // of the tablet's, which the compositor reads as its last
                // state.
                for down in [true, false, true, false] {
                    self.qmp.pointer(at, size, Some(down))?;
                }
                std::thread::sleep(AFTER_CLICK);
                Ok(())
            }
            Step::RightClick(x, y) => {
                let (size, k) = self.display()?;
                let at = (x * k, y * k);
                self.qmp.pointer(at, size, None)?;
                self.qmp.pointer_with(at, size, Some(("right", true)))?;
                self.qmp.pointer_with(at, size, Some(("right", false)))?;
                std::thread::sleep(AFTER_CLICK);
                Ok(())
            }
            Step::ModClick(m, x, y) => {
                let (size, k) = self.display()?;
                let at = (x * k, y * k);
                let code = modifier_qcode(m);
                self.qmp.pointer(at, size, None)?;
                self.qmp.keys(&[(code, true)])?;
                std::thread::sleep(self.modifier_settle());
                self.qmp.pointer(at, size, Some(true))?;
                self.qmp.pointer(at, size, Some(false))?;
                std::thread::sleep(self.modifier_settle());
                self.qmp.keys(&[(code, false)])?;
                std::thread::sleep(AFTER_CLICK);
                Ok(())
            }
            Step::ModDrag(m, x, y, x2, y2, hold) => {
                let (size, k) = self.display()?;
                let code = modifier_qcode(m);
                self.qmp.pointer((x * k, y * k), size, None)?;
                self.qmp.pointer((x * k, y * k), size, Some(true))?;
                std::thread::sleep(Duration::from_millis(*hold));
                const STEPS: i64 = 8;
                for i in 1..=STEPS {
                    let at = (
                        (x + (x2 - x) * i / STEPS) * k,
                        (y + (y2 - y) * i / STEPS) * k,
                    );
                    self.qmp.pointer(at, size, None)?;
                    std::thread::sleep(self.modifier_settle());
                }
                // The press is held until the program has answered it: a drag
                // is taken only while its press is held (ADR-0170).
                std::thread::sleep(DRAG_SETTLE);
                self.qmp.keys(&[(code, true)])?;
                std::thread::sleep(self.modifier_settle());
                self.qmp.pointer((x2 * k, y2 * k), size, Some(false))?;
                std::thread::sleep(self.modifier_settle());
                self.qmp.keys(&[(code, false)])
            }
            Step::Drag(x, y, x2, y2, hold) => {
                let (size, k) = self.display()?;
                self.qmp.pointer((x * k, y * k), size, None)?;
                self.qmp.pointer((x * k, y * k), size, Some(true))?;
                // A hand holds a press before it moves: long enough for the
                // program under it to answer the press (a drag's start).
                std::thread::sleep(Duration::from_millis(*hold));
                // In steps, as a hand moves: the compositor follows each.
                const STEPS: i64 = 8;
                for i in 1..=STEPS {
                    let at = (
                        (x + (x2 - x) * i / STEPS) * k,
                        (y + (y2 - y) * i / STEPS) * k,
                    );
                    self.qmp.pointer(at, size, None)?;
                    std::thread::sleep(self.key_interval());
                }
                std::thread::sleep(DRAG_SETTLE);
                self.qmp.pointer((x2 * k, y2 * k), size, Some(false))
            }
            Step::Shot(name) => {
                self.qmp.dump()?;
                std::fs::create_dir_all("build/shots").map_err(|e| e.to_string())?;
                // One file per run: gui-auto and gui-tcg keep their own.
                let to = format!("build/shots/{name}-{}.ppm", file_safe(&self.label));
                std::fs::copy(&self.qmp.shot, &to).map_err(|e| format!("{to}: {e}"))?;
                Ok(())
            }
            Step::Exit => loop {
                self.pump()?;
                match self.child.try_wait().map_err(|e| e.to_string())? {
                    Some(status) if status.code() == Some(0) => return Ok(()),
                    Some(status) => return Err(format!("QEMU ended with {status}")),
                    None => std::thread::sleep(Duration::from_millis(20)),
                }
            },
        }
    }
}

pub fn run(file: &str, label: &str, timeout: u64, qmp: &str, command: &[String]) -> ExitCode {
    let steps = match std::fs::read_to_string(file)
        .map_err(|e| e.to_string())
        .and_then(|text| parse(&text))
    {
        Ok(steps) => steps,
        Err(e) => {
            esay!("[{label}] {file}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let _ = std::fs::remove_file(qmp);
    let mut child = match Command::new(&command[0])
        .args(&command[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            esay!("[{label}] cannot start {}: {e}", command[0]);
            return ExitCode::FAILURE;
        }
    };
    let mut stdout = child.stdout.take().expect("piped");
    // Bounded: a guest that prints faster than it is read waits.
    let (tx, rx) = mpsc::sync_channel(1024);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(&mut stdout);
        let mut buf = Vec::new();
        while let Some(read) = output::read_line(&mut reader, &mut buf) {
            let line = String::from_utf8_lossy(&buf).trim_end().to_string();
            if tx.send((line, read)).is_err() {
                break;
            }
        }
        // Drained, not kept, so that QEMU never blocks on a full pipe.
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
    });
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let result = Qmp::connect(qmp, deadline).and_then(|mut q| {
        q.shot = shot_path(label);
        let mut run = Run {
            child,
            qmp: q,
            serial: rx,
            seen: String::new(),
            mark: 0,
            out: output::Output::new(label),
            label: label.to_string(),
            deadline,
            row: None,
            origin: None,
            reader: reader(),
            scale: Scale::ONE,
            layout: 0,
        };
        // A failure names its step, so that a script of a thousand says where.
        let r = steps.iter().enumerate().try_for_each(|(i, s)| {
            run.step(s)
                .map_err(|e| format!("step {} of {} ({s:?}): {e}", i + 1, steps.len()))
        });
        // The last drain fails the run on a flood or forbidden text, not on
        // a deadline the steps already met.
        let r = r.and(match run.pump() {
            Err(e) if e == "timeout" => Ok(()),
            other => other,
        });
        let _ = run.child.kill();
        let _ = run.child.wait();
        run.out.finish();
        r
    });
    let _ = std::fs::remove_file(qmp);
    match result {
        Ok(()) => {
            say!("[{label}] PASS: {} steps", steps.len());
            ExitCode::SUCCESS
        }
        Err(e) => {
            say!("[{label}] FAIL: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_parses() {
        let steps =
            parse("# x\nserial init\nscreen zosh$ _\ntype ls\\r\nkey f12\nregion 0 0 2 2 203040\nlatency 3\nexit\n")
                .unwrap();
        assert_eq!(steps.len(), 7);
        assert_eq!(steps[2], Step::Type(b"ls\r".to_vec()));
        assert_eq!(steps[4], Step::Region(0, 0, 2, 2, 0x20_3040));
        assert!(parse("region 1 2 3\n").is_err());
        assert!(parse("dance\n").is_err());
        assert_eq!(
            parse("chord ctrl c\n").unwrap(),
            [Step::Chord("ctrl".into(), "c".into())]
        );
        assert!(parse("chord ctrl\n").is_err());
        assert!(parse("chord hyper c\n").is_err());
        assert_eq!(
            parse("shot term-1\n").unwrap(),
            [Step::Shot("term-1".into())]
        );
        assert!(parse("shot ../x\n").is_err());
        assert!(parse("shot\n").is_err());
        assert_eq!(parse("click 3 4\n").unwrap(), [Step::Click(3, 4)]);
        assert_eq!(
            parse("then files: x\n").unwrap(),
            [Step::Then(String::from("files: x"))]
        );
        assert_eq!(parse("dclick 3 4\n").unwrap(), [Step::DoubleClick(3, 4)]);
        assert_eq!(parse("rclick 3 4\n").unwrap(), [Step::RightClick(3, 4)]);
        assert_eq!(
            parse("mclick meta 3 4\n").unwrap(),
            [Step::ModClick(String::from("meta"), 3, 4)]
        );
        assert_eq!(
            parse("mdrag alt 1 2 3 4 300\n").unwrap(),
            [Step::ModDrag(String::from("alt"), 1, 2, 3, 4, 300)]
        );
        assert!(parse("mclick hyper 3 4\n").is_err());
        assert_eq!(
            parse("chord meta+shift n\n").unwrap(),
            [Step::Chord(String::from("meta+shift"), String::from("n"))]
        );
        assert!(parse("chord meta+hyper n\n").is_err());
        assert!(parse("dclick 3\n").is_err());
        assert_eq!(parse("layout cz\n").unwrap(), [Step::Layout(3)]);
        assert!(parse("layout de\n").is_err());
        assert!(parse("chord meta spc\n").is_ok());
        assert_eq!(
            parse("drag 1 2 3 4\n").unwrap(),
            [Step::Drag(1, 2, 3, 4, 0)]
        );
        assert_eq!(
            parse("drag 1 2 3 4 500\n").unwrap(),
            [Step::Drag(1, 2, 3, 4, 500)]
        );
        assert!(parse("drag 1 2 3 4 -5\n").is_err());
        assert!(parse("click 3\n").is_err());
        assert!(parse("drag 1 2 3 x\n").is_err());
    }

    #[test]
    fn a_ppm_parses_and_a_bad_one_does_not() {
        let mut ppm = b"P6\n# c\n2 1\n255\n".to_vec();
        ppm.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        let s = parse_ppm(&ppm).unwrap();
        assert_eq!((s.width, s.height), (2, 1));
        assert_eq!(s.pixels, [0x01_0203, 0x04_0506]);
        assert!(parse_ppm(b"P5\n1 1\n255\n\0").is_err());
        assert!(parse_ppm(b"P6\n2 2\n255\n\0\0\0").is_err());
        assert!(parse_ppm(b"P6\n").is_err());
    }

    /// The screen reader against the real terminal's rendering, placed as
    /// the compositor places the first window.
    #[test]
    fn the_terminal_reads_back_as_text() {
        let (w, h) = (1024usize, 768usize);
        let mut look = Look::new(Scale::ONE, Appearance::Light);
        let (columns, rows) = look.grid_for(w as u32, h as u32);
        let mut term = zoraya_term::Term::new(columns, rows);
        term.write("zosh$ cat a.txt\nhi звіт-2026.txt příklad.txt\nzosh$ ".as_bytes());
        let (sw, sh) = look.surface_size(columns, rows);
        let mut surface = vec![0u32; sw * sh];
        look.clear(&mut surface);
        term.render(&mut look, &mut surface, sw, true);
        let mut pixels = vec![0x0020_3040u32; w * h];
        let (ox, oy) = zoraya_compositor::first_body(Scale::ONE);
        for y in 0..sh {
            for x in 0..sw {
                pixels[(oy as usize + y) * w + ox as usize + x] = surface[y * sw + x];
            }
        }
        let screen = Screen {
            width: w,
            height: h,
            pixels,
        };
        let mut reader = reader();
        let text = screen.rows(&mut reader);
        assert_eq!(text[0], "zosh$ cat a.txt");
        assert_eq!(text[1], reader.canonical("hi звіт-2026.txt příklad.txt"));
        assert_eq!(text[2], "zosh$ _");
        assert!(text[3..].iter().all(String::is_empty));
    }

    #[test]
    fn letters_of_every_layout_have_keys_and_dead_keys_reach_the_rest() {
        let cz = &zoraya_keymap::LAYOUTS[3];
        assert_eq!(presses(cz, 'ř'), Some(vec![(6, false, false)]));
        assert_eq!(presses(cz, '1'), Some(vec![(2, true, false)]));
        // ď has no key: the dead caron (Shift with =) and d.
        assert_eq!(
            presses(cz, 'ď'),
            Some(vec![(13, true, false), (32, false, false)])
        );
        assert_eq!(
            presses(cz, 'ó'),
            Some(vec![(13, false, false), (24, false, false)])
        );
        assert_eq!(presses(cz, '€'), Some(vec![(18, false, true)]));
        let ua = &zoraya_keymap::LAYOUTS[1];
        for c in "звіт-ґанок".chars() {
            let p = presses(ua, c).unwrap_or_else(|| panic!("{c}"));
            assert!(p.iter().all(|(code, _, _)| qcode_of(*code).is_some()));
        }
        assert_eq!(presses(ua, 'Ж'), Some(vec![(39, true, false)]));
        assert!(presses(ua, 'q').is_none());
        assert_eq!(qcode_of(13), Some("equal"));
        assert_eq!(qcode_of(200), None);
    }

    #[test]
    fn every_typed_character_has_a_key() {
        for c in "write a.txt hi\rcat ls poweroff ABC-_:/.".chars() {
            assert!(qcode(c).is_some(), "{c:?}");
        }
        assert_eq!(qcode('A'), Some(("a", true)));
        assert_eq!(qcode('\u{1}'), None);
    }
}
