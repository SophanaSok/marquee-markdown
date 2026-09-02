//! Asking the terminal what colors it is drawing with.
//!
//! `OSC 10`, `OSC 11` and `OSC 4` are questions a terminal answers on the same
//! stream it takes keystrokes from, so asking while the reader is running is
//! only safe when nothing else is reading: a reply the event thread swallows
//! is a reply nobody is waiting for any more.
//!
//! The first exchange happens in `cli::run`, before the screen is taken and
//! before that thread exists. Later ones happen while it is running, and are
//! safe for a different reason: [`crate::app::recolor`] takes
//! [`crate::app::gate::pause`] first, which does not return until every reader
//! has parked at the top of its loop, and hands the proof to
//! `event::discard_pending_input` afterwards so a late reply never parses into
//! ordinary-looking keys. That is the same handshake an editor handoff uses,
//! and this module is sound under it and under nothing weaker. Do not call
//! into here from anywhere that does not hold a `Paused`.
//!
//! Four details make the exchange safe:
//!
//! - **It runs on `/dev/tty`, not on standard input or output.** So a piped
//!   document (`cat x.md | mmd`) is still a document, redirected output still
//!   gets no bytes it did not ask for, and nothing here shares a descriptor
//!   with crossterm.
//! - **It ends on an answer, not on a clock.** Every query is followed by a
//!   Primary Device Attributes request, which every terminal that could run
//!   this reader answers; seeing that reply means every reply that was coming
//!   has arrived. The timeout is the backstop for the terminals that answer
//!   nothing at all — `screen` swallows the query outright — and `VTIME` is
//!   what makes waiting for it cost one blocked read rather than a thread.
//! - **The terminal is put back exactly as it was**, from a `Drop` impl, on
//!   every path out including a panic. Leaving a terminal in raw mode is the
//!   failure this module could most easily cause and would least obviously
//!   explain.
//! - **Input that was already waiting is left alone.** A reply and a keystroke
//!   are the same bytes on the same stream, and reading is destructive: what
//!   this consumes looking for an answer, the reader never sees. So if
//!   anything is already queued when we arrive — somebody typed ahead, or a
//!   script piped its keys in — the question is not asked at all. What remains
//!   is the round trip itself, a few milliseconds before the first frame is
//!   drawn, which is the same window every terminal program that asks this
//!   question lives with.
//!
//! A terminal that says nothing is not an error. It is the ordinary case, and
//! the answer is [`TerminalColors::UNKNOWN`], which every caller already has a
//! plan for. Only `--style system` asks at all. Measured against a pty that
//! answers on cue:
//!
//! | terminal | outcome | cost over not asking |
//! | --- | --- | --- |
//! | answers | its colors | none measurable |
//! | tmux | falls back — it answers the device query and nothing else | none measurable |
//! | answers later than the timeout | falls back | the timeout |
//! | answers nothing at all (`screen`) | falls back | the timeout |
//!
//! The sentinel is what makes the middle two rows cheap: tmux replies to the
//! device query promptly, so the read ends there rather than on the clock.

use std::time::Duration;

use crate::theme::Rgb;
use crate::theme::system::TerminalColors;

/// How long to wait for a terminal that is not going to answer.
///
/// Long enough to cross a slow link, short enough not to read as a hang. The
/// common path does not spend it: the device-attributes reply ends the wait as
/// soon as it lands, which on a local terminal is single-digit milliseconds.
pub const TIMEOUT: Duration = Duration::from_millis(100);

/// How much to ask for.
///
/// The distinction exists because the two callers want different things.
/// Building a palette needs every answer; noticing that the palette *changed*
/// needs one, and asking for eighteen to find out that nothing moved is
/// eighteen round trips of nothing. A background is enough to detect a theme
/// switch: no colorscheme worth switching to keeps the old page color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// Both defaults and all sixteen slots — everything a palette is built
    /// from.
    Everything,
    /// The background alone, as a change detector.
    Background,
}

/// Ask the terminal about its colors, or give up.
///
/// Never fails and never blocks longer than `timeout`: a terminal that will
/// not answer is reported as [`TerminalColors::UNKNOWN`] rather than as an
/// error, because there is nothing for a caller to do about it that is
/// different from what it does with a partial answer.
#[must_use]
pub fn query(timeout: Duration) -> TerminalColors {
    query_for(Ask::Everything, timeout)
}

