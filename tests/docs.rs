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
fn the_scoop_template_fills_in_to_a_manifest_scoop_can_use() {
    // The manifest is built by the release workflow from `checksums.txt`,
    // because a hash cannot exist before the archive it describes. What lives
    // in the repository is the template, so there is no pinned version here to
    // fall behind — which is what happened when there was: it sat at 0.1.0
    // through two releases.
    //
    // This guards the contract the workflow depends on: the placeholders it
    // substitutes, and that substituting them yields the manifest Scoop wants.
    //
    // `packaging/` is excluded from the published crate, so inside the package
    // there is nothing to check.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("packaging/scoop/marquee-markdown.template.json");
    let Ok(template) = std::fs::read_to_string(&path) else {
        return;
    };

    let filled = template
        .replace("@VERSION@", "9.9.9")
        .replace("@HASH@", &"a".repeat(64));
    assert!(
        !filled.contains('@'),
        "a placeholder the release workflow does not substitute survived: {filled}"
    );

    let manifest: serde_json::Value =
        serde_json::from_str(&filled).expect("valid JSON once filled");
    assert_eq!(manifest["version"], "9.9.9");
    let arch = &manifest["architecture"]["64bit"];
    for field in ["url", "extract_dir"] {
        let value = arch[field].as_str().unwrap_or_else(|| panic!("a {field}"));
        assert!(
            value.contains("v9.9.9"),
            "{field} does not carry the version: {value}"
        );
    }
    assert_eq!(arch["hash"].as_str().expect("a hash").len(), 64);

    // Scoop's own `$version` in the autoupdate block is its syntax, not ours,
    // and substituting it would break a bucket's ability to update itself.
    let autoupdate = manifest["autoupdate"]["architecture"]["64bit"]["url"]
        .as_str()
        .expect("an autoupdate url");
    assert!(
        autoupdate.contains("$version"),
        "the autoupdate block lost its own placeholder: {autoupdate}"
    );
}

#[test]
fn the_homebrew_formula_points_at_a_real_release() {
    // Unlike the Scoop manifest, this one cannot be generated at release time:
    // Homebrew wants a formula in a tap, and a tap is edited by pull request.
    // So the version is pinned here, and pinned is how the Scoop manifest once
    // sat at 0.1.0 through two releases — and how this formula came to have no
    // `url` at all through four.
    //
    // The check is against the changelog rather than `Cargo.toml`, because the
    // formula points at a *released* version and `Cargo.toml` is the *next*
    // one. Between bumping the version and updating the tap they differ, and a
    // test that demanded they match would go red on every release commit with
    // no way to fix it — the hash cannot be computed before the tag exists.
    //
    // `packaging/` is excluded from the published crate, so inside the package
    // there is nothing to check.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let Ok(formula) = std::fs::read_to_string(root.join("packaging/homebrew/marquee-markdown.rb"))
    else {
        return;
    };

    let field = |name: &str| {
        formula
            .lines()
            .find_map(|line| line.trim().strip_prefix(name)?.trim().strip_prefix('"'))
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_else(|| panic!("the formula has no {name}"))
            .to_owned()
    };

    // A source tarball, because `install` builds with cargo. Pointing at a
    // release archive would download a binary and then build another.
    let url = field("url ");
    let tag = url
        .rsplit('/')
        .next()
        .and_then(|file| file.strip_suffix(".tar.gz"))
        .unwrap_or_else(|| panic!("not a source tarball: {url}"));

    let changelog = std::fs::read_to_string(root.join("CHANGELOG.md")).expect("CHANGELOG.md");
    let released = format!("## [{}] - ", tag.trim_start_matches('v'));
    assert!(
        changelog.contains(&released),
        "the formula is at {tag}, which the changelog has no dated release for"
    );

    let sha = field("sha256 ");
    assert!(
        sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit()),
        "not a sha256: {sha}"
    );
}
