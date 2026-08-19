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
