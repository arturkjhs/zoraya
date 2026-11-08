#![no_std]
#![no_main]

//! `requests NAME`: says each interrupt and quit request it takes, on its
//! `err`, and never ends on them; `requests --read NAME` reads its input to
//! the end and says how much came. For the job-control tests (ADR-0158,
//! INV-0158-1 and INV-0158-2).

use zoraya_abi::{Disposition, RequestKind};
use zoraya_user::{
    clock_monotonic, command, eprintln, input_read, process_set_disposition, process_take_request,
    sleep_until,
};

zoraya_user::describe!("tests", "requests");

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = command::start(DESCRIPTION);
    let p = c.parse();
    let name = p.operand(&c.d, "NAME").unwrap_or("");
    if p.flag(&c.d, "read") {
        let mut total = 0usize;
        let mut buffer = [0u8; 256];
        while let Ok(n @ 1..) = input_read(&mut buffer) {
            total += n;
        }
        eprintln!("{name}: end of input after {total} bytes");
        return 0;
    }
    for kind in [RequestKind::Interrupt, RequestKind::Quit] {
        process_set_disposition(kind, Disposition::Deliver).expect("disposition");
    }
    eprintln!("{name}: ready");
    let (mut interrupts, mut quits) = (0u32, 0u32);
    loop {
        while let Ok((kind, _)) = process_take_request() {
            match kind {
                RequestKind::Interrupt => {
                    interrupts += 1;
                    eprintln!("{name}: interrupt {interrupts}");
                }
                RequestKind::Quit => {
                    quits += 1;
                    eprintln!("{name}: quit {quits}");
                }
                _ => {}
            }
        }
        sleep_until(clock_monotonic() + 10_000_000);
    }
}
