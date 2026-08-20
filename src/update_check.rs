//! Telling the reader a newer release exists.
//!
//! The check never stands between the reader and the document: the notice
//! comes from the answer the *previous* run cached, and a cache past its age
//! is refreshed by a detached background thread whose result only a later run
//! will see. Nothing here blocks startup, rendering, or exit.
//!
//! It also stays quiet unless someone is there to read it — standard error
//! must be a terminal — and says nothing at all under `CI` or when the
//! configuration turned it off. The fetch itself goes through [`Fetcher`], so
//! every decision in this module is tested without a network.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::source::fetch::Fetcher;

/// Where the latest version is asked for.
pub const CRATE_URL: &str = "https://crates.io/api/v1/crates/marquee-markdown";

/// How long a cached answer is trusted before a background refresh.
const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// What the last check learned, and when (seconds since the epoch).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Cache {
    checked_at: u64,
    latest: String,
}

/// The one line to say on the way out, if a newer release is known.
///
/// Reads the cache synchronously and, when it has aged out, starts a detached
/// refresh whose answer only a later run will see. `enabled` is the resolved
/// `update-check` setting; on top of it the check stays silent under `CI` and
/// when standard error is not a terminal, so scripts and builds never see the
/// notice and never cause a request.
#[must_use]
pub fn check(enabled: bool, program: &str) -> Option<String> {
    if !enabled || in_ci() || !crate::util::tty::stderr_is_terminal() {
        return None;
    }
    let path = cache_path()?;
    let cache = read_cache(&path);
    if is_stale(cache.as_ref(), unix_now()) {
        // Detached on purpose: exit must never wait on a slow server. The
        // cache is written atomically, so a thread killed mid-refresh leaves
        // the old answer behind rather than half of a new one.
        let _ = std::thread::Builder::new()
            .name("update-check".to_owned())
            .spawn(move || {
                let _ = refresh(&crate::source::HttpFetcher::new(), &path, unix_now());
            });
    }
    let cache = cache?;
    is_newer(env!("CARGO_PKG_VERSION"), &cache.latest)
        .then(|| notice(program, env!("CARGO_PKG_VERSION"), &cache.latest))
}

/// The newest stable version in a crates.io crate response.
#[must_use]
pub fn latest_from_json(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let latest = value.get("crate")?.get("max_stable_version")?.as_str()?;
    parse_triple(latest).is_some().then(|| latest.to_owned())
}

/// Whether `latest` is a release after `current`.
///
/// A plain `x.y.z` comparison, which is all a `max_stable_version` can be.
/// Anything that does not parse compares as not newer: a malformed answer
/// must never produce a nagging notice.
#[must_use]
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_triple(current), parse_triple(latest)) {
        (Some(current), Some(latest)) => latest > current,
        _ => false,
    }
}

/// `"1.2.3"` as `(1, 2, 3)`.
fn parse_triple(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|part| part.parse::<u64>().ok());
    let triple = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(triple)
}

/// The line itself, named after however the binary was invoked so the advice
/// matches what the reader actually types.
fn notice(program: &str, current: &str, latest: &str) -> String {
    format!(
        "{program} {latest} is available (you have {current}) — upgrade: cargo install marquee-markdown\n  https://github.com/SophanaSok/marquee-markdown/releases"
    )
}

/// Whether this is a CI run, per the convention every provider sets.
fn in_ci() -> bool {
    std::env::var_os("CI").is_some_and(|value| !value.is_empty())
}

/// Where the answer is kept between runs — the first use of the cache
/// directory in this crate. Unlike configuration, losing it costs nothing.
fn cache_path() -> Option<PathBuf> {
    Some(
        dirs::cache_dir()?
            .join("marquee-markdown")
            .join("update-check.json"),
    )
}

/// The cached answer, if there is one and it is readable. Anything wrong with
/// it — absent, truncated, from a different shape — reads as no cache.
fn read_cache(path: &Path) -> Option<Cache> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write the cache atomically: a temporary file beside it, then a rename. The
/// refresh thread dies without ceremony when the process exits, and a
/// half-written file would otherwise be read back as no cache at all.
fn write_cache(path: &Path, cache: &Cache) -> std::io::Result<()> {
    let dir = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string(cache).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(&tmp, path)
}

/// Seconds since the epoch, or zero on a clock set before 1970.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |age| age.as_secs())
}

/// Whether the cache has aged out. A timestamp from the future counts as
/// fresh: a clock that jumped backwards must not cause a request per run.
fn is_stale(cache: Option<&Cache>, now: u64) -> bool {
    match cache {
        Some(cache) => now.saturating_sub(cache.checked_at) >= MAX_AGE.as_secs(),
        None => true,
    }
}

