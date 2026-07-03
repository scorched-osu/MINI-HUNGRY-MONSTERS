//! Turn resolution for battles (shared by pot-stake Battles and Match games).
//!
//! Order of operations inside a turn:
//!   1. DEF consumables apply their (support-boosted) defense buffs.
//!   2. HP+ consumables heal.
//!   3. DPS consumables deal damage. The faster side (or a UNIQUE with a
//!      first-strike trait) lands first; if that KOs the opponent, the
//!      opponent does not retaliate. Equal speed = simultaneous. Trait damage
//!      bonuses and defense-pierce apply here.
//!   4. Buff durations tick down.
//!
//! A support action only amplifies a consumable whose type it targets
//! (PWR+ -> DPS, GUARD+ -> DEF, MEND+ -> HP+); otherwise it is wasted.

use crate::actions::{get_action, ActionDef, ActionType};
use crate::state::{Combat, TurnOutcome, DRAW, MAX_BATTLE_TURNS, NONE_U8, TURN_DEADLINE_SECS};

/// Multiplier (in percent) a support action grants its paired consumable.
fn support_multiplier(consumable: &ActionDef, support_id: u8) -> u32 {
    if support_id == NONE_U8 {
        return 100;
    }
    match get_action(support_id) {
        Some(s) if s.boosts == Some(consumable.atype) => s.magnitude,
        _ => 100,
    }
}

/// DPS damage dealt: POWER * magnitude% * support% * (1 + trait bonus),
/// mitigated by the target's defense after trait defense-pierce. Minimum 1.
fn dps_damage(
    power: u32,
    action: &ActionDef,
    support_mult: u32,
    dmg_bonus_bps: u16,
    target_def: u32,
    pierce_bps: u16,
) -> u32 {
    let mut raw = (power as u64) * (action.magnitude as u64) * (support_mult as u64) / 10_000;
    raw = raw * (10_000 + dmg_bonus_bps as u64) / 10_000;
    let eff_def = (target_def as u64) * (10_000 - pierce_bps as u64) / 10_000;
    let dealt = raw * 100 / (100 + eff_def);
    u32::try_from(dealt).unwrap_or(u32::MAX).max(1)
}

/// Which side strikes first: an explicit first-strike trait wins, otherwise the
/// higher speed; equal speed returns `None` (simultaneous).
fn first_striker(c: &Combat) -> Option<usize> {
    match (c.always_first[0], c.always_first[1]) {
        (true, false) => return Some(0),
        (false, true) => return Some(1),
        _ => {}
    }
    if c.speed[0] > c.speed[1] {
        Some(0)
    } else if c.speed[1] > c.speed[0] {
        Some(1)
    } else {
        None
    }
}

