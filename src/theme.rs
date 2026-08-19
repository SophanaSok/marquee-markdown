//! Claude-artifact color palettes and the style tokens the renderer draws with.
//!
//! Two true-color palettes ship with the reader: [`ThemeVariant::Paper`] (light,
//! warm off-white) and [`ThemeVariant::Slate`] (dark, warm near-black). Both use
//! the same clay accent, which is what makes a rendered document read as an
//! artifact rather than as terminal output.
//!
//! Every renderer reads styles from [`Theme`] rather than naming colors directly,
//! so adding a palette never means touching layout code.

use std::fmt;
use std::str::FromStr;

use ratatui::style::{Color, Modifier, Style};

/// Which palette to draw with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeVariant {
    /// Warm off-white page — Claude's light artifact surface.
    #[default]
    Paper,
    /// Warm near-black page — Claude's dark artifact surface.
    Slate,
}

impl ThemeVariant {
    /// The other variant, for the runtime light/dark toggle.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Paper => Self::Slate,
            Self::Slate => Self::Paper,
        }
    }

    /// Stable lowercase name, as accepted by `--style` and written in config.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Slate => "slate",
        }
    }

    /// Every selectable variant, for `--style` validation and help text.
    #[must_use]
    pub const fn all() -> [Self; 2] {
        [Self::Paper, Self::Slate]
    }
}

impl fmt::Display for ThemeVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for ThemeVariant {
    type Err = UnknownTheme;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "paper" | "light" => Ok(Self::Paper),
            "slate" | "dark" => Ok(Self::Slate),
            other => Err(UnknownTheme(other.to_owned())),
        }
    }
}

/// `--style` was given a name that is not a known palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownTheme(pub String);

impl fmt::Display for UnknownTheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown style {:?} (expected one of: paper, slate)",
            self.0
        )
    }
}

impl std::error::Error for UnknownTheme {}

/// The raw colors behind a variant.
///
/// `bg` is painted edge to edge, including the gutters either side of the
/// reading column; `surface` is the raised fill used for code blocks, inline
/// code chips and table headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub bg: Color,
    pub surface: Color,
    pub fg: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub border: Color,
    /// Callout hues, warm-shifted to sit alongside the clay accent.
    pub info: Color,
    pub success: Color,
    pub important: Color,
    pub warning: Color,
    pub danger: Color,
}

/// Claude's light artifact palette.
pub const PAPER: Palette = Palette {
    bg: Color::Rgb(0xfa, 0xf9, 0xf5),
    surface: Color::Rgb(0xf0, 0xee, 0xe6),
    fg: Color::Rgb(0x14, 0x14, 0x13),
    muted: Color::Rgb(0x6f, 0x6e, 0x69),
    accent: Color::Rgb(0xd9, 0x77, 0x57),
    accent_soft: Color::Rgb(0xd4, 0xa2, 0x7f),
    border: Color::Rgb(0xe5, 0xe4, 0xdf),
    info: Color::Rgb(0x5a, 0x7d, 0x9a),
    success: Color::Rgb(0x5c, 0x8a, 0x5c),
    important: Color::Rgb(0x8a, 0x6a, 0x9a),
    warning: Color::Rgb(0xb8, 0x86, 0x2b),
    danger: Color::Rgb(0xb4, 0x48, 0x3c),
};

/// Claude's dark artifact palette.
pub const SLATE: Palette = Palette {
    bg: Color::Rgb(0x26, 0x26, 0x24),
    surface: Color::Rgb(0x1f, 0x1e, 0x1d),
    fg: Color::Rgb(0xf5, 0xf4, 0xef),
    muted: Color::Rgb(0x87, 0x86, 0x7f),
    accent: Color::Rgb(0xd9, 0x77, 0x57),
    accent_soft: Color::Rgb(0xd4, 0xa2, 0x7f),
    border: Color::Rgb(0x3d, 0x3d, 0x3a),
    info: Color::Rgb(0x7e, 0xa3, 0xc4),
    success: Color::Rgb(0x85, 0xb0, 0x85),
    important: Color::Rgb(0xb0, 0x94, 0xc0),
    warning: Color::Rgb(0xd9, 0xa4, 0x41),
    danger: Color::Rgb(0xdd, 0x7a, 0x6d),
};

/// A resolved palette plus the styles every renderer draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub variant: ThemeVariant,
    pub palette: Palette,
}

impl Default for Theme {
    fn default() -> Self {
        Self::new(ThemeVariant::default())
    }
}

impl Theme {
    #[must_use]
    pub const fn new(variant: ThemeVariant) -> Self {
        let palette = match variant {
            ThemeVariant::Paper => PAPER,
            ThemeVariant::Slate => SLATE,
        };
        Self { variant, palette }
    }

