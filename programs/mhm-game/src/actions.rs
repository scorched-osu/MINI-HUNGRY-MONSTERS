//! The battle action catalog.
//!
//! Every action sits in one of two slots:
//!   * CONSUMABLE — does something concrete: deal damage (DPS), fortify (DEF)
//!     or heal (HP+).
//!   * SUPPORT — non-consumable boosters (SUPP type) that amplify the
//!     consumable played alongside them, but only when the support's boost
//!     target matches the consumable's type (PWR+ boosts DPS, GUARD+ boosts
//!     DEF, MEND+ boosts HP+). A mismatched support does nothing.

use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActionSlot {
    Consumable,
    Support,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActionType {
    /// Damage-dealing attack.
    Dps,
    /// Defensive: temporarily bolsters defense so the monster takes less damage.
    Def,
    /// Healing / Health Points Up.
    HpPlus,
    /// Support: amplifies the consumable action it is paired with.
    Supp,
}

pub struct ActionDef {
    pub id: u8,
    pub name: &'static str,
    pub slot: ActionSlot,
    pub atype: ActionType,
    /// Meaning depends on `atype`:
    ///   Dps    -> % of the attacker's POWER dealt as raw damage.
    ///   Def    -> flat defense added for `duration` turns.
    ///   HpPlus -> % of MAX HP restored.
    ///   Supp   -> % bonus applied to a matching consumable (150 = +50%).
    pub magnitude: u32,
    /// Turns a DEF buff lasts (unused for other types).
    pub duration: u8,
    /// For Supp actions: which consumable ActionType it boosts.
    pub boosts: Option<ActionType>,
}

pub const ACTIONS: &[ActionDef] = &[
    // ---- CONSUMABLE / DPS ----
    ActionDef { id: 0, name: "Nibble",       slot: ActionSlot::Consumable, atype: ActionType::Dps,    magnitude: 60,  duration: 0, boosts: None },
    ActionDef { id: 1, name: "Chomp",        slot: ActionSlot::Consumable, atype: ActionType::Dps,    magnitude: 100, duration: 0, boosts: None },
    ActionDef { id: 2, name: "Devour",       slot: ActionSlot::Consumable, atype: ActionType::Dps,    magnitude: 150, duration: 0, boosts: None },
    // ---- CONSUMABLE / DEF ----
    ActionDef { id: 3, name: "Harden Shell", slot: ActionSlot::Consumable, atype: ActionType::Def,    magnitude: 25,  duration: 2, boosts: None },
    ActionDef { id: 4, name: "Iron Belly",   slot: ActionSlot::Consumable, atype: ActionType::Def,    magnitude: 50,  duration: 1, boosts: None },
    // ---- CONSUMABLE / HP+ ----
    ActionDef { id: 5, name: "Snack",        slot: ActionSlot::Consumable, atype: ActionType::HpPlus, magnitude: 20,  duration: 0, boosts: None },
    ActionDef { id: 6, name: "Feast",        slot: ActionSlot::Consumable, atype: ActionType::HpPlus, magnitude: 40,  duration: 0, boosts: None },
    // ---- SUPPORT / SUPP ----
    ActionDef { id: 7, name: "PWR+",         slot: ActionSlot::Support,    atype: ActionType::Supp,   magnitude: 150, duration: 0, boosts: Some(ActionType::Dps) },
    ActionDef { id: 8, name: "GUARD+",       slot: ActionSlot::Support,    atype: ActionType::Supp,   magnitude: 150, duration: 0, boosts: Some(ActionType::Def) },
    ActionDef { id: 9, name: "MEND+",        slot: ActionSlot::Support,    atype: ActionType::Supp,   magnitude: 150, duration: 0, boosts: Some(ActionType::HpPlus) },
];

pub fn get_action(id: u8) -> Option<&'static ActionDef> {
    ACTIONS.get(id as usize).filter(|a| a.id == id)
}