/// Resolve one turn. Assumes both sides have submitted. Mutates the board and
/// reports whether the game continues, was won, or drawn.
pub fn resolve_turn(c: &mut Combat, now: i64) -> TurnOutcome {
    let picks: [(&ActionDef, u32); 2] = core::array::from_fn(|i| {
        // Submissions are validated on the way in, so this lookup cannot fail.
        let action = get_action(c.pending[i].consumable).expect("validated action id");
        let mult = support_multiplier(action, c.pending[i].support);
        (action, mult)
    });

    // 1. Defense buffs first, so they protect against this turn's attacks.
    for i in 0..2 {
        let (action, mult) = picks[i];
        if action.atype == ActionType::Def {
            let amount = (action.magnitude as u64 * mult as u64 / 100) as u32;
            c.def_buff[i] = c.def_buff[i].saturating_add(amount);
            c.def_buff_turns[i] = c.def_buff_turns[i].max(action.duration);
        }
    }

    // 2. Heals.
    for i in 0..2 {
        let (action, mult) = picks[i];
        if action.atype == ActionType::HpPlus {
            let heal =
                (c.max_hp[i] as u64 * action.magnitude as u64 * mult as u64 / 10_000) as u32;
            c.hp[i] = c.hp[i].saturating_add(heal).min(c.max_hp[i]);
        }
    }

    // 3. DPS. Compute each side's outgoing damage, then apply by strike order.
    let mut out = [0u32; 2]; // out[i] = damage dealt BY side i to side 1-i.
    for i in 0..2 {
        let (action, mult) = picks[i];
        if action.atype == ActionType::Dps {
            let target = 1 - i;
            let total_def = c.defense[target].saturating_add(c.def_buff[target]);
            out[i] = dps_damage(
                c.power[i],
                action,
                mult,
                c.dmg_bonus_bps[i],
                total_def,
                c.defense_pierce_bps[i],
            );
        }
    }
    match first_striker(c) {
        None => {
            // Simultaneous: independent targets, apply both from pre-damage HP.
            c.hp[1] = c.hp[1].saturating_sub(out[0]);
            c.hp[0] = c.hp[0].saturating_sub(out[1]);
        }
        Some(first) => {
            let second = 1 - first;
            c.hp[second] = c.hp[second].saturating_sub(out[first]);
            // A KO'd opponent cannot retaliate.
            if c.hp[second] > 0 {
                c.hp[first] = c.hp[first].saturating_sub(out[second]);
            }
        }
    }

    // 4. Buff durations tick down at end of turn.
    for i in 0..2 {
        if c.def_buff_turns[i] > 0 {
            c.def_buff_turns[i] -= 1;
            if c.def_buff_turns[i] == 0 {
                c.def_buff[i] = 0;
            }
        }
    }

    // Decide KO / turn-limit outcomes.
    let ko = [c.hp[0] == 0, c.hp[1] == 0];
    if ko[0] || ko[1] {
        return match (ko[0], ko[1]) {
            (true, true) => TurnOutcome::Draw,
            (true, false) => TurnOutcome::Win(1),
            (false, true) => TurnOutcome::Win(0),
            (false, false) => unreachable!(),
        };
    }

    if c.turn >= MAX_BATTLE_TURNS {
        // Decided by remaining HP percentage (scaled to avoid rounding ties).
        let pct0 = c.hp[0] as u64 * 1_000_000 / c.max_hp[0] as u64;
        let pct1 = c.hp[1] as u64 * 1_000_000 / c.max_hp[1] as u64;
        return if pct0 > pct1 {
            TurnOutcome::Win(0)
        } else if pct1 > pct0 {
            TurnOutcome::Win(1)
        } else {
            TurnOutcome::Draw
        };
    }

    // Next turn.
    c.turn += 1;
    c.deadline = now + TURN_DEADLINE_SECS;
    c.pending = Default::default();
    TurnOutcome::Continue
}

