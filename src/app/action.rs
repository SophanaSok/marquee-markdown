//! Everything the reader can be asked to do.
//!
//! Input never reaches the update loop as a key: a [`Keymap`](super::keymap)
//! turns a chord into an `Action` first, and the update loop only ever matches
//! on actions. That indirection is what lets keys be rebound from a config
//! file without touching a line of behavior, and it is why the help overlay
//! can be generated rather than written out by hand.

use std::fmt;
use std::str::FromStr;

/// A single thing the reader can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Action {
    /// Leave the program.
    Quit,
    /// Step back out of whatever is innermost: overlay, then prompt, then view.
    Escape,
    /// Show or hide the key reference.
    ToggleHelp,
    /// Switch between the light and dark palette.
    ToggleTheme,
    /// Scroll down one line.
    LineDown,
    /// Scroll up one line.
    LineUp,
    /// Scroll down half a screen.
    HalfPageDown,
    /// Scroll up half a screen.
    HalfPageUp,
    /// Scroll down a full screen.
    PageDown,
    /// Scroll up a full screen.
    PageUp,
    /// Jump to the start of the document.
    Top,
    /// Jump to the end of the document.
    Bottom,
    /// Shift the view one column left; only does anything when wrapping is off.
    ScrollLeft,
    /// Shift the view one column right; only does anything when wrapping is off.
    ScrollRight,
    /// Show or hide the table of contents.
    ToggleToc,
    /// Move focus between the document and the table of contents.
    FocusNext,
    /// Move the table-of-contents cursor down one entry.
    TocDown,
    /// Move the table-of-contents cursor up one entry.
    TocUp,
    /// Move the table-of-contents cursor to the first entry.
    TocTop,
    /// Move the table-of-contents cursor to the last entry.
    TocBottom,
    /// Fold the selected section shut, or move to its parent if it is a leaf.
    TocCollapse,
    /// Unfold the selected section, or move to its first child if it is open.
    TocExpand,
    /// Jump the document to the selected entry.
    TocOpen,
    /// Begin an in-document search.
    SearchStart,
    /// Go to the next hit.
    SearchNext,
    /// Go to the previous hit.
    SearchPrevious,
    /// Accept what has been typed at a prompt.
    PromptAccept,
    /// Delete the character before the cursor, cancelling on an empty prompt.
    PromptBackspace,
    /// Clear the prompt without cancelling it.
    PromptClear,
}

