//! The status bar: where the reader is, and what just happened.

use ratatui::Frame;
use ratatui::text::{Line, Span};

use crate::app::state::App;
use crate::render::{measure, tui};

/// Draw the status bar.
pub fn draw(frame: &mut Frame, app: &App) {
    let area = app.panes.status;
    if area.is_empty() {
        return;
    }
    let line = compose(app, area.width);
    tui::paint(frame.buffer_mut(), area, app.theme.status_bar());
    frame
        .buffer_mut()
        .set_line(area.x, area.y, &line, area.width);
}

/// Build the status line, exactly `width` cells wide.
///
/// Separated out so the composition — which part gives way when the terminal
/// is narrow — can be tested without a terminal.
#[must_use]
pub fn compose(app: &App, width: u16) -> Line<'static> {
    let theme = &app.theme;
    let total = usize::from(width);

    let (right, right_style) = match &app.message {
        Some(message) => (format!(" {message} "), theme.status_message()),
        None => (
            format!(" {}%  ? help ", app.view.progress(app.extent())),
            theme.status_bar(),
        ),
    };
    let right = measure::truncate(&right, total, "…");
    let right_width = measure::width(&right);

    // The document name is the last thing to give way; the section heading
    // yields first, since the document is still visible on screen above it.
    let budget = total - right_width;
    let name = measure::truncate(&format!(" {} ", app.doc.source.display_name), budget, "… ");
    let name_width = measure::width(&name);
    let section = app
        .active_heading()
        .map(|anchor| measure::truncate(&format!("› {} ", anchor.text), budget - name_width, "… "))
        .unwrap_or_default();
    let section_width = measure::width(&section);
    let gap = budget - name_width - section_width;

    Line::from(vec![
        Span::styled(name, theme.status_active()),
        Span::styled(section, theme.status_bar()),
        Span::styled(" ".repeat(gap), theme.status_bar()),
        Span::styled(right, right_style),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::Options;
    use crate::source::{Base, Source};
    use crate::theme::{Theme, ThemeVariant};
    use ratatui::layout::Rect;

    fn app(text: &str, name: &str) -> App {
        let mut app = App::new(
            Source::from_text(text, None, name.to_owned(), Base::Cwd),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        );
        crate::app::reconcile(&mut app, Rect::new(0, 0, 60, 24));
        app
    }

    fn text_of(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn the_status_line_is_exactly_the_width_it_was_asked_for() {
        let app = app("# Section One\n\nbody\n", "a-rather-long-document-name.md");
        for width in 1..90u16 {
            let line = compose(&app, width);
            assert_eq!(
                measure::width(&text_of(&line)),
                usize::from(width),
                "width {width}"
            );
        }
    }

    #[test]
    fn it_names_the_document_and_the_section_being_read() {
        let app = app("# Section One\n\nbody\n", "doc.md");
        let text = text_of(&compose(&app, 60));
        assert!(text.contains("doc.md"), "{text:?}");
        assert!(text.contains("Section One"), "{text:?}");
    }

    #[test]
    fn a_message_replaces_the_progress_readout() {
        let mut app = app("# T\n", "doc.md");
        app.message = Some("press q to quit".to_owned());
        let text = text_of(&compose(&app, 60));
        assert!(text.contains("press q to quit"), "{text:?}");
        assert!(!text.contains("help"), "{text:?}");
    }

    #[test]
    fn the_section_gives_way_before_the_document_name_does() {
        let app = app("# A Very Long Section Heading Indeed\n\nbody\n", "doc.md");
        let text = text_of(&compose(&app, 24));
        assert!(text.contains("doc.md"), "{text:?}");
    }
}