    /// Page fill, painted across the full terminal width including the gutters
    /// either side of the reading column.
    #[must_use]
    pub fn page(self) -> Style {
        Style::new().bg(self.palette.bg).fg(self.palette.fg)
    }

    /// Body prose.
    #[must_use]
    pub fn body(self) -> Style {
        self.page()
    }

    /// Secondary text: captions, the status bar, dimmed metadata.
    #[must_use]
    pub fn muted(self) -> Style {
        Style::new().bg(self.palette.bg).fg(self.palette.muted)
    }

    /// Heading style for levels 1-6. Hierarchy comes from weight and color;
    /// vertical rhythm is applied by the layout pass, not here.
    #[must_use]
    pub fn heading(self, level: u8) -> Style {
        let base = Style::new()
            .bg(self.palette.bg)
            .add_modifier(Modifier::BOLD);
        match level {
            1 => base.fg(self.palette.accent),
            2 | 3 => base.fg(self.palette.fg),
            _ => base.fg(self.palette.muted),
        }
    }

    /// Hairline rules: under H1/H2, and thematic breaks.
    #[must_use]
    pub fn rule(self) -> Style {
        Style::new().bg(self.palette.bg).fg(self.palette.border)
    }

    /// The border of a fenced code container.
    #[must_use]
    pub fn code_border(self) -> Style {
        Style::new().bg(self.palette.bg).fg(self.palette.border)
    }

    /// The language label sitting in the code container's top border.
    #[must_use]
    pub fn code_label(self) -> Style {
        Style::new().bg(self.palette.bg).fg(self.palette.muted)
    }

    /// Fill inside a code container, for cells syntax highlighting leaves bare.
    #[must_use]
    pub fn code_fill(self) -> Style {
        Style::new().bg(self.palette.surface).fg(self.palette.fg)
    }

    /// Inline code chip.
    #[must_use]
    pub fn inline_code(self) -> Style {
        Style::new()
            .bg(self.palette.surface)
            .fg(self.palette.accent)
    }

    /// The vertical bar in a blockquote's gutter.
    #[must_use]
    pub fn quote_bar(self) -> Style {
        Style::new()
            .bg(self.palette.bg)
            .fg(self.palette.accent_soft)
    }

    /// Blockquote body text.
    #[must_use]
    pub fn quote_text(self) -> Style {
        Style::new()
            .bg(self.palette.bg)
            .fg(self.palette.muted)
            .add_modifier(Modifier::ITALIC)
    }

    /// Bar and title color for a GFM alert callout.
    #[must_use]
    pub fn alert(self, kind: AlertKind) -> Style {
        let fg = match kind {
            AlertKind::Note => self.palette.info,
            AlertKind::Tip => self.palette.success,
            AlertKind::Important => self.palette.important,
            AlertKind::Warning => self.palette.warning,
            AlertKind::Caution => self.palette.danger,
        };
        Style::new()
            .bg(self.palette.bg)
            .fg(fg)
            .add_modifier(Modifier::BOLD)
    }

    /// Link text. The URL itself is emitted as an OSC 8 hyperlink where the
    /// output path supports it.
    #[must_use]
    pub fn link(self) -> Style {
        Style::new()
            .bg(self.palette.bg)
            .fg(self.palette.accent)
            .add_modifier(Modifier::UNDERLINED)
    }

    /// Table frame.
    #[must_use]
    pub fn table_border(self) -> Style {
        Style::new().bg(self.palette.bg).fg(self.palette.border)
    }

    /// Table header cells, raised onto the surface fill.
    #[must_use]
    pub fn table_header(self) -> Style {
        Style::new()
            .bg(self.palette.surface)
            .fg(self.palette.fg)
            .add_modifier(Modifier::BOLD)
    }

    /// List markers (bullets, ordered numerals, task checkboxes).
    #[must_use]
    pub fn list_marker(self) -> Style {
        Style::new()
            .bg(self.palette.bg)
            .fg(self.palette.accent_soft)
    }

    /// A search hit in the document body.
    #[must_use]
    pub fn search_match(self) -> Style {
        Style::new()
            .bg(self.palette.accent_soft)
            .fg(self.palette.bg)
    }

    /// The search hit the cursor is currently on.
    #[must_use]
    pub fn search_current(self) -> Style {
        Style::new()
            .bg(self.palette.accent)
            .fg(self.palette.bg)
            .add_modifier(Modifier::BOLD)
    }

