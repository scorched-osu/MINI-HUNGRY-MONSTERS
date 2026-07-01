//! Turn resolution for battles.
//!
//! Order of operations inside a turn (both sides resolve together):
//!   1. DEF consumables apply their (support-boosted) defense buffs.
//!   2. HP+ consumables heal.
//!   3. DPS consumables deal damage simultaneously, using the defense values
//!      that include this turn's fresh DEF buffs.
//!   4. Buff durations tick down.
//!
//! A support action only amplifies a consumable whose type it targets
//! (PWR+ -> DPS, GUARD+ -> DEF, MEND+ -> HP+); otherwise it is wasted.

use crate::actions::{get_action, ActionDef, ActionType};
use crate::state::{Battle, BattleState, DRAW, MAX_BATTLE_TURNS, NONE_U8, TURN_DEADLINE_SECS};

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

/// Raw damage before mitigation: POWER * magnitude% * support%.
fn raw_damage(power: u32, action: &ActionDef, mult: u32) -> u64 {
    (power as u64) * (action.magnitude as u64) * (mult as u64) / 10_000
}

/// Mitigate raw damage against total defense: dmg * 100 / (100 + defense).
fn mitigate(raw: u64, defense: u32) -> u32 {
    let dealt = raw * 100 / (100 + defense as u64);
    u32::try_from(dealt).unwrap_or(u32::MAX).max(1)
}

