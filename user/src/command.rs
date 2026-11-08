//! The start of every native command (ADR-0155 rule 7): its arguments
//! parsed from its description by `zoraya-command`, `--help` and usage
//! mistakes printed in the one format. A described program never reads its
//! argv itself (INV-0155-2, checked by `make check-argv`).

use alloc::string::String;
use alloc::vec::Vec;

use zoraya_command::{Outcome, Parsed, exit};
use zoraya_describe::Description;

use crate::{eprint, print, process_exit, startup};

/// A command's description and its arguments as text.
pub struct Command {
    pub d: Description,
    args: Vec<String>,
}

/// Reads this program's description and arguments. An argument that is not
/// UTF-8 is a usage mistake (ADR-0013).
pub fn start(description: &str) -> Command {
    let d = zoraya_describe::parse(description.as_bytes()).expect("checked when it was built");
    let mut args = Vec::new();
    for (i, arg) in startup().argv.enumerate() {
        match core::str::from_utf8(arg) {
            Ok(s) => args.push(String::from(s)),
            Err(_) => {
                let e = zoraya_command::UsageError(alloc::format!("argument {}: not UTF-8", i + 1));
                eprint!("{}", zoraya_command::usage_error(&d, &e));
                process_exit(i32::from(exit::USAGE));
            }
        }
    }
    Command { d, args }
}

impl Command {
    /// The arguments sorted by the description. `--help` prints the help
    /// and exits 0; a usage mistake prints it and exits 2.
    pub fn parse(&self) -> Parsed<'_> {
        let texts: Vec<&str> = self.args.iter().map(String::as_str).collect();
        match zoraya_command::parse(&self.d, &texts) {
            Ok(Outcome::Run(p)) => p,
            Ok(Outcome::Help) => {
                print!("{}", zoraya_command::help(&self.d));
                process_exit(i32::from(exit::SUCCESS));
            }
            Err(e) => {
                eprint!("{}", zoraya_command::usage_error(&self.d, &e));
                process_exit(i32::from(exit::USAGE));
            }
        }
    }

    /// Prints `NAME: SUBJECT: PROBLEM` (rule 5).
    pub fn error(&self, subject: Option<&str>, problem: &str) {
        eprint!(
            "{}",
            zoraya_command::error(&self.d.name, subject, problem, &[])
        );
    }
}
