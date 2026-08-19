//! Fetching documents that live somewhere other than this machine.
//!
//! Everything here is written against [`Fetcher`], so all of it — including
//! the two forge APIs and their quirks — is exercised in tests with canned
//! responses and no network.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::classify::Forge;
use super::fetch::{Fetched, Fetcher};
use super::kind::{self, FileKind};
use super::{Base, Source};

/// What we ask a plain URL for. Servers that ignore it are the normal case;
/// the ones that honor it hand back markdown instead of a rendered page.
const ACCEPT_TEXT: &str = "text/markdown, text/plain;q=0.9, */*;q=0.8";

/// Fetch a plain HTTP(S) URL.
///
/// # Errors
/// Propagates transport and status failures from the fetcher.
pub fn url(url: &str, fetcher: &dyn Fetcher) -> Result<Source> {
    let fetched = fetcher.get(url, Some(ACCEPT_TEXT))?;
    Ok(document(&fetched, None))
}

/// Fetch a repository's README through its forge's API.
///
/// # Errors
/// Returns an error when the repository cannot be reached, has no README, or
/// the API answers with something unexpected.
pub fn forge(forge: Forge, owner: &str, repo: &str, fetcher: &dyn Fetcher) -> Result<Source> {
    let raw = match forge {
        Forge::GitHub => github_readme(owner, repo, fetcher),
        Forge::GitLab => gitlab_readme(owner, repo, fetcher),
    }
    .with_context(|| format!("cannot find a README for {}/{owner}/{repo}", forge.host()))?;

    let fetched = fetcher.get(&raw, Some(ACCEPT_TEXT))?;
    // The repository is what the reader asked for, so that is what the status
    // bar should say — not whichever of README.md or readme.rst it turned out
    // to be.
    Ok(document(&fetched, Some(format!("{owner}/{repo}"))))
}

/// Where GitHub says a repository's README can be downloaded from.
fn github_readme(owner: &str, repo: &str, fetcher: &dyn Fetcher) -> Result<String> {
    /// The half of the response we use. GitHub sends a great deal more.
    #[derive(Deserialize)]
    struct Readme {
        download_url: Option<String>,
    }

    let url = format!("https://api.github.com/repos/{owner}/{repo}/readme");
    let fetched = fetcher.get(&url, Some("application/vnd.github+json"))?;
    let readme: Readme =
        serde_json::from_str(&fetched.body).context("GitHub sent something unexpected")?;
    readme
        .download_url
        .context("GitHub reports no downloadable README")
}

/// Where GitLab says a repository's README can be downloaded from.
fn gitlab_readme(owner: &str, repo: &str, fetcher: &dyn Fetcher) -> Result<String> {
    #[derive(Deserialize)]
    struct Project {
        readme_url: Option<String>,
    }

    // Owner and repo are percent-encoded into one path segment; `classify`
    // only produces two segments of forge-legal characters, so `/` is the only
    // one that ever needs escaping.
    let url = format!("https://gitlab.com/api/v4/projects/{owner}%2F{repo}");
    let fetched = fetcher.get(&url, Some("application/json"))?;
    let project: Project =
        serde_json::from_str(&fetched.body).context("GitLab sent something unexpected")?;
    let readme = project
        .readme_url
        .context("GitLab reports no README for this project")?;
    // `readme_url` points at the *page* showing the file. The raw file is the
    // same URL with one path segment changed, and fetching the page instead
    // would render a screenful of HTML.
    Ok(readme.replacen("/-/blob/", "/-/raw/", 1))
}

/// Build a document from what came back.
fn document(fetched: &Fetched, display_name: Option<String>) -> Source {
    let (base, file_name) = split_url(&fetched.url);
    let display_name = display_name
        .or_else(|| file_name.map(str::to_owned))
        .unwrap_or_else(|| fetched.url.clone());
    Source::from_remote(
        &fetched.body,
        remote_kind(file_name, fetched.content_type.as_deref()),
        display_name,
        Base::Url(base),
    )
}

