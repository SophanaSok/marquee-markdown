//! Application state, and the modes derived from it.
//!
//! One rule holds this together: anything that can be computed from state is
//! computed, never stored. The input mode is derived from what is open rather
//! than tracked alongside it, so the keymap in force and the thing on screen
//! cannot disagree — the bug where a closed prompt still swallows keys is not
//! reachable.

use std::fmt;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use crate::browser::Browser;
use crate::doc::{DocCache, Links, Search, View};
use crate::render::{HtmlMode, ParseOptions};
use crate::source::Source;
use crate::theme::system::TerminalColors;
use crate::theme::{Appearance, Theme, ThemeVariant, registry};

use super::event::Event;
use super::keymap::{Keymap, Mode};
use super::layout::Panes;

/// Settings that come from the command line rather than from interaction.
///
/// Adding a setting is routine here, and adding a public field to a struct
/// anyone can write as a literal is a breaking change. `non_exhaustive` moves
/// that cost to now: outside this crate these are built from [`Default`] and
/// then assigned to, so the next setting is not an API break.
///
/// Not `Copy`: the configuration file's path lives here, and it is what the
/// theme picker writes a chosen theme back to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Options {
    /// The `-w` flag: `Some(0)` disables wrapping.
    pub width: Option<u16>,
    /// The `-l` flag.
    pub line_numbers: bool,
    /// The `-m` flag.
    pub mouse: bool,
    /// The `-a` flag: list hidden and ignored files when browsing.
    pub all: bool,
    /// The `-n` flag: keep the line breaks the author typed.
    pub preserve_new_lines: bool,
    /// Start with the contents pane showing.
    pub contents: bool,
    /// Start with the hint line showing above the status bar.
    pub hints: bool,
    /// What to do with raw HTML.
    pub html: HtmlMode,
    /// The configuration file in force, if one was found. Where the theme
    /// picker records a chosen theme; when `None` it creates the file at the
    /// default location instead.
    pub config_path: Option<PathBuf>,
    /// Whether `-s` or `MARQUEE_STYLE` set the theme. Both beat the file on the
    /// next run, so a theme saved from the picker would appear not to take —
    /// which is worth saying rather than letting the reader discover it.
    pub style_overridden: bool,
    /// What the terminal said about its own colors, asked once before the
    /// screen was taken.
    ///
    /// Carried rather than re-asked because the event thread owns standard
    /// input from here on: a question put to the terminal now would wait for
    /// a reply that thread has already swallowed. This is what lets the
    /// picker preview `system` on a key press.
    pub terminal: TerminalColors,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            width: None,
            line_numbers: false,
            mouse: false,
            all: false,
            preserve_new_lines: false,
            html: HtmlMode::default(),
            contents: true,
            hints: true,
            config_path: None,
            style_overridden: false,
            terminal: TerminalColors::UNKNOWN,
        }
    }
}

/// A view layered over the document.
///
/// `non_exhaustive` for the same reason the configuration structs are: the
/// reader grows overlays, and each one would otherwise be a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Overlay {
    /// The key reference.
    Help,
    /// The theme picker.
    Themes,
}

/// What the theme picker is showing, while it is open.
///
/// The list is a snapshot rather than a live read: [`registry::list`] reads a
/// directory, and the draw path may not do that once a frame.
#[derive(Debug, Clone)]
pub struct ThemePicker {
    /// Every selectable theme, as of when the picker opened.
    pub entries: Vec<registry::Entry>,
    /// The row the cursor is on.
    pub cursor: usize,
    /// The theme that was in force when the picker opened, to put back if the
    /// reader changes their mind. Every cursor move previews, so without this
    /// there would be nothing to cancel back to.
    pub restore: Theme,
    /// Themes that would not load, by name. A hand-written theme file can be
    /// malformed, and the picker says so on the row rather than by refusing to
    /// open.
    pub failed: Vec<String>,
}

/// A running file watch, or none.
///
/// A newtype only so [`App`] can still derive `Debug`: the debouncer behind it
/// does not.
#[derive(Default)]
pub struct FileWatch(Option<crate::doc::watch::Watch>);

impl fmt::Debug for FileWatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_some() {
            "FileWatch(watching)"
        } else {
            "FileWatch(idle)"
        })
    }
}

/// Which screen the reader is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    /// Choosing a file.
    Browser,
    /// Reading one.
    #[default]
    Document,
}