    /// A table-of-contents entry that is not the active section.
    #[must_use]
    pub fn toc_item(self) -> Style {
        Style::new().bg(self.palette.bg).fg(self.palette.muted)
    }

    /// The table-of-contents entry matching the current scroll position.
    #[must_use]
    pub fn toc_active(self) -> Style {
        Style::new()
            .bg(self.palette.bg)
            .fg(self.palette.accent)
            .add_modifier(Modifier::BOLD)
    }

    /// Name of the bundled syntect theme paired with this palette.
    ///
    /// Chosen for contrast against the surface fill: a light syntax theme on
    /// paper, a dark one on slate.
    #[must_use]
    pub fn syntax_theme_name(self) -> &'static str {
        match self.variant {
            ThemeVariant::Paper => "InspiredGitHub",
            ThemeVariant::Slate => "base16-eighties.dark",
        }
    }

    /// The vertical hairline separating the sidebar from the document.
    #[must_use]
    pub fn sidebar_divider(self) -> Style {
        Style::new().bg(self.palette.bg).fg(self.palette.border)
    }
}

/// The five GitHub-flavored alert kinds, parsed from `> [!NOTE]`-style markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl AlertKind {
    /// Nerd Font glyph shown at the head of the callout.
    #[must_use]
    pub const fn icon(self) -> char {
        match self {
            Self::Note => '\u{f05a}',
            Self::Tip => '\u{f0eb}',
            Self::Important => '\u{f06a}',
            Self::Warning => '\u{f071}',
            Self::Caution => '\u{f06d}',
        }
    }

    /// Title rendered beside the icon.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::Tip => "Tip",
            Self::Important => "Important",
            Self::Warning => "Warning",
            Self::Caution => "Caution",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_round_trips_through_its_name() {
        for variant in ThemeVariant::all() {
            assert_eq!(variant.name().parse::<ThemeVariant>().unwrap(), variant);
        }
    }

    #[test]
    fn light_and_dark_are_accepted_as_aliases() {
        assert_eq!(
            "light".parse::<ThemeVariant>().unwrap(),
            ThemeVariant::Paper
        );
        assert_eq!("DARK".parse::<ThemeVariant>().unwrap(), ThemeVariant::Slate);
    }

    #[test]
    fn unknown_style_names_are_rejected_with_the_valid_set() {
        let err = "dracula".parse::<ThemeVariant>().unwrap_err();
        assert!(err.to_string().contains("paper"), "{err}");
    }

    #[test]
    fn toggling_twice_is_identity() {
        for variant in ThemeVariant::all() {
            assert_eq!(variant.toggled().toggled(), variant);
        }
    }

    #[test]
    fn both_palettes_separate_page_from_surface() {
        // The code container and table header only read as raised if the two
        // fills actually differ.
        for variant in ThemeVariant::all() {
            let theme = Theme::new(variant);
            assert_ne!(theme.palette.bg, theme.palette.surface, "{variant}");
        }
    }

    #[test]
    fn every_style_paints_a_background() {
        // Any span that leaves bg unset would punch a hole in the painted page.
        let theme = Theme::new(ThemeVariant::Paper);
        let styles = [
            theme.body(),
            theme.muted(),
            theme.heading(1),
            theme.heading(6),
            theme.rule(),
            theme.code_border(),
            theme.code_label(),
            theme.code_fill(),
            theme.inline_code(),
            theme.quote_bar(),
            theme.quote_text(),
            theme.link(),
            theme.table_border(),
            theme.table_header(),
            theme.list_marker(),
            theme.search_match(),
            theme.search_current(),
            theme.toc_item(),
            theme.toc_active(),
            theme.sidebar_divider(),
        ];
        for style in styles {
            assert!(style.bg.is_some(), "style without a background: {style:?}");
        }
    }

    #[test]
    fn headings_lose_prominence_as_level_increases() {
        let theme = Theme::new(ThemeVariant::Slate);
        assert_eq!(theme.heading(1).fg, Some(theme.palette.accent));
        assert_eq!(theme.heading(2).fg, Some(theme.palette.fg));
        assert_eq!(theme.heading(4).fg, Some(theme.palette.muted));
    }

    #[test]
    fn alert_kinds_have_distinct_colors() {
        let theme = Theme::new(ThemeVariant::Paper);
        let kinds = [
            AlertKind::Note,
            AlertKind::Tip,
            AlertKind::Important,
            AlertKind::Warning,
            AlertKind::Caution,
        ];
        let mut seen = Vec::new();
        for kind in kinds {
            let fg = theme.alert(kind).fg.unwrap();
            assert!(!seen.contains(&fg), "{kind:?} reuses another alert color");
            seen.push(fg);
        }
    }
}
