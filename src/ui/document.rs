//! The document pane.

use ratatui::Frame;

use crate::app::state::App;
use crate::render::overlay::Layered;
use crate::render::tui;

/// Draw the visible slice of the document, centered, on a painted page, with
/// the selected link and any search hits picked out.
///
/// The highlights are overlays applied on the way to the buffer, so neither
/// searching nor stepping through links re-lays anything out, and every line
/// index the application holds stays valid.
pub fn draw(frame: &mut Frame, app: &App) {
    let links = app.links.overlay(app.theme.link_active());
    let search = app
        .search
        .overlay(app.theme.search_match(), app.theme.search_current());
    // The link the reader stepped to wins over a search hit underneath it:
    // they moved to it deliberately, and losing sight of it would make the
    // step look like it did nothing.
    //
    // Bound to a local rather than written inline: the temporary array would
    // otherwise be dropped at the end of the statement, which newer compilers
    // forgive and the minimum supported one does not.
    let layers: [&dyn crate::render::overlay::Overlay; 2] = [&links, &search];
    let overlay = Layered(&layers);
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
