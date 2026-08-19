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
use marquee_markdown::doc::search::Match;
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

/// A terminal too short for the whole key reference, so it has to scroll.
fn run_short(script: &str) -> App {
    let mut app = App::new(
        Source::from_text(&document(), None, "doc.md".into(), Base::Cwd),
        Theme::new(ThemeVariant::Slate),
        Options::default(),
    );
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).expect("test terminal");
    let mut events = ScriptedEvents::new(keys(script));
    drive(&mut app, &mut terminal, &mut events).expect("the loop runs");
    app
}

#[test]
fn the_key_reference_scrolls_when_it_does_not_fit() {
    let app = run_short("?jjj");
    assert_eq!(app.help_scroll, 3);
    // And the document did not move underneath it.
    assert_eq!(app.view.top, 0);
}

#[test]
fn the_key_reference_stops_at_its_last_row() {
    let app = run_short("?G");
    let rows = app.keymap.help_rows(app.pane_mode()).len();
    let visible = usize::from(app.panes.body.height + app.panes.status.height) - 2;
    assert_eq!(usize::from(app.help_scroll), rows - visible.min(rows));
    // Scrolling past the end holds there rather than wrapping.
    assert_eq!(run_short("?Gj").help_scroll, app.help_scroll);
    assert_eq!(run_short("?Gg").help_scroll, 0);
}

#[test]
fn reopening_the_key_reference_starts_at_the_top() {
    let app = run_short("?jjj??");
    assert_eq!(app.help_scroll, 0);
    assert!(app.overlay.is_some());
}

#[test]
fn a_reference_that_fits_does_not_scroll_at_all() {
    // The default 80x24 harness is tall enough for some modes; use the
    // browser, whose reference is short.
    let (_dir, app) = browsing(FILES, "?jjjj");
    assert_eq!(app.help_scroll, 0, "scrolled a reference that fits");
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
    let line = back
        .search
        .current_match()
        .map(Match::first_line)
        .expect("a hit");
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
    assert!(
        hit.first_line() < app.doc.doc().lines.len(),
        "stale line index"
    );
}

/// A directory of markdown files, and a reader browsing it.
///
/// The walk's results are fed in as events, which is exactly how they arrive
/// from the real walk — so the browser is exercised through the same door.
fn browsing(files: &[&str], script: &str) -> (tempfile::TempDir, App) {
    use marquee_markdown::browser::{Entry, Scan};

    let dir = tempfile::tempdir().expect("temp dir");
    let mut entries = Vec::new();
    for (index, name) in files.iter().enumerate() {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&path, format!("# {name}\n\nBody of {name}.\n")).expect("write");
        entries.push(Entry {
            path,
            display: (*name).to_owned(),
            // Ordered so the first file listed is the first one named.
            modified: Some(
                std::time::SystemTime::UNIX_EPOCH
                    + std::time::Duration::from_secs(1_000_000 - index as u64),
            ),
        });
    }

    let mut app = App::browsing(
        dir.path().to_path_buf(),
        Theme::new(ThemeVariant::Slate),
        Options::default(),
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    let mut events = ScriptedEvents::new(
        [
            Event::Scan {
                generation: 0,
                scan: Scan::Found(entries),
            },
            Event::Scan {
                generation: 0,
                scan: Scan::Done,
            },
        ]
        .into_iter()
        .chain(keys(script)),
    );
    drive(&mut app, &mut terminal, &mut events).expect("the loop runs");
    (dir, app)
}

const FILES: &[&str] = &[
    "README.md",
    "docs/ROADMAP.md",
    "docs/THEMING.md",
    "notes/meeting.md",
];

#[test]
fn the_browser_lists_what_the_walk_found() {
    let (_dir, app) = browsing(FILES, "");
    assert_eq!(
        app.summary(),
        "mode=browser files=4 cursor=README.md filter=- quit=false"
    );
}

#[test]
fn moving_through_the_list_stops_at_both_ends() {
    assert!(
        browsing(FILES, "jj")
            .1
            .summary()
            .contains("cursor=docs/THEMING.md")
    );
    assert!(
        browsing(FILES, "jjjjjjjj")
            .1
            .summary()
            .contains("cursor=notes/meeting.md")
    );
    assert!(
        browsing(FILES, "jjkk")
            .1
            .summary()
            .contains("cursor=README.md")
    );
    assert!(
        browsing(FILES, "G")
            .1
            .summary()
            .contains("cursor=notes/meeting.md")
    );
    assert!(
        browsing(FILES, "Gg")
            .1
            .summary()
            .contains("cursor=README.md")
    );
}

