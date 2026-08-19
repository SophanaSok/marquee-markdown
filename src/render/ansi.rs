//! Serializing a [`RenderedDoc`] to ANSI bytes for the one-shot stdout path.
//!
//! This is the second consumer of the same layout — the TUI renders into a
//! ratatui buffer, this writes escape codes. Because layout never saw an
//! escape byte, column positions recorded in [`LineMeta::links`](super::doc::LineMeta::links) are exact,
//! which is what lets us emit real OSC 8 hyperlinks without disturbing the
//! painted column. (glow counts those escape bytes as display width, which is
//! why its link-bearing lines come out ragged.)

use std::io::{self, Write};

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

use super::doc::RenderedDoc;
use crate::theme::Theme;

/// How to write a document to a stream.
#[derive(Debug, Clone, Copy)]
pub struct AnsiOptions {
    /// Emit SGR color codes. Off for `NO_COLOR` and redirected output.
    pub color: bool,
    /// Emit OSC 8 hyperlinks around link text.
    pub hyperlinks: bool,
    /// Cells of page background painted either side of the content column.
    pub gutter: u16,
}

impl Default for AnsiOptions {
    fn default() -> Self {
        Self {
            color: true,
            hyperlinks: true,
            gutter: 0,
        }
    }
}

/// Write the document to `out`.
///
/// # Errors
/// Propagates any write error from the underlying stream.
pub fn write(
    out: &mut dyn Write,
    doc: &RenderedDoc,
    theme: &Theme,
    options: AnsiOptions,
) -> io::Result<()> {
    let page = if options.color {
        sgr(&theme.page())
    } else {
        String::new()
    };
    let gutter = " ".repeat(usize::from(options.gutter));

    for (index, line) in doc.lines.iter().enumerate() {
        if options.color {
            out.write_all(page.as_bytes())?;
        }
        out.write_all(gutter.as_bytes())?;
        write_line(out, line, doc, index, options)?;
        if options.color {
            out.write_all(page.as_bytes())?;
        }
        out.write_all(gutter.as_bytes())?;
        if options.color {
            out.write_all(b"\x1b[0m")?;
        }
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn write_line(
    out: &mut dyn Write,
    line: &Line<'_>,
    doc: &RenderedDoc,
    index: usize,
    options: AnsiOptions,
) -> io::Result<()> {
    let links = doc
        .meta
        .get(index)
        .map(|m| m.links.as_slice())
        .unwrap_or(&[]);
    let mut column: u16 = 0;
    let mut open_link: Option<u32> = None;

    for span in &line.spans {
        let width = u16::try_from(super::measure::width(&span.content)).unwrap_or(u16::MAX);
        let span_end = column.saturating_add(width);

        // Open or close a hyperlink at this span's boundary.
        if options.hyperlinks {
            let active = links
                .iter()
                .find(|(range, _)| range.start <= column && column < range.end)
                .map(|(_, idx)| *idx);
            if active != open_link {
                if open_link.is_some() {
                    out.write_all(b"\x1b]8;;\x1b\\")?;
                }
                if let Some(idx) = active {
                    if let Some(dest) = doc.links.get(idx as usize) {
                        write!(out, "\x1b]8;;{dest}\x1b\\")?;
                    }
                }
                open_link = active;
            }
        }

        if options.color {
            out.write_all(sgr(&span.style).as_bytes())?;
        }
        out.write_all(span.content.as_bytes())?;
        column = span_end;
    }

    if open_link.is_some() {
        out.write_all(b"\x1b]8;;\x1b\\")?;
    }
    Ok(())
}

/// Build the SGR sequence for a style, resetting first so attributes from the
/// previous span never leak.
fn sgr(style: &Style) -> String {
    let mut out = String::from("\x1b[0m");
    if let Some(Color::Rgb(r, g, b)) = style.bg {
        out.push_str(&format!("\x1b[48;2;{r};{g};{b}m"));
    }
    if let Some(Color::Rgb(r, g, b)) = style.fg {
        out.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
    }
    for (modifier, code) in [
        (Modifier::BOLD, "\x1b[1m"),
        (Modifier::DIM, "\x1b[2m"),
        (Modifier::ITALIC, "\x1b[3m"),
        (Modifier::UNDERLINED, "\x1b[4m"),
        (Modifier::REVERSED, "\x1b[7m"),
        (Modifier::CROSSED_OUT, "\x1b[9m"),
    ] {
        if style.add_modifier.contains(modifier) {
            out.push_str(code);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{self, LayoutOptions};
    use crate::theme::{Theme, ThemeVariant};

    fn render_to_string(source: &str, options: AnsiOptions) -> String {
        let theme = Theme::new(ThemeVariant::Slate);
        let doc = render::render(
            source,
            &theme,
            LayoutOptions {
                width: 40,
                code_line_numbers: false,
            },
        );
        let mut buf = Vec::new();
        write(&mut buf, &doc, &theme, options).expect("write");
        String::from_utf8(buf).expect("utf8")
    }

    fn plain() -> AnsiOptions {
        AnsiOptions {
            color: false,
            hyperlinks: false,
            gutter: 0,
        }
    }

    #[test]
    fn plain_mode_emits_no_escape_sequences() {
        let out = render_to_string("# Title\n\nSome **bold** text.\n", plain());
        assert!(!out.contains('\x1b'), "escapes leaked: {out:?}");
        assert!(out.contains("Title"));
        assert!(out.contains("bold"));
    }

    #[test]
    fn plain_mode_lines_are_all_the_content_width() {
        let out = render_to_string("# Title\n\nbody text here\n", plain());
        for line in out.lines() {
            assert_eq!(line.chars().count(), 40, "{line:?}");
        }
    }

    #[test]
    fn color_mode_resets_at_end_of_line() {
        let out = render_to_string("text\n", AnsiOptions::default());
        for line in out.lines() {
            assert!(line.ends_with("\x1b[0m"), "no reset: {line:?}");
        }
    }

    #[test]
    fn hyperlinks_are_emitted_and_closed() {
        let out = render_to_string("[label](https://example.com)\n", AnsiOptions::default());
        assert!(
            out.contains("\x1b]8;;https://example.com\x1b\\"),
            "no OSC 8 open"
        );
        assert!(out.contains("\x1b]8;;\x1b\\"), "no OSC 8 close");
    }

    #[test]
    fn hyperlink_escapes_do_not_change_the_visible_width() {
        // The bug this design exists to avoid: escape bytes must not be
        // counted as columns, so a link-bearing line stays exactly as wide as
        // one without.
        let with_link = render_to_string("[label](https://example.com/very/long)\n", plain());
        let without = render_to_string("label\n", plain());
        let width = |s: &str| s.lines().next().unwrap().chars().count();
        assert_eq!(width(&with_link), width(&without));
    }

    #[test]
    fn hyperlinks_can_be_disabled() {
        let out = render_to_string(
            "[label](https://example.com)\n",
            AnsiOptions {
                color: false,
                hyperlinks: false,
                gutter: 0,
            },
        );
        assert!(!out.contains("\x1b]8"), "hyperlink emitted when disabled");
    }

    #[test]
    fn gutters_widen_every_line_symmetrically() {
        let out = render_to_string(
            "text\n",
            AnsiOptions {
                color: false,
                hyperlinks: false,
                gutter: 3,
            },
        );
        for line in out.lines() {
            assert_eq!(line.chars().count(), 46, "{line:?}");
            assert!(line.starts_with("   "));
            assert!(line.ends_with("   "));
        }
    }
}