/// Which pane the keyboard is talking to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    /// The document.
    #[default]
    Document,
    /// The table of contents.
    Toc,
}

/// What a prompt is collecting text for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// An in-document search.
    Search,
    /// Narrowing the file list.
    Filter,
}

impl PromptKind {
    /// The sigil shown in front of the text being typed.
    ///
    /// Distinct per prompt on purpose: the browser's filter and the document
    /// search both live on `/`, and the sigil is what tells a reader which one
    /// they are typing into.
    #[must_use]
    pub const fn sigil(self) -> &'static str {
        match self {
            Self::Search => "/",
            Self::Filter => "filter> ",
        }
    }
}

/// Text being typed at the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// What the text is for.
    pub kind: PromptKind,
    /// What has been typed so far.
    pub input: String,
}

/// The table of contents, as the reader has arranged it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Toc {
    /// The selected row, as an index into the outline's rows. Distinct from
    /// the active section, which follows the scroll position: moving the
    /// cursor must not be undone by scrolling, and scrolling must not drag the
    /// cursor around.
    pub cursor: usize,
    /// Which rows are folded shut, indexed by row. Survives re-layout because
    /// the set of headings does not depend on the width.
    pub collapsed: Vec<bool>,
    /// First visible row of the pane; derived from the cursor each frame.
    pub offset: usize,
    /// Row indices currently on show, with folded subtrees left out. Derived.
    pub visible: Vec<usize>,
}

/// Everything the reader knows.
#[derive(Debug)]
pub struct App {
    /// The document and its layout.
    pub doc: DocCache,
    /// The reading position.
    pub view: View,
    /// The active theme.
    pub theme: Theme,
    /// The theme `toggle-theme` switches to; swapped with `theme` on each use,
    /// so a hand-written theme is never lost by toggling away from it.
    pub alternate: Theme,
    /// Chord bindings.
    pub keymap: Keymap,
    /// What is layered over the document, if anything.
    pub overlay: Option<Overlay>,
    /// The theme picker's list and cursor, while `overlay` is
    /// [`Overlay::Themes`]. Held beside the overlay rather than inside it so
    /// `Overlay` stays `Copy`, the same way [`App::help_scroll`] is.
    pub picker: Option<ThemePicker>,
    /// First visible row of the key reference, when it is open and taller
    /// than the terminal. Derived-clamped each frame; reset when it opens.
    pub help_scroll: u16,
    /// Pane geometry, recomputed once per iteration before drawing.
    pub panes: Panes,
    /// Which screen is on show.
    pub screen: Screen,
    /// The file browser, when the reader arrived through one. `None` when a
    /// file was named on the command line, which is what makes `esc` a hint
    /// there and a way back here otherwise.
    pub browser: Option<Browser>,
    /// Which pane the keyboard is talking to.
    pub focus: Focus,
    /// Whether the reader has asked for the contents pane. It can still be
    /// hidden by a narrow terminal or a document with no headings.
    pub toc_visible: bool,
    /// Whether the reader has asked for the hint line. It can still be hidden
    /// by a terminal with no row to spare or none to spare wide enough.
    ///
    /// Session state, like `toc_visible`: `H` is a change of mind for now, and
    /// `[ui] hints` in the configuration file is one for good. Nothing writes
    /// the file behind the reader's back on a keystroke.
    pub hints: bool,
    /// The contents pane.
    pub toc: Toc,
    /// The in-document search.
    pub search: Search,
    /// The links in the document, and which one is stepped to.
    pub links: Links,
    /// Text being typed, if a prompt is open.
    pub prompt: Option<Prompt>,
    /// Index into the outline of the section being read; derived from `view`.
    pub active: Option<usize>,
    /// A transient line shown in the status bar until the next key.
    pub message: Option<String>,
    /// Command-line settings.
    pub options: Options,
    /// Something that needs the terminal to itself, waiting for the loop to
    /// carry it out. Left recorded and unperformed in a headless run, which is
    /// what lets a test check that the right thing was asked for.
    pub pending: Option<crate::app::external::Request>,
    /// Set when the reader should exit.
    pub should_quit: bool,
    /// Where background producers post. `None` in headless tests, which is
    /// also what makes them free of threads.
    pub events: Option<Sender<Event>>,
    /// The watch on the open document, if it has a path and watching worked.
    watch: FileWatch,
}

