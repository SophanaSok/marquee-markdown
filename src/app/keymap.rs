//! Chords, modes, and the table that maps one to an [`Action`].
//!
//! The default table below is the only place a key appears in the codebase.
//! Nothing downstream matches on [`KeyCode`]; the help overlay is rendered
//! from this table rather than from a string literal, so a rebound key is
//! documented correctly the moment it is rebound.
//!
//! The governing rule for the defaults: never re-point a key `glow` already
//! uses. New features take chords `glow` leaves free.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::action::Action;

/// Which set of bindings is live.
///
/// The mode is always derived from application state, never stored, so the
/// keymap and the visible focus cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mode {
    /// Reading a document.
    Document,
    /// The table of contents has focus.
    Toc,
    /// Text is being typed at a prompt.
    Prompt,
    /// The key reference is open.
    Help,
}

impl Mode {
    /// Every mode, for iteration in tests and in the help overlay.
    pub const ALL: &'static [Self] = &[Self::Document, Self::Toc, Self::Prompt, Self::Help];

    /// Name used in configuration files (`[keys.document]`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Toc => "toc",
            Self::Prompt => "prompt",
            Self::Help => "help",
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A key plus its modifiers, normalized so equal-looking chords compare equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    /// The key itself.
    pub code: KeyCode,
    /// Modifiers, with the redundant shift of an uppercase letter removed.
    pub modifiers: KeyModifiers,
}

impl Chord {
    /// Build a chord, normalizing the modifiers.
    #[must_use]
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        // Terminals disagree about whether `G` also reports shift. Since the
        // character already carries the case, dropping the flag makes the two
        // reports the same chord instead of two bindings that must be kept in
        // sync.
        let modifiers = match code {
            KeyCode::Char(c) if c.is_uppercase() || !c.is_alphabetic() => {
                modifiers - KeyModifiers::SHIFT
            }
            _ => modifiers,
        };
        Self { code, modifiers }
    }

    /// The chord a key event represents.
    #[must_use]
    pub fn from_event(event: KeyEvent) -> Self {
        Self::new(event.code, event.modifiers)
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            f.write_str("ctrl+")?;
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            f.write_str("alt+")?;
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            f.write_str("shift+")?;
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("space"),
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::Up => f.write_str("up"),
            KeyCode::Down => f.write_str("down"),
            KeyCode::Left => f.write_str("left"),
            KeyCode::Right => f.write_str("right"),
            KeyCode::Home => f.write_str("home"),
            KeyCode::End => f.write_str("end"),
            KeyCode::PageUp => f.write_str("pgup"),
            KeyCode::PageDown => f.write_str("pgdn"),
            KeyCode::Enter => f.write_str("enter"),
            KeyCode::Tab => f.write_str("tab"),
            KeyCode::BackTab => f.write_str("shift+tab"),
            KeyCode::Backspace => f.write_str("backspace"),
            KeyCode::Delete => f.write_str("delete"),
            KeyCode::Esc => f.write_str("esc"),
            KeyCode::F(n) => write!(f, "f{n}"),
            other => write!(f, "{other:?}"),
        }
    }
}

/// Error returned when a configuration file contains a chord that cannot be
/// parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadChord(pub String);

impl fmt::Display for BadChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot parse key `{}`", self.0)
    }
}

impl std::error::Error for BadChord {}

impl FromStr for Chord {
    type Err = BadChord;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut modifiers = KeyModifiers::NONE;
        let mut rest = s;
        while let Some((head, tail)) = rest.split_once('+') {
            // A literal `+` is a key, not a separator.
            if tail.is_empty() {
                break;
            }
            match head.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
                "alt" | "meta" => modifiers |= KeyModifiers::ALT,
                "shift" => modifiers |= KeyModifiers::SHIFT,
                _ => return Err(BadChord(s.to_owned())),
            }
            rest = tail;
        }

        let code = match rest.to_ascii_lowercase().as_str() {
            "space" => KeyCode::Char(' '),
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pgup" | "pageup" => KeyCode::PageUp,
            "pgdn" | "pagedown" => KeyCode::PageDown,
            "enter" | "return" => KeyCode::Enter,
            "tab" => KeyCode::Tab,
            "backspace" => KeyCode::Backspace,
            "delete" | "del" => KeyCode::Delete,
            "esc" | "escape" => KeyCode::Esc,
            lower => {
                if let Some(number) = lower.strip_prefix('f')
                    && let Ok(n) = number.parse::<u8>()
                    && (1..=24).contains(&n)
                {
                    KeyCode::F(n)
                } else {
                    // Match on the original so case is preserved: `G` and `g`
                    // are different keys.
                    let mut chars = rest.chars();
                    match (chars.next(), chars.next()) {
                        (Some(c), None) => KeyCode::Char(c),
                        _ => return Err(BadChord(s.to_owned())),
                    }
                }
            }
        };
        Ok(Self::new(code, modifiers))
    }
}

