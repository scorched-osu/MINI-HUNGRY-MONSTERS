use anchor_lang::prelude::*;

pub const CONFIG_SEED: &[u8] = b"config";
pub const MHM_MINT_SEED: &[u8] = b"mhm-mint";
pub const MONSTER_SEED: &[u8] = b"monster";
pub const BATTLE_SEED: &[u8] = b"battle";
pub const LISTING_SEED: &[u8] = b"listing";

/// MHM coin uses 6 decimals; 1 MHM = 1_000_000 micro-MHM.
pub const MHM_DECIMALS: u8 = 6;
/// How many seconds a player has to pick their actions each battle turn.
pub const TURN_DEADLINE_SECS: i64 = 90;
/// Battles that run this long are decided by remaining HP percentage.
pub const MAX_BATTLE_TURNS: u16 = 30;
/// Sentinel for "no support action selected" / "winner not decided".
pub const NONE_U8: u8 = u8::MAX;
/// Winner value meaning the battle ended in a draw.
pub const DRAW: u8 = 2;

pub const NUM_RARITIES: usize = 5;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug, InitSpace)]
pub enum Rarity {
    Standard,
    Rare,
    Epic,
    Legendary,
    Unique,
}

impl Rarity {
    pub fn index(&self) -> usize {
        match self {
            Rarity::Standard => 0,
            Rarity::Rare => 1,
            Rarity::Epic => 2,
            Rarity::Legendary => 3,
            Rarity::Unique => 4,
        }
    }

    pub fn from_index(i: usize) -> Rarity {
        match i {
            0 => Rarity::Standard,
            1 => Rarity::Rare,
            2 => Rarity::Epic,
            3 => Rarity::Legendary,
            _ => Rarity::Unique,
        }
    }
}

/// Global game configuration. Single PDA, seeded by [CONFIG_SEED].
///
/// All economy numbers (mining ranges, prices, rarity odds) live here so the
/// admin can tune them without redeploying the program — the exact mining
/// speeds are still under discussion.
#[account]
#[derive(InitSpace)]
pub struct GameConfig {
    pub admin: Pubkey,
    /// The MHM coin mint (PDA, authority = this config account).
    pub mhm_mint: Pubkey,
    /// Wallet that receives ALL fees: genesis SOL payments, the non-burned
    /// share of MHM monster purchases, and the battle rake.
    pub fee_wallet: Pubkey,
    /// Share of the MHM monster price that is burned (basis points).
    /// The remainder is transferred to `fee_wallet`'s MHM account.
    pub burn_bps: u16,
    /// Rake taken from battle loot (basis points), sent to `fee_wallet`.
    pub battle_fee_bps: u16,
    /// Fee on marketplace sales (basis points), sent to `fee_wallet`.
    pub market_fee_bps: u16,
    /// Price in micro-MHM to buy (hatch) a new monster with MHM.
    pub monster_price_mhm: u64,
    /// Lamport price for genesis hatches (bootstraps the economy before MHM circulates).
    pub genesis_price_lamports: u64,
    /// How many genesis (SOL-priced) hatches remain.
    pub genesis_remaining: u32,
    /// Mining speed ranges per rarity, in micro-MHM per hour: [min, max].
    pub mining_rate_ranges: [[u64; 2]; NUM_RARITIES],
    /// Rarity roll weights in basis points (must sum to 10_000).
    pub rarity_weights_bps: [u16; NUM_RARITIES],
    /// Total monsters ever minted (also used as the next monster id).
    pub monsters_minted: u64,
    /// Total battles ever created (also used as the next battle id).
    pub battles_created: u64,
    pub paused: bool,
    pub bump: u8,
}

