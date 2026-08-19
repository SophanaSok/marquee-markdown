//! The heading tree behind the table of contents.
//!
//! The renderer produces a flat list of anchors in document order. This turns
//! it into a tree, flattened back into rows that each know how far their
//! subtree extends — which is what makes collapsing a section a matter of
//! skipping a range rather than walking pointers.
//!
//! Nesting comes from the order headings appear in, not from their levels
//! arithmetically: a document that jumps from `#` straight to `###` is common
//! and its `###` is a child, not an orphan two levels down.

use std::ops::Range;

use crate::render::Anchor;

/// One row of the table of contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Index into [`RenderedDoc::outline`](crate::render::RenderedDoc::outline).
    pub anchor: usize,
    /// Nesting depth, counted in ancestors rather than in heading levels.
    pub depth: usize,
    /// Rows nested under this one: a contiguous range of row indices that
    /// always follows it directly.
    pub subtree: Range<usize>,
}

impl Row {
    /// Whether this row has anything nested under it.
    #[must_use]
    pub fn has_children(&self) -> bool {
        !self.subtree.is_empty()
    }
}

/// The heading tree, flattened into document order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outline {
    rows: Vec<Row>,
}

impl Outline {
    /// Build the tree from the renderer's flat anchor list.
    #[must_use]
    pub fn build(anchors: &[Anchor]) -> Self {
        let mut rows: Vec<Row> = Vec::with_capacity(anchors.len());
        let mut ancestors: Vec<u8> = Vec::new();

        for (index, anchor) in anchors.iter().enumerate() {
            while ancestors.last().is_some_and(|&level| level >= anchor.level) {
                ancestors.pop();
            }
            rows.push(Row {
                anchor: index,
                depth: ancestors.len(),
                subtree: 0..0,
            });
            ancestors.push(anchor.level);
        }

        // A subtree is every following row deeper than this one, and they are
        // always contiguous.
        for index in 0..rows.len() {
            let depth = rows[index].depth;
            let end = rows[index + 1..]
                .iter()
                .position(|row| row.depth <= depth)
                .map_or(rows.len(), |offset| index + 1 + offset);
            rows[index].subtree = index + 1..end;
        }

        Self { rows }
    }

    /// Every row, in document order.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// How many rows there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the document has no headings at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The row indices visible when `collapsed` rows are folded shut.
    ///
    /// A collapsed row is still listed; only its descendants are skipped.
    #[must_use]
    pub fn visible(&self, collapsed: &[bool]) -> Vec<usize> {
        let mut visible = Vec::with_capacity(self.rows.len());
        let mut index = 0;
        while index < self.rows.len() {
            visible.push(index);
            if collapsed.get(index).copied().unwrap_or(false) {
                index = self.rows[index].subtree.end.max(index + 1);
            } else {
                index += 1;
            }
        }
        visible
    }

    /// The row `row` is nested directly under, if any.
    #[must_use]
    pub fn parent(&self, row: usize) -> Option<usize> {
        (0..row)
            .rev()
            .find(|&candidate| self.rows[candidate].subtree.contains(&row))
    }

    /// The nearest ancestor of `row` that is folded shut, if any — the row a
    /// cursor has to move to when its own row is hidden.
    #[must_use]
    pub fn hidden_behind(&self, row: usize, collapsed: &[bool]) -> Option<usize> {
        (0..row)
            .filter(|&candidate| collapsed.get(candidate).copied().unwrap_or(false))
            .find(|&candidate| self.rows[candidate].subtree.contains(&row))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchors(levels: &[u8]) -> Vec<Anchor> {
        levels
            .iter()
            .enumerate()
            .map(|(index, &level)| Anchor {
                line: index * 3,
                level,
                id: format!("h{index}"),
                text: format!("Heading {index}"),
            })
            .collect()
    }

    fn depths(outline: &Outline) -> Vec<usize> {
        outline.rows().iter().map(|row| row.depth).collect()
    }

    #[test]
    fn nesting_follows_the_heading_levels() {
        let outline = Outline::build(&anchors(&[1, 2, 3, 2, 1]));
        assert_eq!(depths(&outline), vec![0, 1, 2, 1, 0]);
    }

    #[test]
    fn a_skipped_level_does_not_produce_an_orphan() {
        // `#` straight to `###` is common, and the `###` is a child, not a
        // grandchild of nothing.
        let outline = Outline::build(&anchors(&[1, 3, 3, 2]));
        assert_eq!(depths(&outline), vec![0, 1, 1, 1]);
    }

    #[test]
    fn a_document_starting_below_level_one_still_starts_at_the_root() {
        let outline = Outline::build(&anchors(&[2, 3, 2]));
        assert_eq!(depths(&outline), vec![0, 1, 0]);
    }

    #[test]
    fn a_subtree_covers_exactly_the_rows_nested_under_it() {
        let outline = Outline::build(&anchors(&[1, 2, 3, 2, 1]));
        assert_eq!(outline.rows()[0].subtree, 1..4);
        assert_eq!(outline.rows()[1].subtree, 2..3);
        assert_eq!(outline.rows()[2].subtree, 3..3);
        assert_eq!(outline.rows()[4].subtree, 5..5);
        assert!(outline.rows()[0].has_children());
        assert!(!outline.rows()[4].has_children());
    }

    #[test]
    fn collapsing_a_row_hides_its_descendants_but_not_itself() {
        let outline = Outline::build(&anchors(&[1, 2, 3, 2, 1]));
        let mut collapsed = vec![false; outline.len()];
        collapsed[0] = true;
        assert_eq!(outline.visible(&collapsed), vec![0, 4]);
        collapsed[0] = false;
        collapsed[1] = true;
        assert_eq!(outline.visible(&collapsed), vec![0, 1, 3, 4]);
    }

    #[test]
    fn collapsing_a_leaf_hides_nothing() {
        let outline = Outline::build(&anchors(&[1, 2]));
        assert_eq!(outline.visible(&[false, true]), vec![0, 1]);
    }

    #[test]
    fn nested_collapses_do_not_hide_each_other_twice() {
        let outline = Outline::build(&anchors(&[1, 2, 3, 1]));
        assert_eq!(outline.visible(&[true, true, false, false]), vec![0, 3]);
    }

    #[test]
    fn a_hidden_row_reports_the_ancestor_hiding_it() {
        let outline = Outline::build(&anchors(&[1, 2, 3, 1]));
        let collapsed = [true, false, false, false];
        assert_eq!(outline.hidden_behind(2, &collapsed), Some(0));
        assert_eq!(outline.hidden_behind(3, &collapsed), None);
        assert_eq!(outline.hidden_behind(0, &collapsed), None);
    }

    #[test]
    fn a_document_without_headings_produces_no_rows() {
        let outline = Outline::build(&[]);
        assert!(outline.is_empty());
        assert_eq!(outline.visible(&[]), Vec::<usize>::new());
    }

    #[test]
    fn a_collapse_flag_list_shorter_than_the_outline_is_treated_as_open() {
        let outline = Outline::build(&anchors(&[1, 2, 3]));
        assert_eq!(outline.visible(&[]), vec![0, 1, 2]);
    }

    #[test]
    fn a_row_knows_its_parent() {
        let outline = Outline::build(&anchors(&[1, 2, 3, 2, 1]));
        assert_eq!(outline.parent(0), None);
        assert_eq!(outline.parent(1), Some(0));
        assert_eq!(outline.parent(2), Some(1));
        assert_eq!(outline.parent(3), Some(0));
        assert_eq!(outline.parent(4), None);
    }
}
