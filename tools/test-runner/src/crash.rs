//! Kills QEMU at random points while the guest writes, then checks the disk.
//!
//! Each round: a fresh image, boot, `crashwriter`, SIGKILL after a random
//! time, `zoraya-fsck`, boot, `crashcheck N` with the last durable file seen
//! on the serial line, power off, `zoraya-fsck`. One more round checks that a
//! write nobody syncs is on the disk after `T_COMMIT` (ADR-0010 rule 5).
//!
//! The limit of this test: killing QEMU keeps what it already gave the host,
//! so it stops a writer at a random point in its sequence of requests and does
//! not lose or reorder writes that were not flushed. Host tests of `zoraya-fs`
//! do that, at every step, against a simulated device.

#[allow(dead_code)] // Reason: the crash test uses only the buffer bounds of the module.
#[path = "output.rs"]
mod output;

use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, ExitCode, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RUN_QEMU: &str = "tools/run-qemu.sh";
const PROMPT: &str = "zosh$ ";
const WAIT: Duration = Duration::from_secs(60);
/// `T_COMMIT` and one commit, with room.
const COMMITTED: Duration = Duration::from_secs(8);

struct Guest {
    child: Child,
    input: ChildStdin,
    bytes: Receiver<Vec<u8>>,
    text: String,
    seen: usize,
}

