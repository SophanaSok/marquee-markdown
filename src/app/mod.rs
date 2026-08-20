//! The application shell: state, input, and the loop that ties them together.
//!
//! One iteration, in order:
//!
//! ```text
//! RECONCILE  pane geometry → layout cache → derived state   (pure, no input)
//! DRAW       ui::draw(&App)                                 (no mutation)
//! RECEIVE    one event from the event source
//! UPDATE     update::handle(&mut App, event)                (the only mutation site)
//! ```
//!
//! Reconciling before drawing rather than during it is what lets the draw path
//! take `&App`: pane sizes and the active section already exist by the time a
//! widget asks for them.

pub mod action;
pub mod derived;
pub mod event;
pub mod external;
pub mod gate;
pub mod keymap;
pub mod layout;
pub mod state;
pub mod terminal;
pub mod update;

use std::path::PathBuf;

use anyhow::Result;
use ratatui::layout::Rect;

use crate::render::LayoutOptions;
use crate::source::Source;
use crate::theme::Theme;

pub use state::{App, Options, Overlay, Screen};

/// Run the full-screen reader over a document until the reader quits.
///
/// # Errors
/// Returns an error when the terminal cannot be taken over or drawing fails.
pub fn run(source: Source, theme: Theme, options: Options, keymap: keymap::Keymap) -> Result<()> {
    let mut app = App::new(source, theme, options);
    app.keymap = keymap;
    take_over(app, options)
}

/// Open the file browser over `root`.
///
/// The directory walk starts before the terminal is taken over, so the first
/// screenful is already arriving by the time there is something to draw it on.
///
/// # Errors
/// Returns an error when the terminal cannot be taken over or drawing fails.
pub fn browse(root: PathBuf, theme: Theme, options: Options, keymap: keymap::Keymap) -> Result<()> {
    let mut app = App::browsing(root, theme, options);
    app.keymap = keymap;
    take_over(app, options)
}

/// Take over the terminal and run `app` until it quits.
///
/// `start` is handed the event sender so background work can be started with
/// somewhere to report to.
fn take_over(mut app: App, options: Options) -> Result<()> {
    terminal::install_panic_hook();
    let mut screen = terminal::Screen::enter(options.mouse)?;
    let (mut events, sender) = event::Events::new();
    app.events = Some(sender);
    app.start_watching();
    // The initial scan takes the same path a rescan does, so there is one
    // spawn site rather than two that can drift.
    if app.screen == Screen::Browser {
        update::respawn_walk(&app);
    }
    drive(&mut app, screen.terminal(), &mut events)
}

/// The loop itself, over any backend and any event source, so it can be run
/// headless in a test.
///
/// # Errors
/// Propagates drawing and input failures.
pub fn drive<B>(
    app: &mut App,
    terminal: &mut ratatui::Terminal<B>,
    events: &mut dyn event::EventSource,
) -> Result<()>
where
    B: ratatui::backend::Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    let mut batch = Vec::new();
    loop {
        reconcile(app, terminal.get_frame().area());
        terminal.draw(|frame| crate::ui::draw(frame, app))?;
        if app.should_quit {
            return Ok(());
        }
        let Some(event) = events.next()? else {
            return Ok(());
        };
        // Everything already waiting is applied before the next frame. A
        // window being dragged produces a resize per pixel column, and
        // re-laying the document out for each one — then throwing all but the
        // last away — is the difference between a smooth drag and a stuttering
        // one on a large document.
        batch.clear();
        batch.push(event);
        events.drain(&mut batch)?;
        event::coalesce(&mut batch);
        for event in batch.drain(..) {
            update::handle(app, event);
        }
        // Only a run with a real terminal carries these out; a headless one
        // leaves the request recorded so a test can see what was asked for
        // without an editor opening in the middle of `cargo test`.
        if app.events.is_some()
            && let Some(request) = app.pending.take()
        {
            perform(app, &request);
            // The window may have been resized while the other program had
            // the terminal. SIGWINCH goes to the foreground process group,
            // which was not this one, so nothing reported it — and drawing at
            // the old width leaves a mangled frame standing until the reader
            // happens to press a key. Asking the backend costs an ioctl and
            // no round-trip, unlike every other question a terminal can be
            // asked. Before the swaps, which are what blank the buffers.
            let _ = terminal.autoresize();
            // The other program has been all over the screen, so every cell
            // has to be written again.
            //
            // `Terminal::clear` is the obvious call and the wrong one: it
            // snapshots the cursor position first, and that query is a
            // round-trip through the terminal. By here the reader thread is
            // reading standard input again and would swallow the reply.
            // Resetting both buffers instead makes the next diff write
            // everything, with nothing asked of the terminal.
            terminal.swap_buffers();
            terminal.swap_buffers();
        }
    }
}

