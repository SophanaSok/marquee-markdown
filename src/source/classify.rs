//! Pure source classification: what does a command-line argument refer to?
//!
//! Kept free of I/O behind the [`FsProbe`] trait so every branch is unit
//! testable without a filesystem, a network, or a tempdir. Resolution (reading
//! files, fetching URLs) lives in the sibling modules.

use std::path::{Path, PathBuf};

/// Which forge a repository shorthand refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forge {
    GitHub,
    GitLab,
}

impl Forge {
    /// Host as it appears in a bare `host/owner/repo` argument.
    #[must_use]
    pub const fn host(self) -> &'static str {
        match self {
            Self::GitHub => "github.com",
            Self::GitLab => "gitlab.com",
        }
    }

    /// Scheme as it appears in a `scheme://owner/repo` argument.
    #[must_use]
    pub const fn scheme(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::GitLab => "gitlab",
        }
    }
}

/// What the argument denotes, before any I/O happens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceSpec {
    /// Read markdown from standard input.
    Stdin,
    /// A repository's README, resolved through the forge's API.
    Forge {
        forge: Forge,
        owner: String,
        repo: String,
    },
    /// A plain HTTP(S) URL to fetch.
    Url(String),
    /// A directory to search for a README.
    Dir(PathBuf),
    /// A single file to read.
    File(PathBuf),
    /// No argument and no piped input: browse the working directory.
    BrowseCwd,
}

/// Filesystem probing, injected so classification stays pure in tests.
pub trait FsProbe {
    /// Whether `path` names an existing directory.
    fn is_dir(&self, path: &Path) -> bool;
}

/// The real filesystem.
pub struct RealFs;

impl FsProbe for RealFs {
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }
}

/// Classify a positional argument.
///
/// Piped standard input wins over any argument, matching glow: `cat x.md |
/// prog other.md` reads the pipe.
#[must_use]
pub fn classify(arg: Option<&str>, stdin_is_pipe: bool, fs: &dyn FsProbe) -> SourceSpec {
    if stdin_is_pipe {
        return SourceSpec::Stdin;
    }
    let Some(arg) = arg else {
        return SourceSpec::BrowseCwd;
    };
    if arg == "-" {
        return SourceSpec::Stdin;
    }
    for forge in [Forge::GitHub, Forge::GitLab] {
        let scheme = format!("{}://", forge.scheme());
        if let Some(rest) = arg.strip_prefix(&scheme)
            && let Some(spec) = repo_spec(forge, rest)
        {
            return spec;
        }
        // Bare `github.com/owner/repo`, optionally with a www. prefix.
        let bare = arg.strip_prefix("www.").unwrap_or(arg);
        if let Some(rest) = bare.strip_prefix(&format!("{}/", forge.host()))
            && let Some(spec) = repo_spec(forge, rest)
        {
            return spec;
        }
    }
    if arg.starts_with("http://") || arg.starts_with("https://") {
        return SourceSpec::Url(arg.to_owned());
    }
    let path = PathBuf::from(arg);
    if fs.is_dir(&path) {
        return SourceSpec::Dir(path);
    }
    SourceSpec::File(path)
}

