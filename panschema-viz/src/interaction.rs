//! Interaction state management for graph visualization.
//!
//! Handles selection, dragging, and fixed node state.

use std::collections::HashSet;

/// Drag state machine for node manipulation.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum DragState {
    /// No drag in progress
    #[default]
    None,
    /// Mouse is over a node (for cursor feedback)
    #[allow(dead_code)]
    Hovering(usize),
    /// Actively dragging a node
    #[allow(dead_code)]
    Dragging {
        node: usize,
        /// Starting canvas X coordinate
        start_x: f32,
        /// Starting canvas Y coordinate
        start_y: f32,
    },
}

/// Manages interaction state for the graph visualization.
#[derive(Debug, Default)]
pub struct InteractionState {
    /// Currently selected node (for details panel, highlighting)
    pub selected_node: Option<usize>,
    /// Current drag state
    #[allow(dead_code)]
    pub drag: DragState,
    /// Set of nodes pinned by user (won't move during simulation)
    pub fixed_nodes: HashSet<usize>,
    /// Currently focused node (for dimming unconnected nodes)
    pub focused_node: Option<usize>,
    /// Precomputed set of nodes within `focused_node`'s neighborhood
    /// (1-hop ∪ 2-hop ∪ … up to the configured max-hop depth). Cached
    /// at `focus_node()` time so the renderer doesn't re-walk the
    /// adjacency every frame; cleared by `clear_focus()`. Empty when
    /// `focused_node` is `None`.
    pub focused_neighbors: HashSet<usize>,
    /// Set of hidden node types (for filtering)
    pub hidden_types: HashSet<String>,
    /// Nodes emphasized transiently while the user hovers a rule (its
    /// trigger/governed slots and owning class). Empty when nothing is
    /// hovered; drawn with a highlight ring, dimming the rest.
    pub highlighted_nodes: HashSet<usize>,
}

impl InteractionState {
    /// Create a new interaction state with no selection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Select a node by index, or deselect if None.
    pub fn select_node(&mut self, index: Option<usize>) {
        self.selected_node = index;
    }

    /// Deselect the current node.
    pub fn deselect(&mut self) {
        self.selected_node = None;
    }

    /// Start dragging a node from the given canvas position.
    #[allow(dead_code)]
    pub fn start_drag(&mut self, node: usize, x: f32, y: f32) {
        self.drag = DragState::Dragging {
            node,
            start_x: x,
            start_y: y,
        };
        // Select the node being dragged
        self.selected_node = Some(node);
    }

    /// End the current drag operation.
    ///
    /// If `keep_fixed` is true, the node will remain fixed after release.
    #[allow(dead_code)]
    pub fn end_drag(&mut self, keep_fixed: bool) {
        if let DragState::Dragging { node, .. } = self.drag
            && keep_fixed
        {
            self.fixed_nodes.insert(node);
        }
        self.drag = DragState::None;
    }

    /// Toggle the fixed state of a node.
    #[allow(dead_code)]
    pub fn toggle_fixed(&mut self, node: usize) {
        if self.fixed_nodes.contains(&node) {
            self.fixed_nodes.remove(&node);
        } else {
            self.fixed_nodes.insert(node);
        }
    }

    /// Check if a node is currently fixed.
    pub fn is_fixed(&self, node: usize) -> bool {
        self.fixed_nodes.contains(&node)
    }

    /// Check if a specific node is being dragged.
    #[allow(dead_code)]
    pub fn is_dragging(&self, node: usize) -> bool {
        matches!(self.drag, DragState::Dragging { node: n, .. } if n == node)
    }

    /// Get the index of the node being dragged, if any.
    #[allow(dead_code)]
    pub fn dragging_node(&self) -> Option<usize> {
        match self.drag {
            DragState::Dragging { node, .. } => Some(node),
            _ => None,
        }
    }

    /// Set hover state for a node.
    #[allow(dead_code)]
    pub fn set_hover(&mut self, node: Option<usize>) {
        match node {
            Some(n) if !matches!(self.drag, DragState::Dragging { .. }) => {
                self.drag = DragState::Hovering(n);
            }
            None if matches!(self.drag, DragState::Hovering(_)) => {
                self.drag = DragState::None;
            }
            _ => {}
        }
    }

    /// Unfix a node, allowing it to move freely in the simulation.
    #[allow(dead_code)]
    pub fn unfix_node(&mut self, node: usize) {
        self.fixed_nodes.remove(&node);
    }

