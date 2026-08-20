//! Where a setting comes from, and which source wins.
//!
//! Every source produces a [`Layer`] in which absent means "no opinion", and
//! [`Layer::over`] is the one place precedence is defined for the whole
//! program. Doing it field by field at each call site is how a program ends up
//! with a flag that beats the config in one place and loses in another.

use super::schema::File;
use crate::render::HtmlMode;

/// Settings from one source. `None` means the source said nothing, which is
/// different from saying "off".
///
/// Adding a setting is routine here, and adding a public field to a struct
/// anyone can write as a literal is a breaking change — which made every
/// new setting cost a release. `non_exhaustive` buys that back: outside
/// this crate these are built from [`Default`] and then assigned to, so a
/// field arriving later is not an API break.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Layer {
    /// Theme name or path.
    pub style: Option<String>,
    /// Content width; `Some(0)` disables wrapping.
    pub width: Option<u16>,
    /// Line numbers in code blocks.
    pub line_numbers: Option<bool>,
    /// Mouse wheel scrolling.
    pub mouse: Option<bool>,
    /// List hidden and ignored files when browsing.
    pub all: Option<bool>,
    /// Keep the line breaks the author typed.
    pub preserve_new_lines: Option<bool>,
    /// Check crates.io for a newer release, and say so on the way out.
    pub update_check: Option<bool>,
    /// Start with the contents pane showing.
    pub contents: Option<bool>,
    /// What to do with raw HTML.
    pub html: Option<HtmlMode>,
}

impl Layer {
    /// Lay this layer over `lower`: this one's opinions win, and anything it
    /// has no opinion about falls through.
    ///
    /// One `or` per field, in one function. This *is* the definition of
    /// precedence, and it is testable without a filesystem or an environment.
    #[must_use]
    pub fn over(self, lower: Self) -> Self {
        Self {
            style: self.style.or(lower.style),
            width: self.width.or(lower.width),
            line_numbers: self.line_numbers.or(lower.line_numbers),
            mouse: self.mouse.or(lower.mouse),
            all: self.all.or(lower.all),
            preserve_new_lines: self.preserve_new_lines.or(lower.preserve_new_lines),
            update_check: self.update_check.or(lower.update_check),
            contents: self.contents.or(lower.contents),
            html: self.html.or(lower.html),
        }
    }

    /// What the program does when nothing else has an opinion.
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            style: Some("auto".to_owned()),
            width: None,
            line_numbers: Some(false),
            mouse: Some(false),
            all: Some(false),
            preserve_new_lines: Some(false),
            update_check: Some(true),
            // The contents pane is the reason the program exists; it still
            // hides itself on a narrow terminal or a document with nothing to
            // list.
            contents: Some(true),
            // Documentation is written in markdown with HTML holes in it, and
            // the tags are not what the author meant to say.
            html: Some(HtmlMode::Render),
        }
    }

    /// The settings a configuration file asked for.
    #[must_use]
    pub fn from_file(file: &File) -> Self {
        Self {
            style: file.general.style.clone(),
            width: file.general.width,
            line_numbers: file.general.line_numbers,
            mouse: file.general.mouse,
            all: file.general.all,
            preserve_new_lines: file.general.preserve_new_lines,
            update_check: file.general.update_check,
            contents: file.ui.contents,
            html: file
                .render
                .html
                .as_deref()
                .and_then(|value| value.parse().ok()),
        }
    }

    /// The settings the environment asked for, and anything it said that could
    /// not be understood.
    ///
    /// Takes a lookup rather than reading the environment directly, so the
    /// whole of it is testable — and because setting environment variables is
    /// `unsafe` in this edition, which a library that forbids unsafe code
    /// cannot do even in a test.
    #[must_use]
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let layer = Self {
            style: get("MARQUEE_STYLE").filter(|value| !value.is_empty()),
            width: number(get, "MARQUEE_WIDTH", &mut warnings),
            line_numbers: flag(get, "MARQUEE_LINE_NUMBERS", &mut warnings),
            mouse: flag(get, "MARQUEE_MOUSE", &mut warnings),
            all: flag(get, "MARQUEE_ALL", &mut warnings),
            preserve_new_lines: flag(get, "MARQUEE_PRESERVE_NEW_LINES", &mut warnings),
            update_check: flag(get, "MARQUEE_UPDATE_CHECK", &mut warnings),
            contents: flag(get, "MARQUEE_UI_CONTENTS", &mut warnings),
            html: choice(get, "MARQUEE_RENDER_HTML", &mut warnings),
        };
        (layer, warnings)
    }
}

/// Read an environment variable that names one of a fixed set of choices.
///
/// A value that is not one of them is reported and ignored, like every other
/// setting this program does not understand: a typo costs one setting, not the
/// whole environment.
fn choice<T: std::str::FromStr<Err = String>>(
    get: &dyn Fn(&str) -> Option<String>,
    name: &str,
    warnings: &mut Vec<String>,
) -> Option<T> {
    let raw = get(name).filter(|value| !value.trim().is_empty())?;
    match raw.parse() {
        Ok(value) => Some(value),
        Err(why) => {
            warnings.push(format!("{name}: {why}"));
            None
        }
    }
}

/// Read a boolean environment variable, generously.
fn flag(
    get: &dyn Fn(&str) -> Option<String>,
    name: &str,
    warnings: &mut Vec<String>,
) -> Option<bool> {
    let raw = get(name)?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        other => {
            warnings.push(format!("{name}: `{other}` is not yes or no; ignoring it"));
            None
        }
    }
}

