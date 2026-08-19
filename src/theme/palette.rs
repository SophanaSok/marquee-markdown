//! Palette definitions and the TOML theme-file format.
//!
//! Built-in palettes are compiled in as constants — zero cost and checked at
//! compile time — while the same shape deserializes from a user's theme file,
//! so contributing a theme never requires writing Rust.

use std::fmt;
use std::str::FromStr;

use ratatui::style::Color;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An RGB color that serializes as a `#rrggbb` string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// As a ratatui color.
    #[must_use]
    pub const fn color(self) -> Color {
        Color::Rgb(self.0, self.1, self.2)
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// A color literal that is not a valid `#rrggbb` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadColor(pub String);

impl fmt::Display for BadColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid color {:?} (expected a hex value such as \"#d97757\")",
            self.0
        )
    }
}

impl std::error::Error for BadColor {}

impl FromStr for Rgb {
    type Err = BadColor;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || BadColor(s.to_owned());
        let hex = s.trim().strip_prefix('#').ok_or_else(bad)?;
        // 3-digit shorthand: #abc expands to #aabbcc.
        let expand = |c: u8| c * 17;
        match hex.len() {
            3 => {
                let mut digits = hex.chars().map(|c| c.to_digit(16).map(|d| d as u8));
                let mut next = || digits.next().flatten().ok_or_else(bad);
                Ok(Self(expand(next()?), expand(next()?), expand(next()?)))
            }
            6 => {
                let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| bad());
                Ok(Self(byte(0)?, byte(2)?, byte(4)?))
            }
            _ => Err(bad()),
        }
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

/// Whether a palette is meant for a light or dark terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Light,
    Dark,
}

/// Callout hues for the five GFM alert kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct Alerts {
    pub note: Rgb,
    pub tip: Rgb,
    pub important: Rgb,
    pub warning: Rgb,
    pub caution: Rgb,
}

/// The colors behind a theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct Palette {
    /// Page fill, painted edge to edge including the gutters.
    pub bg: Rgb,
    /// Raised fill for code cards, inline chips, and table headers.
    pub surface: Rgb,
    pub fg: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub accent_soft: Rgb,
    pub border: Rgb,
    pub alerts: Alerts,
}

/// A theme as written in a file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ThemeFile {
    pub name: String,
    pub appearance: Appearance,
    /// Bundled syntect theme used for code blocks.
    pub syntax: String,
    pub palette: Palette,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_six_digit_hex() {
        assert_eq!("#d97757".parse::<Rgb>().unwrap(), Rgb(0xd9, 0x77, 0x57));
    }

    #[test]
    fn parses_three_digit_shorthand() {
        assert_eq!("#fff".parse::<Rgb>().unwrap(), Rgb(255, 255, 255));
        assert_eq!("#08f".parse::<Rgb>().unwrap(), Rgb(0, 0x88, 0xff));
    }

    #[test]
    fn rejects_malformed_colors_with_a_helpful_message() {
        for bad in ["d97757", "#xyzxyz", "#12345", "", "#"] {
            let err = bad.parse::<Rgb>().unwrap_err();
            assert!(err.to_string().contains("expected a hex value"), "{bad:?}");
        }
    }

    #[test]
    fn hex_round_trips() {
        let original = Rgb(0x1f, 0x1e, 0x1d);
        assert_eq!(original.to_string().parse::<Rgb>().unwrap(), original);
    }

    #[test]
    fn a_theme_file_round_trips_through_toml() {
        let file = ThemeFile {
            name: "custom".into(),
            appearance: Appearance::Dark,
            syntax: "base16-eighties.dark".into(),
            palette: super::super::SLATE,
        };
        let text = toml::to_string(&file).expect("serialize");
        let back: ThemeFile = toml::from_str(&text).expect("deserialize");
        assert_eq!(back, file);
    }

    #[test]
    fn a_theme_file_with_a_bad_color_names_the_offending_value() {
        let text = r#"
name = "broken"
appearance = "dark"
syntax = "x"
[palette]
bg = "not-a-color"
"#;
        let err = toml::from_str::<ThemeFile>(text).unwrap_err().to_string();
        assert!(err.contains("not-a-color"), "{err}");
    }
}
