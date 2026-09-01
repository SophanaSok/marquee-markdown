//! Noticing that the document changed on disk.
//!
//! Watching the *directory* rather than the file is deliberate: most editors
//! save by writing a temporary file and renaming it over the original, which
//! replaces the inode. A watch on the file itself survives exactly one save
//! and then silently stops working, which is worse than not watching at all.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

/// How long to wait for the writes to settle. Editors often touch a file
/// several times in a row, and reloading on each one would flicker.
const SETTLE: Duration = Duration::from_millis(250);

/// A running watch. Dropping it stops the watching.
pub type Watch = Debouncer<notify::RecommendedWatcher, RecommendedCache>;

/// Watch `path` for changes, calling `on_change` when it settles.
///
/// `on_change` returns `false` when nobody is listening any more, which is how
/// the watch learns to stop.
///
/// # Errors
/// Returns an error when the path has no parent directory or the platform
/// watcher cannot be started.
pub fn spawn(path: &Path, on_change: impl Fn() -> bool + Send + 'static) -> Result<Watch> {
    let name = path
        .file_name()
        .map(std::ffi::OsString::from)
        .context("cannot watch a path with no file name")?;
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    // Canonicalize so the watch does not break when the working directory
    // changes underneath it — and because the events come back as absolute
    // paths whatever was passed in.
    let directory = std::fs::canonicalize(&directory).unwrap_or(directory);

    let mut debouncer = new_debouncer(SETTLE, None, move |result: DebounceEventResult| {
        // Errors from the platform watcher are not something a reader can act
        // on, and a document that stops auto-reloading is a smaller problem
        // than a message they cannot dismiss.
        let Ok(events) = result else {
            return;
        };
        if events.iter().any(|event| concerns(&event.paths, &name)) {
            let _ = on_change();
        }
    })
    .context("cannot start watching for changes")?;

    debouncer
        .watch(&directory, RecursiveMode::NonRecursive)
        .with_context(|| format!("cannot watch {}", directory.display()))?;
    Ok(debouncer)
}

