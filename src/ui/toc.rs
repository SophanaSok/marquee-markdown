//! The contents pane.
//!
//! Two different things are highlighted here and they must not be confused:
//! the *active* entry is the section the document is scrolled to and follows
//! the reader around, while the *cursor* is where the reader put it and stays
//! there. A pane that collapses the two feels broken — scrolling would drag
//! the selection away mid-keystroke.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::app::state::{App, Focus};
use crate::render::{measure, tui};

/// Columns given to the fold marker, including the space after it.
const FOLD: usize = 2;
/// Columns each level of nesting indents by.
const INDENT: usize = 2;

/// Draw the contents pane, if it is on show.
pub fn draw(frame: &mut Frame, app: &App) {
    let Some(area) = app.panes.sidebar else {
        return;
    };
    tui::paint(frame.buffer_mut(), area, app.theme.page());
    if area.width == 0 {
        return;
    }

    // The last column is the hairline separating the pane from the document.
    let divider = Rect {
        x: area.x + area.width - 1,
        width: 1,
        ..area
    };
    let text_area = Rect {
        width: area.width - 1,
        ..area
    };
    for y in divider.top()..divider.bottom() {
        frame.buffer_mut()[(divider.x, y)]
            .set_symbol("│")
            .set_style(app.theme.sidebar_divider());
    }

    for row in 0..text_area.height {
        let Some(&index) = app.toc.visible.get(app.toc.offset + usize::from(row)) else {
            break;
        };
        let line = entry(app, index, text_area.width);
        frame
            .buffer_mut()
            .set_line(text_area.x, text_area.y + row, &line, text_area.width);
    }
}

/// One entry, exactly `width` cells wide.
#[must_use]
pub fn entry(app: &App, index: usize, width: u16) -> Line<'static> {
    let theme = &app.theme;
    let Some(row) = app.doc.outline().rows().get(index) else {
        return Line::from(Span::styled(
            " ".repeat(usize::from(width)),
            theme.toc_item(),
        ));
    };
    let is_cursor = index == app.toc.cursor;
    let is_active = app.active == Some(row.anchor);
    let focused = app.focus == Focus::Toc;

    let base = if is_cursor {
        theme.toc_cursor()
    } else if is_active {
        theme.toc_active()
    } else {
        theme.toc_item()
    };
    // The bar says where the cursor is only while the pane has focus, so an
    // unfocused pane cannot look like it is taking keys.
    let (marker, marker_style) = if is_cursor && focused {
        ("▎", theme.toc_cursor_marker())
    } else if is_cursor {
        (" ", theme.toc_cursor())
    } else {
        (" ", theme.toc_item())
    };

    let total = usize::from(width);
    if total <= 1 {
        return Line::from(Span::styled(marker.to_owned(), marker_style));
    }

    // Room is given out in order — marker, fold state, indent, text — so a
    // very narrow pane loses the decoration rather than the heading.
    let available = total - 1;
    let fold_room = FOLD.min(available);
    let indent = (row.depth * INDENT).min(available - fold_room);
    let fold = measure::truncate(
        if row.has_children() {
            if app.toc.collapsed.get(index).copied().unwrap_or(false) {
                "▸ "
            } else {
                "▾ "
            }
        } else {
            "  "
        },
        fold_room,
        "",
    );
    let fold_style = if is_cursor { base } else { theme.toc_fold() };

    let used = 1 + indent + measure::width(&fold);
    let text = app
        .anchor_of(index)
        .map(|anchor| measure::truncate(&anchor.text, total - used, "…"))
        .unwrap_or_default();
    let pad = total - used - measure::width(&text);

    Line::from(vec![
        Span::styled(marker.to_owned(), marker_style),
        Span::styled(" ".repeat(indent), base),
        Span::styled(fold, fold_style),
        Span::styled(text, base),
        Span::styled(" ".repeat(pad), base),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::Options;
    use crate::source::{Base, Source};
    use crate::theme::{Theme, ThemeVariant};

    fn app(text: &str) -> App {
        let mut app = App::new(
            Source::from_text(text, None, "t.md".into(), Base::Cwd),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        );
        crate::app::reconcile(&mut app, Rect::new(0, 0, 100, 30));
        app
    }

    fn nested() -> App {
        app("# One\n\nbody\n\n## Under\n\nbody\n\n# Two\n\nbody\n")
    }

    fn text_of(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn an_entry_is_exactly_the_pane_width() {
        let app = nested();
        for width in 1..40u16 {
            for index in 0..app.doc.outline().len() {
                let line = entry(&app, index, width);
                assert_eq!(
                    measure::width(&text_of(&line)),
                    usize::from(width),
                    "row {index} at width {width}"
                );
            }
        }
    }

    #[test]
    fn a_long_heading_is_truncated_rather_than_wrapped() {
        let app = app("# A heading far longer than any sidebar\n\n## Two\n");
        let text = text_of(&entry(&app, 0, 20));
        assert!(text.contains('…'), "{text:?}");
    }

    #[test]
    fn nesting_shows_as_indentation() {
        let app = nested();
        let parent = text_of(&entry(&app, 0, 24));
        let child = text_of(&entry(&app, 1, 24));
        let lead = |text: &str| text.len() - text.trim_start().len();
        assert!(lead(&child) > lead(&parent), "{child:?} vs {parent:?}");
    }

    #[test]
    fn a_section_with_children_shows_its_fold_state() {
        let mut app = nested();
        assert!(text_of(&entry(&app, 0, 24)).contains('▾'));
        app.toc.collapsed = vec![true, false, false];
        assert!(text_of(&entry(&app, 0, 24)).contains('▸'));
        // A leaf has no marker to show.
        let leaf = text_of(&entry(&app, 1, 24));
        assert!(!leaf.contains('▾') && !leaf.contains('▸'), "{leaf:?}");
    }

    #[test]
    fn the_cursor_bar_appears_only_when_the_pane_has_focus() {
        let mut app = nested();
        app.toc.cursor = 1;
        assert!(!text_of(&entry(&app, 1, 24)).contains('▎'));
        app.focus = Focus::Toc;
        assert!(text_of(&entry(&app, 1, 24)).contains('▎'));
        // And only on the cursor row.
        assert!(!text_of(&entry(&app, 0, 24)).contains('▎'));
    }

    #[test]
    fn the_cursor_and_the_active_entry_are_styled_differently() {
        let mut app = nested();
        app.toc.cursor = 2;
        app.active = Some(0);
        let cursor = entry(&app, 2, 24);
        let active = entry(&app, 0, 24);
        assert_ne!(cursor.spans[1].style, active.spans[1].style);
        assert_eq!(cursor.spans[1].style, app.theme.toc_cursor());
        assert_eq!(active.spans[1].style, app.theme.toc_active());
    }

    #[test]
    fn a_row_index_past_the_outline_still_produces_a_full_width_line() {
        let app = nested();
        let line = entry(&app, 999, 20);
        assert_eq!(measure::width(&text_of(&line)), 20);
    }
}
