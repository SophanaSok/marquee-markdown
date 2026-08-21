//! Building a palette out of the colors the terminal is already using.
//!
//! `--style system` is for a reader whose terminal already carries a
//! colorscheme and would rather not restate it in a theme file. The page is
//! still painted, the cards are still sealed, the column is still centered —
//! in their hues rather than Claude's.
//!
//! Everything here is a pure function of [`TerminalColors`], which is what the
//! terminal answered. Asking is [`crate::util::osc`]'s job, and keeping the
//! two apart is what lets every rule below be tested against a synthetic
//! answer with no terminal in sight.
//!
//! The rules are not a straight copy of what the terminal said. A real
//! colorscheme is chosen for *terminal output*, where a dim yellow on white is
//! a nuisance; here it would be a heading. So every color that ends up as text
//! passes a contrast floor against the page, and is walked toward the
//! foreground until it clears it.

use super::{Alerts, Appearance, Icons, Palette, Rgb, Theme, ThemeFile, ThemeVariant};

/// What the terminal said about its own colors.
///
/// Every field is separately optional: a terminal may answer `OSC 11` and
/// ignore `OSC 4`, and one that answers nothing is not an error — it is the
/// ordinary case for `screen`, for a dumb terminal, and for a pipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalColors {
    /// The default foreground, from `OSC 10`.
    pub fg: Option<Rgb>,
    /// The default background, from `OSC 11`.
    pub bg: Option<Rgb>,
    /// The sixteen ANSI slots, from `OSC 4`. Index is the slot number, so
    /// `ansi[1]` is red and `ansi[9]` is bright red.
    pub ansi: [Option<Rgb>; 16],
}

impl TerminalColors {
    /// A terminal that has not been asked, or that did not answer.
    pub const UNKNOWN: Self = Self {
        fg: None,
        bg: None,
        ansi: [None; 16],
    };

    /// Whether the background is a dark one, or `None` if it is not known.
    ///
    /// This is what `--style auto` has always wanted: the parameter existed
    /// from the start, and both callers passed `None`.
    #[must_use]
    pub fn is_dark(&self) -> Option<bool> {
        self.bg.map(|bg| luminance(bg) < 0.5)
    }
}

impl Default for TerminalColors {
    fn default() -> Self {
        Self::UNKNOWN
    }
}

/// The floor every color that ends up as text has to clear against the page.
///
/// 3.0 is WCAG's ratio for large text and interface components, which is what
/// headings, links, callout titles and list markers are. Body text uses the
/// terminal's own foreground, which is not ours to second-guess.
const MIN_CONTRAST: f64 = 3.0;

/// Below this, the terminal did not tell us anything usable.
///
/// A terminal that reports the same color for foreground and background — or
/// close enough — has an unset or broken palette rather than a low-contrast
/// one, and building a theme from it would produce an unreadable page. Saying
/// so lets the caller fall back to a palette that works.
const MIN_LEGIBLE: f64 = 2.0;

/// Build the `system` theme, or `None` if the terminal said too little.
///
/// `None` is the ordinary answer, not a failure: it means the caller should
/// use whatever `--style auto` would have picked.
#[must_use]
pub fn theme(colors: &TerminalColors) -> Option<Theme> {
    Some(Theme::from(theme_file(colors)?))
}

