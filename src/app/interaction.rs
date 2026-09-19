//! Terminal-independent interaction state: profile row selection.
//!
//! Holds a row index only — no filesystem paths, no hardware handles, no
//! capability policy. Callers supply the row count (built-ins plus custom
//! entries); every move clamps or wraps deterministically, so selection
//! can never index out of bounds.

/// Selected profile row: an index into the ordered catalog rows (built-ins
/// first, then custom entries in catalog order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProfileSelection {
    index: usize,
}

impl ProfileSelection {
    /// Currently selected row.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Moves to the next row, wrapping past the last row back to the
    /// first. An empty row set pins the selection at zero.
    pub fn move_down(&mut self, row_count: usize) {
        if row_count == 0 {
            self.index = 0;
        } else {
            self.index = (self.index + 1) % row_count;
        }
    }

    /// Moves to the previous row, wrapping past the first row to the
    /// last. An empty row set pins the selection at zero.
    pub fn move_up(&mut self, row_count: usize) {
        if row_count == 0 {
            self.index = 0;
        } else {
            self.index = (self.index + row_count - 1) % row_count;
        }
    }

    /// Clamps a possibly stale index into range after the catalog changes.
    pub fn clamp(&mut self, row_count: usize) {
        if row_count == 0 {
            self.index = 0;
        } else if self.index >= row_count {
            self.index = row_count - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_selection_is_first_row() {
        assert_eq!(ProfileSelection::default().index(), 0);
    }

    #[test]
    fn move_down_traverses_then_wraps() {
        let mut selection = ProfileSelection::default();
        selection.move_down(3);
        assert_eq!(selection.index(), 1);
        selection.move_down(3);
        assert_eq!(selection.index(), 2);
        selection.move_down(3);
        assert_eq!(selection.index(), 0);
    }

    #[test]
    fn move_up_wraps_from_first_to_last() {
        let mut selection = ProfileSelection::default();
        selection.move_up(3);
        assert_eq!(selection.index(), 2);
        selection.move_up(3);
        assert_eq!(selection.index(), 1);
    }

    #[test]
    fn empty_rows_pin_at_zero() {
        let mut selection = ProfileSelection::default();
        selection.move_down(0);
        selection.move_up(0);
        selection.clamp(0);
        assert_eq!(selection.index(), 0);
    }

    #[test]
    fn clamp_pulls_stale_index_into_range() {
        let mut selection = ProfileSelection::default();
        for _ in 0..7 {
            selection.move_down(8);
        }
        assert_eq!(selection.index(), 7);
        selection.clamp(3);
        assert_eq!(selection.index(), 2);
        selection.clamp(5);
        assert_eq!(selection.index(), 2);
    }

    #[test]
    fn selection_is_copy() {
        let selection = ProfileSelection::default();
        let moved = selection;
        assert_eq!(selection, moved);
    }
}
