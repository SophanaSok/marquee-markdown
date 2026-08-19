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

use crate::render::block::{Block, BlockKind};
use crate::render::{self, LayoutOptions, RenderedDoc};
use crate::source::Source;
use crate::theme::Theme;

use super::outline::Outline;
use super::view::Extent;

/// A parsed document plus its current layout.
#[derive(Debug)]
pub struct DocCache {
    /// The document this was built from.
    pub source: Source,
    blocks: Vec<Block>,
    doc: RenderedDoc,
    outline: Outline,
    /// How many headings the document has. Counted from the block tree rather
    /// than from a layout, because pane geometry depends on it and the panes
    /// are settled before anything is laid out.
    headings: usize,
    /// What `doc` was laid out from; `None` until the first layout.
    built_from: Option<(LayoutOptions, Theme)>,
    /// Heading to land on after a reload, in preference to a byte offset.
    keep_heading: Option<String>,
    revision: u64,
}

impl DocCache {
    /// Parse a document. Nothing is laid out until the first
    /// [`ensure_rendered`](Self::ensure_rendered), so there is exactly one
    /// layout path rather than one for startup and one for everything after.
    #[must_use]
    pub fn new(source: Source) -> Self {
        let blocks = render::parse::parse(&source.text);
        let headings = count_headings(&blocks);
        Self {
            source,
            blocks,
            headings,
            doc: RenderedDoc::default(),
            outline: Outline::default(),
            built_from: None,
            keep_heading: None,
            revision: 0,
        }
    }

    /// The current layout.
    #[must_use]
    pub fn doc(&self) -> &RenderedDoc {
        &self.doc
    }

    /// How many headings the document has.
    ///
    /// Available before the first layout, unlike [`Self::outline`], which
    /// needs line numbers. Pane geometry asks this instead: a contents pane
    /// that appeared only after the first frame would re-lay out the document
    /// immediately and lose the reader a line.
    #[must_use]
    pub fn heading_count(&self) -> usize {
        self.headings
    }

    /// The heading tree for the current layout.
    ///
    /// Built here rather than by the caller so it cannot go stale: it is
    /// replaced by the same call that replaces the lines it points into.
    #[must_use]
    pub fn outline(&self) -> &Outline {
        &self.outline
    }

    /// How many times the document has been laid out. State derived from line
    /// indices can compare this to know it is stale.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Replace the document, keeping the current layout around only so the
    /// reading position can still be read off it.
    ///
    /// The next [`ensure_rendered`](Self::ensure_rendered) does the work.
    /// Unlike a resize, an edit moves the text itself, so a byte offset no
    /// longer points where it did — inserting a paragraph at the top would
    /// drop the reader a section back. The section being read is remembered
    /// instead, and the offset is kept only as a fallback for a document with
    /// no headings.
    pub fn reload(&mut self, source: Source, top: usize) {
        self.keep_heading = self
            .doc
            .active_anchor(top)
            .and_then(|index| self.doc.outline.get(index))
            .map(|anchor| anchor.id.clone());
        self.blocks = render::parse::parse(&source.text);
        self.headings = count_headings(&self.blocks);
        self.source = source;
        self.built_from = None;
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
        let keep_heading = self.keep_heading.take();
        self.doc = render::layout::layout(&self.blocks, theme, options);
        self.outline = Outline::build(&self.doc.outline);
        self.built_from = Some((options, theme.clone()));
        self.revision += 1;

        if let Some(id) = keep_heading
            && let Some(anchor) = self.doc.outline.iter().find(|anchor| anchor.id == id)
        {
            return anchor.line;
        }
        self.line_at_source_offset(target)
    }