#[test]
fn the_filter_narrows_the_list_as_it_is_typed() {
    // Incremental: the list is already narrowed before enter is pressed.
    let (_dir, app) = browsing(FILES, "/theming");
    assert!(app.summary().contains("files=1"), "{}", app.summary());
    assert!(
        app.summary().contains("cursor=docs/THEMING.md"),
        "{}",
        app.summary()
    );
}

#[test]
fn the_filter_is_fuzzy() {
    let (_dir, app) = browsing(FILES, "/rdmp\n");
    assert!(
        app.summary().contains("cursor=docs/ROADMAP.md"),
        "{}",
        app.summary()
    );
}

#[test]
fn typing_a_filter_does_not_run_the_keys_as_commands() {
    // `q`, `j` and `G` all do something in the browser.
    let (_dir, app) = browsing(FILES, "/qjG");
    assert!(!app.should_quit, "a letter typed into the filter quit");
    assert!(app.summary().contains("filter=qjG|"), "{}", app.summary());
}

#[test]
fn escape_clears_a_committed_filter() {
    let (_dir, app) = browsing(FILES, "/docs\n<esc>");
    assert!(app.summary().contains("files=4"), "{}", app.summary());
    assert!(app.summary().contains("filter=-"), "{}", app.summary());
}

#[test]
fn escape_cancels_a_filter_being_typed_without_committing_it() {
    let (_dir, app) = browsing(FILES, "/docs<esc>");
    assert!(app.summary().contains("files=4"), "{}", app.summary());
    assert!(app.summary().contains("filter=-"), "{}", app.summary());
}

#[test]
fn opening_a_file_reads_it() {
    let (_dir, app) = browsing(FILES, "j\n");
    assert!(
        app.summary().starts_with("mode=document"),
        "{}",
        app.summary()
    );
    assert_eq!(app.doc.source.display_name, "ROADMAP.md");
    assert!(!app.doc.doc().lines.is_empty(), "nothing was laid out");
}

#[test]
fn escape_goes_back_to_the_browser_from_a_document() {
    let (_dir, app) = browsing(FILES, "j\n<esc>");
    assert!(
        app.summary().starts_with("mode=browser"),
        "{}",
        app.summary()
    );
    // And the list is where it was left, not reset.
    assert!(
        app.summary().contains("cursor=docs/ROADMAP.md"),
        "{}",
        app.summary()
    );
}

#[test]
fn a_document_opened_from_the_browser_can_be_read_and_left_repeatedly() {
    let (_dir, app) = browsing(FILES, "j\n<esc>jj\n<esc>");
    assert!(
        app.summary().starts_with("mode=browser"),
        "{}",
        app.summary()
    );
    assert!(
        app.summary().contains("cursor=notes/meeting.md"),
        "{}",
        app.summary()
    );
}

#[test]
fn a_file_named_on_the_command_line_has_no_browser_to_go_back_to() {
    // `esc` there must hint rather than opening a browser that was never asked
    // for.
    let app = run(&document(), "<esc>");
    assert!(app.browser.is_none());
    assert!(app.message.is_some());
    assert!(!app.should_quit);
}

#[test]
fn the_browser_survives_having_nothing_to_list() {
    let (_dir, app) = browsing(&[], "jkG\n/x\n");
    assert!(app.summary().contains("files=0"), "{}", app.summary());
    assert!(!app.should_quit);
}

#[test]
fn paging_moves_a_whole_screen_in_the_browser() {
    let names: Vec<String> = (0..60).map(|n| format!("file-{n:02}.md")).collect();
    let files: Vec<&str> = names.iter().map(String::as_str).collect();
    let (_dir, one) = browsing(&files, "f");
    let (_dir2, two) = browsing(&files, "ff");
    assert!(
        one.summary().contains("cursor=file-23.md"),
        "{}",
        one.summary()
    );
    assert!(
        two.summary().contains("cursor=file-46.md"),
        "{}",
        two.summary()
    );
}

/// A document with links, on disk, so reloading and editing have something
/// real to work with.
fn linked_document() -> String {
    let mut text = String::new();
    for n in 1..=12 {
        text.push_str(&format!("# Chapter {n}\n\n"));
        text.push_str(&format!(
            "Body with [link {n}](https://example.com/{n}) in it.\n\n"
        ));
    }
    text
}

