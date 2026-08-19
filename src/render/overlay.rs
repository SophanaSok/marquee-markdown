//! Restyling column ranges of a line at draw time.
//!
//! Search highlighting, the current match, and the active link all want to
//! recolor a few cells of an already-rendered line. Doing that by re-laying
//! out the document would invalidate every line index in the application; this
//! module patches the line on its way to the buffer instead, so a search
//! changes nothing about the layout.
//!
//! Patches carry a partial [`Style`] and are applied with
//! [`Style::patch`](ratatui::style::Style::patch), so a highlight that sets
//! only a background leaves syntax colors intact underneath it.

use std::ops::Range;

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::measure;

/// A column range to restyle, in cells from the start of the line.
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    /// Half-open column range.
    pub cols: Range<u16>,
    /// Style laid over whatever is already there.
    pub style: Style,
}

/// Something that knows which parts of a line to restyle.
///
/// Implemented by the application over its search matches; the renderer never
/// learns what a match is.
pub trait Overlay {
    /// Append the patches for `line` to `out`, which the caller has cleared.
    ///
    /// Patches must be ordered by column and must not overlap.
    fn patches(&self, line: usize, out: &mut Vec<Patch>);
}

/// An overlay that changes nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct Plain;

impl Overlay for Plain {
    fn patches(&self, _line: usize, _out: &mut Vec<Patch>) {}
}

/// Apply `patches` to `line`, splitting spans at the patch boundaries.
///
/// A double-width cluster straddling a boundary is taken whole by the patch it
/// starts inside, which keeps the line's total width unchanged — the invariant
/// everything downstream depends on.
#[must_use]
pub fn apply(line: &Line<'_>, patches: &[Patch]) -> Line<'static> {
    if patches.is_empty() {
        return owned(line);
    }
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut col: u16 = 0;

    for span in &line.spans {
        let mut text: &str = span.content.as_ref();
        let span_end = col.saturating_add(width_of(text));
        while !text.is_empty() {
            let covering = patches.iter().find(|patch| patch.cols.contains(&col));
            // Cut at whichever comes first: the end of the patch we are inside,
            // the start of the next one, or the end of the span.
            let boundary = match covering {
                Some(patch) => patch.cols.end,
                None => patches
                    .iter()
                    .map(|patch| patch.cols.start)
                    .find(|&start| start > col)
                    .unwrap_or(span_end),
            }
            .min(span_end);

            let (head, head_width, tail) = measure::split_at_col(text, usize::from(boundary - col));
            let (head, head_width, tail) = if head.is_empty() {
                // A cluster wider than the remaining room to the boundary: it
                // belongs to whichever patch it starts in, whole.
                let (cluster, rest) = measure::split_first(text).expect("text is not empty");
                (cluster, measure::grapheme_width(cluster), rest)
            } else {
                (head, head_width, tail)
            };

            let style = covering.map_or(span.style, |patch| span.style.patch(patch.style));
            out.push(Span::styled(head.to_owned(), style));
            col = col.saturating_add(width_of(head));
            debug_assert!(head_width > 0, "no progress applying an overlay");
            text = tail;
        }
        col = span_end;
    }
    Line::from(out)
}

/// Width as a column count, saturating rather than wrapping on a line far
/// wider than any terminal.
fn width_of(text: &str) -> u16 {
    u16::try_from(measure::width(text)).unwrap_or(u16::MAX)
}

/// A copy of `line` that owns its text.
fn owned(line: &Line<'_>) -> Line<'static> {
    Line::from(
        line.spans
            .iter()
            .map(|span| Span::styled(span.content.to_string(), span.style))
            .collect::<Vec<_>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn line() -> Line<'static> {
        Line::from(vec![
            Span::styled("hello ", Style::new().fg(Color::White).bg(Color::Black)),
            Span::styled("world", Style::new().fg(Color::Green).bg(Color::Black)),
        ])
    }

    fn patch(cols: Range<u16>) -> Patch {
        Patch {
            cols,
            style: Style::new().bg(Color::Yellow),
        }
    }

    fn text_of(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// The style in force at column `col`.
    fn style_at(line: &Line<'_>, col: usize) -> Style {
        let mut seen = 0;
        for span in &line.spans {
            seen += measure::width(&span.content);
            if seen > col {
                return span.style;
            }
        }
        panic!("column {col} is past the end of the line");
    }

    #[test]
    fn text_and_width_are_untouched() {
        let line = line();
        for patches in [
            vec![],
            vec![patch(0..3)],
            vec![patch(2..8)],
            vec![patch(0..11)],
            vec![patch(1..2), patch(4..5), patch(9..11)],
        ] {
            let out = apply(&line, &patches);
            assert_eq!(text_of(&out), "hello world");
        }
    }

    #[test]
    fn a_background_patch_leaves_the_foreground_alone() {
        // Otherwise highlighting a match inside a code block would flatten the
        // syntax colors under it.
        let out = apply(&line(), &[patch(7..10)]);
        assert_eq!(style_at(&out, 8).bg, Some(Color::Yellow));
        assert_eq!(
            style_at(&out, 8).fg,
            Some(Color::Green),
            "syntax color lost"
        );
    }

    #[test]
    fn cells_outside_the_patch_keep_their_style() {
        let out = apply(&line(), &[patch(2..4)]);
        assert_eq!(style_at(&out, 1).bg, Some(Color::Black));
        assert_eq!(style_at(&out, 2).bg, Some(Color::Yellow));
        assert_eq!(style_at(&out, 3).bg, Some(Color::Yellow));
        assert_eq!(style_at(&out, 4).bg, Some(Color::Black));
    }

    #[test]
    fn a_patch_spanning_a_span_boundary_covers_both_sides() {
        let out = apply(&line(), &[patch(4..8)]);
        for col in 4..8 {
            assert_eq!(style_at(&out, col).bg, Some(Color::Yellow), "column {col}");
        }
    }

    #[test]
    fn a_patch_edge_inside_a_wide_character_does_not_tear_it() {
        let line = Line::from(Span::styled("日本語", Style::new().bg(Color::Black)));
        // The boundary falls in the middle of the second character.
        let out = apply(&line, &[patch(0..3)]);
        assert_eq!(text_of(&out), "日本語");
        assert_eq!(measure::width(&text_of(&out)), 6, "width changed");
        // The straddling character is taken whole by the patch it starts in.
        assert_eq!(style_at(&out, 2).bg, Some(Color::Yellow));
        assert_eq!(style_at(&out, 4).bg, Some(Color::Black));
    }

    #[test]
    fn a_patch_past_the_end_of_the_line_changes_nothing() {
        let out = apply(&line(), &[patch(50..60)]);
        assert_eq!(text_of(&out), "hello world");
        assert_eq!(style_at(&out, 0).bg, Some(Color::Black));
    }

    #[test]
    fn an_empty_line_survives_being_patched() {
        let out = apply(&Line::from(Vec::<Span>::new()), &[patch(0..4)]);
        assert_eq!(text_of(&out), "");
    }
}
