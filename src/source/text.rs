//! Turning raw bytes into renderable text.
//!
//! Local files, remote bodies, and standard input all arrive as bytes, and
//! they are all judged the same way: valid UTF-8 passes through, mostly-text
//! content in some other encoding is rendered with replacement characters
//! rather than refused, and binary data gets an error that says what it is.
//! A reader who opens a PNG by mistake should be told that, not shown a
//! screenful of mojibake — and a Latin-1 document from 2003 should still
//! render, because refusing a whole file over a stray byte helps nobody.

use anyhow::{Result, bail};

/// Decode `bytes` as text, tolerantly. `what` names the source in the error.
///
/// # Errors
/// Returns an error when the bytes look like binary data. A NUL byte is the
/// tell: no text encoding produces one, and every executable, image, and
/// archive format does within the first few bytes.
pub fn from_bytes(bytes: Vec<u8>, what: &str) -> Result<String> {
    if bytes.contains(&0) {
        bail!("{what} is not a text file");
    }
    Ok(match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_utf8_passes_through_unchanged() {
        let text = from_bytes("# Café ☕\n".as_bytes().to_vec(), "doc.md").expect("text");
        assert_eq!(text, "# Café ☕\n");
    }

    #[test]
    fn a_stray_encoding_is_rendered_rather_than_refused() {
        // "café" in Latin-1: the é is a bare 0xE9.
        let text = from_bytes(b"caf\xe9 au lait\n".to_vec(), "old.md").expect("text");
        assert_eq!(text, "caf\u{FFFD} au lait\n");
    }

    #[test]
    fn binary_data_is_named_for_what_it_is() {
        // A PNG header. The reader opened the wrong file; say so.
        let error = from_bytes(b"\x89PNG\r\n\x1a\n\x00\x00".to_vec(), "logo.png")
            .unwrap_err()
            .to_string();
        assert!(error.contains("logo.png is not a text file"), "{error}");
    }
}
