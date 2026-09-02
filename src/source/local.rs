//! Reading local files and finding a directory's README.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// README filenames probed inside a directory, in priority order.
pub const README_CANDIDATES: &[&str] = &[
    "README.md",
    "README",
    "Readme.md",
    "Readme",
    "readme.md",
    "readme",
];

/// Find a directory's README.
///
/// Reads the directory once and matches case-insensitively, so a `ReadMe.md`
/// is found too, while the declared priority order still decides between
/// several candidates.
pub fn find_readme(dir: &Path) -> Result<PathBuf> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read directory {}", dir.display()))?;

    let mut by_lowercase = std::collections::HashMap::new();
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        by_lowercase
            .entry(name.to_ascii_lowercase())
            .or_insert_with(|| name.to_owned());
    }

    for candidate in README_CANDIDATES {
        if let Some(actual) = by_lowercase.get(&candidate.to_ascii_lowercase()) {
            return Ok(dir.join(actual));
        }
    }
    bail!("missing markdown source in {}", dir.display())
}

/// Read a file to a string with a path-qualified error.
///
/// Decoded the way a remote body is: tolerantly for text in the wrong
/// encoding, with a plain refusal for binary data. Before this went through
/// [`super::text`], a Latin-1 document was refused with "stream did not
/// contain valid UTF-8" while the same bytes from a URL rendered fine.
pub fn read(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    super::text::from_bytes(bytes, &path.display().to_string())
}

/// Read all of standard input.
pub fn read_stdin() -> Result<String> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::io::stdin()
        .read_to_end(&mut buf)
        .context("cannot read standard input")?;
    super::text::from_bytes(buf, "standard input")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(names: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for name in names {
            std::fs::write(dir.path().join(name), "# hi\n").expect("write");
        }
        dir
    }

    #[test]
    fn finds_the_canonical_readme() {
        let dir = dir_with(&["README.md", "other.md"]);
        let found = find_readme(dir.path()).expect("readme");
        assert_eq!(found.file_name().unwrap(), "README.md");
    }

    #[test]
    fn priority_order_decides_between_candidates() {
        // README.md outranks readme even though both exist.
        let dir = dir_with(&["readme", "README.md"]);
        let found = find_readme(dir.path()).expect("readme");
        assert_eq!(found.file_name().unwrap(), "README.md");
    }

    #[test]
    fn matches_case_insensitively() {
        let dir = dir_with(&["ReAdMe.Md"]);
        let found = find_readme(dir.path()).expect("readme");
        assert_eq!(found.file_name().unwrap(), "ReAdMe.Md");
    }

    #[test]
    fn extensionless_readme_is_accepted() {
        let dir = dir_with(&["README"]);
        assert!(find_readme(dir.path()).is_ok());
    }

    #[test]
    fn a_directory_without_a_readme_reports_clearly() {
        let dir = dir_with(&["notes.md"]);
        let err = find_readme(dir.path()).unwrap_err().to_string();
        assert!(err.contains("missing markdown source"), "{err}");
    }

    #[test]
    fn a_file_in_the_wrong_encoding_still_renders() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("latin1.md");
        std::fs::write(&path, b"# caf\xe9\n").expect("write");
        let text = read(&path).expect("text");
        assert!(text.contains("caf\u{FFFD}"), "{text}");
    }

    #[test]
    fn a_binary_file_is_refused_with_a_reason() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("image.gif");
        std::fs::write(&path, b"GIF89a\x00\x00\x01").expect("write");
        let error = read(&path).unwrap_err().to_string();
        assert!(error.contains("not a text file"), "{error}");
    }

    #[test]
    fn a_directory_named_readme_is_not_mistaken_for_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("README.md")).expect("mkdir");
        assert!(find_readme(dir.path()).is_err());
    }
}
