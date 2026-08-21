//! Input, as the update loop sees it.
//!
//! The loop consumes this enum rather than crossterm's, which keeps the update
//! logic testable — a headless test feeds the same events a terminal would —
//! and leaves room for the producers that arrive later (a file watcher, a
//! directory walk) without changing anything downstream.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind};

use super::gate;
use crate::browser::Scan;

/// Something the reader has to react to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// A key was pressed.
    Key(KeyEvent),
    /// The wheel was scrolled, and only the wheel: `translate` drops every
    /// other mouse report before it reaches here.
    Mouse(MouseEvent),
    /// The terminal changed size.
    Resize(u16, u16),
    /// Text was pasted in one go.
    Paste(String),
    /// The directory walk reported in. The generation says *which* walk: a
    /// rescan starts a new one, and batches from the walk it replaced must
    /// not repopulate a list that was just cleared.
    Scan {
        /// Which walk this report belongs to.
        generation: u64,
        /// What it found.
        scan: Scan,
    },
    /// The document changed on disk.
    Reload,
}

/// Where events come from.
///
/// `Ok(None)` means the source is exhausted and the reader should exit; that
/// is what a closed input stream looks like.
pub trait EventSource {
    /// Block until the next event.
    ///
    /// # Errors
    /// Propagates failures from the underlying input stream.
    fn next(&mut self) -> Result<Option<Event>>;

    /// Take every event already waiting, without blocking.
    ///
    /// The loop drains after each blocking receive so a burst — a window being
    /// dragged, a directory walk reporting in — costs one re-layout and one
    /// frame rather than one of each per event. Sources with nothing to
    /// coalesce may leave this alone.
    ///
    /// # Errors
    /// Propagates failures from the underlying input stream.
    fn drain(&mut self, out: &mut Vec<Event>) -> Result<()> {
        let _ = out;
        Ok(())
    }
}

/// Everything that can wake the reader, funnelled into one queue.
///
/// The terminal is read on its own thread and posts into the same channel the
/// directory walk posts into, so the loop waits in one place. Without that,
/// a walk finishing would not redraw until the reader happened to press a key.
#[derive(Debug)]
pub struct Events {
    queue: Receiver<Event>,
}

impl Events {
    /// The queue and its sender, with nothing reading the terminal.
    ///
    /// Split out from [`Events::new`] so that a test can exercise the queue —
    /// which is ordinary channel plumbing — without starting a thread that
    /// would compete with `cargo test` for the developer's own terminal.
    #[must_use]
    pub fn channel() -> (Self, Sender<Event>) {
        let (sender, queue) = mpsc::channel();
        (Self { queue }, sender)
    }

    /// Start reading the terminal, and hand back a sender for the other
    /// producers.
    ///
    /// When every sender has been dropped and the terminal reader has stopped,
    /// the queue closes and the loop ends — which is what makes a closed input
    /// stream an exit rather than a hang.
    #[must_use]
    pub fn new() -> (Self, Sender<Event>) {
        let (events, sender) = Self::channel();
        let terminal = sender.clone();
        thread::spawn(move || {
            // Registered for as long as this thread reads, and deregistered
            // however it stops — a closed stream, a terminal that was never
            // there, or a panic. A reader nobody can stop waiting for is what
            // would turn `gate::pause` into a hang.
            let reader = gate::join();
            loop {
                // The only point at which this thread is provably not inside a
                // read, and so the only place it may stand down. While an
                // editor has the terminal, a thread still reading it takes an
                // arbitrary half of every keystroke meant for that editor —
                // and the replies to the questions it asks on the way up.
                reader.wait_while_paused();
                // Not a blocking `read`: a thread parked in one cannot be
                // asked to stand down at all.
                match event::poll(gate::TICK) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(_) => return,
                }
                // Guaranteed not to block: `poll` said there was something.
                let Ok(event) = event::read() else {
                    return;
                };
                // Events the reader has no use for are dropped here rather
                // than carried through the update loop as a no-op case.
                if let Some(event) = translate(event)
                    && terminal.send(event).is_err()
                {
                    return;
                }
            }
        });
        (events, sender)
    }
}

impl EventSource for Events {
    fn next(&mut self) -> Result<Option<Event>> {
        Ok(self.queue.recv().ok())
    }

    fn drain(&mut self, out: &mut Vec<Event>) -> Result<()> {
        while let Ok(event) = self.queue.try_recv() {
            out.push(event);
        }
        Ok(())
    }
}