/// Ask crates.io and record the answer. Returns what was learned, mostly so
/// tests can see it; failures are dropped silently — a version check must
/// never produce an error the reader has to care about.
fn refresh(fetcher: &dyn Fetcher, path: &Path, now: u64) -> Option<Cache> {
    let fetched = fetcher.get(CRATE_URL, Some("application/json")).ok()?;
    let latest = latest_from_json(&fetched.body)?;
    let cache = Cache {
        checked_at: now,
        latest,
    };
    write_cache(path, &cache).ok()?;
    Some(cache)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::fetch::FakeFetcher;

    const ANSWER: &str =
        r#"{"crate":{"id":"marquee-markdown","max_stable_version":"9.9.9","max_version":"9.9.9"}}"#;

    #[test]
    fn the_crates_io_answer_yields_its_stable_version() {
        assert_eq!(latest_from_json(ANSWER).as_deref(), Some("9.9.9"));
    }

    #[test]
    fn a_malformed_answer_yields_nothing() {
        for body in [
            "",
            "not json",
            "{}",
            r#"{"crate":{}}"#,
            r#"{"crate":{"max_stable_version":"soon"}}"#,
            r#"{"crate":{"max_stable_version":null}}"#,
        ] {
            assert_eq!(latest_from_json(body), None, "{body:?}");
        }
    }

    #[test]
    fn newer_means_strictly_after_the_current_release() {
        assert!(is_newer("0.2.1", "0.2.2"));
        assert!(is_newer("0.2.1", "0.3.0"));
        assert!(is_newer("0.9.9", "1.0.0"));
        assert!(!is_newer("0.2.1", "0.2.1"));
        assert!(!is_newer("0.2.1", "0.2.0"));
        assert!(!is_newer("0.10.0", "0.9.9"), "numeric, not lexicographic");
    }

    #[test]
    fn nonsense_versions_are_never_newer() {
        for latest in ["", "1.2", "1.2.3.4", "a.b.c", "1.2.x", "1.2.3-rc.1"] {
            assert!(!is_newer("0.2.1", latest), "{latest:?}");
        }
    }

    #[test]
    fn the_cache_round_trips_through_its_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("cache").join("update-check.json");
        let cache = Cache {
            checked_at: 123,
            latest: "1.0.0".to_owned(),
        };
        write_cache(&path, &cache).expect("write");
        assert_eq!(read_cache(&path), Some(cache));
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the temporary survived the rename"
        );
    }

    #[test]
    fn a_missing_or_broken_cache_reads_as_none() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(read_cache(&dir.path().join("absent.json")), None);
        let broken = dir.path().join("broken.json");
        std::fs::write(&broken, "{\"checked").expect("write");
        assert_eq!(read_cache(&broken), None);
    }

    #[test]
    fn staleness_is_a_day_counted_generously() {
        let cache = Cache {
            checked_at: 1_000_000,
            latest: "1.0.0".to_owned(),
        };
        assert!(!is_stale(Some(&cache), 1_000_000 + MAX_AGE.as_secs() - 1));
        assert!(is_stale(Some(&cache), 1_000_000 + MAX_AGE.as_secs()));
        assert!(is_stale(None, 0));
        assert!(
            !is_stale(Some(&cache), 0),
            "a clock that jumped backwards should stay quiet"
        );
    }

    #[test]
    fn a_refresh_records_what_crates_io_said() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("update-check.json");
        let fetcher = FakeFetcher::new().with(CRATE_URL, "application/json", ANSWER);
        let cache = refresh(&fetcher, &path, 42).expect("a refresh");
        assert_eq!(cache.latest, "9.9.9");
        assert_eq!(cache.checked_at, 42);
        assert_eq!(read_cache(&path), Some(cache));
        assert_eq!(fetcher.requests().len(), 1);
    }

    #[test]
    fn a_failed_fetch_leaves_no_cache_behind() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("update-check.json");
        assert_eq!(refresh(&FakeFetcher::new(), &path, 42), None);
        assert!(!path.exists());
    }

    #[test]
    fn the_notice_names_the_binary_that_was_invoked() {
        let line = notice("mmd", "0.2.1", "0.3.0");
        assert!(line.contains("mmd 0.3.0 is available"), "{line}");
        assert!(line.contains("you have 0.2.1"), "{line}");
        assert!(line.contains("cargo install marquee-markdown"), "{line}");
    }
}
