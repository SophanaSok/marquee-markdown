//! The rendered document: a fixed-width styled line buffer plus everything the
//! viewer needs to navigate it.
//!
//! Indices into `lines` are the shared currency of the whole application —
//! scroll position, outline anchors, search matches, and link positions all
//! point into it. They are only valid for one layout; any re-layout goes
//! through the owning cache, which remaps them together.

use std::ops::Range;

use ratatui::text::Line;
use smallvec::SmallVec;

/// A fully laid-out document at one content width.
#[derive(Debug, Clone, Default)]
pub struct RenderedDoc {
    /// Styled lines, each exactly `width` cells wide.
    pub lines: Vec<Line<'static>>,
    /// Per-line metadata, parallel to `lines`.
    pub meta: Vec<LineMeta>,
    /// Heading anchors in document order (sorted by line index).
    pub outline: Vec<Anchor>,
    /// Interned link destinations; `LineMeta::links` refers by index.
    pub links: Vec<String>,
    /// Flattened display text of all lines joined by `\n`, for search.
    pub plain: String,
    /// The content width this layout was produced at.
    pub width: u16,
}

/// Metadata for one rendered line.
#[derive(Debug, Clone, Default)]
pub struct LineMeta {
    pub kind: LineKind,
    /// Byte range of the markdown source this line came from; used to restore
    /// the reading position across re-layout.
    pub source: Option<Range<usize>>,
    /// Byte range of this line's text within [`RenderedDoc::plain`].
    pub plain: Range<usize>,
    /// Column ranges (in cells) occupied by links, with link-table indices.
    pub links: SmallVec<[(Range<u16>, u32); 1]>,
    /// Display columns of leading decoration — quote bars, list markers,
    /// hanging indent — at the start of this line's text in `plain`. What
    /// comes after it is the line's content; search strips it so a phrase
    /// can be matched across a soft wrap without the gutter in the middle.
    pub lead_cols: u16,
}

/// What a rendered line is, for styling overlays and navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineKind {
    #[default]
    Blank,
    Body,
    Heading(u8),
    Rule,
    /// Interior row of code block `block` (0-based among code blocks).
    Code {
        block: u32,
    },
    /// Border row (top/bottom) of code block `block`.
    CodeBorder {
        block: u32,
    },
    Table,
    Quote,
    List,
    Html,
}

/// One heading anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    /// Line index of the heading in `lines`.
    pub line: usize,
    /// 1-6.
    pub level: u8,
    /// Deduplicated slug.
    pub id: String,
    /// Plain heading text, for the TOC.
    pub text: String,
}

impl RenderedDoc {
    /// The heading whose section contains `top_line`: the last anchor at or
    /// above it. `None` before the first heading or in an outline-less doc.
    #[must_use]
    pub fn active_anchor(&self, top_line: usize) -> Option<usize> {
        let idx = self.outline.partition_point(|a| a.line <= top_line);
        idx.checked_sub(1)
    }

    /// Map a byte offset in `plain` to its line index.
    #[must_use]
    pub fn line_of_plain_offset(&self, offset: usize) -> usize {
        self.meta
            .partition_point(|m| m.plain.start <= offset)
            .saturating_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(line: usize, level: u8) -> Anchor {
        Anchor {
            line,
            level,
            id: format!("h{line}"),
            text: format!("H{line}"),
        }
    }

    #[test]
    fn active_anchor_tracks_scroll() {
        let doc = RenderedDoc {
            outline: vec![anchor(0, 1), anchor(10, 2), anchor(20, 2)],
            ..Default::default()
        };
        assert_eq!(doc.active_anchor(0), Some(0));
        assert_eq!(doc.active_anchor(9), Some(0));
        assert_eq!(doc.active_anchor(10), Some(1));
        assert_eq!(doc.active_anchor(15), Some(1));
        assert_eq!(doc.active_anchor(20), Some(2));
        assert_eq!(doc.active_anchor(usize::MAX), Some(2));
    }

    #[test]
    fn active_anchor_is_none_before_first_heading() {
        let doc = RenderedDoc {
            outline: vec![anchor(5, 1)],
            ..Default::default()
        };
        assert_eq!(doc.active_anchor(0), None);
        assert_eq!(doc.active_anchor(4), None);
        assert_eq!(doc.active_anchor(5), Some(0));
    }

    #[test]
    fn active_anchor_on_empty_outline() {
        let doc = RenderedDoc::default();
        assert_eq!(doc.active_anchor(0), None);
    }

    #[test]
    fn plain_offset_maps_to_line() {
        let doc = RenderedDoc {
            meta: vec![
                LineMeta {
                    plain: 0..5,
                    ..Default::default()
                },
                LineMeta {
                    plain: 6..11,
                    ..Default::default()
                },
                LineMeta {
                    plain: 12..12,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(doc.line_of_plain_offset(0), 0);
        assert_eq!(doc.line_of_plain_offset(4), 0);
        assert_eq!(doc.line_of_plain_offset(6), 1);
        assert_eq!(doc.line_of_plain_offset(12), 2);
    }
}
