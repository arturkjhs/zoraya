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
//! screen TEXT       wait until a row below the last matched one reads TEXT
//! prefix TEXT       the same for a row that begins with TEXT (a line being
//!                   edited, whose faint suggestion follows what is typed)
//! pause MS          wait MS milliseconds, reading the serial line
//! type KEYS         press and release each key (escapes \r \e \\)
//! key QCODE         press and release one QEMU key code, such as f12
//! chord MOD QCODE   the same with MOD (ctrl, shift, alt) held, such as
//!                   chord ctrl c
//! region X Y W H RGB  every pixel of the rectangle is the colour RGB (hex)
//! click X Y         move the pointer to (X, Y) and press and release the
//!                   left button
//! drag X Y X2 Y2    press the left button at (X, Y), move to (X2, Y2) in
//!                   steps, release
//!
//! Coordinates are logical pixels: the runner multiplies them by the
//! display's scale (ADR-0164), which it takes from the screendump's width
//! as the compositor does (`zoraya_compositor::scale_for`), so one script
//! runs at both scales. The terminal is read at the first window's body
//! (`zoraya_compositor::first_body`).
//! window            a new window: the next `screen` looks from its top row
//! top TEXT          wait until the top row reads TEXT (a cleared screen),
//!                   then a new window as `window`
//! latency N         type `x` N times at the prompt, timing each until it
//!                   shows, then erase them
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
/// TASK-24 7a). At this rate the queue holds 128 ms of typing.
const KEY_INTERVAL: Duration = Duration::from_millis(8);
/// The longest wait for one QMP reply: QEMU answers in milliseconds, so a
/// silent QEMU fails the run instead of blocking it past its deadline.
const QMP_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Serial(String),
    Screen(String),
    Prefix(String),
    Pause(u64),
    Type(Vec<u8>),
    Key(String),
    Chord(String, String),
    Region(i64, i64, i64, i64, u32),
    Latency(usize),
    Click(i64, i64),
    Drag(i64, i64, i64, i64),
    Window,
    Top(String),
    Shot(String),
    Exit,
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
                if !matches!(m, "ctrl" | "shift" | "alt") {
                    return Err(bad("chord's modifier is ctrl, shift or alt"));
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
            "drag" => {
                let v: Vec<i64> = rest
                    .split_whitespace()
                    .map(|n| n.parse().map_err(|_| bad("not a number")))
                    .collect::<Result<_, _>>()?;
                let [x, y, x2, y2] = v[..] else {
                    return Err(bad("drag X Y X2 Y2"));
                };
                Step::Drag(x, y, x2, y2)
            }
            "latency" => Step::Latency(rest.parse().map_err(|_| bad("not a count"))?),
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
        zoraya_compositor::scale_for(self.width as u32)
    }

    /// The terminal's rows as text: the first window's body, where the
    /// compositor puts it, with the terminal's padding and grid.
    pub fn rows(&self, reader: &mut Reader) -> Vec<String> {
        let (columns, rows) = reader
            .look()
            .grid_for(self.width as u32, self.height as u32);
        let (ox, oy) = zoraya_compositor::first_body(self.scale());
        reader.rows(
            columns,
            rows,
            (ox.max(0) as usize, oy.max(0) as usize),
            (self.width, self.height),
            |x, y| self.at(x, y),
        )
    }
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
        (x, y): (i64, i64),
        (width, height): (usize, usize),
        button: Option<bool>,
    ) -> Result<(), String> {
        let axis = |v: i64, side: usize| {
            (v.clamp(0, side as i64 - 1) * 32767 / (side as i64 - 1).max(1)) as u32
        };
        let mut events = format!(
            r#"{{"type":"abs","data":{{"axis":"x","value":{}}}}},{{"type":"abs","data":{{"axis":"y","value":{}}}}}"#,
            axis(x, width),
            axis(y, height)
        );
        if let Some(down) = button {
            events.push_str(&format!(
                r#",{{"type":"btn","data":{{"down":{down},"button":"left"}}}}"#
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
    out: output::Output,
    label: String,
    deadline: Instant,
    /// The row of the last `screen` match.
    row: Option<usize>,
    reader: Reader,
    /// The scale `reader` reads at.
    scale: Scale,
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
            let rows = screen.rows(&mut self.reader);
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
            std::thread::sleep(Duration::from_millis(5));
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
    fn display(&mut self) -> Result<((usize, usize), i64), String> {
        let screen = self.qmp.screen()?;
        self.rescale(&screen);
        Ok((
            (screen.width, screen.height),
            i64::from(screen.scale().factor()),
        ))
    }

    fn step(&mut self, step: &Step) -> Result<(), String> {
        match step {
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
                for c in String::from_utf8_lossy(keys).chars() {
                    let (code, shift) = qcode(c).ok_or(format!("no key for {c:?}"))?;
                    self.qmp.press(code, shift)?;
                    std::thread::sleep(KEY_INTERVAL);
                }
                Ok(())
            }
            Step::Key(code) => self.qmp.press(code, false),
            Step::Chord(m, code) => self.qmp.keys(&[
                (m.as_str(), true),
                (code.as_str(), true),
                (code.as_str(), false),
                (m.as_str(), false),
            ]),
            Step::Region(x, y, w, h, colour) => {
                let screen = self.qmp.screen()?;
                let k = i64::from(screen.scale().factor());
                let (x, y, w, h) = (x * k, y * k, w * k, h * k);
                for py in y..y + h {
                    for px in x..x + w {
                        let p = screen.at(px as usize, py as usize);
                        if p != *colour {
                            return Err(format!("pixel {px},{py} is {p:06x}, not {colour:06x}"));
                        }
                    }
                }
                Ok(())
            }
            Step::Latency(n) => {
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
                std::thread::sleep(Duration::from_millis(5));
            },
            Step::Click(x, y) => {
                let (size, k) = self.display()?;
                let at = (x * k, y * k);
                self.qmp.pointer(at, size, None)?;
                self.qmp.pointer(at, size, Some(true))?;
                self.qmp.pointer(at, size, Some(false))
            }
            Step::Drag(x, y, x2, y2) => {
                let (size, k) = self.display()?;
                self.qmp.pointer((x * k, y * k), size, None)?;
                self.qmp.pointer((x * k, y * k), size, Some(true))?;
                // In steps, as a hand moves: the compositor follows each.
                const STEPS: i64 = 8;
                for i in 1..=STEPS {
                    let at = (
                        (x + (x2 - x) * i / STEPS) * k,
                        (y + (y2 - y) * i / STEPS) * k,
                    );
                    self.qmp.pointer(at, size, None)?;
                    std::thread::sleep(KEY_INTERVAL);
                }
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
            out: output::Output::new(label),
            label: label.to_string(),
            deadline,
            row: None,
            reader: reader(),
            scale: Scale::ONE,
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
        assert!(parse("chord meta c\n").is_err());
        assert_eq!(
            parse("shot term-1\n").unwrap(),
            [Step::Shot("term-1".into())]
        );
        assert!(parse("shot ../x\n").is_err());
        assert!(parse("shot\n").is_err());
        assert_eq!(parse("click 3 4\n").unwrap(), [Step::Click(3, 4)]);
        assert_eq!(parse("drag 1 2 3 4\n").unwrap(), [Step::Drag(1, 2, 3, 4)]);
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
    fn every_typed_character_has_a_key() {
        for c in "write a.txt hi\rcat ls poweroff ABC-_:/.".chars() {
            assert!(qcode(c).is_some(), "{c:?}");
        }
        assert_eq!(qcode('A'), Some(("a", true)));
        assert_eq!(qcode('\u{1}'), None);
    }
}
