//! What a drawn frame must be true of, at any terminal size.
//!
//! The reader paints a page rather than writing text onto the terminal's own
//! background, which only holds if every cell in the frame is written. A gap
//! shows up as a stripe of the user's shell colors through the middle of the
//! document — the kind of defect that is obvious on screen and invisible to a
//! test that only inspects text.

use marquee_markdown::app::{App, Options, reconcile};
use marquee_markdown::render::measure;
use marquee_markdown::source::{Base, Source};
use marquee_markdown::theme::system::{self, TerminalColors};
use marquee_markdown::theme::{Rgb, Theme, ThemeVariant};
use marquee_markdown::ui;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

const SIZES: &[(u16, u16)] = &[
    (80, 24),
    (120, 40),
    (60, 20),
    (40, 10),
    (20, 6),
    (10, 3),
    (1, 1),
];

fn fixture() -> App {
    let text = include_str!("fixtures/kitchen-sink.md");
    App::new(
        Source::from_text(
            text,
            Some("kitchen-sink.md".into()),
            "kitchen-sink.md".into(),
            Base::Cwd,
        ),
        Theme::new(ThemeVariant::Slate),
        Options::default(),
    )
}

fn frame(app: &mut App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    reconcile(app, Rect::new(0, 0, width, height));
    terminal
        .draw(|frame| ui::draw(frame, app))
        .expect("draw succeeds");
    terminal.backend().buffer().clone()
}

/// Whether this cell is the right half of a double-width glyph.
///
/// Nothing is ever drawn there — the frame diff skips it, because the glyph in
/// the cell to the left already covers it — so it is the one cell that is
/// legitimately left as the backend found it.
fn is_covered_by_a_wide_glyph(buf: &Buffer, x: u16, y: u16) -> bool {
    x > buf.area.left() && measure::width(buf[(x - 1, y)].symbol()) == 2
}

fn assert_fully_painted(buf: &Buffer, what: &str) {
    for y in buf.area.top()..buf.area.bottom() {
        for x in buf.area.left()..buf.area.right() {
            if is_covered_by_a_wide_glyph(buf, x, y) {
                continue;
            }
            let cell = &buf[(x, y)];
            assert_ne!(
                cell.style().bg,
                Some(Color::Reset),
                "{what}: unpainted cell at ({x}, {y})"
            );
            assert!(
                cell.style().bg.is_some(),
                "{what}: cell at ({x}, {y}) has no background"
            );
            assert!(
                !cell.symbol().is_empty(),
                "{what}: cell at ({x}, {y}) has no symbol"
            );
        }
    }
}

/// A reader browsing a directory, with results already in.
fn browsing() -> App {
    use marquee_markdown::browser::Entry;
    let mut app = App::browsing(
        "/notes".into(),
        Theme::new(ThemeVariant::Slate),
        Options::default(),
    );
    let browser = app.browser.as_mut().expect("a browser");
    browser.extend((0..40).map(|n| Entry {
        path: format!("/notes/file-{n:02}.md").into(),
        display: format!("file-{n:02}.md"),
        modified: Some(std::time::SystemTime::UNIX_EPOCH),
    }));
    browser.scanning = false;
    app
}

#[test]
fn every_cell_of_the_browser_is_painted_at_every_size() {
    for &(width, height) in SIZES {
        let mut app = browsing();
        let buf = frame(&mut app, width, height);
        assert_fully_painted(&buf, &format!("browser at {width}x{height}"));
    }
}

#[test]
fn the_browser_never_shows_a_contents_pane() {
    // It has no document to list the headings of.
    let mut app = browsing();
    frame(&mut app, 100, 30);
    assert!(app.panes.sidebar.is_none());
}

#[test]
fn an_empty_browser_is_still_a_painted_screen() {
    let mut app = App::browsing(
        "/notes".into(),
        Theme::new(ThemeVariant::Slate),
        Options::default(),
    );
    let buf = frame(&mut app, 80, 24);
    assert_fully_painted(&buf, "empty browser");
}

#[test]
fn every_cell_is_painted_at_every_size() {
    for &(width, height) in SIZES {
        let mut app = fixture();
        let buf = frame(&mut app, width, height);
        assert_fully_painted(&buf, &format!("{width}x{height}"));
    }
}

#[test]
fn every_cell_is_painted_with_the_help_overlay_open() {
    for &(width, height) in SIZES {
        let mut app = fixture();
        app.overlay = Some(marquee_markdown::app::Overlay::Help);
        let buf = frame(&mut app, width, height);
        assert_fully_painted(&buf, &format!("help at {width}x{height}"));
    }
}

