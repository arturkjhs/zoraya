#![no_std]
#![no_main]

//! `fanout <n>`: starts `n` spinning children in this job, and sleeps. One
//! request to the job must end them all (ADR-0004 rule 3).

use zoraya_abi::spawn::Encoder;
use zoraya_abi::{DEADLINE_INFINITE, JOB};
use zoraya_user::{handle_close, image, println, sleep_until, spawn, startup};

#[unsafe(no_mangle)]
fn main() -> i32 {
    let count: Option<u32> = startup()
        .argv
        .next()
        .and_then(|a| core::str::from_utf8(a).ok())
        .and_then(|a| a.parse().ok());
    let Some(count) = count else {
        println!("usage: fanout <n>");
        return 2;
    };
    let spin = image("spin");
    for _ in 0..count {
        let mut buffer = [0; 256];
        let length = Encoder::new(&mut buffer, JOB, spin)
            .finish()
            .expect("the buffer is large enough");
        let child = spawn(&buffer[..length]).expect("spawn");
        handle_close(child).expect("close");
    }
    println!("fanout: {count} children spinning");
    sleep_until(DEADLINE_INFINITE);
    0
}