#[test]
fn stepping_through_links_wraps_and_reveals() {
    let text = linked_document();
    let first = run(&text, "]");
    assert_eq!(first.links.position(), Some(0));
    let second = run(&text, "]]");
    assert_eq!(second.links.position(), Some(1));
    // Backwards from the first wraps to the last.
    let last = run(&text, "][");
    assert_eq!(last.links.position(), Some(11));
    let line = last.links.selected().expect("a link").line;
    let height = usize::from(last.panes.body.height);
    assert!(
        (last.view.top..last.view.top + height).contains(&line),
        "the link was not brought into view"
    );
}

#[test]
fn a_document_with_no_links_says_so_rather_than_doing_nothing() {
    let app = run(&document(), "]");
    assert!(app.links.selected().is_none());
    assert!(app.message.is_some(), "no explanation for the reader");
}

#[test]
fn opening_a_link_before_picking_one_explains_what_to_press() {
    let app = run(&linked_document(), "\n");
    assert!(app.message.is_some());
    assert!(!app.should_quit);
}

#[test]
fn a_theme_change_keeps_the_selected_link() {
    // The layout changes under it, so the line moves but the link should not.
    let app = run(&linked_document(), "]]T");
    assert_eq!(app.links.position(), Some(1));
    let link = app.links.selected().expect("a link");
    assert!(link.line < app.doc.doc().lines.len(), "stale line index");
}

