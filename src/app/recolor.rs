//! Keeping a palette in step with what it was resolved from.
//!
//! `--style system` builds its colors out of what the terminal answered, and a
//! theme file out of what the file said. Both can change underneath a reader
//! who is part-way down a document: a desktop theme switch retints every
//! terminal on the screen, and a palette author saves the file they are
//! tuning. Neither used to reach the page until the next run.
//!
//! Nothing here decides *when* to do this. Four things can ask —
//! [`super::event::Event::Recolor`] arrives from a focus regain, a watched
//! path settling, a signal, and the key bound to
//! [`super::action::Action::Recolor`] — and they all funnel into one request
//! so that a burst is one round trip rather than four.
//!
//! ## Why this may ask the terminal a question and almost nothing else may
//!
//! An `OSC` reply and a keystroke are the same bytes on the same stream, so
//! asking while the reader thread is reading would hand the answer to the
//! wrong reader and leave this one waiting for it. [`gate::pause`] is what
//! makes it safe: it does not return until every reader has parked at the top
//! of its loop, which is the only point at which one is provably not inside a
//! read. That is the same handshake [`super::external::run`] takes before
//! handing an editor the terminal, and the reason this is a `Request` rather
//! than something the update loop does inline.
//!
//! Two things follow from that and are not optional:
//!
//! - **The guard is held across the whole exchange**, and
//!   [`super::event::discard_pending_input`] is called before it drops. A
//!   terminal that answers after the deadline answers anyway, and a late
//!   `OSC` reply parses into perfectly ordinary-looking keys.
//! - **The screen is not given back.** Unlike an editor handoff there is no
//!   other program to make room for, so the alternate screen and raw mode stay
//!   exactly as they are and the reader sees nothing at all.
//!
//! ## What it costs when nothing changed
//!
//! Which is the common case, because focus is regained far more often than a
//! theme is switched. Three guards, in the order they cut work out:
//!
//! 1. A reader not following anything never gets here — the update loop drops
//!    the trigger.
//! 2. A terminal that answered nothing when first asked is never asked again
//!    ([`super::state::Options::terminal_answers`]), so `screen`, a dumb terminal and every
//!    Windows console pay nothing rather than the timeout, forever.
//! 3. A probe asks for the background alone — two sequences, not nineteen
//!    ([`osc::Ask::Background`]) — and only a background that actually moved
//!    pays for the full palette read.
//!
//! With the rate limit on top, alt-tabbing costs at most one two-sequence
//! round trip every [`COOLDOWN`], and usually nothing.

use std::time::{Duration, Instant};

use super::gate;
use super::state::App;
use crate::theme::registry;
use crate::util::osc;

/// Shortest gap between two questions to the terminal.
///
/// Triggers arrive in bursts: one theme switch is a watched path settling
/// *and* a focus regain when the switcher window closes, and on some desktops
/// a signal as well. They describe the same change, so answering the first and
/// ignoring its echoes is right as well as cheap.
///
/// Long enough to swallow a burst, short enough that a reader who switches
/// theme twice on purpose sees the second one. The manual key is deliberately
/// not subject to it: somebody pressing a key has said the rate limit was
/// wrong.
pub const COOLDOWN: Duration = Duration::from_millis(500);

/// Carry out a recolor.
///
/// Called only from the loop, which is the only place that has the terminal.
/// `loud` says whether to report an unchanged palette and a failed theme file:
/// a key press is a question that deserves an answer, and a focus regain is
/// not — a message on every alt-tab would be its own kind of broken.
pub fn run(app: &mut App, loud: bool) {
    let Some(style) = app.following.clone() else {
        return;
    };

    if registry::follows_the_terminal(&style) {
        match ask(app, loud) {
            Some(colors) => app.options.terminal = colors,
            // Rate-limited, silent, or the terminal had nothing new to say.
            // Re-resolving anyway would be a theme rebuilt from the same
            // answer, which is work with no outcome.
            None => return,
        }
    }

    match registry::resolve(&style, &app.options.terminal) {
        Ok(theme) => {
            // Comparing rather than assigning unconditionally so that the
            // status line is not written for a change nobody made, and so a
            // reader can tell "it followed" from "there was nothing to
            // follow".
            if theme == app.theme {
                if loud {
                    app.message = Some(format!("{style} is already up to date"));
                }
                return;
            }
            // The light/dark counterpart is derived from the theme in force,
            // so it has to move with it or `T` would toggle to the palette
            // that was the counterpart of a theme no longer on screen.
            app.alternate = super::state::counterpart(&theme);
            app.theme = theme;
            if loud {
                app.message = Some(format!("reloaded {style}"));
            }
        }
        // A theme file somebody is editing is malformed for as long as it
        // takes them to finish the line. Automatic triggers keep the last
        // theme that worked and say nothing, because a watcher firing on every
        // keystroke would otherwise fill the status bar with the same error.
        Err(error) => {
            if loud {
                app.message = Some(format!("cannot reload {style}: {error}"));
            }
        }
    }
}