/// Carry out a request that needed the terminal to itself.
fn perform(app: &mut App, request: &external::Request) {
    if let Err(error) = external::run(request, app.options.mouse) {
        app.message = Some(format!("{error:#}"));
        return;
    }
    // An edit is almost always a change, and waiting for the watcher to notice
    // would show the reader a document they know is out of date.
    if matches!(request, external::Request::Edit { .. })
        && let Err(error) = app.reload_from_disk()
    {
        app.message = Some(format!("cannot reload: {error}"));
    }
}

/// Bring everything that is derived back in line with the state it derives
/// from: pane geometry, then the document layout, then the active section.
///
/// The layout cache is asked on every iteration and does nothing unless the
/// width or the theme actually changed, which keeps re-layout in one place
/// instead of scattered across the handlers that can trigger it.
pub fn reconcile(app: &mut App, area: Rect) {
    app.panes = layout::compute(area, app);
    let options = LayoutOptions {
        width: app.panes.content_width,
        // Source files always get line numbers, matching glow.
        code_line_numbers: app.options.line_numbers || app.doc.source.is_code,
        preserve_new_lines: app.options.preserve_new_lines,
    };
    let theme = app.theme.clone();
    app.view.top = app.doc.ensure_rendered(options, &theme, app.view.top);
    derived::sync(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Base;
    use crate::theme::ThemeVariant;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn press(code: KeyCode) -> event::Event {
        event::Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn run_keys(text: &str, keys: &[KeyCode]) -> App {
        let mut app = App::new(
            Source::from_text(text, None, "t.md".into(), Base::Cwd),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        );
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        let mut events = event::ScriptedEvents::new(keys.iter().copied().map(press));
        drive(&mut app, &mut terminal, &mut events).unwrap();
        app
    }

    fn long() -> String {
        (1..=100)
            .map(|n| format!("## Heading {n}\n\nbody {n}\n\n"))
            .collect()
    }

    #[test]
    fn the_loop_exits_when_the_reader_quits() {
        let app = run_keys(&long(), &[KeyCode::Char('j'), KeyCode::Char('q')]);
        assert!(app.should_quit);
        assert_eq!(app.view.top, 1);
    }

    #[test]
    fn the_loop_exits_when_input_runs_out() {
        let app = run_keys(&long(), &[KeyCode::Char('j')]);
        assert!(!app.should_quit);
    }

    #[test]
    fn a_document_is_laid_out_before_the_first_frame() {
        let app = run_keys("# Title\n\nbody\n", &[]);
        assert!(!app.doc.doc().lines.is_empty());
        assert_eq!(app.doc.revision(), 1);
    }

    #[test]
    fn switching_theme_re_lays_out_but_keeps_the_reading_position() {
        let text = long();
        let app = run_keys(
            &text,
            &[
                KeyCode::Char('d'),
                KeyCode::Char('d'),
                KeyCode::Char('T'),
                KeyCode::Char('q'),
            ],
        );
        assert_eq!(app.theme.name, "paper");
        assert_eq!(app.doc.revision(), 2, "theme change did not re-lay out");
        assert!(app.view.top > 0, "the reading position was lost");
    }
}
