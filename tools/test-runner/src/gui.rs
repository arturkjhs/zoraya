//! Plays a GUI script against a kernel booted with `--gui --qmp` (TASK-14,
//! ADR-0097): keys go in through QMP, and the screen comes back as QEMU
//! screendumps, which are read as text.
//!
//! **How the screen is read.** The compositor puts the first window at a
//! fixed place, and the terminal draws every character with `zoraya-font`,
//! whose pixels are each either the text colour or the background. A
//! screendump is lossless (PPM), so each cell of the terminal's grid is
//! matched against every glyph exactly: the tolerance is zero, and a cell
//! that matches no glyph reads as `?`. The block cursor, a cell of one
//! colour, reads as `_`. Regions outside the window are checked pixel by
//! pixel against a stated colour.
//!
//! A script is lines, run in order:
//!
//! ```text
//! # a comment
//! serial TEXT       wait until TEXT appears on the serial line
//! screen TEXT       wait until a row below the last matched one reads TEXT
//! type KEYS         press and release each key (escapes \r \e \\)
//! key QCODE         press and release one QEMU key code, such as f12
//! region X Y W H RGB  every pixel of the rectangle is the colour RGB (hex)
//! latency N         type `x` N times at the prompt, timing each until it
//!                   shows, then erase them
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
use zoraya_font::{CELL_HEIGHT, CELL_WIDTH, recognise};
use zoraya_term::{PAD, PALETTE, grid_for};

const FORBIDDEN: [&str; 3] = ["ZORAYA-PANIC", "ZORAYA-FAULT", "panicked"];
/// Where the screendumps go, relative to QEMU's working directory.
const SHOT: &str = "build/gui-screen.ppm";

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Serial(String),
    Screen(String),
    Type(Vec<u8>),
    Key(String),
    Region(i64, i64, i64, i64, u32),
    Latency(usize),
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
            "type" => Step::Type(crate::script::unescape(rest).map_err(|e| bad(&e))?),
            "key" => Step::Key(rest.to_string()),
            "region" => {
                let v: Vec<&str> = rest.split_whitespace().collect();
                if v.len() != 5 {
                    return Err(bad("region X Y W H RGB"));
                }
                let num = |s: &str| s.parse::<i64>().map_err(|_| bad("not a number"));
                let colour = u32::from_str_radix(v[4], 16).map_err(|_| bad("not a colour"))?;
                Step::Region(num(v[0])?, num(v[1])?, num(v[2])?, num(v[3])?, colour)
            }
            "latency" => Step::Latency(rest.parse().map_err(|_| bad("not a count"))?),
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

    /// The terminal's rows as text: the first window, at the compositor's
    /// origin, with the terminal's padding and grid.
    pub fn rows(&self) -> Vec<String> {
        let (columns, rows) = grid_for(self.width as u32, self.height as u32);
        let (ox, oy) = zoraya_compositor::ORIGIN;
        let (left, top) = (ox as usize + PAD, oy as usize + PAD);
        let bg = PALETTE[usize::from(zoraya_term::DEFAULT_BG)];
        (0..rows)
            .map(|r| {
                let line: String = (0..columns)
                    .map(|c| {
                        let (x, y) = (left + c * CELL_WIDTH, top + r * CELL_HEIGHT);
                        if x + CELL_WIDTH > self.width || y + CELL_HEIGHT > self.height {
                            return '?';
                        }
                        let pixel = |dx: usize, dy: usize| self.at(x + dx, y + dy);
                        match recognise(pixel, bg) {
                            Some(c) => c,
                            None if solid(&pixel) => '_',
                            None => '?',
                        }
                    })
                    .collect();
                line.trim_end().to_string()
            })
            .collect()
    }
}

fn solid(pixel: &dyn Fn(usize, usize) -> u32) -> bool {
    let first = pixel(0, 0);
    (0..CELL_HEIGHT).all(|y| (0..CELL_WIDTH).all(|x| pixel(x, y) == first))
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
        _ => return None,
    })
}

struct Qmp {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
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
        let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        let mut q = Qmp { stream, reader };
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
                .map_err(|e| e.to_string())?
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

    fn screen(&mut self) -> Result<Screen, String> {
        let command = format!(r#"{{"execute":"screendump","arguments":{{"filename":"{SHOT}"}}}}"#);
        self.execute(&command)?;
        let bytes = std::fs::read(SHOT).map_err(|e| format!("{SHOT}: {e}"))?;
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
        let start = Instant::now();
        loop {
            self.pump()?;
            let rows = self.qmp.screen()?.rows();
            let from = self.row.map_or(0, |r| r + 1);
            if let Some(at) = rows.iter().skip(from).position(|r| r == text) {
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
            Step::Type(keys) => {
                for c in String::from_utf8_lossy(keys).chars() {
                    let (code, shift) = qcode(c).ok_or(format!("no key for {c:?}"))?;
                    self.qmp.press(code, shift)?;
                }
                Ok(())
            }
            Step::Key(code) => self.qmp.press(code, false),
            Step::Region(x, y, w, h, colour) => {
                let screen = self.qmp.screen()?;
                for py in *y..y + h {
                    for px in *x..x + w {
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
                let prompt = self.qmp.screen()?.rows()[row.unwrap_or(0)].clone();
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
                    let _ = self.qmp.screen()?.rows();
                    idle.push(start.elapsed());
                }
                idle.sort();
                let ms = |d: &Duration| d.as_secs_f64() * 1000.0;
                println!(
                    "[{}] measure: key to screendump {n} keys: min {:.1} ms, median {:.1} ms, max {:.1} ms; one screendump read alone {:.1} ms",
                    self.label,
                    ms(&times[0]),
                    ms(&times[times.len() / 2]),
                    ms(&times[times.len() - 1]),
                    ms(&idle[2])
                );
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
            eprintln!("[{label}] {file}: {e}");
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
            eprintln!("[{label}] cannot start {}: {e}", command[0]);
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
    let result = Qmp::connect(qmp, deadline).and_then(|q| {
        let mut run = Run {
            child,
            qmp: q,
            serial: rx,
            seen: String::new(),
            out: output::Output::new(label),
            label: label.to_string(),
            deadline,
            row: None,
        };
        let r = steps.iter().try_for_each(|s| run.step(s));
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
            println!("[{label}] PASS: {} steps", steps.len());
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("[{label}] FAIL: {e}");
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
        let (columns, rows) = grid_for(w as u32, h as u32);
        let mut term = zoraya_term::Term::new(columns, rows);
        term.write(b"zosh$ cat a.txt\nhi\nzosh$ ");
        let (sw, sh) = zoraya_term::surface_size(columns, rows);
        let mut surface = vec![0u32; sw * sh];
        term.render(&mut surface, sw, true);
        let mut pixels = vec![0x0020_3040u32; w * h];
        let (ox, oy) = zoraya_compositor::ORIGIN;
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
        let text = screen.rows();
        assert_eq!(text[0], "zosh$ cat a.txt");
        assert_eq!(text[1], "hi");
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
