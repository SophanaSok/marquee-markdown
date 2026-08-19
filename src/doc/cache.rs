//! The one place a document is laid out.
//!
//! Every index the application holds — scroll position, outline anchors, and
//! later search matches and link spans — points into
//! [`RenderedDoc::lines`](crate::render::RenderedDoc). A resize, a theme
//! switch, or a reload invalidates all of them at once, so all of them are
//! remapped in one place: nothing else may call the layout engine.
//!
//! Parsing is width- and theme-independent and stays cached across every
//! re-layout, which is what makes resizing cheap.

use crate::render::block::Block;
use crate::render::{self, LayoutOptions, RenderedDoc};
use crate::source::Source;
use crate::theme::Theme;

use super::view::Extent;

/// A parsed document plus its current layout.
#[derive(Debug)]
pub struct DocCache {
    /// The document this was built from.
    pub source: Source,
    blocks: Vec<Block>,
    doc: RenderedDoc,
    /// What `doc` was laid out from; `None` until the first layout.
    built_from: Option<(LayoutOptions, Theme)>,
    revision: u64,
}

impl DocCache {
    /// Parse a document. Nothing is laid out until the first
    /// [`ensure_rendered`](Self::ensure_rendered), so there is exactly one
    /// layout path rather than one for startup and one for everything after.
    #[must_use]
    pub fn new(source: Source) -> Self {
        let blocks = render::parse::parse(&source.text);
        Self {
            source,
            blocks,
            doc: RenderedDoc::default(),
            built_from: None,
            revision: 0,
        }
    }

    /// The current layout.
    #[must_use]
    pub fn doc(&self) -> &RenderedDoc {
        &self.doc
    }

    /// How many times the document has been laid out. State derived from line
    /// indices can compare this to know it is stale.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The scrolling bounds for a viewport of the given size.
    #[must_use]
    pub fn extent(&self, height: u16, area_width: u16) -> Extent {
        Extent {
            lines: self.doc.lines.len(),
            height,
            doc_width: self.doc.width,
            area_width,
        }
    }

    /// Lay the document out if `options` or `theme` differ from what produced
    /// the current layout, and return the line index `top` becomes.
    ///
    /// The reading position is carried across by source byte offset rather
    /// than by line number: at a new width the same prose lands on a different
    /// line, and a reader who resizes their terminal should stay where they
    /// were reading instead of being teleported.
    pub fn ensure_rendered(&mut self, options: LayoutOptions, theme: &Theme, top: usize) -> usize {
        if self
            .built_from
            .as_ref()
            .is_some_and(|(o, t)| *o == options && t == theme)
        {
            return top;
        }

        let target = self.source_offset_of(top);
        self.doc = render::layout::layout(&self.blocks, theme, options);
        self.built_from = Some((options, theme.clone()));
        self.revision += 1;
        self.line_at_source_offset(target)
    }

    /// The source byte offset the reader is currently at: taken from `line`,
    /// or from the nearest line above it that came from the source at all —
    /// blank lines and container borders carry no offset of their own.
    fn source_offset_of(&self, line: usize) -> usize {
        let upto = line.min(self.doc.meta.len().saturating_sub(1));
        self.doc
            .meta
            .get(..=upto)
            .unwrap_or_default()
            .iter()
            .rev()
            .find_map(|meta| meta.source.as_ref().map(|range| range.start))
            .unwrap_or(0)
    }

    /// The first line rendering source byte `offset` or later.
    fn line_at_source_offset(&self, offset: usize) -> usize {
        self.doc
            .meta
            .iter()
            .position(|meta| {
                meta.source
                    .as_ref()
                    .is_some_and(|range| range.start >= offset)
            })
            .unwrap_or_else(|| self.doc.lines.len().saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Base;
    use crate::theme::ThemeVariant;

    fn cache(text: &str) -> DocCache {
        DocCache::new(Source::from_text(
            text,
            Some("doc.md".into()),
            "doc.md".into(),
            Base::Cwd,
        ))
    }

    fn options(width: u16) -> LayoutOptions {
        LayoutOptions {
            width,
            code_line_numbers: false,
        }
    }

    fn long_document() -> String {
        (1..=40)
            .map(|n| format!("## Section {n}\n\nSome prose in section {n} that is long enough to wrap at a narrow width.\n\n"))
            .collect()
    }

    #[test]
    fn nothing_is_laid_out_until_asked() {
        let cache = cache("# Title\n");
        assert!(cache.doc().lines.is_empty());
        assert_eq!(cache.revision(), 0);
    }

    #[test]
    fn laying_out_twice_with_the_same_inputs_does_no_work() {
        let mut cache = cache("# Title\n\nbody\n");
        let theme = Theme::new(ThemeVariant::Slate);
        cache.ensure_rendered(options(40), &theme, 0);
        assert_eq!(cache.revision(), 1);
        cache.ensure_rendered(options(40), &theme, 0);
        assert_eq!(cache.revision(), 1, "re-laid out with unchanged inputs");
    }

    #[test]
    fn a_theme_change_forces_a_new_layout() {
        let mut cache = cache("# Title\n");
        cache.ensure_rendered(options(40), &Theme::new(ThemeVariant::Slate), 0);
        cache.ensure_rendered(options(40), &Theme::new(ThemeVariant::Paper), 0);
        assert_eq!(cache.revision(), 2);
    }

    #[test]
    fn resizing_keeps_the_reader_where_they_were_reading() {
        let text = long_document();
        let mut cache = cache(&text);
        let theme = Theme::new(ThemeVariant::Slate);
        cache.ensure_rendered(options(80), &theme, 0);

        // Park on a heading well into the document, then narrow the column.
        let anchor = cache.doc().outline[25].clone();
        let top = cache.ensure_rendered(options(40), &theme, anchor.line);

        let landed = cache
            .doc()
            .active_anchor(top)
            .map(|index| cache.doc().outline[index].id.clone());
        assert_eq!(landed.as_deref(), Some(anchor.id.as_str()));
    }

    #[test]
    fn the_top_of_the_document_stays_at_the_top() {
        let text = long_document();
        let mut cache = cache(&text);
        let theme = Theme::new(ThemeVariant::Slate);
        cache.ensure_rendered(options(80), &theme, 0);
        assert_eq!(cache.ensure_rendered(options(30), &theme, 0), 0);
    }

    #[test]
    fn a_position_past_the_end_of_the_new_layout_is_still_a_valid_line() {
        let text = long_document();
        let mut cache = cache(&text);
        let theme = Theme::new(ThemeVariant::Slate);
        cache.ensure_rendered(options(40), &theme, 0);
        let last = cache.doc().lines.len() - 1;
        let top = cache.ensure_rendered(options(200), &theme, last);
        assert!(top < cache.doc().lines.len(), "{top} is out of range");
    }

    #[test]
    fn an_empty_document_survives_layout() {
        let mut cache = cache("");
        let theme = Theme::new(ThemeVariant::Slate);
        assert_eq!(cache.ensure_rendered(options(40), &theme, 0), 0);
        assert_eq!(cache.extent(20, 40).max_top(), 0);
    }
}
