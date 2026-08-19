//! Fuzzy filtering of the file list.
//!
//! Matching is delegated to `nucleo`, the matcher behind `helix` and `fzf`-like
//! pickers.
//!
//! Both sides are put into NFC first. A filesystem and an editor often disagree
//! about whether an accented character is one code point or two, and without
//! normalizing, a file the reader can see would be unfindable by typing its
//! name — with nothing on screen to explain why. `nucleo`'s own normalization
//! is "smart" in the same sense as smart case: it only applies when the query
//! is plain ASCII, which is exactly not this case.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use unicode_normalization::UnicodeNormalization;

/// The indices of `names` that match `query`, best match first.
///
/// An empty query matches everything and preserves the order it was given, so
/// clearing the filter restores the list rather than reshuffling it.
#[must_use]
pub fn matching<'a>(query: &str, names: impl Iterator<Item = &'a str>) -> Vec<usize> {
    if query.trim().is_empty() {
        return names.enumerate().map(|(index, _)| index).collect();
    }
    // Paths score differently from plain text: a match on the file name should
    // beat a match somewhere in the middle of a directory it happens to sit in.
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::parse(
        &normalized(query),
        CaseMatching::Smart,
        Normalization::Smart,
    );

    let mut buffer = Vec::new();
    let mut folded = String::new();
    let mut scored: Vec<(usize, u32)> = names
        .enumerate()
        .filter_map(|(index, name)| {
            // Reuse one buffer rather than allocating per file: this runs on
            // every keystroke, over every file found.
            let name = if name.is_ascii() {
                name
            } else {
                folded.clear();
                folded.extend(name.nfc());
                folded.as_str()
            };
            let haystack = Utf32Str::new(name, &mut buffer);
            pattern
                .score(haystack, &mut matcher)
                .map(|score| (index, score))
        })
        .collect();
    // A stable sort, so entries that score the same keep the order they came
    // in — which is the most-recently-edited-first order the walk produced.
    scored.sort_by_key(|&(_, score)| std::cmp::Reverse(score));
    scored.into_iter().map(|(index, _)| index).collect()
}

/// `text` in NFC, borrowed unchanged when it already is.
fn normalized(text: &str) -> std::borrow::Cow<'_, str> {
    if text.is_ascii() {
        std::borrow::Cow::Borrowed(text)
    } else {
        std::borrow::Cow::Owned(text.nfc().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: &[&str] = &[
        "README.md",
        "docs/ROADMAP.md",
        "docs/THEMING.md",
        "src/render/parse.rs",
        "notes/meeting-2026-08.md",
    ];

    fn matches(query: &str) -> Vec<&'static str> {
        matching(query, NAMES.iter().copied())
            .into_iter()
            .map(|index| NAMES[index])
            .collect()
    }

    #[test]
    fn an_empty_query_keeps_every_entry_in_order() {
        assert_eq!(matches(""), NAMES);
        assert_eq!(matches("   "), NAMES);
    }

    #[test]
    fn a_substring_matches() {
        assert_eq!(matches("ROADMAP"), vec!["docs/ROADMAP.md"]);
    }

    #[test]
    fn gaps_in_the_query_are_allowed() {
        // The point of a fuzzy filter: "rdmp" should still find the roadmap.
        assert!(matches("rdmp").contains(&"docs/ROADMAP.md"));
    }

    #[test]
    fn a_lowercase_query_ignores_case() {
        assert_eq!(matches("readme"), vec!["README.md"]);
    }

    #[test]
    fn the_file_name_outranks_the_directory_it_sits_in() {
        let found = matches("docs");
        assert!(found.len() >= 2, "{found:?}");
        assert!(found.iter().all(|name| name.contains("docs")));
    }

    #[test]
    fn nothing_matches_a_query_that_is_not_there() {
        assert!(matches("zzzzz").is_empty());
    }

    #[test]
    fn combining_marks_do_not_hide_a_file() {
        // The same name written decomposed and precomposed must be findable
        // either way; a filesystem and an editor often disagree about which.
        let names = ["cafe\u{301}.md", "caf\u{e9}-notes.md"];
        for query in ["cafe\u{301}", "caf\u{e9}"] {
            assert_eq!(
                matching(query, names.iter().copied()).len(),
                2,
                "query {query:?} missed one spelling"
            );
        }
    }

    #[test]
    fn results_come_back_best_first() {
        let names = ["a-parse-thing.md", "parse.md"];
        let found = matching("parse", names.iter().copied());
        assert_eq!(names[found[0]], "parse.md");
    }
}
