//! The rendering engine: markdown source → styled, fixed-width line buffer.
//!
//! This module tree is the reusable core of the crate and stays free of any
//! dependency on the application shell (`app`, `ui`, `browser`, `doc`) — the
//! test in `tests/layering.rs` enforces that.
//!
//! # Example
//!
//! ```
//! use marquee_markdown::render::{self, LayoutOptions};
//! use marquee_markdown::theme::{Theme, ThemeVariant};
//!
//! let doc = render::render(
//!     "# Title\n\nSome prose.",
//!     &Theme::new(ThemeVariant::Slate),
//!     LayoutOptions { width: 40, code_line_numbers: false },
//! );
//! assert_eq!(doc.outline[0].text, "Title");
//! assert!(doc.lines.iter().all(|l| l.width() == 40));
//! ```

pub mod ansi;
pub mod block;
pub mod doc;
pub mod frag;
pub mod highlight;
pub mod layout;
pub mod measure;
pub mod parse;
pub mod sink;
pub mod tui;
pub mod wrap;

pub use doc::{Anchor, LineKind, LineMeta, RenderedDoc};
pub use layout::LayoutOptions;

use crate::theme::Theme;

/// Parse and lay out a markdown document in one call.
///
/// For repeated re-layout at different widths, parse once with
/// [`parse::parse`] and call [`layout::layout`] directly — parsing is the
/// expensive half and its result is width-independent.
#[must_use]
pub fn render(source: &str, theme: &Theme, options: LayoutOptions) -> RenderedDoc {
    layout::layout(&parse::parse(source), theme, options)
}
