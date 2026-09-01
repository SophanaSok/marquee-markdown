//! The hint line's content: a few chips of `key label`, one per mode.
//!
//! The keys come from the live [`Keymap`] rather than from a string, for the
//! same reason the key reference does: a hint naming a key the reader rebound
//! is worse than no hint at all. Only the labels are written here, and they
//! are shorter than [`Action::describe`] because a chip has to read at a
//! glance — "scroll", not "down a line".
//!
//! Pure, and in `app` rather than `ui`, because pane geometry asks it whether
//! anything fits before the row is given a line to sit on. Drawing then asks
//! the same function for the same answer, so the row is never reserved for
//! chips that will not be drawn.
//!
//! Order is what a narrow terminal degrades along: chips are dropped from the
//! end, so the table reads most useful first. Nothing wraps and nothing
//! overflows.

use super::action::Action;
use super::keymap::{Keymap, Mode};
use crate::render::measure;

/// What separates two chips.
pub const SEPARATOR: &str = " \u{b7} ";
/// The blank column the line starts with, matching the status bar's.
pub const INDENT: &str = " ";

/// One chip: the keys that do a thing, and what to call it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    /// The chords, as the reader would type them, joined with `/`.
    pub keys: String,
    /// What pressing one of them does.
    pub label: &'static str,
    /// The actions the chip stands for. Carried so something else on screen
    /// can ask whether the hint line is already saying a thing — the status
    /// bar drops its own `? help` when it is.
    pub actions: &'static [Action],
}

impl Chip {
    /// How many columns the chip occupies, its single space included.
    #[must_use]
    pub fn width(&self) -> usize {
        measure::width(&self.keys) + 1 + measure::width(self.label)
    }
}

/// A chip before its keys have been looked up.
struct Entry {
    /// The actions whose first chord to show; the first one that is bound
    /// wins a slot, and a pair such as down-and-up shows as `j/k`.
    actions: &'static [Action],
    /// The chip's label.
    label: &'static str,
}

/// Reading. The keys a first-time reader needs before they need any others.
const DOCUMENT: &[Entry] = &[
    Entry {
        actions: &[Action::LineDown, Action::LineUp],
        label: "scroll",
    },
    Entry {
        actions: &[Action::SearchStart],
        label: "search",
    },
    Entry {
        actions: &[Action::ToggleToc],
        label: "contents",
    },
    Entry {
        actions: &[Action::ToggleHelp],
        label: "help",
    },
    Entry {
        actions: &[Action::Quit],
        label: "quit",
    },
    Entry {
        actions: &[Action::ThemePicker],
        label: "theme",
    },
    Entry {
        actions: &[Action::ToggleHints],
        label: "hints",
    },
];

/// Choosing a file.
const BROWSER: &[Entry] = &[
    Entry {
        actions: &[Action::BrowserDown, Action::BrowserUp],
        label: "move",
    },
    Entry {
        actions: &[Action::BrowserOpen],
        label: "read",
    },
    Entry {
        actions: &[Action::FilterStart],
        label: "filter",
    },
    Entry {
        actions: &[Action::ToggleHelp],
        label: "help",
    },
    Entry {
        actions: &[Action::Quit],
        label: "quit",
    },
    Entry {
        actions: &[Action::ToggleHints],
        label: "hints",
    },
];

/// The contents pane, when it has focus. Folding is the thing a reader who
/// has just pressed `tab` does not know about.
const TOC: &[Entry] = &[
    Entry {
        actions: &[Action::TocDown, Action::TocUp],
        label: "move",
    },
    Entry {
        actions: &[Action::TocOpen],
        label: "go to",
    },
    Entry {
        actions: &[Action::TocCollapse, Action::TocExpand],
        label: "fold",
    },
    Entry {
        actions: &[Action::FocusNext],
        label: "document",
    },
    Entry {
        actions: &[Action::ToggleHelp],
        label: "help",
    },
    Entry {
        actions: &[Action::ToggleHints],
        label: "hints",
    },
];

/// Typing at the status bar. Short on purpose: the prompt is a place readers
/// get stuck in, and the way out is the whole message.
const PROMPT: &[Entry] = &[
    Entry {
        actions: &[Action::PromptAccept],
        label: "accept",
    },
    Entry {
        actions: &[Action::Escape],
        label: "cancel",
    },
    Entry {
        actions: &[Action::PromptClear],
        label: "clear",
    },
];

/// The key reference, which is itself a list that can scroll.
const HELP: &[Entry] = &[
    Entry {
        actions: &[Action::LineDown, Action::LineUp],
        label: "scroll",
    },
    Entry {
        actions: &[Action::Escape],
        label: "close",
    },
];

/// The theme picker, where every move is a preview and nothing is kept until
/// it is accepted.
const THEMES: &[Entry] = &[
    Entry {
        actions: &[Action::ThemeDown, Action::ThemeUp],
        label: "preview",
    },
    Entry {
        actions: &[Action::ThemeAccept],
        label: "keep",
    },
    Entry {
        actions: &[Action::Escape],
        label: "cancel",
    },
];

/// The chips a mode advertises, before any of them are dropped for width.
fn table(mode: Mode) -> &'static [Entry] {
    match mode {
        Mode::Browser => BROWSER,
        Mode::Toc => TOC,
        Mode::Prompt => PROMPT,
        Mode::Help => HELP,
        Mode::Themes => THEMES,
        // A mode this version does not know about is still a pane being read.
        _ => DOCUMENT,
    }
}

