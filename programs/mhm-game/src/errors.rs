use anchor_lang::prelude::*;

#[error_code]
pub enum MhmError {
    #[msg("The game is paused")]
    GamePaused,
    #[msg("Rarity weights must sum to 10000 basis points")]
    BadRarityWeights,
    #[msg("Mining rate range is invalid (min > max)")]
    BadMiningRange,
    #[msg("No genesis hatches remaining")]
    GenesisSoldOut,
    #[msg("Signer does not hold this monster's NFT")]
    NotMonsterHolder,
    #[msg("Monster is locked in a battle")]
    MonsterInBattle,
    #[msg("Monster is not part of this battle")]
    MonsterNotInBattle,
    #[msg("Nothing to claim")]
    NothingToClaim,
    #[msg("Monster is already at the maximum level")]
    MaxLevelReached,
    #[msg("Battle is not open for joining")]
    BattleNotOpen,
    #[msg("Battle is not active")]
    BattleNotActive,
    #[msg("Battle is not finished")]
    BattleNotFinished,
    #[msg("You are not a player in this battle")]
    NotABattlePlayer,
    #[msg("You cannot battle your own monster")]
    CannotBattleSelf,
    #[msg("Unknown action id")]
    UnknownAction,
    #[msg("That action does not fit in the consumable slot")]
    NotAConsumable,
    #[msg("That action does not fit in the support slot")]
    NotASupport,
    #[msg("Actions already submitted for this turn")]
    AlreadySubmitted,
    #[msg("The 90 second turn deadline has passed")]
    TurnDeadlinePassed,
    #[msg("The turn deadline has not passed yet")]
    DeadlineNotReached,
    #[msg("You did not submit an action this turn, so you cannot claim the timeout win")]
    CallerDidNotSubmit,
    #[msg("Only the battle creator can cancel")]
    NotBattleCreator,
    #[msg("Winner token account does not belong to the winning player")]
    BadWinnerAccount,
    #[msg("Missing winner token account")]
    MissingWinnerAccount,
    #[msg("Basis points value cannot exceed 10000")]
    BadBps,
    #[msg("Fee token account does not belong to the configured fee wallet")]
    BadFeeAccount,
    #[msg("Missing fee token account")]
    MissingFeeAccount,
    #[msg("Metadata base URI is too long (max 160 bytes)")]
    UriTooLong,
    #[msg("The reveal slot has not been produced yet; try again shortly")]
    RevealTooEarly,
    #[msg("The reveal slot's hash is no longer available; this hatch has expired")]
    RevealExpired,
    #[msg("Pending mint does not match the provided monster mint")]
    PendingMintMismatch,
    #[msg("Invalid SlotHashes sysvar account")]
    BadSlotHashes,
    #[msg("Match is not open for joining")]
    MatchNotOpen,
    #[msg("Match is not active")]
    MatchNotActive,
    #[msg("Match is not finished")]
    MatchNotFinished,
    #[msg("You are not a player in this match")]
    NotAMatchPlayer,
    #[msg("You cannot match against your own monster")]
    CannotMatchSelf,
    #[msg("Only the match creator can cancel")]
    NotMatchCreator,
    #[msg("Destination NFT account does not belong to the correct recipient")]
    BadMatchRecipient,
    #[msg("Listing price must be greater than zero")]
    BadPrice,
    #[msg("Only the seller can cancel this listing")]
    NotSeller,
    #[msg("Unauthorized: admin only")]
    Unauthorized,
    #[msg("Arithmetic overflow")]
    Overflow,
}
