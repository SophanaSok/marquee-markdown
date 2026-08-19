//! Claude-artifact color palettes and the style tokens the renderer draws with.
//!
//! Two true-color palettes ship with the reader: [`ThemeVariant::Paper`] (light,
//! warm off-white) and [`ThemeVariant::Slate`] (dark, warm near-black). Both use
//! the same clay accent, which is what makes a rendered document read as an
//! artifact rather than as terminal output.
//!
//! Every renderer reads styles from [`Theme`] rather than naming colors directly,
//! so adding a palette never means touching layout code.

pub mod palette;
pub mod registry;

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use ratatui::style::{Modifier, Style};

pub use palette::{Alerts, Appearance, Palette, Rgb, ThemeFile};

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

    /// The compiled-in definition for this variant.
    #[must_use]
    pub fn definition(self) -> ThemeFile {
        match self {
            Self::Paper => ThemeFile {
                name: "paper".to_owned(),
                appearance: Appearance::Light,
                syntax: "InspiredGitHub".to_owned(),
                palette: PAPER,
            },
            Self::Slate => ThemeFile {
                name: "slate".to_owned(),
                appearance: Appearance::Dark,
                syntax: "base16-eighties.dark".to_owned(),
                palette: SLATE,
            },
        }
    }
}

impl From<ThemeFile> for Theme {
    fn from(file: ThemeFile) -> Self {
        Self {
            name: file.name,
            appearance: file.appearance,
            syntax: file.syntax,
            palette: file.palette,
            plain: false,
        }
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

/// Claude's light artifact palette.
pub const PAPER: Palette = Palette {
    bg: Rgb(0xfa, 0xf9, 0xf5),
    surface: Rgb(0xf0, 0xee, 0xe6),
    fg: Rgb(0x14, 0x14, 0x13),
    muted: Rgb(0x6f, 0x6e, 0x69),
    accent: Rgb(0xd9, 0x77, 0x57),
    accent_soft: Rgb(0xd4, 0xa2, 0x7f),
    border: Rgb(0xe5, 0xe4, 0xdf),
    alerts: Alerts {
        note: Rgb(0x5a, 0x7d, 0x9a),
        tip: Rgb(0x5c, 0x8a, 0x5c),
        important: Rgb(0x8a, 0x6a, 0x9a),
        warning: Rgb(0xb8, 0x86, 0x2b),
        caution: Rgb(0xb4, 0x48, 0x3c),
    },
};

/// Claude's dark artifact palette.
pub const SLATE: Palette = Palette {
    bg: Rgb(0x26, 0x26, 0x24),
    surface: Rgb(0x1f, 0x1e, 0x1d),
    fg: Rgb(0xf5, 0xf4, 0xef),
    muted: Rgb(0x87, 0x86, 0x7f),
    accent: Rgb(0xd9, 0x77, 0x57),
    accent_soft: Rgb(0xd4, 0xa2, 0x7f),
    border: Rgb(0x3d, 0x3d, 0x3a),
    alerts: Alerts {
        note: Rgb(0x7e, 0xa3, 0xc4),
        tip: Rgb(0x85, 0xb0, 0x85),
        important: Rgb(0xb0, 0x94, 0xc0),
        warning: Rgb(0xd9, 0xa4, 0x41),
        caution: Rgb(0xdd, 0x7a, 0x6d),
    },
};

/// A resolved palette plus the styles every renderer draws with.
///
/// Built with [`Theme::new`] for a built-in variant or [`Theme::from_file`]
/// for a user-authored theme; both produce the same thing, so a file theme is
/// never a second-class citizen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Theme name, as selected by `--style`.
    pub name: String,
    /// Whether this palette targets a light or dark terminal.
    pub appearance: Appearance,
    /// Bundled syntect theme used for code blocks.
    pub syntax: String,
    pub palette: Palette,
    /// Set when styling is disabled entirely (piped output).
    pub plain: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self::new(ThemeVariant::default())
    }
}

impl Theme {
    /// Build one of the compiled-in themes.
    #[must_use]
    pub fn new(variant: ThemeVariant) -> Self {
        Self::from(variant.definition())
    }

