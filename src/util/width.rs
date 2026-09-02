//! Content width rules.
//!
//! Matching glow: when a width is not given explicitly, use the terminal width
//! capped at a readable maximum, or a fixed default when output is redirected.

/// Widest content column chosen automatically. Beyond this, prose becomes hard
/// to track from line to line.
pub const AUTO_MAX: u16 = 120;
/// Width used when output is not a terminal.
pub const NON_TTY_DEFAULT: u16 = 80;
/// Narrowest column we will lay out at.
pub const MIN: u16 = 10;
/// Widest column we will lay out at, and the column "do not wrap" renders at.
///
/// Every rendered line is padded to exactly the content width, so the width
/// bounds the output size directly: an uncapped `-w 65535` turns a 33 KB
/// document into 48 MB of mostly spaces. A thousand columns is wider than any
/// prose line while keeping the padding proportionate.
pub const MAX: u16 = 1000;

/// Resolve the content width.
///
/// `requested` is the `-w` flag: `Some(0)` disables wrapping (rendering at the
/// widest column allowed), `Some(n)` pins the width, `None` derives it.
#[must_use]
pub fn resolve(requested: Option<u16>, terminal: Option<u16>) -> u16 {
    match requested {
        // 0 means "do not wrap"; approximate with the widest column allowed.
        Some(0) => MAX,
        Some(n) => n.clamp(MIN, MAX),
        None => match terminal {
            Some(cols) => cols.clamp(MIN, AUTO_MAX),
            None => NON_TTY_DEFAULT,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_width_wins() {
        assert_eq!(resolve(Some(60), Some(200)), 60);
    }

    #[test]
    fn explicit_width_is_clamped_to_a_usable_minimum() {
        assert_eq!(resolve(Some(1), Some(200)), MIN);
    }

    #[test]
    fn zero_disables_wrapping() {
        assert!(resolve(Some(0), Some(80)) > AUTO_MAX);
    }

    #[test]
    fn an_absurd_explicit_width_is_capped() {
        // Output scales with the width — every line is padded to it — so a
        // width nothing displays must not multiply the document by a
        // thousand.
        assert_eq!(resolve(Some(u16::MAX), None), MAX);
        assert_eq!(resolve(Some(u16::MAX), Some(80)), MAX);
    }

    #[test]
    fn terminal_width_is_capped() {
        assert_eq!(resolve(None, Some(300)), AUTO_MAX);
        assert_eq!(resolve(None, Some(90)), 90);
    }

    #[test]
    fn redirected_output_uses_the_fixed_default() {
        assert_eq!(resolve(None, None), NON_TTY_DEFAULT);
    }

    #[test]
    fn a_tiny_terminal_still_yields_a_layoutable_width() {
        assert_eq!(resolve(None, Some(3)), MIN);
    }
}
