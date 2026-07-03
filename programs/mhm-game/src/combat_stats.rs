//! Effective combat stats: base (rolled at mint) scaled by level, plus the
//! rarity edges — attack speed and LEGENDARY+ special traits that bite harder
//! against lower-rarity opponents.
//!
//! Pure and unit-tested. `fighter()` is the single source of truth for how a
//! monster performs in a battle; both the quick pot-stake battle and the
//! NFT-staked Grudge Match build their combatants through it.

pub const MAX_LEVEL: u16 = 20;

// Per-level growth over level 1, in percent of the monster's base stat.
pub const POWER_PCT_PER_LVL: u32 = 6;
pub const HP_PCT_PER_LVL: u32 = 5;
pub const DEF_PCT_PER_LVL: u32 = 4;

// Attack speed: higher rarity is faster; a small per-level bump lets a
// well-leveled low tier catch a fresh high tier ("strong but beatable").
pub const BASE_SPEED: [u32; 5] = [100, 110, 122, 136, 152];
pub const SPEED_PER_LVL: u32 = 2;

/// A monster resolved for battle against a specific opponent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Fighter {
    pub max_hp: u32,
    pub power: u32,
    pub defense: u32,
    pub speed: u32,
    /// Extra DPS damage vs this opponent, in basis points (trait bonus).
    pub dmg_bonus_bps: u16,
    /// Fraction of the opponent's defense ignored on DPS, in basis points.
    pub defense_pierce_bps: u16,
    /// Always strikes first vs this opponent regardless of speed (UNIQUE).
    pub always_first: bool,
}

fn scale(base: u32, pct_per_lvl: u32, level: u16) -> u32 {
    let levels = level.saturating_sub(1) as u64;
    let factor = 100 + pct_per_lvl as u64 * levels;
    ((base as u64 * factor) / 100).min(u32::MAX as u64) as u32
}

/// Special-trait modifiers a monster of `rarity_idx` gets against an opponent
/// of `opp_rarity_idx`. Traits only exist at LEGENDARY (3) and UNIQUE (4), and
/// only bite against a *strictly lower* rarity — same-or-higher matchups get
/// nothing, so top-tier mirror matches stay fair.
fn traits_vs(rarity_idx: usize, opp_rarity_idx: usize) -> (u16, u16, bool) {
    if rarity_idx <= opp_rarity_idx {
        return (0, 0, false);
    }
    match rarity_idx {
        3 => (2000, 1500, false), // LEGENDARY "Apex": +20% dmg, 15% pierce
        4 => (3500, 3000, true),  // UNIQUE "Apex Dominion": +35% dmg, 30% pierce, first strike
        _ => (0, 0, false),
    }
}

/// Resolve a monster into a `Fighter` for a battle against `opp_rarity_idx`.
pub fn fighter(
    base_max_hp: u32,
    base_power: u32,
    base_defense: u32,
    rarity_idx: usize,
    level: u16,
    opp_rarity_idx: usize,
) -> Fighter {
    let level = level.clamp(1, MAX_LEVEL);
    let (dmg_bonus_bps, defense_pierce_bps, always_first) = traits_vs(rarity_idx, opp_rarity_idx);
    Fighter {
        max_hp: scale(base_max_hp, HP_PCT_PER_LVL, level),
        power: scale(base_power, POWER_PCT_PER_LVL, level),
        defense: scale(base_defense, DEF_PCT_PER_LVL, level),
        speed: BASE_SPEED[rarity_idx.min(4)] + SPEED_PER_LVL * (level.saturating_sub(1) as u32),
        dmg_bonus_bps,
        defense_pierce_bps,
        always_first,
    }
}

/// MHM (micro-MHM) cost to go from `level` to `level + 1`.
///
/// Scales with rarity and current level, so low tiers are cheap to level and
/// high tiers are costly — `base_cost * rarity_mult * level`. Returns 0 at the
/// cap. `base_cost` is a config value (micro-MHM).
pub fn level_up_cost(base_cost: u64, rarity_idx: usize, level: u16) -> u64 {
    if level >= MAX_LEVEL {
        return 0;
    }
    const RARITY_MULT: [u64; 5] = [1, 2, 4, 8, 16];
    base_cost
        .saturating_mul(RARITY_MULT[rarity_idx.min(4)])
        .saturating_mul(level as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_one_is_base_stats() {
        let f = fighter(100, 50, 20, 0, 1, 0);
        assert_eq!((f.max_hp, f.power, f.defense), (100, 50, 20));
        assert_eq!(f.speed, BASE_SPEED[0]);
    }

    #[test]
    fn leveling_raises_stats_and_speed() {
        let lo = fighter(100, 50, 20, 0, 1, 0);
        let hi = fighter(100, 50, 20, 0, MAX_LEVEL, 0);
        assert!(hi.power > lo.power && hi.max_hp > lo.max_hp && hi.speed > lo.speed);
        // 19 levels * 6% = +114% power.
        assert_eq!(hi.power, 50 * (100 + 6 * 19) / 100);
    }

    #[test]
    fn level_is_clamped_to_cap() {
        assert_eq!(fighter(100, 50, 20, 0, 999, 0), fighter(100, 50, 20, 0, MAX_LEVEL, 0));
        assert_eq!(fighter(100, 50, 20, 0, 0, 0), fighter(100, 50, 20, 0, 1, 0));
    }

    #[test]
    fn traits_only_bite_lower_rarity() {
        // Legendary vs standard: bonuses on.
        let leg = fighter(220, 110, 65, 3, 1, 0);
        assert_eq!((leg.dmg_bonus_bps, leg.defense_pierce_bps, leg.always_first), (2000, 1500, false));
        // Legendary vs legendary: no bonus.
        let mirror = fighter(220, 110, 65, 3, 1, 3);
        assert_eq!((mirror.dmg_bonus_bps, mirror.defense_pierce_bps, mirror.always_first), (0, 0, false));
        // Unique vs epic: strongest, first strike.
        let uni = fighter(300, 150, 90, 4, 1, 2);
        assert_eq!((uni.dmg_bonus_bps, uni.defense_pierce_bps, uni.always_first), (3500, 3000, true));
        // Standard vs unique: no traits at low tiers.
        let std = fighter(100, 50, 20, 0, 1, 4);
        assert_eq!((std.dmg_bonus_bps, std.defense_pierce_bps, std.always_first), (0, 0, false));
    }

    #[test]
    fn maxed_low_tier_approaches_fresh_high_tier() {
        // A fully-leveled STANDARD should rival a level-1 LEGENDARY on raw
        // power/speed (the legendary still wins on traits) — "beatable".
        let maxed_std = fighter(120, 65, 30, 0, MAX_LEVEL, 3);
        let fresh_leg = fighter(220, 110, 65, 3, 1, 0);
        assert!(maxed_std.power * 100 >= fresh_leg.power * 90);
        assert!(maxed_std.speed >= fresh_leg.speed);
    }

    #[test]
    fn level_up_cost_scales_and_caps() {
        // Cheaper for low rarity, pricier for high.
        assert!(level_up_cost(1_000_000, 4, 1) > level_up_cost(1_000_000, 0, 1));
        // Rises with level.
        assert!(level_up_cost(1_000_000, 0, 5) > level_up_cost(1_000_000, 0, 1));
        // Zero at the cap.
        assert_eq!(level_up_cost(1_000_000, 0, MAX_LEVEL), 0);
    }
}
