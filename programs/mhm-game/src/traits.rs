//! Pure monster-generation logic.
//!
//! Given a 32-byte entropy seed and the tunable config (per-rarity mining
//! ranges + rarity weights), this rolls a monster's rarity, mining speed and
//! battle stats. It is a deterministic function of the seed, so it is fully
//! unit-testable and — crucially — entropy-source-agnostic: swapping the
//! clock-derived seed for a VRF / commit-reveal seed is a change at the call
//! site only, never here.

use crate::rng::Roll;
use crate::state::{Rarity, NUM_RARITIES};

/// Base battle stats per rarity tier (Standard..Unique); a random bonus is
/// rolled on top at mint time.
pub const BASE_MAX_HP: [u32; NUM_RARITIES] = [100, 130, 170, 220, 300];
pub const BASE_POWER: [u32; NUM_RARITIES] = [50, 65, 85, 110, 150];
pub const BASE_DEFENSE: [u32; NUM_RARITIES] = [20, 30, 45, 65, 90];
pub const HP_ROLL: u32 = 20;
pub const POWER_ROLL: u32 = 15;
pub const DEFENSE_ROLL: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RolledTraits {
    pub rarity: Rarity,
    pub rarity_index: usize,
    pub mining_rate: u64,
    pub max_hp: u32,
    pub power: u32,
    pub defense: u32,
}

/// Roll a monster's traits from an entropy seed.
///
/// `rarity_weights_bps` must sum to 10_000 (enforced at config time); if a
/// roll somehow exceeds the cumulative weight it falls through to the last
/// (rarest) tier, matching the on-chain guarantee that every roll yields a
/// valid rarity.
pub fn roll_traits(
    seed: [u8; 32],
    mining_rate_ranges: &[[u64; 2]; NUM_RARITIES],
    rarity_weights_bps: &[u16; NUM_RARITIES],
) -> RolledTraits {
    let mut roll = Roll::from_seed(seed);

    // Weighted rarity roll in [0, 10_000).
    let pick = roll.bps();
    let mut cumulative: u16 = 0;
    let mut rarity_index = NUM_RARITIES - 1;
    for (i, weight) in rarity_weights_bps.iter().enumerate() {
        cumulative = cumulative.saturating_add(*weight);
        if pick < cumulative {
            rarity_index = i;
            break;
        }
    }
    let rarity = Rarity::from_index(rarity_index);

    let range = mining_rate_ranges[rarity_index];
    let mining_rate = roll.range_u64(range[0], range[1]);

    RolledTraits {
        rarity,
        rarity_index,
        mining_rate,
        max_hp: BASE_MAX_HP[rarity_index] + roll.range_u32(0, HP_ROLL),
        power: BASE_POWER[rarity_index] + roll.range_u32(0, POWER_ROLL),
        defense: BASE_DEFENSE[rarity_index] + roll.range_u32(0, DEFENSE_ROLL),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::solana_program::keccak;

    const RANGES: [[u64; 2]; NUM_RARITIES] =
        [[100, 1000], [1000, 3000], [3000, 7000], [7000, 13000], [13000, 21000]];
    // 60 / 25 / 10 / 4 / 1 %.
    const WEIGHTS: [u16; NUM_RARITIES] = [6000, 2500, 1000, 400, 100];

    fn seed(i: u64) -> [u8; 32] {
        keccak::hashv(&[b"seed", &i.to_le_bytes()]).to_bytes()
    }

    #[test]
    fn is_deterministic_in_the_seed() {
        let s = seed(42);
        assert_eq!(roll_traits(s, &RANGES, &WEIGHTS), roll_traits(s, &RANGES, &WEIGHTS));
    }

    #[test]
    fn stats_and_mining_stay_within_tier_bounds() {
        for i in 0..5000u64 {
            let t = roll_traits(seed(i), &RANGES, &WEIGHTS);
            let r = t.rarity_index;
            assert!(t.mining_rate >= RANGES[r][0] && t.mining_rate <= RANGES[r][1]);
            assert!(t.max_hp >= BASE_MAX_HP[r] && t.max_hp <= BASE_MAX_HP[r] + HP_ROLL);
            assert!(t.power >= BASE_POWER[r] && t.power <= BASE_POWER[r] + POWER_ROLL);
            assert!(t.defense >= BASE_DEFENSE[r] && t.defense <= BASE_DEFENSE[r] + DEFENSE_ROLL);
            assert_eq!(t.rarity, Rarity::from_index(r));
        }
    }

    #[test]
    fn rarity_distribution_roughly_matches_weights() {
        let n = 100_000u64;
        let mut counts = [0u32; NUM_RARITIES];
        for i in 0..n {
            counts[roll_traits(seed(i), &RANGES, &WEIGHTS).rarity_index] += 1;
        }
        // Every tier should appear, and each should land within a loose band
        // of its configured weight (keccak stream is well-distributed).
        for (idx, &w) in WEIGHTS.iter().enumerate() {
            let expected = n as f64 * (w as f64 / 10_000.0);
            let actual = counts[idx] as f64;
            assert!(counts[idx] > 0, "tier {idx} never rolled");
            let tol = (expected * 0.20).max(60.0);
            assert!(
                (actual - expected).abs() <= tol,
                "tier {idx}: expected ~{expected:.0}, got {actual:.0}",
            );
        }
    }

    #[test]
    fn all_weight_on_one_tier_forces_that_tier() {
        let only_epic = [0u16, 0, 10_000, 0, 0];
        for i in 0..200u64 {
            assert_eq!(roll_traits(seed(i), &RANGES, &only_epic).rarity, Rarity::Epic);
        }
    }

    #[test]
    fn zero_width_mining_range_is_exact() {
        let fixed = [[500, 500]; NUM_RARITIES];
        assert_eq!(roll_traits(seed(7), &fixed, &WEIGHTS).mining_rate, 500);
    }
}