#[test]
fn editing_asks_for_the_right_file_and_the_line_on_screen() {
    use marquee_markdown::app::external::Request;

    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("doc.md");
    let text: String = (1..=60).map(|n| format!("Paragraph {n}.\n\n")).collect();
    std::fs::write(&path, &text).expect("write");

    let mut app = App::new(
        Source::from_text(&text, Some(path.clone()), "doc.md".into(), Base::Cwd),
        Theme::new(ThemeVariant::Slate),
        Options::default(),
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    let mut events = ScriptedEvents::new(keys("ddde"));
    drive(&mut app, &mut terminal, &mut events).expect("the loop runs");

    // Headless, so the request is recorded rather than carried out: no editor
    // opens in the middle of a test run.
    let Some(Request::Edit { path: asked, line }) = app.pending else {
        panic!("no edit was asked for: {:?}", app.pending);
    };
    assert_eq!(asked, path);
    assert!(
        line > 1,
        "the editor would open at the top, not at line {line}"
    );
}

#[test]
fn editing_a_document_that_is_not_a_file_explains_itself() {
    let app = run(&document(), "e");
    assert!(app.pending.is_none());
    assert!(app.message.is_some());
}

#[test]
fn reloading_picks_up_what_changed_on_disk() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("doc.md");
    std::fs::write(&path, "# One\n\nBody.\n").expect("write");

    let mut app = App::new(
        Source::from_text(
            "# One\n\nBody.\n",
            Some(path.clone()),
            "doc.md".into(),
            Base::Cwd,
        ),
        Theme::new(ThemeVariant::Slate),
        Options::default(),
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");

    std::fs::write(&path, "# One\n\nBody.\n\n# Two\n\nMore.\n").expect("write");
    let mut events = ScriptedEvents::new(keys("r"));
    drive(&mut app, &mut terminal, &mut events).expect("the loop runs");

    assert_eq!(app.doc.outline().len(), 2, "the new heading did not arrive");
    assert_eq!(app.message.as_deref(), Some("reloaded"));
}

#[test]
fn reloading_keeps_the_reader_in_the_section_they_were_in() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("doc.md");
    let body: String = (1..=12)
        .map(|n| format!("# Chapter {n}\n\nBody of chapter {n}.\n\n"))
        .collect();
    std::fs::write(&path, &body).expect("write");

    let mut app = App::new(
        Source::from_text(&body, Some(path.clone()), "doc.md".into(), Base::Cwd),
        Theme::new(ThemeVariant::Slate),
        Options::default(),
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    let mut events = ScriptedEvents::new(keys("\t"));
    drive(&mut app, &mut terminal, &mut events).expect("focus the contents");

    // Jump to chapter 6, then have the file grow at the top.
    let mut events = ScriptedEvents::new(keys("jjjjj\n"));
    drive(&mut app, &mut terminal, &mut events).expect("open the entry");
    let before = app.active_heading().map(|anchor| anchor.id.clone());
    assert_eq!(before.as_deref(), Some("chapter-6"));

    std::fs::write(&path, format!("A new opening paragraph.\n\n{body}")).expect("write");
    let mut events = ScriptedEvents::new(keys("r"));
    drive(&mut app, &mut terminal, &mut events).expect("reload");

    assert_eq!(
        app.active_heading().map(|anchor| anchor.id.clone()),
        before,
        "the reader was moved to a different section"
    );
}

#[test]
fn a_reload_that_fails_leaves_the_reader_reading() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("doc.md");
    std::fs::write(&path, "# One\n").expect("write");

    let mut app = App::new(
        Source::from_text("# One\n", Some(path.clone()), "doc.md".into(), Base::Cwd),
        Theme::new(ThemeVariant::Slate),
        Options::default(),
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    std::fs::remove_file(&path).expect("remove");

    let mut events = ScriptedEvents::new(keys("r"));
    drive(&mut app, &mut terminal, &mut events).expect("the loop survives");
    assert!(!app.should_quit);
    assert!(
        app.message.as_deref().is_some_and(|m| m.contains("reload")),
        "{:?}",
        app.message
    );
    assert!(!app.doc.doc().lines.is_empty(), "the document was lost");
}

#[test]
fn a_rescan_clears_the_list_and_the_new_walk_repopulates_it() {
    use marquee_markdown::browser::{Entry, Scan};

    let (dir, mut app) = browsing(FILES, "jj");
    assert!(
        app.summary().contains("cursor=docs/THEMING.md"),
        "{}",
        app.summary()
    );

    // Press r, then feed what the *new* walk (generation 1) finds — the old
    // selection among them, plus a file created since.
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    let refound = vec![
        Entry {
            path: dir.path().join("docs/THEMING.md"),
            display: "docs/THEMING.md".into(),
            modified: None,
        },
        Entry {
            path: dir.path().join("BRAND-NEW.md"),
            display: "BRAND-NEW.md".into(),
            modified: None,
        },
    ];
    let mut events = ScriptedEvents::new(keys("r").into_iter().chain([
        Event::Scan {
            generation: 1,
            scan: Scan::Found(refound),
        },
        Event::Scan {
            generation: 1,
            scan: Scan::Done,
        },
    ]));
    drive(&mut app, &mut terminal, &mut events).expect("the loop runs");

    assert!(app.summary().contains("files=2"), "{}", app.summary());
    // The cursor followed the file it was on, not a row number.
    assert!(
        app.summary().contains("cursor=docs/THEMING.md"),
        "{}",
        app.summary()
    );
    let browser = app.browser.as_ref().unwrap();
    assert!(!browser.scanning);
}

#[test]
fn reports_from_a_superseded_walk_are_dropped() {
    use marquee_markdown::browser::{Entry, Scan};

    let (dir, mut app) = browsing(FILES, "");
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    // Rescan (generation becomes 1), then a straggling batch from the old
    // walk (generation 0) arrives. It must not repopulate the cleared list.
    let stale = vec![Entry {
        path: dir.path().join("README.md"),
        display: "README.md".into(),
        modified: None,
    }];
    let mut events = ScriptedEvents::new(keys("r").into_iter().chain([
        Event::Scan {
            generation: 0,
            scan: Scan::Found(stale),
        },
        Event::Scan {
            generation: 0,
            scan: Scan::Done,
        },
    ]));
    drive(&mut app, &mut terminal, &mut events).expect("the loop runs");

    assert!(app.summary().contains("files=0"), "{}", app.summary());
    // And the stale Done did not end the scan the reader is waiting on.
    assert!(app.browser.as_ref().unwrap().scanning);
}

#[test]
fn toggling_hidden_files_flips_the_flag_and_rescans() {
    let (_dir, app) = browsing(FILES, ".");
    assert!(app.options.all, "the flag did not flip");
    assert!(app.browser.as_ref().unwrap().scanning, "no rescan began");
    assert!(
        app.message.is_some(),
        "nothing told the reader what changed"
    );
    let (_dir2, app) = browsing(FILES, "..");
    assert!(!app.options.all, "the flag did not flip back");
}

#[test]
fn a_rescan_survives_headlessly_with_no_event_queue() {
    // app.events is None in tests; the state changes must still happen and
    // nothing may panic or spawn.
    let (_dir, app) = browsing(FILES, "r");
    assert!(app.summary().contains("files=0"), "{}", app.summary());
    assert!(app.browser.as_ref().unwrap().scanning);
    assert_eq!(app.browser.as_ref().unwrap().generation(), 1);
}

#[test]
fn a_rescan_keeps_the_filter() {
    use marquee_markdown::browser::Scan;
    let (_dir, mut app) = browsing(FILES, "/docs\n");
    assert!(app.summary().contains("filter=docs"), "{}", app.summary());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    let mut events = ScriptedEvents::new(keys("r").into_iter().chain([Event::Scan {
        generation: 1,
        scan: Scan::Done,
    }]));
    drive(&mut app, &mut terminal, &mut events).expect("the loop runs");
    assert!(app.summary().contains("filter=docs"), "{}", app.summary());
}

#[test]
fn typing_a_search_narrows_the_matches_live() {
    // The count updates with each keystroke, before enter is ever pressed —
    // and the view stays put: narrowing is feedback, not navigation.
    let text = document();
    let partial = run(&text, "/heading");
    assert!(
        partial.summary().contains("search=/heading|[1/60]"),
        "{}",
        partial.summary()
    );
    assert_eq!(partial.view.top, 0, "typing scrolled the view");

    let narrower = run(&text, "/heading 4");
    assert!(
        narrower.summary().contains("search=/heading 4|[1/11]"),
        "{}",
        narrower.summary()
    );
}

#[test]
fn escape_while_typing_reverts_to_the_committed_search() {
    // A committed search, then a new query typed and abandoned: the old
    // highlight comes back with no key doing any explicit restoring.
    let app = run(&document(), "/heading 4\r/zzz<esc>");
    assert!(
        app.summary().contains("search=heading 4[1/11]"),
        "{}",
        app.summary()
    );
}

#[test]
fn n_steps_through_a_match_that_spans_two_lines() {
    // A paragraph long enough to wrap at the 80-column harness width, with
    // the phrase straddling the break.
    let mut text = String::from(
        "Filler filler filler filler filler filler filler filler filler \
         crossing phrase and more words after it.\n\n",
    );
    text.push_str("Later the crossing phrase appears again, on one line.\n");
    let app = run(&text, "/crossing phrase\rn");
    assert!(app.summary().contains("[2/2]"), "{}", app.summary());
    let hit = app.search.current_match().expect("a hit");
    let height = usize::from(app.panes.body.height);
    assert!(
        (app.view.top..app.view.top + height).contains(&hit.first_line()),
        "the hit was not revealed"
    );
}

#[test]
fn a_link_to_a_heading_in_this_document_scrolls_to_it() {
    // The outline already knows where every slug is; handing `#slug` to the
    // system opener would do nothing a reader recognises as following it.
    let mut text = String::from("See [the last part](#chapter-12) below.\n\n");
    text.push_str(&nested());
    let app = run(&text, "]\r");

    // The heading is on screen. It cannot always be *at* the top — a target
    // in the last screenful has nowhere further to scroll — so visibility is
    // the property, not the offset.
    let line = app
        .doc
        .doc()
        .outline
        .iter()
        .find(|anchor| anchor.id == "chapter-12")
        .expect("the heading exists")
        .line;
    let height = usize::from(app.panes.body.height);
    assert!(
        (app.view.top..app.view.top + height).contains(&line),
        "line {line} not visible from top {}",
        app.view.top
    );
    assert!(app.view.top > 0, "the view did not move");
    assert!(app.message.is_some(), "nothing confirmed the jump");
}

#[test]
fn a_link_to_a_heading_that_is_not_there_says_so() {
    let text = format!("See [nowhere](#no-such-heading).\n\n{}", nested());
    let app = run(&text, "]\r");
    assert_eq!(app.view.top, 0, "it moved anyway");
    assert!(
        app.message
            .as_deref()
            .is_some_and(|m| m.contains("no-such-heading")),
        "{:?}",
        app.message
    );
}

#[test]
fn copying_an_in_document_link_copies_what_the_markdown_says() {
    // There is no address to give instead, and `#section` is what belongs
    // back in a markdown file.
    let text = format!("See [below](#chapter-3).\n\n{}", nested());
    let app = run(&text, "]y");
    assert!(
        app.message.as_deref().is_some_and(|m| m.contains("copied")),
        "{:?}",
        app.message
    );
}
