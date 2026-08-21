//! Resolving a `--style` argument to a [`Theme`].
//!
//! Selection order: a filesystem path wins, then a user theme in the config
//! directory, then a compiled-in name, then the `auto`/`system`/`notty`
//! specials. `auto` is an alias for the dark palette, spelled the way glow
//! spells it; `system` is the one that looks at the terminal. Everything goes through the same constructor — a community
//! theme, a shipped one, and the one built from the terminal's own colors are
//! the same kind of thing.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use super::system::{self, TerminalColors};
use super::{Appearance, Theme, ThemeVariant};

/// Directory holding user-authored themes.
#[must_use]
pub fn user_theme_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("marquee-markdown").join("themes"))
}

/// A theme that is available to select, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub origin: Origin,
}

/// Where a theme was found.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Origin {
    /// Compiled into the binary.
    BuiltIn,
    /// Loaded from the user's theme directory.
    User(PathBuf),
    /// Built from the colors the terminal reported for itself.
    Terminal,
}

/// Every selectable theme, built-ins first.
///
/// `system` is always here, whether or not the terminal turned out to answer.
/// Listing it conditionally would make `themes` say different things down a
/// pipe than on a screen, and would make it appear and disappear from the
/// picker for reasons the reader cannot see; a terminal that says nothing
/// simply makes `system` a shipped palette, which is what it honestly is.
#[must_use]
pub fn list() -> Vec<Entry> {
    let mut out: Vec<Entry> = ThemeVariant::all()
        .iter()
        .map(|v| Entry {
            name: v.name().to_owned(),
            origin: Origin::BuiltIn,
        })
        .collect();
    out.push(Entry {
        name: SYSTEM.to_owned(),
        origin: Origin::Terminal,
    });
    if let Some(dir) = user_theme_dir()
        && let Ok(entries) = std::fs::read_dir(&dir)
    {
        let mut user: Vec<Entry> = entries
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                if path.extension().is_some_and(|x| x == "toml") {
                    let name = path.file_stem()?.to_str()?.to_owned();
                    Some(Entry {
                        name,
                        origin: Origin::User(path),
                    })
                } else {
                    None
                }
            })
            .collect();
        user.sort_by(|a, b| a.name.cmp(&b.name));
        out.extend(user);
    }
    out
}

/// The name of the theme built from the terminal's own colors.
pub const SYSTEM: &str = "system";

/// Resolve a `--style` value.
///
/// `terminal` is what the terminal answered when asked about its own colors —
/// [`TerminalColors::UNKNOWN`] when it was not asked or did not answer. Only
/// `system` consults it. `auto` does not, despite the name: it is an alias for
/// the dark palette.
///
/// # Errors
/// Returns an error when the name matches no theme, listing what is available.
pub fn resolve(style: &str, terminal: &TerminalColors) -> Result<Theme> {
    let style = style.trim();

    // An explicit path always wins, so a theme can be tried without installing.
    let as_path = Path::new(style);
    if style.contains(['/', '\\']) || as_path.extension().is_some_and(|e| e == "toml") {
        return Theme::from_file(as_path);
    }

    match style.to_ascii_lowercase().as_str() {
        // An alias for the dark palette, not an adaptive choice — the name is
        // glow's, kept so its flags carry over, and the behavior is glow's
        // too. It resolves the same whatever the terminal says, and stays that
        // way deliberately: `auto` is the default, so anything it decided
        // differently would be decided differently for every reader who never
        // chose a theme. `--style system` is the adaptive one, and asking for
        // it is how you say you want it.
        "auto" => Ok(Theme::new(ThemeVariant::Slate)),
        // A terminal that answered too little is not an error: falling back to
        // a shipped palette keeps a document on the screen, which is what the
        // reader asked for. Refusing to start over a palette question would
        // not be.
        SYSTEM => Ok(system::theme(terminal).unwrap_or_else(|| Theme::new(nearest(terminal)))),
        "notty" | "plain" | "none" => Ok(Theme::plain()),
        name => {
            if let Ok(variant) = name.parse::<ThemeVariant>() {
                return Ok(Theme::new(variant));
            }
            if let Some(Entry {
                origin: Origin::User(path),
                ..
            }) = list().into_iter().find(|e| e.name == name)
            {
                return Theme::from_file(&path);
            }
            let available: Vec<String> = list().into_iter().map(|e| e.name).collect();
            bail!(
                "unknown style {style:?} (available: {}, auto, notty)",
                available.join(", ")
            )
        }
    }
}

/// The shipped palette closest to the terminal's own background.
///
/// Where `system` lands when the terminal described itself too poorly to build
/// from. Half an answer about the background is still worth having, so this
/// uses it — unlike `auto`, which is an alias for the dark palette and asks
/// nothing.
fn nearest(terminal: &TerminalColors) -> ThemeVariant {
    match terminal.is_dark() {
        Some(false) => ThemeVariant::Paper,
        // Default to the dark palette when unknown, as glow does.
        _ => ThemeVariant::Slate,
    }
}

