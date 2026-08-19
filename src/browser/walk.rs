//! Finding markdown files under a directory, without making anyone wait.
//!
//! The walk runs on its own thread and reports in batches, so the first
//! screenful appears immediately and a large tree fills in behind it. A
//! browser that blocks its first frame on a full traversal is the thing that
//! makes a file picker feel broken, and `~` or a monorepo is exactly where a
//! reader will point this.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use ignore::WalkBuilder;

use crate::source::kind;

/// How often the walk reports in, even when it has found nothing. The quiet
/// reports are what keep the "scanning" indicator honest while the walk is
/// working through a directory it will not list any of.
const FLUSH_INTERVAL: Duration = Duration::from_millis(80);
/// Report at least this often by count, so a fast walk still streams.
const FLUSH_COUNT: usize = 64;

/// One markdown file the walk found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Where the file is, for opening it.
    pub path: PathBuf,
    /// Path relative to the directory being browsed, for the list and the
    /// filter. This is what the reader sees and types against.
    pub display: String,
    /// Last modification time, when the filesystem offered one.
    pub modified: Option<std::time::SystemTime>,
}

/// What a running walk reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scan {
    /// Files found since the last report; may be empty as a sign of life.
    Found(Vec<Entry>),
    /// The walk is over.
    Done,
}

/// Walk `root` on a background thread, reporting through `sink`.
///
/// `sink` returns `false` when the receiver has gone away, which is how the
/// thread learns to stop rather than walking a huge tree nobody is watching.
pub fn spawn(
    root: PathBuf,
    all: bool,
    sink: impl Fn(Scan) -> bool + Send + 'static,
) -> thread::JoinHandle<()> {
    thread::spawn(move || walk(&root, all, &sink))
}

/// The walk itself, separated from the thread so it can be run inline in a
/// test.
pub fn walk(root: &Path, all: bool, sink: &(impl Fn(Scan) -> bool + ?Sized)) {
    let walker = WalkBuilder::new(root)
        // `-a` means "show me everything", which is both halves of hidden:
        // dotfiles and anything the repository ignores.
        .hidden(!all)
        .git_ignore(!all)
        .git_global(!all)
        .git_exclude(!all)
        .parents(!all)
        // Honor ignore files even outside a git repository: a `.gitignore` in
        // a plain directory still says what the person who wrote it does not
        // want listed.
        .require_git(false)
        .build();

    let mut batch = Vec::new();
    let mut last_report = Instant::now();

    for entry in walker {
        let Ok(entry) = entry else {
            // An unreadable directory is not worth interrupting the reader
            // over; the rest of the tree is still useful.
            continue;
        };
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            continue;
        }
        if !kind::has_markdown_extension(entry.path()) {
            continue;
        }
        batch.push(describe(entry.path(), root));

        if batch.len() >= FLUSH_COUNT || last_report.elapsed() >= FLUSH_INTERVAL {
            last_report = Instant::now();
            if !sink(Scan::Found(std::mem::take(&mut batch))) {
                return;
            }
        }
    }

    if !batch.is_empty() && !sink(Scan::Found(batch)) {
        return;
    }
    sink(Scan::Done);
}

/// Build an entry for one file.
fn describe(path: &Path, root: &Path) -> Entry {
    let display = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    let modified = path.metadata().and_then(|meta| meta.modified()).ok();
    Entry {
        path: path.to_path_buf(),
        display,
        modified,
    }
}

