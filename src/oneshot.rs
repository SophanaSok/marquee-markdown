//! The non-interactive path: render a document straight to standard output.
//!
//! This is what runs for `marquee-markdown file.md`, and what a pipe or a
//! redirect gets. Styling degrades automatically when the destination is not a
//! terminal, so `… | less` and `… > out.txt` both produce sensible output.

use std::io::Write;

use anyhow::{Context, Result};

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
    /// The `-n` flag: keep the line breaks the author typed.
    pub preserve_new_lines: bool,
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
    pub fn detect(
        requested_width: Option<u16>,
        line_numbers: bool,
        preserve_new_lines: bool,
    ) -> Self {
        let is_terminal = tty::stdout_is_terminal();
        Self {
            requested_width,
            line_numbers,
            preserve_new_lines,
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
            preserve_new_lines: settings.preserve_new_lines,
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

/// Render `source` through the reader's pager.
///
/// The pager inherits the terminal, so the rendering is exactly what it would
/// be without one — colors and centering included, since `less -R` and its
/// kin pass ANSI through.
///
/// # Errors
/// Returns an error when the pager cannot be started. A pager closed early is
/// not an error: that is how a reader says they have seen enough.
pub fn page(source: &Source, theme: &Theme, settings: Settings) -> Result<()> {
    page_with(&pager(), source, theme, settings)
}

/// Render `source` through a named pager.
///
/// # Errors
/// Returns an error when the pager cannot be started.
pub fn page_with(
    pager: &(String, Vec<String>),
    source: &Source,
    theme: &Theme,
    settings: Settings,
) -> Result<()> {
    use std::process::{Command, Stdio};

    let (program, arguments) = pager;
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("cannot run {program}"))?;

    let written = match child.stdin.as_mut() {
        Some(stdin) => render_to(stdin, source, theme, settings),
        None => Ok(()),
    };
    // Close the pipe so the pager knows the document has ended, then wait for
    // the reader to finish with it.
    drop(child.stdin.take());
    child.wait().with_context(|| format!("{program} failed"))?;

    match written {
        // The reader quit the pager before we finished writing, which is a
        // normal way to use one.
        Err(error) if is_broken_pipe(&error) => Ok(()),
        other => other,
    }
}

/// Which pager to use, and its arguments.
#[must_use]
pub fn pager() -> (String, Vec<String>) {
    pager_from(std::env::var("PAGER").ok().as_deref())
}

/// Work out the pager from a `PAGER` setting.
///
/// Pure, so the fallbacks are testable without a library that forbids unsafe
/// code having to reach for the unsafe environment-setting functions.
///
/// `less` needs `-R` or it prints escape sequences as text, which is worse
/// than no color at all.
#[must_use]
pub fn pager_from(setting: Option<&str>) -> (String, Vec<String>) {
    match setting {
        Some(value) if !value.trim().is_empty() => {
            let mut parts = value.split_whitespace().map(str::to_owned);
            let program = parts.next().unwrap_or_else(|| "less".to_owned());
            (program, parts.collect())
        }
        _ => ("less".to_owned(), vec!["-R".to_owned()]),
    }
}

/// Whether an error chain bottoms out in a closed output pipe.
fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::BrokenPipe)
    })
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
            preserve_new_lines: false,
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

    #[test]
    fn the_default_pager_can_show_color() {
        // Without -R, less prints the escape sequences as text.
        assert_eq!(pager_from(None), ("less".to_owned(), vec!["-R".to_owned()]));
    }

    #[test]
    fn a_pager_with_arguments_is_split_into_program_and_arguments() {
        assert_eq!(
            pager_from(Some("less -F -X")),
            ("less".to_owned(), vec!["-F".to_owned(), "-X".to_owned()])
        );
    }

    #[test]
    fn an_empty_pager_setting_falls_back_rather_than_running_nothing() {
        assert_eq!(pager_from(Some("   ")).0, "less");
        assert_eq!(pager_from(Some("")).0, "less");
    }

    #[test]
    fn a_document_can_be_paged_through_a_program_that_reads_it() {
        // `cat` is a pager that never blocks, which is what makes this safe to
        // run in a test.
        let cat = ("cat".to_owned(), Vec::new());
        let result = page_with(&cat, &markdown("# Title\n"), &Theme::plain(), settings());
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn a_pager_that_does_not_exist_says_so() {
        let missing = ("definitely-not-a-real-pager".to_owned(), Vec::new());
        let error =
            page_with(&missing, &markdown("# T\n"), &Theme::plain(), settings()).unwrap_err();
        assert!(error.to_string().contains("definitely-not-a-real-pager"));
    }
}
