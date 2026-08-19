//! The file browser.
//!
//! Rows are sliced by hand rather than handed to a `List`: `ListState` mutates
//! its offset during render, which would put a `&mut App` in the draw path.
//! The scroll offset is settled before drawing, like everything else here.

use std::time::SystemTime;

use ratatui::Frame;
use ratatui::text::{Line, Span};

use crate::app::state::App;
use crate::browser::format;
use crate::render::{measure, tui};

/// Draw the file list.
pub fn draw(frame: &mut Frame, app: &App) {
    let area = app.panes.body;
    tui::paint(frame.buffer_mut(), area, app.theme.page());
    let Some(browser) = &app.browser else {
        return;
    };
    if area.is_empty() {
        return;
    }

    if browser.is_empty() {
        let line = empty_state(app, area.width);
        frame
            .buffer_mut()
            .set_line(area.x, area.y, &line, area.width);
        return;
    }

    let now = SystemTime::now();
    for row in 0..area.height {
        let position = browser.offset + usize::from(row);
        if position >= browser.len() {
            break;
        }
        let line = entry(app, position, area.width, now);
        frame
            .buffer_mut()
            .set_line(area.x, area.y + row, &line, area.width);
    }
}

/// What to say when there is nothing to list.
///
/// The three cases are genuinely different — still looking, nothing here, and
/// nothing matching — and telling them apart is the difference between a
/// reader waiting and a reader retyping their filter.
#[must_use]
pub fn empty_state(app: &App, width: u16) -> Line<'static> {
    let browser = app.browser.as_ref();
    let text = match browser {
        Some(browser) if browser.scanning => " looking…".to_owned(),
        Some(browser) if !browser.filter.is_empty() || app.prompt.is_some() => {
            " nothing matches".to_owned()
        }
        _ => " no markdown files here".to_owned(),
    };
    pad(
        &[(text, app.theme.list_meta())],
        width,
        app.theme.list_meta(),
    )
}

/// One row, exactly `width` cells wide.
#[must_use]
pub fn entry(app: &App, position: usize, width: u16, now: SystemTime) -> Line<'static> {
    let theme = &app.theme;
    let Some(browser) = &app.browser else {
        return pad(&[], width, theme.list_item());
    };
    let Some(entry) = browser.entry_at(position) else {
        return pad(&[], width, theme.list_item());
    };

    let selected = position == browser.cursor();
    let base = if selected {
        theme.list_cursor()
    } else {
        theme.list_item()
    };
    let meta = if selected { base } else { theme.list_meta() };
    let marker = if selected { "▎" } else { " " };
    let marker_style = if selected {
        theme.list_cursor_marker()
    } else {
        theme.list_item()
    };

    let total = usize::from(width);
    if total <= 1 {
        return Line::from(Span::styled(marker.to_owned(), marker_style));
    }

    // The age is worth showing only when the name has not already used up the
    // row; a truncated filename with a timestamp beside it helps nobody.
    let age = entry
        .modified
        .map(|modified| format::relative_time(modified, now))
        .unwrap_or_default();
    let age_room = measure::width(&age) + 2;
    let name_room = if total > age_room + 12 {
        total - 1 - age_room
    } else {
        total - 1
    };

    let name = measure::truncate(&entry.display, name_room, "…");
    let mut spans = vec![
        (marker.to_owned(), marker_style),
        (" ".to_owned(), base),
        (name, base),
    ];
    if name_room < total - 1 {
        let used: usize = spans.iter().map(|(text, _)| measure::width(text)).sum();
        spans.push((" ".repeat(total - used - measure::width(&age) - 1), base));
        spans.push((age, meta));
    }
    pad(&spans, width, base)
}

/// Assemble spans into a line of exactly `width` cells.
///
/// The single place this widget's width discipline lives: callers work out
/// what they want to say, and this makes it fit whatever the pane actually is.
fn pad(
    spans: &[(String, ratatui::style::Style)],
    width: u16,
    fill: ratatui::style::Style,
) -> Line<'static> {
    let total = usize::from(width);
    let mut used = 0;
    let mut out: Vec<Span<'static>> = Vec::with_capacity(spans.len() + 1);
    for (text, style) in spans {
        if used >= total {
            break;
        }
        let text = measure::truncate(text, total - used, "…");
        used += measure::width(&text);
        if !text.is_empty() {
            out.push(Span::styled(text, *style));
        }
    }
    if used < total {
        out.push(Span::styled(" ".repeat(total - used), fill));
    }
    Line::from(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{Options, Prompt, PromptKind};
    use crate::browser::Entry;
    use crate::theme::{Theme, ThemeVariant};
    use ratatui::layout::Rect;
    use std::path::PathBuf;
    use std::time::Duration;

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000)
    }

    fn app() -> App {
        let mut app = App::browsing(
            PathBuf::from("/root"),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        );
        let browser = app.browser.as_mut().expect("a browser");
        browser.extend([
            Entry {
                path: "/root/README.md".into(),
                display: "README.md".into(),
                modified: Some(now() - Duration::from_secs(7_200)),
            },
            Entry {
                path: "/root/docs/a-very-long-document-name-indeed.md".into(),
                display: "docs/a-very-long-document-name-indeed.md".into(),
                modified: Some(now() - Duration::from_secs(60)),
            },
        ]);
        browser.scanning = false;
        crate::app::reconcile(&mut app, Rect::new(0, 0, 60, 20));
        app
    }

    fn text_of(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn a_row_is_exactly_the_pane_width() {
        let app = app();
        for width in 1..80u16 {
            for position in 0..app.browser.as_ref().unwrap().len() {
                let line = entry(&app, position, width, now());
                assert_eq!(
                    measure::width(&text_of(&line)),
                    usize::from(width),
                    "row {position} at width {width}"
                );
            }
            assert_eq!(
                measure::width(&text_of(&empty_state(&app, width))),
                usize::from(width)
            );
        }
    }

    #[test]
    fn a_row_shows_the_name_and_how_old_it_is() {
        let text = text_of(&entry(&app(), 1, 60, now()));
        assert!(text.contains("README.md"), "{text:?}");
        assert!(text.contains("2h ago"), "{text:?}");
    }

    #[test]
    fn a_narrow_pane_drops_the_age_rather_than_the_name() {
        let text = text_of(&entry(&app(), 1, 16, now()));
        assert!(text.contains("READ"), "{text:?}");
        assert!(!text.contains("ago"), "{text:?}");
    }

    #[test]
    fn the_cursor_row_is_marked() {
        let app = app();
        assert!(text_of(&entry(&app, 0, 40, now())).contains('▎'));
        assert!(!text_of(&entry(&app, 1, 40, now())).contains('▎'));
    }

    #[test]
    fn a_long_name_is_truncated_rather_than_wrapped() {
        let text = text_of(&entry(&app(), 0, 30, now()));
        assert!(text.contains('…'), "{text:?}");
    }

    #[test]
    fn the_empty_state_says_which_kind_of_empty_it_is() {
        let mut app = app();
        app.browser.as_mut().unwrap().scanning = true;
        assert!(text_of(&empty_state(&app, 40)).contains("looking"));

        app.browser.as_mut().unwrap().scanning = false;
        assert!(text_of(&empty_state(&app, 40)).contains("no markdown files"));

        app.prompt = Some(Prompt {
            kind: PromptKind::Filter,
            input: "zzz".to_owned(),
        });
        assert!(text_of(&empty_state(&app, 40)).contains("nothing matches"));
    }
}
