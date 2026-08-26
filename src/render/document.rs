//! A parsed document, ready to be laid out at any width.
//!
//! Parsing is the expensive half and its result does not depend on the width
//! or the theme, so a reader that resizes should not pay for it again. This
//! type owns that result.
//!
//! It is also deliberately opaque. The block tree behind it is the renderer's
//! working representation and changes as the pipeline does; keeping it behind
//! this type is what lets the public API stay small enough to promise.

use super::block::{Block, BlockKind};
use super::doc::RenderedDoc;
use super::highlight::HighlightCache;
use super::layout::LayoutOptions;
use super::parse::ParseOptions;
use super::{layout, parse};
use crate::theme::Theme;

/// Markdown that has been parsed but not yet laid out.
pub struct Document {
    blocks: Vec<Block>,
    headings: usize,
    /// Work that neither the width nor the options change, kept across
    /// layouts. Not part of what a `Document` *is*, which is why the three
    /// impls below step around it rather than deriving over it.
    highlights: HighlightCache,
}

/// Written out rather than derived so the cache stays out of it: a memo of
/// what has been asked for so far is not something to print.
impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("blocks", &self.blocks)
            .field("headings", &self.headings)
            .finish_non_exhaustive()
    }
}

/// A clone starts with an empty cache rather than duplicating however much of
/// one had been filled. The copy renders identically either way; this only
/// decides whether it pays for the first layout again.
impl Clone for Document {
    fn clone(&self) -> Self {
        Self {
            blocks: self.blocks.clone(),
            headings: self.headings,
            highlights: HighlightCache::default(),
        }
    }
}

/// Two documents are equal when they say the same thing. What either has
/// happened to highlight so far is not part of that — and a cache that could
/// make `==` false would make it depend on what had been drawn.
impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        self.blocks == other.blocks && self.headings == other.headings
    }
}

impl Document {
    /// Parse markdown, with default options.
    #[must_use]
    pub fn parse(source: &str) -> Self {
        Self::parse_with(source, ParseOptions::default())
    }

    /// Parse markdown.
    ///
    /// [`ParseOptions`] change the tree rather than its presentation, so they
    /// belong here rather than on [`Self::layout`] — an HTML heading has to
    /// reach [`Self::heading_count`], which is answered before anything is
    /// laid out.
    #[must_use]
    pub fn parse_with(source: &str, options: ParseOptions) -> Self {
        let blocks = parse::parse_with(source, options);
        let headings = count_headings(&blocks);
        Self {
            blocks,
            headings,
            highlights: HighlightCache::default(),
        }
    }

    /// Lay the document out at one width.
    ///
    /// Cheap enough to call on every resize: neither the parsing nor the
    /// syntax highlighting is repeated. Highlighting depends on the theme
    /// rather than the width, so switching themes does pay for it again —
    /// once.
    #[must_use]
    pub fn layout(&self, theme: &Theme, options: LayoutOptions) -> RenderedDoc {
        layout::layout_with_cache(&self.blocks, theme, options, &self.highlights)
    }

    /// How many headings the document has, anywhere in it.
    ///
    /// Available without laying anything out, which matters to anything that
    /// has to decide how much room to give the text — a table-of-contents
    /// pane, say — before there is a layout to ask.
    #[must_use]
    pub fn heading_count(&self) -> usize {
        self.headings
    }

    /// Whether the document has no content at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// The block tree. Internal: its shape is not part of the public API.
    #[doc(hidden)]
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// How many code blocks this document has had to highlight so far.
    ///
    /// Internal, and here so the memo can be held to its promise mechanically:
    /// a resize must not move this number. Timing would say the same thing
    /// far less reliably.
    #[doc(hidden)]
    #[must_use]
    pub fn highlight_computations(&self) -> usize {
        self.highlights.computed()
    }

    /// How many code blocks the document contains, at any depth.
    ///
    /// Internal, and the number [`Self::highlight_computations`] is expected
    /// to settle at.
    #[doc(hidden)]
    #[must_use]
    pub fn code_block_count(&self) -> usize {
        count_code_blocks(&self.blocks)
    }
}

/// `Document` is in the promised API, so its auto traits are part of what is
/// promised — `cargo semver-checks` fails the build if they change. Holding
/// the highlight cache behind a `RefCell` rather than a `Mutex` would have
/// taken `Sync` away, and this is where that gets caught: at compile time,
/// here, rather than in CI on the way to a release.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Document>();
};

/// Count code blocks anywhere in the tree, including inside quotes and lists.
fn count_code_blocks(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| match &block.kind {
            BlockKind::CodeBlock { .. } => 1,
            BlockKind::BlockQuote { children, .. }
            | BlockKind::FootnoteDefinition { children, .. } => count_code_blocks(children),
            BlockKind::List { items, .. } => items
                .iter()
                .map(|item| count_code_blocks(&item.children))
                .sum(),
            _ => 0,
        })
        .sum()
}

/// Count headings anywhere in the tree, including inside quotes and lists.
fn count_headings(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| match &block.kind {
            BlockKind::Heading { .. } => 1,
            BlockKind::BlockQuote { children, .. }
            | BlockKind::FootnoteDefinition { children, .. } => count_headings(children),
            BlockKind::List { items, .. } => items
                .iter()
                .map(|item| count_headings(&item.children))
                .sum(),
            _ => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeVariant;

    fn options(width: u16) -> LayoutOptions {
        LayoutOptions {
            width,
            code_line_numbers: false,
            preserve_new_lines: false,
        }
    }

    #[test]
    fn one_parse_serves_any_number_of_widths() {
        let document = Document::parse("# Title\n\nSome prose that will wrap differently.\n");
        let theme = Theme::new(ThemeVariant::Slate);
        for width in [20u16, 40, 80] {
            let laid_out = document.layout(&theme, options(width));
            assert_eq!(laid_out.width, width);
            assert!(
                laid_out
                    .lines
                    .iter()
                    .all(|line| line.width() == usize::from(width))
            );
        }
    }

    #[test]
    fn headings_are_counted_without_laying_anything_out() {
        let document = Document::parse("# One\n\n## Two\n\n> ### Quoted\n\nbody\n");
        assert_eq!(document.heading_count(), 3);
    }

    #[test]
    fn the_count_agrees_with_what_a_layout_produces() {
        let document = Document::parse("# One\n\n## Two\n\n### Three\n\nbody\n");
        let laid_out = document.layout(&Theme::new(ThemeVariant::Slate), options(40));
        assert_eq!(document.heading_count(), laid_out.outline.len());
    }

    #[test]
    fn an_empty_document_is_empty() {
        assert!(Document::parse("").is_empty());
        assert!(!Document::parse("x\n").is_empty());
    }
}