/// How to render what came back.
///
/// The extension in the URL is trusted ahead of the `Content-Type`, because
/// plenty of servers hand out markdown as `text/plain` or even `text/html`,
/// and a `.md` in the path is a stronger statement of intent than a header a
/// server may not have thought about.
fn remote_kind(file_name: Option<&str>, content_type: Option<&str>) -> FileKind {
    if let Some(name) = file_name
        && Path::new(name).extension().is_some()
    {
        return kind::of_path(Path::new(name));
    }
    match content_type {
        // A page with no extension and an HTML type is markup, not prose.
        // Showing it as highlighted source is at least honest about that,
        // where rendering it as markdown produces noise.
        Some("text/html" | "application/xhtml+xml") => FileKind::Code {
            language: "html".to_owned(),
        },
        _ => FileKind::Markdown,
    }
}

/// Split a URL into the directory it lives in and its final segment.
///
/// The directory is what relative links resolve against, and it always ends in
/// a slash so a link can simply be appended.
#[must_use]
pub fn split_url(url: &str) -> (String, Option<&str>) {
    // Skip past `scheme://` so the slashes in it are never mistaken for a path.
    let start = url.find("://").map_or(0, |index| index + 3);
    let path = &url[start..];
    let path = path.split(['?', '#']).next().unwrap_or(path);
    match path.rfind('/') {
        Some(index) => {
            let name = &path[index + 1..];
            (
                url[..start + index + 1].to_owned(),
                (!name.is_empty()).then_some(name),
            )
        }
        // A bare host: everything relative to it hangs off the root.
        None => (format!("{url}/"), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::fetch::FakeFetcher;

    #[test]
    fn a_url_splits_into_a_base_and_a_file_name() {
        assert_eq!(
            split_url("https://x.dev/docs/guide.md"),
            ("https://x.dev/docs/".to_owned(), Some("guide.md"))
        );
        assert_eq!(
            split_url("https://x.dev/docs/"),
            ("https://x.dev/docs/".to_owned(), None)
        );
        assert_eq!(
            split_url("https://x.dev"),
            ("https://x.dev/".to_owned(), None)
        );
        assert_eq!(
            split_url("https://x.dev/a.md?raw=1#top"),
            ("https://x.dev/".to_owned(), Some("a.md"))
        );
    }

    #[test]
    fn a_fetched_markdown_file_renders_as_markdown() {
        let fetcher = FakeFetcher::new().with(
            "https://x.dev/docs/guide.md",
            "text/markdown",
            "# Guide\n\nBody.\n",
        );
        let source = url("https://x.dev/docs/guide.md", &fetcher).expect("a document");
        assert!(!source.is_code);
        assert_eq!(source.display_name, "guide.md");
        assert_eq!(source.base, Base::Url("https://x.dev/docs/".to_owned()));
        assert!(source.path.is_none(), "a URL is not a file to reload");
    }

    #[test]
    fn a_fetched_source_file_renders_as_highlighted_code() {
        let fetcher =
            FakeFetcher::new().with("https://x.dev/main.rs", "text/plain", "fn main() {}");
        let source = url("https://x.dev/main.rs", &fetcher).expect("a document");
        assert!(source.is_code);
        assert!(source.text.contains("```rust") || source.text.contains("```rs"));
    }

    #[test]
    fn an_extension_is_trusted_over_a_careless_content_type() {
        // Plenty of servers hand out markdown as text/plain or text/html.
        let fetcher = FakeFetcher::new().with("https://x.dev/a.md", "text/html", "# Real\n");
        let source = url("https://x.dev/a.md", &fetcher).expect("a document");
        assert!(!source.is_code, "markdown was hidden in a code block");
    }

    #[test]
    fn a_page_with_no_extension_and_an_html_type_is_shown_as_markup() {
        let fetcher = FakeFetcher::new().with("https://x.dev/page", "text/html", "<h1>Hello</h1>");
        let source = url("https://x.dev/page", &fetcher).expect("a document");
        assert!(source.is_code, "a web page was rendered as prose");
        assert!(source.text.contains("```html"));
    }

    #[test]
    fn an_extensionless_plain_document_is_treated_as_markdown() {
        let fetcher = FakeFetcher::new().with("https://x.dev/README", "text/plain", "# Hi\n");
        assert!(!url("https://x.dev/README", &fetcher).unwrap().is_code);
    }

    #[test]
    fn frontmatter_is_stripped_from_a_fetched_document() {
        let fetcher = FakeFetcher::new().with(
            "https://x.dev/a.md",
            "text/markdown",
            "---\ntitle: Secret\n---\n# Real\n",
        );
        let source = url("https://x.dev/a.md", &fetcher).expect("a document");
        assert!(!source.text.contains("Secret"));
        assert_eq!(source.frontmatter.as_deref(), Some("title: Secret\n"));
    }

    #[test]
    fn relative_links_resolve_against_where_the_body_actually_came_from() {
        // After a redirect that is not where we asked.
        let fetcher = FakeFetcher::new().redirecting(
            "https://x.dev/short",
            "https://raw.example/u/r/HEAD/docs/README.md",
            "text/markdown",
            "# Hi\n",
        );
        let source = url("https://x.dev/short", &fetcher).expect("a document");
        assert_eq!(
            source.base,
            Base::Url("https://raw.example/u/r/HEAD/docs/".to_owned())
        );
    }

    #[test]
    fn a_url_that_is_not_there_says_so() {
        let error = url("https://x.dev/gone", &FakeFetcher::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("404"), "{error}");
    }

    fn github_fetcher() -> FakeFetcher {
        FakeFetcher::new()
            .with(
                "https://api.github.com/repos/charmbracelet/glow/readme",
                "application/json",
                r#"{"name":"README.md","download_url":"https://raw.githubusercontent.com/charmbracelet/glow/HEAD/README.md"}"#,
            )
            .with(
                "https://raw.githubusercontent.com/charmbracelet/glow/HEAD/README.md",
                "text/plain",
                "# Glow\n\nRender markdown.\n",
            )
    }

    #[test]
    fn a_github_repository_resolves_through_its_api_to_the_raw_file() {
        let fetcher = github_fetcher();
        let source = forge(Forge::GitHub, "charmbracelet", "glow", &fetcher).expect("a document");
        assert!(source.text.contains("Render markdown."));
        // The repository is what the reader asked for, so that is what is
        // named.
        assert_eq!(source.display_name, "charmbracelet/glow");
        assert_eq!(
            source.base,
            Base::Url("https://raw.githubusercontent.com/charmbracelet/glow/HEAD/".to_owned())
        );
    }

    #[test]
    fn the_github_api_is_asked_for_json_and_the_file_for_text() {
        let fetcher = github_fetcher();
        forge(Forge::GitHub, "charmbracelet", "glow", &fetcher).expect("a document");
        let requests = fetcher.requests();
        assert_eq!(requests.len(), 2, "{requests:?}");
        assert_eq!(
            requests[0].1.as_deref(),
            Some("application/vnd.github+json")
        );
        assert_eq!(requests[1].1.as_deref(), Some(ACCEPT_TEXT));
    }

    #[test]
    fn a_repository_with_no_readme_says_which_repository() {
        let fetcher = FakeFetcher::new().with(
            "https://api.github.com/repos/o/r/readme",
            "application/json",
            r#"{"name":"README.md"}"#,
        );
        let error = forge(Forge::GitHub, "o", "r", &fetcher)
            .unwrap_err()
            .to_string();
        assert!(error.contains("github.com/o/r"), "{error}");
    }

    #[test]
    fn a_gitlab_project_page_url_is_rewritten_to_the_raw_file() {
        // The API hands back a link to the page showing the file; fetching
        // that would render a screenful of HTML.
        let fetcher = FakeFetcher::new()
            .with(
                "https://gitlab.com/api/v4/projects/gitlab-org%2Fgitlab",
                "application/json",
                r#"{"readme_url":"https://gitlab.com/gitlab-org/gitlab/-/blob/master/README.md"}"#,
            )
            .with(
                "https://gitlab.com/gitlab-org/gitlab/-/raw/master/README.md",
                "text/plain",
                "# GitLab\n",
            );
        let source = forge(Forge::GitLab, "gitlab-org", "gitlab", &fetcher).expect("a document");
        assert!(source.text.contains("# GitLab"));
        assert!(
            fetcher.requests()[1].0.contains("/-/raw/"),
            "{:?}",
            fetcher.requests()
        );
    }

    #[test]
    fn a_malformed_api_response_is_reported_rather_than_panicking() {
        let fetcher = FakeFetcher::new().with(
            "https://api.github.com/repos/o/r/readme",
            "application/json",
            "not json at all",
        );
        let error = forge(Forge::GitHub, "o", "r", &fetcher)
            .unwrap_err()
            .to_string();
        assert!(error.contains("README"), "{error}");
    }
}
