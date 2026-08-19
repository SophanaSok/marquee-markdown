//! The document pane.

use ratatui::Frame;

use crate::app::state::App;
use crate::render::tui;

/// Draw the visible slice of the document, centered, on a painted page, with
/// search hits highlighted.
///
/// The highlight is an overlay applied on the way to the buffer, so searching
/// changes nothing about the layout and every line index the application holds
/// stays valid.
pub fn draw(frame: &mut Frame, app: &App) {
    let overlay = app
        .search
        .overlay(app.theme.search_match(), app.theme.search_current());
    tui::render(
        frame.buffer_mut(),
        app.panes.body,
        app.doc.doc(),
        app.view.top,
        app.view.left,
        app.theme.page(),
        &overlay,
    );
}
