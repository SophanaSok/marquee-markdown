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
    for row in key_tables().into_iter().flatten() {
        let column = row.split('|').nth(1).expect("a key column");
        for cell in column.split('`').skip(1).step_by(2) {
            cell.parse::<marquee_markdown::app::keymap::Chord>()
                .unwrap_or_else(|error| panic!("{error} (from a README key table)"));
            checked += 1;
        }
    }
    assert!(checked > 20, "only {checked} keys found in the tables");
}

/// The rows of every key table in the README.
///
/// A table ends at the first line that is not a table row, rather than at a
/// blank line. The difference only shows on Windows, where the checkout has
/// CRLF endings and so contains no `\n\n` to find: splitting on one ran every
/// table on to the end of the file and fed whatever backticks it met to the
/// chord parser. `lines` is the only reader here that is agnostic about that.
fn key_tables() -> Vec<Vec<&'static str>> {
    let readme = include_str!("../README.md");
    let tables: Vec<Vec<&str>> = readme
        .split("| Key | |")
        .skip(1)
        .map(|rest| {
            rest.lines()
                // The first line is what is left of the header row itself.
                .skip(1)
                .take_while(|line| line.starts_with('|'))
                .collect()
        })
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
    // one. Demanding they match is unsatisfiable, not merely awkward: the
    // `sha256` is of the tarball GitHub builds from the tag, the tag points at
    // the release commit, so a correct hash inside that commit would be the
    // hash of a tree containing itself. Every way out is worse — tagging first
    // leaves the tag naming a commit that fails this test, and a placeholder
    // hash passes it while breaking `brew install`.
    //
    // Hence two entries rather than one. The tap is updated after the tag, so
    // the formula spends the gap exactly one release behind; allowing only the
    // newest would red every release commit. Allowing any dated release, which
    // is what this first did, permits the failure it was written to stop — a
    // formula frozen while the versions march past, which is the Scoop-at-0.1.0
    // story above. One release of lag is the process. Two is a forgotten tap.
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

    // Newest first, and `## [Unreleased]` carries no date, so it drops out here
    // rather than needing a case of its own — which is precisely what lets the
    // formula lag a release without the test having to know it is mid-release.
    let changelog = std::fs::read_to_string(root.join("CHANGELOG.md")).expect("CHANGELOG.md");
    let dated: Vec<&str> = changelog
        .lines()
        .filter_map(|line| Some(line.strip_prefix("## [")?.split_once("] - ")?.0))
        .collect();

    match dated.iter().position(|&v| v == tag.trim_start_matches('v')) {
        Some(0 | 1) => {}
        Some(behind) => panic!(
            "the formula is at {tag}, {behind} releases behind {}: the tap was \
             not updated after the last one",
            dated[0]
        ),
        None => panic!("the formula is at {tag}, which the changelog has no dated release for"),
    }

    let sha = field("sha256 ");
    assert!(
        sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit()),
        "not a sha256: {sha}"
    );
}

/// The versions pinned in every manifest that cannot be generated at release
/// time, and how far behind the changelog each is allowed to be.
///
/// Homebrew has its own test above, because it also has a `sha256` to check.
/// These three are the same problem in a different file: a version that has to
/// be written down, and so can be forgotten. All of them are bumped *after*
/// the tag — a source tarball's hash cannot exist before the tag it is made
/// from — so one release of lag is the process and two is a forgotten bump.
const PINNED: &[(&str, &str)] = &[
    ("packaging/aur/marquee-markdown/PKGBUILD", "pkgver="),
    ("packaging/aur/marquee-markdown-bin/PKGBUILD", "pkgver="),
    ("packaging/nix/default.nix", "version = \""),
];

