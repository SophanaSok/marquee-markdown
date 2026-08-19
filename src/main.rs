//! Binary entry point.

use anyhow::Result;
use clap::{CommandFactory, Parser};

use marquee_markdown::app;
use marquee_markdown::cli::{self, Cli, Command, RunMode};
use marquee_markdown::source::{self, RealFs};
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
        return run_command(command);
    }

    let spec = source::classify(cli.source.as_deref(), util::tty::stdin_is_pipe(), &RealFs);
    let stdout_is_tty = util::tty::stdout_is_terminal();
    let mode = cli::run_mode(&cli, &spec, stdout_is_tty);

    // Redirected output gets no styling, matching glow.
    let theme = if stdout_is_tty {
        registry::resolve(&cli.style, None)?
    } else {
        marquee_markdown::theme::Theme::plain()
    };

    match mode {
        RunMode::OneShot => {
            let source = source::resolve(&spec)?;
            let settings = oneshot::Settings::detect(cli.width, cli.line_numbers);
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            oneshot::render_to(&mut out, &source, &theme, settings)?;
            Ok(())
        }
        RunMode::Tui => {
            let source = source::resolve(&spec)?;
            app::run(
                source,
                theme,
                app::Options {
                    width: cli.width,
                    line_numbers: cli.line_numbers,
                    mouse: cli.mouse,
                },
            )
        }
        RunMode::Pager => anyhow::bail!("--pager is not built yet; run without -p for now"),
        RunMode::Browser => {
            anyhow::bail!("the file browser is not built yet; name a file to read for now")
        }
    }
}

fn run_command(command: &Command) -> Result<()> {
    match command {
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
