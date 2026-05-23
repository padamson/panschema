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

/// Count the number of edge pairs whose segments cross in the plane.
///
/// Edges sharing an endpoint never count, even when floating-point
/// noise places the shared point fractionally to one side of the
/// other segment — they cross at the shared vertex by definition.
/// Collinear-overlapping segments return false: that's a separate
/// visual case from the proper-crossing one we care about here.
///
/// Out-of-bounds edge indices are silently skipped, mirroring how
/// `SimEdge` construction filters missing targets.
pub fn count_edge_crossings_2d(positions: &[(f32, f32)], edges: &[(usize, usize)]) -> usize {
    let mut count = 0usize;
    for (i, &(a, b)) in edges.iter().enumerate() {
        if a >= positions.len() || b >= positions.len() {
            continue;
        }
        for &(c, d) in &edges[i + 1..] {
            if c >= positions.len() || d >= positions.len() {
                continue;
            }
            if a == c || a == d || b == c || b == d {
                continue;
            }
            if segments_cross(positions[a], positions[b], positions[c], positions[d]) {
                count += 1;
            }
        }
    }
    count
}

/// Returns true iff the open segments (p1,p2) and (p3,p4) cross
/// transversally — i.e. strict opposite-side orientation of each
/// endpoint pair against the other segment. Collinear / endpoint-on-
/// segment cases return false.
fn segments_cross(p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), p4: (f32, f32)) -> bool {
    let d1 = ccw(p3, p4, p1);
    let d2 = ccw(p3, p4, p2);
    let d3 = ccw(p1, p2, p3);
    let d4 = ccw(p1, p2, p4);
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// Signed twice-area of triangle abc; sign is the orientation of the
/// turn from `b→a` to `b→c` (positive = counterclockwise).
fn ccw(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_has_no_crossings() {
        let positions = [(0.0, 0.0), (1.0, 0.0), (0.5, 1.0)];
        let edges = [(0, 1), (1, 2), (2, 0)];
        assert_eq!(count_edge_crossings_2d(&positions, &edges), 0);
    }

    #[test]
    fn square_diagonals_cross_once() {
        let positions = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        let edges = [(0, 2), (1, 3)];
        assert_eq!(count_edge_crossings_2d(&positions, &edges), 1);
    }

    #[test]
    fn star_edges_sharing_center_dont_cross() {
        let positions = [(0.0, 0.0), (1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)];
        let edges = [(0, 1), (0, 2), (0, 3), (0, 4)];
        assert_eq!(count_edge_crossings_2d(&positions, &edges), 0);
    }

    #[test]
    fn parallel_segments_dont_cross() {
        let positions = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)];
        let edges = [(0, 1), (2, 3)];
        assert_eq!(count_edge_crossings_2d(&positions, &edges), 0);
    }

    #[test]
    fn k4_in_convex_position_has_exactly_one_crossing() {
        // 4 corners of a unit square + all 6 edges. The two diagonals
        // cross; the 4 perimeter edges all share endpoints with their
        // neighbors and so don't count.
        let positions = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        let edges = [(0, 1), (1, 2), (2, 3), (3, 0), (0, 2), (1, 3)];
        assert_eq!(count_edge_crossings_2d(&positions, &edges), 1);
    }

    #[test]
    fn out_of_bounds_edge_indices_are_skipped() {
        let positions = [(0.0, 0.0), (1.0, 0.0)];
        let edges = [(0, 1), (5, 7)];
        assert_eq!(count_edge_crossings_2d(&positions, &edges), 0);
    }
}
