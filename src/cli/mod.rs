//! Command-line interface.
//!
//! Flag names and semantics follow `glow` so muscle memory carries over; the
//! additions (a table-of-contents sidebar, in-document search) live on keys
//! and config rather than new flags.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// A terminal markdown reader that renders documents like Claude artifacts.
#[derive(Debug, Parser)]
#[command(name = "marquee-markdown", version, about, long_about = None)]
pub struct Cli {
    /// File, directory, URL, or `owner/repo` shorthand. `-` reads stdin.
    pub source: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,

    /// Show hidden and git-ignored files when browsing.
    #[arg(short, long)]
    pub all: bool,

    /// Show line numbers.
    #[arg(short = 'l', long)]
    pub line_numbers: bool,

    /// Enable mouse wheel scrolling.
    #[arg(short, long)]
    pub mouse: bool,

    /// Display with a pager.
    #[arg(short, long)]
    pub pager: bool,

    /// Preserve newlines within paragraphs.
    #[arg(short = 'n', long)]
    pub preserve_new_lines: bool,

    /// Style name, or a path to a theme file.
    #[arg(short, long, default_value = "auto")]
    pub style: String,

    /// Display in the full-screen reader.
    #[arg(short, long)]
    pub tui: bool,

    /// Word-wrap width; 0 disables wrapping.
    #[arg(short, long)]
    pub width: Option<u16>,

    /// Path to a configuration file.
    #[arg(long)]
    pub config: Option<PathBuf>,
}

/// Subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// List the available styles and where they came from.
    Themes,
    /// Print a man page to standard output.
    Man,
    /// Print a shell completion script to standard output.
    Completion {
        /// Shell to generate for.
        shell: clap_complete::Shell,
    },
}

/// How the program should run, once flags and the environment are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Render once to standard output.
    OneShot,
    /// Render once, through the user's pager.
    Pager,
    /// Full-screen reader for a single document.
    Tui,
    /// Full-screen file browser.
    Browser,
}

impl Cli {
    /// Validate flag combinations.
    ///
    /// # Errors
    /// Returns an error when mutually exclusive flags are combined.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.pager && self.tui {
            anyhow::bail!("cannot use both --pager and --tui");
        }
        Ok(())
    }
}

/// Decide how to run.
///
/// Mirrors glow: with no argument and a terminal on stdin the browser opens;
/// a directory argument browses that directory; everything else renders once
/// unless `--tui` or `--pager` says otherwise.
#[must_use]
pub fn run_mode(cli: &Cli, spec: &crate::source::SourceSpec, stdout_is_tty: bool) -> RunMode {
    use crate::source::SourceSpec;
    if cli.pager {
        return RunMode::Pager;
    }
    if cli.tui {
        return RunMode::Tui;
    }
    match spec {
        SourceSpec::BrowseCwd | SourceSpec::Dir(_) if stdout_is_tty => RunMode::Browser,
        _ => RunMode::OneShot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceSpec;
    use clap::CommandFactory;

    fn cli_of(args: &[&str]) -> Cli {
        Cli::parse_from(std::iter::once("marquee-markdown").chain(args.iter().copied()))
    }

    #[test]
    fn the_command_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_glow_flag_is_accepted_in_short_form() {
        let cli = cli_of(&["-a", "-l", "-m", "-n", "-w", "72", "-s", "paper", "x.md"]);
        assert!(cli.all && cli.line_numbers && cli.mouse && cli.preserve_new_lines);
        assert_eq!(cli.width, Some(72));
        assert_eq!(cli.style, "paper");
        assert_eq!(cli.source.as_deref(), Some("x.md"));
    }

    #[test]
    fn style_defaults_to_auto() {
        assert_eq!(cli_of(&[]).style, "auto");
    }

    #[test]
    fn pager_and_tui_together_is_rejected() {
        let err = cli_of(&["-p", "-t"]).validate().unwrap_err().to_string();
        assert!(err.contains("both"), "{err}");
    }

    #[test]
    fn each_alone_is_accepted() {
        assert!(cli_of(&["-p"]).validate().is_ok());
        assert!(cli_of(&["-t"]).validate().is_ok());
    }

    #[test]
    fn no_argument_on_a_terminal_opens_the_browser() {
        let cli = cli_of(&[]);
        assert_eq!(
            run_mode(&cli, &SourceSpec::BrowseCwd, true),
            RunMode::Browser
        );
    }

    #[test]
    fn a_directory_argument_opens_the_browser() {
        let cli = cli_of(&["docs"]);
        assert_eq!(
            run_mode(&cli, &SourceSpec::Dir("docs".into()), true),
            RunMode::Browser
        );
    }

    #[test]
    fn redirected_output_never_opens_a_full_screen_view() {
        let cli = cli_of(&[]);
        assert_eq!(
            run_mode(&cli, &SourceSpec::BrowseCwd, false),
            RunMode::OneShot
        );
    }

    #[test]
    fn a_file_argument_renders_once() {
        let cli = cli_of(&["x.md"]);
        assert_eq!(
            run_mode(&cli, &SourceSpec::File("x.md".into()), true),
            RunMode::OneShot
        );
    }

    #[test]
    fn explicit_flags_override_the_default_mode() {
        assert_eq!(
            run_mode(
                &cli_of(&["-t", "x.md"]),
                &SourceSpec::File("x.md".into()),
                true
            ),
            RunMode::Tui
        );
        assert_eq!(
            run_mode(
                &cli_of(&["-p", "docs"]),
                &SourceSpec::Dir("docs".into()),
                true
            ),
            RunMode::Pager
        );
    }
}
