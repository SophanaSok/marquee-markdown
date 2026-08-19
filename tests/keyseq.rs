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
            // A terminal reports these as keys of their own, never as
            // characters, so the script notation matches what arrives.
            let code = match c {
                '\t' => crossterm::event::KeyCode::Tab,
                '\n' | '\r' => crossterm::event::KeyCode::Enter,
                other => crossterm::event::KeyCode::Char(other),
            };
            events.push(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
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

/// A document whose headings nest, for the contents pane.
fn nested() -> String {
    let mut text = String::new();
    for n in 1..=12 {
        text.push_str(&format!("# Chapter {n}\n\nBody text for chapter {n}.\n\n"));
        text.push_str(&format!("## Part {n}a\n\nMore body text.\n\n"));
        text.push_str(&format!("## Part {n}b\n\nMore body text.\n\n"));
    }
    text
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
        "mode=document top=0 left=0 section=heading-1 toc=heading-1 search=- theme=slate quit=false"
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
        "mode=document top=0 left=0 section=heading-1 toc=heading-1 search=- theme=slate quit=false"
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
        "mode=document top=0 left=0 section=- toc=off search=- theme=slate quit=false"
    );
}

#[test]
fn the_contents_pane_can_be_hidden_and_brought_back() {
    let text = nested();
    assert!(run(&text, "t").summary().contains("toc=off"));
    assert!(run(&text, "tt").summary().contains("toc=chapter-1"));
}

#[test]
fn focus_moves_to_the_contents_pane_and_back() {
    let text = nested();
    assert!(run(&text, "\t").summary().starts_with("mode=toc"));
    assert!(run(&text, "\t\t").summary().starts_with("mode=document"));
    // Escape leaves the pane rather than quitting.
    let app = run(&text, "\t<esc>");
    assert!(app.summary().starts_with("mode=document"));
    assert!(!app.should_quit);
}

#[test]
fn moving_the_contents_cursor_leaves_the_document_where_it_is() {
    let app = run(&nested(), "\tjjj");
    assert_eq!(app.view.top, 0, "the document scrolled with the cursor");
    assert!(app.summary().contains("toc=chapter-2"), "{}", app.summary());
}

#[test]
fn choosing_an_entry_goes_there_and_hands_focus_back() {
    let app = run(&nested(), "\tjj\n");
    assert!(app.view.top > 0, "the document did not move");
    assert!(app.summary().starts_with("mode=document"));
    assert_eq!(
        app.active_heading().map(|anchor| anchor.id.as_str()),
        Some("part-1b")
    );
}

#[test]
fn folding_a_chapter_steps_over_its_parts() {
    // With chapter 1 folded, one press of `j` reaches chapter 2 rather than
    // landing on a part that is no longer on show.
    let app = run(&nested(), "\thj");
    assert!(app.summary().contains("toc=chapter-2"), "{}", app.summary());
}

#[test]
fn unfolding_steps_back_into_the_chapter() {
    // Fold chapter 1 shut, step past it to chapter 2, then step into it.
    let app = run(&nested(), "\thjl");
    assert!(app.summary().contains("toc=part-2a"), "{}", app.summary());
}

#[test]
fn the_contents_cursor_survives_a_theme_change() {
    // The theme re-lays out the document, which renumbers every line; the
    // cursor is a row in the outline, not a line, and must not move.
    let app = run(&nested(), "\tjjT");
    assert!(app.summary().contains("toc=part-1b"), "{}", app.summary());
}

#[test]
fn typing_a_search_does_not_run_the_keys_as_commands() {
    // `q`, `j` and `G` all do something in the document. In a prompt they are
    // letters, and a reader who types "quit" into a search box expects to
    // still be reading afterwards.
    let app = run(&document(), "/qjG");
    assert!(!app.should_quit, "a letter typed into the prompt quit");
    assert_eq!(app.view.top, 0, "a letter typed into the prompt scrolled");
    assert!(app.summary().contains("search=/qjG|"), "{}", app.summary());
}

#[test]
fn a_search_finds_its_hits_and_says_how_many() {
    let app = run(&document(), "/heading 4\n");
    // "Heading 4", "Heading 40" through "Heading 49": eleven in all.
    assert!(
        app.summary().contains("search=heading 4[1/11]"),
        "{}",
        app.summary()
    );
}

#[test]
fn stepping_through_hits_moves_the_reader() {
    let text = document();
    let first = run(&text, "/heading 4\n");
    let second = run(&text, "/heading 4\nn");
    assert!(second.view.top > first.view.top, "`n` did not move on");

    // Stepping back selects the first hit again and brings it into view. It
    // does not restore the exact scroll position, and should not pretend to:
    // what the reader asked for is to see that hit.
    let back = run(&text, "/heading 4\nnN");
    assert_eq!(back.search.current(), Some(0));
    let line = back.search.current_match().expect("a hit").line;
    let height = usize::from(back.panes.body.height);
    assert!(
        (back.view.top..back.view.top + height).contains(&line),
        "hit on line {line} is off screen at top {}",
        back.view.top
    );
}

#[test]
fn a_search_with_no_hits_says_so_and_changes_nothing() {
    let app = run(&document(), "/absent\n");
    assert_eq!(app.view.top, 0);
    assert!(
        app.summary().contains("search=absent[0/0]"),
        "{}",
        app.summary()
    );
}

#[test]
fn escape_cancels_a_prompt_without_running_it() {
    let app = run(&document(), "/heading 4<esc>");
    assert!(app.summary().contains("search=-"), "{}", app.summary());
    assert!(!app.should_quit);
}

#[test]
fn backspacing_out_of_an_empty_prompt_leaves_it() {
    let app = run(&document(), "/ab<backspace><backspace><backspace>");
    assert!(app.summary().contains("search=-"), "{}", app.summary());
    assert!(app.summary().starts_with("mode=document"));
}

#[test]
fn escape_clears_a_finished_search() {
    let app = run(&document(), "/heading 4\n<esc>");
    assert!(app.summary().contains("search=-"), "{}", app.summary());
}

#[test]
fn the_escape_ladder_unwinds_one_rung_at_a_time() {
    let text = nested();
    // Search, then focus the contents pane, then open help: three escapes get
    // back to a plain document, and none of them quits.
    let app = run(&text, "/chapter\n\t?<esc><esc><esc>");
    assert!(!app.should_quit);
    assert!(
        app.summary().starts_with("mode=document"),
        "{}",
        app.summary()
    );
    assert!(app.summary().contains("search=-"), "{}", app.summary());
}

#[test]
fn a_search_survives_being_re_laid_out() {
    // Switching theme re-lays out the document; the hits are line indices and
    // would otherwise point at whatever now happens to be there.
    let app = run(&document(), "/heading 4\nnnT");
    assert!(app.summary().contains("[3/11]"), "{}", app.summary());
    let hit = app.search.current_match().expect("a hit");
    assert!(hit.line < app.doc.doc().lines.len(), "stale line index");
}
