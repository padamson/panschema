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

    #[test]
    fn partially_out_of_bounds_edge_skipped() {
        // One endpoint in bounds, the other not — the bounds check must
        // be `||` (either out-of-bounds triggers skip), not `&&` (both).
        let positions = [(0.0, 0.0), (1.0, 0.0)];
        let edges = [(0, 1), (0, 7)];
        assert_eq!(count_edge_crossings_2d(&positions, &edges), 0);
    }

    #[test]
    fn t_junction_doesnt_cross_when_one_segment_lies_above() {
        // AB is the x-axis segment from (0,0) to (2,0). CD is a vertical
        // segment from (1,1) to (1,2), above AB. A and B are on opposite
        // sides of CD's infinite line (x-axis splits at x=1) — so the
        // first half of the segments-cross predicate would say "yes" —
        // but C and D are both above AB, so the second half says "no".
        // Proper crossing requires BOTH halves to be true. Catches an
        // `&&` → `||` swap in the conjunction between halves.
        let positions = [(0.0, 0.0), (2.0, 0.0), (1.0, 1.0), (1.0, 2.0)];
        let edges = [(0, 1), (2, 3)];
        assert_eq!(count_edge_crossings_2d(&positions, &edges), 0);
    }

    #[test]
    fn t_junction_doesnt_cross_when_first_segment_misses_second() {
        // Mirror of the case above: AB is on one side of CD's line, so
        // its predicate-half says "no." CD straddles AB's line, so its
        // half says "yes." Catches an `&&` → `||` swap.
        let positions = [(0.0, 1.0), (2.0, 1.0), (1.0, -1.0), (1.0, 2.0)];
        let edges = [(0, 1), (2, 3)];
        // AB is at y=1, CD spans y=-1 to y=2 vertically at x=1. CD
        // crosses AB's line at (1, 1) — which is on AB. So this is
        // a genuine crossing.
        assert_eq!(count_edge_crossings_2d(&positions, &edges), 1);
    }

    #[test]
    fn endpoints_on_same_side_no_cross() {
        // Two segments both above the x-axis, neither crossing the
        // other. Specifically chosen so the orientation predicates
        // produce two same-signed values per pair — catches the
        // inner `||` mutants in the conjunctions like
        // `(d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)`.
        let positions = [(0.0, 1.0), (2.0, 1.0), (3.0, 2.0), (5.0, 2.0)];
        let edges = [(0, 1), (2, 3)];
        assert_eq!(count_edge_crossings_2d(&positions, &edges), 0);
    }

    #[test]
    fn endpoint_on_other_segment_doesnt_count() {
        // A=(0,0), B=(2,0); C=(1,0), D=(1,1). C lies exactly on AB.
        // The CCW orientation predicate of A vs CD gives d3 = 0
        // (collinear). The strict-inequality test `> 0 && < 0` fails
        // when one side is 0, so this returns "no cross" — collinear
        // touches aren't proper crossings. Catches `> 0` → `>= 0`
        // boundary mutants that would let the zero through.
        let positions = [(0.0, 0.0), (2.0, 0.0), (1.0, 0.0), (1.0, 1.0)];
        let edges = [(0, 1), (2, 3)];
        assert_eq!(count_edge_crossings_2d(&positions, &edges), 0);
    }

    #[test]
    fn ccw_returns_zero_for_collinear_three_points() {
        // Direct test of the CCW helper. Three collinear points
        // produce a zero cross-product. Catches `-` → `+` in the
        // signed-area expression: the result would be non-zero for
        // collinear inputs.
        let v = ccw((0.0, 0.0), (1.0, 0.0), (2.0, 0.0));
        assert!(v.abs() < 1e-6, "collinear ccw should be ~0, got {v}");
    }

    #[test]
    fn ccw_sign_distinguishes_orientation() {
        // Point above the directed segment (0,0)→(1,0) gives positive
        // CCW; below gives negative. Catches sign-flip mutants in the
        // CCW expression.
        let above = ccw((0.0, 0.0), (1.0, 0.0), (0.5, 1.0));
        let below = ccw((0.0, 0.0), (1.0, 0.0), (0.5, -1.0));
        assert!(above > 0.0, "above-segment point should be ccw-positive");
        assert!(below < 0.0, "below-segment point should be ccw-negative");
    }
}
