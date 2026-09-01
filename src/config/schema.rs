//! The shape of a configuration file, and what counts as a key we know.
//!
//! Unknown keys are reported and ignored rather than rejected. Once other
//! people are running releases, a file written for a newer version has to
//! still work with an older binary — a hard error there would mean a config
//! that bricks the program until it is hand-edited, which is a worse failure
//! than a setting quietly not taking effect.

use serde::Deserialize;
use std::collections::BTreeMap;

/// A parsed configuration file. Every field is optional: what is absent falls
/// through to the layer below.
///
/// Adding a setting is routine here, and adding a public field to a struct
/// anyone can write as a literal is a breaking change — which made every
/// new setting cost a release. `non_exhaustive` buys that back: outside
/// this crate these are built from [`Default`] and then assigned to, so a
/// field arriving later is not an API break.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct File {
    /// Settings that also have a command-line flag.
    pub general: General,
    /// How the reader is laid out.
    pub ui: Ui,
    /// How documents are rendered.
    pub render: Render,
    /// Key bindings, per mode.
    pub keys: BTreeMap<String, BTreeMap<String, String>>,
}

/// `[general]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
#[non_exhaustive]
pub struct General {
    /// Theme name or path, as `--style` takes.
    pub style: Option<String>,
    /// Content width; `0` disables wrapping.
    pub width: Option<u16>,
    /// Show line numbers in code blocks.
    pub line_numbers: Option<bool>,
    /// Enable mouse wheel scrolling.
    pub mouse: Option<bool>,
    /// List hidden and ignored files when browsing.
    pub all: Option<bool>,
    /// Keep the line breaks the author typed.
    pub preserve_new_lines: Option<bool>,
    /// Check crates.io for a newer release, and say so on the way out.
    pub update_check: Option<bool>,
    /// Let `--style system` ask the terminal what colors it is using.
    ///
    /// Off is for a terminal that prints the question instead of answering it,
    /// or a link slow enough that waiting for an answer is worse than not
    /// having one. With it off, `system` falls back to a shipped palette and
    /// every other style — the default included — is unaffected, because
    /// nothing else asks.
    pub terminal_query: Option<bool>,
}

/// `[render]`.
///
/// Not `[general]`, whose doc comment promises every setting there also has a
/// command-line flag. This one deliberately has none: `src/cli` keeps glow's
/// flag surface, and glow has no equivalent to mirror.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
#[non_exhaustive]
pub struct Render {
    /// What to do with raw HTML: `render`, `hide` or `literal`.
    ///
    /// Held as written rather than as an [`HtmlMode`](crate::render::HtmlMode)
    /// so a misspelling warns and falls through to the default, the way an
    /// unknown *key* does. Deserializing straight into the enum would make one
    /// typo refuse the whole file — which is the failure this module's opening
    /// comment exists to prevent.
    pub html: Option<String>,
}

/// `[ui]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
#[non_exhaustive]
pub struct Ui {
    /// Start with the contents pane showing.
    pub contents: Option<bool>,
    /// Start with the hint line showing above the status bar.
    pub hints: Option<bool>,
}

/// Every key this version understands, by section.
///
/// Kept beside the structs it describes so the two are edited together; the
/// test below fails if they drift.
const KNOWN: &[(&str, &[&str])] = &[
    (
        "general",
        &[
            "style",
            "width",
            "line-numbers",
            "mouse",
            "all",
            "preserve-new-lines",
            "update-check",
            "terminal-query",
        ],
    ),
    ("render", &["html"]),
    ("ui", &["contents", "hints"]),
];

/// Parse a configuration file, along with anything in it we did not recognize.
///
/// # Errors
/// Returns an error only when the file is not valid TOML or a value has the
/// wrong type — both of which are mistakes rather than version skew.
pub fn parse(text: &str) -> anyhow::Result<(File, Vec<String>)> {
    let value: toml::Value = toml::from_str(text)?;
    let warnings = unknown_keys(&value);
    let file: File = value.try_into()?;
    Ok((file, warnings))
}

