//! Live checks against the real forges.
//!
//! Every one of these is `#[ignore]`d: CI must never need the network, and a
//! test that reaches out is a test that will one day fail for reasons nobody
//! can reproduce. Their job is to catch the thing a fake cannot — an API that
//! changed shape since the fakes were written.
//!
//! Run them deliberately:
//!
//! ```sh
//! cargo test --test network -- --ignored
//! ```

use marquee_markdown::source::{self, Forge, HttpFetcher, SourceSpec};

#[test]
#[ignore = "reaches the network"]
fn github_still_answers_the_shape_the_fakes_assume() {
    let spec = SourceSpec::Forge {
        forge: Forge::GitHub,
        owner: "charmbracelet".to_owned(),
        repo: "glow".to_owned(),
    };
    let source = source::resolve(&spec, &HttpFetcher::new()).expect("fetch the glow README");
    assert!(!source.text.is_empty());
    assert_eq!(source.display_name, "charmbracelet/glow");
    assert!(!source.is_code, "the README came back as a code block");
    assert!(
        matches!(&source.base, source::Base::Url(url) if url.starts_with("https://")),
        "{:?}",
        source.base
    );
}

#[test]
#[ignore = "reaches the network"]
fn gitlab_still_answers_the_shape_the_fakes_assume() {
    let spec = SourceSpec::Forge {
        forge: Forge::GitLab,
        owner: "gitlab-org".to_owned(),
        repo: "gitlab".to_owned(),
    };
    let source = source::resolve(&spec, &HttpFetcher::new()).expect("fetch the GitLab README");
    assert!(!source.text.is_empty());
    assert!(!source.is_code, "the README came back as a code block");
}

#[test]
#[ignore = "reaches the network"]
fn a_plain_url_comes_back_as_markdown() {
    let spec = SourceSpec::Url(
        "https://raw.githubusercontent.com/charmbracelet/glow/HEAD/README.md".to_owned(),
    );
    let source = source::resolve(&spec, &HttpFetcher::new()).expect("fetch a raw file");
    assert!(source.text.contains("Glow"));
    assert!(!source.is_code);
}

#[test]
#[ignore = "reaches the network"]
fn a_repository_that_does_not_exist_says_so_without_panicking() {
    let spec = SourceSpec::Forge {
        forge: Forge::GitHub,
        owner: "marquee-markdown".to_owned(),
        repo: "definitely-not-a-real-repository".to_owned(),
    };
    let error = source::resolve(&spec, &HttpFetcher::new())
        .expect_err("a missing repository is an error")
        .to_string();
    assert!(error.contains("README"), "{error}");
}

#[test]
#[ignore = "reaches the network"]
fn crates_io_still_answers_the_shape_the_update_check_assumes() {
    use marquee_markdown::source::Fetcher as _;
    use marquee_markdown::update_check;

    let fetched = HttpFetcher::new()
        .get(update_check::CRATE_URL, Some("application/json"))
        .expect("fetch the crates.io entry");
    let latest = update_check::latest_from_json(&fetched.body)
        .expect("a max_stable_version the parser understands");
    assert_eq!(latest.split('.').count(), 3, "{latest}");
    assert!(
        update_check::is_newer("0.0.0", &latest),
        "every published version is newer than 0.0.0"
    );
}

#[test]
#[ignore = "reaches the network"]
fn the_scoop_manifest_names_the_latest_release() {
    // The offline test can only check the manifest against itself. This is the
    // one that catches it falling behind — it sat at 0.1.0 while 0.2.1 shipped
    // — and that its pinned checksum is the one the release actually published.
    use marquee_markdown::source::Fetcher as _;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("packaging/scoop/marquee-markdown.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("the manifest"))
            .expect("valid JSON");
    let pinned = manifest["version"].as_str().expect("a version");

    let fetcher = HttpFetcher::new();
    let latest = fetcher
        .get(
            "https://api.github.com/repos/SophanaSok/marquee-markdown/releases/latest",
            Some("application/vnd.github+json"),
        )
        .expect("the latest release");
    let release: serde_json::Value = serde_json::from_str(&latest.body).expect("valid JSON");
    let tag = release["tag_name"].as_str().expect("a tag");
    assert_eq!(
        format!("v{pinned}"),
        tag,
        "packaging/scoop/marquee-markdown.json pins {pinned}, but the latest release is {tag} — \
         move its version, url and extract_dir, and take the hash from that release's checksums.txt"
    );

    let checksums = fetcher
        .get(
            &format!(
                "https://github.com/SophanaSok/marquee-markdown/releases/download/{tag}/checksums.txt"
            ),
            None,
        )
        .expect("the checksums");
    let archive = format!("marquee-markdown-{tag}-x86_64-windows.zip");
    let expected = checksums
        .body
        .lines()
        .find(|line| line.ends_with(&archive))
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or_else(|| panic!("{archive} is not in checksums.txt"));
    assert_eq!(
        manifest["architecture"]["64bit"]["hash"]
            .as_str()
            .expect("a hash"),
        expected,
        "the pinned hash is not the one {tag} published for {archive}"
    );
}