/// Ask the terminal only what `ask` names.
///
/// # Safety of a running reader
/// Sound only while the terminal reader is standing down — see the module
/// header. [`crate::app::recolor`] is the only caller that runs with the
/// screen taken, and it holds a [`crate::app::gate::Paused`] across this.
#[must_use]
pub fn query_for(ask: Ask, timeout: Duration) -> TerminalColors {
    platform::query(ask, timeout)
}

/// The bytes to send, ending in the sentinel whatever was asked for.
fn request(ask: Ask) -> Vec<u8> {
    let mut out = Vec::new();
    // The background goes first in both shapes, so a terminal that answers
    // one question and then stops still answers the one that detects a change.
    out.extend_from_slice(b"\x1b]11;?\x07");
    if matches!(ask, Ask::Everything) {
        out.extend_from_slice(b"\x1b]10;?\x07");
        for slot in 0..16 {
            out.extend_from_slice(format!("\x1b]4;{slot};?\x07").as_bytes());
        }
    }
    // Primary Device Attributes, last, as the sentinel. Answered even by
    // terminals that ignore every question above it, which is what makes the
    // read end on an answer rather than on the clock.
    out.extend_from_slice(b"\x1b[c");
    out
}

/// The most reply we will hold before deciding the stream is not replies.
///
/// Eighteen answers do not reach a kilobyte. This is a bound on a terminal
/// that streams, not a size the real exchange approaches.
const MAX_REPLY: usize = 4096;

/// Whether a Primary Device Attributes reply has arrived, meaning every reply
/// that was coming has.
///
/// The reply is `CSI ? … c`, and its parameters vary by terminal, so this
/// looks for the shape rather than for any particular one.
fn answered(bytes: &[u8]) -> bool {
    let mut rest = bytes;
    while let Some(at) = find(rest, b"\x1b[?") {
        rest = &rest[at + 3..];
        if let Some(end) = rest.iter().position(|b| !b.is_ascii_digit() && *b != b';') {
            if rest[end] == b'c' {
                return true;
            }
            rest = &rest[end..];
        } else {
            return false;
        }
    }
    false
}

/// Read every `OSC` reply out of what the terminal sent.
///
/// Anything unrecognized is dropped rather than reported: a terminal is free
/// to answer some questions, answer them out of order, interleave them, or
/// send something else entirely, and none of that is a failure.
fn parse(bytes: &[u8]) -> TerminalColors {
    let mut colors = TerminalColors::UNKNOWN;
    let mut rest = bytes;
    while let Some(at) = find(rest, b"\x1b]") {
        rest = &rest[at + 2..];
        let Some((payload, after)) = take_string(rest) else {
            break;
        };
        rest = after;
        apply(&mut colors, payload);
    }
    colors
}

/// Split off one `OSC` payload and whatever follows it.
///
/// Both terminators are accepted because both are used: `BEL` by most
/// terminals, `ST` by the ones following the standard.
fn take_string(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let bel = bytes.iter().position(|b| *b == 0x07);
    let st = find(bytes, b"\x1b\\");
    match (bel, st) {
        (Some(a), Some(b)) if a < b => Some((&bytes[..a], &bytes[a + 1..])),
        (_, Some(b)) => Some((&bytes[..b], &bytes[b + 2..])),
        (Some(a), None) => Some((&bytes[..a], &bytes[a + 1..])),
        (None, None) => None,
    }
}

/// Record one payload, if it is one of the three answers we asked for.
fn apply(colors: &mut TerminalColors, payload: &[u8]) {
    let Ok(text) = std::str::from_utf8(payload) else {
        return;
    };
    let mut fields = text.split(';');
    match fields.next() {
        Some("10") => colors.fg = fields.next().and_then(color),
        Some("11") => colors.bg = fields.next().and_then(color),
        Some("4") => {
            let slot = fields.next().and_then(|s| s.parse::<usize>().ok());
            let value = fields.next().and_then(color);
            if let (Some(slot), Some(value)) = (slot, value)
                && slot < colors.ansi.len()
            {
                colors.ansi[slot] = Some(value);
            }
        }
        _ => {}
    }
}

