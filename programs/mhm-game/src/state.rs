use anchor_lang::prelude::*;

pub const CONFIG_SEED: &[u8] = b"config";
pub const MHM_MINT_SEED: &[u8] = b"mhm-mint";
pub const MONSTER_SEED: &[u8] = b"monster";
pub const BATTLE_SEED: &[u8] = b"battle";
pub const LISTING_SEED: &[u8] = b"listing";
pub const PENDING_SEED: &[u8] = b"pending";
pub const MATCH_SEED: &[u8] = b"match";

/// Game wins needed to take an NFT-staked Grudge Match (best of 5 → first to 3).
pub const MATCH_WINS_NEEDED: u8 = 3;
/// Hard cap on games in a match (best of 5 is 5, extra headroom for draws,
/// which replay without awarding a win).
pub const MAX_MATCH_GAMES: u8 = 9;

/// A hatch commits to a slot this many slots in the future; the roll is then
/// seeded from that slot's hash (unknowable at commit time). Small so the
/// reveal can happen almost immediately, but non-zero so the entropy slot
/// does not yet exist when payment is taken.
pub const REVEAL_DELAY_SLOTS: u64 = 2;

/// The reveal must land within this many slots of `target_slot`. Kept well
/// under the ~512-entry SlotHashes buffer so the seed slot cannot drift as
/// old entries age out (which would otherwise let a minter grind by timing
/// the reveal). ~256 slots ≈ 1.5–2 minutes — ample, since reveal normally
/// follows the commit within seconds.
pub const REVEAL_WINDOW_SLOTS: u64 = 256;

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

    /// Lowercase slug used to build the metadata URI.
    pub fn slug(&self) -> &'static str {
        match self {
            Rarity::Standard => "standard",
            Rarity::Rare => "rare",
            Rarity::Epic => "epic",
            Rarity::Legendary => "legendary",
            Rarity::Unique => "unique",
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
    /// Base MHM (micro-MHM) cost unit for leveling up a monster; the actual
    /// cost scales by rarity and current level (see combat_stats::level_up_cost).
    pub level_up_base_cost: u64,
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
    /// Base URI for NFT metadata; the rarity slug + ".json" is appended
    /// (e.g. "<base>/epic.json"). Tunable via update_config.
    #[max_len(160)]
    pub metadata_base_uri: String,
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
    /// Level (1..=MAX_LEVEL). Raises battle stats; leveled via `level_up`.
    pub level: u16,
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

/// A hatch that has been paid for and is awaiting its randomness reveal.
///
/// Commit–reveal defeats mint-roll grinding: payment is taken now, but the
/// monster's traits are seeded from the hash of `target_slot` — a slot that
/// does not yet exist at commit time, so the outcome cannot be predicted (or
/// aborted-and-retried within one atomic transaction). Once `target_slot` is
/// produced its hash is fixed, so the minter cannot re-roll a paid commit;
/// abandoning it just forfeits the payment.
#[account]
#[derive(InitSpace)]
pub struct PendingMint {
    pub minter: Pubkey,
    pub monster_mint: Pubkey,
    pub monster_id: u64,
    /// Slot whose hash seeds the roll (commit_slot + REVEAL_DELAY_SLOTS).
    pub target_slot: u64,
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

/// The mutable state of a single turn-based fight between two monsters. Shared
/// by the quick pot-stake `Battle` and each game of an NFT-staked `Match`, so
/// the combat engine (see `combat.rs`) has one implementation.
///
/// Per-side stats are the effective (level-scaled, trait-adjusted) values
/// computed by `combat_stats::fighter` when the two sides lock in.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, InitSpace)]
pub struct Combat {
    pub hp: [u32; 2],
    pub max_hp: [u32; 2],
    pub power: [u32; 2],
    pub defense: [u32; 2],
    /// Attack speed; the faster side lands its DPS first each turn.
    pub speed: [u32; 2],
    /// Trait bonus to DPS damage vs the opponent, in basis points.
    pub dmg_bonus_bps: [u16; 2],
    /// Fraction of the opponent's defense ignored on DPS, in basis points.
    pub defense_pierce_bps: [u16; 2],
    /// Strike-first override (UNIQUE trait vs lower rarity).
    pub always_first: [bool; 2],
    /// Temporary defense buff from DEF actions.
    pub def_buff: [u32; 2],
    /// Turns the defense buff has left.
    pub def_buff_turns: [u8; 2],
    pub pending: [PendingAction; 2],
    pub turn: u16,
    /// Unix time by which both players must have submitted this turn's actions.
    pub deadline: i64,
}

/// Outcome of resolving one turn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnOutcome {
    /// Fight continues; `turn`/`deadline`/`pending` have advanced.
    Continue,
    /// Side 0 or 1 won this game.
    Win(u8),
    /// Both KO'd simultaneously, or a tie on the turn cap.
    Draw,
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
    /// Fighter inputs snapshotted per side (rarity index, level, base stats),
    /// so effective stats + traits can be built once both sides are known.
    pub rarity: [u8; 2],
    pub level: [u16; 2],
    pub base_hp: [u32; 2],
    pub base_power: [u32; 2],
    pub base_defense: [u32; 2],
    pub board: Combat,
    /// 0 or 1 = winning side, DRAW (2) = draw, NONE_U8 = undecided.
    pub winner: u8,
    pub bump: u8,
}

impl Combat {
    /// Build a fresh board from two resolved fighters, starting turn 1 with the
    /// given deadline.
    pub fn new(
        a: &crate::combat_stats::Fighter,
        b: &crate::combat_stats::Fighter,
        deadline: i64,
    ) -> Combat {
        Combat {
            hp: [a.max_hp, b.max_hp],
            max_hp: [a.max_hp, b.max_hp],
            power: [a.power, b.power],
            defense: [a.defense, b.defense],
            speed: [a.speed, b.speed],
            dmg_bonus_bps: [a.dmg_bonus_bps, b.dmg_bonus_bps],
            defense_pierce_bps: [a.defense_pierce_bps, b.defense_pierce_bps],
            always_first: [a.always_first, b.always_first],
            def_buff: [0, 0],
            def_buff_turns: [0, 0],
            pending: Default::default(),
            turn: 1,
            deadline,
        }
    }

    /// Reset HP/buffs/pending for the next game of a match, keeping stats.
    pub fn reset_for_next_game(&mut self, deadline: i64) {
        self.hp = self.max_hp;
        self.def_buff = [0, 0];
        self.def_buff_turns = [0, 0];
        self.pending = Default::default();
        self.turn = 1;
        self.deadline = deadline;
    }
}

impl Battle {
    pub fn side_of(&self, player: &Pubkey) -> Option<usize> {
        self.players.iter().position(|p| p == player)
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug, InitSpace)]
pub enum MatchState {
    /// Created by a challenger (NFT escrowed), waiting for an opponent.
    Open,
    /// Both NFTs escrowed, games in progress.
    Active,
    /// A winner has been decided; NFTs not yet paid out.
    Finished,
    /// Cancelled before anyone joined; NFT returned.
    Cancelled,
    /// NFTs paid out. Terminal.
    Settled,
}

/// An NFT-staked, best-of-5 Grudge Match. HIGH STAKES: both monsters' NFTs are
/// escrowed on-chain; the winner (first to [MATCH_WINS_NEEDED] game wins) takes
/// BOTH NFTs — the loser's monster is gone. Reuses the [Combat] engine per game.
#[account]
#[derive(InitSpace)]
pub struct GrudgeMatch {
    pub id: u64,
    pub state: MatchState,
    pub players: [Pubkey; 2],
    /// The escrowed monster NFT mints.
    pub monsters: [Pubkey; 2],
    // Fighter inputs snapshotted per side.
    pub rarity: [u8; 2],
    pub level: [u16; 2],
    pub base_hp: [u32; 2],
    pub base_power: [u32; 2],
    pub base_defense: [u32; 2],
    /// Games won by each side this match.
    pub game_wins: [u8; 2],
    pub games_played: u8,
    /// Current game's combat board.
    pub board: Combat,
    /// 0 or 1 = winning side, DRAW (2) = drawn match, NONE_U8 = undecided.
    pub winner: u8,
    pub bump: u8,
}

impl GrudgeMatch {
    pub fn side_of(&self, player: &Pubkey) -> Option<usize> {
        self.players.iter().position(|p| p == player)
    }
}

/// Award a game win to `side` and report whether the match is now decided
/// (returns the winning side once it reaches [MATCH_WINS_NEEDED]).
pub fn record_game_win(game_wins: &mut [u8; 2], side: usize) -> Option<u8> {
    game_wins[side] = game_wins[side].saturating_add(1);
    if game_wins[side] >= MATCH_WINS_NEEDED {
        Some(side as u8)
    } else {
        None
    }
}

/// Decide a match forced to stop at the game cap: higher game-win count wins,
/// a tie is a draw (DRAW).
pub fn decide_on_cap(game_wins: &[u8; 2]) -> u8 {
    if game_wins[0] > game_wins[1] {
        0
    } else if game_wins[1] > game_wins[0] {
        1
    } else {
        DRAW
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_of_five_needs_three_game_wins() {
        let mut w = [0u8, 0];
        assert_eq!(record_game_win(&mut w, 0), None); // 1-0
        assert_eq!(record_game_win(&mut w, 1), None); // 1-1
        assert_eq!(record_game_win(&mut w, 0), None); // 2-1
        assert_eq!(record_game_win(&mut w, 1), None); // 2-2
        assert_eq!(record_game_win(&mut w, 0), Some(0)); // 3-2 -> side 0 wins
        assert_eq!(w, [3, 2]);
    }

    #[test]
    fn sweep_wins_at_three_zero() {
        let mut w = [0u8, 0];
        record_game_win(&mut w, 1);
        record_game_win(&mut w, 1);
        assert_eq!(record_game_win(&mut w, 1), Some(1));
    }

    #[test]
    fn cap_decides_by_game_wins_or_draw() {
        assert_eq!(decide_on_cap(&[2, 1]), 0);
        assert_eq!(decide_on_cap(&[1, 2]), 1);
        assert_eq!(decide_on_cap(&[2, 2]), DRAW);
    }
}