/// Error returned when two bindings in one mode claim the same chord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateBinding {
    /// The contested chord.
    pub chord: Chord,
    /// The mode it is bound in twice.
    pub mode: Mode,
    /// The action already holding it.
    pub existing: Action,
    /// The action that tried to take it.
    pub incoming: Action,
}

impl fmt::Display for DuplicateBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is bound twice in {} mode: {} and {}",
            self.chord, self.mode, self.existing, self.incoming
        )
    }
}

impl std::error::Error for DuplicateBinding {}

/// Chord-to-action bindings, per mode.
///
/// Declaration order is preserved so the help overlay can list a chord's
/// aliases the way they were written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keymap {
    order: Vec<(Mode, Chord, Action)>,
    lookup: HashMap<(Mode, Chord), Action>,
}

/// The default bindings, as `(mode, chord, action)` in help-overlay order.
const DEFAULTS: &[(Mode, &str, Action)] = &[
    (Mode::Document, "j", Action::LineDown),
    (Mode::Document, "down", Action::LineDown),
    (Mode::Document, "k", Action::LineUp),
    (Mode::Document, "up", Action::LineUp),
    (Mode::Document, "d", Action::HalfPageDown),
    (Mode::Document, "ctrl+d", Action::HalfPageDown),
    (Mode::Document, "u", Action::HalfPageUp),
    (Mode::Document, "ctrl+u", Action::HalfPageUp),
    (Mode::Document, "f", Action::PageDown),
    (Mode::Document, "space", Action::PageDown),
    (Mode::Document, "pgdn", Action::PageDown),
    (Mode::Document, "b", Action::PageUp),
    (Mode::Document, "pgup", Action::PageUp),
    (Mode::Document, "g", Action::Top),
    (Mode::Document, "home", Action::Top),
    (Mode::Document, "G", Action::Bottom),
    (Mode::Document, "end", Action::Bottom),
    (Mode::Document, "h", Action::ScrollLeft),
    (Mode::Document, "left", Action::ScrollLeft),
    (Mode::Document, "l", Action::ScrollRight),
    (Mode::Document, "right", Action::ScrollRight),
    (Mode::Document, "/", Action::SearchStart),
    (Mode::Document, "n", Action::SearchNext),
    (Mode::Document, "N", Action::SearchPrevious),
    (Mode::Document, "t", Action::ToggleToc),
    (Mode::Document, "tab", Action::FocusNext),
    (Mode::Document, "T", Action::ToggleTheme),
    (Mode::Document, "?", Action::ToggleHelp),
    (Mode::Document, "esc", Action::Escape),
    (Mode::Document, "q", Action::Quit),
    (Mode::Document, "ctrl+c", Action::Quit),
    // The contents pane takes the same movement keys, pointed at itself. `h`
    // and `l` fold and unfold here, which is what those keys mean in every
    // tree view; in the document they still scroll sideways.
    (Mode::Toc, "j", Action::TocDown),
    (Mode::Toc, "down", Action::TocDown),
    (Mode::Toc, "k", Action::TocUp),
    (Mode::Toc, "up", Action::TocUp),
    (Mode::Toc, "g", Action::TocTop),
    (Mode::Toc, "home", Action::TocTop),
    (Mode::Toc, "G", Action::TocBottom),
    (Mode::Toc, "end", Action::TocBottom),
    (Mode::Toc, "h", Action::TocCollapse),
    (Mode::Toc, "left", Action::TocCollapse),
    (Mode::Toc, "l", Action::TocExpand),
    (Mode::Toc, "right", Action::TocExpand),
    (Mode::Toc, "enter", Action::TocOpen),
    (Mode::Toc, "tab", Action::FocusNext),
    (Mode::Toc, "t", Action::ToggleToc),
    (Mode::Toc, "/", Action::SearchStart),
    (Mode::Toc, "T", Action::ToggleTheme),
    (Mode::Toc, "?", Action::ToggleHelp),
    (Mode::Toc, "esc", Action::Escape),
    (Mode::Toc, "q", Action::Quit),
    (Mode::Toc, "ctrl+c", Action::Quit),
    // A prompt binds almost nothing on purpose: every other printable key has
    // to reach the text being typed, or `q` in a search box quits the reader.
    (Mode::Prompt, "enter", Action::PromptAccept),
    (Mode::Prompt, "backspace", Action::PromptBackspace),
    (Mode::Prompt, "ctrl+u", Action::PromptClear),
    (Mode::Prompt, "esc", Action::Escape),
    (Mode::Prompt, "ctrl+c", Action::Quit),
    (Mode::Help, "?", Action::ToggleHelp),
    (Mode::Help, "esc", Action::Escape),
    (Mode::Help, "q", Action::Escape),
    (Mode::Help, "ctrl+c", Action::Quit),
];