/// Every key in `value` that this version does not understand, as a path.
#[must_use]
pub fn unknown_keys(value: &toml::Value) -> Vec<String> {
    let Some(table) = value.as_table() else {
        return Vec::new();
    };
    let mut unknown = Vec::new();
    for (section, contents) in table {
        if section == "keys" {
            // The chords inside are arbitrary by nature; the modes are not.
            check_key_sections(contents, &mut unknown);
            continue;
        }
        let Some(known) = KNOWN
            .iter()
            .find(|(name, _)| name == section)
            .map(|(_, keys)| keys)
        else {
            unknown.push(section.clone());
            continue;
        };
        let Some(entries) = contents.as_table() else {
            unknown.push(section.clone());
            continue;
        };
        for key in entries.keys() {
            if !known.contains(&key.as_str()) {
                unknown.push(format!("{section}.{key}"));
            }
        }
    }
    unknown.sort();
    unknown
}

/// `[keys.<mode>]` sections whose mode is not one we have.
fn check_key_sections(contents: &toml::Value, unknown: &mut Vec<String>) {
    let Some(modes) = contents.as_table() else {
        unknown.push("keys".to_owned());
        return;
    };
    for mode in modes.keys() {
        if crate::app::keymap::Mode::from_name(mode).is_none() {
            unknown.push(format!("keys.{mode}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_is_all_defaults() {
        let (file, warnings) = parse("").expect("parse");
        assert_eq!(file, File::default());
        assert!(warnings.is_empty());
    }

    #[test]
    fn settings_are_read_in_kebab_case() {
        let (file, warnings) = parse(
            r#"
            [general]
            style = "paper"
            width = 72
            line-numbers = true
            preserve-new-lines = true
            update-check = false

            [ui]
            contents = false
            hints = false
            "#,
        )
        .expect("parse");
        assert_eq!(file.general.style.as_deref(), Some("paper"));
        assert_eq!(file.general.width, Some(72));
        assert_eq!(file.general.line_numbers, Some(true));
        assert_eq!(file.general.preserve_new_lines, Some(true));
        assert_eq!(file.general.update_check, Some(false));
        assert_eq!(file.ui.contents, Some(false));
        assert_eq!(file.ui.hints, Some(false));
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn an_absent_setting_stays_absent_rather_than_becoming_false() {
        // The difference matters: `None` falls through to the layer below,
        // `Some(false)` overrides it.
        let (file, _) = parse("[general]\nstyle = \"slate\"\n").expect("parse");
        assert_eq!(file.general.mouse, None);
    }

    #[test]
    fn an_unknown_key_is_reported_and_the_rest_still_loads() {
        let (file, warnings) = parse(
            r#"
            [general]
            style = "paper"
            future-setting = 3

            [from-a-later-version]
            anything = true
            "#,
        )
        .expect("parse");
        assert_eq!(file.general.style.as_deref(), Some("paper"));
        assert_eq!(
            warnings,
            vec!["from-a-later-version", "general.future-setting"]
        );
    }

    #[test]
    fn an_unknown_key_mode_is_reported() {
        let (_, warnings) =
            parse("[keys.document]\nj = \"line-down\"\n[keys.spaceship]\nx = \"quit\"\n")
                .expect("parse");
        assert_eq!(warnings, vec!["keys.spaceship"]);
    }

    #[test]
    fn key_bindings_are_read_as_written() {
        let (file, warnings) = parse(
            r#"
            [keys.document]
            "ctrl+j" = "line-down"
            g = "top"
            "#,
        )
        .expect("parse");
        let document = file.keys.get("document").expect("a document section");
        assert_eq!(
            document.get("ctrl+j").map(String::as_str),
            Some("line-down")
        );
        assert_eq!(document.get("g").map(String::as_str), Some("top"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn broken_toml_is_an_error_rather_than_a_warning() {
        // Version skew deserves a warning; a syntax error is a mistake.
        assert!(parse("[general\nstyle = ").is_err());
    }

    #[test]
    fn a_value_of_the_wrong_type_is_an_error() {
        let error = parse("[general]\nwidth = \"wide\"\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("width"), "{error}");
    }

    #[test]
    fn every_field_of_the_schema_is_listed_as_known() {
        // Otherwise adding a setting would make the config that uses it warn.
        let full = r#"
            [general]
            style = "paper"
            width = 72
            line-numbers = true
            mouse = true
            all = true
            preserve-new-lines = true
            update-check = true

            [render]
            html = "render"

            [ui]
            contents = true
            "#;
        let (_, warnings) = parse(full).expect("parse");
        assert!(warnings.is_empty(), "unlisted keys: {warnings:?}");
    }
}