/// Whether a theme is meant for a dark terminal.
#[must_use]
pub fn is_dark(theme: &Theme) -> bool {
    theme.appearance == Appearance::Dark
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Rgb;

    /// The answer a terminal with a real colorscheme gives.
    fn answered(bg: Rgb, fg: Rgb) -> TerminalColors {
        TerminalColors {
            fg: Some(fg),
            bg: Some(bg),
            ..TerminalColors::UNKNOWN
        }
    }

    fn dark_terminal() -> TerminalColors {
        answered(Rgb(0x18, 0x18, 0x18), Rgb(0xd8, 0xd8, 0xd8))
    }

    fn light_terminal() -> TerminalColors {
        answered(Rgb(0xf8, 0xf8, 0xf8), Rgb(0x18, 0x18, 0x18))
    }

    fn silent() -> TerminalColors {
        TerminalColors::UNKNOWN
    }

    #[test]
    fn built_in_names_resolve() {
        assert_eq!(resolve("paper", &silent()).unwrap().name, "paper");
        assert_eq!(resolve("slate", &silent()).unwrap().name, "slate");
    }

    #[test]
    fn light_and_dark_aliases_resolve() {
        assert_eq!(resolve("light", &silent()).unwrap().name, "paper");
        assert_eq!(resolve("dark", &silent()).unwrap().name, "slate");
    }

    #[test]
    fn the_default_style_and_auto_are_the_same_theme() {
        // The shipped default names the palette rather than `auto`, and `auto`
        // is kept because glow spells it that way. The two have always meant
        // the same thing; this is what stops them drifting apart, in either
        // direction, without somebody deciding to.
        let default = crate::config::Layer::defaults()
            .style
            .expect("a default style");
        assert_eq!(
            resolve(&default, &silent()).unwrap().name,
            resolve("auto", &silent()).unwrap().name
        );
    }

    #[test]
    fn auto_is_an_alias_for_the_dark_palette() {
        // `auto` is an alias, not an adaptive choice, and it is the default —
        // so what it decides is what every reader who never chose a theme
        // sees. It resolves the same with the answer sitting right there.
        for terminal in [silent(), dark_terminal(), light_terminal()] {
            assert_eq!(resolve("auto", &terminal).unwrap().name, "slate");
        }
    }

    #[test]
    fn system_is_the_terminals_own_colors() {
        let theme = resolve("system", &dark_terminal()).unwrap();
        assert_eq!(theme.name, "system");
        assert_eq!(theme.palette.bg, Rgb(0x18, 0x18, 0x18));
        assert_eq!(theme.palette.fg, Rgb(0xd8, 0xd8, 0xd8));
        assert_eq!(theme.appearance, Appearance::Dark);
    }

    #[test]
    fn system_follows_a_light_terminal_too() {
        let theme = resolve("system", &light_terminal()).unwrap();
        assert_eq!(theme.palette.bg, Rgb(0xf8, 0xf8, 0xf8));
        assert_eq!(theme.appearance, Appearance::Light);
    }

    #[test]
    fn system_is_spelled_however_it_is_typed() {
        assert_eq!(resolve("SYSTEM", &dark_terminal()).unwrap().name, "system");
        assert_eq!(
            resolve("  system ", &dark_terminal()).unwrap().name,
            "system"
        );
    }

    #[test]
    fn system_falls_back_when_the_terminal_says_nothing() {
        // Not an error: `screen` swallows the query outright, and a reader who
        // asked for a document should still get one.
        assert_eq!(resolve("system", &silent()).unwrap().name, "slate");
        assert_eq!(
            resolve(
                "system",
                &answered(Rgb(0xff, 0xff, 0xff), Rgb(0xff, 0xff, 0xff))
            )
            .unwrap()
            .name,
            "paper",
            "an unusable answer still follows the background it did give"
        );
    }

    #[test]
    fn notty_yields_a_plain_theme() {
        let theme = resolve("notty", &silent()).unwrap();
        assert!(theme.plain);
    }

    #[test]
    fn an_unknown_name_lists_what_is_available() {
        let err = resolve("dracula", &silent()).unwrap_err().to_string();
        assert!(err.contains("paper"), "{err}");
        assert!(err.contains("slate"), "{err}");
        assert!(err.contains("system"), "{err}");
    }

    #[test]
    fn a_theme_file_path_is_loaded_directly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mine.toml");
        let file = ThemeVariant::Paper.definition();
        let mut file = file;
        file.name = "mine".to_owned();
        std::fs::write(&path, toml::to_string(&file).unwrap()).expect("write");

        let theme = resolve(path.to_str().unwrap(), &silent()).expect("load");
        assert_eq!(theme.name, "mine");
        // A file theme is a first-class theme, not a degraded one.
        assert_eq!(theme.palette, super::super::PAPER);
        assert!(!theme.plain);
    }

    #[test]
    fn a_malformed_theme_file_reports_the_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("broken.toml");
        std::fs::write(&path, "name = 3").expect("write");
        let err = resolve(path.to_str().unwrap(), &silent())
            .unwrap_err()
            .to_string();
        assert!(err.contains("broken.toml"), "{err}");
    }

    #[test]
    fn built_ins_are_always_listed() {
        let names: Vec<String> = list().into_iter().map(|e| e.name).collect();
        assert!(names.contains(&"paper".to_owned()));
        assert!(names.contains(&"slate".to_owned()));
    }

    #[test]
    fn system_is_listed_whether_or_not_the_terminal_would_answer() {
        // The list is read with no terminal in reach, so it cannot depend on
        // one; `themes` down a pipe has to say what `themes` on a screen says.
        let entry = list()
            .into_iter()
            .find(|entry| entry.name == SYSTEM)
            .expect("system is selectable");
        assert_eq!(entry.origin, Origin::Terminal);
    }

    #[test]
    fn every_listed_name_resolves() {
        // The picker previews whatever the list offers, so a name it can show
        // and not resolve would be a row that fails on arrival.
        for entry in list() {
            resolve(&entry.name, &dark_terminal())
                .unwrap_or_else(|error| panic!("{}: {error}", entry.name));
        }
    }
}
