//! `marquee-markdown` — a terminal markdown reader that renders documents the
//! way Claude artifacts do, with a table-of-contents panel for navigation.
//!
//! The crate is split so the rendering engine can be used without the TUI
//! shell: [`theme`] holds the palettes and [`render`] turns markdown into a
//! styled, fixed-width line buffer plus a navigable outline. Those two modules
//! are the library, and they follow semantic versioning.
//!
//! Everything else is the reader itself — the event loop, the widgets, the
//! file browser, source resolution, configuration. It is public so the two
//! binaries and the integration tests can reach it, and hidden from this
//! documentation because it is not part of the promised API: it changes
//! whenever the reader does.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod render;
pub mod theme;

// The reader. Public for the binaries and the tests, and `#[doc(hidden)]` for
// the same reason the render pipeline is: `cargo semver-checks` holds every
// visible item to semver, and these modules have changed shape in most minor
// releases. If a consumer needs something from here, that is worth an issue —
// it probably belongs in `render` or `theme`.
#[doc(hidden)]
pub mod app;
#[doc(hidden)]
pub mod browser;
#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod doc;
#[doc(hidden)]
pub mod oneshot;
#[doc(hidden)]
pub mod source;
#[doc(hidden)]
pub mod ui;
#[doc(hidden)]
pub mod update_check;
#[doc(hidden)]
pub mod util;
