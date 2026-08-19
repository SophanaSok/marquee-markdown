//! The `marquee-markdown` binary.
//!
//! A stub over [`marquee_markdown::cli::run`], which the short alias `mmd`
//! shares — so the two names cannot drift apart.

fn main() -> std::process::ExitCode {
    marquee_markdown::cli::run::main()
}
