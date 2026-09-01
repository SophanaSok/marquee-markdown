//! The status bar: where the reader is, and what just happened.

use ratatui::Frame;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::action::Action;
use crate::app::state::{App, Screen};
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
        // The sigil is what tells the browser's filter apart from the
        // document's search: both live on `/`, and a reader who cannot see
        // which one they are typing into has to guess.
        // A block stands in for the cursor: the real one is hidden, and a
        // prompt with no visible caret does not look like it is taking input.
        return (
            format!(" {}{}▏", prompt.kind.sigil(), prompt.input),
            app.theme.status_active(),
            String::new(),
        );
    }
    if app.screen == Screen::Browser {
        let root = app
            .browser
            .as_ref()
            .map(|browser| browser.root.display().to_string())
            .unwrap_or_default();
        return (
            format!(" {root} "),
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

/// The right of the bar while browsing: how many files, and whether the walk
/// is still turning up more.
fn browser_right_side(app: &App) -> (String, Style) {
    let Some(browser) = &app.browser else {
        return (String::new(), app.theme.status_bar());
    };
    let count = browser.len();
    let files = if count == 1 { "file" } else { "files" };
    if browser.scanning {
        // Say the count is still moving, so a reader does not act on a number
        // that is about to change.
        return (
            format!(" {count} {files} · looking… "),
            app.theme.status_message(),
        );
    }
    let total = browser.entries().len();
    let text = if count == total {
        format!(" {count} {files}{} ", help_tail(app))
    } else {
        format!(" {count} of {total} {files} ")
    };
    (text, app.theme.status_bar())
}

/// The right of the bar: a message, the search, or how far through we are.
fn right_side(app: &App) -> (String, Style) {
    if let Some(message) = &app.message {
        return (format!(" {message} "), app.theme.status_message());
    }
    if app.screen == Screen::Browser {
        return browser_right_side(app);
    }
    if let Some(prompt) = &app.prompt {
        // Matches narrow live while a search is typed, so the count is the
        // feedback; before anything is typed there is nothing to count yet.
        if prompt.kind == crate::app::state::PromptKind::Search && !prompt.input.is_empty() {
            let count = app.search.matches().len();
            let text = if count == 0 {
                " no matches ".to_owned()
            } else {
                format!(
                    " {}/{count} ",
                    app.search.current().map_or(0, |index| index + 1)
                )
            };
            return (text, app.theme.status_message());
        }
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
        format!(" {}%{} ", app.view.progress(app.extent()), help_tail(app)),
        app.theme.status_bar(),
    )
}

/// The `? help` the bar ends with, or nothing when the hint line above it is
/// already saying so.
///
/// The bar pointed at the key reference because nothing else did. The hint
/// line does now, and two of them a row apart reads as a stutter rather than
/// as emphasis — but only while the line is there and wide enough to have kept
/// the hint, so this asks rather than assumes.
fn help_tail(app: &App) -> &'static str {
    if app.hint_names(Action::ToggleHelp) {
        ""
    } else {
        "  ? help"
    }
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
    fn the_browser_names_the_directory_and_counts_what_is_in_it() {
        use crate::browser::Entry;
        let mut app = App::browsing(
            "/home/reader/notes".into(),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        );
        let browser = app.browser.as_mut().unwrap();
        browser.extend([Entry {
            path: "/home/reader/notes/a.md".into(),
            display: "a.md".into(),
            modified: None,
        }]);
        browser.scanning = false;
        crate::app::reconcile(&mut app, Rect::new(0, 0, 60, 24));

        let text = text_of(&compose(&app, 60));
        assert!(text.contains("/home/reader/notes"), "{text:?}");
        assert!(text.contains("1 file"), "{text:?}");
    }

    #[test]
    fn a_running_scan_says_the_count_is_not_final() {
        let mut app = App::browsing(
            "/notes".into(),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        );
        crate::app::reconcile(&mut app, Rect::new(0, 0, 60, 24));
        assert!(text_of(&compose(&app, 60)).contains("looking"));
    }

    #[test]
    fn a_filtered_list_says_how_much_it_is_hiding() {
        use crate::browser::Entry;
        let mut app = App::browsing(
            "/notes".into(),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        );
        let browser = app.browser.as_mut().unwrap();
        browser.extend((0..5).map(|n| Entry {
            path: format!("/notes/{n}.md").into(),
            display: format!("{n}.md"),
            modified: None,
        }));
        browser.scanning = false;
        browser.filter = "3".to_owned();
        crate::app::reconcile(&mut app, Rect::new(0, 0, 60, 24));
        assert!(text_of(&compose(&app, 60)).contains("1 of 5"));
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
    fn typing_a_search_shows_the_narrowing_count() {
        let mut app = app("needle one\n\nneedle two\n", "doc.md");
        app.prompt = Some(crate::app::state::Prompt {
            kind: crate::app::state::PromptKind::Search,
            input: "needle".to_owned(),
        });
        // The reconcile step is what feeds the live input to the matcher.
        crate::app::derived::sync(&mut app);
        let text = text_of(&compose(&app, 60));
        assert!(text.contains("1/2"), "{text:?}");

        app.prompt.as_mut().unwrap().input = "needle tw".to_owned();
        crate::app::derived::sync(&mut app);
        let text = text_of(&compose(&app, 60));
        assert!(text.contains("1/1"), "{text:?}");

        app.prompt.as_mut().unwrap().input = "zzz".to_owned();
        crate::app::derived::sync(&mut app);
        assert!(text_of(&compose(&app, 60)).contains("no matches"));
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

    /// The bar and the hint line are a row apart, and one `? help` between
    /// them is the point of having the line at all.
    #[test]
    fn the_bar_does_not_repeat_the_help_the_hint_line_is_already_offering() {
        let app = app("# T\n", "doc.md");
        assert!(app.panes.hints.is_some(), "the hint line is not on screen");
        let text = text_of(&compose(&app, 60));
        assert!(!text.contains("? help"), "said twice: {text:?}");
        assert!(
            text.contains('%'),
            "the progress readout went with it: {text:?}"
        );
    }

    #[test]
    fn the_bar_says_it_again_once_the_hint_line_is_gone() {
        let mut app = app("# T\n", "doc.md");
        app.hints = false;
        crate::app::reconcile(&mut app, Rect::new(0, 0, 60, 24));
        let text = text_of(&compose(&app, 60));
        assert!(text.contains("? help"), "{text:?}");
    }

    /// The help chip is fourth in the line, so a narrow terminal drops it —
    /// and the bar has to notice that rather than assume the line covers it.
    #[test]
    fn a_hint_line_too_narrow_to_offer_help_leaves_the_bar_saying_it() {
        for width in [20u16, 30, 40] {
            let mut app = app("# T\n", "doc.md");
            crate::app::reconcile(&mut app, Rect::new(0, 0, width, 24));
            let text = text_of(&compose(&app, width));
            assert!(
                !app.hint_names(crate::app::action::Action::ToggleHelp),
                "the hint line kept help at {width} columns"
            );
            assert!(text.contains("? help"), "at {width}: {text:?}");
        }
    }

    #[test]
    fn the_browser_bar_drops_its_help_for_the_same_reason() {
        use crate::browser::Entry;
        let mut app = App::browsing(
            "/notes".into(),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        );
        let browser = app.browser.as_mut().unwrap();
        browser.extend([Entry {
            path: "/notes/a.md".into(),
            display: "a.md".into(),
            modified: None,
        }]);
        browser.scanning = false;
        crate::app::reconcile(&mut app, Rect::new(0, 0, 60, 24));
        let text = text_of(&compose(&app, 60));
        assert!(text.contains("1 file"), "{text:?}");
        assert!(!text.contains("? help"), "said twice: {text:?}");

        app.hints = false;
        crate::app::reconcile(&mut app, Rect::new(0, 0, 60, 24));
        assert!(text_of(&compose(&app, 60)).contains("? help"));
    }

    #[test]
    fn the_section_gives_way_before_the_document_name_does() {
        let app = app("# A Very Long Section Heading Indeed\n\nbody\n", "doc.md");
        let text = text_of(&compose(&app, 24));
        assert!(text.contains("doc.md"), "{text:?}");
    }
}
