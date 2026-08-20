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
use super::layout::LayoutOptions;
use super::parse::ParseOptions;
use super::{layout, parse};
use crate::theme::Theme;

/// Markdown that has been parsed but not yet laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    blocks: Vec<Block>,
    headings: usize,
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
        Self { blocks, headings }
    }

    /// Lay the document out at one width.
    ///
    /// Cheap enough to call on every resize; the parsing is not repeated.
    #[must_use]
    pub fn layout(&self, theme: &Theme, options: LayoutOptions) -> RenderedDoc {
        layout::layout(&self.blocks, theme, options)
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
