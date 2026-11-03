//! `posix-surface-check`: the export check of ADR-0100 item 7 and the `svc`
//! scan of INV-0100-2, with this tool's own `ar` and `ET_REL` readers.

// Only the capped allocator (capped.rs) has `unsafe`; everything else is
// refused by this lint.
#![deny(unsafe_code)]

pub mod ar;
pub mod elf;
pub mod rename;
pub mod surface;

#[allow(unsafe_code)]
pub mod capped;

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: capped::Capped = capped::Capped::new(capped::LIMIT);

#[cfg(test)]
mod tests;