    /// Set the focused node (for dimming unconnected nodes) and
    /// precompute the neighborhood up to `max_hops` away. Both the
    /// hovered node itself and every node reachable within that hop
    /// distance render at full opacity; the rest dim. `max_hops = 2`
    /// is the schema-author sweet spot: 1-hop shows direct
    /// connections, 2-hop reveals the local cluster without dragging
    /// in the whole graph.
    pub fn focus_node(&mut self, node: usize, edges: &[(usize, usize)], max_hops: usize) {
        self.focused_node = Some(node);
        self.focused_neighbors.clear();
        if max_hops == 0 {
            return;
        }
        // BFS expansion from `node` to depth `max_hops`. The frontier
        // doubles as the "what was just added" set; the next iteration
        // walks edges from those nodes only, so the total work is
        // O(visited × avg_degree) — sub-millisecond for any schema in
        // practice.
        let mut frontier: HashSet<usize> = HashSet::from([node]);
        let mut visited: HashSet<usize> = HashSet::from([node]);
        for _ in 0..max_hops {
            let mut next_frontier: HashSet<usize> = HashSet::new();
            for &(src, tgt) in edges {
                if frontier.contains(&src) && !visited.contains(&tgt) {
                    next_frontier.insert(tgt);
                }
                if frontier.contains(&tgt) && !visited.contains(&src) {
                    next_frontier.insert(src);
                }
            }
            if next_frontier.is_empty() {
                break;
            }
            visited.extend(&next_frontier);
            frontier = next_frontier;
        }
        // `visited` includes the focal node — drop it from the
        // neighbors set so the renderer can keep the focal-vs-neighbor
        // distinction it already maintains.
        visited.remove(&node);
        self.focused_neighbors = visited;
    }

    /// Clear focus mode.
    pub fn clear_focus(&mut self) {
        self.focused_node = None;
        self.focused_neighbors.clear();
    }

    /// Get the currently focused node.
    pub fn focused_node(&self) -> Option<usize> {
        self.focused_node
    }

    /// Replace the highlighted-node set (a rule's participant nodes).
    pub fn set_highlight(&mut self, nodes: impl IntoIterator<Item = usize>) {
        self.highlighted_nodes = nodes.into_iter().collect();
    }

    /// Clear the highlight (hover ended).
    pub fn clear_highlight(&mut self) {
        self.highlighted_nodes.clear();
    }

    /// Hide a node type from display.
    #[allow(dead_code)]
    pub fn hide_type(&mut self, node_type: &str) {
        self.hidden_types.insert(node_type.to_string());
    }

    /// Show a node type that was hidden.
    #[allow(dead_code)]
    pub fn show_type(&mut self, node_type: &str) {
        self.hidden_types.remove(node_type);
    }

    /// Toggle visibility of a node type.
    pub fn toggle_type(&mut self, node_type: &str) {
        if self.hidden_types.contains(node_type) {
            self.hidden_types.remove(node_type);
        } else {
            self.hidden_types.insert(node_type.to_string());
        }
    }

