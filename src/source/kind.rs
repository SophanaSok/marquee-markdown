//! What kind of file are we looking at, and how should it be rendered?
//!
//! Matching glow: the markdown extensions plus *extensionless* files are
//! treated as markdown; anything else is wrapped in a fenced block using its
//! extension as the language, which turns the reader into a syntax-highlighted
//! code viewer.

use std::path::Path;

/// Extensions treated as markdown.
pub const MARKDOWN_EXTENSIONS: &[&str] = &["md", "mdown", "mkdn", "mkd", "markdown"];

/// How a file's contents should be handed to the renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    Markdown,
    /// Source code; the payload is the fence language to highlight with.
    Code {
        language: String,
    },
}

impl FileKind {
    /// Whether this file renders as markdown.
    #[must_use]
    pub fn is_markdown(&self) -> bool {
        matches!(self, Self::Markdown)
    }
}

/// Classify a path by extension.
#[must_use]
pub fn of_path(path: &Path) -> FileKind {
    match path.extension().and_then(|e| e.to_str()) {
        None => FileKind::Markdown,
        Some(ext) => {
            let lower = ext.to_ascii_lowercase();
            if MARKDOWN_EXTENSIONS.contains(&lower.as_str()) {
                FileKind::Markdown
            } else {
                FileKind::Code { language: lower }
            }
        }
    }
}

/// Wrap source code in a fenced block so the markdown renderer highlights it.
///
/// Uses a fence long enough to survive any backtick run inside the file.
#[must_use]
pub fn wrap_as_code(text: &str, language: &str) -> String {
    let longest_run = text
        .split('\n')
        .flat_map(|line| line.split(|c| c != '`').map(str::len))
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest_run.max(2) + 1);
    format!("{fence}{language}\n{text}\n{fence}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_extensions_are_markdown() {
        for ext in MARKDOWN_EXTENSIONS {
            let path = format!("doc.{ext}");
            assert!(of_path(Path::new(&path)).is_markdown(), "{ext}");
        }
    }

    #[test]
    fn extension_matching_is_case_insensitive() {
        assert!(of_path(Path::new("README.MD")).is_markdown());
    }

    #[test]
    fn extensionless_files_are_markdown() {
        assert!(of_path(Path::new("README")).is_markdown());
        assert!(of_path(Path::new("/etc/hosts")).is_markdown());
    }

    #[test]
    fn other_extensions_become_code_with_that_language() {
        assert_eq!(
            of_path(Path::new("main.rs")),
            FileKind::Code {
                language: "rs".into()
            }
        );
        assert_eq!(
            of_path(Path::new("a/b/script.PY")),
            FileKind::Code {
                language: "py".into()
            }
        );
    }

    #[test]
    fn wrapping_produces_a_parseable_fence() {
        let out = wrap_as_code("fn main() {}", "rs");
        assert!(out.starts_with("```rs\n"));
        assert!(out.trim_end().ends_with("```"));
    }

    #[test]
    fn fence_grows_past_backticks_in_the_content() {
        // A file containing a ``` line must not close the fence early.
        let out = wrap_as_code("before\n```\nafter", "txt");
        let fence_len = out.chars().take_while(|c| *c == '`').count();
        assert!(fence_len >= 4, "fence too short: {fence_len}");
        // The content's own fence must be strictly shorter than the wrapper.
        assert!(out.contains("\n```\n"));
    }

    #[test]
    fn wrapped_code_round_trips_through_the_parser() {
        let src = "let x = 1;\n```\nnot a real fence";
        let wrapped = wrap_as_code(src, "rs");
        let blocks = crate::render::parse::parse(&wrapped);
        assert_eq!(blocks.len(), 1, "expected exactly one code block");
        let crate::render::block::BlockKind::CodeBlock { language, text } = &blocks[0].kind else {
            panic!("expected a code block, got {:?}", blocks[0].kind);
        };
        assert_eq!(language.as_deref(), Some("rs"));
        assert_eq!(text, src);
    }
}