/// Ask the terminal what colors it is using now, if it is worth asking.
///
/// `None` means "nothing to act on": too soon, a terminal that never answers,
/// or one whose background has not moved. Only a real change returns a full
/// answer, and only then has the sixteen-slot read been paid for.
fn ask(app: &mut App, loud: bool) -> Option<crate::theme::system::TerminalColors> {
    if !app.options.terminal_answers {
        if loud {
            app.message = Some("this terminal does not report its colors".to_owned());
        }
        return None;
    }
    if !loud && !cooled_down(app, Instant::now()) {
        return None;
    }

    // From here the reader is stood down and this thread is the only one on
    // the terminal. Everything between here and the drop has to be quick:
    // keys pressed meanwhile are queued by the terminal, not lost, but the
    // reader is not being redrawn either.
    let paused = gate::pause();
    let probe = osc::query_for(osc::Ask::Background, osc::TIMEOUT);
    let answer = match probe.bg {
        // The probe went unanswered. Nothing was learned, and the full read
        // would learn the same nothing at eight times the cost — so this is
        // where a terminal that has gone quiet stops costing anything, rather
        // than where it starts costing two timeouts per trigger.
        None => None,
        // The background is what a colorscheme changes first and most, so an
        // unchanged one means an unchanged scheme.
        Some(bg) if Some(bg) == app.options.terminal.bg => None,
        // It moved. Now the sixteen slots are worth asking for.
        Some(_) => Some(osc::query_for(osc::Ask::Everything, osc::TIMEOUT)),
    };
    // Before the guard drops, and for the same reason the editor handoff does
    // it: a reply that arrived after the deadline is still on its way in, and
    // crossterm would parse it into a handful of bindings.
    super::event::discard_pending_input(&paused);
    drop(paused);

    // A terminal that answered the probe and then said nothing to the full
    // question has not gone quiet for good — it answered a moment ago — so
    // this is a lost answer rather than a silent terminal, and keeping the
    // palette that works is better than rebuilding from nothing.
    answer.filter(|colors| colors.bg.is_some())
}

/// Whether enough time has passed since the last question, recording this one
/// when it has.
fn cooled_down(app: &mut App, now: Instant) -> bool {
    if let Some(last) = app.last_probe
        && now.duration_since(last) < COOLDOWN
    {
        return false;
    }
    app.last_probe = Some(now);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::Options;
    use crate::source::{Base, Source};
    use crate::theme::{Theme, ThemeVariant};

    /// A reader with no terminal and no threads, following `system`.
    fn app() -> App {
        App::new(
            Source::from_text("# T\n\nbody\n", None, "t.md".into(), Base::Cwd),
            Theme::new(ThemeVariant::Slate),
            Options {
                style: registry::SYSTEM.to_owned(),
                terminal_answers: true,
                ..Options::default()
            },
        )
    }

    #[test]
    fn a_burst_of_triggers_costs_one_question() {
        // One theme switch is a watched path *and* a focus regain, and on some
        // desktops a signal too. They describe the same change.
        let mut app = app();
        let start = Instant::now();
        assert!(
            cooled_down(&mut app, start),
            "the first ask must go through"
        );
        assert!(!cooled_down(&mut app, start + COOLDOWN / 2));
        assert!(!cooled_down(
            &mut app,
            start + COOLDOWN - Duration::from_millis(1)
        ));
        assert!(cooled_down(&mut app, start + COOLDOWN));
    }

    #[test]
    fn the_cooldown_runs_from_the_last_question_not_the_first() {
        let mut app = app();
        let start = Instant::now();
        assert!(cooled_down(&mut app, start));
        assert!(cooled_down(&mut app, start + COOLDOWN));
        // Half a cooldown after the second, which is a cooldown and a half
        // after the first: a window anchored to the first would let this in.
        assert!(!cooled_down(&mut app, start + COOLDOWN + COOLDOWN / 2));
    }

    #[test]
    fn a_terminal_that_never_answered_is_never_asked_again() {
        // The guard that keeps this free under `screen`, behind a pipe, and on
        // Windows. Silence is taken as final on purpose.
        let mut app = app();
        app.options.terminal_answers = false;
        app.following = Some(registry::SYSTEM.to_owned());
        let before = app.last_probe;
        run(&mut app, false);
        assert_eq!(app.last_probe, before, "a silent terminal was asked again");
    }

    #[test]
    fn a_reader_following_nothing_is_left_alone() {
        let mut app = app();
        app.following = None;
        let theme = app.theme.clone();
        run(&mut app, false);
        assert_eq!(app.theme, theme);
        assert!(app.message.is_none());
    }
}