/// Every chip `mode` advertises, with its keys resolved against `keymap`.
///
/// An action nothing is bound to is left out rather than shown keyless: a
/// reader who unbound `/` has no search to be told about.
#[must_use]
pub fn chips(keymap: &Keymap, mode: Mode) -> Vec<Chip> {
    table(mode)
        .iter()
        .filter_map(|entry| {
            let keys: Vec<String> = entry
                .actions
                .iter()
                .filter_map(|&action| keymap.chords(mode, action).first().map(ToString::to_string))
                .collect();
            (!keys.is_empty()).then(|| Chip {
                keys: keys.join("/"),
                label: entry.label,
                actions: entry.actions,
            })
        })
        .collect()
}

/// Whether the chips that fit in `width` columns name `action`.
///
/// The question the status bar asks before repeating itself. It is not enough
/// that the action is in the table: help is the fourth chip, so a narrow
/// terminal drops it, and the status bar has to say `? help` again when that
/// happens.
#[must_use]
pub fn names(keymap: &Keymap, mode: Mode, width: u16, action: Action) -> bool {
    fitting(keymap, mode, width)
        .iter()
        .any(|chip| chip.actions.contains(&action))
}

/// The chips that fit in `width` columns, dropping from the end.
///
/// A chip is shown whole or not at all — half a key name is a wrong key name —
/// so this truncates the list rather than the text, and the caller pads what
/// is left over. Empty when not even the first chip fits, which is what tells
/// pane geometry not to spend a row on the line.
#[must_use]
pub fn fitting(keymap: &Keymap, mode: Mode, width: u16) -> Vec<Chip> {
    let total = usize::from(width);
    let mut used = measure::width(INDENT);
    let separator = measure::width(SEPARATOR);
    let mut out: Vec<Chip> = Vec::new();
    for chip in chips(keymap, mode) {
        let cost = chip.width() + if out.is_empty() { 0 } else { separator };
        if used + cost > total {
            break;
        }
        used += cost;
        out.push(chip);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> Keymap {
        Keymap::defaults()
    }

    #[test]
    fn every_mode_has_something_to_say() {
        for &mode in Mode::ALL {
            assert!(!chips(&map(), mode).is_empty(), "{mode} advertises nothing");
        }
    }

    /// A chip whose action is unbound in its own mode would silently vanish,
    /// which is a hint table that has drifted from the keymap.
    #[test]
    fn every_chip_in_the_table_is_bound_by_default() {
        for &mode in Mode::ALL {
            assert_eq!(
                chips(&map(), mode).len(),
                table(mode).len(),
                "{mode} has an entry nothing is bound to"
            );
        }
    }

    #[test]
    fn the_keys_are_the_ones_that_are_bound() {
        let chips = chips(&map(), Mode::Document);
        assert_eq!(chips[0].keys, "j/k");
        assert_eq!(chips[0].label, "scroll");
        assert!(chips.iter().any(|chip| chip.keys == "q"));
    }

    #[test]
    fn rebinding_a_key_rebinds_the_hint() {
        let mut keymap = Keymap::defaults();
        keymap.rebind(Mode::Document, "n".parse().unwrap(), Action::LineDown);
        // Both of the defaults, or the hint would still have `down` to show.
        for chord in ["j", "down"] {
            keymap.unbind(Mode::Document, chord.parse().unwrap());
        }
        assert_eq!(chips(&keymap, Mode::Document)[0].keys, "n/k");
    }

    /// `j` and `down` both scroll; the chip shows the one the table declares
    /// first, which is the one worth teaching.
    #[test]
    fn an_action_with_several_keys_shows_the_first_one_declared() {
        let chips = chips(&map(), Mode::Document);
        assert_eq!(chips[0].keys, "j/k");
        assert!(
            chips.iter().all(|chip| !chip.keys.contains("down")),
            "the alias won over the key it aliases"
        );
    }

    #[test]
    fn an_unbound_action_costs_its_chip_and_nothing_else() {
        let mut keymap = Keymap::defaults();
        keymap.unbind(Mode::Document, "/".parse().unwrap());
        let chips = chips(&keymap, Mode::Document);
        assert!(!chips.iter().any(|chip| chip.label == "search"));
        assert!(chips.iter().any(|chip| chip.label == "scroll"));
    }

    #[test]
    fn a_narrow_line_drops_chips_from_the_end() {
        let keymap = map();
        let all = fitting(&keymap, Mode::Document, u16::MAX);
        assert_eq!(all, chips(&keymap, Mode::Document));
        for width in 0..=120u16 {
            let fitted = fitting(&keymap, Mode::Document, width);
            assert_eq!(
                fitted,
                all[..fitted.len()],
                "width {width} dropped from the middle"
            );
            assert!(
                rendered_width(&fitted) <= usize::from(width),
                "width {width}"
            );
        }
    }

    #[test]
    fn nothing_fits_in_a_column_or_two() {
        assert!(fitting(&map(), Mode::Document, 0).is_empty());
        assert!(fitting(&map(), Mode::Document, 4).is_empty());
    }

    /// The columns `fitting` promised the caller it would stay inside.
    fn rendered_width(chips: &[Chip]) -> usize {
        if chips.is_empty() {
            return 0;
        }
        measure::width(INDENT)
            + chips.iter().map(Chip::width).sum::<usize>()
            + (chips.len() - 1) * measure::width(SEPARATOR)
    }
}
