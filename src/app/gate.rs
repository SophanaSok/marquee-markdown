//! Standing the terminal reader down while another program owns the terminal.
//!
//! Exactly one thread in this process may read the terminal: crossterm's
//! reader is process-global, and two readers on one tty split escape sequences
//! between them. That is harmless while the reader is the only program
//! running, and wrong the moment an editor is handed the terminal — both
//! processes then block on the same device and each takes an arbitrary half of
//! everything typed.
//!
//! So the reader is stood down for the duration. [`Gate::pause`] does not
//! merely set a flag: it waits until every reader has acknowledged from the
//! top of its loop, which is the only point at which one is provably not
//! inside a read. That handshake is what makes it safe for the pausing thread
//! to read the terminal itself, which is how the leftovers of somebody else's
//! session get thrown away on the way back.
//!
//! The gate is process-global for the same reason [`super::terminal::enter`]
//! and [`super::terminal::restore`] are: the thing it guards is. A run with no
//! reader at all — every headless test — registers nothing, and pausing is
//! then a no-op that returns without waiting for anything.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// How long a reader waits for input before looking up to see whether it has
/// been asked to stand down.
///
/// This is the one knob, and it is a trade rather than a tuning: it bounds how
/// long [`pause`] blocks before an editor can start, so it wants to be small,
/// and the price is a wakeup this often while nothing is happening.
///
/// Twenty a second is not a new cost. `notify-debouncer-full` takes a tick of
/// a quarter of its settle time, so whenever a local file is open this process
/// already wakes and scans a debounce map some sixteen times a second. What
/// this must not become is a blocking read: a thread parked in one cannot be
/// asked to stand down at all, which is the whole bug.
pub const TICK: Duration = Duration::from_millis(50);

/// Longest [`pause`] waits for an acknowledgement before going ahead anyway.
///
/// A reader that has neither parked nor deregistered within this is wedged
/// somewhere unexpected, and an editor that never opens is a worse outcome for
/// the reader than one that opens having lost a keystroke.
const ACKNOWLEDGE: Duration = Duration::from_millis(500);

/// The one gate the terminal reader answers to.
static GATE: Gate = Gate::new();

/// Shut the gate, and wait for the terminal reader to stand down.
///
/// Input resumes when the returned guard is dropped, including while a panic
/// is unwinding through the caller.
#[must_use = "input stays paused until the guard is dropped"]
pub fn pause() -> Paused<'static> {
    GATE.pause()
}

/// Register as the terminal reader for as long as the guard lives.
#[must_use = "the reader is only registered while the guard lives"]
pub fn join() -> Reader<'static> {
    GATE.join()
}

#[derive(Debug)]
struct State {
    /// How many pauses are outstanding.
    ///
    /// A count rather than a flag: with a flag, the first of two overlapping
    /// pauses to be dropped would reopen the gate under the second, and the
    /// "provably not inside a read" guarantee would silently stop holding.
    /// Today's two callers are driven serially from one place, but that is a
    /// fact about the callers, and this is the module whose whole job is not
    /// trusting facts about callers.
    pauses: usize,
    /// How many readers are registered.
    live: usize,
    /// How many of those are parked, waiting to be let go.
    parked: usize,
}

impl State {
    /// Whether readers have been asked to stand down.
    const fn closed(&self) -> bool {
        self.pauses > 0
    }
}

/// A gate that readers park at.
///
/// Public, and not hard-wired to the static, so its tests can drive their own
/// instance rather than the one the running program depends on.
#[derive(Debug)]
pub struct Gate {
    state: Mutex<State>,
    change: Condvar,
}

impl Gate {
    /// An open gate with no readers.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                pauses: 0,
                live: 0,
                parked: 0,
            }),
            change: Condvar::new(),
        }
    }

    /// The lock, ignoring poisoning.
    ///
    /// A panic elsewhere is not a reason to stop letting the reader read. What
    /// this lock protects is three integers with no invariant a panic can
    /// leave half-applied, and treating poisoning as fatal would turn any
    /// unrelated panic into a terminal that never accepts another key.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Register a reader.
    pub fn join(&self) -> Reader<'_> {
        self.lock().live += 1;
        Reader { gate: self }
    }

    /// Shut the gate and wait until every reader has parked.
    pub fn pause(&self) -> Paused<'_> {
        let mut state = self.lock();
        state.pauses += 1;
        self.change.notify_all();
        // A reader that has gone — a closed input stream, a run with no
        // terminal at all — is not waited for, because it decremented `live`
        // on the way out. A run that never had one, which is every headless
        // test, waits for nothing at all.
        let _ = self
            .change
            .wait_timeout_while(state, ACKNOWLEDGE, |state| state.parked < state.live)
            .unwrap_or_else(PoisonError::into_inner);
        Paused { gate: self }
    }

    /// Whether the gate is shut, how many readers are registered, and how many
    /// have parked. For assertions.
    #[must_use]
    pub fn snapshot(&self) -> (bool, usize, usize) {
        let state = self.lock();
        (state.closed(), state.live, state.parked)
    }
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

