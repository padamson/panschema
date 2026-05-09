//! Helpers shared between the 2D (`simulation`) and 3D (`simulation3d`)
//! force simulations. Anything in here must be dimension-agnostic — it
//! works on indices and edge endpoints, not positions.

/// Assign each node a connected-component id (0-based) by BFS over the
/// undirected edge graph. Nodes with no edges become their own component.
pub(crate) fn compute_components(
    num_nodes: usize,
    edge_endpoints: impl IntoIterator<Item = (usize, usize)>,
) -> Vec<usize> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); num_nodes];
    for (source, target) in edge_endpoints {
        adj[source].push(target);
        adj[target].push(source);
    }
    let mut component = vec![usize::MAX; num_nodes];
    let mut next_id = 0usize;
    for start in 0..num_nodes {
        if component[start] != usize::MAX {
            continue;
        }
        let mut queue = std::collections::VecDeque::from([start]);
        component[start] = next_id;
        while let Some(node) = queue.pop_front() {
            for &neighbor in &adj[node] {
                if component[neighbor] == usize::MAX {
                    component[neighbor] = next_id;
                    queue.push_back(neighbor);
                }
            }
        }
        next_id += 1;
    }
    component
}