/// The same thing as a [`ThemeFile`], which is the shape a hand-written theme
/// has. Going through it rather than around it is deliberate: a theme the
/// terminal described and a theme somebody wrote are then the same kind of
/// thing, built by the same constructor.
fn theme_file(colors: &TerminalColors) -> Option<ThemeFile> {
    let (bg, fg) = (colors.bg?, colors.fg?);
    if contrast(bg, fg) < MIN_LEGIBLE {
        return None;
    }
    let dark = luminance(bg) < 0.5;
    let appearance = if dark {
        Appearance::Dark
    } else {
        Appearance::Light
    };
    let fallback = if dark { super::SLATE } else { super::PAPER };

    // ANSI 8 is what terminal colorschemes use for comments and dimmed text,
    // which is exactly what `muted` is for. When it is missing — or is the
    // same flat black as slot 0, which some schemes leave it as — a blend of
    // foreground into the page says the same thing.
    let muted = slot(colors, 0, dark)
        .filter(|c| contrast(bg, *c) >= MIN_CONTRAST)
        .unwrap_or_else(|| mix(fg, bg, 0.45));
    let muted = legible(muted, bg, fg);

    // Warm first, because a clay accent is what makes a rendered document read
    // as an artifact. But a light scheme's yellow on its own near-white page
    // is not a heading, so the first slot that actually clears the floor wins,
    // and the shipped accent is the backstop when none does.
    let accent = [3usize, 1, 5, 4]
        .into_iter()
        .filter_map(|index| slot(colors, index, dark))
        .find(|c| contrast(bg, *c) >= MIN_CONTRAST)
        .unwrap_or(fallback.accent);
    let accent = legible(accent, bg, fg);

    let alert =
        |index: usize, spare: Rgb| legible(slot(colors, index, dark).unwrap_or(spare), bg, fg);

    Some(ThemeFile {
        name: "system".to_owned(),
        appearance,
        syntax: fallback_syntax(dark),
        palette: Palette {
            bg,
            // Cards and table headers are raised off the page, not written on
            // it. A small step toward the foreground reads that way on a page
            // of any lightness, including pure black and pure white, where
            // darkening or lightening alone would do nothing.
            surface: separated(bg, fg, 0.06),
            fg,
            muted,
            accent,
            // The soft accent is the shipped palettes' accent desaturated, not
            // lightened: in both of them it moves toward the muted tone rather
            // than toward or away from the page.
            accent_soft: legible(mix(accent, muted, 0.4), bg, fg),
            border: separated(bg, fg, 0.15),
            alerts: Alerts {
                note: alert(4, fallback.alerts.note),
                tip: alert(2, fallback.alerts.tip),
                important: alert(5, fallback.alerts.important),
                warning: alert(3, fallback.alerts.warning),
                caution: alert(1, fallback.alerts.caution),
            },
        },
        // Glyph choice is a font question, and the terminal was asked about
        // colors. The defaults draw in any monospace font.
        icons: Icons::default(),
    })
}

/// The bundled syntect theme to highlight code with.
///
/// The same two the shipped palettes name. `highlight.rs` forces the theme's
/// surface as the background on every span, so this only decides the hues.
fn fallback_syntax(dark: bool) -> String {
    if dark {
        ThemeVariant::Slate.definition().syntax
    } else {
        ThemeVariant::Paper.definition().syntax
    }
}

/// One ANSI slot, preferring the bright variant on a dark page.
///
/// Colorschemes tune the normal slots to be legible on a light page and the
/// bright ones on a dark page; a terminal's own applications follow the same
/// convention. Either is accepted when only one was answered.
fn slot(colors: &TerminalColors, index: usize, dark: bool) -> Option<Rgb> {
    let (first, second) = if dark {
        (index + 8, index)
    } else {
        (index, index + 8)
    };
    colors
        .ansi
        .get(first)
        .copied()
        .flatten()
        .or_else(|| colors.ansi.get(second).copied().flatten())
}

/// Walk `color` toward `fg` until it clears the floor against `bg`.
///
/// Returning something readable matters more than returning exactly what the
/// terminal said: an unreadable heading is a bug, and a slightly-off hue is
/// not. `fg` is the destination because it is the one color the terminal has
/// already committed to being readable on this page.
fn legible(color: Rgb, bg: Rgb, fg: Rgb) -> Rgb {
    if contrast(bg, color) >= MIN_CONTRAST {
        return color;
    }
    // Twenty steps is finer than the eye resolves over this distance, and
    // ends at `fg` itself, which the caller has already checked is legible.
    (1..=20)
        .map(|step| mix(color, fg, f64::from(step) / 20.0))
        .find(|candidate| contrast(bg, *candidate) >= MIN_CONTRAST)
        .unwrap_or(fg)
}

/// `bg` stepped `amount` toward `fg`, guaranteed to differ from `bg`.
///
/// Rounding to eight bits can collapse a small step back onto the page color,
/// which would make code cards and table borders vanish. Stepping further
/// until it does not is cheaper than reasoning about when it would.
fn separated(bg: Rgb, fg: Rgb, amount: f64) -> Rgb {
    let mut amount = amount;
    while amount < 1.0 {
        let candidate = mix(bg, fg, amount);
        if candidate != bg {
            return candidate;
        }
        amount += 0.02;
    }
    fg
}

/// `from` moved `amount` of the way to `to`.
fn mix(from: Rgb, to: Rgb, amount: f64) -> Rgb {
    let amount = amount.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| {
        let blended = f64::from(a) + (f64::from(b) - f64::from(a)) * amount;
        // `round` then clamp: the arithmetic cannot leave 0..=255, but saying
        // so is what keeps the cast lossless rather than implementation
        // defined at the edges.
        blended.round().clamp(0.0, 255.0) as u8
    };
    Rgb(
        channel(from.0, to.0),
        channel(from.1, to.1),
        channel(from.2, to.2),
    )
}