impl App {
    /// Build the reader over a document.
    #[must_use]
    pub fn new(source: Source, theme: Theme, options: Options) -> Self {
        let alternate = Theme::new(match theme.appearance {
            Appearance::Light => ThemeVariant::Slate,
            Appearance::Dark => ThemeVariant::Paper,
        });
        Self {
            doc: DocCache::with_options(source, ParseOptions { html: options.html }),
            view: View::default(),
            theme,
            alternate,
            keymap: Keymap::defaults(),
            overlay: None,
            picker: None,
            help_scroll: 0,
            panes: Panes::default(),
            screen: Screen::Document,
            browser: None,
            focus: Focus::Document,
            toc_visible: options.contents,
            hints: options.hints,
            toc: Toc::default(),
            search: Search::default(),
            links: Links::default(),
            prompt: None,
            active: None,
            message: None,
            options,
            pending: None,
            should_quit: false,
            events: None,
            watch: FileWatch::default(),
        }
    }

    /// Open the file browser over `root`, with no document loaded yet.
    #[must_use]
    pub fn browsing(root: std::path::PathBuf, theme: Theme, options: Options) -> Self {
        // The empty document is a placeholder until a file is chosen; there is
        // deliberately no second code path for "no document", because every
        // one of those is a place the two can disagree.
        let placeholder = Source::from_text("", None, String::new(), crate::source::Base::Cwd);
        let mut app = Self::new(placeholder, theme, options);
        app.screen = Screen::Browser;
        app.browser = Some(Browser::new(root));
        app
    }

    /// Start reading `source`, leaving the browser as it was so `esc` comes
    /// back to it.
    pub fn read(&mut self, source: Source) {
        self.doc = DocCache::with_options(
            source,
            ParseOptions {
                html: self.options.html,
            },
        );
        self.view = View::default();
        self.toc = Toc::default();
        self.search.clear();
        self.links = Links::default();
        self.screen = Screen::Document;
        self.focus = Focus::Document;
        self.start_watching();
    }

    /// Watch the open document for changes, replacing any previous watch.
    ///
    /// Does nothing without an event queue to report to, which is what keeps
    /// headless tests free of threads and of the filesystem.
    pub fn start_watching(&mut self) {
        self.watch = FileWatch::default();
        let (Some(path), Some(sender)) = (self.doc.source.path.clone(), self.events.clone()) else {
            return;
        };
        // Watching is a convenience; a platform that will not do it should not
        // stop the reader from opening the file.
        self.watch = FileWatch(
            crate::doc::watch::spawn(&path, move || sender.send(Event::Reload).is_ok()).ok(),
        );
    }

    /// Whether the open document is being watched for changes.
    #[must_use]
    pub fn is_watching(&self) -> bool {
        self.watch.0.is_some()
    }

