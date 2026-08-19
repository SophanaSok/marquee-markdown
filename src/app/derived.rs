//! State that is recomputed rather than tracked.
//!
//! Runs once per iteration, after the update and before drawing. Everything
//! here is a function of the document, the view, and the pane sizes; putting
//! it in one place is what keeps drawing free of mutation.

use super::state::App;

/// Bring derived state back in line with the document and the view.
pub fn sync(app: &mut App) {
    let extent = app.extent();
    app.view.clamp(extent);
    // The active section is scroll-derived and never written back anywhere:
    // scrolling moves the highlight without disturbing a cursor the reader
    // placed themselves.
    app.active = app.doc.doc().active_anchor(app.view.top);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::Options;
    use crate::render::LayoutOptions;
    use crate::source::{Base, Source};
    use crate::theme::{Theme, ThemeVariant};
    use ratatui::layout::Rect;

    fn app_with(text: &str, height: u16) -> App {
        let mut app = App::new(
            Source::from_text(text, None, "t.md".into(), Base::Cwd),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        );
        app.panes = crate::app::layout::compute(Rect::new(0, 0, 60, height), &app);
        let options = LayoutOptions {
            width: app.panes.content_width,
            code_line_numbers: false,
        };
        app.view.top = app.doc.ensure_rendered(options, &app.theme, 0);
        app
    }

    /// Long enough that the second heading can actually be scrolled to; a
    /// document that fits on screen is clamped to the top and would make this
    /// test pass for the wrong reason.
    fn two_sections() -> String {
        let filler: String = (1..=40).map(|n| format!("body line {n}\n\n")).collect();
        format!("# One\n\n{filler}# Two\n\n{filler}")
    }

    #[test]
    fn the_active_section_follows_the_scroll_position() {
        let text = two_sections();
        let mut app = app_with(&text, 10);
        sync(&mut app);
        assert_eq!(
            app.active_heading().map(|a| a.id.clone()).as_deref(),
            Some("one")
        );

        let second = app.doc.doc().outline[1].line;
        app.view.top = second;
        sync(&mut app);
        assert_eq!(
            app.active_heading().map(|a| a.id.clone()).as_deref(),
            Some("two")
        );
    }

    #[test]
    fn a_position_past_the_end_is_pulled_back_before_drawing() {
        let mut app = app_with("# One\n\nbody\n", 10);
        app.view.top = 9_000;
        sync(&mut app);
        assert_eq!(app.view.top, app.extent().max_top());
    }
}
