// Vendored from egraph-rs (https://github.com/likr/egraph-rs), MIT licensed.
// Copyright (c) the egraph-rs authors. See THIRD-PARTY.md for the revision
// and for what was and was not vendored.

use ndarray::prelude::*;
use petgraph::visit::IntoNodeIdentifiers;
use std::{collections::HashMap, hash::Hash};

/// A trait representing a distance matrix, used for graph algorithms.
///
/// This trait provides methods to access and modify distances between nodes,
/// represented by both their identifier (`N`) and their internal index (`usize`).
/// The nodes in the rows and columns can be different sets.
///
/// `N` is the type of the node identifier (e.g., `NodeIndex`).
/// `S` is the type of the distance value, typically a floating-point type implementing `NdFloat`.
pub trait DistanceMatrix<N, S> {
    /// Returns the distance between nodes `u` (row) and `v` (column).
    ///
    /// Returns `Option::None` if either `u` is not in the row domain or `v` is not in the column domain.
    fn get(&self, u: N, v: N) -> Option<S>;

    /// Sets the distance between nodes `u` (row) and `v` (column) to `d`.
    ///
    /// Returns `Option::None` if either `u` is not in the row domain or `v` is not in the column domain.
    /// Returns `Some(())` on success.
    fn set(&mut self, u: N, v: N, d: S) -> Option<()>;

    /// Returns the distance between the node at row index `i` and the node at column index `j`.
    ///
    /// This method bypasses the hash lookup for node identifiers and is thus potentially faster than `get`.
    ///
    /// # Panics
    ///
    /// Panics if `i` or `j` is out of bounds for the matrix dimensions.
    fn get_by_index(&self, i: usize, j: usize) -> S;

    /// Sets the distance between the node at row index `i` and the node at column index `j` to `d`.
    ///
    /// This method bypasses the hash lookup for node identifiers and is thus potentially faster than `set`.
    ///
    /// # Panics
    ///
    /// Panics if `i` or `j` is out of bounds for the matrix dimensions.
    fn set_by_index(&mut self, i: usize, j: usize, d: S);

    /// Returns the dimensions (number of rows, number of columns) of the distance matrix.
    fn shape(&self) -> (usize, usize);

    /// Returns the row index associated with node identifier `u`.
    ///
    /// Returns `Option::None` if `u` is not found in the row indices.
    fn row_index(&self, u: N) -> Option<usize>;

    /// Returns the column index associated with node identifier `u`.
    ///
    /// Returns `Option::None` if `u` is not found in the column indices.
    fn col_index(&self, u: N) -> Option<usize>;

    /// Returns an iterator over the node identifiers corresponding to the rows.
    fn row_indices(&'_ self) -> IndexIterator<'_, N>;

    /// Returns an iterator over the node identifiers corresponding to the columns.
    fn col_indices(&'_ self) -> IndexIterator<'_, N>;
}

/// An iterator over the node identifiers in a distance matrix dimension (row or column).
pub struct IndexIterator<'a, N> {
    indices: &'a Vec<N>,
    index: usize,
}

impl<N> Iterator for IndexIterator<'_, N>
where
    N: Copy,
{
    type Item = N;
    fn next(&mut self) -> Option<Self::Item> {
        let index = self.index;
        self.index += 1;
        if index < self.indices.len() {
            Some(self.indices[index])
        } else {
            None
        }
    }
}

/// A distance matrix where the rows and columns represent the same set of nodes,
/// typically all nodes in the graph.
///
/// This implementation uses an `ndarray::Array2` internally.
/// Node identifiers (`N`) are mapped to `usize` indices for array access.
pub struct FullDistanceMatrix<N, S> {
    /// Vector of node identifiers, mapping index to node identifier.
    indices: Vec<N>,
    /// Hash map from node identifier to index.
    index_map: HashMap<N, usize>,
    d: Array2<S>,
}

impl<N, S> DistanceMatrix<N, S> for FullDistanceMatrix<N, S>
where
    N: Eq + Hash,
    S: NdFloat,
{
    fn get(&self, u: N, v: N) -> Option<S> {
        self.index(u, v).map(|(i, j)| self.d[[i, j]])
    }

    fn set(&mut self, u: N, v: N, d: S) -> Option<()> {
        self.index(u, v).map(|(i, j)| self.d[[i, j]] = d)
    }

    fn get_by_index(&self, i: usize, j: usize) -> S {
        self.d[[i, j]]
    }

    fn set_by_index(&mut self, i: usize, j: usize, d: S) {
        self.d[[i, j]] = d;
    }

    fn shape(&self) -> (usize, usize) {
        (self.indices.len(), self.indices.len())
    }

    fn row_index(&self, u: N) -> Option<usize> {
        self.index_map.get(&u).copied()
    }

    fn col_index(&self, u: N) -> Option<usize> {
        self.index_map.get(&u).copied()
    }

    fn row_indices(&'_ self) -> IndexIterator<'_, N> {
        IndexIterator {
            indices: &self.indices,
            index: 0,
        }
    }

    fn col_indices(&'_ self) -> IndexIterator<'_, N> {
        IndexIterator {
            indices: &self.indices,
            index: 0,
        }
    }
}

impl<N, S> FullDistanceMatrix<N, S>
where
    N: Eq + Hash,
    S: NdFloat,
{
    pub fn new<G>(graph: G) -> Self
    where
        G: IntoNodeIdentifiers,
        G::NodeId: Into<N>,
        N: Copy,
    {
        let indices = graph
            .node_identifiers()
            .map(|u| u.into())
            .collect::<Vec<_>>();
        let mut index_map = HashMap::new();
        for (i, &u) in indices.iter().enumerate() {
            index_map.insert(u, i);
        }
        let n = indices.len();
        Self {
            indices,
            index_map,
            d: Array::from_elem((n, n), S::infinity()),
        }
    }

    fn index(&self, u: N, v: N) -> Option<(usize, usize)> {
        self.index_map
            .get(&u)
            .zip(self.index_map.get(&v))
            .map(|(&i, &j)| (i, j))
    }
}