/// Read a numeric environment variable.
fn number(
    get: &dyn Fn(&str) -> Option<String>,
    name: &str,
    warnings: &mut Vec<String>,
) -> Option<u16> {
    let raw = get(name)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse() {
        Ok(value) => Some(value),
        Err(_) => {
            warnings.push(format!("{name}: `{trimmed}` is not a number; ignoring it"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        move |name: &str| {
            owned
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn a_higher_layer_wins_where_it_has_an_opinion() {
        let higher = Layer {
            style: Some("paper".to_owned()),
            ..Layer::default()
        };
        let lower = Layer {
            style: Some("slate".to_owned()),
            width: Some(72),
            ..Layer::default()
        };
        let resolved = higher.over(lower);
        assert_eq!(resolved.style.as_deref(), Some("paper"));
        assert_eq!(resolved.width, Some(72), "the lower layer was lost");
    }

    #[test]
    fn saying_off_is_not_the_same_as_saying_nothing() {
        // The distinction the whole design rests on: a flag that is absent
        // must not override a configuration file that turned something on.
        let quiet = Layer::default();
        let on = Layer {
            line_numbers: Some(true),
            ..Layer::default()
        };
        assert_eq!(quiet.clone().over(on.clone()).line_numbers, Some(true));

        let off = Layer {
            line_numbers: Some(false),
            ..Layer::default()
        };
        assert_eq!(off.over(on).line_numbers, Some(false));
    }

    #[test]
    fn the_full_ladder_resolves_in_order() {
        let flags = Layer {
            width: Some(60),
            ..Layer::default()
        };
        let environment = Layer {
            width: Some(70),
            style: Some("paper".to_owned()),
            ..Layer::default()
        };
        let file = Layer {
            width: Some(80),
            style: Some("slate".to_owned()),
            mouse: Some(true),
            ..Layer::default()
        };
        let resolved = flags.over(environment).over(file).over(Layer::defaults());
        assert_eq!(resolved.width, Some(60), "flags lost");
        assert_eq!(resolved.style.as_deref(), Some("paper"), "environment lost");
        assert_eq!(resolved.mouse, Some(true), "file lost");
        assert_eq!(resolved.all, Some(false), "defaults lost");
    }

    #[test]
    fn defaults_have_an_opinion_about_everything_that_needs_one() {
        let defaults = Layer::defaults();
        assert!(defaults.style.is_some());
        assert!(defaults.line_numbers.is_some());
        assert!(defaults.mouse.is_some());
        assert!(defaults.all.is_some());
        assert!(defaults.preserve_new_lines.is_some());
        assert!(defaults.update_check.is_some());
        assert!(defaults.contents.is_some());
        // Width is deliberately undecided: it comes from the terminal.
        assert!(defaults.width.is_none());
    }

    #[test]
    fn booleans_from_the_environment_are_read_generously() {
        for (value, expected) in [
            ("1", true),
            ("true", true),
            ("YES", true),
            ("on", true),
            ("0", false),
            ("false", false),
            ("no", false),
            ("Off", false),
        ] {
            let (layer, warnings) = Layer::from_env(&env(&[("MARQUEE_MOUSE", value)]));
            assert_eq!(layer.mouse, Some(expected), "{value}");
            assert!(warnings.is_empty(), "{warnings:?}");
        }
    }

    #[test]
    fn nonsense_in_the_environment_is_reported_and_ignored() {
        let (layer, warnings) = Layer::from_env(&env(&[
            ("MARQUEE_MOUSE", "maybe"),
            ("MARQUEE_WIDTH", "wide"),
        ]));
        assert_eq!(layer.mouse, None);
        assert_eq!(layer.width, None);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("MARQUEE_MOUSE")));
        assert!(warnings.iter().any(|w| w.contains("MARQUEE_WIDTH")));
    }

    #[test]
    fn an_empty_environment_variable_says_nothing() {
        // Otherwise `MARQUEE_STYLE=` in a shell profile would select a theme
        // called the empty string.
        let (layer, warnings) = Layer::from_env(&env(&[
            ("MARQUEE_STYLE", ""),
            ("MARQUEE_WIDTH", "  "),
            ("MARQUEE_MOUSE", ""),
        ]));
        assert_eq!(layer, Layer::default());
        assert!(warnings.is_empty());
    }

    #[test]
    fn the_environment_reads_general_settings_without_a_section_name() {
        let (layer, _) = Layer::from_env(&env(&[
            ("MARQUEE_STYLE", "paper"),
            ("MARQUEE_WIDTH", "72"),
            ("MARQUEE_PRESERVE_NEW_LINES", "true"),
            ("MARQUEE_UI_CONTENTS", "false"),
        ]));
        assert_eq!(layer.style.as_deref(), Some("paper"));
        assert_eq!(layer.width, Some(72));
        assert_eq!(layer.preserve_new_lines, Some(true));
        assert_eq!(layer.contents, Some(false), "sections are spelled out");
    }

    #[test]
    fn a_file_becomes_a_layer_field_for_field() {
        let (file, _) = super::super::schema::parse(
            "[general]\nstyle = \"paper\"\nall = true\n\n[ui]\ncontents = false\n",
        )
        .expect("parse");
        let layer = Layer::from_file(&file);
        assert_eq!(layer.style.as_deref(), Some("paper"));
        assert_eq!(layer.all, Some(true));
        assert_eq!(layer.contents, Some(false));
        assert_eq!(layer.mouse, None);
    }
}
