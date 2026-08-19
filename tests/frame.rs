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
use marquee_markdown::theme::{Theme, ThemeVariant};
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
fn every_cell_is_painted_in_both_themes() {
    for variant in [ThemeVariant::Paper, ThemeVariant::Slate] {
        let mut app = fixture();
        app.theme = Theme::new(variant);
        let buf = frame(&mut app, 80, 24);
        assert_fully_painted(&buf, variant.name());
    }
}

#[test]
fn the_status_bar_occupies_the_last_row_and_all_of_it() {
    let mut app = fixture();
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
    let buf = frame(&mut app, 80, 24);
    // Twenty columns of page either side of a forty-column reading column.
    assert_eq!(buf[(0, 0)].style().bg, app.theme.page().bg);
    assert_eq!(buf[(79, 0)].style().bg, app.theme.page().bg);
    assert_eq!(app.panes.content_width, 40);
}