/// Convert a `TurnOutcome` to the `winner` byte stored on Battle/Match
/// (0/1 = side, DRAW = 2), or `None` if the game continues.
pub fn outcome_winner(outcome: TurnOutcome) -> Option<u8> {
    match outcome {
        TurnOutcome::Continue => None,
        TurnOutcome::Win(side) => Some(side),
        TurnOutcome::Draw => Some(DRAW),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat_stats::Fighter;
    use crate::state::PendingAction;

    // Action ids from the catalog.
    const NIBBLE: u8 = 0;
    const CHOMP: u8 = 1;
    const DEVOUR: u8 = 2;
    const HARDEN: u8 = 3;
    const SNACK: u8 = 5;
    const PWR_PLUS: u8 = 7;
    const GUARD_PLUS: u8 = 8;
    const MEND_PLUS: u8 = 9;

    // Two evenly-matched, equal-speed fighters with no traits: keeps the
    // classic-combat assertions (simultaneous damage) intact.
    fn even() -> Combat {
        let f = Fighter { max_hp: 100, power: 50, defense: 20, speed: 100, ..Default::default() };
        Combat::new(&f, &f, 0)
    }

    fn submit(c: &mut Combat, a: (u8, u8), b: (u8, u8)) -> TurnOutcome {
        c.pending[0] = PendingAction { submitted: true, consumable: a.0, support: a.1 };
        c.pending[1] = PendingAction { submitted: true, consumable: b.0, support: b.1 };
        resolve_turn(c, 1_000)
    }

    #[test]
    fn dps_deals_mitigated_damage_both_ways_when_equal_speed() {
        let mut c = even();
        let o = submit(&mut c, (CHOMP, NONE_U8), (NIBBLE, NONE_U8));
        assert_eq!(o, TurnOutcome::Continue);
        // Chomp: 50 power * 100% = 50 raw, vs 20 def -> 50*100/120 = 41.
        assert_eq!(c.hp[1], 100 - 41);
        // Nibble: 50 * 60% = 30 raw, vs 20 def -> 30*100/120 = 25.
        assert_eq!(c.hp[0], 100 - 25);
        assert_eq!(c.turn, 2);
    }

    #[test]
    fn pwr_plus_boosts_dps_only_when_types_match() {
        let mut boosted = even();
        submit(&mut boosted, (CHOMP, PWR_PLUS), (SNACK, NONE_U8));
        // 50 * 100% * 150% = 75 raw -> 75*100/120 = 62.
        assert_eq!(boosted.hp[1], 100 - 62);

        let mut mismatched = even();
        submit(&mut mismatched, (CHOMP, MEND_PLUS), (SNACK, NONE_U8));
        assert_eq!(mismatched.hp[1], 100 - 41);
    }

    #[test]
    fn def_buff_applies_before_damage_and_expires() {
        let mut c = even();
        submit(&mut c, (HARDEN, NONE_U8), (CHOMP, NONE_U8));
        // Harden +25 def this turn: 50 raw vs 45 def -> 50*100/145 = 34.
        assert_eq!(c.hp[0], 100 - 34);
        assert_eq!(c.def_buff[0], 25);
        submit(&mut c, (SNACK, NONE_U8), (SNACK, NONE_U8));
        assert_eq!(c.def_buff[0], 0);
    }

    #[test]
    fn guard_plus_boosts_def_consumable() {
        let mut c = even();
        submit(&mut c, (HARDEN, GUARD_PLUS), (CHOMP, NONE_U8));
        // 25 * 150% = 37 def buff. 50 raw vs 57 def -> 31.
        assert_eq!(c.def_buff[0], 37);
        assert_eq!(c.hp[0], 100 - 31);
    }

    #[test]
    fn heals_resolve_before_damage_and_cap_at_max_hp() {
        let mut c = even();
        c.hp = [50, 100];
        submit(&mut c, (SNACK, MEND_PLUS), (CHOMP, NONE_U8));
        // Boosted snack: 20% * 150% of 100 = 30 heal -> 80, then -41.
        assert_eq!(c.hp[0], 80 - 41);

        let mut capped = even();
        capped.hp = [95, 100];
        submit(&mut capped, (SNACK, NONE_U8), (SNACK, NONE_U8));
        assert_eq!(capped.hp[0], 100);
    }

    #[test]
    fn ko_wins_the_game() {
        let mut c = even();
        c.hp = [100, 10];
        let o = submit(&mut c, (DEVOUR, PWR_PLUS), (NIBBLE, NONE_U8));
        assert_eq!(o, TurnOutcome::Win(0));
    }

    #[test]
    fn mutual_ko_is_a_draw() {
        let mut c = even();
        c.hp = [5, 5];
        assert_eq!(submit(&mut c, (CHOMP, NONE_U8), (CHOMP, NONE_U8)), TurnOutcome::Draw);
    }

    #[test]
    fn turn_limit_decides_by_remaining_hp_percentage() {
        let mut c = even();
        c.turn = MAX_BATTLE_TURNS;
        c.hp = [50, 100];
        assert_eq!(submit(&mut c, (SNACK, NONE_U8), (SNACK, NONE_U8)), TurnOutcome::Win(1));
    }

    #[test]
    fn faster_side_strikes_first_and_can_avoid_retaliation() {
        // Side 0 is faster and one-shots side 1, which is at 1 HP: side 1's
        // (otherwise lethal) attack never lands.
        let fast = Fighter { max_hp: 100, power: 50, defense: 20, speed: 130, ..Default::default() };
        let slow = Fighter { max_hp: 1, power: 50, defense: 20, speed: 100, ..Default::default() };
        let mut c = Combat::new(&fast, &slow, 0);
        c.hp[0] = 10; // side 0 would die to side 1's ~41 damage if it landed.
        let o = submit(&mut c, (CHOMP, NONE_U8), (CHOMP, NONE_U8));
        assert_eq!(o, TurnOutcome::Win(0));
        assert_eq!(c.hp[0], 10); // untouched — side 1 was KO'd first.
    }

    #[test]
    fn unique_trait_bonus_increases_damage_vs_lower_rarity() {
        let plain = Fighter { max_hp: 100, power: 50, defense: 20, speed: 100, ..Default::default() };
        let uniq = Fighter {
            max_hp: 100, power: 50, defense: 20, speed: 100,
            dmg_bonus_bps: 3500, defense_pierce_bps: 3000, always_first: true,
        };
        let mut c = Combat::new(&uniq, &plain, 0);
        // Only side 0 attacks; measure its boosted hit vs the plain 41.
        let o = submit(&mut c, (CHOMP, NONE_U8), (SNACK, NONE_U8));
        assert_eq!(o, TurnOutcome::Continue);
        assert!(100 - c.hp[1] > 41, "trait bonus should exceed the plain 41 damage");
    }
}
