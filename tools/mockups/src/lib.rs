//! `zoraya-mockups`: renders the stage 7 desktop mockups (TASK-19) from a
//! direction's token file, with its own TrueType reader, scan-line
//! rasteriser and PNG encoder, so that it needs no crate. Illustrations,
//! not the implementation: ADR-0161 and ADR-0162 describe what the system
//! will build.

pub mod canvas;
pub mod clear;
pub mod files;
pub mod png;
pub mod raster;
pub mod scene;
pub mod tokens;
pub mod ttf;