/// An X11 color specification, as terminals actually emit them.
///
/// `rgb:` with one to four hex digits per component is what the question asks
/// for and what nearly everything answers; `#` forms and a stray `rgba:` turn
/// up often enough to be worth accepting rather than discarding a whole
/// palette over.
fn color(spec: &str) -> Option<Rgb> {
    let spec = spec.trim();
    if let Some(rest) = spec
        .strip_prefix("rgb:")
        .or_else(|| spec.strip_prefix("rgba:"))
    {
        let mut parts = rest.split('/');
        let mut next = || parts.next().and_then(component);
        return Some(Rgb(next()?, next()?, next()?));
    }
    if let Some(rest) = spec.strip_prefix('#') {
        // `#rgb`, `#rrggbb`, `#rrrgggbbb`, `#rrrrggggbbbb`: one third each,
        // however many digits that is.
        if rest.len() % 3 != 0 || rest.is_empty() || rest.len() > 12 {
            return None;
        }
        let width = rest.len() / 3;
        let at = |index: usize| component(&rest[index * width..(index + 1) * width]);
        return Some(Rgb(at(0)?, at(1)?, at(2)?));
    }
    None
}

/// One hex component of any width, scaled to eight bits.
fn component(digits: &str) -> Option<u8> {
    if digits.is_empty() || digits.len() > 4 {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    // `ffff` and `ff` and `f` all mean full intensity, so scale by the widest
    // value the digit count can hold rather than by a fixed divisor.
    let full = 16u32.pow(u32::try_from(digits.len()).ok()?) - 1;
    u8::try_from(value * 255 / full).ok()
}

/// The first position of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(unix)]
mod platform {
    use std::fs::{File, OpenOptions};
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

    use rustix::io::ioctl_fionread;
    use rustix::termios::{OptionalActions, SpecialCodeIndex, Termios, tcgetattr, tcsetattr};

    use super::{Ask, MAX_REPLY, answered, parse, request};
    use crate::theme::system::TerminalColors;

    pub fn query(ask: Ask, timeout: Duration) -> TerminalColors {
        let Ok(tty) = OpenOptions::new().read(true).write(true).open("/dev/tty") else {
            return TerminalColors::UNKNOWN;
        };
        // Bytes already in the queue are the reader's, not ours. Asking now
        // would read them looking for an answer and throw them away, which is
        // how a piped `printf 'jjq'` becomes a reader waiting forever for keys
        // it was already sent.
        if !idle(&tty) {
            return TerminalColors::UNKNOWN;
        }
        let Ok(saved) = tcgetattr(&tty) else {
            return TerminalColors::UNKNOWN;
        };
        let mut raw = saved.clone();
        raw.make_raw();
        // `VMIN` 0 with a `VTIME` makes a read return empty-handed when the
        // terminal has nothing to say, which is the whole timeout mechanism:
        // no second thread, and so no thread left blocked on a descriptor the
        // reader is about to want.
        raw.special_codes[SpecialCodeIndex::VMIN] = 0;
        raw.special_codes[SpecialCodeIndex::VTIME] = deciseconds(timeout);
        if tcsetattr(&tty, OptionalActions::Now, &raw).is_err() {
            return TerminalColors::UNKNOWN;
        }
        // From here on the terminal is in raw mode, and putting it back is not
        // optional. The guard owns that, including if anything below panics.
        let guard = Restore { tty: &tty, saved };
        let replies = exchange(&tty, ask, timeout);
        drop(guard);
        parse(&replies)
    }

    /// Whether the terminal has nothing queued that the reader will want.
    ///
    /// `FIONREAD` counts what is waiting without consuming it, which is the
    /// only way to ask this question that does not answer it destructively. A
    /// terminal that will not say counts as busy: not asking costs a palette,
    /// and asking wrongly costs somebody's keystrokes.
    fn idle(tty: &File) -> bool {
        ioctl_fionread(tty).is_ok_and(|waiting| waiting == 0)
    }

    /// Puts the terminal back the way it was found, on every path out.
    struct Restore<'a> {
        tty: &'a File,
        saved: Termios,
    }

    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            let _ = tcsetattr(self.tty, OptionalActions::Now, &self.saved);
        }
    }

    /// `VTIME` is in tenths of a second, and zero would mean "wait forever".
    fn deciseconds(timeout: Duration) -> u8 {
        let tenths = timeout.as_millis().div_ceil(100);
        u8::try_from(tenths).unwrap_or(u8::MAX).max(1)
    }

    /// Send the questions, collect whatever comes back.
    ///
    /// Returns what it has rather than an error: a short read, a closed
    /// terminal and a terminal with no opinion are all the same answer.
    fn exchange(tty: &File, ask: Ask, timeout: Duration) -> Vec<u8> {
        let mut writer = tty;
        if writer.write_all(&request(ask)).is_err() || writer.flush().is_err() {
            return Vec::new();
        }
        let deadline = Instant::now() + timeout;
        let mut reader = tty;
        let mut replies = Vec::new();
        let mut chunk = [0u8; 256];
        while Instant::now() < deadline && replies.len() < MAX_REPLY {
            match reader.read(&mut chunk) {
                // `VTIME` expired with nothing to show for it.
                Ok(0) => break,
                Ok(read) => {
                    replies.extend_from_slice(&chunk[..read]);
                    // Nothing here begins a reply, so this is somebody typing
                    // and the terminal is not going to answer. Stopping now
                    // costs the byte that said so; carrying on to the timeout
                    // would cost every key pressed until it expired.
                    if !replies.contains(&0x1b) {
                        break;
                    }
                    if answered(&replies) {
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        replies
    }
}

#[cfg(not(unix))]
mod platform {
    use std::time::Duration;

    use super::Ask;
    use crate::theme::system::TerminalColors;

    /// Windows consoles deliver these replies through the console input API
    /// rather than as bytes on a device, which is a different mechanism than
    /// the one above rather than a variation on it. Until that is written,
    /// saying "no answer" gets the documented fallback rather than a wrong
    /// palette.
    pub fn query(_ask: Ask, _timeout: Duration) -> TerminalColors {
        TerminalColors::UNKNOWN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_background_reply_is_read() {
        let colors = parse(b"\x1b]11;rgb:fafa/f9f9/f5f5\x1b\\");
        assert_eq!(colors.bg, Some(Rgb(0xfa, 0xf9, 0xf5)));
        assert_eq!(colors.fg, None);
    }

    #[test]
    fn both_terminators_are_accepted() {
        // Most terminals end with BEL, the standard says ST, and a palette is
        // not worth losing over which one arrived.
        let bel = parse(b"\x1b]11;rgb:0000/0000/0000\x07");
        let st = parse(b"\x1b]11;rgb:0000/0000/0000\x1b\\");
        assert_eq!(bel.bg, Some(Rgb(0, 0, 0)));
        assert_eq!(st.bg, bel.bg);
    }

    #[test]
    fn every_component_width_scales_to_full_intensity() {
        for spec in [
            "rgb:f/f/f",
            "rgb:ff/ff/ff",
            "rgb:fff/fff/fff",
            "rgb:ffff/ffff/ffff",
        ] {
            assert_eq!(color(spec), Some(Rgb(255, 255, 255)), "{spec}");
        }
        for spec in ["rgb:0/0/0", "rgb:00/00/00", "rgb:0000/0000/0000"] {
            assert_eq!(color(spec), Some(Rgb(0, 0, 0)), "{spec}");
        }
    }

    #[test]
    fn hash_forms_are_accepted() {
        assert_eq!(color("#ff0000"), Some(Rgb(255, 0, 0)));
        assert_eq!(color("#f00"), Some(Rgb(255, 0, 0)));
        assert_eq!(color("#ffff00000000"), Some(Rgb(255, 0, 0)));
    }

    #[test]
    fn an_alpha_prefix_does_not_lose_the_color() {
        assert_eq!(color("rgba:ffff/0000/0000/ffff"), Some(Rgb(255, 0, 0)));
    }

    #[test]
    fn a_malformed_specification_is_dropped_rather_than_guessed_at() {
        for spec in ["", "rgb:", "rgb:zz/zz/zz", "rgb:ff/ff", "#12345", "blue"] {
            assert_eq!(color(spec), None, "{spec}");
        }
    }

    #[test]
    fn slot_replies_land_in_their_slots() {
        let colors = parse(b"\x1b]4;1;rgb:abab/4646/4242\x07\x1b]4;9;rgb:ffff/6060/6060\x07");
        assert_eq!(colors.ansi[1], Some(Rgb(0xab, 0x46, 0x42)));
        assert_eq!(colors.ansi[9], Some(Rgb(0xff, 0x60, 0x60)));
        assert_eq!(colors.ansi[2], None);
    }

    #[test]
    fn replies_may_arrive_in_any_order_and_interleaved() {
        let colors = parse(
            b"\x1b]4;2;rgb:a1a1/b5b5/6c6c\x07\x1b]11;rgb:1818/1818/1818\x07\
              \x1b]10;rgb:d8d8/d8d8/d8d8\x1b\\",
        );
        assert_eq!(colors.bg, Some(Rgb(0x18, 0x18, 0x18)));
        assert_eq!(colors.fg, Some(Rgb(0xd8, 0xd8, 0xd8)));
        assert_eq!(colors.ansi[2], Some(Rgb(0xa1, 0xb5, 0x6c)));
    }

    #[test]
    fn a_slot_outside_the_sixteen_is_ignored_rather_than_panicking() {
        // 256-color terminals will answer about slot 200 if asked, and a
        // terminal is free to volunteer one.
        let colors = parse(b"\x1b]4;200;rgb:0000/0000/0000\x07");
        assert_eq!(colors, TerminalColors::UNKNOWN);
    }

    #[test]
    fn noise_yields_no_colors_rather_than_a_panic() {
        for noise in [
            &b""[..],
            b"\x1b",
            b"\x1b]",
            b"\x1b]11",
            b"\x1b]11;rgb:fafa/f9f9", // truncated mid-reply, no terminator
            b"hello world",
            b"\x1b]11;\x07",
            b"\xff\xfe\x1b]11;rgb:\x07",
        ] {
            assert_eq!(parse(noise), TerminalColors::UNKNOWN, "{noise:?}");
        }
    }

    #[test]
    fn a_device_attributes_reply_ends_the_wait() {
        assert!(answered(b"\x1b[?62;1;6;9;15;22c"));
        assert!(answered(b"\x1b[?1;2c"));
        assert!(answered(b"\x1b]11;rgb:0/0/0\x07\x1b[?6c"));
    }

    #[test]
    fn an_unfinished_device_attributes_reply_does_not_end_the_wait() {
        assert!(!answered(b"\x1b[?62;1;6"));
        assert!(!answered(b""));
        assert!(!answered(b"\x1b]11;rgb:0/0/0\x07"));
        // A cursor position report is also `CSI` with numbers, and is not it.
        assert!(!answered(b"\x1b[24;80R"));
    }

    #[test]
    fn keystrokes_are_not_mistaken_for_replies() {
        // What a reader typed before the first frame is not an answer, and
        // parsing it as one would invent a palette out of `jjq`.
        assert_eq!(parse(b"jjq"), TerminalColors::UNKNOWN);
    }

    #[test]
    fn the_request_asks_about_everything_and_ends_with_the_sentinel() {
        let request = request(Ask::Everything);
        assert!(find(&request, b"\x1b]10;?").is_some());
        assert!(find(&request, b"\x1b]11;?").is_some());
        assert!(find(&request, b"\x1b]4;0;?").is_some());
        assert!(find(&request, b"\x1b]4;15;?").is_some());
        assert!(find(&request, b"\x1b]4;16;?").is_none());
        assert!(request.ends_with(b"\x1b[c"), "the sentinel must go last");
    }

    #[test]
    fn a_background_probe_asks_two_questions_and_no_more() {
        // The whole point of the probe: a reader who alt-tabs all day pays two
        // sequences to learn that nothing changed, not nineteen.
        let probe = request(Ask::Background);
        assert!(find(&probe, b"\x1b]11;?").is_some());
        assert!(find(&probe, b"\x1b]10;?").is_none());
        assert!(find(&probe, b"\x1b]4;").is_none());
        assert!(probe.ends_with(b"\x1b[c"), "the sentinel must go last");
        assert!(probe.len() < request(Ask::Everything).len() / 4);
    }

    #[test]
    fn the_background_comes_first_in_both_shapes() {
        // A terminal that answers one question and then stops still has to
        // answer the one a change is detected from.
        for ask in [Ask::Everything, Ask::Background] {
            let bytes = request(ask);
            assert!(bytes.starts_with(b"\x1b]11;?"), "{ask:?}");
        }
    }
}