#[test]
fn every_pinned_package_manifest_points_at_a_real_release() {
    // `packaging/` is excluded from the published crate, so inside the package
    // there is nothing to check.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

    let changelog = std::fs::read_to_string(root.join("CHANGELOG.md")).expect("CHANGELOG.md");
    let dated: Vec<&str> = changelog
        .lines()
        .filter_map(|line| Some(line.strip_prefix("## [")?.split_once("] - ")?.0))
        .collect();

    for (path, key) in PINNED {
        let Ok(text) = std::fs::read_to_string(root.join(path)) else {
            continue;
        };
        // Take the version characters themselves rather than trimming the
        // punctuation around them: `pkgver=0.6.1` and `version = "0.6.1";`
        // are the same value wearing different syntax.
        let version: String = text
            .lines()
            .find_map(|line| line.trim().strip_prefix(key))
            .map(|rest| {
                rest.trim_start_matches(['"', ' '])
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect()
            })
            .unwrap_or_else(|| panic!("{path} has no line starting {key:?}"));

        match dated.iter().position(|&v| v == version) {
            Some(0 | 1) => {}
            Some(behind) => panic!(
                "{path} is at {version}, {behind} releases behind {}: it was \
                 not bumped after the last one",
                dated[0]
            ),
            None => panic!("{path} is at {version}, which the changelog has no dated release for"),
        }
    }
}

/// The longest name `pkill -x` can ever match.
///
/// The kernel stores a process name in a fixed 16-byte field, so `comm` is
/// fifteen characters and a `pkill -x` pattern longer than that matches
/// nothing at all — it does not fall back to a prefix, it just never fires.
const COMM_MAX: usize = 15;

#[test]
fn the_theme_hook_signals_a_name_pkill_can_actually_match() {
    // This was shipped spelling the binary in full, which looks obviously
    // right and silently signals nothing: `marquee-markdown` is sixteen
    // characters, so only `mmd` was ever reached. It cost a live desktop test
    // to notice, which is exactly the kind of thing worth a guard.
    let hook = std::fs::read_to_string("packaging/omarchy/theme-set.d/reload-marquee")
        .expect("the theme hook is in the tree");

    let mut signalled = Vec::new();
    for line in hook.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        // `pkill -USR1 -x <name>`: the name is the last word.
        if let Some(name) = line.strip_prefix("pkill ").and_then(|rest| {
            rest.contains("-x")
                .then(|| rest.split_whitespace().next_back())
                .flatten()
        }) {
            assert!(
                name.len() <= COMM_MAX,
                "the hook signals {name:?}, which is {} characters: `pkill -x` \
                 caps at {COMM_MAX} and would match nothing",
                name.len()
            );
            signalled.push(name.to_owned());
        }
    }

    // Both binaries, or a reader who ran the one that is not covered never
    // hears about a theme change.
    assert!(
        signalled.iter().any(|name| name == "mmd"),
        "the hook has to signal `mmd`: {signalled:?}"
    );
    assert!(
        signalled
            .iter()
            .any(|name| "marquee-markdown".starts_with(name.as_str()) && name.len() == COMM_MAX),
        "the hook has to signal the long binary under its truncated name: {signalled:?}"
    );
}

#[test]
fn one_description_serves_every_channel_and_keeps_to_the_strictest_rules() {
    // The crate description is also the Debian synopsis, and the same wording
    // is spelled again as the RPM summary, the Homebrew `desc`, and the Scoop
    // `description`. So it obeys the union of the channels' rules — Debian:
    // under 80 characters, no initial article (lintian `synopsis-too-long`,
    // the Developers Reference); Homebrew: no article, capitalized, no
    // trailing full stop, does not begin with the name — and the copies must
    // not drift. It shipped at 98 characters with a leading "A " once; every
    // channel that renders a synopsis would have flagged it before any test
    // here did.
    let manifest: toml::Value = toml::from_str(include_str!("../Cargo.toml")).expect("Cargo.toml");
    let description = manifest["package"]["description"]
        .as_str()
        .expect("a description");

    assert!(
        description.len() < 80,
        "{} characters; Debian wants the synopsis under 80: {description}",
        description.len()
    );
    for article in ["A ", "An ", "The "] {
        assert!(
            !description.starts_with(article),
            "leading article: {description}"
        );
    }
    assert!(
        !description.ends_with('.'),
        "trailing full stop: {description}"
    );
    assert!(
        !description.to_ascii_lowercase().starts_with("marquee"),
        "starts with the name: {description}"
    );
    assert!(
        description
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase()),
        "not capitalized: {description}"
    );

    let summary = manifest["package"]["metadata"]["generate-rpm"]["summary"]
        .as_str()
        .expect("an RPM summary");
    assert_eq!(summary, description, "the RPM summary drifted");

    // The formula and the template are excluded from the published crate, so
    // inside the package there is nothing more to hold together.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Ok(formula) =
        std::fs::read_to_string(root.join("packaging/homebrew/marquee-markdown.rb"))
    {
        let desc = formula
            .lines()
            .find_map(|line| line.trim().strip_prefix("desc \""))
            .and_then(|rest| rest.split('"').next())
            .expect("the formula has a desc");
        assert_eq!(desc, description, "the Homebrew desc drifted");
    }
    if let Ok(template) =
        std::fs::read_to_string(root.join("packaging/scoop/marquee-markdown.template.json"))
    {
        let template: serde_json::Value = serde_json::from_str(&template).expect("template JSON");
        assert_eq!(
            template["description"].as_str().expect("a description"),
            description,
            "the Scoop description drifted"
        );
    }
}

