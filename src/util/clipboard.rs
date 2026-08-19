//! Putting text on the clipboard.
//!
//! OSC 52 first, because it is the only method that works when the reader is
//! on the other end of an SSH connection: the escape sequence travels back
//! through the terminal, so the text lands on the machine the person is
//! sitting at rather than on the server. A system clipboard call would put it
//! on the wrong computer, silently.
//!
//! `arboard` is the fallback for terminals that do not support OSC 52 and for
//! payloads too large to send that way. Note that on X11 a clipboard set that
//! way is served by this process and is lost when it exits — another reason to
//! prefer the escape sequence.

use std::io::Write;

use anyhow::{Context, Result};

/// The largest base64 payload to send through OSC 52. `tmux` refuses anything
/// larger, and a silently truncated clipboard is worse than a slower path.
const OSC52_LIMIT: usize = 74_994;

/// How the text got there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Through the terminal, so it works over SSH and inside tmux.
    Terminal,
    /// Through the operating system's clipboard.
    System,
}

impl Method {
    /// A short phrase for the status bar.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Terminal => "copied",
            Self::System => "copied to the system clipboard",
        }
    }
}

/// Copy `text`, preferring the terminal.
///
/// # Errors
/// Returns an error only when both methods fail. Callers must treat that as a
/// message for the reader, never as a reason to leave the event loop: the
/// system clipboard can fail for reasons — a headless session, no compositor —
/// that have nothing to do with the document being read.
pub fn copy(text: &str) -> Result<Method> {
    let encoded = base64(text.as_bytes());
    if encoded.len() <= OSC52_LIMIT && write_osc52(&encoded).is_ok() {
        return Ok(Method::Terminal);
    }
    system(text)?;
    Ok(Method::System)
}

/// Send the clipboard escape sequence to the terminal.
fn write_osc52(encoded: &str) -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    write!(out, "\x1b]52;c;{encoded}\x07")?;
    out.flush()
}

/// Set the operating system's clipboard.
fn system(text: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new().context("no system clipboard available")?;
    clipboard
        .set_text(text.to_owned())
        .context("the system clipboard refused the text")
}

/// Standard base64, which is what OSC 52 carries.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let bits = chunk.iter().enumerate().fold(0u32, |acc, (index, &byte)| {
            acc | u32::from(byte) << (16 - 8 * index)
        });
        for index in 0..4 {
            if index <= chunk.len() {
                let sextet = (bits >> (18 - 6 * index)) & 0b11_1111;
                out.push(char::from(ALPHABET[sextet as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_examples() {
        // From RFC 4648, which is what every terminal decodes with.
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_bytes_that_are_not_text() {
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
        assert_eq!(base64(&[0x00, 0x00, 0x00]), "AAAA");
    }

    #[test]
    fn base64_length_is_always_a_multiple_of_four() {
        for length in 0..64 {
            let bytes = vec![b'x'; length];
            assert_eq!(base64(&bytes).len() % 4, 0, "length {length}");
        }
    }

    #[test]
    fn a_multibyte_document_round_trips_through_the_encoder() {
        let text = "日本語 — emoji 🎨 and combining é";
        let encoded = base64(text.as_bytes());
        assert!(encoded.is_ascii(), "the escape sequence must be ASCII");
        assert_eq!(encoded.len() % 4, 0);
    }

    #[test]
    fn the_method_says_something_a_reader_can_read() {
        assert!(Method::Terminal.describe().contains("copied"));
        assert!(Method::System.describe().contains("copied"));
    }
}
