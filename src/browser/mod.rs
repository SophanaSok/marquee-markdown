//! The file browser: what markdown there is under a directory, and which of it
//! the reader is pointing at.
//!
//! Kept free of the terminal so the whole of it — scanning, filtering,
//! selection — can be exercised without drawing anything.

pub mod filter;
pub mod format;
pub mod walk;

use std::path::{Path, PathBuf};

pub use walk::{Entry, Scan};

/// The state of one browsing session.
#[derive(Debug, Clone, Default)]
pub struct Browser {
    /// The directory being browsed.
    pub root: PathBuf,
    /// Everything found so far, most recently edited first.
    entries: Vec<Entry>,
    /// Indices into `entries` that survive the filter, best match first.
    matches: Vec<usize>,
    /// Position in `matches`.
    cursor: usize,
    /// First visible row; derived from the cursor each frame.
    pub offset: usize,
    /// The filter that has been committed, as opposed to one being typed.
    pub filter: String,
    /// Whether the walk is still running.
    pub scanning: bool,
    /// The `(query, entry count)` the matches were built from. Comparing
    /// against it is what makes filtering idempotent, so it can be called
    /// every frame without doing the work every frame.
    applied: Option<(String, usize)>,
    /// Which walk the list belongs to. A rescan bumps it, and reports from
    /// the walk it replaced are dropped rather than repopulating a list that
    /// was just cleared.
    generation: u64,
    /// The file the cursor was on when a rescan began; re-selected as soon
    /// as the new walk finds it again.
    reselect: Option<PathBuf>,
}

