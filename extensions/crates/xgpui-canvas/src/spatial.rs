use crate::{Point, SceneItem, WorldBounds};
use std::ops::Range;

const LEAF_SIZE: usize = 8;

struct Node {
    parent: Option<usize>,
    bounds: WorldBounds,
    items: Range<usize>,
    children: Option<(usize, usize)>,
}

/// Median-split AABB hierarchy with bounds refitting. IDs remain paint indices.
pub(crate) struct SpatialIndex {
    nodes: Vec<Node>,
    order: Vec<usize>,
    leaves: Vec<usize>,
}

impl SpatialIndex {
    pub fn bounds(&self) -> Option<WorldBounds> {
        self.nodes.first().map(|node| node.bounds)
    }
    pub fn new<T>(items: &[SceneItem<T>]) -> Self {
        let mut index = Self {
            nodes: Vec::new(),
            order: (0..items.len()).collect(),
            leaves: vec![0; items.len()],
        };
        if !items.is_empty() {
            index.build(items, 0..items.len());
        }
        index
    }

    fn build<T>(&mut self, items: &[SceneItem<T>], range: Range<usize>) -> usize {
        let mut bounds = items[self.order[range.start]].bounds;
        for &index in &self.order[range.clone()] {
            let item = items[index].bounds;
            bounds.min.x = bounds.min.x.min(item.min.x);
            bounds.min.y = bounds.min.y.min(item.min.y);
            bounds.max.x = bounds.max.x.max(item.max.x);
            bounds.max.y = bounds.max.y.max(item.max.y);
        }
        let node = self.nodes.len();
        self.nodes.push(Node {
            parent: None,
            bounds,
            items: range.clone(),
            children: None,
        });
        if range.len() > LEAF_SIZE {
            let split_x = bounds.max.x - bounds.min.x >= bounds.max.y - bounds.min.y;
            let mid = range.start + range.len() / 2;
            self.order[range.clone()].select_nth_unstable_by(range.len() / 2, |&a, &b| {
                let center = |index: usize| {
                    let b = items[index].bounds;
                    // Halve first so valid extreme finite coordinates do not overflow.
                    if split_x {
                        b.min.x * 0.5 + b.max.x * 0.5
                    } else {
                        b.min.y * 0.5 + b.max.y * 0.5
                    }
                };
                center(a).total_cmp(&center(b)).then(a.cmp(&b))
            });
            let left = self.build(items, range.start..mid);
            let right = self.build(items, mid..range.end);
            self.nodes[node].children = Some((left, right));
            self.nodes[left].parent = Some(node);
            self.nodes[right].parent = Some(node);
        } else {
            for &item in &self.order[range] {
                self.leaves[item] = node;
            }
        }
        node
    }

    pub fn refit<T>(&mut self, items: &[SceneItem<T>], item: usize) {
        let mut next = Some(self.leaves[item]);
        while let Some(index) = next {
            let mut bounds = if let Some((left, _)) = self.nodes[index].children {
                self.nodes[left].bounds
            } else {
                items[self.order[self.nodes[index].items.start]].bounds
            };
            if let Some((_, right)) = self.nodes[index].children {
                bounds = union(bounds, self.nodes[right].bounds);
            } else {
                for &item in &self.order[self.nodes[index].items.clone()] {
                    bounds = union(bounds, items[item].bounds);
                }
            }
            self.nodes[index].bounds = bounds;
            next = self.nodes[index].parent;
        }
    }

    pub fn query<T>(&self, items: &[SceneItem<T>], bounds: WorldBounds) -> Vec<usize> {
        let mut matches = Vec::new();
        self.visit(bounds, |node| {
            for &index in &self.order[node.items.clone()] {
                if items[index].bounds.intersects(bounds) {
                    matches.push(index);
                }
            }
        });
        // Traversal follows spatial partitions; restore the caller's paint order.
        matches.sort_unstable();
        matches
    }

    pub fn hit_test<T>(&self, items: &[SceneItem<T>], point: Point) -> Option<usize> {
        let mut best = None;
        self.visit(
            WorldBounds {
                min: point,
                max: point,
            },
            |node| {
                for &index in &self.order[node.items.clone()] {
                    if best.is_none_or(|best| index > best) && items[index].bounds.contains(point) {
                        best = Some(index);
                    }
                }
            },
        );
        best
    }

    fn visit(&self, bounds: WorldBounds, mut leaf: impl FnMut(&Node)) {
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0];
        while let Some(index) = stack.pop() {
            let node = &self.nodes[index];
            if !node.bounds.intersects(bounds) {
                continue;
            }
            if let Some((left, right)) = node.children {
                stack.push(right);
                stack.push(left);
            } else {
                leaf(node);
            }
        }
    }
}

fn union(a: WorldBounds, b: WorldBounds) -> WorldBounds {
    WorldBounds {
        min: Point::new(a.min.x.min(b.min.x), a.min.y.min(b.min.y)),
        max: Point::new(a.max.x.max(b.max.x), a.max.y.max(b.max.y)),
    }
}
