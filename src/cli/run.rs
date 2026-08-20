//! Running the program: everything the binaries do.
//!
//! The logic lives here rather than in `main.rs` so the two binaries —
//! `marquee-markdown` and its short alias `mmd` — are stubs over one
//! implementation rather than two copies of it.

use std::io::Write;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};

use super::{Cli, Command, RunMode};
use crate::config::Config;
use crate::source::{self, HttpFetcher, RealFs};
use crate::theme::registry;
use crate::{app, cli, oneshot, update_check, util};

/// The whole program. Returns the process exit code.
///
/// A downstream reader closing early (`… | head`) is how a filter is normally
/// told to stop, not a failure worth reporting.
#[must_use]
pub fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) if is_broken_pipe(&error) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// The command definition, named after however this binary was invoked.
///
/// `marquee-markdown` and `mmd` are one program under two names. Completions
/// registered for a name the reader does not type simply never fire, and a man
/// page titled with the other name is filed in the wrong place — so both
/// describe whichever name was actually run.
fn invoked_command() -> clap::Command {
    // `clap::builder::Str` is cheap to make from a leaked &'static str, and
    // this runs once per invocation of a subcommand that is about to print a
    // whole man page.
    let name: &'static str = String::leak(program_name(std::env::args_os().next().as_deref()));
    Cli::command().name(name).bin_name(name)
}

/// The program name from an `argv[0]`, without its directory or extension.
#[must_use]
pub fn program_name(argv0: Option<&std::ffi::OsStr>) -> String {
    argv0
        .map(std::path::Path::new)
        .and_then(std::path::Path::file_stem)
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_NAME").to_owned())
}

/// Whether an error chain bottoms out in a closed output pipe.
fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::BrokenPipe)
    })
}

/// Parse the command line and do what it says.
///
/// # Errors
/// Returns whatever went wrong, for `main` to report.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    cli.validate()?;

    if let Some(command) = &cli.command {
        return run_command(command, &cli);
    }

    let config = load_config(&cli)?;
    // Warnings go to standard error before anything takes over the screen, so
    // they are still there after the reader quits.
    for warning in &config.warnings {
        eprintln!("warning: {warning}");
    }

    // Decided before the screen is taken, mentioned after it is given back,
    // so the notice is the last thing left in the scrollback.
    let notice = update_check::check(
        config.update_check,
        &program_name(std::env::args_os().next().as_deref()),
    );

    let spec = source::classify(cli.source.as_deref(), util::tty::stdin_is_pipe(), &RealFs);
    let stdout_is_tty = util::tty::stdout_is_terminal();
    let mode = cli::run_mode(&cli, &spec, stdout_is_tty);

    // Redirected output gets no styling, matching glow.
    let theme = if stdout_is_tty {
        registry::resolve(&config.style, None)?
    } else {
        crate::theme::Theme::plain()
    };

    let result = match mode {
        RunMode::OneShot => {
            let source = source::resolve(&spec, &HttpFetcher::new())?;
            let settings = settings(&config);
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            oneshot::render_to(&mut out, &source, &theme, settings)?;
            Ok(())
        }
        RunMode::Tui => {
            let source = source::resolve(&spec, &HttpFetcher::new())?;
            app::run(source, theme, options(&config), config.keymap)
        }
        RunMode::Browser => {
            let root = match &spec {
                source::SourceSpec::Dir(path) => path.clone(),
                _ => std::env::current_dir()?,
            };
            app::browse(root, theme, options(&config), config.keymap)
        }
        RunMode::Pager => {
            let source = source::resolve(&spec, &HttpFetcher::new())?;
            oneshot::page(&source, &theme, settings(&config))
        }
    };

    if result.is_ok()
        && let Some(notice) = notice
    {
        eprintln!("{notice}");
    }
    result
}

/// Resolve the configuration: the command line over the environment over a
/// file over the defaults.
fn load_config(cli: &Cli) -> Result<Config> {
    Config::load(cli.layer(), cli.config.as_deref(), &|name| {
        std::env::var(name).ok()
    })
}

/// The one-shot renderer's settings.
fn settings(config: &Config) -> oneshot::Settings {
    oneshot::Settings::detect(config.width, config.line_numbers, config.preserve_new_lines)
}

/// The settings the reader cares about.
fn options(config: &Config) -> app::Options {
    app::Options {
        width: config.width,
        line_numbers: config.line_numbers,
        mouse: config.mouse,
        all: config.all,
        preserve_new_lines: config.preserve_new_lines,
        contents: config.contents,
    }
}

/// Run a subcommand.
///
/// Everything is built in memory and written once through a `Write`, rather
/// than printed. `print!` panics when the pipe closes, and `clap_complete`
/// panics internally on a write error — so `… completion bash | head` aborted
/// with a backtrace instead of stopping quietly, which is what closing a pipe
/// is supposed to mean.
fn run_command(command: &Command, cli: &Cli) -> Result<()> {
    let text = match command {
        Command::Config => {
            let config = load_config(cli)?;
            for warning in &config.warnings {
                eprintln!("warning: {warning}");
            }
            config.to_toml()
        }
        Command::Keys => {
            let config = load_config(cli)?;
            crate::config::keys::reference(&config.keymap)
        }
        Command::Themes => registry::list()
            .into_iter()
            .map(|entry| {
                let origin = match &entry.origin {
                    registry::Origin::BuiltIn => "built-in".to_owned(),
                    registry::Origin::User(path) => path.display().to_string(),
                };
                format!("{:<12} {origin}\n", entry.name)
            })
            .collect(),
        Command::Man => {
            let mut rendered = Vec::new();
            clap_mangen::Man::new(invoked_command()).render(&mut rendered)?;
            String::from_utf8(rendered).context("the man page was not valid UTF-8")?
        }
        Command::Completion { shell } => {
            let mut rendered = Vec::new();
            let mut command = invoked_command();
            let name = command.get_name().to_owned();
            // Writing into a buffer cannot fail, which is the only way to keep
            // this out of the panic path.
            clap_complete::generate(*shell, &mut command, name, &mut rendered);
            String::from_utf8(rendered).context("the completion script was not valid UTF-8")?
        }
    };

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    out.write_all(text.as_bytes())?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn the_program_name_loses_its_directory_and_extension() {
        assert_eq!(program_name(Some(OsStr::new("/usr/bin/mmd"))), "mmd");
        assert_eq!(
            program_name(Some(OsStr::new("target/release/marquee-markdown"))),
            "marquee-markdown"
        );
        assert_eq!(program_name(Some(OsStr::new("mmd.exe"))), "mmd");
    }

    #[test]
    #[cfg(windows)]
    fn a_windows_path_is_split_on_a_windows_separator() {
        // Only Windows parses a backslash as a separator, so this case cannot
        // be asserted anywhere else.
        assert_eq!(program_name(Some(OsStr::new("C:\\bin\\mmd.exe"))), "mmd");
    }

    #[test]
    fn a_missing_or_empty_argv0_falls_back_to_the_package_name() {
        assert_eq!(program_name(None), "marquee-markdown");
        assert_eq!(program_name(Some(OsStr::new(""))), "marquee-markdown");
    }
}