/// Throw away terminal input left over from another program.
///
/// An editor asks the terminal a series of questions on the way up — what it
/// is, what it supports, where the cursor is — and gives up on the ones that
/// go unanswered. Those answers still arrive, often after the editor has
/// exited, and crossterm parses them into perfectly ordinary-looking keys: a
/// device-attributes reply is an escape, a bracket and some digits, which is a
/// handful of bindings fired at a document the reader was not looking at.
///
/// The queue of already-translated events is deliberately left alone. Keys
/// typed before the editor started are the reader's and belong to the reader;
/// only what the terminal has to say about somebody else's session goes.
///
/// Taking the guard is not decoration. Crossterm forbids reading from two
/// threads, so this is sound only while the terminal reader is standing down,
/// and asking for the proof in the signature is what stops it being called
/// anywhere else.
///
/// Not reachable from a test: it needs a terminal with something left in it.
/// `scripts/handoff-check.py` covers it.
pub fn discard_pending_input(_paused: &gate::Paused<'_>) {
    // A quiet window rather than one instantaneous sweep. The dangerous reply
    // is the late one — the query the other program had already timed out on —
    // and a sweep that runs the microsecond it exits is the one that misses it.
    let deadline = Instant::now() + SETTLE;
    while Instant::now() < deadline {
        // Not `Duration::ZERO`: `poll` also reports "nothing waiting" when it
        // could not take crossterm's own lock within the timeout.
        match event::poll(QUIET) {
            Ok(true) => {
                if event::read().is_err() {
                    return;
                }
            }
            _ => return,
        }
    }
}

/// How long the terminal has to stay quiet before its leftovers are believed
/// to be finished.
const QUIET: Duration = Duration::from_millis(15);

/// A ceiling on the above, so a held-down key cannot stall the handover.
const SETTLE: Duration = Duration::from_millis(120);

/// Drop all but the first reload in a batch.
///
/// Reloading reads the file as it is now, so a second reload in the same batch
/// can only produce what the first already did — at the cost of another parse
/// and another layout of the whole document. They arrive in bunches after an
/// editing session, because every save the watcher noticed while the editor
/// had the terminal is still sitting in the queue.
///
/// The first is kept rather than the last so that a key later in the batch
/// still acts on the document it was aimed at.
pub fn coalesce(batch: &mut Vec<Event>) {
    let mut reloaded = false;
    batch
        .retain(|event| !matches!(event, Event::Reload) || !std::mem::replace(&mut reloaded, true));
}

/// Convert a crossterm event, dropping the ones the reader ignores.
#[must_use]
pub fn translate(event: event::Event) -> Option<Event> {
    match event {
        // Terminals with the enhanced keyboard protocol report releases and
        // repeats too; acting on both halves of a keypress would scroll twice.
        event::Event::Key(key) if key.kind == KeyEventKind::Press => Some(Event::Key(key)),
        event::Event::Mouse(mouse) if is_wheel(mouse.kind) => Some(Event::Mouse(mouse)),
        event::Event::Resize(cols, rows) => Some(Event::Resize(cols, rows)),
        event::Event::Paste(text) => Some(Event::Paste(text)),
        _ => None,
    }
}

/// Whether a mouse report is one of the wheel's.
///
/// The wheel is the only thing this program does with a mouse, and
/// `terminal::setup` asks the terminal for no more than that — but a mode
/// change is a request, not a guarantee. A terminal that ignores it, a report
/// already in flight when it went out, and the Windows console, where mouse
/// input is one flag with no separate motion bit, all still deliver movement.
/// Dropped here alongside the key releases rather than carried into the update
/// loop: reaching `update::mouse_event` only to fall through its catch-all arm
/// costs a reconcile and a whole frame drawn and diffed away, once per cell the
/// pointer crosses.
const fn is_wheel(kind: MouseEventKind) -> bool {
    matches!(
        kind,
        MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
    )
}

/// An event source backed by a fixed list, for tests.
#[derive(Debug, Default)]
pub struct ScriptedEvents {
    queue: std::collections::VecDeque<Event>,
}

impl ScriptedEvents {
    /// Build a source that yields `events` and then reports exhaustion.
    #[must_use]
    pub fn new(events: impl IntoIterator<Item = Event>) -> Self {
        Self {
            queue: events.into_iter().collect(),
        }
    }
}

impl EventSource for ScriptedEvents {
    fn next(&mut self) -> Result<Option<Event>> {
        Ok(self.queue.pop_front())
    }
    // Deliberately not draining: a scripted run applies one event per
    // iteration, so a test sees every intermediate frame the reader would.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers, MouseButton};