impl Browser {
    /// Start browsing `root`, with nothing found yet.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            scanning: true,
            ..Self::default()
        }
    }

    /// Which walk the list belongs to.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Throw the list away and get ready for a fresh walk.
    ///
    /// The filter survives, and the file the cursor was on is remembered so
    /// it is re-selected the moment the new walk finds it — a rescan should
    /// feel like the list updating, not like starting over. `applied` is
    /// cleared explicitly: its `(query, count)` guard would otherwise treat a
    /// rescan that lands on the same count as nothing having changed.
    pub fn begin_rescan(&mut self) {
        self.reselect = self.selected().map(|entry| entry.path.clone());
        self.entries.clear();
        self.matches.clear();
        self.cursor = 0;
        self.scanning = true;
        self.applied = None;
        self.generation += 1;
    }

    /// The walk finished.
    pub fn finish_scan(&mut self) {
        self.scanning = false;
        // If the file the cursor was on never reappeared, it is gone; the
        // cursor has already fallen back to the top.
        self.reselect = None;
    }

    /// Take a batch of results from the walk.
    ///
    /// Appends only. Sorting happens in [`Self::refresh`], because reordering
    /// `entries` invalidates every index in `matches` — including the one the
    /// cursor is on, which then has to be recovered from an index that no
    /// longer means what it did.
    pub fn extend(&mut self, found: impl IntoIterator<Item = Entry>) {
        self.entries.extend(found);
    }

    /// Everything found, in list order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entries that survive the filter, best match first.
    #[must_use]
    pub fn matches(&self) -> &[usize] {
        &self.matches
    }

    /// How many entries are on show.
    #[must_use]
    pub fn len(&self) -> usize {
        self.matches.len()
    }

    /// Whether the list is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    /// Where the cursor is, as a position in the filtered list.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The entry the cursor is on.
    #[must_use]
    pub fn selected(&self) -> Option<&Entry> {
        self.entries.get(*self.matches.get(self.cursor)?)
    }

    /// The entry at a position in the filtered list.
    #[must_use]
    pub fn entry_at(&self, position: usize) -> Option<&Entry> {
        self.entries.get(*self.matches.get(position)?)
    }

    /// Rebuild the filtered list for `query`, if anything has changed.
    ///
    /// The selected file is kept selected across a re-filter and across new
    /// results arriving: the list reorders under the cursor while a scan is
    /// running, and a cursor that stayed at a fixed row would wander through
    /// the files on its own.
    pub fn refresh(&mut self, query: &str) {
        let state = (query.to_owned(), self.entries.len());
        if self.applied.as_ref() == Some(&state) {
            return;
        }
        // Read the selection before anything moves: `matches` still describes
        // the list the cursor was placed in.
        let selected = self.selected().map(|entry| entry.path.clone());
        walk::sort(&mut self.entries);
        self.matches = filter::matching(query, self.entries.iter().map(|e| e.display.as_str()));
        self.applied = Some(state);
        // A rescan in progress re-selects the remembered file the moment it
        // reappears; otherwise the cursor follows the selection it had.
        if let Some(waiting) = self.reselect.clone()
            && let Some(position) = self.position_of(&waiting)
        {
            self.cursor = position;
            self.reselect = None;
            return;
        }
        self.cursor = selected
            .and_then(|path| self.position_of(&path))
            .unwrap_or(0);
    }

    /// Where a path sits in the filtered list.
    #[must_use]
    pub fn position_of(&self, path: &Path) -> Option<usize> {
        self.matches
            .iter()
            .position(|&index| self.entries[index].path == path)
    }

    /// Move the cursor by `delta` entries, stopping at either end.
    pub fn move_cursor(&mut self, delta: isize) {
        self.cursor = self
            .cursor
            .saturating_add_signed(delta)
            .min(self.len().saturating_sub(1));
    }

    /// Put the cursor on the first entry.
    pub fn to_first(&mut self) {
        self.cursor = 0;
    }

    /// Put the cursor on the last entry.
    pub fn to_last(&mut self) {
        self.cursor = self.len().saturating_sub(1);
    }

    /// Pull the cursor back inside the list, and scroll so it is on screen.
    ///
    /// Both are derived rather than maintained: results arriving and filters
    /// changing move entries around underneath, and a cursor corrected in one
    /// place cannot disagree with itself.
    pub fn clamp(&mut self, height: u16) {
        self.cursor = self.cursor.min(self.len().saturating_sub(1));
        let height = usize::from(height);
        if height == 0 {
            self.offset = 0;
            return;
        }
        self.offset = self.offset.min(self.len().saturating_sub(height));
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + height {
            self.offset = self.cursor + 1 - height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn entry(name: &str, age: u64) -> Entry {
        Entry {
            path: PathBuf::from("/root").join(name),
            display: name.to_owned(),
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 - age)),
        }
    }

    fn browser() -> Browser {
        let mut browser = Browser::new("/root".into());
        browser.extend([
            entry("README.md", 30),
            entry("docs/ROADMAP.md", 10),
            entry("docs/THEMING.md", 20),
        ]);
        browser.refresh("");
        browser
    }

    fn shown(browser: &Browser) -> Vec<&str> {
        (0..browser.len())
            .map(|position| browser.entry_at(position).unwrap().display.as_str())
            .collect()
    }

    #[test]
    fn the_list_starts_with_the_most_recently_edited() {
        assert_eq!(
            shown(&browser()),
            vec!["docs/ROADMAP.md", "docs/THEMING.md", "README.md"]
        );
    }

    #[test]
    fn a_filter_narrows_the_list_and_clearing_it_restores_the_order() {
        let mut browser = browser();
        browser.refresh("theming");
        assert_eq!(shown(&browser), vec!["docs/THEMING.md"]);
        browser.refresh("");
        assert_eq!(shown(&browser).len(), 3);
    }

    #[test]
    fn filtering_twice_with_the_same_query_does_no_work() {
        let mut browser = browser();
        browser.refresh("docs");
        browser.move_cursor(1);
        let cursor = browser.cursor();
        browser.refresh("docs");
        assert_eq!(browser.cursor(), cursor, "the cursor was reset");
    }

    #[test]
    fn results_arriving_do_not_move_the_selection_off_the_file_it_is_on() {
        // The list is sorted by modification time, so a newer file arriving
        // mid-scan inserts itself above the cursor.
        let mut browser = browser();
        browser.move_cursor(2);
        let selected = browser.selected().unwrap().path.clone();
        browser.extend([entry("brand-new.md", 0)]);
        browser.refresh("");
        assert_eq!(browser.selected().unwrap().path, selected);
    }

    #[test]
    fn a_filter_that_hides_the_selection_falls_back_to_the_first_entry() {
        let mut browser = browser();
        browser.move_cursor(2);
        browser.refresh("theming");
        assert_eq!(browser.cursor(), 0);
        assert_eq!(browser.selected().unwrap().display, "docs/THEMING.md");
    }

    #[test]
    fn the_cursor_stops_at_both_ends() {
        let mut browser = browser();
        browser.move_cursor(-5);
        assert_eq!(browser.cursor(), 0);
        browser.move_cursor(500);
        assert_eq!(browser.cursor(), 2);
        browser.to_first();
        assert_eq!(browser.cursor(), 0);
        browser.to_last();
        assert_eq!(browser.cursor(), 2);
    }

    #[test]
    fn an_empty_list_has_nothing_selected_and_does_not_panic() {
        let mut browser = Browser::new("/root".into());
        browser.refresh("");
        assert!(browser.is_empty());
        assert!(browser.selected().is_none());
        browser.move_cursor(1);
        browser.to_last();
        browser.clamp(10);
        assert_eq!(browser.cursor(), 0);
    }

    #[test]
    fn scrolling_follows_the_cursor_by_the_least_it_can() {
        let mut browser = Browser::new("/root".into());
        browser.extend((0..50).map(|n| entry(&format!("file-{n:02}.md"), n)));
        browser.refresh("");
        browser.clamp(10);
        assert_eq!(browser.offset, 0);

        browser.move_cursor(9);
        browser.clamp(10);
        assert_eq!(browser.offset, 0, "scrolled before it had to");

        browser.move_cursor(1);
        browser.clamp(10);
        assert_eq!(browser.offset, 1);

        browser.to_last();
        browser.clamp(10);
        assert_eq!(browser.offset, 40);

        browser.to_first();
        browser.clamp(10);
        assert_eq!(browser.offset, 0);
    }

    #[test]
    fn a_list_shorter_than_the_screen_never_scrolls() {
        let mut browser = browser();
        browser.to_last();
        browser.clamp(40);
        assert_eq!(browser.offset, 0);
    }

    #[test]
    fn a_rescan_clears_the_list_but_keeps_the_selection_by_path() {
        let mut browser = browser();
        browser.move_cursor(1);
        let path = browser.selected().unwrap().path.clone();

        browser.begin_rescan();
        assert!(browser.is_empty());
        assert!(browser.scanning);
        assert_eq!(browser.generation(), 1);

        // The new walk finds the same file again (among others).
        browser.extend([entry("docs/THEMING.md", 20), entry("new-arrival.md", 5)]);
        browser.refresh("");
        assert_eq!(browser.selected().unwrap().path, path);
    }

    #[test]
    fn a_rescan_that_loses_the_selected_file_falls_back_to_the_top() {
        let mut browser = browser();
        browser.move_cursor(2);
        browser.begin_rescan();
        browser.extend([entry("docs/ROADMAP.md", 10)]);
        browser.refresh("");
        browser.finish_scan();
        assert_eq!(browser.cursor(), 0);
        assert!(!browser.scanning);
    }

    #[test]
    fn a_rescan_landing_on_the_same_count_still_refreshes() {
        // The (query, count) idempotence guard must not eat a rescan whose
        // new list happens to be the same size as the old one.
        let mut browser = browser();
        browser.begin_rescan();
        browser.extend([entry("a.md", 1), entry("b.md", 2), entry("c.md", 3)]);
        browser.refresh("");
        assert_eq!(shown(&browser), vec!["a.md", "b.md", "c.md"]);
    }
}
