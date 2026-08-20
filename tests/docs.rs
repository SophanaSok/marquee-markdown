//! Checks that the documentation still describes the program.
//!
//! Documentation drifts silently, and a key reference that is wrong is worse
//! than none. These assert on presence rather than on formatting, so they
//! catch an undocumented addition without objecting to prose edits.

use marquee_markdown::app::keymap::{Keymap, Mode};

#[test]
fn the_readme_documents_every_key_the_reader_binds() {
    let readme = include_str!("../README.md");
    let keymap = Keymap::defaults();
    for mode in [Mode::Document, Mode::Browser, Mode::Toc] {
        for (chord, action) in keymap.bindings(mode) {
            assert!(
                readme.contains(&format!("`{chord}`")),
                "`{chord}` ({action}) is bound in {mode} mode but not in the README"
            );
        }
    }
}

#[test]
fn the_readme_pins_no_version_that_could_go_stale() {
    // This drifted twice: prose announcing "0.1.0 is out" while the crate was
    // at 0.2.1, and install examples naming a file only one release ever had.
    // The crates.io badge already renders the current version live, so nothing
    // in the prose needs to repeat it. The MSRV is exempt — it is a floor the
    // release process does not move.
    let readme = include_str!("../README.md");
    let allowed = ["1.88", "3.0.0"]; // MSRV, and the glow release compared against
    for (number, line) in readme.lines().enumerate() {
        for word in line.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
            let digits: Vec<&str> = word.split('.').collect();
            let is_version = digits.len() == 3
                && digits
                    .iter()
                    .all(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
            assert!(
                !is_version || allowed.contains(&word),
                "README line {} pins version {word:?}, which will go stale:\n  {line}",
                number + 1
            );
        }
    }
}

#[test]
fn the_readme_spells_keys_the_way_a_config_file_will() {
    // The tables double as a reference for `[keys.*]`, so every key they name
    // has to parse as a chord.
    let mut checked = 0;
    for table in key_tables() {
        for row in table.lines().filter(|row| row.starts_with('|')) {
            let column = row.split('|').nth(1).expect("a key column");
            for cell in column.split('`').skip(1).step_by(2) {
                cell.parse::<marquee_markdown::app::keymap::Chord>()
                    .unwrap_or_else(|error| panic!("{error} (from a README key table)"));
                checked += 1;
            }
        }
    }
    assert!(checked > 20, "only {checked} keys found in the tables");
}

/// The body of every key table in the README.
fn key_tables() -> Vec<&'static str> {
    let readme = include_str!("../README.md");
    let tables: Vec<_> = readme
        .split("| Key | |")
        .skip(1)
        .map(|rest| rest.split("\n\n").next().expect("the table ends"))
        .collect();
    assert!(!tables.is_empty(), "the README has no key table");
    tables
}

#[test]
#[cfg_attr(
    not(unix),
    ignore = "the checked-in reference is generated on unix, where ctrl+z exists"
)]
fn the_keybindings_reference_is_current() {
    // Generated rather than written, for the same reason the help overlay is:
    // a key reference that has drifted is worse than none at all.
    let checked_in = include_str!("../docs/KEYBINDINGS.md");
    let generated = marquee_markdown::config::keys::reference(&Keymap::defaults());
    assert_eq!(
        checked_in, generated,
        "docs/KEYBINDINGS.md is out of date — regenerate it with \
         `cargo run -- keys > docs/KEYBINDINGS.md`"
    );
}

#[test]
fn every_relative_link_in_the_readme_resolves() {
    // A renamed document or a deleted file breaks these silently otherwise —
    // on GitHub, on crates.io, and in every packaged copy of this README.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for target in relative_targets() {
        // `packaging/` is excluded from the published crate, so inside the
        // package its absence is expected rather than a broken link.
        if target.starts_with("packaging") && !root.join("packaging").exists() {
            continue;
        }
        assert!(
            root.join(&target).exists(),
            "the README links to {target:?}, which does not exist"
        );
    }
}

/// Every link or image target in the README that is a path rather than a URL
/// or an anchor.
fn relative_targets() -> Vec<String> {
    let readme = include_str!("../README.md");
    let mut targets = Vec::new();
    for (open, close) in [("](", ')'), ("href=\"", '"'), ("src=\"", '"')] {
        for rest in readme.split(open).skip(1) {
            let target = rest.split(close).next().expect("the target ends");
            if target.starts_with("http") || target.starts_with('#') || target.is_empty() {
                continue;
            }
            targets.push(target.to_owned());
        }
    }
    assert!(
        !targets.is_empty(),
        "the README has no relative links at all"
    );
    targets
}

#[test]
fn readme_images_are_absolute_and_point_at_real_files() {
    // crates.io renders the README too, and rewrites relative image paths
    // against the repository — a path that has worked on GitHub and broken on
    // crates.io more than once. Absolute raw URLs render identically on both,
    // and each must name a file that is actually in the tree, or a rename
    // would only be noticed on the crates.io page after a release.
    let readme = include_str!("../README.md");
    let raw = "https://raw.githubusercontent.com/SophanaSok/marquee-markdown/main/";
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut repository_images = 0;
    for rest in readme.split("src=\"").skip(1) {
        let target = rest.split('"').next().expect("the target ends");
        assert!(
            target.starts_with("https://"),
            "image {target:?} is not an absolute URL, so crates.io would rewrite it"
        );
        if let Some(path) = target.strip_prefix(raw) {
            assert!(
                root.join(path).exists(),
                "image {target:?} points at nothing in the tree"
            );
            repository_images += 1;
        }
    }
    assert!(
        repository_images >= 5,
        "only {repository_images} repository images found in the README"
    );
}

#[test]
fn the_scoop_manifest_is_internally_consistent() {
    // The manifest pins a *released* artifact, so it lags `Cargo.toml` by
    // design: between a version bump and the release that carries it, there is
    // no archive to point at and no checksum to name. What can be checked
    // without a network is that its three version-bearing fields agree — a
    // manifest whose version was moved and whose url was not installs the old
    // release under the new name. Whether it names the *latest* release is
    // checked live, in `tests/network.rs`.
    //
    // `packaging/` is excluded from the published crate, so inside the package
    // there is nothing to check.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("packaging/scoop/marquee-markdown.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    let version = manifest["version"].as_str().expect("a version field");
    let arch = &manifest["architecture"]["64bit"];
    for field in ["url", "extract_dir"] {
        let value = arch[field].as_str().unwrap_or_else(|| panic!("a {field}"));
        assert!(
            value.contains(&format!("v{version}")),
            "the scoop manifest says version {version} but its {field} says {value} — \
             move them together"
        );
    }
    let hash = arch["hash"].as_str().expect("a hash field");
    assert_eq!(hash.len(), 64, "the pinned hash is not a sha256: {hash}");
}