    /// Load a theme from a TOML file.
    ///
    /// # Errors
    /// Returns an error when the file cannot be read or does not parse as a
    /// theme definition.
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        use anyhow::Context;
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read theme {}", path.display()))?;
        let file: ThemeFile = toml::from_str(&text)
            .with_context(|| format!("cannot parse theme {}", path.display()))?;
        Ok(Self::from(file))
    }

    /// A theme that emits no color at all, for redirected output.
    #[must_use]
    pub fn plain() -> Self {
        let mut theme = Self::new(ThemeVariant::Slate);
        theme.name = "notty".to_owned();
        theme.plain = true;
        theme
    }

    /// Page fill, painted across the full terminal width including the gutters
    /// either side of the reading column.
    #[must_use]
    pub fn page(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.fg.color())
    }

    /// Body prose.
    #[must_use]
    pub fn body(&self) -> Style {
        self.page()
    }

    /// Secondary text: captions, the status bar, dimmed metadata.
    #[must_use]
    pub fn muted(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.muted.color())
    }

    /// Heading style for levels 1-6. Hierarchy comes from weight and color;
    /// vertical rhythm is applied by the layout pass, not here.
    #[must_use]
    pub fn heading(&self, level: u8) -> Style {
        let base = Style::new()
            .bg(self.palette.bg.color())
            .add_modifier(Modifier::BOLD);
        match level {
            1 => base.fg(self.palette.accent.color()),
            2 | 3 => base.fg(self.palette.fg.color()),
            _ => base.fg(self.palette.muted.color()),
        }
    }

    /// Hairline rules: under H1/H2, and thematic breaks.
    #[must_use]
    pub fn rule(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.border.color())
    }

    /// The border of a fenced code container.
    #[must_use]
    pub fn code_border(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.border.color())
    }

    /// The language label sitting in the code container's top border.
    #[must_use]
    pub fn code_label(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.muted.color())
    }

    /// Fill inside a code container, for cells syntax highlighting leaves bare.
    #[must_use]
    pub fn code_fill(&self) -> Style {
        Style::new()
            .bg(self.palette.surface.color())
            .fg(self.palette.fg.color())
    }

    /// Inline code chip.
    #[must_use]
    pub fn inline_code(&self) -> Style {
        Style::new()
            .bg(self.palette.surface.color())
            .fg(self.palette.accent.color())
    }

    /// The vertical bar in a blockquote's gutter.
    #[must_use]
    pub fn quote_bar(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.accent_soft.color())
    }

    /// Blockquote body text.
    #[must_use]
    pub fn quote_text(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.muted.color())
            .add_modifier(Modifier::ITALIC)
    }

    /// Bar and title color for a GFM alert callout.
    #[must_use]
    pub fn alert(&self, kind: AlertKind) -> Style {
        let fg = match kind {
            AlertKind::Note => self.palette.alerts.note.color(),
            AlertKind::Tip => self.palette.alerts.tip.color(),
            AlertKind::Important => self.palette.alerts.important.color(),
            AlertKind::Warning => self.palette.alerts.warning.color(),
            AlertKind::Caution => self.palette.alerts.caution.color(),
        };
        Style::new()
            .bg(self.palette.bg.color())
            .fg(fg)
            .add_modifier(Modifier::BOLD)
    }

    /// Link text. The URL itself is emitted as an OSC 8 hyperlink where the
    /// output path supports it.
    #[must_use]
    pub fn link(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.accent.color())
            .add_modifier(Modifier::UNDERLINED)
    }

    /// Table frame.
    #[must_use]
    pub fn table_border(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.border.color())
    }

    /// Table header cells, raised onto the surface fill.
    #[must_use]
    pub fn table_header(&self) -> Style {
        Style::new()
            .bg(self.palette.surface.color())
            .fg(self.palette.fg.color())
            .add_modifier(Modifier::BOLD)
    }

    /// List markers (bullets, ordered numerals, task checkboxes).
    #[must_use]
    pub fn list_marker(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.accent_soft.color())
    }

    /// A search hit in the document body.
    #[must_use]
    pub fn search_match(&self) -> Style {
        Style::new()
            .bg(self.palette.accent_soft.color())
            .fg(self.palette.bg.color())
    }

    /// The search hit the cursor is currently on.
    #[must_use]
    pub fn search_current(&self) -> Style {
        Style::new()
            .bg(self.palette.accent.color())
            .fg(self.palette.bg.color())
            .add_modifier(Modifier::BOLD)
    }

    /// A row of a list pane — the contents pane or the file browser — that is
    /// not picked out in any way.
    #[must_use]
    pub fn list_item(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.muted.color())
    }

    /// The table-of-contents entry matching the current scroll position.
    #[must_use]
    pub fn toc_active(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.accent.color())
            .add_modifier(Modifier::BOLD)
    }

    /// The status bar along the bottom of the reader.
    #[must_use]
    pub fn status_bar(&self) -> Style {
        Style::new()
            .bg(self.palette.surface.color())
            .fg(self.palette.muted.color())
    }

    /// The part of the status bar that says where the reader is: the document
    /// name and the section being read.
    #[must_use]
    pub fn status_active(&self) -> Style {
        Style::new()
            .bg(self.palette.surface.color())
            .fg(self.palette.fg.color())
            .add_modifier(Modifier::BOLD)
    }

    /// A transient message in the status bar.
    #[must_use]
    pub fn status_message(&self) -> Style {
        Style::new()
            .bg(self.palette.surface.color())
            .fg(self.palette.accent.color())
    }

    /// The frame around an overlay panel such as the key reference.
    #[must_use]
    pub fn overlay_border(&self) -> Style {
        Style::new()
            .bg(self.palette.surface.color())
            .fg(self.palette.border.color())
    }

    /// The interior of an overlay panel.
    #[must_use]
    pub fn overlay_body(&self) -> Style {
        Style::new()
            .bg(self.palette.surface.color())
            .fg(self.palette.fg.color())
    }

    /// A key name inside the key reference.
    #[must_use]
    pub fn overlay_key(&self) -> Style {
        Style::new()
            .bg(self.palette.surface.color())
            .fg(self.palette.accent.color())
            .add_modifier(Modifier::BOLD)
    }

    /// The title of an overlay panel.
    #[must_use]
    pub fn overlay_title(&self) -> Style {
        Style::new()
            .bg(self.palette.surface.color())
            .fg(self.palette.accent.color())
            .add_modifier(Modifier::BOLD)
    }

    /// The row the cursor is on. Distinct from [`Self::toc_active`]: the
    /// cursor is where the reader put it, the active entry is where the
    /// document is scrolled to, and they are often different rows.
    #[must_use]
    pub fn list_cursor(&self) -> Style {
        Style::new()
            .bg(self.palette.surface.color())
            .fg(self.palette.fg.color())
    }

    /// The bar marking the cursor row while its pane has focus.
    #[must_use]
    pub fn list_cursor_marker(&self) -> Style {
        Style::new()
            .bg(self.palette.surface.color())
            .fg(self.palette.accent.color())
    }

    /// The fold marker in front of a contents entry with children.
    #[must_use]
    pub fn toc_fold(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.muted.color())
    }

    /// Secondary text in a list row, such as a file's age.
    #[must_use]
    pub fn list_meta(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.muted.color())
    }

    /// Name of the bundled syntect theme paired with this palette.
    #[must_use]
    pub fn syntax_theme_name(&self) -> &str {
        &self.syntax
    }

    /// The vertical hairline separating the sidebar from the document.
    #[must_use]
    pub fn sidebar_divider(&self) -> Style {
        Style::new()
            .bg(self.palette.bg.color())
            .fg(self.palette.border.color())
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
            theme.status_bar(),
            theme.status_active(),
            theme.status_message(),
            theme.overlay_border(),
            theme.overlay_body(),
            theme.overlay_key(),
            theme.overlay_title(),
            theme.table_header(),
            theme.list_marker(),
            theme.search_match(),
            theme.search_current(),
            theme.list_item(),
            theme.toc_active(),
            theme.list_cursor(),
            theme.list_cursor_marker(),
            theme.list_meta(),
            theme.toc_fold(),
            theme.sidebar_divider(),
        ];
        for style in styles {
            assert!(style.bg.is_some(), "style without a background: {style:?}");
        }
    }

    #[test]
    fn headings_lose_prominence_as_level_increases() {
        let theme = Theme::new(ThemeVariant::Slate);
        assert_eq!(theme.heading(1).fg, Some(theme.palette.accent.color()));
        assert_eq!(theme.heading(2).fg, Some(theme.palette.fg.color()));
        assert_eq!(theme.heading(4).fg, Some(theme.palette.muted.color()));
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
