//! Applying a configuration file's key bindings over the defaults.
//!
//! A binding in a file is an instruction, not a proposal: it replaces whatever
//! held that chord. Anything that cannot be understood is reported and skipped,
//! so one typo costs the reader one key rather than the whole keymap.

use std::collections::BTreeMap;

use crate::app::action::Action;
use crate::app::keymap::{Chord, Keymap, Mode};

/// Action names that mean "make this key do nothing".
const UNBIND: &[&str] = &["", "none", "unbind", "nothing"];

/// Lay a file's `[keys.*]` sections over `keymap`, returning what could not be
/// used.
pub fn merge(
    keymap: &mut Keymap,
    sections: &BTreeMap<String, BTreeMap<String, String>>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    for (mode_name, bindings) in sections {
        // An unknown mode has already been reported as an unknown key by the
        // schema; reporting it twice would be noise.
        let Some(mode) = Mode::from_name(mode_name) else {
            continue;
        };
        for (chord_text, action_text) in bindings {
            let Ok(chord) = chord_text.parse::<Chord>() else {
                warnings.push(format!(
                    "keys.{mode_name}: `{chord_text}` is not a key this understands"
                ));
                continue;
            };
            if UNBIND.contains(&action_text.trim().to_ascii_lowercase().as_str()) {
                keymap.unbind(mode, chord);
                continue;
            }
            match action_text.parse::<Action>() {
                Ok(action) => keymap.rebind(mode, chord, action),
                Err(_) => warnings.push(format!(
                    "keys.{mode_name}: `{action_text}` is not something this can do"
                )),
            }
        }
    }
    warnings
}

/// The default bindings, as a markdown reference.
///
/// Generated rather than written by hand, for the same reason the help overlay
/// is: a key reference that has drifted is worse than none.
#[must_use]
pub fn reference(keymap: &Keymap) -> String {
    let mut out = String::from(
        "# Keybindings\n\n\
         Generated from the default keymap — do not edit by hand. Regenerate with:\n\n\
         ```sh\n\
         cargo run -- keys > docs/KEYBINDINGS.md\n\
         ```\n\n\
         Keys are written the way a configuration file spells them, so anything\n\
         here can be pasted into `[keys.<mode>]`.\n",
    );
    for mode in Mode::ALL {
        let rows = keymap.help_rows(*mode);
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "\n## `{}`\n\n| Key | Action | |\n| --- | --- | --- |\n",
            mode.name()
        ));
        for (keys, action) in rows {
            let keys = keys
                .split(' ')
                .map(|chord| format!("`{chord}`"))
                .collect::<Vec<_>>()
                .join(" ");
            out.push_str(&format!(
                "| {keys} | `{}` | {} |\n",
                action.name(),
                action.describe()
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn sections(pairs: &[(&str, &[(&str, &str)])]) -> BTreeMap<String, BTreeMap<String, String>> {
        pairs
            .iter()
            .map(|(mode, bindings)| {
                (
                    (*mode).to_owned(),
                    bindings
                        .iter()
                        .map(|(chord, action)| ((*chord).to_owned(), (*action).to_owned()))
                        .collect(),
                )
            })
            .collect()
    }

    fn action_for(keymap: &Keymap, mode: Mode, code: KeyCode) -> Option<Action> {
        keymap.action(mode, KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn a_binding_replaces_whatever_held_the_key() {
        let mut keymap = Keymap::defaults();
        let warnings = merge(&mut keymap, &sections(&[("document", &[("j", "quit")])]));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            action_for(&keymap, Mode::Document, KeyCode::Char('j')),
            Some(Action::Quit)
        );
    }

    #[test]
    fn a_binding_in_one_mode_leaves_the_others_alone() {
        let mut keymap = Keymap::defaults();
        merge(&mut keymap, &sections(&[("browser", &[("j", "quit")])]));
        assert_eq!(
            action_for(&keymap, Mode::Document, KeyCode::Char('j')),
            Some(Action::LineDown)
        );
        assert_eq!(
            action_for(&keymap, Mode::Browser, KeyCode::Char('j')),
            Some(Action::Quit)
        );
    }

    #[test]
    fn a_key_can_be_taken_away() {
        let mut keymap = Keymap::defaults();
        for spelling in ["none", "NONE", "unbind", "nothing", ""] {
            let mut keymap = keymap.clone();
            merge(&mut keymap, &sections(&[("document", &[("q", spelling)])]));
            assert_eq!(
                action_for(&keymap, Mode::Document, KeyCode::Char('q')),
                None,
                "{spelling}"
            );
        }
        merge(&mut keymap, &sections(&[("document", &[("q", "none")])]));
        assert_eq!(
            action_for(&keymap, Mode::Document, KeyCode::Char('q')),
            None
        );
    }

    #[test]
    fn a_key_that_cannot_be_parsed_costs_one_key_and_not_the_keymap() {
        let mut keymap = Keymap::defaults();
        let warnings = merge(
            &mut keymap,
            &sections(&[("document", &[("hyper+j", "quit"), ("x", "top")])]),
        );
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("hyper+j"), "{warnings:?}");
        // The rest of the section still took effect.
        assert_eq!(
            action_for(&keymap, Mode::Document, KeyCode::Char('x')),
            Some(Action::Top)
        );
        // And the defaults survived.
        assert_eq!(
            action_for(&keymap, Mode::Document, KeyCode::Char('j')),
            Some(Action::LineDown)
        );
    }

    #[test]
    fn an_action_that_does_not_exist_is_reported_by_name() {
        let mut keymap = Keymap::defaults();
        let warnings = merge(
            &mut keymap,
            &sections(&[("document", &[("x", "make-coffee")])]),
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("make-coffee"), "{warnings:?}");
        assert_eq!(
            action_for(&keymap, Mode::Document, KeyCode::Char('x')),
            None
        );
    }

    #[test]
    fn an_unknown_mode_is_skipped_here_because_the_schema_reports_it() {
        let mut keymap = Keymap::defaults();
        let warnings = merge(&mut keymap, &sections(&[("spaceship", &[("x", "quit")])]));
        assert!(warnings.is_empty(), "reported twice: {warnings:?}");
    }

    #[test]
    fn the_generated_reference_covers_every_mode_and_action() {
        let keymap = Keymap::defaults();
        let text = reference(&keymap);
        for mode in Mode::ALL {
            assert!(text.contains(&format!("## `{}`", mode.name())), "{}", mode);
        }
        for action in Action::ALL {
            assert!(
                text.contains(&format!("`{}`", action.name())),
                "{action} is missing from the reference"
            );
        }
    }

    #[test]
    fn the_reference_spells_keys_the_way_a_config_file_does() {
        let text = reference(&Keymap::defaults());
        assert!(text.contains("`ctrl+d`"), "{text}");
        assert!(text.contains("`pgdn`"), "{text}");
    }
}
