//! Writing one setting back into the configuration file.
//!
//! The reader can change its theme from the theme picker, and that choice is
//! meant to outlive the session — which means editing a file the reader wrote
//! by hand. Two rules follow from that, and they are the whole of this module:
//!
//! - **Keep everything that is not the setting.** Comments, key order, spacing
//!   and every other setting survive, because [`toml_edit`] edits the document
//!   rather than re-serializing it. Rendering [`Config`](super::Config) back
//!   out would flatten the file into today's defaults and lose the comments.
//! - **Never leave a half-written file.** The write goes to a sibling and is
//!   renamed over the original, so an interrupted save loses the change rather
//!   than the configuration.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml_edit::{DocumentMut, Item, Table, value};

/// The file a setting should be written to.
///
/// `found` is [`Config::path`](super::Config::path): the file that was actually
/// read, including one named by `--config` or `MARQUEE_CONFIG`, since that is
/// the file whose settings are in force. With no file at all there is nothing
/// to preserve, so the default location is created.
///
/// # Errors
/// Returns an error when there is no configuration directory to write into,
/// which is the same condition that leaves the reader with no file to read.
pub fn target(found: Option<&Path>) -> Result<PathBuf> {
    match found {
        Some(path) => Ok(path.to_path_buf()),
        None => super::default_path()
            .context("there is no configuration directory to write to on this system"),
    }
}

/// Record `style` as `[general] style`, leaving the rest of `path` alone.
///
/// Creates the file, and any directory above it, when it is not there yet.
///
/// # Errors
/// Returns an error when the file cannot be read, is not valid TOML, or cannot
/// be written.
pub fn set_style(path: &Path, style: &str) -> Result<()> {
    edit(path, |doc| {
        general(doc)["style"] = value(style);
    })
}

/// Apply `change` to the document at `path` and write it back.
fn edit(path: &Path, change: impl FnOnce(&mut DocumentMut)) -> Result<()> {
    // Absent is empty rather than an error: saving a setting is how most people
    // will end up with a configuration file in the first place.
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).with_context(|| format!("cannot read {}", path.display())),
    };

    let mut doc: DocumentMut = text
        .parse()
        .with_context(|| format!("cannot parse {}", path.display()))?;
    change(&mut doc);
    write_atomically(path, &doc.to_string())
}

/// The `[general]` table, created if the file has not got one.
///
/// An existing `[general]` is used as it stands, whatever form it takes, so a
/// file written as `general.style = "…"` keeps that shape instead of growing a
/// second definition of the same table.
fn general(doc: &mut DocumentMut) -> &mut Item {
    let entry = doc
        .entry("general")
        .or_insert_with(|| Item::Table(Table::new()));
    if !entry.is_table() && !entry.is_inline_table() {
        // Whatever was there is not a table, so it cannot hold the setting.
        // Replacing it is the only way to write one, and the alternative —
        // failing — would leave the reader unable to save at all.
        *entry = Item::Table(Table::new());
    }
    entry
}

/// Write `text` to `path` by way of a sibling file, so `path` is either the old
/// contents or the new ones and never half of each.
fn write_atomically(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }

    // Alongside the target rather than in the temporary directory: a rename is
    // only atomic within one filesystem, and `/tmp` is often a different one.
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".new");
    let temporary = PathBuf::from(temporary);

    std::fs::write(&temporary, text)
        .with_context(|| format!("cannot write {}", temporary.display()))?;
    std::fs::rename(&temporary, path)
        .with_context(|| format!("cannot replace {}", path.display()))
        .inspect_err(|_| {
            // Leaving the half of the operation that did work behind would
            // litter the configuration directory with `.new` files.
            let _ = std::fs::remove_file(&temporary);
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(before: &str) -> String {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, before).expect("write");
        set_style(&path, "paper").expect("set style");
        std::fs::read_to_string(&path).expect("read back")
    }

    /// The reason this module exists rather than rendering `Config` back out.
    #[test]
    fn everything_that_is_not_the_setting_survives() {
        let out = written(
            "# my settings\n\
             [general]\n\
             # the theme I like\n\
             style = \"slate\"\n\
             width = 72        # narrow on purpose\n\
             \n\
             [keys.document]\n\
             \"ctrl+n\" = \"line-down\"\n",
        );
        assert!(out.contains("# my settings"), "{out}");
        assert!(out.contains("# the theme I like"), "{out}");
        assert!(out.contains("# narrow on purpose"), "{out}");
        assert!(out.contains("width = 72"), "{out}");
        assert!(out.contains("\"ctrl+n\" = \"line-down\""), "{out}");
        assert!(out.contains("style = \"paper\""), "{out}");
        assert!(!out.contains("slate"), "the old value is gone: {out}");
    }

    #[test]
    fn a_general_table_without_the_setting_gains_it() {
        let out = written("[general]\nwidth = 72\n");
        assert!(out.contains("width = 72"), "{out}");
        assert!(out.contains("style = \"paper\""), "{out}");
    }

    #[test]
    fn a_file_with_no_general_table_gains_one() {
        let out = written("[ui]\ncontents = false\n");
        assert!(out.contains("contents = false"), "{out}");
        assert!(out.contains("style = \"paper\""), "{out}");
        let (file, unknown) = super::super::schema::parse(&out).expect("parse");
        assert!(unknown.is_empty(), "{unknown:?}");
        assert_eq!(file.general.style.as_deref(), Some("paper"));
    }

    #[test]
    fn a_dotted_key_is_edited_in_place_rather_than_duplicated() {
        let out = written("general.style = \"slate\"\n");
        assert_eq!(out.matches("style").count(), 1, "{out}");
        let (file, _) = super::super::schema::parse(&out).expect("parse");
        assert_eq!(file.general.style.as_deref(), Some("paper"));
    }

    #[test]
    fn a_file_that_is_not_there_yet_is_created() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("config.toml");
        set_style(&path, "slate").expect("set style");
        let out = std::fs::read_to_string(&path).expect("read back");
        let (file, _) = super::super::schema::parse(&out).expect("parse");
        assert_eq!(file.general.style.as_deref(), Some("slate"));
    }

    /// Round-tripping through the real parser, because a write that produces
    /// something this program cannot read back is worse than no write at all.
    #[test]
    fn what_is_written_is_what_is_read_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        for style in ["paper", "slate", "some-user-theme"] {
            set_style(&path, style).expect("set style");
            let text = std::fs::read_to_string(&path).expect("read back");
            let (file, unknown) = super::super::schema::parse(&text).expect("parse");
            assert_eq!(file.general.style.as_deref(), Some(style));
            assert!(unknown.is_empty(), "{unknown:?}");
        }
    }

    #[test]
    fn malformed_toml_is_an_error_that_names_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[general\n").expect("write");
        let error = set_style(&path, "paper").unwrap_err().to_string();
        assert!(error.contains("config.toml"), "{error}");
    }

    #[test]
    fn a_failed_write_leaves_no_litter_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A directory where the file should be: writing the sibling works, and
        // renaming over it does not.
        let path = dir.path().join("config.toml");
        std::fs::create_dir(&path).expect("mkdir");
        assert!(set_style(&path, "paper").is_err());
        assert!(!dir.path().join("config.toml.new").exists());
    }

    #[test]
    fn the_target_is_the_file_that_was_read() {
        let found = Path::new("/somewhere/else/config.toml");
        assert_eq!(target(Some(found)).expect("target"), found);
    }
}