/// Whether an event concerns the file being watched.
///
/// Matched on the file name rather than the whole path. Exactly one directory
/// is watched and it is not recursive, so the name is enough — and comparing
/// whole paths does not work: an argument like `README.md` is relative, while
/// the events always come back absolute, so nothing would ever match.
///
/// A rename reports the temporary file as well as the destination, so any path
/// in the event matching is enough.
#[must_use]
pub fn concerns(paths: &[PathBuf], name: &std::ffi::OsStr) -> bool {
    paths
        .iter()
        .any(|changed| changed.file_name() == Some(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Wait for a condition, polling, so the test is not a fixed sleep that is
    /// either flaky or slow.
    fn eventually(mut check: impl FnMut() -> bool) -> bool {
        for _ in 0..200 {
            if check() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }

    /// Throw away whatever the setup caused, so that what follows is an
    /// assertion about the change the test makes.
    ///
    /// Creating the document is a write like any other, and arming the watch
    /// immediately afterwards does not reliably exclude it: the close can be
    /// reported once the watch is up. Rare — one round in forty of four suites
    /// running at once found it, and a quiet machine never does — but a test
    /// suite running at a machine's full width meets it, which is how the nix
    /// sandbox found it while every local run and three CI platforms did not.
    ///
    /// It costs a reader one redundant re-read of a document it has only just
    /// opened, which is the same bargain the sibling case already accepts on
    /// macOS. For a test it is the difference between asserting on the change
    /// and asserting on the setup, in both directions: a positive test can
    /// pass on the leaked event without the watch working at all.
    ///
    /// Drains until the watcher goes quiet rather than for a fixed span. A
    /// sleep-then-reset is the same race with a longer fuse: the event it
    /// discards has no deadline, so a machine loaded enough to delay it past
    /// the sleep puts it back on the other side of the reset — which is the
    /// failure being fixed, only rarer and harder to place. Waiting for
    /// quiet instead extends itself exactly as far as the machine is slow.
    fn discard_setup_events(hits: &AtomicUsize) {
        let mut seen = hits.load(Ordering::SeqCst);
        let mut since = std::time::Instant::now();
        while since.elapsed() < SETTLE * 2 {
            std::thread::sleep(Duration::from_millis(25));
            let now = hits.load(Ordering::SeqCst);
            if now != seen {
                seen = now;
                since = std::time::Instant::now();
            }
        }
        hits.store(0, Ordering::SeqCst);
    }

    #[test]
    fn a_save_is_noticed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("doc.md");
        std::fs::write(&path, "# One\n").expect("write");

        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        let _watch = spawn(&path, move || {
            counter.fetch_add(1, Ordering::SeqCst);
            true
        })
        .expect("watch");
        discard_setup_events(&hits);

        std::fs::write(&path, "# Two\n").expect("write");
        assert!(
            eventually(|| hits.load(Ordering::SeqCst) > 0),
            "the change was never reported"
        );
    }

    #[test]
    fn a_save_by_rename_is_noticed_too() {
        // How most editors save. A watch on the file itself would miss this.
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("doc.md");
        let temporary = dir.path().join("doc.md.tmp");
        std::fs::write(&path, "# One\n").expect("write");

        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        let _watch = spawn(&path, move || {
            counter.fetch_add(1, Ordering::SeqCst);
            true
        })
        .expect("watch");
        discard_setup_events(&hits);

        std::fs::write(&temporary, "# Two\n").expect("write");
        std::fs::rename(&temporary, &path).expect("rename");
        assert!(
            eventually(|| hits.load(Ordering::SeqCst) > 0),
            "a rename over the file was not reported"
        );
    }

    #[test]
    // macOS reports file system events through FSEvents, which describes
    // changes at directory granularity. A sibling file changing in the watched
    // directory can therefore reach us, and the reader re-reads a document
    // that has not changed — wasteful, but not wrong, and not something this
    // side of the platform boundary can prevent.
    #[cfg_attr(target_os = "macos", ignore = "FSEvents reports per directory")]
    fn a_sibling_file_changing_is_not_reported() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("doc.md");
        std::fs::write(&path, "# One\n").expect("write");

        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        let _watch = spawn(&path, move || {
            counter.fetch_add(1, Ordering::SeqCst);
            true
        })
        .expect("watch");
        discard_setup_events(&hits);

        std::fs::write(dir.path().join("other.md"), "# Other\n").expect("write");
        // Give the watcher long enough that a false positive would have shown.
        std::thread::sleep(SETTLE * 3);
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_relative_argument_matches_the_absolute_paths_events_carry() {
        // `marquee-markdown README.md` gives a relative path, and every event
        // comes back absolute. Comparing whole paths would never match, and
        // the document would silently stop reloading.
        let name = std::ffi::OsString::from("README.md");
        assert!(concerns(
            &[PathBuf::from("/home/reader/project/README.md")],
            &name
        ));
        assert!(!concerns(
            &[PathBuf::from("/home/reader/project/OTHER.md")],
            &name
        ));
    }

    #[test]
    fn a_rename_reports_the_temporary_file_alongside_the_real_one() {
        let name = std::ffi::OsString::from("doc.md");
        assert!(concerns(
            &[PathBuf::from("/x/doc.md.tmp"), PathBuf::from("/x/doc.md")],
            &name
        ));
    }

    #[test]
    fn an_event_carrying_nothing_concerns_nothing() {
        assert!(!concerns(&[], std::ffi::OsStr::new("doc.md")));
    }

    #[test]
    fn a_path_with_no_directory_part_watches_the_working_directory() {
        // `marquee-markdown README.md` gives a bare relative path.
        let watch = spawn(Path::new("README.md"), || true);
        assert!(watch.is_ok(), "{:?}", watch.err());
    }
}
