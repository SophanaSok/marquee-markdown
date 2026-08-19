//! `marquee-markdown` — a terminal markdown reader that renders documents the
//! way Claude artifacts do, with a table-of-contents panel for navigation.
//!
//! The crate is split so the rendering engine can be used without the TUI
//! shell: [`theme`] holds the palettes and [`render`] turns markdown into a
//! styled, fixed-width line buffer plus a navigable outline. [`source`] and
//! [`util`] make up the application plumbing around it.

#![forbid(unsafe_code)]

pub mod app;
pub mod cli;
pub mod doc;
pub mod oneshot;
pub mod render;
pub mod source;
pub mod theme;
pub mod ui;
pub mod util;
