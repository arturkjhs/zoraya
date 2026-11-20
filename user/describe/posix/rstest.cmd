name     rstest
summary  the test program of Rust's std on Zoraya (a POSIX program built by tools/zcargo)
usage    rstest MODE [ARG...]
program  /bin/rstest
operand  MODE  text  the group to run: hello, syscall, threads, hash, time, fs DIR, denied DIR, process, panic, uncaught, abort, overflow
operand  [ARG...]  text  the mode's arguments; a directory a mode reads or writes needs a +r or +w of the line, or the program's own /tmp
exit     0  the group passed
exit     1  the group failed
exit     2  a usage mistake
---
rstest is the test program of TASK-31's work/rust-std (ADR-0450 to ADR-0454): Rust's prebuilt std for
aarch64-unknown-linux-musl, linked by tools/zcargo against libc.a, run as a POSIX program. Each mode runs one group of
checks and prints "rstest: MODE passed": the futex and getrandom answers of syscall(), threads, Mutex and Condvar that
sleep, HashMap, std::fs in the program's own /tmp and in a directory the line grants, std::process::Command (a child
that hashes has its own random connection, a child has no network, a program cannot change its identity), panics that
unwind. uncaught ends with std's message and exit status 101; abort with signal 6; overflow with a fault, with no Rust
message.

EXAMPLES
  rstest threads
  +w /work/granted rstest fs /work/granted
