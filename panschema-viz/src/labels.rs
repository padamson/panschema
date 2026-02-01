//! Label visibility state management
//!
//! Controls which labels are shown in the graph visualization.

/// Label visibility options
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelOptions {
    /// Master toggle for all labels
    pub all_labels: bool,
    /// Show node labels (when all_labels is true)
    pub node_labels: bool,
    /// Show edge labels (when all_labels is true)
    pub edge_labels: bool,
}

impl Default for LabelOptions {
    fn default() -> Self {
        Self {
            all_labels: true,
            node_labels: true,
            edge_labels: true,
        }
    }
}

impl LabelOptions {
    /// Create new label options with all labels visible
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if node labels should be displayed
    pub fn show_node_labels(&self) -> bool {
        self.all_labels && self.node_labels
    }

    /// Check if edge labels should be displayed
    pub fn show_edge_labels(&self) -> bool {
        self.all_labels && self.edge_labels
    }

    /// Toggle all labels on/off
    pub fn toggle_all(&mut self) {
        self.all_labels = !self.all_labels;
    }

    /// Toggle node labels on/off
    pub fn toggle_node_labels(&mut self) {
        self.node_labels = !self.node_labels;
    }

    /// Toggle edge labels on/off
    pub fn toggle_edge_labels(&mut self) {
        self.edge_labels = !self.edge_labels;
    }

    /// Set all labels visibility
    pub fn set_all(&mut self, visible: bool) {
        self.all_labels = visible;
    }

    /// Set node labels visibility
    pub fn set_node_labels(&mut self, visible: bool) {
        self.node_labels = visible;
    }

    /// Set edge labels visibility
    pub fn set_edge_labels(&mut self, visible: bool) {
        self.edge_labels = visible;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_all_labels_visible() {
        let opts = LabelOptions::default();
        assert!(opts.all_labels);
        assert!(opts.node_labels);
        assert!(opts.edge_labels);
        assert!(opts.show_node_labels());
        assert!(opts.show_edge_labels());
    }

    #[test]
    fn toggle_all_disables_both() {
        let mut opts = LabelOptions::new();
        opts.toggle_all();

        assert!(!opts.all_labels);
        assert!(!opts.show_node_labels());
        assert!(!opts.show_edge_labels());
    }

    #[test]
    fn toggle_all_twice_restores() {
        let mut opts = LabelOptions::new();
        opts.toggle_all();
        opts.toggle_all();

        assert!(opts.show_node_labels());
        assert!(opts.show_edge_labels());
    }

    #[test]
    fn toggle_node_labels_only() {
        let mut opts = LabelOptions::new();
        opts.toggle_node_labels();

        assert!(!opts.show_node_labels());
        assert!(opts.show_edge_labels());
    }

    #[test]
    fn toggle_edge_labels_only() {
        let mut opts = LabelOptions::new();
        opts.toggle_edge_labels();

        assert!(opts.show_node_labels());
        assert!(!opts.show_edge_labels());
    }

    #[test]
    fn all_labels_off_overrides_individual() {
        let mut opts = LabelOptions::new();
        opts.set_all(false);

        // Individual flags are still true, but show_ methods return false
        assert!(opts.node_labels);
        assert!(opts.edge_labels);
        assert!(!opts.show_node_labels());
        assert!(!opts.show_edge_labels());
    }

    #[test]
    fn set_methods_work() {
        let mut opts = LabelOptions::new();

        opts.set_node_labels(false);
        assert!(!opts.show_node_labels());
        assert!(opts.show_edge_labels());

        opts.set_edge_labels(false);
        assert!(!opts.show_node_labels());
        assert!(!opts.show_edge_labels());

        opts.set_node_labels(true);
        opts.set_edge_labels(true);
        assert!(opts.show_node_labels());
        assert!(opts.show_edge_labels());
    }
}
