//! Render a markdown file to stdout for visual inspection.
//!
//! Usage: `cargo run --example preview -- [file.md] [width] [paper|slate]`

use std::io::Write;

use marquee_markdown::render::{self, LayoutOptions};
use marquee_markdown::theme::{Theme, ThemeVariant};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args
        .first()
        .cloned()
        .unwrap_or_else(|| "tests/fixtures/kitchen-sink.md".to_owned());
    let width: u16 = args.get(1).and_then(|w| w.parse().ok()).unwrap_or(80);
    let variant: ThemeVariant = args
        .get(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(ThemeVariant::Slate);

    let source = std::fs::read_to_string(&path)?;
    let theme = Theme::new(variant);
    let doc = render::render(
        &source,
        &theme,
        LayoutOptions {
            width,
            code_line_numbers: false,
            preserve_new_lines: false,
        },
    );

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let term_width = crossterm::terminal::size().map(|(w, _)| w).unwrap_or(width);
    let gutter = term_width.saturating_sub(width) / 2;

    for line in &doc.lines {
        write!(out, "{}", sgr(theme.palette.bg.color(), None))?;
        write!(out, "{}", " ".repeat(usize::from(gutter)))?;
        for span in &line.spans {
            write!(out, "{}{}", span_sgr(span), span.content)?;
        }
        write!(out, "{}", sgr(theme.palette.bg.color(), None))?;
        writeln!(out, "{}\x1b[0m", " ".repeat(usize::from(gutter)))?;
    }
    Ok(())
}

fn span_sgr(span: &ratatui::text::Span<'_>) -> String {
    use ratatui::style::Modifier;
    let mut out = String::from("\x1b[0m");
    out.push_str(&sgr(
        span.style.bg.unwrap_or(ratatui::style::Color::Reset),
        span.style.fg,
    ));
    let m = span.style.add_modifier;
    for (flag, code) in [
        (Modifier::BOLD, "\x1b[1m"),
        (Modifier::ITALIC, "\x1b[3m"),
        (Modifier::UNDERLINED, "\x1b[4m"),
        (Modifier::CROSSED_OUT, "\x1b[9m"),
    ] {
        if m.contains(flag) {
            out.push_str(code);
        }
    }
    out
}

fn sgr(bg: ratatui::style::Color, fg: Option<ratatui::style::Color>) -> String {
    let mut out = String::new();
    if let ratatui::style::Color::Rgb(r, g, b) = bg {
        out.push_str(&format!("\x1b[48;2;{r};{g};{b}m"));
    }
    if let Some(ratatui::style::Color::Rgb(r, g, b)) = fg {
        out.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
    }
    out
}
