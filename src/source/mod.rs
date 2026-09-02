//! Turning a command-line argument into markdown text to render.

pub mod classify;
pub mod fetch;
pub mod frontmatter;
pub mod kind;
pub mod local;
pub mod remote;
pub mod text;

use std::path::{Path, PathBuf};

use anyhow::Result;

pub use classify::{Forge, FsProbe, RealFs, SourceSpec, classify};
pub use fetch::{Fetcher, HttpFetcher};
pub use kind::FileKind;

/// Where relative links and images in a document resolve against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Base {
    /// The directory containing the source file.
    Dir(PathBuf),
    /// The parent URL of a fetched document.
    Url(String),
    /// The process working directory (used for standard input).
    Cwd,
}

/// A document ready to render.
#[derive(Debug, Clone)]
pub struct Source {
    /// Markdown text, with any frontmatter already removed and non-markdown
    /// files already wrapped in a fence.
    pub text: String,
    /// Frontmatter that was stripped, if any.
    pub frontmatter: Option<String>,
    /// Human-readable name for the status bar.
    pub display_name: String,
    /// Filesystem path, when the document came from one; enables reload,
    /// editing, and file watching.
    pub path: Option<PathBuf>,
    /// Base for resolving relative links.
    pub base: Base,
    /// Whether this was a code file rather than markdown; line numbers are
    /// forced on for those, matching glow.
    pub is_code: bool,
}

impl Source {
    /// Build a source from local file contents, applying the shared
    /// post-processing every path goes through.
    #[must_use]
    pub fn from_text(text: &str, path: Option<PathBuf>, display_name: String, base: Base) -> Self {
        let file_kind = path.as_deref().map_or(FileKind::Markdown, kind::of_path);
        Self::build(text, file_kind, path, display_name, base)
    }

    /// Build a source that did not come from a file, so its kind has already
    /// been worked out from where it did come from.
    #[must_use]
    pub fn from_remote(text: &str, file_kind: FileKind, display_name: String, base: Base) -> Self {
        // No path: a fetched document cannot be reloaded from disk or opened
        // in an editor, and claiming otherwise would be a lie the rest of the
        // application acts on.
        Self::build(text, file_kind, None, display_name, base)
    }

    /// The shared post-processing every source goes through, whatever it came
    /// from: strip frontmatter from markdown, fence anything else.
    fn build(
        text: &str,
        file_kind: FileKind,
        path: Option<PathBuf>,
        display_name: String,
        base: Base,
    ) -> Self {
        match file_kind {
            FileKind::Markdown => {
                let (front, body) = frontmatter::split(text);
                Self {
                    text: body.to_owned(),
                    frontmatter: front.map(str::to_owned),
                    display_name,
                    path,
                    base,
                    is_code: false,
                }
            }
            FileKind::Code { language } => Self {
                text: kind::wrap_as_code(text, &language),
                frontmatter: None,
                display_name,
                path,
                base,
                is_code: true,
            },
        }
    }
}

/// Read the document a specification refers to.
///
/// The fetcher is injected rather than constructed here so the remote paths
/// can be exercised without a network. Local specifications never touch it,
/// and [`HttpFetcher`] does not connect until asked.
///
/// # Errors
/// Returns an error when the document cannot be read or fetched.
pub fn resolve(spec: &SourceSpec, fetcher: &dyn Fetcher) -> Result<Source> {
    match spec {
        SourceSpec::Stdin => {
            let text = local::read_stdin()?;
            let (front, body) = frontmatter::split(&text);
            Ok(Source {
                text: body.to_owned(),
                frontmatter: front.map(str::to_owned),
                display_name: "stdin".to_owned(),
                path: None,
                base: Base::Cwd,
                is_code: false,
            })
        }
        SourceSpec::File(path) => {
            let text = local::read(path)?;
            Ok(Source::from_text(
                &text,
                Some(path.clone()),
                display_name(path),
                base_of(path),
            ))
        }
        SourceSpec::Dir(dir) => {
            let path = local::find_readme(dir)?;
            let text = local::read(&path)?;
            Ok(Source::from_text(
                &text,
                Some(path.clone()),
                display_name(&path),
                base_of(&path),
            ))
        }
        SourceSpec::Url(url) => remote::url(url, fetcher),
        SourceSpec::Forge { forge, owner, repo } => remote::forge(*forge, owner, repo, fetcher),
        SourceSpec::BrowseCwd => {
            anyhow::bail!("no markdown source given")
        }
    }
}

fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    )
}

fn base_of(path: &Path) -> Base {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map_or(Base::Cwd, |p| Base::Dir(p.to_path_buf()))
}

#[cfg(test)]
mod tests {
    /// A fetcher that answers nothing: the local paths must never call it, and
    /// a test that starts to would fail with a 404 rather than reach out.
    fn no_network() -> fetch::FakeFetcher {
        fetch::FakeFetcher::new()
    }

    use super::*;

    #[test]
    fn markdown_files_have_frontmatter_removed() {
        let src = Source::from_text(
            "---\ntitle: T\n---\n# Body\n",
            Some("doc.md".into()),
            "doc.md".into(),
            Base::Cwd,
        );
        assert_eq!(src.text, "# Body\n");
        assert_eq!(src.frontmatter.as_deref(), Some("title: T\n"));
        assert!(!src.is_code);
    }

    #[test]
    fn code_files_are_fenced_and_flagged() {
        let src = Source::from_text(
            "fn main() {}",
            Some("main.rs".into()),
            "main.rs".into(),
            Base::Cwd,
        );
        assert!(src.is_code);
        assert!(src.text.starts_with("```rs\n"));
        // Frontmatter stripping must not apply to source code.
        assert!(src.frontmatter.is_none());
    }

    #[test]
    fn a_code_file_starting_with_dashes_is_not_treated_as_frontmatter() {
        let src = Source::from_text(
            "---\nkey: value\n---\nmore: yaml\n",
            Some("config.yaml".into()),
            "config.yaml".into(),
            Base::Cwd,
        );
        assert!(src.is_code);
        assert!(
            src.text.contains("key: value"),
            "content lost: {}",
            src.text
        );
    }

    #[test]
    fn base_is_the_containing_directory() {
        let src = Source::from_text("x", Some("docs/a.md".into()), "a.md".into(), Base::Cwd);
        let _ = src;
        assert_eq!(base_of(Path::new("docs/a.md")), Base::Dir("docs".into()));
        assert_eq!(base_of(Path::new("a.md")), Base::Cwd);
    }

    #[test]
    fn resolving_a_directory_finds_its_readme() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("README.md"), "# Hello\n").expect("write");
        let src =
            resolve(&SourceSpec::Dir(dir.path().to_path_buf()), &no_network()).expect("resolve");
        assert_eq!(src.text, "# Hello\n");
        assert_eq!(src.display_name, "README.md");
    }

    #[test]
    fn resolving_a_missing_file_reports_the_path() {
        let err = resolve(&SourceSpec::File("no/such/file.md".into()), &no_network())
            .unwrap_err()
            .to_string();
        assert!(err.contains("no/such/file.md"), "{err}");
    }

    #[test]
    fn a_url_goes_through_the_fetcher_and_nowhere_else() {
        let fetcher = fetch::FakeFetcher::new().with(
            "https://example.com/doc.md",
            "text/markdown",
            "# Remote\n",
        );
        let src = resolve(
            &SourceSpec::Url("https://example.com/doc.md".into()),
            &fetcher,
        )
        .expect("resolve");
        assert!(src.text.contains("# Remote"));
        assert_eq!(fetcher.requests().len(), 1);
    }

    #[test]
    fn a_local_source_never_touches_the_fetcher() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("README.md"), "# Local\n").expect("write");
        let fetcher = no_network();
        resolve(&SourceSpec::Dir(dir.path().to_path_buf()), &fetcher).expect("resolve");
        assert!(fetcher.requests().is_empty(), "a local read reached out");
    }
}