impl Keymap {
    /// The built-in bindings.
    ///
    /// # Panics
    /// Panics if the built-in table is malformed, which a test catches long
    /// before a release.
    #[must_use]
    pub fn defaults() -> Self {
        let mut map = Self::default();
        for &(mode, chord, action) in DEFAULTS {
            let chord = chord.parse().expect("built-in chord parses");
            map.bind(mode, chord, action)
                .expect("built-in bindings are unique");
        }
        map
    }

    /// Add a binding.
    ///
    /// # Errors
    /// Returns an error when the chord is already bound in that mode. Silently
    /// overwriting would make one of the two bindings unreachable while still
    /// showing both in the help overlay.
    pub fn bind(
        &mut self,
        mode: Mode,
        chord: Chord,
        action: Action,
    ) -> Result<(), DuplicateBinding> {
        if let Some(&existing) = self.lookup.get(&(mode, chord)) {
            return Err(DuplicateBinding {
                chord,
                mode,
                existing,
                incoming: action,
            });
        }
        self.lookup.insert((mode, chord), action);
        self.order.push((mode, chord, action));
        Ok(())
    }

    /// The action a key event triggers in `mode`.
    #[must_use]
    pub fn action(&self, mode: Mode, event: KeyEvent) -> Option<Action> {
        self.lookup.get(&(mode, Chord::from_event(event))).copied()
    }

