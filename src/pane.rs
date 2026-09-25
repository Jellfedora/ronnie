//! Split layout of a tab: a binary tree whose leaves are pane ids.

use egui::{CursorIcon, Id, Pos2, Rect, Sense, Ui, Vec2};

pub type PaneId = u64;

/// Width of the grab area between two panes (the visible line is 1px).
const DIVIDER_GRAB: f32 = 6.0;
/// Panes stop short of the divider so its grab area never overlaps a terminal.
const HALF_GRAB: f32 = DIVIDER_GRAB / 2.0;
const MIN_RATIO: f32 = 0.1;

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    /// Children side by side.
    Horizontal,
    /// Children stacked.
    Vertical,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug)]
pub enum Node {
    Leaf(PaneId),
    Split { axis: Axis, ratio: f32, a: Box<Node>, b: Box<Node> },
}

impl Node {
    /// Replaces the leaf `target` by a split holding it and `new`, placed on the `side` of it.
    pub fn split(&mut self, target: PaneId, new: PaneId, side: Direction) -> bool {
        match self {
            Node::Leaf(id) if *id == target => {
                let axis = match side {
                    Direction::Left | Direction::Right => Axis::Horizontal,
                    Direction::Up | Direction::Down => Axis::Vertical,
                };
                let (old, new) = (Box::new(Node::Leaf(target)), Box::new(Node::Leaf(new)));
                let (a, b) = if matches!(side, Direction::Left | Direction::Up) { (new, old) } else { (old, new) };
                *self = Node::Split { axis, ratio: 0.5, a, b };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => a.split(target, new, side) || b.split(target, new, side),
        }
    }

    /// Removes a leaf; its sibling takes the parent's place. Returns false if `target` is the only leaf.
    pub fn remove(&mut self, target: PaneId) -> bool {
        let Node::Split { a, b, .. } = self else { return false };
        let keep = if matches!(**a, Node::Leaf(id) if id == target) {
            b
        } else if matches!(**b, Node::Leaf(id) if id == target) {
            a
        } else {
            return a.remove(target) || b.remove(target);
        };
        let keep = std::mem::replace(&mut **keep, Node::Leaf(0));
        *self = keep;
        true
    }

    /// Pane ids in layout order (left/top first).
    pub fn leaves(&self) -> Vec<PaneId> {
        match self {
            Node::Leaf(id) => vec![*id],
            Node::Split { a, b, .. } => {
                let mut v = a.leaves();
                v.extend(b.leaves());
                v
            }
        }
    }

    pub fn first_leaf(&self) -> PaneId {
        match self {
            Node::Leaf(id) => *id,
            Node::Split { a, .. } => a.first_leaf(),
        }
    }

    /// Lays out the tree in `rect`, handling divider drags. Returns each pane's rect.
    pub fn show(&mut self, ui: &Ui, rect: Rect, id: Id, line: egui::Stroke, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            Node::Leaf(pane) => out.push((*pane, rect)),
            Node::Split { axis, ratio, a, b } => {
                let (total, start) = match axis {
                    Axis::Horizontal => (rect.width(), rect.min.x),
                    Axis::Vertical => (rect.height(), rect.min.y),
                };
                let at = start + total * *ratio;
                let (ra, rb, divider) = match axis {
                    Axis::Horizontal => (
                        Rect::from_min_max(rect.min, Pos2::new(at - HALF_GRAB, rect.max.y)),
                        Rect::from_min_max(Pos2::new(at + 1.0 + HALF_GRAB, rect.min.y), rect.max),
                        Rect::from_center_size(Pos2::new(at + 0.5, rect.center().y), Vec2::new(DIVIDER_GRAB + 1.0, rect.height())),
                    ),
                    Axis::Vertical => (
                        Rect::from_min_max(rect.min, Pos2::new(rect.max.x, at - HALF_GRAB)),
                        Rect::from_min_max(Pos2::new(rect.min.x, at + 1.0 + HALF_GRAB), rect.max),
                        Rect::from_center_size(Pos2::new(rect.center().x, at + 0.5), Vec2::new(rect.width(), DIVIDER_GRAB + 1.0)),
                    ),
                };

                a.show(ui, ra, id.with("a"), line, out);
                b.show(ui, rb, id.with("b"), line, out);

                let resp = ui.interact(divider, id.with("divider"), Sense::drag());
                let cursor = match axis {
                    Axis::Horizontal => CursorIcon::ResizeHorizontal,
                    Axis::Vertical => CursorIcon::ResizeVertical,
                };
                if resp.hovered() || resp.dragged() {
                    ui.ctx().set_cursor_icon(cursor);
                }
                if resp.dragged() {
                    if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                        let pos = if *axis == Axis::Horizontal { p.x } else { p.y };
                        *ratio = ((pos - start) / total).clamp(MIN_RATIO, 1.0 - MIN_RATIO);
                    }
                }
                let painter = ui.painter();
                match axis {
                    Axis::Horizontal => painter.vline(at + 0.5, rect.y_range(), line),
                    Axis::Vertical => painter.hline(rect.x_range(), at + 0.5, line),
                };
            }
        }
    }
}

/// The pane closest to `from` in `dir`, among panes overlapping it on the other axis.
pub fn neighbor(rects: &[(PaneId, Rect)], from: PaneId, dir: Direction) -> Option<PaneId> {
    let (_, src) = rects.iter().find(|(id, _)| *id == from)?;
    rects
        .iter()
        .filter(|(id, _)| *id != from)
        .filter_map(|(id, r)| {
            let (gap, overlap) = match dir {
                Direction::Left => (src.min.x - r.max.x, overlap(src.y_range(), r.y_range())),
                Direction::Right => (r.min.x - src.max.x, overlap(src.y_range(), r.y_range())),
                Direction::Up => (src.min.y - r.max.y, overlap(src.x_range(), r.x_range())),
                Direction::Down => (r.min.y - src.max.y, overlap(src.x_range(), r.x_range())),
            };
            (gap > -2.0 && overlap > 0.0).then_some((*id, gap, overlap))
        })
        // Nearest first, then the one sharing the most edge.
        .min_by(|x, y| x.1.total_cmp(&y.1).then(y.2.total_cmp(&x.2)))
        .map(|(id, ..)| id)
}

fn overlap(a: egui::Rangef, b: egui::Rangef) -> f32 {
    a.max.min(b.max) - a.min.max(b.min)
}
