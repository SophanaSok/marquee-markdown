//! Resolving a `--style` argument to a [`Theme`].
//!
//! Selection order: a filesystem path wins, then a user theme in the config
//! directory, then a compiled-in name, then the `auto`/`notty` specials. User
//! themes and built-ins go through the same constructor, so a community theme
//! behaves identically to a shipped one.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

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
pub enum Origin {
    /// Compiled into the binary.
    BuiltIn,
    /// Loaded from the user's theme directory.
    User(PathBuf),
}

/// Every selectable theme, built-ins first.
#[must_use]
pub fn list() -> Vec<Entry> {
    let mut out: Vec<Entry> = ThemeVariant::all()
        .iter()
        .map(|v| Entry {
            name: v.name().to_owned(),
            origin: Origin::BuiltIn,
        })
        .collect();
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

/// Resolve a `--style` value.
///
/// `terminal_is_dark` decides what `auto` means; pass `None` when it cannot be
/// determined, in which case `auto` picks the dark palette (matching glow).
///
/// # Errors
/// Returns an error when the name matches no theme, listing what is available.
pub fn resolve(style: &str, terminal_is_dark: Option<bool>) -> Result<Theme> {
    let style = style.trim();

    // An explicit path always wins, so a theme can be tried without installing.
    let as_path = Path::new(style);
    if style.contains(['/', '\\']) || as_path.extension().is_some_and(|e| e == "toml") {
        return Theme::from_file(as_path);
    }

    match style.to_ascii_lowercase().as_str() {
        "auto" => Ok(Theme::new(match terminal_is_dark {
            Some(false) => ThemeVariant::Paper,
            // Default to the dark palette when unknown, as glow does.
            _ => ThemeVariant::Slate,
        })),
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

/// Whether a theme is meant for a dark terminal.
#[must_use]
pub fn is_dark(theme: &Theme) -> bool {
    theme.appearance == Appearance::Dark
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_names_resolve() {
        assert_eq!(resolve("paper", None).unwrap().name, "paper");
        assert_eq!(resolve("slate", None).unwrap().name, "slate");
    }

    #[test]
    fn light_and_dark_aliases_resolve() {
        assert_eq!(resolve("light", None).unwrap().name, "paper");
        assert_eq!(resolve("dark", None).unwrap().name, "slate");
    }

    #[test]
    fn auto_follows_the_terminal_background() {
        assert_eq!(resolve("auto", Some(false)).unwrap().name, "paper");
        assert_eq!(resolve("auto", Some(true)).unwrap().name, "slate");
    }

    #[test]
    fn auto_defaults_to_dark_when_unknown() {
        assert_eq!(resolve("auto", None).unwrap().name, "slate");
    }

    #[test]
    fn notty_yields_a_plain_theme() {
        let theme = resolve("notty", None).unwrap();
        assert!(theme.plain);
    }

    #[test]
    fn an_unknown_name_lists_what_is_available() {
        let err = resolve("dracula", None).unwrap_err().to_string();
        assert!(err.contains("paper"), "{err}");
        assert!(err.contains("slate"), "{err}");
    }

    #[test]
    fn a_theme_file_path_is_loaded_directly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mine.toml");
        let file = ThemeVariant::Paper.definition();
        let mut file = file;
        file.name = "mine".to_owned();
        std::fs::write(&path, toml::to_string(&file).unwrap()).expect("write");

        let theme = resolve(path.to_str().unwrap(), None).expect("load");
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
        let err = resolve(path.to_str().unwrap(), None)
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
}
