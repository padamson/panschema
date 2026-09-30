//! A seeded generator for the SGD layout's pair shuffle.
//!
//! SGD visits node pairs in a shuffled order each iteration. Upstream took a
//! `rand::Rng` for that, but it is the only randomness panschema-viz uses and
//! it is always seeded, so a crate for it — plus the `getrandom` backend that
//! crate needs on wasm32 — buys nothing. This file is panschema's own, not
//! vendored.

use super::algo::Shuffle;

/// SplitMix64 (Steele, Lea & Flood, 2014): a 64-bit generator that is
/// trivially seedable and gives the same sequence on every platform.
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// An index in `0..bound`, by Lemire's multiply-shift. Its bias is at most
    /// `bound / 2^64`, immaterial for ordering node pairs.
    fn below(&mut self, bound: usize) -> usize {
        ((u128::from(self.next_u64()) * bound as u128) >> 64) as usize
    }
}

impl Shuffle for SplitMix64 {
    /// Shuffles `items` in place (Fisher–Yates).
    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn matches_the_published_splitmix64_sequence() {
        let mut rng = SplitMix64::new(0);
        let first_three = [rng.next_u64(), rng.next_u64(), rng.next_u64()];
        assert_eq!(
            first_three,
            [
                0xE220_A839_7B1D_CDAF,
                0x6E78_9E6A_A1B9_65F4,
                0x06C4_5D18_8009_454F
            ]
        );
    }

    // SGD's layout is part of what `panschema publish` writes, so this order is
    // a published contract: change it and every SGD graph moves. The expected
    // value comes from a separate reference implementation of SplitMix64 and
    // this Fisher–Yates, not from running this one.
    #[test]
    fn shuffle_order_for_a_fixed_seed_is_stable() {
        let mut items = [0, 1, 2, 3, 4, 5, 6, 7];
        SplitMix64::new(42).shuffle(&mut items);
        assert_eq!(items, [4, 3, 2, 0, 7, 6, 1, 5]);
    }

    #[test]
    fn shuffle_keeps_every_element() {
        let mut items: Vec<u32> = (0..100).collect();
        SplitMix64::new(42).shuffle(&mut items);
        let mut sorted = items.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..100).collect::<Vec<_>>(), "shuffled: {items:?}");
    }

    #[test]
    fn shuffle_reorders() {
        let mut items: Vec<u32> = (0..100).collect();
        SplitMix64::new(42).shuffle(&mut items);
        assert_ne!(items, (0..100).collect::<Vec<_>>());
    }

    // An off-by-one that draws `j` from `0..i` instead of `0..=i` still yields
    // a permutation (Sattolo's algorithm) but never leaves an element in place,
    // so only cyclic orders appear and four of the six are unreachable.
    #[test]
    fn shuffle_reaches_every_ordering_of_three() {
        let seen: BTreeSet<[u8; 3]> = (0..200)
            .map(|seed| {
                let mut items = [0, 1, 2];
                SplitMix64::new(seed).shuffle(&mut items);
                items
            })
            .collect();
        assert_eq!(seen.len(), 6, "orderings reached: {seen:?}");
    }
}