#[test]
fn a_scrolled_key_reference_stays_painted_on_a_short_terminal() {
    let mut app = fixture();
    app.overlay = Some(marquee_markdown::app::Overlay::Help);
    app.help_scroll = 10;
    let buf = frame(&mut app, 60, 12);
    assert_fully_painted(&buf, "scrolled help at 60x12");
}

/// Every theme a frame has to be fully painted in.
///
/// The shipped two, and one built from a terminal's own colors — a palette
/// somebody else's colorscheme chose. The page is painted edge to edge in all
/// of them or in none of them.
fn themes() -> Vec<(String, Theme)> {
    let terminal = TerminalColors {
        fg: Some(Rgb(0xd8, 0xd8, 0xd8)),
        bg: Some(Rgb(0x18, 0x18, 0x18)),
        ..TerminalColors::UNKNOWN
    };
    let mut out: Vec<(String, Theme)> = [ThemeVariant::Paper, ThemeVariant::Slate]
        .into_iter()
        .map(|variant| (variant.name().to_owned(), Theme::new(variant)))
        .collect();
    out.push((
        "system".to_owned(),
        system::theme(&terminal).expect("theme"),
    ));
    out
}

#[test]
fn the_theme_picker_paints_every_cell_at_every_size() {
    for &(width, height) in SIZES {
        for (name, theme) in themes() {
            let mut app = fixture();
            app.theme = theme;
            // Through the update loop rather than by hand, so the panel is
            // drawn from a picker the reader could actually have opened.
            marquee_markdown::app::update::handle(
                &mut app,
                marquee_markdown::app::event::Event::Key(crossterm::event::KeyEvent::from(
                    crossterm::event::KeyCode::Char('s'),
                )),
            );
            assert_eq!(app.overlay, Some(marquee_markdown::app::Overlay::Themes));
            let buf = frame(&mut app, width, height);
            assert_fully_painted(&buf, &format!("theme picker at {width}x{height} in {name}"));
        }
    }
}

#[test]
fn every_cell_is_painted_in_every_theme() {
    for (name, theme) in themes() {
        let mut app = fixture();
        app.theme = theme;
        let buf = frame(&mut app, 80, 24);
        assert_fully_painted(&buf, &name);
    }
}

#[test]
fn the_status_bar_occupies_the_last_row_and_all_of_it() {
    let mut app = fixture();
    app.hints = false;
    let buf = frame(&mut app, 80, 24);
    let expected = app.theme.status_bar().bg;
    for x in 0..80 {
        assert_eq!(
            buf[(x, 23)].style().bg,
            expected,
            "status bar at column {x}"
        );
    }
    // And the row above it is document, not status bar.
    assert_eq!(buf[(0, 22)].style().bg, app.theme.page().bg);
}

#[test]
fn the_hint_line_takes_the_row_above_the_status_bar_and_all_of_it() {
    let mut app = fixture();
    let buf = frame(&mut app, 80, 24);
    let row = app.panes.hints.expect("a hint line on an 80x24 terminal");
    assert_eq!(row.y, 22);
    for x in 0..80 {
        assert_eq!(
            buf[(x, row.y)].style().bg,
            app.theme.status_bar().bg,
            "hint line at column {x}"
        );
    }
    // The document has the row above it, and the status bar the one below.
    assert_eq!(buf[(0, 21)].style().bg, app.theme.page().bg);
    assert_eq!(buf[(0, 23)].style().bg, app.theme.status_bar().bg);
}

/// The keys it names are the keys that are bound, at every width it survives.
#[test]
fn the_hint_line_says_what_the_keymap_says() {
    for width in [40, 60, 80, 120, 200] {
        let mut app = fixture();
        let buf = frame(&mut app, width, 24);
        let row = app.panes.hints.expect("a hint line");
        let text: String = (0..width).map(|x| buf[(x, row.y)].symbol()).collect();
        assert!(text.starts_with(" j/k scroll"), "at {width}: {text:?}");
        assert_eq!(
            measure::width(&text),
            usize::from(width),
            "the hint line is not the width of its row at {width}"
        );
    }
}

