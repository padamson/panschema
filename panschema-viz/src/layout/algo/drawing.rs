// Vendored from egraph-rs (https://github.com/likr/egraph-rs), MIT licensed.
// Copyright (c) the egraph-rs authors. See THIRD-PARTY.md for the revision
// and for what was and was not vendored.

use super::{DrawingIndex, Metric};

/// A generic trait representing a drawing or layout of items (nodes) in a specific metric space.
///
/// This trait provides access to item positions, by node or by raw index, and to the
/// difference between two of them. `DrawingEuclidean2d` is its only implementation here.
pub trait Drawing {
    /// The type used to index items (nodes) in the drawing. Must implement `DrawingIndex`.
    type Index: DrawingIndex;
    /// The type representing the position of an item in the metric space. Must implement `Metric`.
    type Item: Metric;

    /// Returns the total number of items (nodes) in the drawing.
    fn len(&self) -> usize;

    /// Returns an immutable reference to the position of the item identified by `u`.
    /// Returns `None` if the item `u` is not found.
    fn position(&self, u: Self::Index) -> Option<&Self::Item>;

    /// Returns a mutable reference to the position of the item identified by `u`.
    /// Returns `None` if the item `u` is not found.
    fn position_mut(&mut self, u: Self::Index) -> Option<&mut Self::Item>;

    /// Returns an immutable reference to the position (`Item`) at the given raw numerical index `i`.
    /// Panics if `i` is out of bounds.
    fn raw_entry(&self, i: usize) -> &Self::Item;

    /// Returns a mutable reference to the position (`Item`) at the given raw numerical index `i`.
    /// Panics if `i` is out of bounds.
    fn raw_entry_mut(&mut self, i: usize) -> &mut Self::Item;

    /// Calculates the difference vector (delta) between the items at raw numerical indices `i` and `j`.
    /// Panics if `i` or `j` are out of bounds.
    fn delta(&self, i: usize, j: usize) -> <Self::Item as Metric>::D;
}
