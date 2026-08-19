//! Binary entry point.

use anyhow::Result;
use clap::{CommandFactory, Parser};

use marquee_markdown::app;
use marquee_markdown::cli::{self, Cli, Command, RunMode};
use marquee_markdown::config::Config;
use marquee_markdown::source::{self, HttpFetcher, RealFs};
use marquee_markdown::theme::registry;
use marquee_markdown::{oneshot, util};

fn main() {
    if let Err(error) = run() {
        // A downstream reader closing early (`… | head`) is how a filter is
        // normally told to stop, not a failure worth reporting.
        if is_broken_pipe(&error) {
            return;
        }
        eprintln!("error: {error:#}");
        std::process::exit(1);
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

fn run() -> Result<()> {
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

    let spec = source::classify(cli.source.as_deref(), util::tty::stdin_is_pipe(), &RealFs);
    let stdout_is_tty = util::tty::stdout_is_terminal();
    let mode = cli::run_mode(&cli, &spec, stdout_is_tty);

    // Redirected output gets no styling, matching glow.
    let theme = if stdout_is_tty {
        registry::resolve(&config.style, None)?
    } else {
        marquee_markdown::theme::Theme::plain()
    };

    match mode {
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
    }
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

fn run_command(command: &Command, cli: &Cli) -> Result<()> {
    match command {
        Command::Config => {
            let config = load_config(cli)?;
            for warning in &config.warnings {
                eprintln!("warning: {warning}");
            }
            print!("{}", config.to_toml());
            Ok(())
        }
        Command::Keys => {
            let config = load_config(cli)?;
            print!(
                "{}",
                marquee_markdown::config::keys::reference(&config.keymap)
            );
            Ok(())
        }
        Command::Themes => {
            for entry in registry::list() {
                let origin = match &entry.origin {
                    registry::Origin::BuiltIn => "built-in".to_owned(),
                    registry::Origin::User(path) => path.display().to_string(),
                };
                println!("{:<12} {origin}", entry.name);
            }
            Ok(())
        }
        Command::Man => {
            let man = clap_mangen::Man::new(Cli::command());
            man.render(&mut std::io::stdout())?;
            Ok(())
        }
        Command::Completion { shell } => {
            let mut command = Cli::command();
            let name = command.get_name().to_owned();
            clap_complete::generate(*shell, &mut command, name, &mut std::io::stdout());
            Ok(())
        }
    }
}
