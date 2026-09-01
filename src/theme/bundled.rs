//! Community palettes compiled into the binary.
//!
//! These are TOML files under `themes/`, embedded with `include_str!` and
//! parsed by [`Theme::from_toml`] — the same parser a user theme goes through.
//! That is deliberate, and it is the same rule the loader has always followed:
//! a shipped palette must not be a privileged code path, or the schema drifts
//! and the first person to contribute a theme discovers it.
//!
//! Adding one is a file and a line in [`ALL`]. `themes_parse` in the tests
//! below fails the build if the file does not parse, and `docs.rs` fails it if
//! the README does not list it.
//!
//! Each palette names one of the seven syntect themes that ship with the
//! highlighter. Only Solarized has an exact counterpart there; the rest are
//! paired by eye with the nearest of `base16-{mocha,ocean,eighties}` and
//! `InspiredGitHub`. Pairing a new palette well is color work, not a config
//! line — see `docs/THEMES.md`.

use anyhow::Result;

use super::Theme;

/// A palette shipped with the reader.
#[derive(Debug, Clone, Copy)]
pub struct Bundled {
    /// Selectable name, matching the file stem under `themes/`.
    pub name: &'static str,
    /// The file's text, embedded at build time.
    pub toml: &'static str,
}

/// Every bundled palette, in the order `themes` lists them.
pub const ALL: &[Bundled] = &[
    Bundled {
        name: "catppuccin-latte",
        toml: include_str!("../../themes/catppuccin-latte.toml"),
    },
    Bundled {
        name: "catppuccin-mocha",
        toml: include_str!("../../themes/catppuccin-mocha.toml"),
    },
    Bundled {
        name: "dracula",
        toml: include_str!("../../themes/dracula.toml"),
    },
    Bundled {
        name: "gruvbox-dark",
        toml: include_str!("../../themes/gruvbox-dark.toml"),
    },
    Bundled {
        name: "nord",
        toml: include_str!("../../themes/nord.toml"),
    },
    Bundled {
        name: "solarized-dark",
        toml: include_str!("../../themes/solarized-dark.toml"),
    },
    Bundled {
        name: "solarized-light",
        toml: include_str!("../../themes/solarized-light.toml"),
    },
    Bundled {
        name: "tokyo-night",
        toml: include_str!("../../themes/tokyo-night.toml"),
    },
];

/// The bundled palette with this name, if there is one.
#[must_use]
pub fn find(name: &str) -> Option<&'static Bundled> {
    ALL.iter().find(|b| b.name == name)
}

/// Load a bundled palette by name.
///
/// # Errors
/// Returns an error when the embedded text does not parse — which the tests
/// below make unreachable in a build that ran them.
pub fn load(bundled: &Bundled) -> Result<Theme> {
    Theme::from_toml(bundled.toml, bundled.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Appearance;

    #[test]
    fn every_bundled_theme_parses() {
        for b in ALL {
            let theme = load(b).unwrap_or_else(|e| panic!("{}: {e:#}", b.name));
            assert_eq!(
                theme.name, b.name,
                "the `name` in {}.toml must match its file stem, or `--style \
                 {}` resolves to a theme calling itself something else",
                b.name, b.name
            );
        }
    }

    #[test]
    fn names_are_unique_and_sorted() {
        // Sorted because `themes` and the picker list them in this order, and
        // unique because `find` takes the first and the loser would still be
        // listed.
        let names: Vec<&str> = ALL.iter().map(|b| b.name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names, sorted, "ALL must be sorted with no duplicates");
    }

    #[test]
    fn the_syntax_theme_named_is_one_the_highlighter_has() {
        // A name syntect does not know panics at the first code block, which
        // is a long way from where the typo is.
        for b in ALL {
            let theme = load(b).expect("parses");
            assert!(
                crate::render::highlight::has_syntax_theme(&theme.syntax),
                "{}: unknown syntax theme {:?}",
                b.name,
                theme.syntax
            );
        }
    }

    #[test]
    fn both_appearances_are_represented() {
        // The light/dark toggle and `system`'s fallback both pick by
        // appearance; a set that is all one thing makes those meaningless.
        assert!(
            ALL.iter()
                .filter_map(|b| load(b).ok())
                .any(|t| t.appearance == Appearance::Light)
        );
        assert!(
            ALL.iter()
                .filter_map(|b| load(b).ok())
                .any(|t| t.appearance == Appearance::Dark)
        );
    }
}
