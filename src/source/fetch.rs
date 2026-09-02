//! Getting a document from the network, behind a trait.
//!
//! [`Fetcher`] is the seam that keeps every remote code path testable with no
//! network at all: `remote.rs` is written against it, and its tests run
//! against [`FakeFetcher`]. A contributor's CI must never need to reach the
//! internet to check a pull request, and a test that does is a test that will
//! eventually fail for reasons nobody can reproduce.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result, bail};

/// How long to wait for a server before giving up.
const TIMEOUT: Duration = Duration::from_secs(20);
/// Largest body we will read. A reader pointed at a multi-gigabyte file should
/// get an error, not an unresponsive terminal and a growing heap.
const MAX_BODY: u64 = 8 * 1024 * 1024;

/// What came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// The body, as text.
    pub body: String,
    /// The URL the body actually came from, after any redirects. Relative
    /// links in the document resolve against this rather than against what was
    /// asked for.
    pub url: String,
    /// The media type from `Content-Type`, lowercased and without parameters.
    pub content_type: Option<String>,
}

/// Something that can fetch a URL.
pub trait Fetcher {
    /// Fetch `url`, optionally asking for a particular media type.
    ///
    /// # Errors
    /// Returns an error when the request fails or the server does not answer
    /// with a success status.
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Fetched>;
}

/// The real thing.
///
/// The HTTP client is built on first use, so constructing one costs nothing on
/// the overwhelmingly common path where the document is a local file.
#[derive(Debug, Default)]
pub struct HttpFetcher {
    client: OnceLock<reqwest::blocking::Client>,
}

impl HttpFetcher {
    /// A fetcher that has not connected to anything yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn client(&self) -> Result<&reqwest::blocking::Client> {
        if let Some(client) = self.client.get() {
            return Ok(client);
        }
        let client = reqwest::blocking::Client::builder()
            // GitHub answers 403 to a request without a user agent, so this is
            // load-bearing rather than politeness.
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .timeout(TIMEOUT)
            .build()
            .context("cannot start the HTTP client")?;
        Ok(self.client.get_or_init(|| client))
    }
}

impl Fetcher for HttpFetcher {
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Fetched> {
        let mut request = self.client()?.get(url);
        if let Some(accept) = accept {
            request = request.header(reqwest::header::ACCEPT, accept);
        }
        let response = request
            .send()
            .with_context(|| format!("cannot reach {url}"))?;

        let status = response.status();
        if !status.is_success() {
            bail!("{url} returned {status}");
        }
        let final_url = response.url().to_string();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(media_type);

        let body = read_capped(response, url)?;

        Ok(Fetched {
            body: crate::source::text::from_bytes(body, url)?,
            url: final_url,
            content_type,
        })
    }
}

/// Read a body of at most [`MAX_BODY`] bytes.
///
/// One byte past the cap is read so that a body of exactly the cap can be
/// told apart from one that was cut off at it; only the latter is an error.
fn read_capped(reader: impl Read, url: &str) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    reader
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)
        .with_context(|| format!("cannot read the body of {url}"))?;
    if body.len() as u64 > MAX_BODY {
        bail!("{url} is larger than {} MiB", MAX_BODY / 1024 / 1024);
    }
    Ok(body)
}

/// The media type from a `Content-Type` header, lowercased, without the
/// charset and other parameters.
#[must_use]
pub fn media_type(header: &str) -> String {
    header
        .split(';')
        .next()
        .unwrap_or(header)
        .trim()
        .to_ascii_lowercase()
}

/// A fetcher with canned answers, for tests.
///
/// Public rather than test-only so integration tests and downstream users can
/// exercise the remote paths without a network.
#[derive(Debug, Default)]
pub struct FakeFetcher {
    pages: HashMap<String, Fetched>,
    requests: RefCell<Vec<(String, Option<String>)>>,
}

impl FakeFetcher {
    /// A fetcher that knows about nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer `url` with `body`.
    #[must_use]
    pub fn with(mut self, url: &str, content_type: &str, body: &str) -> Self {
        self.pages.insert(
            url.to_owned(),
            Fetched {
                body: body.to_owned(),
                url: url.to_owned(),
                content_type: Some(media_type(content_type)),
            },
        );
        self
    }

    /// Answer `url` with a body that came from somewhere else, as a redirect
    /// would.
    #[must_use]
    pub fn redirecting(mut self, url: &str, to: &str, content_type: &str, body: &str) -> Self {
        self.pages.insert(
            url.to_owned(),
            Fetched {
                body: body.to_owned(),
                url: to.to_owned(),
                content_type: Some(media_type(content_type)),
            },
        );
        self
    }

    /// Every request made, in order, with the `Accept` header it carried.
    #[must_use]
    pub fn requests(&self) -> Vec<(String, Option<String>)> {
        self.requests.borrow().clone()
    }
}

impl Fetcher for FakeFetcher {
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Fetched> {
        self.requests
            .borrow_mut()
            .push((url.to_owned(), accept.map(str::to_owned)));
        match self.pages.get(url) {
            Some(page) => Ok(page.clone()),
            None => bail!("{url} returned 404 Not Found"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_of_exactly_the_cap_is_accepted() {
        let body = read_capped(std::io::repeat(b'x').take(MAX_BODY), "https://x/big")
            .expect("a body at the cap");
        assert_eq!(body.len() as u64, MAX_BODY);
    }

    #[test]
    fn a_body_past_the_cap_is_refused() {
        let error = read_capped(std::io::repeat(b'x').take(MAX_BODY + 1), "https://x/big")
            .unwrap_err()
            .to_string();
        assert!(error.contains("larger than 8 MiB"), "{error}");
    }

    #[test]
    fn a_media_type_loses_its_parameters_and_its_case() {
        assert_eq!(media_type("text/markdown; charset=UTF-8"), "text/markdown");
        assert_eq!(media_type("TEXT/HTML"), "text/html");
        assert_eq!(media_type("  text/plain  "), "text/plain");
    }

    #[test]
    fn the_fake_answers_what_it_was_told_and_404s_the_rest() {
        let fetcher = FakeFetcher::new().with("https://x/a.md", "text/markdown", "# Hi");
        let got = fetcher.get("https://x/a.md", None).expect("a page");
        assert_eq!(got.body, "# Hi");
        assert_eq!(got.content_type.as_deref(), Some("text/markdown"));

        let error = fetcher
            .get("https://x/missing", None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("404"), "{error}");
    }

    #[test]
    fn the_fake_records_what_was_asked_for() {
        let fetcher = FakeFetcher::new().with("https://x/a", "text/plain", "body");
        let _ = fetcher.get("https://x/a", Some("application/json"));
        let _ = fetcher.get("https://x/b", None);
        assert_eq!(
            fetcher.requests(),
            vec![
                (
                    "https://x/a".to_owned(),
                    Some("application/json".to_owned())
                ),
                ("https://x/b".to_owned(), None),
            ]
        );
    }

    #[test]
    fn a_redirect_reports_where_the_body_actually_came_from() {
        let fetcher = FakeFetcher::new().redirecting(
            "https://x/short",
            "https://y/long/doc.md",
            "text/markdown",
            "# Hi",
        );
        let got = fetcher.get("https://x/short", None).expect("a page");
        assert_eq!(got.url, "https://y/long/doc.md");
    }
}