/// Narrowing drops hints from the end rather than wrapping or overflowing,
/// and the row goes back to the document once nothing fits in it.
#[test]
fn the_hint_line_gives_way_from_the_end_as_the_terminal_narrows() {
    let mut previous = usize::MAX;
    for width in (4..=120u16).rev() {
        let mut app = fixture();
        let buf = frame(&mut app, width, 24);
        let Some(row) = app.panes.hints else {
            // Once the line is gone it stays gone as the terminal narrows
            // further, and the document has every row but the status bar.
            assert_eq!(app.panes.body.height, 23, "at width {width}");
            continue;
        };
        let text: String = (0..width).map(|x| buf[(x, row.y)].symbol()).collect();
        let hints = text.matches('\u{b7}').count() + 1;
        assert!(
            hints <= previous,
            "width {width} showed more hints than a wider terminal: {text:?}"
        );
        previous = hints;
        assert_eq!(
            measure::width(&text),
            usize::from(width),
            "at width {width}"
        );
    }
}

#[test]
fn every_cell_is_painted_with_the_hint_line_on_and_off() {
    for &(width, height) in SIZES {
        for hints in [true, false] {
            let mut app = fixture();
            app.hints = hints;
            let buf = frame(&mut app, width, height);
            assert_fully_painted(&buf, &format!("hints={hints} at {width}x{height}"));
        }
    }
}

#[test]
fn the_browser_gets_a_hint_line_of_its_own() {
    let mut app = browsing();
    let buf = frame(&mut app, 80, 24);
    let row = app.panes.hints.expect("a hint line over the file list");
    let text: String = (0..80).map(|x| buf[(x, row.y)].symbol()).collect();
    assert!(text.contains("enter read"), "{text:?}");
    assert!(
        !text.contains("contents"),
        "document hints in the browser: {text:?}"
    );
}

#[test]
fn scrolling_to_the_end_still_paints_the_rows_past_the_last_line() {
    let mut app = fixture();
    reconcile(&mut app, Rect::new(0, 0, 80, 24));
    app.view.top = app.doc.doc().lines.len() + 100;
    let buf = frame(&mut app, 80, 24);
    assert_fully_painted(&buf, "past the end");
}

#[test]
fn a_document_narrower_than_the_terminal_is_centered() {
    let mut app = fixture();
    app.options.width = Some(40);
    app.toc_visible = false;
    let buf = frame(&mut app, 80, 24);
    // Twenty columns of page either side of a forty-column reading column.
    assert_eq!(buf[(0, 0)].style().bg, app.theme.page().bg);
    assert_eq!(buf[(79, 0)].style().bg, app.theme.page().bg);
    assert_eq!(app.panes.content_width, 40);
}

#[test]
fn the_contents_pane_is_divided_from_the_document() {
    let mut app = fixture();
    let buf = frame(&mut app, 100, 30);
    let sidebar = app
        .panes
        .sidebar
        .expect("a sidebar over a document with headings");
    let divider = sidebar.x + sidebar.width - 1;
    for y in 0..sidebar.height {
        assert_eq!(buf[(divider, y)].symbol(), "│", "row {y}");
    }
    assert_eq!(app.panes.body.x, sidebar.width);
}

#[test]
fn hiding_the_contents_pane_gives_its_columns_to_the_document() {
    let mut app = fixture();
    let with = frame(&mut app, 100, 30);
    let divider = app.panes.sidebar.expect("a sidebar").width - 1;
    app.toc_visible = false;
    let without = frame(&mut app, 100, 30);
    assert!(app.panes.sidebar.is_none());
    assert_ne!(
        with[(divider, 0)].symbol(),
        without[(divider, 0)].symbol(),
        "the divider is still drawn"
    );
    assert_fully_painted(&without, "contents hidden");
}

#[test]
fn a_search_hit_is_highlighted_where_it_sits() {
    let mut app = fixture();
    app.toc_visible = false;
    frame(&mut app, 80, 24);
    app.search
        .search(app.doc.doc(), app.doc.revision(), "unicode", 0);
    let hit = app
        .search
        .current_match()
        .expect("the fixture contains `unicode`")
        .segments[0]
        .clone();
    app.view.top = hit.line;
    let buf = frame(&mut app, 80, 24);

    let gutter = app.panes.body.x + (app.panes.body.width - app.doc.doc().width) / 2;
    let x = gutter + hit.cols.start;
    assert_eq!(
        buf[(x, 0)].style().bg,
        app.theme.search_current().bg,
        "the selected hit is not highlighted"
    );
    // And the cell just past it is not.
    let past = x + (hit.cols.end - hit.cols.start);
    assert_ne!(buf[(past, 0)].style().bg, app.theme.search_current().bg);
}
