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
    #[allow(dead_code)] // Used in sub-slice 6.3
    Hovering(usize),
    /// Actively dragging a node
    #[allow(dead_code)] // Used in sub-slice 6.3
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
    #[allow(dead_code)] // Used in sub-slice 6.3
    pub drag: DragState,
    /// Set of nodes pinned by user (won't move during simulation)
    pub fixed_nodes: HashSet<usize>,
    /// Currently focused node (for dimming unconnected nodes)
    pub focused_node: Option<usize>,
    /// Set of hidden node types (for filtering)
    pub hidden_types: HashSet<String>,
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
    #[allow(dead_code)] // Used in sub-slice 6.3
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
    #[allow(dead_code)] // Used in sub-slice 6.3
    pub fn end_drag(&mut self, keep_fixed: bool) {
        if let DragState::Dragging { node, .. } = self.drag
            && keep_fixed
        {
            self.fixed_nodes.insert(node);
        }
        self.drag = DragState::None;
    }

    /// Toggle the fixed state of a node.
    #[allow(dead_code)] // Used in sub-slice 6.4
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
    #[allow(dead_code)] // Used in sub-slice 6.3
    pub fn is_dragging(&self, node: usize) -> bool {
        matches!(self.drag, DragState::Dragging { node: n, .. } if n == node)
    }

    /// Get the index of the node being dragged, if any.
    #[allow(dead_code)] // Used in sub-slice 6.3
    pub fn dragging_node(&self) -> Option<usize> {
        match self.drag {
            DragState::Dragging { node, .. } => Some(node),
            _ => None,
        }
    }

    /// Set hover state for a node.
    #[allow(dead_code)] // Used in sub-slice 6.3
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
    #[allow(dead_code)] // Used in sub-slice 6.7
    pub fn unfix_node(&mut self, node: usize) {
        self.fixed_nodes.remove(&node);
    }

    /// Set the focused node (for dimming unconnected nodes).
    pub fn focus_node(&mut self, node: usize) {
        self.focused_node = Some(node);
    }

    /// Clear focus mode.
    pub fn clear_focus(&mut self) {
        self.focused_node = None;
    }

    /// Get the currently focused node.
    pub fn focused_node(&self) -> Option<usize> {
        self.focused_node
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
}
