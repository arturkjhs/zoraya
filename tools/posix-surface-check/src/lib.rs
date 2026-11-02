//! `posix-surface-check`: the export check of ADR-0100 item 7 and the `svc`
//! scan of INV-0100-2, with this tool's own `ar` and `ET_REL` readers.

#![forbid(unsafe_code)]

pub mod ar;
pub mod elf;
pub mod rename;
pub mod surface;

#[cfg(test)]
mod tests;