    /// Every binding in `mode`, in declaration order.
    pub fn bindings(&self, mode: Mode) -> impl Iterator<Item = (Chord, Action)> + '_ {
        self.order
            .iter()
            .filter(move |(m, _, _)| *m == mode)
            .map(|&(_, chord, action)| (chord, action))
    }

    /// The chords bound to `action` in `mode`, in declaration order.
    #[must_use]
    pub fn chords(&self, mode: Mode, action: Action) -> Vec<Chord> {
        self.bindings(mode)
            .filter(|(_, bound)| *bound == action)
            .map(|(chord, _)| chord)
            .collect()
    }

    /// The actions reachable in `mode`, in declaration order, each with its
    /// chords joined for display. This is what the help overlay renders.
    #[must_use]
    pub fn help_rows(&self, mode: Mode) -> Vec<(String, Action)> {
        let mut seen = Vec::new();
        let mut rows = Vec::new();
        for (_, action) in self.bindings(mode) {
            if seen.contains(&action) {
                continue;
            }
            seen.push(action);
            let keys = self
                .chords(mode, action)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            rows.push((keys, action));
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn the_built_in_table_has_no_duplicate_chord_in_a_mode() {
        // `defaults` panics on a duplicate; this states the requirement.
        let map = Keymap::defaults();
        for mode in Mode::ALL {
            let mut seen = Vec::new();
            for (chord, _) in map.bindings(*mode) {
                assert!(!seen.contains(&chord), "`{chord}` bound twice in {mode}");
                seen.push(chord);
            }
        }
    }

    #[test]
    fn a_duplicate_binding_is_reported_rather_than_overwriting() {
        let mut map = Keymap::defaults();
        let err = map
            .bind(Mode::Document, "j".parse().unwrap(), Action::Quit)
            .unwrap_err();
        assert_eq!(err.existing, Action::LineDown);
        assert!(err.to_string().contains("bound twice"), "{err}");
        // The original binding survives.
        assert_eq!(
            map.action(Mode::Document, key(KeyCode::Char('j'))),
            Some(Action::LineDown)
        );
    }

    #[test]
    fn the_same_chord_may_be_bound_in_different_modes() {
        let map = Keymap::defaults();
        assert_eq!(
            map.action(Mode::Document, key(KeyCode::Char('q'))),
            Some(Action::Quit)
        );
        assert_eq!(
            map.action(Mode::Help, key(KeyCode::Char('q'))),
            Some(Action::Escape)
        );
    }

    #[test]
    fn case_distinguishes_keys_but_shift_reporting_does_not() {
        let map = Keymap::defaults();
        assert_eq!(
            map.action(Mode::Document, key(KeyCode::Char('g'))),
            Some(Action::Top)
        );
        assert_eq!(
            map.action(Mode::Document, key(KeyCode::Char('G'))),
            Some(Action::Bottom)
        );
        // A terminal that also reports shift for `G` must not lose the binding.
        let shifted = KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(map.action(Mode::Document, shifted), Some(Action::Bottom));
    }

    #[test]
    fn an_unbound_key_maps_to_nothing() {
        let map = Keymap::defaults();
        assert_eq!(map.action(Mode::Document, key(KeyCode::Char('z'))), None);
    }

    #[test]
    fn chords_round_trip_through_text() {
        for text in [
            "j", "G", "?", "space", "esc", "ctrl+d", "alt+x", "pgdn", "f5", "home",
        ] {
            let chord: Chord = text.parse().expect(text);
            assert_eq!(chord.to_string(), text, "round trip for {text}");
        }
    }

    #[test]
    fn a_malformed_chord_is_rejected() {
        for text in ["hyper+j", "notakey", ""] {
            assert!(text.parse::<Chord>().is_err(), "accepted `{text}`");
        }
    }

    #[test]
    fn every_documented_glow_pager_key_still_does_what_it_did() {
        let map = Keymap::defaults();
        let expected = [
            (KeyCode::Char('k'), Action::LineUp),
            (KeyCode::Char('j'), Action::LineDown),
            (KeyCode::Char('b'), Action::PageUp),
            (KeyCode::Char('f'), Action::PageDown),
            (KeyCode::Char('u'), Action::HalfPageUp),
            (KeyCode::Char('d'), Action::HalfPageDown),
            (KeyCode::Char('g'), Action::Top),
            (KeyCode::Char('G'), Action::Bottom),
            (KeyCode::Char('q'), Action::Quit),
        ];
        for (code, action) in expected {
            assert_eq!(
                map.action(Mode::Document, key(code)),
                Some(action),
                "glow parity for {code:?}"
            );
        }
    }

    #[test]
    fn help_rows_list_each_action_once_with_all_its_keys() {
        let map = Keymap::defaults();
        let rows = map.help_rows(Mode::Document);
        let down = rows
            .iter()
            .find(|(_, action)| *action == Action::LineDown)
            .expect("line-down is listed");
        assert_eq!(down.0, "j down");
        let actions: Vec<_> = rows.iter().map(|(_, a)| *a).collect();
        let mut unique = actions.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(actions.len(), unique.len(), "an action is listed twice");
    }

    #[test]
    fn every_action_is_reachable_from_some_mode() {
        let map = Keymap::defaults();
        for &action in Action::ALL {
            let bound = Mode::ALL
                .iter()
                .any(|mode| !map.chords(*mode, action).is_empty());
            assert!(bound, "{action} has no default binding");
        }
    }
}