    /// Check if a node type is visible.
    pub fn is_type_visible(&self, node_type: &str) -> bool {
        !self.hidden_types.contains(node_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_no_selection() {
        let state = InteractionState::new();
        assert_eq!(state.selected_node, None);
        assert_eq!(state.drag, DragState::None);
        assert!(state.fixed_nodes.is_empty());
    }

    #[test]
    fn select_node_updates_state() {
        let mut state = InteractionState::new();
        state.select_node(Some(5));
        assert_eq!(state.selected_node, Some(5));

        state.select_node(Some(10));
        assert_eq!(state.selected_node, Some(10));
    }

    #[test]
    fn deselect_clears_selection() {
        let mut state = InteractionState::new();
        state.select_node(Some(5));
        state.deselect();
        assert_eq!(state.selected_node, None);
    }

    #[test]
    fn start_drag_sets_state() {
        let mut state = InteractionState::new();
        state.start_drag(3, 100.0, 200.0);

        assert_eq!(
            state.drag,
            DragState::Dragging {
                node: 3,
                start_x: 100.0,
                start_y: 200.0
            }
        );
        // Starting drag also selects the node
        assert_eq!(state.selected_node, Some(3));
    }

    #[test]
    fn end_drag_clears_state() {
        let mut state = InteractionState::new();
        state.start_drag(3, 100.0, 200.0);
        state.end_drag(false);

        assert_eq!(state.drag, DragState::None);
        assert!(!state.is_fixed(3));
    }

    #[test]
    fn end_drag_with_keep_fixed_adds_to_set() {
        let mut state = InteractionState::new();
        state.start_drag(3, 100.0, 200.0);
        state.end_drag(true);

        assert_eq!(state.drag, DragState::None);
        assert!(state.is_fixed(3));
    }

    #[test]
    fn toggle_fixed_adds_and_removes() {
        let mut state = InteractionState::new();

        // First toggle adds
        state.toggle_fixed(5);
        assert!(state.is_fixed(5));

        // Second toggle removes
        state.toggle_fixed(5);
        assert!(!state.is_fixed(5));

        // Third toggle adds again
        state.toggle_fixed(5);
        assert!(state.is_fixed(5));
    }

    #[test]
    fn is_fixed_returns_correct_value() {
        let mut state = InteractionState::new();

        assert!(!state.is_fixed(0));
        assert!(!state.is_fixed(1));

        state.fixed_nodes.insert(1);
        assert!(!state.is_fixed(0));
        assert!(state.is_fixed(1));
    }

    #[test]
    fn is_dragging_checks_specific_node() {
        let mut state = InteractionState::new();
        state.start_drag(3, 100.0, 200.0);

        assert!(state.is_dragging(3));
        assert!(!state.is_dragging(0));
        assert!(!state.is_dragging(5));
    }

    #[test]
    fn dragging_node_returns_index() {
        let mut state = InteractionState::new();
        assert_eq!(state.dragging_node(), None);

        state.start_drag(7, 50.0, 50.0);
        assert_eq!(state.dragging_node(), Some(7));

        state.end_drag(false);
        assert_eq!(state.dragging_node(), None);
    }

    #[test]
    fn set_hover_updates_drag_state() {
        let mut state = InteractionState::new();

        state.set_hover(Some(2));
        assert_eq!(state.drag, DragState::Hovering(2));

        state.set_hover(None);
        assert_eq!(state.drag, DragState::None);
    }

    #[test]
    fn set_hover_does_not_interrupt_drag() {
        let mut state = InteractionState::new();
        state.start_drag(3, 100.0, 200.0);

        // Hover should not change drag state while dragging
        state.set_hover(Some(5));
        assert!(state.is_dragging(3));

        state.set_hover(None);
        assert!(state.is_dragging(3));
    }

    #[test]
    fn unfix_node_removes_from_set() {
        let mut state = InteractionState::new();
        state.fixed_nodes.insert(5);
        assert!(state.is_fixed(5));

        state.unfix_node(5);
        assert!(!state.is_fixed(5));
    }

    // Edge fixture: a 5-node path graph 0—1—2—3—4. From node 0,
    // 1-hop = {1}, 2-hop = {1, 2}, 3-hop = {1, 2, 3}. Used by the
    // focus-mode BFS expansion tests below.
    fn make_path_edges() -> Vec<(usize, usize)> {
        vec![(0, 1), (1, 2), (2, 3), (3, 4)]
    }

    #[test]
    fn focus_node_with_zero_hops_clears_neighbors_but_sets_focal() {
        // max_hops = 0 means "focus this node alone, dim everything
        // else". The focal node itself stays set; the neighbor set
        // is empty.
        let mut state = InteractionState::new();
        state.focus_node(2, &make_path_edges(), 0);
        assert_eq!(state.focused_node, Some(2));
        assert!(state.focused_neighbors.is_empty());
    }

    #[test]
    fn focus_node_with_one_hop_finds_direct_neighbors_only() {
        // From node 2 (middle of the path), 1-hop = {1, 3}. The
        // focal node 2 must not appear in the neighbors set so the
        // renderer can keep the focal-vs-neighbor distinction.
        let mut state = InteractionState::new();
        state.focus_node(2, &make_path_edges(), 1);
        assert_eq!(state.focused_node, Some(2));
        assert_eq!(state.focused_neighbors, HashSet::from([1, 3]));
    }

    #[test]
    fn focus_node_with_two_hops_finds_neighbors_and_their_neighbors() {
        // From node 0, 1-hop = {1}, 2-hop adds {2}; the full set
        // returned is {1, 2}. Node 4 is 4 hops away and stays
        // outside the focus set, dimmed at render time.
        let mut state = InteractionState::new();
        state.focus_node(0, &make_path_edges(), 2);
        assert_eq!(state.focused_neighbors, HashSet::from([1, 2]));
    }

    #[test]
    fn focus_node_with_overshoot_hops_stops_at_graph_boundary() {
        // Asking for more hops than the graph supports terminates
        // gracefully when the BFS frontier goes empty (no new nodes
        // reachable). The 5-node path graph has diameter 4; max_hops
        // = 10 from one endpoint still yields just the other 4
        // nodes.
        let mut state = InteractionState::new();
        state.focus_node(0, &make_path_edges(), 10);
        assert_eq!(state.focused_neighbors, HashSet::from([1, 2, 3, 4]));
    }

    #[test]
    fn focus_node_handles_disconnected_node_with_no_neighbors() {
        // An isolated node (no incident edges) has an empty
        // neighborhood at any hop depth. The focal node is still
        // set so the renderer can highlight it solo.
        let mut state = InteractionState::new();
        state.focus_node(7, &[], 2);
        assert_eq!(state.focused_node, Some(7));
        assert!(state.focused_neighbors.is_empty());
    }

    #[test]
    fn clear_focus_removes_focal_and_neighbors() {
        let mut state = InteractionState::new();
        state.focus_node(0, &make_path_edges(), 2);
        assert!(!state.focused_neighbors.is_empty());

        state.clear_focus();
        assert_eq!(state.focused_node, None);
        assert!(state.focused_neighbors.is_empty());
    }
}