    /// Re-read the open document from disk.
    ///
    /// # Errors
    /// Returns an error when the document did not come from a file, or the
    /// file can no longer be read.
    pub fn reload_from_disk(&mut self) -> anyhow::Result<()> {
        let path = self
            .doc
            .source
            .path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("this document did not come from a file"))?;
        let source = crate::source::resolve(
            &crate::source::SourceSpec::File(path),
            &crate::source::HttpFetcher::new(),
        )?;
        self.doc.reload(source, self.view.top);
        // Which sections exist may have changed, so which are folded cannot be
        // carried over by position.
        self.toc.collapsed.clear();
        Ok(())
    }

    /// Which bindings are in force, derived from what is open and what has
    /// focus. Never stored: a mode kept alongside the state it describes is
    /// how a closed prompt ends up still swallowing keys.
    #[must_use]
    pub fn mode(&self) -> Mode {
        match self.overlay {
            Some(Overlay::Help) => Mode::Help,
            Some(Overlay::Themes) => Mode::Themes,
            None if self.prompt.is_some() => Mode::Prompt,
            None => self.pane_mode(),
        }
    }

    /// The mode of the pane underneath any overlay — what the key reference
    /// should describe, since opening it does not move focus.
    #[must_use]
    pub fn pane_mode(&self) -> Mode {
        match (self.screen, self.focus) {
            (Screen::Browser, _) => Mode::Browser,
            (Screen::Document, Focus::Document) => Mode::Document,
            (Screen::Document, Focus::Toc) => Mode::Toc,
        }
    }

    /// Whether the hint line is on screen and already naming `action`.
    ///
    /// Asked by the status bar, which carries its own `? help` and should not
    /// say a second time what the row above it is already saying. Both halves
    /// of the question matter: the line has to be there, and it has to be wide
    /// enough to have kept that particular hint.
    #[must_use]
    pub fn hint_names(&self, action: super::action::Action) -> bool {
        self.panes
            .hints
            .is_some_and(|row| super::hints::names(&self.keymap, self.mode(), row.width, action))
    }

    /// The outline row the cursor is on.
    #[must_use]
    pub fn toc_row(&self) -> Option<&crate::doc::outline::Row> {
        self.doc.outline().rows().get(self.toc.cursor)
    }

    /// The heading an outline row points at.
    #[must_use]
    pub fn anchor_of(&self, row: usize) -> Option<&crate::render::Anchor> {
        let row = self.doc.outline().rows().get(row)?;
        self.doc.doc().outline.get(row.anchor)
    }

    /// The scrolling bounds for the current pane geometry.
    #[must_use]
    pub fn extent(&self) -> crate::doc::Extent {
        self.doc
            .extent(self.panes.body.height, self.panes.body.width)
    }

    /// The heading whose section is being read.
    #[must_use]
    pub fn active_heading(&self) -> Option<&crate::render::Anchor> {
        self.active
            .and_then(|index| self.doc.doc().outline.get(index))
    }

    /// A one-line summary of the state that matters, for headless tests.
    ///
    /// Deliberately terse and stable: key-sequence tests assert on this, so it
    /// is a description of behavior rather than of structure.
    #[must_use]
    pub fn summary(&self) -> String {
        if self.screen == Screen::Browser {
            let browser = self
                .browser
                .as_ref()
                .expect("the browser screen has a browser");
            return format!(
                "mode={} files={} cursor={} filter={} quit={}",
                self.mode(),
                browser.len(),
                browser
                    .selected()
                    .map_or("-", |entry| entry.display.as_str()),
                match (self.prompt.as_ref(), browser.filter.as_str()) {
                    (Some(prompt), _) => format!("{}|", prompt.input),
                    (None, "") => "-".to_owned(),
                    (None, committed) => committed.to_owned(),
                },
                self.should_quit,
            );
        }
        let section = self.active_heading().map_or("-", |anchor| &anchor.id);
        let toc = if self.panes.sidebar.is_none() {
            "off".to_owned()
        } else {
            self.anchor_of(self.toc.cursor)
                .map_or_else(|| "-".to_owned(), |anchor| anchor.id.clone())
        };
        let search = match (self.prompt.as_ref(), self.search.is_active()) {
            // The live count follows the prompt text, so a test can watch the
            // matches narrow while the query is still being typed.
            (Some(prompt), _) => format!(
                "{}{}|[{}/{}]",
                prompt.kind.sigil(),
                prompt.input,
                self.search.current().map_or(0, |index| index + 1),
                self.search.matches().len()
            ),
            (None, true) => format!(
                "{}[{}/{}]",
                self.search.query(),
                self.search.current().map_or(0, |index| index + 1),
                self.search.matches().len()
            ),
            (None, false) => "-".to_owned(),
        };
        format!(
            "mode={} top={} left={} section={section} toc={toc} search={search} theme={} quit={}",
            self.mode(),
            self.view.top,
            self.view.left,
            self.theme.name,
            self.should_quit,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Base;

    fn app() -> App {
        App::new(
            Source::from_text("# T\n\nbody\n", None, "t.md".into(), Base::Cwd),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        )
    }

    #[test]
    fn the_mode_follows_the_overlay() {
        let mut app = app();
        assert_eq!(app.mode(), Mode::Document);
        app.overlay = Some(Overlay::Help);
        assert_eq!(app.mode(), Mode::Help);
        app.overlay = None;
        assert_eq!(app.mode(), Mode::Document);
    }

    #[test]
    fn the_alternate_theme_is_the_other_appearance() {
        let app = app();
        assert_eq!(app.theme.appearance, Appearance::Dark);
        assert_eq!(app.alternate.appearance, Appearance::Light);
    }

    #[test]
    fn a_summary_is_produced_before_anything_is_laid_out() {
        assert_eq!(
            app().summary(),
            "mode=document top=0 left=0 section=- toc=off search=- theme=slate quit=false"
        );
    }
}
