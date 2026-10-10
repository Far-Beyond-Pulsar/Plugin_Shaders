//! Blueprint graph container and state management.
//!
//! This module defines the main `BlueprintGraph` type that holds all nodes,
//! connections, comments, and view state for a single blueprint document.

use super::types::{BlueprintComment, BlueprintNode, Connection, VirtualizationStats};
use gpui::*;

/// The main container for a blueprint graph, including all nodes, connections,
/// comments, selection state, and viewport information.
#[derive(Clone, Debug, Default)]
pub struct BlueprintGraph {
    pub nodes: Vec<BlueprintNode>,
    pub connections: Vec<Connection>,
    pub comments: Vec<BlueprintComment>,
    pub selected_nodes: Vec<String>,
    pub selected_comments: Vec<String>,
    pub zoom_level: f32,
    pub pan_offset: Point<f32>,
    pub virtualization_stats: VirtualizationStats,
}

/// A value that counts its mutable borrows, so caches derived from it can
/// tell when it may have changed.
///
/// Every `&mut` access bumps [`Tracked::revision`] — whether or not it
/// actually writes — so a cache keyed on the revision may recompute
/// needlessly but can never go stale. Reads go through `Deref` and leave the
/// revision alone.
#[derive(Debug, Default)]
pub struct Tracked<T> {
    value: T,
    revision: u64,
}

impl<T> Tracked<T> {
    pub fn new(value: T) -> Self {
        Self { value, revision: 0 }
    }

    /// Changes whenever the value may have been mutated.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

impl<T> std::ops::Deref for Tracked<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> std::ops::DerefMut for Tracked<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.revision = self.revision.wrapping_add(1);
        &mut self.value
    }
}

#[cfg(test)]
mod tracked_tests {
    use super::Tracked;

    #[test]
    fn reads_keep_the_revision_and_mutable_access_bumps_it() {
        let mut graph = Tracked::new(super::BlueprintGraph::default());
        let start = graph.revision();

        let _ = graph.nodes.len();
        let _ = &graph.connections;
        assert_eq!(graph.revision(), start);

        graph.zoom_level = 2.0;
        assert_ne!(graph.revision(), start);

        let after_write = graph.revision();
        *graph = super::BlueprintGraph::default();
        assert_ne!(graph.revision(), after_write);
    }
}