/// A registered reader.
///
/// Deregisters on drop, so a reader that stops — or panics — is never waited
/// for. A reader nobody can stop waiting for is what would turn [`pause`] into
/// a hang.
#[derive(Debug)]
pub struct Reader<'a> {
    gate: &'a Gate,
}

impl Reader<'_> {
    /// Park while the gate is shut.
    ///
    /// Called at the top of the reader's loop and nowhere else: what [`pause`]
    /// promises its caller is that a parked reader is not inside a read, and
    /// that is only true here.
    pub fn wait_while_paused(&self) {
        let mut state = self.gate.lock();
        if !state.closed() {
            return;
        }
        state.parked += 1;
        // Release whoever is waiting on the handshake.
        self.gate.change.notify_all();
        while state.closed() {
            state = self
                .gate
                .change
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        state.parked -= 1;
    }
}

impl Drop for Reader<'_> {
    fn drop(&mut self) {
        self.gate.lock().live -= 1;
        // Somebody may be waiting for an acknowledgement this reader will now
        // never send.
        self.gate.change.notify_all();
    }
}

/// Input is paused for as long as this lives.
#[derive(Debug)]
pub struct Paused<'a> {
    gate: &'a Gate,
}

impl Drop for Paused<'_> {
    fn drop(&mut self) {
        // One pause fewer; the gate opens when the last one goes.
        self.gate.lock().pauses -= 1;
        self.gate.change.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::thread::{self, JoinHandle};

    /// A reader thread with nothing to read.
    ///
    /// It raises a flag while it is "inside a read", which is exactly the
    /// thing a pause has to have ended. Asserting on that flag rather than on
    /// elapsed time is what makes these tests deterministic.
    struct Fake {
        working: Arc<AtomicBool>,
        rounds: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    }

    impl Fake {
        /// Start a reader against `gate`, which must outlive it. The thread is
        /// always joined in `Drop`, which is what makes that true.
        fn start(gate: &'static Gate) -> Self {
            let working = Arc::new(AtomicBool::new(false));
            let rounds = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let handle = thread::spawn({
                let (working, rounds, stop) = (working.clone(), rounds.clone(), stop.clone());
                move || {
                    let reader = gate.join();
                    while !stop.load(Ordering::SeqCst) {
                        reader.wait_while_paused();
                        working.store(true, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(1));
                        working.store(false, Ordering::SeqCst);
                        rounds.fetch_add(1, Ordering::SeqCst);
                    }
                }
            });
            Self {
                working,
                rounds,
                stop,
                handle: Some(handle),
            }
        }

        /// Wait until the reader has been round the loop at least once, so a
        /// test is not racing its startup.
        fn started(&self) {
            while self.rounds.load(Ordering::SeqCst) == 0 {
                thread::yield_now();
            }
        }

        fn rounds(&self) -> usize {
            self.rounds.load(Ordering::SeqCst)
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    /// A gate of its own for each test that needs one, leaked so that readers
    /// can borrow it for `'static`. Tests never touch the process-wide gate,
    /// which is what lets them run alongside each other.
    fn gate() -> &'static Gate {
        Box::leak(Box::new(Gate::new()))
    }

    #[test]
    fn a_pause_waits_until_the_reader_is_out_of_the_read() {
        // The whole fix, in one assertion: once `pause` has returned, the
        // reader is not inside a read and cannot enter one. Handing an editor
        // the terminal while it was would cost that editor its first
        // keystrokes.
        let gate = gate();
        let fake = Fake::start(gate);
        fake.started();
        let paused = gate.pause();
        for _ in 0..30 {
            assert!(
                !fake.working.load(Ordering::SeqCst),
                "the reader was still reading after a pause returned"
            );
            thread::sleep(Duration::from_millis(1));
        }
        drop(paused);
    }

    #[test]
    fn a_paused_reader_does_no_work_until_it_is_let_go() {
        let gate = gate();
        let fake = Fake::start(gate);
        fake.started();
        let paused = gate.pause();
        let stopped = fake.rounds();
        thread::sleep(Duration::from_millis(40));
        assert_eq!(fake.rounds(), stopped, "a parked reader kept going");
        drop(paused);
        while fake.rounds() == stopped {
            thread::yield_now();
        }
    }

    #[test]
    fn pausing_with_no_reader_does_not_block() {
        // Every headless run: `App.events` is `None`, so nothing ever reads
        // the terminal and there is nobody to wait for.
        let gate = gate();
        let start = std::time::Instant::now();
        drop(gate.pause());
        assert!(start.elapsed() < ACKNOWLEDGE / 4);
    }

    #[test]
    fn a_reader_that_has_gone_is_not_waited_for() {
        // A closed input stream, or no terminal at all. Waiting for an
        // acknowledgement that can never come would hang the editor.
        let gate = gate();
        drop(gate.join());
        let start = std::time::Instant::now();
        drop(gate.pause());
        assert!(start.elapsed() < ACKNOWLEDGE / 4);
    }

    #[test]
    fn a_reader_that_panicked_is_not_waited_for() {
        let gate = gate();
        let _ = thread::spawn(move || {
            let _reader = gate.join();
            panic!("the reader fell over");
        })
        .join();
        let start = std::time::Instant::now();
        drop(gate.pause());
        assert!(start.elapsed() < ACKNOWLEDGE / 4);
    }

    #[test]
    fn the_gate_reopens_when_a_panic_unwinds_through_the_guard() {
        let gate = gate();
        let fake = Fake::start(gate);
        fake.started();
        let fell_over = catch_unwind(AssertUnwindSafe(|| {
            let _paused = gate.pause();
            panic!("the editor fell over");
        }));
        assert!(fell_over.is_err());
        assert!(!gate.snapshot().0, "the gate stayed shut");
        let stopped = fake.rounds();
        while fake.rounds() == stopped {
            thread::yield_now();
        }
    }

    #[test]
    fn a_poisoned_gate_does_not_wedge_the_reader() {
        // Recovering from poisoning rather than propagating it is deliberate:
        // an unrelated panic must not leave a terminal that never accepts
        // another key.
        let gate = gate();
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _held = gate.state.lock().unwrap();
            panic!("while holding the lock");
        }));
        let fake = Fake::start(gate);
        fake.started();
        drop(gate.pause());
        assert!(!gate.snapshot().0);
    }

    #[test]
    fn every_reader_has_to_stand_down_not_just_one() {
        let gate = gate();
        let readers: Vec<Fake> = (0..4).map(|_| Fake::start(gate)).collect();
        for reader in &readers {
            reader.started();
        }
        let paused = gate.pause();
        for reader in &readers {
            assert!(
                !reader.working.load(Ordering::SeqCst),
                "a reader was still reading after a pause returned"
            );
        }
        drop(paused);
    }

    #[test]
    fn overlapping_pauses_keep_the_gate_shut_until_the_last_lets_go() {
        // Two callers pausing at once is unreachable today — both are driven
        // serially from `app::perform` — but the guarantee must not depend on
        // that staying true. With a flag instead of a count, dropping the
        // first pause would reopen the gate under the second.
        let gate = gate();
        let fake = Fake::start(gate);
        fake.started();
        let first = gate.pause();
        let second = gate.pause();
        drop(first);
        assert!(gate.snapshot().0, "the gate reopened under a live pause");
        let stopped = fake.rounds();
        thread::sleep(Duration::from_millis(20));
        assert_eq!(
            fake.rounds(),
            stopped,
            "a reader resumed under a live pause"
        );
        drop(second);
        assert!(!gate.snapshot().0);
        while fake.rounds() == stopped {
            thread::yield_now();
        }
    }

    #[test]
    fn pausing_and_resuming_repeatedly_leaves_the_counts_where_they_started() {
        // The leak detector for the parked arithmetic: a count that drifts
        // would eventually make `pause` wait for an acknowledgement that has
        // already been given.
        let gate = gate();
        let fake = Fake::start(gate);
        fake.started();
        for _ in 0..100 {
            drop(gate.pause());
        }
        drop(fake);
        assert_eq!(gate.snapshot(), (false, 0, 0));
    }
}
