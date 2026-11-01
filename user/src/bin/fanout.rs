#![no_std]
#![no_main]

//! `fanout <n> [tick [leave]]`: starts `n` children in this job, spinning or
//! ticking, and sleeps, or with `leave` exits and leaves them. One request to the job must end them all (ADR-0004 rule 3).

use zoraya_abi::spawn::Encoder;
use zoraya_abi::{DEADLINE_INFINITE, JOB};
use zoraya_user::{console_entry, handle_close, image, println, sleep_until, spawn, startup};

#[unsafe(no_mangle)]
fn main() -> i32 {
    let mut argv = startup().argv;
    let count: Option<u32> = argv
        .next()
        .and_then(|a| core::str::from_utf8(a).ok())
        .and_then(|a| a.parse().ok());
    let Some(count) = count else {
        println!("usage: fanout <n> [tick]");
        return 2;
    };
    let ticking = argv.next() == Some(b"tick");
    let program = image(if ticking { "ticker" } else { "spin" });
    for _ in 0..count {
        let mut buffer = [0; 256];
        let mut e = Encoder::new(&mut buffer, JOB, program);
        if ticking {
            e.handle(console_entry());
        }
        let length = e.finish().expect("the buffer is large enough");
        let child = spawn(&buffer[..length]).expect("spawn");
        handle_close(child).expect("close");
    }
    println!("fanout: {count} children spinning");
    if argv.next() == Some(b"leave") {
        return 0;
    }
    sleep_until(DEADLINE_INFINITE);
    0
}