impl Guest {
    fn boot(kernel: &str, image: &str) -> Result<Guest, String> {
        let mut child = Command::new(RUN_QEMU)
            .args([
                "--test", "--keys", "--accel", "auto", "--disk", image, kernel,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("{RUN_QEMU}: {e}"))?;
        let input = child.stdin.take().ok_or("no stdin")?;
        let mut output = child.stdout.take().ok_or("no stdout")?;
        // Bounded: a guest that prints faster than it is read waits.
        let (send, bytes) = mpsc::sync_channel(256);
        std::thread::spawn(move || {
            let mut buffer = [0; 4096];
            while let Ok(n) = output.read(&mut buffer) {
                if n == 0 || send.send(buffer[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Ok(Guest {
            child,
            input,
            bytes,
            text: String::new(),
            seen: 0,
        })
    }

    /// Takes what arrives before `until`: false if nothing did; an error
    /// past `output::LOG_CAP` of output.
    fn pump(&mut self, until: Instant) -> Result<bool, String> {
        let left = until.saturating_duration_since(Instant::now());
        match self.bytes.recv_timeout(left) {
            Ok(bytes) => {
                output::keep(&mut self.text, &String::from_utf8_lossy(&bytes))?;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    fn expect(&mut self, wanted: &str) -> Result<(), String> {
        let until = Instant::now() + WAIT;
        loop {
            if self.text.contains("panicked") || self.text.contains("storage failed") {
                return Err(format!(
                    "the guest failed:\n{}",
                    output::excerpt(&self.text)
                ));
            }
            if let Some(at) = self.text[self.seen..].find(wanted) {
                self.seen += at + wanted.len();
                return Ok(());
            }
            if !self.pump(until)? {
                return Err(format!(
                    "no {wanted:?} in:\n{}",
                    output::excerpt(&self.text)
                ));
            }
        }
    }

    fn send(&mut self, line: &str) -> Result<(), String> {
        let keys = format!("{line}\r");
        self.input
            .write_all(keys.as_bytes())
            .and_then(|()| self.input.flush())
            .map_err(|e| e.to_string())
    }

    /// SIGKILL, and everything it printed before.
    fn kill(mut self) -> Result<String, String> {
        self.child.kill().map_err(|e| e.to_string())?;
        self.child.wait().map_err(|e| e.to_string())?;
        while let Ok(bytes) = self.bytes.recv_timeout(Duration::from_secs(2)) {
            output::keep(&mut self.text, &String::from_utf8_lossy(&bytes))?;
        }
        Ok(std::mem::take(&mut self.text))
    }

    fn power_off(mut self) -> Result<(), String> {
        self.send("poweroff")?;
        let status = self.child.wait().map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("QEMU ended with {status}"))
        }
    }
}

/// A round that fails returns early: its QEMU must not outlive it.
impl Drop for Guest {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn run(tool: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(tool)
        .args(args)
        .output()
        .map_err(|e| format!("{tool}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{tool} {args:?} failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn fresh(image: &str) -> Result<(), String> {
    let mkfs = "target/debug/zoraya-mkfs";
    run(mkfs, &["--size", "256M", "--from", "rootfs", image])
}

fn fsck(image: &str) -> Result<(), String> {
    run("target/debug/zoraya-fsck", &[image])
}

/// The last whole line that says what is durable.
fn last_durable(text: &str) -> i64 {
    let whole = &text[..text.rfind('\n').unwrap_or(0)];
    whole
        .lines()
        .filter_map(|line| line.trim().strip_prefix("crash: durable "))
        .filter_map(|n| n.parse().ok())
        .next_back()
        .unwrap_or(-1)
}

struct Totals {
    rounds: u32,
    /// Rounds with files past the last line, so the kill was mid-work.
    past_durable: u32,
    /// Files the guest had reported durable, over all rounds.
    durable_files: i64,
}

fn round(kernel: &str, image: &str, delay: Duration, totals: &mut Totals) -> Result<(), String> {
    fresh(image)?;
    let mut guest = Guest::boot(kernel, image)?;
    guest.expect(PROMPT)?;
    // Often, so that kills land in commits; rarely, so that the engine's own
    // commits leave files past the last sync, which must still be a prefix.
    let every = if totals.rounds.is_multiple_of(2) {
        8
    } else {
        512
    };
    guest.send(&format!("crashwriter {every}"))?;
    guest.expect("crash: start")?;
    std::thread::sleep(delay);
    let text = guest.kill()?;
    for bad in ["crash: open", "crash: write", "crash: sync", "panicked"] {
        if text.contains(bad) {
            return Err(format!("the writer failed:\n{}", output::excerpt(&text)));
        }
    }
    let durable = last_durable(&text);
    fsck(image)?;
    let mut guest = Guest::boot(kernel, image)?;
    guest.expect(PROMPT)?;
    guest.send(&format!("crashcheck {durable}"))?;
    guest.expect("crash: ok ")?;
    guest.expect(" files")?;
    let found: i64 = guest.text[..guest.seen - " files".len()]
        .rsplit(' ')
        .next()
        .and_then(|n| n.parse().ok())
        .ok_or("no count")?;
    guest.expect(PROMPT)?;
    guest.power_off()?;
    fsck(image)?;
    totals.rounds += 1;
    totals.durable_files += durable + 1;
    if found > durable + 1 {
        totals.past_durable += 1;
    }
    println!(
        "round {}: killed after {} ms, {} durable, {found} found",
        totals.rounds,
        delay.as_millis(),
        durable + 1
    );
    Ok(())
}

/// ADR-0010 rule 5: nobody syncs, and it is on the disk after the interval.
fn without_sync(kernel: &str, image: &str) -> Result<(), String> {
    fresh(image)?;
    let mut guest = Guest::boot(kernel, image)?;
    guest.expect(PROMPT)?;
    guest.send("write unsynced.txt nobody asked")?;
    guest.expect(PROMPT)?;
    std::thread::sleep(COMMITTED);
    guest.kill()?;
    fsck(image)?;
    let mut guest = Guest::boot(kernel, image)?;
    guest.expect(PROMPT)?;
    guest.send("cat unsynced.txt")?;
    guest.expect("\nnobody asked")?;
    guest.expect(PROMPT)?;
    guest.power_off()?;
    fsck(image)?;
    println!("no sync: durable after {} s", COMMITTED.as_secs());
    Ok(())
}

/// xorshift64: the delays only need to be spread, and to repeat from a seed.
fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn parse() -> Result<(u32, u64, String, String), String> {
    let (mut rounds, mut kernel, mut image) = (10, None, None);
    let clock = SystemTime::now().duration_since(UNIX_EPOCH);
    let mut seed = clock.map_err(|e| e.to_string())?.as_nanos() as u64 | 1;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next().ok_or("a value is missing")?;
        match arg.as_str() {
            "--rounds" => rounds = value.parse().map_err(|_| "--rounds")?,
            "--seed" => seed = value.parse().map_err(|_| "--seed")?,
            "--kernel" => kernel = Some(value),
            "--image" => image = Some(value),
            _ => return Err(format!("unknown {arg}")),
        }
    }
    let kernel = kernel.ok_or("--kernel")?;
    Ok((rounds, seed, kernel, image.ok_or("--image")?))
}

fn all() -> Result<(), String> {
    let (rounds, seed, kernel, image) = parse()?;
    println!("seed {seed}");
    let mut state = seed;
    let mut totals = Totals {
        rounds: 0,
        past_durable: 0,
        durable_files: 0,
    };
    without_sync(&kernel, &image)?;
    for _ in 0..rounds {
        let delay = Duration::from_millis(next(&mut state) % 2000);
        round(&kernel, &image, delay, &mut totals)?;
    }
    println!(
        "crash test: {} rounds, all clean; {} durable files all intact; {} rounds with files past the last sync",
        totals.rounds, totals.durable_files, totals.past_durable
    );
    Ok(())
}

fn main() -> ExitCode {
    match all() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zoraya-crash-test: {e}");
            ExitCode::FAILURE
        }
    }
}