#[test]
fn the_release_archive_names_match_binstall_and_the_scoop_template() {
    // `cargo binstall` and Scoop construct download names from hand-written
    // strings in `Cargo.toml` and the template, while the workflow names the
    // real archives independently in `release.yml`. A rename on either side
    // breaks installs for every user with a green CI — the same class of bug
    // the Scoop hash test was written against. `.github/` is excluded from
    // the published crate, so inside the package there is nothing to check.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let Ok(workflow) = std::fs::read_to_string(root.join(".github/workflows/release.yml")) else {
        return;
    };

    // The one line every fetcher's expectations descend from.
    assert!(
        workflow.contains(r#"name="marquee-markdown-${GITHUB_REF_NAME}-${{ matrix.name }}""#),
        "the archive naming line in release.yml changed; the binstall overrides \
         and the Scoop template name archives after it"
    );

    let manifest = include_str!("../Cargo.toml");
    for (platform, ext) in [
        ("x86_64-linux", "tar.gz"),
        ("aarch64-macos", "tar.gz"),
        ("x86_64-macos", "tar.gz"),
        ("x86_64-windows", "zip"),
    ] {
        assert!(
            workflow.contains(&format!("name: {platform}")),
            "release.yml no longer builds {platform}, but a binstall override \
             still points at it"
        );
        let stem = format!("{{ name }}-v{{ version }}-{platform}.{ext}");
        assert!(
            manifest.contains(&stem),
            "no binstall override fetches …-{platform}.{ext}; \
             `cargo binstall` would fall back to building from source"
        );
    }

    if let Ok(template) =
        std::fs::read_to_string(root.join("packaging/scoop/marquee-markdown.template.json"))
    {
        let stem = "marquee-markdown-v@VERSION@-x86_64-windows.zip";
        assert!(
            template.contains(stem),
            "the template lost the archive name"
        );
        assert!(
            workflow.contains("marquee-markdown-v$version-x86_64-windows.zip"),
            "the workflow's Scoop step names a different archive than it builds"
        );
    }
}

#[test]
fn the_linux_packages_carry_man_pages_and_completions_for_both_binaries() {
    // Debian counts a binary without a man page as a bug (lintian
    // `binary-without-manpage`), and both packages shipped bare binaries for
    // four releases before anyone noticed — nothing builds a .deb until a tag
    // is pushed. The asset lists are plain strings in `Cargo.toml`, so the
    // cheapest guard is that each generated file is named by both tables.
    let manifest = include_str!("../Cargo.toml");
    for name in ["marquee-markdown", "mmd"] {
        for file in [
            format!("dist/man/{name}.1.gz"),
            format!("dist/completions/{name}.bash"),
            format!("dist/completions/{name}.zsh"),
            format!("dist/completions/{name}.fish"),
        ] {
            let uses = manifest.matches(file.as_str()).count();
            assert!(
                uses >= 2,
                "{file} appears {uses} time(s) in Cargo.toml; the deb and rpm \
                 asset lists must both ship it"
            );
        }
    }
}
