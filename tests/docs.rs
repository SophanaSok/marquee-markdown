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
    for (chord, action) in keymap.bindings(Mode::Document) {
        assert!(
            readme.contains(&format!("`{chord}`")),
            "`{chord}` ({action}) is bound but not in the README key table"
        );
    }
}

#[test]
fn the_readme_spells_keys_the_way_a_config_file_will() {
    // The table doubles as a reference for `[keys.document]`, so every key it
    // names has to parse as a chord.
    let readme = include_str!("../README.md");
    let table = readme
        .split("| Key | |")
        .nth(1)
        .expect("the README has a key table")
        .split("\n\n")
        .next()
        .expect("the table ends");
    let mut checked = 0;
    for row in table.lines().filter(|row| row.starts_with('|')) {
        let column = row.split('|').nth(1).expect("a key column");
        for cell in column.split('`').skip(1).step_by(2) {
            cell.parse::<marquee_markdown::app::keymap::Chord>()
                .unwrap_or_else(|error| panic!("{error} (from the README key table)"));
            checked += 1;
        }
    }
    assert!(checked > 10, "only {checked} keys found in the table");
}