/// Parse `owner/repo` (tolerating a trailing slash). Anything else is not a
/// repository shorthand and falls through to the later cases.
fn repo_spec(forge: Forge, rest: &str) -> Option<SourceSpec> {
    let rest = rest.trim_end_matches('/');
    let mut parts = rest.split('/');
    let owner = parts.next().filter(|s| !s.is_empty())?;
    let repo = parts.next().filter(|s| !s.is_empty())?;
    if parts.next().is_some() {
        return None; // deeper paths are not repo shorthands
    }
    Some(SourceSpec::Forge {
        forge,
        owner: owner.to_owned(),
        repo: repo.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A probe that treats a fixed set of paths as directories.
    struct FakeFs(&'static [&'static str]);

    impl FsProbe for FakeFs {
        fn is_dir(&self, path: &Path) -> bool {
            self.0.iter().any(|d| Path::new(d) == path)
        }
    }

    const NO_DIRS: FakeFs = FakeFs(&[]);

    fn classify_arg(arg: &str) -> SourceSpec {
        classify(Some(arg), false, &NO_DIRS)
    }

    #[test]
    fn piped_stdin_beats_any_argument() {
        assert_eq!(
            classify(Some("README.md"), true, &NO_DIRS),
            SourceSpec::Stdin
        );
        assert_eq!(classify(None, true, &NO_DIRS), SourceSpec::Stdin);
    }

    #[test]
    fn dash_means_stdin() {
        assert_eq!(classify_arg("-"), SourceSpec::Stdin);
    }

    #[test]
    fn no_argument_browses_the_working_directory() {
        assert_eq!(classify(None, false, &NO_DIRS), SourceSpec::BrowseCwd);
    }

    #[test]
    fn forge_schemes_resolve_to_repositories() {
        assert_eq!(
            classify_arg("github://charmbracelet/glow"),
            SourceSpec::Forge {
                forge: Forge::GitHub,
                owner: "charmbracelet".into(),
                repo: "glow".into()
            }
        );
        assert_eq!(
            classify_arg("gitlab://inkscape/inkscape"),
            SourceSpec::Forge {
                forge: Forge::GitLab,
                owner: "inkscape".into(),
                repo: "inkscape".into()
            }
        );
    }

    #[test]
    fn bare_forge_hosts_resolve_to_repositories() {
        assert_eq!(
            classify_arg("github.com/charmbracelet/glow"),
            SourceSpec::Forge {
                forge: Forge::GitHub,
                owner: "charmbracelet".into(),
                repo: "glow".into()
            }
        );
        assert_eq!(
            classify_arg("www.gitlab.com/foo/bar"),
            SourceSpec::Forge {
                forge: Forge::GitLab,
                owner: "foo".into(),
                repo: "bar".into()
            }
        );
    }

    #[test]
    fn trailing_slash_on_a_repo_is_tolerated() {
        assert_eq!(
            classify_arg("github.com/owner/repo/"),
            SourceSpec::Forge {
                forge: Forge::GitHub,
                owner: "owner".into(),
                repo: "repo".into()
            }
        );
    }

    #[test]
    fn deep_forge_paths_are_not_repo_shorthands() {
        // A path into a repo is a plain file path, not a README lookup.
        assert_eq!(
            classify_arg("github.com/owner/repo/blob/main/x.md"),
            SourceSpec::File("github.com/owner/repo/blob/main/x.md".into())
        );
    }

    #[test]
    fn full_https_forge_urls_are_fetched_literally() {
        // glow's behavior: only the bare form triggers the README API.
        assert_eq!(
            classify_arg("https://github.com/owner/repo"),
            SourceSpec::Url("https://github.com/owner/repo".into())
        );
    }

    #[test]
    fn http_and_https_urls_are_urls() {
        assert_eq!(
            classify_arg("https://example.com/a.md"),
            SourceSpec::Url("https://example.com/a.md".into())
        );
        assert_eq!(
            classify_arg("http://example.com/a.md"),
            SourceSpec::Url("http://example.com/a.md".into())
        );
    }

    #[test]
    fn directories_are_directories_and_everything_else_is_a_file() {
        let fs = FakeFs(&["docs", "."]);
        assert_eq!(
            classify(Some("docs"), false, &fs),
            SourceSpec::Dir("docs".into())
        );
        assert_eq!(classify(Some("."), false, &fs), SourceSpec::Dir(".".into()));
        assert_eq!(
            classify(Some("docs/x.md"), false, &fs),
            SourceSpec::File("docs/x.md".into())
        );
    }

    #[test]
    fn incomplete_repo_shorthands_fall_through_to_paths() {
        assert_eq!(
            classify_arg("github.com/owner"),
            SourceSpec::File("github.com/owner".into())
        );
        assert_eq!(
            classify_arg("github://owner"),
            SourceSpec::File("github://owner".into())
        );
    }
}
