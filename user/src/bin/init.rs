#![no_std]
#![no_main]

//! The one process the kernel starts (ADR-0053). Its table starts the shell,
//! in a sub-job, with only the handles the shell needs. When the shell ends,
//! it turns the machine off.

use zoraya_abi::spawn::Encoder;
use zoraya_abi::{
    CONSOLE, ConsoleObject, DIRECTORY, DirectoryObject, Handle, JOB, JobFlags, JobObject,
    ObjectType, POWER, PowerObject, Rights,
};
use zoraya_user::{
    directory_lookup, dup, handle_close, job_create, power_off, println, spawn, wait_exit,
};

/// `zosh`'s handles, at the slots ADR-0043 and ADR-0062 give a first process.
fn start_shell() -> Option<Handle> {
    let job = job_create(JOB, JobFlags::NONE).ok()?;
    let image = directory_lookup(DIRECTORY, "zosh").ok()?;
    let mut buffer = [0; 512];
    let mut e = Encoder::new(&mut buffer, job, image);
    e.handle(dup(CONSOLE.slot(), CONSOLE, ConsoleObject::RIGHTS));
    e.handle(dup(JOB.slot(), job, JobObject::RIGHTS));
    e.handle(dup(DIRECTORY.slot(), DIRECTORY, DirectoryObject::RIGHTS));
    e.handle(dup(POWER.slot(), POWER, without_duplicate()));
    let length = e.finish()?;
    let shell = spawn(&buffer[..length]).ok();
    let _ = handle_close(image);
    let _ = handle_close(job);
    shell
}

/// The shell turns the machine off and hands that to nobody.
const fn without_duplicate() -> Rights {
    PowerObject::RIGHTS
        .without(Rights::DUPLICATE)
        .without(Rights::TRANSFER)
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let Some(shell) = start_shell() else {
        println!("init: the shell did not start");
        return 1;
    };
    let record = wait_exit(shell);
    println!("init: shell ended: {record:?}");
    let status = power_off(POWER);
    println!("init: power off: {status:?}");
    2
}
