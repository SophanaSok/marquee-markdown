//! The rendering engine: markdown source → styled, fixed-width line buffer.
//!
//! This module tree is the reusable core of the crate and stays free of any
//! dependency on the application shell (`app`, `ui`, `browser`, `doc`) — the
//! test in `tests/layering.rs` enforces that.
//!
//! # What is stable
//!
//! The API this module promises is deliberately small: [`render`] and
//! [`render_with`], [`Document`], [`RenderedDoc`] and the metadata hanging off
//! it, [`LayoutOptions`], [`ParseOptions`], [`HtmlMode`], the two serializers
//! ([`ansi`] and [`tui`]), [`overlay`], [`measure`], and the whole of
//! [`theme`](crate::theme). From 1.0 those follow semantic versioning.
//!
//! The pipeline behind them — parsing, fragmentation, wrapping, the block
//! tree, the per-block emitters — is public so the binary and the tests can
//! reach it, and because it is worth reading. It is marked `#[doc(hidden)]`
//! and may change in any release. If you find yourself needing something from
//! it, that is worth an issue: it probably means the stable surface is missing
//! something.
//!
//! # Example
//!
//! ```
//! use marquee_markdown::render::{self, Document, LayoutOptions};
//! use marquee_markdown::theme::{Theme, ThemeVariant};
//!
//! let options = LayoutOptions {
//!     width: 40,
//!     code_line_numbers: false,
//!     preserve_new_lines: false,
//! };
//! let theme = Theme::new(ThemeVariant::Slate);
//!
//! let doc = render::render("# Title\n\nSome prose.", &theme, options);
//! assert_eq!(doc.outline[0].text, "Title");
//! assert!(doc.lines.iter().all(|line| line.width() == 40));
//!
//! // Raw HTML is interpreted by default, so an HTML heading joins the
//! // outline like a markdown one. `ParseOptions` says otherwise.
//! use marquee_markdown::render::{HtmlMode, ParseOptions};
//! let mut parse = ParseOptions::default();
//! parse.html = HtmlMode::Literal;
//! let doc = render::render_with("<h1>Title</h1>", &theme, parse, options);
//! assert!(doc.outline.is_empty());
//!
//! // Parse once, lay out as often as the window changes size.
//! let parsed = Document::parse("# Title\n\nSome prose.");
//! for width in [20, 40, 80] {
//!     let doc = parsed.layout(&theme, LayoutOptions { width, ..options });
//!     assert_eq!(doc.width, width);
//! }
//! ```

pub mod ansi;
pub mod doc;
pub mod document;
pub mod measure;
pub mod overlay;
pub mod tui;

// The pipeline. Public because the binary, the tests and the example reach
// into it, and because it is worth reading — but not part of the promised API:
// its shape changes whenever the renderer does. Everything a consumer needs is
// re-exported above, or reachable through `Document`.
#[doc(hidden)]
pub mod block;
#[doc(hidden)]
pub mod frag;
#[doc(hidden)]
pub mod highlight;
#[doc(hidden)]
pub mod html;
#[doc(hidden)]
pub mod layout;
#[doc(hidden)]
pub mod parse;
#[doc(hidden)]
pub mod sink;
#[doc(hidden)]
pub mod wrap;

pub use doc::{Anchor, LineKind, LineMeta, RenderedDoc};
pub use document::Document;
pub use html::HtmlMode;
pub use layout::LayoutOptions;
pub use parse::ParseOptions;

use crate::theme::Theme;

/// Parse and lay out a markdown document in one call.
///
/// For repeated re-layout at different widths — a reader that resizes — parse
/// once into a [`Document`] and lay that out instead. Parsing is the expensive
/// half and its result does not depend on the width.
#[must_use]
pub fn render(source: &str, theme: &Theme, options: LayoutOptions) -> RenderedDoc {
    Document::parse(source).layout(theme, options)
}

/// Parse and lay out, saying how raw HTML should be treated.
///
/// The two option sets are separate because they invalidate different things:
/// a [`ParseOptions`] change means re-parsing, a [`LayoutOptions`] change means
/// only re-laying-out, which is what a resize does many times a second.
#[must_use]
pub fn render_with(
    source: &str,
    theme: &Theme,
    parse: ParseOptions,
    layout: LayoutOptions,
) -> RenderedDoc {
    Document::parse_with(source, parse).layout(theme, layout)
}
