//! Drawing. Every function here takes `&App` and returns nothing.
//!
//! The reader's house rule is that rendering is pure: widgets are derived from
//! state, and nothing is computed for the first time during a draw. Pane sizes
//! and the active section are already settled by the time this runs, so a
//! frame can be produced twice with identical results — which is what makes
//! the snapshot tests meaningful.

pub mod browser;
pub mod document;
pub mod help;
pub mod hints;
pub mod status;
pub mod themes;
pub mod toc;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Clear;

use crate::app::state::{App, Overlay, Screen};

/// Draw one frame.
pub fn draw(frame: &mut Frame, app: &App) {
    match app.screen {
        Screen::Browser => browser::draw(frame, app),
        Screen::Document => {
            document::draw(frame, app);
            toc::draw(frame, app);
        }
    }
    hints::draw(frame, app);
    status::draw(frame, app);
    match app.overlay {
        Some(Overlay::Help) => help::draw(frame, app),
        Some(Overlay::Themes) => themes::draw(frame, app),
        None => {}
    }
}

/// Clear a panel's area, and repair the double-width glyph it just cut in half.
///
/// A panel's left edge can land on the second cell of a wide glyph in the
/// document behind it. The first cell is inside the panel and gets painted
/// over; the second is one column outside and belongs to nobody — the layout
/// wrote one glyph across two cells, and ratatui leaves the second as the
/// backend found it, because the glyph to its left was supposed to cover it.
/// Overwrite that glyph and the cell is orphaned: an unpainted hole showing
/// the terminal's own background, one column tall, hard against the panel.
///
/// It has to be measured *before* [`Clear`] runs, because clearing is what
/// destroys the evidence. Nothing drawn afterwards reaches past `area`, so
/// resealing here is safe: the block and its contents stay inside it.
///
/// `page` is what to paint the repaired cell with — the surface the document
/// behind the panel is drawn on.
pub(crate) fn clear_panel(frame: &mut Frame, area: Rect, page: Style) {
    let edge = area.right();
    let orphans: Vec<u16> = if edge < frame.area().right() {
        let buf = frame.buffer_mut();
        (area.top()..area.bottom().min(buf.area.bottom()))
            .filter(|&y| crate::render::measure::width(buf[(edge - 1, y)].symbol()) == 2)
            .collect()
    } else {
        Vec::new()
    };

    frame.render_widget(Clear, area);

    let buf = frame.buffer_mut();
    for y in orphans {
        let cell = &mut buf[(edge, y)];
        cell.set_symbol(" ");
        cell.set_style(page);
    }
}

/// A rectangle of at most `width` by `height`, centered in `area`.
///
/// Shared by the overlay panels so they agree about where the middle is.
#[must_use]
pub(crate) fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    /// The panel's right edge bisecting a wide glyph must not leave a hole.
    ///
    /// Reproduced directly rather than through a document: the app-level frame
    /// tests only catch this when the fixture happens to put a wide glyph
    /// exactly where the panel edge lands, which changes whenever a panel
    /// changes width — shipping a few themes was enough to move it.
    #[test]
    fn clearing_a_panel_reseals_the_glyph_its_edge_cut_in_half() {
        let page = Style::default().bg(Color::Blue);
        let panel = Rect::new(0, 0, 10, 3);
        let mut terminal = Terminal::new(TestBackend::new(20, 3)).expect("test terminal");
        terminal
            .draw(|frame| {
                let area = frame.area();
                let buf = frame.buffer_mut();
                for y in 0..area.height {
                    for x in 0..area.width {
                        buf[(x, y)].set_symbol(" ").set_style(page);
                    }
                    // A double-width glyph straddling the panel's edge: its
                    // first cell is the last column the panel covers, its
                    // second cell the first column outside.
                    buf[(panel.right() - 1, y)].set_symbol("\u{4f60}");
                    buf[(panel.right(), y)]
                        .set_symbol("")
                        .set_style(Style::reset());
                }
                clear_panel(frame, panel, page);
            })
            .expect("draw succeeds");

        let buf = terminal.backend().buffer();
        for y in 0..3 {
            let cell = &buf[(panel.right(), y)];
            assert_eq!(
                cell.style().bg,
                Some(Color::Blue),
                "orphaned half at row {y} was left unpainted"
            );
            assert!(!cell.symbol().is_empty(), "row {y} has no symbol");
        }
    }

    /// The common case: no wide glyph, nothing to repair, nothing touched.
    #[test]
    fn clearing_a_panel_leaves_its_neighbour_alone() {
        let page = Style::default().bg(Color::Blue);
        let outside = Style::default().bg(Color::Green);
        let panel = Rect::new(0, 0, 10, 1);
        let mut terminal = Terminal::new(TestBackend::new(20, 1)).expect("test terminal");
        terminal
            .draw(|frame| {
                let buf = frame.buffer_mut();
                for x in 0..20 {
                    buf[(x, 0)].set_symbol("x").set_style(outside);
                }
                clear_panel(frame, panel, page);
            })
            .expect("draw succeeds");

        let cell = &terminal.backend().buffer()[(panel.right(), 0)];
        assert_eq!(cell.style().bg, Some(Color::Green));
        assert_eq!(cell.symbol(), "x");
    }

    /// A panel flush with the right edge has no neighbour to repair.
    #[test]
    fn a_panel_against_the_right_edge_is_handled() {
        let page = Style::default().bg(Color::Blue);
        let mut terminal = Terminal::new(TestBackend::new(10, 1)).expect("test terminal");
        terminal
            .draw(|frame| clear_panel(frame, frame.area(), page))
            .expect("draw succeeds");
        assert_eq!(terminal.backend().buffer().area.width, 10);
    }
}
