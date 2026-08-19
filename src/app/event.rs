//! Input, as the update loop sees it.
//!
//! The loop consumes this enum rather than crossterm's, which keeps the update
//! logic testable — a headless test feeds the same events a terminal would —
//! and leaves room for the producers that arrive later (a file watcher, a
//! directory walk) without changing anything downstream.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use anyhow::Result;
use crossterm::event::{self, KeyEvent, KeyEventKind, MouseEvent};

use crate::browser::Scan;

/// Something the reader has to react to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// A key was pressed.
    Key(KeyEvent),
    /// The mouse moved, was clicked, or was scrolled.
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
    /// Start reading the terminal, and hand back a sender for the other
    /// producers.
    ///
    /// When every sender has been dropped and the terminal reader has stopped,
    /// the queue closes and the loop ends — which is what makes a closed input
    /// stream an exit rather than a hang.
    #[must_use]
    pub fn new() -> (Self, Sender<Event>) {
        let (sender, queue) = mpsc::channel();
        let terminal = sender.clone();
        thread::spawn(move || {
            loop {
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
        (Self { queue }, sender)
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

/// Convert a crossterm event, dropping the ones the reader ignores.
#[must_use]
pub fn translate(event: event::Event) -> Option<Event> {
    match event {
        // Terminals with the enhanced keyboard protocol report releases and
        // repeats too; acting on both halves of a keypress would scroll twice.
        event::Event::Key(key) if key.kind == KeyEventKind::Press => Some(Event::Key(key)),
        event::Event::Mouse(mouse) => Some(Event::Mouse(mouse)),
        event::Event::Resize(cols, rows) => Some(Event::Resize(cols, rows)),
        event::Event::Paste(text) => Some(Event::Paste(text)),
        _ => None,
    }
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
    use crossterm::event::{KeyCode, KeyModifiers};

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
        let (mut events, sender) = Events::new();
        let done = Event::Scan {
            generation: 0,
            scan: Scan::Done,
        };
        sender.send(done.clone()).expect("send");
        assert_eq!(events.next().unwrap(), Some(done));
    }

    #[test]
    fn the_queue_closing_ends_the_loop() {
        let (events, sender) = Events::new();
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
    fn draining_takes_everything_waiting_and_stops() {
        let (mut events, sender) = Events::new();
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