/// Order for the list: most recently edited first, and alphabetically among
/// files with the same stamp, so the order is stable between runs.
pub fn sort(entries: &mut [Entry]) {
    entries.sort_by(|left, right| {
        right
            .modified
            .cmp(&left.modified)
            .then_with(|| left.display.cmp(&right.display))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::SystemTime;

    /// Run a walk to completion, collecting what it reported.
    fn collect(root: &Path, all: bool) -> (Vec<Entry>, usize) {
        let found = Mutex::new(Vec::new());
        let reports = Mutex::new(0usize);
        walk(root, all, &|scan: Scan| {
            *reports.lock().unwrap() += 1;
            if let Scan::Found(entries) = scan {
                found.lock().unwrap().extend(entries);
            }
            true
        });
        let entries = found.into_inner().unwrap();
        let reports = reports.into_inner().unwrap();
        (entries, reports)
    }

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("README.md"), "# One\n").unwrap();
        std::fs::write(root.join("docs/GUIDE.markdown"), "# Two\n").unwrap();
        std::fs::write(root.join("notes.txt"), "not markdown").unwrap();
        std::fs::write(root.join("src.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join(".hidden/secret.md"), "# Hidden\n").unwrap();
        std::fs::write(root.join("target/built.md"), "# Ignored\n").unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        dir
    }

    fn names(entries: &[Entry]) -> Vec<String> {
        let mut names: Vec<_> = entries.iter().map(|e| e.display.clone()).collect();
        names.sort();
        names
    }

    #[test]
    fn only_markdown_files_are_listed() {
        let dir = tree();
        let (entries, _) = collect(dir.path(), false);
        assert_eq!(names(&entries), vec!["README.md", "docs/GUIDE.markdown"]);
    }

    #[test]
    fn hidden_and_ignored_files_are_left_out_until_asked_for() {
        let dir = tree();
        let (entries, _) = collect(dir.path(), true);
        assert!(names(&entries).contains(&".hidden/secret.md".to_owned()));
        assert!(names(&entries).contains(&"target/built.md".to_owned()));
    }

    #[test]
    fn paths_are_shown_relative_to_what_is_being_browsed() {
        let dir = tree();
        let (entries, _) = collect(dir.path(), false);
        let guide = entries
            .iter()
            .find(|entry| entry.display.contains("GUIDE"))
            .expect("the guide");
        assert_eq!(guide.display, "docs/GUIDE.markdown");
        assert!(guide.path.is_absolute() || guide.path.starts_with(dir.path()));
        assert!(guide.path.exists(), "the path cannot be opened");
    }

    #[test]
    fn the_walk_always_says_when_it_is_finished() {
        let dir = tree();
        let (_, reports) = collect(dir.path(), false);
        assert!(reports >= 1, "the walk never reported at all");
    }

    #[test]
    fn a_receiver_that_has_gone_away_stops_the_walk() {
        let dir = tree();
        let calls = Mutex::new(0usize);
        walk(dir.path(), true, &|_: Scan| {
            *calls.lock().unwrap() += 1;
            false
        });
        assert_eq!(calls.into_inner().unwrap(), 1, "the walk kept going");
    }

    #[test]
    fn a_directory_that_is_not_there_is_not_fatal() {
        let (entries, reports) = collect(Path::new("/no/such/directory"), false);
        assert!(entries.is_empty());
        assert!(reports >= 1, "the walk never finished");
    }

    #[test]
    fn the_list_is_ordered_most_recently_edited_first() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut entries = vec![
            Entry {
                path: "b.md".into(),
                display: "b.md".into(),
                modified: Some(now - Duration::from_secs(60)),
            },
            Entry {
                path: "a.md".into(),
                display: "a.md".into(),
                modified: Some(now),
            },
            Entry {
                path: "c.md".into(),
                display: "c.md".into(),
                modified: None,
            },
        ];
        sort(&mut entries);
        assert_eq!(names_in_order(&entries), vec!["a.md", "b.md", "c.md"]);
    }

    #[test]
    fn files_with_the_same_stamp_are_ordered_by_name() {
        let stamp = Some(SystemTime::UNIX_EPOCH);
        let mut entries = vec![
            Entry {
                path: "z.md".into(),
                display: "z.md".into(),
                modified: stamp,
            },
            Entry {
                path: "a.md".into(),
                display: "a.md".into(),
                modified: stamp,
            },
        ];
        sort(&mut entries);
        assert_eq!(names_in_order(&entries), vec!["a.md", "z.md"]);
    }

    fn names_in_order(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.display.as_str()).collect()
    }
}
