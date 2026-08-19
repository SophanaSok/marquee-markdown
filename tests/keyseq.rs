//! Whole key sequences, driven headlessly.
//!
//! The update loop and the reconcile step are deterministic and drawing is
//! pure, so a test can type at the reader and assert on what it becomes. This
//! is the cheapest coverage in the project for the bug class that matters most
//! in a modal interface: a key doing the wrong thing because the wrong mode
//! was in force.

use marquee_markdown::app::event::{Event, ScriptedEvents};
use marquee_markdown::app::keymap::Chord;
use marquee_markdown::app::{App, Options, drive};
use marquee_markdown::source::{Base, Source};
use marquee_markdown::theme::{Theme, ThemeVariant};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// Turn a script into key events.
///
/// Bare characters are themselves; anything in angle brackets is a chord name,
/// parsed by the same code that parses a configuration file — so the test
/// notation and the config notation cannot drift apart.
fn keys(script: &str) -> Vec<Event> {
    use crossterm::event::{KeyEvent, KeyModifiers};

    let mut events = Vec::new();
    let mut rest = script;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix('<') {
            let (name, tail) = tail.split_once('>').expect("unterminated chord in script");
            let chord: Chord = name.parse().expect("chord in script parses");
            events.push(Event::Key(KeyEvent::new(chord.code, chord.modifiers)));
            rest = tail;
        } else {
            let c = rest.chars().next().expect("non-empty");
            events.push(Event::Key(KeyEvent::new(
                crossterm::event::KeyCode::Char(c),
                KeyModifiers::NONE,
            )));
            rest = &rest[c.len_utf8()..];
        }
    }
    events
}

/// A document with enough sections to scroll through.
fn document() -> String {
    (1..=60)
        .map(|n| format!("## Heading {n}\n\nBody text for section {n}.\n\n"))
        .collect()
}

/// Type `script` at a reader over `text` and return the resulting state.
fn run(text: &str, script: &str) -> App {
    let mut app = App::new(
        Source::from_text(text, None, "doc.md".into(), Base::Cwd),
        Theme::new(ThemeVariant::Slate),
        Options::default(),
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    let mut events = ScriptedEvents::new(keys(script));
    drive(&mut app, &mut terminal, &mut events).expect("the loop runs");
    app
}

#[test]
fn scrolling_down_and_back_returns_to_the_start() {
    let app = run(&document(), "jjjkkk");
    assert_eq!(
        app.summary(),
        "mode=document top=0 left=0 section=heading-1 theme=slate quit=false"
    );
}

#[test]
fn the_ends_of_the_document_are_reachable_and_stable() {
    let text = document();
    let bottom = run(&text, "G");
    assert!(bottom.view.top > 0);
    // Pressing it again changes nothing: the end is an end, not a wrap.
    assert_eq!(run(&text, "GG").summary(), bottom.summary());
    assert_eq!(run(&text, "Gg").view.top, 0);
}

#[test]
fn a_page_moves_further_than_a_half_page_moves_further_than_a_line() {
    let text = document();
    let line = run(&text, "j").view.top;
    let half = run(&text, "d").view.top;
    let full = run(&text, "f").view.top;
    assert!(line < half && half < full, "{line} {half} {full}");
}

#[test]
fn typing_q_with_the_help_overlay_open_closes_it_instead_of_quitting() {
    // The classic modal bug: a global key firing while something else has
    // focus. Here it costs the reader their document.
    let app = run(&document(), "?q");
    assert!(!app.should_quit, "help swallowed the document");
    assert_eq!(
        app.summary(),
        "mode=document top=0 left=0 section=heading-1 theme=slate quit=false"
    );
}

#[test]
fn keys_pressed_behind_the_help_overlay_do_not_reach_the_document() {
    let text = document();
    let before = run(&text, "jj");
    let after = run(&text, "jj?jjjGdf?");
    assert_eq!(after.view.top, before.view.top);
    assert!(after.overlay.is_none(), "the overlay did not close");
}

#[test]
fn escape_does_not_quit_when_there_is_nothing_to_close() {
    let app = run(&document(), "<esc><esc><esc>");
    assert!(!app.should_quit);
    assert!(app.message.is_some(), "no hint about how to leave");
}

#[test]
fn quitting_ends_the_loop_before_the_rest_of_the_script_is_read() {
    let app = run(&document(), "jqGGGG");
    assert!(app.should_quit);
    assert_eq!(app.view.top, 1, "keys were read after the reader quit");
}

#[test]
fn the_theme_can_be_switched_and_switched_back() {
    assert_eq!(run(&document(), "T").theme.name, "paper");
    assert_eq!(run(&document(), "TT").theme.name, "slate");
}

#[test]
fn the_active_section_tracks_the_reading_position() {
    let app = run(&document(), "GG");
    let heading = app.active_heading().expect("a section is active");
    assert_ne!(heading.id, "heading-1", "the outline did not follow");
}

#[test]
fn wrapped_text_cannot_be_scrolled_sideways() {
    let app = run(&document(), "llllll");
    assert_eq!(app.view.left, 0);
}

#[test]
fn control_chords_from_the_script_are_bound_the_same_as_their_aliases() {
    let text = document();
    assert_eq!(run(&text, "<ctrl+d>").view.top, run(&text, "d").view.top);
    assert_eq!(run(&text, "<pgdn>").view.top, run(&text, "f").view.top);
    assert_eq!(run(&text, "<home>").view.top, run(&text, "g").view.top);
}

#[test]
fn an_empty_document_takes_every_key_without_moving_or_panicking() {
    let app = run("", "jkdufbgG?<esc>llhh");
    assert_eq!(
        app.summary(),
        "mode=document top=0 left=0 section=- theme=slate quit=false"
    );
}
