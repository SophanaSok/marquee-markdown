//! The status bar: where the reader is, and what just happened.

use ratatui::Frame;
use ratatui::style::Style;
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

    let (right, right_style) = right_side(app);
    let right = measure::truncate(&right, total, "…");
    let right_width = measure::width(&right);
    let budget = total - right_width;

    let (left, left_style, trailing) = left_side(app);
    let left = measure::truncate(&left, budget, "… ");
    let left_width = measure::width(&left);
    let trailing = measure::truncate(&trailing, budget - left_width, "… ");
    let trailing_width = measure::width(&trailing);
    let gap = budget - left_width - trailing_width;

    Line::from(vec![
        Span::styled(left, left_style),
        Span::styled(trailing, theme.status_bar()),
        Span::styled(" ".repeat(gap), theme.status_bar()),
        Span::styled(right, right_style),
    ])
}

/// The left of the bar: what is being typed, or where the reader is.
///
/// The document name is the last thing to give way; the section heading yields
/// first, since the document itself is still on screen above it.
fn left_side(app: &App) -> (String, Style, String) {
    if let Some(prompt) = &app.prompt {
        // A block stands in for the cursor: the real one is hidden, and a
        // prompt with no visible caret does not look like it is taking input.
        return (
            format!(" {}{}▏", prompt.kind.sigil(), prompt.input),
            app.theme.status_active(),
            String::new(),
        );
    }
    let section = app
        .active_heading()
        .map(|anchor| format!("› {} ", anchor.text))
        .unwrap_or_default();
    (
        format!(" {} ", app.doc.source.display_name),
        app.theme.status_active(),
        section,
    )
}

/// The right of the bar: a message, the search, or how far through we are.
fn right_side(app: &App) -> (String, Style) {
    if let Some(message) = &app.message {
        return (format!(" {message} "), app.theme.status_message());
    }
    if app.prompt.is_some() {
        return (" enter to search ".to_owned(), app.theme.status_bar());
    }
    if app.search.is_active() {
        let count = app.search.matches().len();
        let text = if count == 0 {
            format!(" /{} no matches ", app.search.query())
        } else {
            format!(
                " /{} {}/{count} ",
                app.search.query(),
                app.search.current().map_or(0, |index| index + 1)
            )
        };
        return (text, app.theme.status_message());
    }
    (
        format!(" {}%  ? help ", app.view.progress(app.extent())),
        app.theme.status_bar(),
    )
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
    fn an_open_prompt_shows_what_is_being_typed() {
        let mut app = app("# T\n", "doc.md");
        app.prompt = Some(crate::app::state::Prompt {
            kind: crate::app::state::PromptKind::Search,
            input: "needle".to_owned(),
        });
        let text = text_of(&compose(&app, 60));
        assert!(text.contains("/needle"), "{text:?}");
        assert!(
            !text.contains("doc.md"),
            "the prompt shares the bar: {text:?}"
        );
    }

    #[test]
    fn an_active_search_shows_which_hit_is_selected() {
        let mut app = app("needle one\n\nneedle two\n", "doc.md");
        app.search
            .search(app.doc.doc(), app.doc.revision(), "needle", 0);
        let text = text_of(&compose(&app, 60));
        assert!(text.contains("/needle"), "{text:?}");
        assert!(text.contains("1/2"), "{text:?}");
    }

    #[test]
    fn a_search_with_no_hits_says_so() {
        let mut app = app("nothing\n", "doc.md");
        app.search
            .search(app.doc.doc(), app.doc.revision(), "absent", 0);
        assert!(text_of(&compose(&app, 60)).contains("no matches"));
    }

    #[test]
    fn the_section_gives_way_before_the_document_name_does() {
        let app = app("# A Very Long Section Heading Indeed\n\nbody\n", "doc.md");
        let text = text_of(&compose(&app, 24));
        assert!(text.contains("doc.md"), "{text:?}");
    }
}