    /// Which line of the file on disk a rendered line came from, counting from
    /// one.
    ///
    /// The parsed text is not the file: frontmatter has been stripped from it
    /// and a code file has been wrapped in a fence, so the offsets inside it
    /// need putting back where they came from before an editor is told to jump
    /// there.
    #[must_use]
    pub fn source_line_of(&self, line: usize) -> usize {
        let offset = self.source_offset_of(line).min(self.source.text.len());
        let within = self.source.text[..offset]
            .bytes()
            .filter(|&byte| byte == b'\n')
            .count();
        let stripped = self
            .source
            .frontmatter
            .as_ref()
            // The delimiter lines either side went too.
            .map_or(0, |front| front.lines().count() + 2);
        // A wrapped code file gained an opening fence line.
        let added = usize::from(self.source.is_code);
        // Never below the first line: a rendered line above any source — a
        // code container's border, say — belongs to the start of the file.
        (within + stripped + 1).saturating_sub(added).max(1)
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

/// Count headings anywhere in the tree, including inside quotes and lists.
fn count_headings(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| match &block.kind {
            BlockKind::Heading { .. } => 1,
            BlockKind::BlockQuote { children, .. }
            | BlockKind::FootnoteDefinition { children, .. } => count_headings(children),
            BlockKind::List { items, .. } => items
                .iter()
                .map(|item| count_headings(&item.children))
                .sum(),
            _ => 0,
        })
        .sum()
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

    #[test]
    fn the_heading_tree_is_replaced_with_the_lines_it_points_into() {
        let mut cache = cache("# One\n\n## Two\n\nbody\n");
        assert!(cache.outline().is_empty(), "built before any layout");
        cache.ensure_rendered(options(40), &Theme::new(ThemeVariant::Slate), 0);
        assert_eq!(cache.outline().len(), 2);
        assert_eq!(cache.outline().rows()[1].depth, 1);
        for row in cache.outline().rows() {
            assert!(cache.doc().outline.get(row.anchor).is_some());
        }
    }

    #[test]
    fn headings_are_counted_before_anything_is_laid_out() {
        let cache = cache("# One\n\n## Two\n\n> ### Quoted\n\nbody\n");
        assert_eq!(cache.heading_count(), 3);
        assert!(cache.outline().is_empty(), "counted from a layout");
    }

    #[test]
    fn the_count_agrees_with_the_outline_once_there_is_one() {
        let mut cache = cache("# One\n\n## Two\n\n### Three\n\nbody\n");
        cache.ensure_rendered(options(40), &Theme::new(ThemeVariant::Slate), 0);
        assert_eq!(cache.heading_count(), cache.outline().len());
    }

    #[test]
    fn reloading_keeps_the_reader_near_where_they_were() {
        let before: String = (1..=40)
            .map(|n| format!("## Section {n}\n\nBody of section {n}.\n\n"))
            .collect();
        let mut cache = cache(&before);
        let theme = Theme::new(ThemeVariant::Slate);
        cache.ensure_rendered(options(60), &theme, 0);
        let parked = cache.doc().outline[20].clone();

        // The same document with a line added at the top.
        let after = format!("A new opening line.\n\n{before}");
        cache.reload(
            Source::from_text(&after, Some("doc.md".into()), "doc.md".into(), Base::Cwd),
            parked.line,
        );
        let top = cache.ensure_rendered(options(60), &theme, parked.line);

        let landed = cache
            .doc()
            .active_anchor(top)
            .map(|index| cache.doc().outline[index].id.clone());
        assert_eq!(landed.as_deref(), Some(parked.id.as_str()));
    }

    #[test]
    fn reloading_re_reads_the_document_rather_than_re_laying_out_the_old_one() {
        let mut cache = cache("# One\n");
        let theme = Theme::new(ThemeVariant::Slate);
        cache.ensure_rendered(options(40), &theme, 0);
        assert_eq!(cache.heading_count(), 1);

        cache.reload(
            Source::from_text(
                "# One\n\n## Two\n\n## Three\n",
                Some("doc.md".into()),
                "doc.md".into(),
                Base::Cwd,
            ),
            0,
        );
        cache.ensure_rendered(options(40), &theme, 0);
        assert_eq!(cache.heading_count(), 3);
        assert_eq!(cache.outline().len(), 3);
        assert_eq!(cache.revision(), 2, "the layout was not rebuilt");
    }

    #[test]
    fn a_reload_that_renames_the_section_falls_back_to_the_offset() {
        let text: String = (1..=20)
            .map(|n| format!("## Section {n}\n\nBody of section {n}.\n\n"))
            .collect();
        let mut cache = cache(&text);
        let theme = Theme::new(ThemeVariant::Slate);
        cache.ensure_rendered(options(60), &theme, 0);
        let parked = cache.doc().outline[10].line;

        let renamed = text.replace("## Section 11", "## Renamed entirely");
        cache.reload(
            Source::from_text(&renamed, Some("d.md".into()), "d.md".into(), Base::Cwd),
            parked,
        );
        let top = cache.ensure_rendered(options(60), &theme, parked);
        assert!(top < cache.doc().lines.len());
        // Near where they were, since the byte offsets barely moved.
        assert!(top.abs_diff(parked) < 8, "{top} vs {parked}");
    }

    #[test]
    fn a_document_with_no_headings_reloads_on_its_offsets() {
        let text: String = (1..=60).map(|n| format!("Paragraph {n}.\n\n")).collect();
        let mut cache = cache(&text);
        let theme = Theme::new(ThemeVariant::Slate);
        cache.ensure_rendered(options(60), &theme, 0);
        cache.reload(
            Source::from_text(&text, Some("d.md".into()), "d.md".into(), Base::Cwd),
            40,
        );
        assert_eq!(cache.ensure_rendered(options(60), &theme, 40), 40);
    }

    #[test]
    fn a_rendered_line_maps_back_to_a_line_of_the_file() {
        let mut cache = cache("# One\n\nBody.\n\n# Two\n\nMore.\n");
        let theme = Theme::new(ThemeVariant::Slate);
        cache.ensure_rendered(options(60), &theme, 0);
        assert_eq!(cache.source_line_of(0), 1);
        let second = cache.doc().outline[1].line;
        assert_eq!(cache.source_line_of(second), 5);
    }

    #[test]
    fn frontmatter_is_counted_back_in() {
        // The parsed text starts after the frontmatter, but the editor has to
        // open the file, which still has it.
        let mut cache = cache("---\ntitle: T\ntags: [a]\n---\n# One\n\nBody.\n");
        let theme = Theme::new(ThemeVariant::Slate);
        cache.ensure_rendered(options(60), &theme, 0);
        assert!(cache.source.frontmatter.is_some());
        assert_eq!(cache.source_line_of(0), 5);
    }

    #[test]
    fn a_wrapped_code_file_does_not_count_its_own_fence() {
        let source = Source::from_text(
            "let a = 1;\nlet b = 2;\nlet c = 3;\n",
            Some("x.rs".into()),
            "x.rs".into(),
            Base::Cwd,
        );
        let mut cache = DocCache::new(source);
        let theme = Theme::new(ThemeVariant::Slate);
        cache.ensure_rendered(options(60), &theme, 0);
        assert!(cache.source.is_code);
        assert_eq!(cache.source_line_of(0), 1);
    }
}
