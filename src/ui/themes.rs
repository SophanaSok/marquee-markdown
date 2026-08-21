//! The theme picker.
//!
//! A list of every theme the registry can find, with the cursor previewing as
//! it moves — so the panel is deliberately narrow and off to one side of the
//! screen's middle would be wrong: what the reader is judging is the document
//! behind it, and the panel only has to stay out of the way while they do.
//!
//! The scroll offset is derived from the cursor rather than stored. There is
//! only ever one right answer — the offset that keeps the cursor on screen —
//! so computing it here means it cannot fall out of step with the cursor.

use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::app::state::{App, ThemePicker};
use crate::render::measure;
use crate::theme::{Appearance, registry::Origin};

/// Columns between the columns of a row.
const GAP: usize = 2;

/// Draw the theme picker, if it is open.
pub fn draw(frame: &mut Frame, app: &App) {
    let Some(picker) = app.picker.as_ref() else {
        return;
    };
    if picker.entries.is_empty() {
        return;
    }

    let rows: Vec<Row> = picker.entries.iter().map(|e| row(picker, e)).collect();
    let name_width = rows
        .iter()
        .map(|r| measure::width(&r.name))
        .max()
        .unwrap_or(0);
    let meta_width = rows
        .iter()
        .map(|r| measure::width(&r.meta))
        .max()
        .unwrap_or(0);
    // Marker, a space, the name, a gap, then where it came from.
    let widest = 2 + name_width + GAP + meta_width;

    let area = super::centered(
        frame.area(),
        u16::try_from(widest + 4).unwrap_or(u16::MAX),
        u16::try_from(rows.len() + 2).unwrap_or(u16::MAX),
    );
    if area.width < 4 || area.height < 3 {
        return;
    }

    let theme = &app.theme;
    let visible = usize::from(area.height - 2);
    let offset = offset(picker.cursor, visible, rows.len());
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.overlay_border())
        .style(theme.overlay_body())
        .title(Span::styled(
            title(picker, offset, visible, rows.len()),
            theme.overlay_title(),
        ));
    let inner = block.inner(area);

    let lines: Vec<Line<'static>> = rows
        .iter()
        .skip(offset)
        .take(visible)
        .enumerate()
        .map(|(index, row)| line(app, row, offset + index == picker.cursor, name_width))
        .collect();

    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines).style(theme.overlay_body()), inner);
}

/// One theme, reduced to the two strings the panel shows.
struct Row {
    name: String,
    meta: String,
}

/// What to say about a theme beside its name.
fn row(picker: &ThemePicker, entry: &crate::theme::registry::Entry) -> Row {
    let meta = if picker.failed.contains(&entry.name) {
        // Said on the row as well as in the status bar, because the status bar
        // is about the last key pressed and this is about the theme.
        "unreadable".to_owned()
    } else {
        let origin = match entry.origin {
            Origin::BuiltIn => "built-in",
            Origin::User(_) => "yours",
            Origin::Terminal => "your terminal",
        };
        match appearance(entry) {
            Some(Appearance::Light) => format!("light · {origin}"),
            Some(Appearance::Dark) => format!("dark · {origin}"),
            // A theme that has not been loaded yet says only where it is from;
            // loading every theme to fill in a column would put the whole
            // theme directory through the parser to open a list.
            None => origin.to_owned(),
        }
    };
    Row {
        name: entry.name.clone(),
        meta,
    }
}

/// Whether a theme is light or dark, when that is known without reading a file.
fn appearance(entry: &crate::theme::registry::Entry) -> Option<Appearance> {
    match entry.origin {
        Origin::BuiltIn => entry
            .name
            .parse::<crate::theme::ThemeVariant>()
            .ok()
            .map(|variant| variant.definition().appearance),
        // `system` is light or dark depending on what the terminal answered,
        // which the list does not carry; the row says where it came from and
        // the preview says the rest.
        Origin::User(_) | Origin::Terminal => None,
    }
}

/// One row, styled for whether the cursor is on it.
fn line(app: &App, row: &Row, is_cursor: bool, name_width: usize) -> Line<'static> {
    let theme = &app.theme;
    let (marker, name_style) = if is_cursor {
        ("▎", theme.overlay_key())
    } else {
        (" ", theme.overlay_body())
    };
    let pad = name_width.saturating_sub(measure::width(&row.name));

    Line::from(vec![
        Span::styled(marker.to_owned(), theme.overlay_key()),
        Span::styled(" ".to_owned(), theme.overlay_body()),
        Span::styled(row.name.clone(), name_style),
        Span::styled(" ".repeat(pad + GAP), theme.overlay_body()),
        Span::styled(row.meta.clone(), theme.overlay_meta()),
    ])
}