/// WCAG relative luminance, 0.0 (black) to 1.0 (white).
fn luminance(color: Rgb) -> f64 {
    let channel = |value: u8| {
        let scaled = f64::from(value) / 255.0;
        if scaled <= 0.040_45 {
            scaled / 12.92
        } else {
            ((scaled + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.0) + 0.7152 * channel(color.1) + 0.0722 * channel(color.2)
}

/// WCAG contrast ratio between two colors, 1.0 (identical) to 21.0.
fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    let (lighter, darker) = if a > b { (a, b) } else { (b, a) };
    (lighter + 0.05) / (darker + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plausible dark colorscheme: near-black page, warm off-white text,
    /// both sets of ANSI slots.
    fn dark_terminal() -> TerminalColors {
        let mut colors = TerminalColors {
            fg: Some(Rgb(0xd8, 0xd8, 0xd8)),
            bg: Some(Rgb(0x18, 0x18, 0x18)),
            ..TerminalColors::UNKNOWN
        };
        let slots = [
            0x181818, 0xab4642, 0xa1b56c, 0xf7ca88, 0x7cafc2, 0xba8baf, 0x86c1b9, 0xd8d8d8,
            0x585858, 0xab4642, 0xa1b56c, 0xf7ca88, 0x7cafc2, 0xba8baf, 0x86c1b9, 0xf8f8f8,
        ];
        for (index, value) in slots.into_iter().enumerate() {
            colors.ansi[index] = Some(Rgb(
                (value >> 16) as u8,
                ((value >> 8) & 0xff) as u8,
                (value & 0xff) as u8,
            ));
        }
        colors
    }

    /// The same scheme inverted: a near-white page, which is where a naive
    /// mapping puts an illegible yellow on a heading.
    fn light_terminal() -> TerminalColors {
        let mut colors = dark_terminal();
        colors.fg = Some(Rgb(0x18, 0x18, 0x18));
        colors.bg = Some(Rgb(0xf8, 0xf8, 0xf8));
        colors
    }

    #[test]
    fn a_dark_background_yields_a_dark_theme() {
        let theme = theme(&dark_terminal()).expect("theme");
        assert_eq!(theme.name, "system");
        assert_eq!(theme.appearance, Appearance::Dark);
        assert_eq!(theme.syntax, ThemeVariant::Slate.definition().syntax);
    }

    #[test]
    fn a_light_background_yields_a_light_theme() {
        let theme = theme(&light_terminal()).expect("theme");
        assert_eq!(theme.appearance, Appearance::Light);
        assert_eq!(theme.syntax, ThemeVariant::Paper.definition().syntax);
    }

    #[test]
    fn the_page_and_the_text_are_the_terminals_own() {
        // The whole point: what the terminal said about its page and its text
        // is used verbatim, not approximated.
        let theme = theme(&dark_terminal()).expect("theme");
        assert_eq!(theme.palette.bg, Rgb(0x18, 0x18, 0x18));
        assert_eq!(theme.palette.fg, Rgb(0xd8, 0xd8, 0xd8));
    }

    #[test]
    fn the_page_and_the_surface_are_separate() {
        // The same invariant `both_palettes_separate_page_from_surface` holds
        // the shipped palettes to. A card that matches the page is not a card.
        for colors in [dark_terminal(), light_terminal()] {
            let palette = theme(&colors).expect("theme").palette;
            assert_ne!(palette.surface, palette.bg);
            assert_ne!(palette.border, palette.bg);
        }
    }

    #[test]
    fn a_page_that_leaves_no_room_still_separates_from_its_surface() {
        // Pure black and pure white are where "darken the page a little" and
        // "lighten the page a little" each do nothing at all.
        for (bg, fg) in [
            (Rgb(0, 0, 0), Rgb(0xff, 0xff, 0xff)),
            (Rgb(0xff, 0xff, 0xff), Rgb(0, 0, 0)),
        ] {
            let colors = TerminalColors {
                fg: Some(fg),
                bg: Some(bg),
                ..TerminalColors::UNKNOWN
            };
            let palette = theme(&colors).expect("theme").palette;
            assert_ne!(palette.surface, bg, "surface collapsed onto {bg}");
            assert_ne!(palette.border, bg, "border collapsed onto {bg}");
        }
    }

    #[test]
    fn every_color_that_becomes_text_is_readable_on_the_page() {
        for colors in [dark_terminal(), light_terminal()] {
            let palette = theme(&colors).expect("theme").palette;
            let named = [
                ("muted", palette.muted),
                ("accent", palette.accent),
                ("accent_soft", palette.accent_soft),
                ("note", palette.alerts.note),
                ("tip", palette.alerts.tip),
                ("important", palette.alerts.important),
                ("warning", palette.alerts.warning),
                ("caution", palette.alerts.caution),
            ];
            let bg = palette.bg;
            for (name, color) in named {
                let ratio = contrast(bg, color);
                assert!(
                    ratio >= MIN_CONTRAST,
                    "{name} is {color} on {bg}, ratio {ratio:.2}"
                );
            }
        }
    }

    #[test]
    fn a_scheme_whose_slots_are_all_the_page_color_is_still_readable() {
        // The pathological answer: a terminal that reports every ANSI slot as
        // its own background. Naively that paints invisible headings.
        let colors = TerminalColors {
            fg: Some(Rgb(0xff, 0xff, 0xff)),
            bg: Some(Rgb(0, 0, 0)),
            ansi: [Some(Rgb(0, 0, 0)); 16],
        };
        let palette = theme(&colors).expect("theme").palette;
        for color in [palette.muted, palette.accent, palette.alerts.warning] {
            assert!(contrast(palette.bg, color) >= MIN_CONTRAST, "{color}");
        }
    }

    #[test]
    fn a_terminal_that_answered_only_about_the_page_still_gets_a_theme() {
        // Answering OSC 10 and 11 but not OSC 4 is common; the shipped hues
        // fill in the rest rather than the whole thing being refused.
        let colors = TerminalColors {
            fg: Some(Rgb(0xf5, 0xf4, 0xef)),
            bg: Some(Rgb(0x26, 0x26, 0x24)),
            ..TerminalColors::UNKNOWN
        };
        let theme = theme(&colors).expect("theme");
        assert_eq!(theme.palette.bg, Rgb(0x26, 0x26, 0x24));
        assert!(contrast(theme.palette.bg, theme.palette.accent) >= MIN_CONTRAST);
    }

    #[test]
    fn a_terminal_that_said_nothing_gets_no_theme() {
        assert!(theme(&TerminalColors::UNKNOWN).is_none());
    }

    #[test]
    fn half_an_answer_is_not_enough() {
        // A page color with no text color cannot say what "muted" means.
        let colors = TerminalColors {
            bg: Some(Rgb(0x18, 0x18, 0x18)),
            ..TerminalColors::UNKNOWN
        };
        assert!(theme(&colors).is_none());
    }

    #[test]
    fn a_terminal_that_reports_the_same_color_twice_gets_no_theme() {
        // An unset palette rather than a low-contrast one. Building on it
        // would produce a page with nothing legible on it.
        let colors = TerminalColors {
            fg: Some(Rgb(0x18, 0x18, 0x18)),
            bg: Some(Rgb(0x18, 0x18, 0x18)),
            ..TerminalColors::UNKNOWN
        };
        assert!(theme(&colors).is_none());
    }

    #[test]
    fn darkness_follows_the_background() {
        assert_eq!(dark_terminal().is_dark(), Some(true));
        assert_eq!(light_terminal().is_dark(), Some(false));
        assert_eq!(TerminalColors::UNKNOWN.is_dark(), None);
    }

    #[test]
    fn a_dark_page_prefers_the_bright_slot_and_a_light_page_the_normal_one() {
        let mut colors = dark_terminal();
        colors.ansi[1] = Some(Rgb(0x40, 0x00, 0x00)); // dim red
        colors.ansi[9] = Some(Rgb(0xff, 0x60, 0x60)); // bright red
        assert_eq!(slot(&colors, 1, true), Some(Rgb(0xff, 0x60, 0x60)));
        assert_eq!(slot(&colors, 1, false), Some(Rgb(0x40, 0x00, 0x00)));
    }

    #[test]
    fn a_missing_slot_falls_back_to_its_counterpart() {
        let mut colors = dark_terminal();
        colors.ansi[9] = None;
        assert_eq!(slot(&colors, 1, true), colors.ansi[1]);
        colors.ansi[1] = None;
        assert_eq!(slot(&colors, 1, true), None);
    }

    #[test]
    fn mixing_ends_where_it_says_it_does() {
        let (a, b) = (Rgb(0, 0, 0), Rgb(255, 255, 255));
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, 0.5), Rgb(128, 128, 128));
        // Out of range is clamped rather than wrapping past either end.
        assert_eq!(mix(a, b, -1.0), a);
        assert_eq!(mix(a, b, 2.0), b);
    }

    #[test]
    fn contrast_matches_the_published_extremes() {
        let (black, white) = (Rgb(0, 0, 0), Rgb(255, 255, 255));
        assert!((contrast(black, white) - 21.0).abs() < 0.01);
        assert!((contrast(white, white) - 1.0).abs() < 0.01);
    }

    #[test]
    fn a_color_that_already_clears_the_floor_is_left_alone() {
        let (bg, fg) = (Rgb(0, 0, 0), Rgb(255, 255, 255));
        let accent = Rgb(0xf7, 0xca, 0x88);
        assert_eq!(legible(accent, bg, fg), accent);
    }
}
