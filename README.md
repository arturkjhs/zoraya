# Zoraya

An experiment in building an operating system from scratch with AI agents: a capability-based microkernel for aarch64
(QEMU `virt`), written in Rust, with POSIX as an isolated personality, a desktop whose compositor shows each window's
grants, rooms with quotas, and a design recorded in ADRs. Version 0.1 closes the experiment.

- **[The final report, `docs/zoraya-0.1.md`](docs/zoraya-0.1.md):** what Zoraya is, what exists, the numbers, how to build
  and run it, the limits, the work left and how the work was done.
- [`docs/design.md`](docs/design.md): what exists and where it lives; [`docs/adr/index.md`](docs/adr/index.md): the decisions.
- [`docs/tour.md`](docs/tour.md): a tour of the shell; [`CLAUDE.md`](CLAUDE.md): the rules agents worked by.

Build and run (Apple Silicon Mac, QEMU 11, Rust 1.99.1 from `rust-toolchain.toml`): `make ports`, `make apps`, then
`make run-gui` (the desktop) or `make run` (the serial console). `make check` runs the fast checks.

License: see [`LICENSE`](LICENSE) (BSD-2-Clause, with the exceptions it names).