impl Action {
    /// Every action, in the order the help overlay lists them.
    pub const ALL: &'static [Self] = &[
        Self::LineDown,
        Self::LineUp,
        Self::HalfPageDown,
        Self::HalfPageUp,
        Self::PageDown,
        Self::PageUp,
        Self::Top,
        Self::Bottom,
        Self::ScrollLeft,
        Self::ScrollRight,
        Self::SearchStart,
        Self::SearchNext,
        Self::SearchPrevious,
        Self::ToggleToc,
        Self::FocusNext,
        Self::TocDown,
        Self::TocUp,
        Self::TocTop,
        Self::TocBottom,
        Self::TocCollapse,
        Self::TocExpand,
        Self::TocOpen,
        Self::PromptAccept,
        Self::PromptBackspace,
        Self::PromptClear,
        Self::ToggleTheme,
        Self::ToggleHelp,
        Self::Escape,
        Self::Quit,
    ];

    /// Stable identifier, as written in a configuration file.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Quit => "quit",
            Self::Escape => "escape",
            Self::ToggleHelp => "toggle-help",
            Self::ToggleTheme => "toggle-theme",
            Self::LineDown => "line-down",
            Self::LineUp => "line-up",
            Self::HalfPageDown => "half-page-down",
            Self::HalfPageUp => "half-page-up",
            Self::PageDown => "page-down",
            Self::PageUp => "page-up",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::ScrollLeft => "scroll-left",
            Self::ScrollRight => "scroll-right",
            Self::ToggleToc => "toggle-toc",
            Self::FocusNext => "focus-next",
            Self::TocDown => "toc-down",
            Self::TocUp => "toc-up",
            Self::TocTop => "toc-top",
            Self::TocBottom => "toc-bottom",
            Self::TocCollapse => "toc-collapse",
            Self::TocExpand => "toc-expand",
            Self::TocOpen => "toc-open",
            Self::SearchStart => "search-start",
            Self::SearchNext => "search-next",
            Self::SearchPrevious => "search-previous",
            Self::PromptAccept => "prompt-accept",
            Self::PromptBackspace => "prompt-backspace",
            Self::PromptClear => "prompt-clear",
        }
    }

    /// One-line description, shown in the help overlay.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Quit => "quit",
            Self::Escape => "close overlay",
            Self::ToggleHelp => "toggle this help",
            Self::ToggleTheme => "switch light / dark",
            Self::LineDown => "down a line",
            Self::LineUp => "up a line",
            Self::HalfPageDown => "down half a page",
            Self::HalfPageUp => "up half a page",
            Self::PageDown => "down a page",
            Self::PageUp => "up a page",
            Self::Top => "go to top",
            Self::Bottom => "go to bottom",
            Self::ScrollLeft => "scroll left",
            Self::ScrollRight => "scroll right",
            Self::ToggleToc => "show / hide contents",
            Self::FocusNext => "focus contents / document",
            Self::TocDown => "next entry",
            Self::TocUp => "previous entry",
            Self::TocTop => "first entry",
            Self::TocBottom => "last entry",
            Self::TocCollapse => "fold section",
            Self::TocExpand => "unfold section",
            Self::TocOpen => "go to entry",
            Self::SearchStart => "search",
            Self::SearchNext => "next hit",
            Self::SearchPrevious => "previous hit",
            Self::PromptAccept => "accept",
            Self::PromptBackspace => "delete back",
            Self::PromptClear => "clear",
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Error returned when a configuration file names an action that does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownAction(pub String);

impl fmt::Display for UnknownAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown action `{}`", self.0)
    }
}

impl std::error::Error for UnknownAction {}

impl FromStr for Action {
    type Err = UnknownAction;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|action| action.name() == s)
            .ok_or_else(|| UnknownAction(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Adding a variant breaks this match, which is the reminder to add it to
    /// [`Action::ALL`] as well — otherwise it would be unbindable from a
    /// config file and invisible in the help overlay.
    fn _every_variant_is_accounted_for(action: Action) {
        match action {
            Action::Quit
            | Action::Escape
            | Action::ToggleHelp
            | Action::ToggleTheme
            | Action::LineDown
            | Action::LineUp
            | Action::HalfPageDown
            | Action::HalfPageUp
            | Action::PageDown
            | Action::PageUp
            | Action::Top
            | Action::Bottom
            | Action::ScrollLeft
            | Action::ScrollRight
            | Action::ToggleToc
            | Action::FocusNext
            | Action::TocDown
            | Action::TocUp
            | Action::TocTop
            | Action::TocBottom
            | Action::TocCollapse
            | Action::TocExpand
            | Action::TocOpen
            | Action::SearchStart
            | Action::SearchNext
            | Action::SearchPrevious
            | Action::PromptAccept
            | Action::PromptBackspace
            | Action::PromptClear => {}
        }
    }

    #[test]
    fn every_variant_reaches_the_list() {
        assert_eq!(Action::ALL.len(), 29, "Action::ALL is out of date");
    }

    #[test]
    fn every_action_is_listed_exactly_once() {
        let unique: HashSet<_> = Action::ALL.iter().collect();
        assert_eq!(unique.len(), Action::ALL.len(), "duplicate in Action::ALL");
    }

    #[test]
    fn names_round_trip_and_are_unique() {
        let mut names = HashSet::new();
        for &action in Action::ALL {
            assert!(names.insert(action.name()), "duplicate name {action}");
            assert_eq!(action.name().parse(), Ok(action));
        }
    }

    #[test]
    fn an_unknown_name_is_rejected_with_the_name_in_the_message() {
        let err = "fly".parse::<Action>().unwrap_err();
        assert!(err.to_string().contains("fly"), "{err}");
    }
}