/// Resolve one turn. Assumes both sides have submitted. Updates HP, buffs and
/// either advances the turn counter or finishes the battle.
pub fn resolve_turn(battle: &mut Battle, now: i64) {
    let picks: Vec<(&ActionDef, u32)> = (0..2)
        .map(|i| {
            let p = &battle.pending[i];
            // Submissions are validated on the way in, so this lookup cannot fail.
            let action = get_action(p.consumable).expect("validated action id");
            let mult = support_multiplier(action, p.support);
            (action, mult)
        })
        .collect();

    // 1. Defense buffs first, so they protect against this turn's attacks.
    for i in 0..2 {
        let (action, mult) = picks[i];
        if action.atype == ActionType::Def {
            let amount = (action.magnitude as u64 * mult as u64 / 100) as u32;
            battle.def_buff[i] = battle.def_buff[i].saturating_add(amount);
            battle.def_buff_turns[i] = battle.def_buff_turns[i].max(action.duration);
        }
    }

    // 2. Heals.
    for i in 0..2 {
        let (action, mult) = picks[i];
        if action.atype == ActionType::HpPlus {
            let heal = (battle.max_hp[i] as u64 * action.magnitude as u64 * mult as u64
                / 10_000) as u32;
            battle.hp[i] = battle.hp[i].saturating_add(heal).min(battle.max_hp[i]);
        }
    }

    // 3. Simultaneous damage.
    let mut damage = [0u32; 2];
    for i in 0..2 {
        let (action, mult) = picks[i];
        if action.atype == ActionType::Dps {
            let target = 1 - i;
            let raw = raw_damage(battle.power[i], action, mult);
            let total_def = battle.defense[target].saturating_add(battle.def_buff[target]);
            damage[target] = mitigate(raw, total_def);
        }
    }
    for i in 0..2 {
        battle.hp[i] = battle.hp[i].saturating_sub(damage[i]);
    }

    // 4. Buff durations tick down at end of turn.
    for i in 0..2 {
        if battle.def_buff_turns[i] > 0 {
            battle.def_buff_turns[i] -= 1;
            if battle.def_buff_turns[i] == 0 {
                battle.def_buff[i] = 0;
            }
        }
    }

    // Decide KO / turn-limit outcomes.
    let ko = [battle.hp[0] == 0, battle.hp[1] == 0];
    if ko[0] || ko[1] {
        battle.state = BattleState::Finished;
        battle.winner = match (ko[0], ko[1]) {
            (true, true) => DRAW,
            (true, false) => 1,
            (false, true) => 0,
            (false, false) => unreachable!(),
        };
        return;
    }

    if battle.turn >= MAX_BATTLE_TURNS {
        // Decided by remaining HP percentage (scaled to avoid rounding ties).
        let pct0 = battle.hp[0] as u64 * 1_000_000 / battle.max_hp[0] as u64;
        let pct1 = battle.hp[1] as u64 * 1_000_000 / battle.max_hp[1] as u64;
        battle.state = BattleState::Finished;
        battle.winner = if pct0 > pct1 {
            0
        } else if pct1 > pct0 {
            1
        } else {
            DRAW
        };
        return;
    }

    // Next turn.
    battle.turn += 1;
    battle.deadline = now + TURN_DEADLINE_SECS;
    battle.pending = Default::default();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::PendingAction;
    use anchor_lang::prelude::Pubkey;

    // Action ids from the catalog.
    const NIBBLE: u8 = 0;
    const CHOMP: u8 = 1;
    const DEVOUR: u8 = 2;
    const HARDEN: u8 = 3;
    const SNACK: u8 = 5;
    const PWR_PLUS: u8 = 7;
    const GUARD_PLUS: u8 = 8;
    const MEND_PLUS: u8 = 9;

    fn test_battle() -> Battle {
        Battle {
            id: 0,
            state: BattleState::Active,
            players: [Pubkey::new_unique(), Pubkey::new_unique()],
            monsters: [Pubkey::new_unique(), Pubkey::new_unique()],
            pots: [1_000_000, 2_000_000],
            hp: [100, 100],
            max_hp: [100, 100],
            power: [50, 50],
            defense: [20, 20],
            def_buff: [0, 0],
            def_buff_turns: [0, 0],
            pending: Default::default(),
            turn: 1,
            deadline: 0,
            winner: NONE_U8,
            bump: 0,
        }
    }

    fn submit(battle: &mut Battle, a: (u8, u8), b: (u8, u8)) {
        battle.pending[0] = PendingAction { submitted: true, consumable: a.0, support: a.1 };
        battle.pending[1] = PendingAction { submitted: true, consumable: b.0, support: b.1 };
        resolve_turn(battle, 1_000);
    }

    #[test]
    fn dps_deals_mitigated_damage_both_ways() {
        let mut b = test_battle();
        submit(&mut b, (CHOMP, NONE_U8), (NIBBLE, NONE_U8));
        // Chomp: 50 power * 100% = 50 raw, vs 20 def -> 50*100/120 = 41.
        assert_eq!(b.hp[1], 100 - 41);
        // Nibble: 50 * 60% = 30 raw, vs 20 def -> 30*100/120 = 25.
        assert_eq!(b.hp[0], 100 - 25);
        assert_eq!(b.turn, 2);
        assert_eq!(b.state, BattleState::Active);
    }

    #[test]
    fn pwr_plus_boosts_dps_only_when_types_match() {
        let mut boosted = test_battle();
        submit(&mut boosted, (CHOMP, PWR_PLUS), (SNACK, NONE_U8));
        // Boosted chomp: 50 * 100% * 150% = 75 raw -> 75*100/120 = 62.
        assert_eq!(boosted.hp[1], 100 - 62);

        // Mismatched support (MEND+ on a DPS attack) does nothing.
        let mut mismatched = test_battle();
        submit(&mut mismatched, (CHOMP, MEND_PLUS), (SNACK, NONE_U8));
        assert_eq!(mismatched.hp[1], 100 - 41);
    }

    #[test]
    fn def_buff_applies_before_damage_and_expires() {
        let mut b = test_battle();
        submit(&mut b, (HARDEN, NONE_U8), (CHOMP, NONE_U8));
        // Harden adds +25 def THIS turn: 50 raw vs 45 def -> 50*100/145 = 34.
        assert_eq!(b.hp[0], 100 - 34);
        // Harden lasts 2 turns: still active on turn 2.
        assert_eq!(b.def_buff[0], 25);
        submit(&mut b, (SNACK, NONE_U8), (SNACK, NONE_U8));
        // ...and expires after it.
        assert_eq!(b.def_buff[0], 0);
    }

    #[test]
    fn guard_plus_boosts_def_consumable() {
        let mut b = test_battle();
        submit(&mut b, (HARDEN, GUARD_PLUS), (CHOMP, NONE_U8));
        // Boosted harden: 25 * 150% = 37 def buff. 50 raw vs 57 def -> 31.
        assert_eq!(b.def_buff[0], 37);
        assert_eq!(b.hp[0], 100 - 31);
    }

    #[test]
    fn heals_resolve_before_damage_and_cap_at_max_hp() {
        let mut b = test_battle();
        b.hp = [50, 100];
        submit(&mut b, (SNACK, MEND_PLUS), (CHOMP, NONE_U8));
        // Boosted snack: 20% * 150% of 100 max = 30 heal -> 80, then -41 dmg.
        assert_eq!(b.hp[0], 80 - 41);

        let mut capped = test_battle();
        capped.hp = [95, 100];
        submit(&mut capped, (SNACK, NONE_U8), (SNACK, NONE_U8));
        assert_eq!(capped.hp[0], 100);
    }

    #[test]
    fn ko_finishes_battle_with_winner() {
        let mut b = test_battle();
        b.hp = [100, 10];
        submit(&mut b, (DEVOUR, PWR_PLUS), (NIBBLE, NONE_U8));
        assert_eq!(b.state, BattleState::Finished);
        assert_eq!(b.winner, 0);
    }

    #[test]
    fn mutual_ko_is_a_draw() {
        let mut b = test_battle();
        b.hp = [5, 5];
        submit(&mut b, (CHOMP, NONE_U8), (CHOMP, NONE_U8));
        assert_eq!(b.state, BattleState::Finished);
        assert_eq!(b.winner, DRAW);
    }

    #[test]
    fn turn_limit_decides_by_remaining_hp_percentage() {
        let mut b = test_battle();
        b.turn = MAX_BATTLE_TURNS;
        b.hp = [50, 100];
        // Side 0 heals to 70%, side 1 stays capped at 100%: side 1 wins.
        submit(&mut b, (SNACK, NONE_U8), (SNACK, NONE_U8));
        assert_eq!(b.state, BattleState::Finished);
        assert_eq!(b.winner, 1);
    }

    #[test]
    fn damage_always_at_least_one() {
        let mut b = test_battle();
        b.power = [1, 1];
        b.defense = [10_000, 10_000];
        submit(&mut b, (NIBBLE, NONE_U8), (NIBBLE, NONE_U8));
        assert_eq!(b.hp, [99, 99]);
    }
}
