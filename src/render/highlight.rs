//! Syntax highlighting for code blocks.
//!
//! Maps syntect styles directly onto `ratatui::Style` — no ANSI round-trip —
//! and forces the theme's surface background so the syntax theme's own page
//! color never leaks into the code card. Syntax and theme sets are loaded once
//! per process.

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;

use crate::theme::Theme;

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

fn theme_set() -> &'static ThemeSet {
    static SET: OnceLock<ThemeSet> = OnceLock::new();
    SET.get_or_init(ThemeSet::load_defaults)
}

/// Highlight `text`, returning per-source-line styled pieces.
///
/// Unknown or absent languages fall back to unstyled text in the theme's code
/// foreground; output always has exactly one entry per source line.
#[must_use]
pub fn highlight(text: &str, language: Option<&str>, theme: &Theme) -> Vec<Vec<(Style, String)>> {
    let lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.lines().collect()
    };

    let Some(syntax) = language.filter(|l| !l.is_empty()).and_then(|l| {
        let set = syntax_set();
        set.find_syntax_by_token(l)
            .or_else(|| set.find_syntax_by_extension(l))
    }) else {
        let plain = theme.code_fill();
        return lines
            .iter()
            .map(|l| vec![(plain, (*l).to_owned())])
            .collect();
    };

    let syntect_theme = &theme_set().themes[theme.syntax_theme_name()];
    let mut highlighter = HighlightLines::new(syntax, syntect_theme);
    let surface = theme.palette.surface.color();

    lines
        .iter()
        .map(|line| {
            // syntect wants the newline for stateful parsing.
            let with_newline = format!("{line}\n");
            match highlighter.highlight_line(&with_newline, syntax_set()) {
                Ok(ranges) => ranges
                    .into_iter()
                    .map(|(style, piece)| {
                        (
                            convert(style, surface),
                            piece.trim_end_matches('\n').to_owned(),
                        )
                    })
                    .filter(|(_, piece)| !piece.is_empty())
                    .collect(),
                Err(_) => vec![(theme.code_fill(), (*line).to_owned())],
            }
        })
        .collect()
}

/// syntect style → ratatui style, with the surface bg forced.
fn convert(style: syntect::highlighting::Style, surface: Color) -> Style {
    let fg = style.foreground;
    let mut out = Style::new().fg(Color::Rgb(fg.r, fg.g, fg.b)).bg(surface);
    if style.font_style.contains(FontStyle::BOLD) {
        out = out.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        out = out.add_modifier(Modifier::ITALIC);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Theme, ThemeVariant};

    fn theme() -> Theme {
        Theme::new(ThemeVariant::Slate)
    }

    #[test]
    fn one_output_entry_per_source_line() {
        let out = highlight("a\nb\nc", Some("rust"), &theme());
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn unknown_language_falls_back_to_plain() {
        let out = highlight("whatever text", Some("no-such-lang"), &theme());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0][0].1, "whatever text");
    }

    #[test]
    fn empty_text_yields_one_empty_line() {
        let out = highlight("", None, &theme());
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn all_spans_carry_the_surface_background() {
        let t = theme();
        let out = highlight("fn main() { let x = 1; }", Some("rust"), &t);
        for (style, _) in out[0].iter() {
            assert_eq!(style.bg, Some(t.palette.surface.color()));
        }
    }

    #[test]
    fn rust_code_gets_more_than_one_color() {
        let out = highlight("fn main() { let x = \"s\"; }", Some("rust"), &theme());
        let distinct: std::collections::HashSet<_> = out[0].iter().map(|(s, _)| s.fg).collect();
        assert!(distinct.len() > 1, "expected varied colors: {distinct:?}");
    }

    #[test]
    fn text_round_trips_exactly() {
        let src = "let a = 1;\n    indented();";
        let out = highlight(src, Some("rust"), &theme());
        let rejoined: Vec<String> = out
            .iter()
            .map(|line| line.iter().map(|(_, t)| t.as_str()).collect())
            .collect();
        assert_eq!(rejoined.join("\n"), src);
    }
}
