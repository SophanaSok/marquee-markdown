//! Serializing a rendered document into a ratatui buffer.
//!
//! The counterpart to [`ansi`](super::ansi): same lines, same widths, a
//! different destination. Everything here is a pure function of the document
//! and the viewport, so drawing never mutates application state.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::doc::RenderedDoc;
use super::measure;
use super::overlay::{self, Overlay};

/// Clip a line to the column window `[left, left + width)`, padding the result
/// out to exactly `width` cells.
///
/// The width invariant survives the clip: a double-width cluster straddling
/// either edge becomes a single space rather than half a character, so the
/// columns of every line still agree with each other.
#[must_use]
pub fn clip(line: &Line<'_>, left: u16, width: u16, fill: Style) -> Line<'static> {
    let from = usize::from(left);
    let end = from + usize::from(width);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut emitted = 0usize;
    let mut col = 0usize;
    let mut last_style = fill;

    for span in &line.spans {
        let start = col;
        col += measure::width(&span.content);
        if col <= from || start >= end {
            continue;
        }
        last_style = span.style;

        let mut text: &str = span.content.as_ref();
        let mut cursor = start;

        if cursor < from {
            let (_, head_w, tail) = measure::split_at_col(text, from - cursor);
            cursor += head_w;
            text = tail;
            // A wide cluster straddling the left edge: its right half shows as
            // a space, keeping every following column where it belongs.
            if cursor < from
                && let Some((cluster, rest)) = measure::split_first(text)
            {
                cursor += measure::grapheme_width(cluster);
                text = rest;
                spans.push(Span::styled(" ", span.style));
                emitted += 1;
            }
        }

        let budget = end.saturating_sub(cursor);
        if budget == 0 {
            break;
        }
        let (head, head_w, _) = measure::split_at_col(text, budget);
        if !head.is_empty() {
            spans.push(Span::styled(head.to_owned(), span.style));
            emitted += head_w;
        }
    }

    if emitted < usize::from(width) {
        let pad = usize::from(width) - emitted;
        spans.push(Span::styled(" ".repeat(pad), last_style));
    }
    Line::from(spans)
}

/// Draw the document into `area`, with `top` as the first visible line and
/// `left` as the first visible column of the content column.
///
/// The content column is centered in `area` and the whole area — gutters
/// included — is painted with `fill` first, so the page reads as one surface
/// rather than as text on the terminal's own background.
///
/// `overlay` restyles column ranges of individual lines on their way to the
/// buffer; pass [`Plain`](super::overlay::Plain) for none.
pub fn render(
    buf: &mut Buffer,
    area: Rect,
    doc: &RenderedDoc,
    top: usize,
    left: u16,
    fill: Style,
    overlay: &dyn Overlay,
) {
    // Clamp to the buffer: a resize can leave pane geometry a frame behind,
    // and drawing outside the buffer would panic rather than look wrong.
    let area = area.intersection(buf.area);
    paint(buf, area, fill);
    if area.is_empty() {
        return;
    }
    let visible = doc.width.min(area.width);
    let gutter = area.width.saturating_sub(visible) / 2;
    let mut patches = Vec::new();
    for row in 0..area.height {
        let index = top + usize::from(row);
        let Some(line) = doc.lines.get(index) else {
            break;
        };
        patches.clear();
        overlay.patches(index, &mut patches);
        let patched;
        let line = if patches.is_empty() {
            line
        } else {
            patched = overlay::apply(line, &patches);
            &patched
        };
        let clipped = clip(line, left, visible, fill);
        // `set_line` clips to both `visible` and the buffer, and leaves the
        // cell a double-width glyph covers untouched — which is what the
        // frame diff expects, since nothing is ever drawn there.
        buf.set_line(area.x + gutter, area.y + row, &clipped, visible);
    }
}

/// Fill an area with spaces in `style`.
///
/// Setting the style alone would leave whatever symbols the previous frame
/// drew, so the cells have to be written too.
pub fn paint(buf: &mut Buffer, area: Rect, style: Style) {
    let area = area.intersection(buf.area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].set_symbol(" ").set_style(style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{self, LayoutOptions};
    use crate::theme::{Theme, ThemeVariant};
    use ratatui::style::Color;

    fn line_of(text: &str) -> Line<'static> {
        Line::from(Span::styled(text.to_owned(), Style::default()))
    }

    fn text_of(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn clipping_preserves_the_requested_width() {
        let line = line_of("hello world");
        for left in 0..14u16 {
            for width in 1..14u16 {
                let out = clip(&line, left, width, Style::default());
                assert_eq!(
                    measure::width(&text_of(&out)),
                    usize::from(width),
                    "left={left} width={width}"
                );
            }
        }
    }

    #[test]
    fn a_wide_cluster_split_by_an_edge_becomes_a_space() {
        let line = line_of("日本語");
        // Starting one column into the first character shows its right half as
        // a blank, not as a torn glyph.
        let out = clip(&line, 1, 5, Style::default());
        assert_eq!(text_of(&out), " 本語");
        // The same at the trailing edge.
        let out = clip(&line, 0, 3, Style::default());
        assert_eq!(text_of(&out), "日 ");
    }

    #[test]
    fn clipping_a_real_document_never_breaks_the_invariant() {
        let source = include_str!("../../tests/fixtures/kitchen-sink.md");
        let theme = Theme::new(ThemeVariant::Slate);
        let doc = render::render(
            source,
            &theme,
            LayoutOptions {
                width: 60,
                code_line_numbers: false,
                preserve_new_lines: false,
            },
        );
        for line in &doc.lines {
            for left in [0u16, 1, 7, 30, 59, 60, 200] {
                let out = clip(line, left, 40, Style::default());
                assert_eq!(measure::width(&text_of(&out)), 40, "left={left}");
            }
        }
    }

    #[test]
    fn padding_carries_the_style_of_the_text_it_follows() {
        let styled = Line::from(Span::styled(
            "ab".to_owned(),
            Style::default().bg(Color::Red),
        ));
        let out = clip(&styled, 0, 6, Style::default());
        assert_eq!(out.spans.last().unwrap().style.bg, Some(Color::Red));
    }

    #[test]
    fn rendering_paints_the_gutters() {
        let theme = Theme::new(ThemeVariant::Slate);
        let doc = render::render(
            "hi",
            &theme,
            LayoutOptions {
                width: 10,
                code_line_numbers: false,
                preserve_new_lines: false,
            },
        );
        let area = Rect::new(0, 0, 20, 3);
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &doc, 0, 0, theme.page(), &overlay::Plain);
        for x in 0..20 {
            assert_eq!(buf[(x, 0)].style().bg, theme.page().bg, "column {x}");
        }
    }

    #[test]
    fn drawing_outside_the_buffer_is_clipped_rather_than_fatal() {
        let theme = Theme::new(ThemeVariant::Slate);
        let doc = render::render(
            "# Title\n\nbody\n",
            &theme,
            LayoutOptions {
                width: 80,
                code_line_numbers: false,
                preserve_new_lines: false,
            },
        );
        // Pane geometry from before a resize, against a buffer from after it.
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 6));
        render(
            &mut buf,
            Rect::new(0, 0, 80, 24),
            &doc,
            0,
            0,
            theme.page(),
            &overlay::Plain,
        );
    }
}
