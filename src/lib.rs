//! `marquee-markdown` — a terminal markdown reader that renders documents the
//! way Claude artifacts do, with a table-of-contents panel for navigation.
//!
//! The crate is split so the rendering engine can be used without the TUI
//! shell: [`theme`] holds the palettes and [`render`] turns markdown into a
//! styled, fixed-width line buffer plus a navigable outline.

#![forbid(unsafe_code)]

pub mod render;
pub mod theme;
