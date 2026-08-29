//! The editable document: an ordered list of annotations over a base image,
//! with undo/redo.
//!
//! History is snapshot-based rather than command-based. Snapshots of a shape
//! list are tiny — a few hundred bytes even for a busy annotation — and the
//! alternative means writing, and correctly inverting, an operation type per
//! tool. That is a large amount of code whose only payoff would be memory we
//! are not short of.

use serde::{Deserialize, Serialize};

use crate::frame::Frame;
use crate::shape::{Point, Rect, Shape};

/// How many undo steps to keep. Beyond this the oldest are dropped.
const HISTORY_LIMIT: usize = 200;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Document {
    /// Painting order: index 0 is furthest back.
    shapes: Vec<Shape>,
    /// Crop, padding, corners, shadow and background. Applied after the
    /// annotations, because it changes the size of the output rather than
    /// drawing onto it.
    #[serde(default)]
    frame: Frame,
    #[serde(skip)]
    undo_stack: Vec<Vec<Shape>>,
    #[serde(skip)]
    redo_stack: Vec<Vec<Shape>>,
    #[serde(skip)]
    frame_undo_stack: Vec<Frame>,
    #[serde(skip)]
    frame_redo_stack: Vec<Frame>,
    #[serde(skip)]
    selected: Option<usize>,
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shapes(&self) -> &[Shape] {
        &self.shapes
    }

    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    /// Replace the frame, recording one undo step.
    ///
    /// Frame changes share the annotation history so that undo walks back
    /// through everything the user did in the order they did it, rather than
    /// leaving two separate timelines to reason about.
    pub fn set_frame(&mut self, frame: Frame) {
        if self.frame == frame {
            return;
        }
        self.checkpoint();
        self.frame = frame;
    }

    pub fn len(&self) -> usize {
        self.shapes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    pub fn selected(&self) -> Option<&Shape> {
        self.selected.and_then(|i| self.shapes.get(i))
    }

    /// Record the current state so the next mutation can be undone.
    fn checkpoint(&mut self) {
        self.undo_stack.push(self.shapes.clone());
        self.frame_undo_stack.push(self.frame);
        if self.undo_stack.len() > HISTORY_LIMIT {
            self.undo_stack.remove(0);
            self.frame_undo_stack.remove(0);
        }
        // Any new edit invalidates the redo branch, exactly as every editor
        // the user has ever used behaves.
        self.redo_stack.clear();
        self.frame_redo_stack.clear();
    }

    pub fn push(&mut self, shape: Shape) -> usize {
        self.checkpoint();
        self.shapes.push(shape);
        self.renumber_steps();
        let index = self.shapes.len() - 1;
        self.selected = Some(index);
        index
    }

    pub fn remove(&mut self, index: usize) -> Option<Shape> {
        if index >= self.shapes.len() {
            return None;
        }
        self.checkpoint();
        let removed = self.shapes.remove(index);
        self.renumber_steps();
        self.selected = None;
        Some(removed)
    }

    pub fn remove_selected(&mut self) -> Option<Shape> {
        self.selected.and_then(|i| self.remove(i))
    }

    pub fn clear(&mut self) {
        if self.shapes.is_empty() {
            return;
        }
        self.checkpoint();
        self.shapes.clear();
        self.selected = None;
    }

    /// Duplicate the selected shape, offset so it is visibly a copy.
    pub fn duplicate_selected(&mut self) -> Option<usize> {
        let index = self.selected?;
        let mut copy = self.shapes.get(index)?.clone();
        copy.translate(12.0, 12.0);
        Some(self.push(copy))
    }

    pub fn translate_selected(&mut self, dx: f32, dy: f32) {
        let Some(index) = self.selected else { return };
        self.checkpoint();
        if let Some(shape) = self.shapes.get_mut(index) {
            shape.translate(dx, dy);
        }
    }

    /// Select the front-most shape under `p`, or clear the selection.
    pub fn select_at(&mut self, p: Point) -> Option<usize> {
        self.selected = self
            .shapes
            .iter()
            .enumerate()
            .rev()
            .find(|(_, shape)| shape.hit_test(p))
            .map(|(i, _)| i);
        self.selected
    }

    pub fn select(&mut self, index: Option<usize>) {
        self.selected = index.filter(|i| *i < self.shapes.len());
    }

    // ── Z-order ─────────────────────────────────────────────────────────

    pub fn bring_to_front(&mut self) {
        self.reorder_selected(|_, len| len.saturating_sub(1));
    }

    pub fn send_to_back(&mut self) {
        self.reorder_selected(|_, _| 0);
    }

    pub fn bring_forward(&mut self) {
        self.reorder_selected(|i, len| (i + 1).min(len.saturating_sub(1)));
    }

    pub fn send_backward(&mut self) {
        self.reorder_selected(|i, _| i.saturating_sub(1));
    }

    fn reorder_selected(&mut self, target: impl Fn(usize, usize) -> usize) {
        let Some(index) = self.selected else { return };
        let len = self.shapes.len();
        let to = target(index, len);
        if to == index {
            return;
        }
        self.checkpoint();
        let shape = self.shapes.remove(index);
        self.shapes.insert(to, shape);
        self.selected = Some(to);
        self.renumber_steps();
    }

    // ── History ─────────────────────────────────────────────────────────

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack
            .push(std::mem::replace(&mut self.shapes, previous));

        if let Some(frame) = self.frame_undo_stack.pop() {
            self.frame_redo_stack
                .push(std::mem::replace(&mut self.frame, frame));
        }
        self.selected = None;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack
            .push(std::mem::replace(&mut self.shapes, next));

        if let Some(frame) = self.frame_redo_stack.pop() {
            self.frame_undo_stack
                .push(std::mem::replace(&mut self.frame, frame));
        }
        self.selected = None;
        true
    }

    // ── Step numbering ──────────────────────────────────────────────────

    /// Renumber step callouts 1..n in painting order.
    ///
    /// ShareX leaves a gap when you delete a step; renumbering keeps a
    /// numbered walkthrough correct without the user redoing every callout.
    fn renumber_steps(&mut self) {
        let mut next = 1;
        for shape in self.shapes.iter_mut() {
            if let Shape::Step { number, .. } = shape {
                *number = next;
                next += 1;
            }
        }
    }

    /// The number the next step callout should carry.
    pub fn next_step_number(&self) -> u32 {
        self.shapes
            .iter()
            .filter(|s| matches!(s, Shape::Step { .. }))
            .count() as u32
            + 1
    }

    /// The area every annotation touches, for partial re-rendering.
    pub fn dirty_bounds(&self) -> Option<Rect> {
        let mut iter = self.shapes.iter().map(|s| s.bounds());
        let first = iter.next()?;
        Some(iter.fold(first, |acc, b| {
            Rect::from_corners(
                Point::new(acc.x.min(b.x), acc.y.min(b.y)),
                Point::new(acc.right().max(b.right()), acc.bottom().max(b.bottom())),
            )
        }))
    }

    pub fn has_redactions(&self) -> bool {
        self.shapes.iter().any(Shape::is_redaction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::{Color, Stroke};

    fn rect_at(x: f32, y: f32) -> Shape {
        Shape::Rectangle {
            rect: Rect::new(x, y, 20.0, 20.0),
            stroke: Stroke::default(),
            fill: Color::TRANSPARENT,
            corner_radius: 0.0,
        }
    }

    fn step() -> Shape {
        Shape::Step {
            center: Point::new(0.0, 0.0),
            radius: 12.0,
            number: 0,
            fill: Color::ACCENT,
            text_color: Color::WHITE,
        }
    }

    fn numbers(doc: &Document) -> Vec<u32> {
        doc.shapes()
            .iter()
            .filter_map(|s| match s {
                Shape::Step { number, .. } => Some(*number),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn undo_restores_the_previous_shape_list() {
        let mut doc = Document::new();
        doc.push(rect_at(0.0, 0.0));
        doc.push(rect_at(50.0, 50.0));
        assert_eq!(doc.len(), 2);

        assert!(doc.undo());
        assert_eq!(doc.len(), 1);
        assert!(doc.undo());
        assert!(doc.is_empty());
        assert!(!doc.undo(), "nothing left to undo");
    }

    #[test]
    fn redo_replays_an_undone_edit() {
        let mut doc = Document::new();
        doc.push(rect_at(0.0, 0.0));
        doc.undo();
        assert!(doc.is_empty());

        assert!(doc.redo());
        assert_eq!(doc.len(), 1);
        assert!(!doc.redo());
    }

    #[test]
    fn a_new_edit_discards_the_redo_branch() {
        let mut doc = Document::new();
        doc.push(rect_at(0.0, 0.0));
        doc.push(rect_at(10.0, 10.0));
        doc.undo();
        assert!(doc.can_redo());

        doc.push(rect_at(99.0, 99.0));

        assert!(!doc.can_redo(), "branching must drop the abandoned future");
        assert_eq!(doc.len(), 2);
    }

    #[test]
    fn history_is_bounded() {
        let mut doc = Document::new();
        for i in 0..(HISTORY_LIMIT + 50) {
            doc.push(rect_at(i as f32, 0.0));
        }
        assert!(doc.undo_stack.len() <= HISTORY_LIMIT);
    }

    #[test]
    fn steps_renumber_when_one_is_deleted() {
        let mut doc = Document::new();
        doc.push(step());
        doc.push(step());
        doc.push(step());
        assert_eq!(numbers(&doc), vec![1, 2, 3]);

        doc.remove(1);

        assert_eq!(numbers(&doc), vec![1, 2], "no gap may be left behind");
    }

    #[test]
    fn steps_renumber_when_reordered() {
        let mut doc = Document::new();
        doc.push(step());
        doc.push(step());
        assert_eq!(numbers(&doc), vec![1, 2]);

        doc.select(Some(1));
        doc.send_to_back();

        assert_eq!(numbers(&doc), vec![1, 2]);
        assert_eq!(doc.selected_index(), Some(0));
    }

    #[test]
    fn next_step_number_counts_only_steps() {
        let mut doc = Document::new();
        doc.push(rect_at(0.0, 0.0));
        assert_eq!(doc.next_step_number(), 1);
        doc.push(step());
        assert_eq!(doc.next_step_number(), 2);
    }

    #[test]
    fn selection_picks_the_front_most_shape() {
        let mut doc = Document::new();
        doc.push(rect_at(0.0, 0.0));
        doc.push(rect_at(5.0, 5.0)); // overlaps the first, painted later

        let hit = doc.select_at(Point::new(10.0, 10.0));

        assert_eq!(hit, Some(1), "the shape on top wins");
    }

    #[test]
    fn selection_clears_when_clicking_empty_space() {
        let mut doc = Document::new();
        doc.push(rect_at(0.0, 0.0));
        assert!(doc.select_at(Point::new(500.0, 500.0)).is_none());
        assert!(doc.selected().is_none());
    }

    #[test]
    fn z_order_moves_are_clamped_at_the_ends() {
        let mut doc = Document::new();
        doc.push(rect_at(0.0, 0.0));
        doc.push(rect_at(1.0, 1.0));

        doc.select(Some(0));
        doc.send_to_back();
        assert_eq!(doc.selected_index(), Some(0), "already at the back");

        doc.select(Some(1));
        doc.bring_to_front();
        assert_eq!(doc.selected_index(), Some(1), "already at the front");

        doc.select(Some(0));
        doc.bring_to_front();
        assert_eq!(doc.selected_index(), Some(1));
    }

    #[test]
    fn duplicate_offsets_the_copy_so_it_is_visible() {
        let mut doc = Document::new();
        doc.push(rect_at(0.0, 0.0));

        let copy = doc.duplicate_selected().expect("something is selected");

        assert_eq!(doc.len(), 2);
        assert_ne!(
            doc.shapes()[copy].bounds().x,
            doc.shapes()[0].bounds().x,
            "a copy stacked exactly on the original looks like nothing happened"
        );
    }

    #[test]
    fn dirty_bounds_cover_every_shape() {
        let mut doc = Document::new();
        assert_eq!(doc.dirty_bounds(), None);

        doc.push(rect_at(0.0, 0.0));
        doc.push(rect_at(100.0, 60.0));

        let bounds = doc.dirty_bounds().unwrap();
        for shape in doc.shapes() {
            let b = shape.bounds();
            assert!(bounds.contains(Point::new(b.x, b.y)));
            assert!(bounds.contains(Point::new(b.right(), b.bottom())));
        }
    }

    #[test]
    fn undo_walks_back_through_frame_changes_too() {
        use crate::frame::Frame;

        let mut doc = Document::new();
        doc.push(rect_at(0.0, 0.0));
        doc.set_frame(Frame {
            padding: 24.0,
            ..Default::default()
        });
        assert_eq!(doc.frame().padding, 24.0);

        // One undo for the frame, one for the shape — the two histories are
        // interleaved, not separate timelines.
        assert!(doc.undo());
        assert_eq!(doc.frame().padding, 0.0);
        assert_eq!(doc.len(), 1);

        assert!(doc.undo());
        assert!(doc.is_empty());

        assert!(doc.redo());
        assert_eq!(doc.len(), 1);
        assert!(doc.redo());
        assert_eq!(doc.frame().padding, 24.0);
    }

    #[test]
    fn setting_the_same_frame_twice_records_no_history() {
        use crate::frame::Frame;

        let mut doc = Document::new();
        let frame = Frame {
            padding: 8.0,
            ..Default::default()
        };
        doc.set_frame(frame);
        let after_first = doc.can_undo();
        doc.set_frame(frame);

        assert!(after_first);
        doc.undo();
        assert_eq!(doc.frame().padding, 0.0, "one undo must clear it");
    }

    #[test]
    fn documents_serialise_without_their_history() {
        let mut doc = Document::new();
        doc.push(rect_at(0.0, 0.0));
        doc.undo();
        doc.redo();

        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();

        assert_eq!(back.len(), 1);
        assert!(
            !back.can_undo() && !back.can_redo(),
            "history is a session concern, not part of the saved file"
        );
    }
}