/// The first row to show, so that `cursor` is on screen.
fn offset(cursor: usize, visible: usize, total: usize) -> usize {
    if visible == 0 || total <= visible {
        return 0;
    }
    // Keep the cursor in view, scrolling by as little as gets it there.
    cursor
        .saturating_sub(visible - 1)
        .min(total.saturating_sub(visible))
}

/// The panel's title: what it is, and how to leave it.
fn title(picker: &ThemePicker, offset: usize, visible: usize, total: usize) -> String {
    if visible < total {
        format!(
            " themes \u{b7} {}\u{2013}{} of {total} \u{b7} enter \u{b7} esc ",
            offset + 1,
            (offset + visible).min(total),
        )
    } else if picker.entries.len() == 1 {
        // Nothing to move between, so offering the movement keys would be
        // telling the reader about a list they have not got.
        " themes \u{b7} esc ".to_owned()
    } else {
        " themes \u{b7} enter \u{b7} esc ".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::Overlay;
    use crate::app::update;
    use crate::theme::registry::Entry;

    fn picker(names: &[&str], cursor: usize) -> ThemePicker {
        ThemePicker {
            entries: names
                .iter()
                .map(|name| Entry {
                    name: (*name).to_owned(),
                    origin: Origin::BuiltIn,
                })
                .collect(),
            cursor,
            restore: crate::theme::Theme::new(crate::theme::ThemeVariant::Slate),
            failed: Vec::new(),
        }
    }

    #[test]
    fn a_list_that_fits_is_never_scrolled() {
        assert_eq!(offset(0, 10, 4), 0);
        assert_eq!(offset(3, 10, 4), 0);
    }

    #[test]
    fn the_cursor_stays_on_screen_as_it_moves_down() {
        // Three rows visible out of ten: the cursor sits on the last one.
        assert_eq!(offset(0, 3, 10), 0);
        assert_eq!(offset(2, 3, 10), 0);
        assert_eq!(offset(3, 3, 10), 1);
        assert_eq!(offset(9, 3, 10), 7);
    }

    #[test]
    fn the_offset_never_runs_past_the_end() {
        assert_eq!(offset(9, 3, 10) + 3, 10);
        assert_eq!(offset(0, 0, 10), 0);
    }

    #[test]
    fn a_built_in_says_whether_it_is_light_or_dark() {
        let entry = Entry {
            name: "paper".to_owned(),
            origin: Origin::BuiltIn,
        };
        assert_eq!(appearance(&entry), Some(Appearance::Light));
        assert_eq!(row(&picker(&[], 0), &entry).meta, "light · built-in");
    }

    #[test]
    fn a_theme_that_would_not_load_says_so_on_its_row() {
        let mut picker = picker(&["broken"], 0);
        picker.failed.push("broken".to_owned());
        assert_eq!(row(&picker, &picker.entries[0]).meta, "unreadable");
    }

    #[test]
    fn the_title_says_how_to_leave() {
        let picker = picker(&["paper", "slate"], 0);
        assert!(title(&picker, 0, 2, 2).contains("esc"));
        assert!(title(&picker, 0, 2, 2).contains("enter"));
    }

    #[test]
    fn a_scrolling_title_says_where_in_the_list_it_is() {
        let picker = picker(&["a", "b", "c", "d"], 3);
        let title = title(&picker, 2, 2, 4);
        assert!(title.contains("3\u{2013}4 of 4"), "{title}");
    }

    /// The panel is drawn from `App`, so the smallest terminals must not panic
    /// and must not draw a frame with no room inside it.
    #[test]
    fn a_terminal_with_no_room_draws_nothing_rather_than_panicking() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::app::state::App::new(
            crate::source::Source::from_text(
                "# heading\n\nbody\n",
                None,
                "t.md".to_owned(),
                crate::source::Base::Cwd,
            ),
            crate::theme::Theme::new(crate::theme::ThemeVariant::Slate),
            crate::app::state::Options::default(),
        );
        update::handle(
            &mut app,
            crate::app::event::Event::Key(crossterm::event::KeyEvent::from(
                crossterm::event::KeyCode::Char('s'),
            )),
        );
        assert_eq!(app.overlay, Some(Overlay::Themes));

        for (width, height) in [(1, 1), (2, 3), (4, 3), (20, 6), (80, 24)] {
            let mut terminal =
                Terminal::new(TestBackend::new(width, height)).expect("test terminal");
            terminal
                .draw(|frame| draw(frame, &app))
                .expect("draw the picker");
        }
    }
}