/// On-chain state for one MINI-HUNGRY-MONSTER NFT.
///
/// The NFT itself is an SPL mint with supply 1 / decimals 0; whoever holds
/// that token owns the monster (so monsters trade on any SPL marketplace).
/// This PDA (seeded by the mint) carries the game data.
#[account]
#[derive(InitSpace)]
pub struct Monster {
    /// The SPL mint of this NFT.
    pub mint: Pubkey,
    pub id: u64,
    pub rarity: Rarity,
    /// Mining speed in micro-MHM per hour.
    pub mining_rate: u64,
    /// Timestamp mining last settled from.
    pub last_settled_ts: i64,
    /// Mined-but-unclaimed micro-MHM banked on this monster.
    /// THIS is the pot an opponent wins if they beat this monster in battle.
    pub unclaimed: u64,
    // Battle stats.
    pub max_hp: u32,
    pub power: u32,
    pub defense: u32,
    pub wins: u32,
    pub losses: u32,
    /// While in a battle the monster is locked: no claiming, no other battles.
    pub in_battle: bool,
    /// The battle this monster is locked into (default Pubkey when not in battle).
    pub battle: Pubkey,
    pub bump: u8,
}

impl Monster {
    /// micro-MHM mined and not yet settled into `unclaimed`.
    pub fn accrued(&self, now: i64) -> u64 {
        if self.in_battle {
            return 0;
        }
        let elapsed = now.saturating_sub(self.last_settled_ts).max(0) as u128;
        let earned = (self.mining_rate as u128).saturating_mul(elapsed) / 3600;
        u64::try_from(earned).unwrap_or(u64::MAX)
    }

    /// Move all accrued mining into `unclaimed` and reset the clock.
    pub fn settle(&mut self, now: i64) {
        let accrued = self.accrued(now);
        self.unclaimed = self.unclaimed.saturating_add(accrued);
        self.last_settled_ts = now;
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug, InitSpace)]
pub enum BattleState {
    /// Created by a challenger, waiting for an opponent.
    Open,
    /// Both monsters locked in, turns in progress.
    Active,
    /// A winner (or draw) has been decided; pots not yet paid out.
    Finished,
    /// Cancelled before anyone joined.
    Cancelled,
    /// Pots paid out and monsters unlocked. Terminal.
    Settled,
}

/// A player's selected actions for the current turn.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, InitSpace)]
pub struct PendingAction {
    pub submitted: bool,
    /// Action id from the catalog; must be a CONSUMABLE-slot action.
    pub consumable: u8,
    /// Action id from the catalog; must be a SUPPORT-slot action, or NONE_U8.
    pub support: u8,
}

/// One battle between two monsters. Index 0 = challenger, 1 = joiner.
#[account]
#[derive(InitSpace)]
pub struct Battle {
    pub id: u64,
    pub state: BattleState,
    /// Wallets in control of each side (snapshotted when each side locks in).
    pub players: [Pubkey; 2],
    /// Monster NFT mints on each side.
    pub monsters: [Pubkey; 2],
    /// micro-MHM at stake per side: the monster's whole unclaimed mining pot.
    pub pots: [u64; 2],
    pub hp: [u32; 2],
    pub max_hp: [u32; 2],
    pub power: [u32; 2],
    pub defense: [u32; 2],
    /// Temporary defense buff from DEF actions.
    pub def_buff: [u32; 2],
    /// Turns the defense buff has left.
    pub def_buff_turns: [u8; 2],
    pub pending: [PendingAction; 2],
    pub turn: u16,
    /// Unix time by which both players must have submitted this turn's actions.
    pub deadline: i64,
    /// 0 or 1 = winning side, DRAW (2) = draw, NONE_U8 = undecided.
    pub winner: u8,
    pub bump: u8,
}

impl Battle {
    pub fn side_of(&self, player: &Pubkey) -> Option<usize> {
        self.players.iter().position(|p| p == player)
    }
}

/// A marketplace listing: the monster NFT sits in a program escrow until the
/// listing is bought (price paid in MHM) or cancelled. PDA seeded by the
/// monster mint, so a monster can have at most one live listing.
///
/// The monster keeps mining while listed — its unclaimed pot travels with it
/// to the buyer, and is part of what's being priced.
#[account]
#[derive(InitSpace)]
pub struct Listing {
    pub seller: Pubkey,
    pub monster_mint: Pubkey,
    /// Asking price in micro-MHM.
    pub price: u64,
    pub created_ts: i64,
    pub bump: u8,
}
