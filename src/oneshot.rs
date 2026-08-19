//! The non-interactive path: render a document straight to standard output.
//!
//! This is what runs for `marquee-markdown file.md`, and what a pipe or a
//! redirect gets. Styling degrades automatically when the destination is not a
//! terminal, so `… | less` and `… > out.txt` both produce sensible output.

use std::io::Write;

use anyhow::Result;

use crate::render::ansi::{self, AnsiOptions};
use crate::render::{self, LayoutOptions};
use crate::source::Source;
use crate::theme::Theme;
use crate::util::{tty, width};

/// Everything the one-shot renderer needs to decide how to draw.
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    /// The `-w` flag, if given.
    pub requested_width: Option<u16>,
    /// The `-l` flag; forced on for source files.
    pub line_numbers: bool,
    /// Whether the destination is a terminal.
    pub is_terminal: bool,
    /// Whether color should be emitted.
    pub color: bool,
    /// Terminal width, when known.
    pub terminal_width: Option<u16>,
}

impl Settings {
    /// Detect settings from the current process environment.
    #[must_use]
    pub fn detect(requested_width: Option<u16>, line_numbers: bool) -> Self {
        let is_terminal = tty::stdout_is_terminal();
        Self {
            requested_width,
            line_numbers,
            is_terminal,
            color: !tty::color_disabled(),
            terminal_width: tty::terminal_width(),
        }
    }
}

/// Render `source` to `out`.
///
/// # Errors
/// Propagates write failures from the destination stream.
pub fn render_to(
    out: &mut dyn Write,
    source: &Source,
    theme: &Theme,
    settings: Settings,
) -> Result<()> {
    let content_width = width::resolve(settings.requested_width, settings.terminal_width);
    // Leave room for gutters when we know how wide the terminal is.
    let content_width = match settings.terminal_width {
        Some(term) if settings.requested_width.is_none() => content_width.min(term),
        _ => content_width,
    };

    let doc = render::render(
        &source.text,
        theme,
        LayoutOptions {
            width: content_width,
            // Source files always get line numbers, matching glow.
            code_line_numbers: settings.line_numbers || source.is_code,
        },
    );

    // Center the column only when drawing to a terminal; a redirect should
    // contain the content and nothing else.
    let gutter = match settings.terminal_width {
        Some(term) if settings.is_terminal => term.saturating_sub(doc.width) / 2,
        _ => 0,
    };

    let color = settings.color && !theme.plain;
    ansi::write(
        out,
        &doc,
        theme,
        AnsiOptions {
            color,
            // Hyperlinks are safe to emit even without color; terminals that
            // do not understand OSC 8 ignore it.
            hyperlinks: settings.is_terminal,
            gutter,
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Base;
    use crate::theme::ThemeVariant;

    fn settings() -> Settings {
        Settings {
            requested_width: Some(40),
            line_numbers: false,
            is_terminal: false,
            color: false,
            terminal_width: None,
        }
    }

    fn render_str(source: &Source, settings: Settings) -> String {
        let theme = Theme::new(ThemeVariant::Slate);
        let mut buf = Vec::new();
        render_to(&mut buf, source, &theme, settings).expect("render");
        String::from_utf8(buf).expect("utf8")
    }

    fn markdown(text: &str) -> Source {
        Source::from_text(text, Some("doc.md".into()), "doc.md".into(), Base::Cwd)
    }

    #[test]
    fn redirected_output_has_no_escapes_and_no_gutters() {
        let out = render_str(&markdown("# Title\n\nbody\n"), settings());
        assert!(!out.contains('\x1b'), "escapes in redirected output");
        for line in out.lines() {
            assert_eq!(line.chars().count(), 40);
            assert!(!line.starts_with(' ') || line.trim().is_empty());
        }
    }

    #[test]
    fn a_plain_theme_suppresses_color_even_on_a_terminal() {
        let mut s = settings();
        s.is_terminal = true;
        s.color = true;
        let mut buf = Vec::new();
        render_to(&mut buf, &markdown("# T\n"), &Theme::plain(), s).expect("render");
        let out = String::from_utf8(buf).unwrap();
        assert!(!out.contains("\x1b[38"), "color emitted for a plain theme");
    }

    #[test]
    fn source_files_get_line_numbers_without_asking() {
        let code = Source::from_text(
            "let a = 1;\nlet b = 2;\n",
            Some("x.rs".into()),
            "x.rs".into(),
            Base::Cwd,
        );
        assert!(code.is_code);
        let out = render_str(&code, settings());
        assert!(out.contains(" 1 "), "no line numbers:\n{out}");
        assert!(out.contains(" 2 "), "no line numbers:\n{out}");
    }

    #[test]
    fn frontmatter_never_reaches_the_output() {
        let out = render_str(&markdown("---\ntitle: Secret\n---\n# Real\n"), settings());
        assert!(!out.contains("Secret"), "frontmatter rendered:\n{out}");
        assert!(out.contains("Real"));
    }

    #[test]
    fn headings_lose_their_hash_marks() {
        let out = render_str(&markdown("# Title\n"), settings());
        assert!(out.contains("Title"));
        assert!(!out.contains("# Title"), "hash leaked:\n{out}");
    }

    #[test]
    fn an_explicit_width_is_honored_exactly() {
        for width in [20u16, 60, 100] {
            let mut s = settings();
            s.requested_width = Some(width);
            let out = render_str(&markdown("# T\n\nsome body text to wrap\n"), s);
            for line in out.lines() {
                assert_eq!(line.chars().count(), usize::from(width));
            }
        }
    }
}