    #[test]
    fn key_releases_are_dropped_so_a_press_acts_once() {
        let mut key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert!(translate(event::Event::Key(key)).is_some());
        key.kind = KeyEventKind::Release;
        assert!(translate(event::Event::Key(key)).is_none());
        key.kind = KeyEventKind::Repeat;
        assert!(translate(event::Event::Key(key)).is_none());
    }

    #[test]
    fn only_the_wheel_survives_translation() {
        // Everything else is a report this program asked the terminal not to
        // send and would throw away on arrival — after a redraw.
        let report = |kind| {
            event::Event::Mouse(MouseEvent {
                kind,
                column: 4,
                row: 2,
                modifiers: KeyModifiers::NONE,
            })
        };
        for kind in [
            MouseEventKind::ScrollUp,
            MouseEventKind::ScrollDown,
            MouseEventKind::ScrollLeft,
            MouseEventKind::ScrollRight,
        ] {
            assert!(translate(report(kind)).is_some(), "{kind:?} was dropped");
        }
        for kind in [
            MouseEventKind::Moved,
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
        ] {
            assert!(
                translate(report(kind)).is_none(),
                "{kind:?} reached the loop"
            );
        }
    }

    #[test]
    fn focus_changes_are_dropped() {
        assert!(translate(event::Event::FocusGained).is_none());
        assert!(translate(event::Event::FocusLost).is_none());
    }

    #[test]
    fn a_scripted_source_runs_dry() {
        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        let mut events = ScriptedEvents::new([Event::Key(key)]);
        assert_eq!(events.next().unwrap(), Some(Event::Key(key)));
        assert_eq!(events.next().unwrap(), None);
    }

    #[test]
    fn a_walk_report_reaches_the_loop_through_the_same_queue_as_a_key() {
        let (mut events, sender) = Events::channel();
        let done = Event::Scan {
            generation: 0,
            scan: Scan::Done,
        };
        sender.send(done.clone()).expect("send");
        assert_eq!(events.next().unwrap(), Some(done));
    }

    #[test]
    fn the_queue_closing_ends_the_loop() {
        let (events, sender) = Events::channel();
        drop(sender);
        // The terminal reader thread still holds a sender, so this stays open;
        // what matters is that `next` reports exhaustion rather than blocking
        // once every producer is gone. Exercised through the scripted source,
        // which has no threads.
        let mut scripted = ScriptedEvents::default();
        assert_eq!(scripted.next().unwrap(), None);
        drop(events);
    }

    #[test]
    fn the_queue_behaves_the_same_with_nothing_reading_the_terminal() {
        // The split exists so the tests above need no reader thread; this is
        // what stops it rotting into two queues that differ.
        let (mut events, sender) = Events::channel();
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        sender.send(Event::Key(key)).expect("send");
        assert_eq!(events.next().unwrap(), Some(Event::Key(key)));
        drop(sender);
        assert_eq!(events.next().unwrap(), None, "a closed queue must exhaust");
    }

    #[test]
    fn only_the_first_reload_in_a_batch_survives() {
        // Three saves during one editing session: the file is read once when
        // the batch is applied, so the other two are a parse and a layout of
        // the whole document whose result is thrown away.
        let mut batch = vec![Event::Reload, Event::Reload, Event::Reload];
        coalesce(&mut batch);
        assert_eq!(batch, vec![Event::Reload]);
    }

    #[test]
    fn coalescing_keeps_the_keys_and_their_order() {
        let key = |c| Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        let mut batch = vec![Event::Reload, key('j'), Event::Reload, key('k')];
        coalesce(&mut batch);
        // The reload stays where it was, so a key later in the batch still
        // acts on the document it was aimed at.
        assert_eq!(batch, vec![Event::Reload, key('j'), key('k')]);
    }

    #[test]
    fn coalescing_a_batch_with_no_reloads_changes_nothing() {
        let key = |c| Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        let mut batch = vec![key('j'), key('k')];
        let untouched = batch.clone();
        coalesce(&mut batch);
        assert_eq!(batch, untouched);
    }

    #[test]
    fn draining_takes_everything_waiting_and_stops() {
        let (mut events, sender) = Events::channel();
        for _ in 0..3 {
            sender
                .send(Event::Scan {
                    generation: 0,
                    scan: Scan::Done,
                })
                .expect("send");
        }
        let mut batch = Vec::new();
        events.drain(&mut batch).expect("drain");
        assert_eq!(batch.len(), 3);
        batch.clear();
        events.drain(&mut batch).expect("drain");
        assert!(batch.is_empty(), "drain blocked or invented events");
    }
}
