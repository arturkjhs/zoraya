//! The system fonts (ADR-0162): the files of `fonts/`, unmodified, as
//! bytes. Inter 4.1 and JetBrains Mono 2.304, both under the SIL Open Font
//! License 1.1 (`fonts/Inter-LICENSE.txt`, `fonts/JetBrainsMono-OFL.txt`),
//! listed with their SHA-256 in `docs/dependencies.md` (INV-0162-5).
//!
//! A program embeds only the fonts it names: each is a separate constant.
//! ADR-0162 has clients load the fonts from `/system/fonts` through a
//! handle their launcher gives them; until the launcher of ADR-0168 exists
//! (stage 7d), the programs `init` starts embed them (proposed ADR-0240).

#![no_std]
#![forbid(unsafe_code)]

/// Inter Regular, the UI font (`font.ui`).
pub const INTER_REGULAR: &[u8] = include_bytes!("../../../fonts/Inter-Regular.ttf");
/// Inter Medium (`font.ui.medium`).
pub const INTER_MEDIUM: &[u8] = include_bytes!("../../../fonts/Inter-Medium.ttf");
/// JetBrains Mono Regular, the terminal's font (`font.mono`).
pub const MONO_REGULAR: &[u8] = include_bytes!("../../../fonts/JetBrainsMono-Regular.ttf");
/// JetBrains Mono Bold (`font.mono.bold`).
pub const MONO_BOLD: &[u8] = include_bytes!("../../../fonts/JetBrainsMono-Bold.ttf");

/// Every file of `fonts/`, by name.
pub const FILES: [(&str, &[u8]); 6] = [
    ("Inter-Regular.ttf", INTER_REGULAR),
    ("Inter-Medium.ttf", INTER_MEDIUM),
    ("JetBrainsMono-Regular.ttf", MONO_REGULAR),
    ("JetBrainsMono-Bold.ttf", MONO_BOLD),
    (
        "Inter-LICENSE.txt",
        include_bytes!("../../../fonts/Inter-LICENSE.txt"),
    ),
    (
        "JetBrainsMono-OFL.txt",
        include_bytes!("../../../fonts/JetBrainsMono-OFL.txt"),
    ),
];

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
