//! The document pane.

use ratatui::Frame;

use crate::app::state::App;
use crate::render::tui;

/// Draw the visible slice of the document, centered, on a painted page.
pub fn draw(frame: &mut Frame, app: &App) {
    let area = app.panes.body;
    tui::render(
        frame.buffer_mut(),
        area,
        app.doc.doc(),
        app.view.top,
        app.view.left,
        app.theme.page(),
    );
}
