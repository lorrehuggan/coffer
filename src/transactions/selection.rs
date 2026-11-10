//! The table's selection: which transactions are selected, which row the
//! keyboard is on, and where a ⇧ range starts. Held by id, not by row
//! position, so it survives the list changing around it.

use std::collections::BTreeSet;

/// What a click on a row does to the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickKind {
    /// A plain click: select only this row.
    Replace,
    /// ⌘-click (Ctrl on Linux and Windows): add or remove this row.
    Toggle,
    /// ⇧-click: select from the anchor to this row.
    Extend,
}

/// The selected rows, the keyboard row and the range anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection<Id: Copy + Ord> {
    selected: BTreeSet<Id>,
    current: Option<Id>,
    anchor: Option<Id>,
}

impl<Id: Copy + Ord> Default for Selection<Id> {
    fn default() -> Self {
        Self {
            selected: BTreeSet::new(),
            current: None,
            anchor: None,
        }
    }
}

impl<Id: Copy + Ord> Selection<Id> {
    /// Selects exactly `ids`; the last one becomes the keyboard row and the
    /// anchor.
    pub fn set(&mut self, ids: impl IntoIterator<Item = Id>) {
        self.selected = BTreeSet::new();
        for id in ids {
            self.selected.insert(id);
            self.current = Some(id);
        }
        self.anchor = self.current;
    }

    /// A click on `id`. `order` is the rows' display order, for ranges.
    pub fn click(&mut self, id: Id, kind: ClickKind, order: &[Id]) {
        match kind {
            ClickKind::Replace => self.set([id]),
            ClickKind::Toggle => {
                if !self.selected.remove(&id) {
                    self.selected.insert(id);
                }
                self.current = Some(id);
                self.anchor = Some(id);
            }
            ClickKind::Extend => {
                let anchor = self.anchor.unwrap_or(id);
                self.selected = range(order, anchor, id).into_iter().collect();
                self.current = Some(id);
                self.anchor = Some(anchor);
            }
        }
    }

    /// Moves the keyboard row `delta` rows through `order`, stopping at the
    /// ends, and selects it; with `extend`, selects the range from the
    /// anchor to it (⇧↑/⇧↓). With no keyboard row yet, starts at the first
    /// row (down) or the last (up).
    pub fn move_by(&mut self, delta: isize, extend: bool, order: &[Id]) {
        let Some(last) = order.len().checked_sub(1) else {
            return;
        };
        let target = match self.current.and_then(|id| position(order, id)) {
            Some(index) => index.saturating_add_signed(delta).min(last),
            None if delta < 0 => last,
            None => 0,
        };
        let kind = if extend {
            ClickKind::Extend
        } else {
            ClickKind::Replace
        };
        self.click(order[target], kind, order);
    }

    /// Selects nothing; the keyboard row stays where it was.
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = self.current;
    }

    pub fn contains(&self, id: Id) -> bool {
        self.selected.contains(&id)
    }

    pub fn len(&self) -> usize {
        self.selected.len()
    }

    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// The row the keyboard is on.
    pub fn current(&self) -> Option<Id> {
        self.current
    }

    /// The selected ids in display `order`.
    pub fn in_order<'a>(&'a self, order: &'a [Id]) -> impl Iterator<Item = Id> + 'a {
        order
            .iter()
            .copied()
            .filter(|id| self.selected.contains(id))
    }
}

fn position<Id: Copy + Ord>(order: &[Id], id: Id) -> Option<usize> {
    order.iter().position(|candidate| *candidate == id)
}

/// The ids from `from` to `to` in `order`, whichever comes first; just `to`
/// when `from` isn't in `order`, and nothing when `to` isn't.
fn range<Id: Copy + Ord>(order: &[Id], from: Id, to: Id) -> Vec<Id> {
    match (position(order, from), position(order, to)) {
        (Some(a), Some(b)) => order[a.min(b)..=a.max(b)].to_vec(),
        (None, Some(_)) => vec![to],
        (_, None) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDER: [u8; 5] = [10, 20, 30, 40, 50];

    fn selected(selection: &Selection<u8>) -> Vec<u8> {
        selection.in_order(&ORDER).collect()
    }

    #[test]
    fn a_plain_click_replaces_the_selection() {
        let mut selection = Selection::default();
        selection.set([10, 20]);
        selection.click(40, ClickKind::Replace, &ORDER);
        assert_eq!(selected(&selection), [40]);
        assert_eq!(selection.current(), Some(40));
    }

    #[test]
    fn toggle_adds_and_removes() {
        let mut selection = Selection::default();
        selection.click(20, ClickKind::Replace, &ORDER);
        selection.click(40, ClickKind::Toggle, &ORDER);
        assert_eq!(selected(&selection), [20, 40]);
        selection.click(20, ClickKind::Toggle, &ORDER);
        assert_eq!(selected(&selection), [40]);
    }

    #[test]
    fn extend_selects_from_the_anchor_either_way() {
        let mut selection = Selection::default();
        selection.click(30, ClickKind::Replace, &ORDER);
        selection.click(50, ClickKind::Extend, &ORDER);
        assert_eq!(selected(&selection), [30, 40, 50]);
        // The anchor stays put, so the range can flip direction.
        selection.click(10, ClickKind::Extend, &ORDER);
        assert_eq!(selected(&selection), [10, 20, 30]);
    }

    #[test]
    fn extend_without_an_anchor_selects_the_row() {
        let mut selection = Selection::default();
        selection.click(40, ClickKind::Extend, &ORDER);
        assert_eq!(selected(&selection), [40]);
    }

    #[test]
    fn keyboard_moves_stop_at_the_ends() {
        let mut selection = Selection::default();
        selection.move_by(1, false, &ORDER);
        assert_eq!(
            selected(&selection),
            [10],
            "down with nothing selected starts at the top"
        );
        selection.move_by(-1, false, &ORDER);
        assert_eq!(selected(&selection), [10]);
        selection.move_by(10, false, &ORDER);
        assert_eq!(selected(&selection), [50]);
        selection.move_by(-1, true, &ORDER);
        assert_eq!(selected(&selection), [40, 50]);
    }

    #[test]
    fn up_with_nothing_selected_starts_at_the_bottom() {
        let mut selection = Selection::default();
        selection.move_by(-1, false, &ORDER);
        assert_eq!(selected(&selection), [50]);
    }

    #[test]
    fn clear_keeps_the_keyboard_row() {
        let mut selection = Selection::default();
        selection.set([20, 30]);
        selection.clear();
        assert!(selection.is_empty());
        assert_eq!(selection.current(), Some(30));
        selection.move_by(1, false, &ORDER);
        assert_eq!(selected(&selection), [40]);
    }

    #[test]
    fn empty_order_is_a_no_op() {
        let mut selection = Selection::<u8>::default();
        selection.move_by(1, false, &[]);
        assert!(selection.is_empty());
    }
}
