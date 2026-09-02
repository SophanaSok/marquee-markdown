//! Splitting an editor or pager setting into a program and its arguments.
//!
//! `EDITOR="emacsclient -nw"` is an ordinary setting, so the whole string
//! cannot be a program name — but whitespace alone is not a safe split
//! either: `EDITOR='"C:\Program Files\Editor\edit.exe" -w'` names one
//! program, not a program called `C:\Program` with two arguments. Quotes
//! group; there is deliberately no escape character, so Windows paths pass
//! through unmangled. Anything needing more shell than this belongs in a
//! wrapper script, which every editor setting convention already supports.

/// Split a command setting into words, honoring single and double quotes.
///
/// Empty when the setting is empty or all whitespace — the caller's cue to
/// fall back to a default. An unclosed quote is taken as running to the end
/// of the string rather than refused; a setting is not a place to report
/// syntax errors.
#[must_use]
pub fn split(setting: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    for ch in setting.chars() {
        match quote {
            Some(open) if ch == open => quote = None,
            Some(_) => word.push(ch),
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            None => word.push(ch),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_words_split_on_whitespace() {
        assert_eq!(split("less -F -X"), vec!["less", "-F", "-X"]);
    }

    #[test]
    fn a_quoted_path_with_spaces_stays_one_word() {
        assert_eq!(
            split(r#""C:\Program Files\Editor\edit.exe" -w"#),
            vec![r"C:\Program Files\Editor\edit.exe", "-w"]
        );
        assert_eq!(
            split("'/opt/my editor/bin/edit' --wait"),
            vec!["/opt/my editor/bin/edit", "--wait"]
        );
    }

    #[test]
    fn quotes_can_cover_part_of_a_word() {
        assert_eq!(
            split(r#"edit --title="two words""#),
            vec!["edit", "--title=two words"]
        );
    }

    #[test]
    fn an_empty_or_blank_setting_yields_nothing() {
        assert!(split("").is_empty());
        assert!(split("   ").is_empty());
        assert!(split("\"\"").is_empty());
    }

    #[test]
    fn an_unclosed_quote_runs_to_the_end_rather_than_failing() {
        assert_eq!(
            split("edit \"unfinished path"),
            vec!["edit", "unfinished path"]
        );
    }

    #[test]
    fn backslashes_are_not_escapes() {
        // A Windows path must survive without doubling its separators.
        assert_eq!(split(r"C:\tools\edit.exe"), vec![r"C:\tools\edit.exe"]);
    }
}
