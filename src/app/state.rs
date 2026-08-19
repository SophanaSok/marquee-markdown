//! Application state, and the modes derived from it.
//!
//! One rule holds this together: anything that can be computed from state is
//! computed, never stored. The input mode is derived from what is open rather
//! than tracked alongside it, so the keymap in force and the thing on screen
//! cannot disagree — the bug where a closed prompt still swallows keys is not
//! reachable.

use crate::doc::{DocCache, View};
use crate::source::Source;
use crate::theme::{Appearance, Theme, ThemeVariant};

use super::keymap::{Keymap, Mode};
use super::layout::Panes;

/// Settings that come from the command line rather than from interaction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// The `-w` flag: `Some(0)` disables wrapping.
    pub width: Option<u16>,
    /// The `-l` flag.
    pub line_numbers: bool,
    /// The `-m` flag.
    pub mouse: bool,
}

/// A view layered over the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// The key reference.
    Help,
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
    /// Pane geometry, recomputed once per iteration before drawing.
    pub panes: Panes,
    /// Index into the outline of the section being read; derived from `view`.
    pub active: Option<usize>,
    /// A transient line shown in the status bar until the next key.
    pub message: Option<String>,
    /// Command-line settings.
    pub options: Options,
    /// Set when the reader should exit.
    pub should_quit: bool,
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
            doc: DocCache::new(source),
            view: View::default(),
            theme,
            alternate,
            keymap: Keymap::defaults(),
            overlay: None,
            panes: Panes::default(),
            active: None,
            message: None,
            options,
            should_quit: false,
        }
    }

    /// Which bindings are in force, derived from what is open.
    #[must_use]
    pub fn mode(&self) -> Mode {
        match self.overlay {
            Some(Overlay::Help) => Mode::Help,
            None => Mode::Document,
        }
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
        let section = self.active_heading().map_or("-", |anchor| &anchor.id);
        format!(
            "mode={} top={} left={} section={section} theme={} quit={}",
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
            "mode=document top=0 left=0 section=- theme=slate quit=false"
        );
    }
}
