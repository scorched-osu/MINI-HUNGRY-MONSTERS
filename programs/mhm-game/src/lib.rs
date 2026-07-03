//! MINI-HUNGRY-MONSTERS
//!
//! An idle NFT mining + turn-based battle game on Solana.
//!
//! * Monsters are SPL NFTs (supply 1, decimals 0) with on-chain game state.
//!   They come in 5 rarity tiers — STANDARD, RARE, EPIC, LEGENDARY, UNIQUE —
//!   each tier rolling a mining speed from a config-tunable range.
//! * Holding a monster passively "mines" MHM (mini-hungry-monster coin), an
//!   SPL token minted by this program. Holders claim mined MHM at any time.
//! * MHM buys more monsters (the payment is burned — deflationary sink).
//! * Battles are HIGH RISK / high reward: each monster stakes its ENTIRE
//!   unclaimed mining pot. Turn-based combat, 90 seconds to pick actions
//!   each turn. Winner takes the loser's whole pot.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke, system_instruction};
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::metadata::{
    create_metadata_accounts_v3, mpl_token_metadata::types::DataV2, CreateMetadataAccountsV3,
    Metadata as TokenMetadata,
};
use anchor_spl::token::spl_token::instruction::AuthorityType;
use anchor_spl::token::{
    self, Burn, CloseAccount, Mint, MintTo, SetAuthority, Token, TokenAccount, Transfer,
};

pub mod actions;
pub mod combat;
pub mod combat_stats;
pub mod errors;
pub mod rng;
pub mod state;
pub mod traits;

use actions::{get_action, ActionSlot};
use errors::MhmError;
use state::*;

declare_id!("594wvdBsGrU6g6LswPmsx7tk1Cc7fgrpDAxvF8CEWq9K");

#[program]
pub mod mhm_game {
    use super::*;

    /// One-time setup: creates the global config and the MHM coin mint
    /// (authority = config PDA, so only this program can mint MHM).
    pub fn initialize(ctx: Context<Initialize>, params: InitializeParams) -> Result<()> {
        let weight_sum: u32 = params.rarity_weights_bps.iter().map(|w| *w as u32).sum();
        require!(weight_sum == 10_000, MhmError::BadRarityWeights);
        for range in params.mining_rate_ranges.iter() {
            require!(range[0] <= range[1], MhmError::BadMiningRange);
        }
        require!(params.burn_bps <= 10_000, MhmError::BadBps);
        require!(params.battle_fee_bps <= 10_000, MhmError::BadBps);
        require!(params.market_fee_bps <= 10_000, MhmError::BadBps);
        require!(params.metadata_base_uri.len() <= 160, MhmError::UriTooLong);

        let config = &mut ctx.accounts.config;
        config.admin = ctx.accounts.admin.key();
        config.mhm_mint = ctx.accounts.mhm_mint.key();
        config.fee_wallet = params.fee_wallet;
        config.burn_bps = params.burn_bps;
        config.battle_fee_bps = params.battle_fee_bps;
        config.market_fee_bps = params.market_fee_bps;
        config.level_up_base_cost = params.level_up_base_cost;
        config.metadata_base_uri = params.metadata_base_uri;
        config.monster_price_mhm = params.monster_price_mhm;
        config.genesis_price_lamports = params.genesis_price_lamports;
        config.genesis_remaining = params.genesis_remaining;
        config.mining_rate_ranges = params.mining_rate_ranges;
        config.rarity_weights_bps = params.rarity_weights_bps;
        config.monsters_minted = 0;
        config.battles_created = 0;
        config.paused = false;
        config.bump = ctx.bumps.config;
        Ok(())
    }

    /// Admin knob-turning: mining speeds, prices and odds are still being
    /// tuned, so they are updatable without redeploying.
    pub fn update_config(ctx: Context<UpdateConfig>, params: UpdateConfigParams) -> Result<()> {
        let config = &mut ctx.accounts.config;
        if let Some(price) = params.monster_price_mhm {
            config.monster_price_mhm = price;
        }
        if let Some(price) = params.genesis_price_lamports {
            config.genesis_price_lamports = price;
        }
        if let Some(remaining) = params.genesis_remaining {
            config.genesis_remaining = remaining;
        }
        if let Some(ranges) = params.mining_rate_ranges {
            for range in ranges.iter() {
                require!(range[0] <= range[1], MhmError::BadMiningRange);
            }
            config.mining_rate_ranges = ranges;
        }
        if let Some(weights) = params.rarity_weights_bps {
            let sum: u32 = weights.iter().map(|w| *w as u32).sum();
            require!(sum == 10_000, MhmError::BadRarityWeights);
            config.rarity_weights_bps = weights;
        }
        if let Some(fee_wallet) = params.fee_wallet {
            config.fee_wallet = fee_wallet;
        }
        if let Some(burn_bps) = params.burn_bps {
            require!(burn_bps <= 10_000, MhmError::BadBps);
            config.burn_bps = burn_bps;
        }
        if let Some(battle_fee_bps) = params.battle_fee_bps {
            require!(battle_fee_bps <= 10_000, MhmError::BadBps);
            config.battle_fee_bps = battle_fee_bps;
        }
        if let Some(market_fee_bps) = params.market_fee_bps {
            require!(market_fee_bps <= 10_000, MhmError::BadBps);
            config.market_fee_bps = market_fee_bps;
        }
        if let Some(level_up_base_cost) = params.level_up_base_cost {
            config.level_up_base_cost = level_up_base_cost;
        }
        if let Some(metadata_base_uri) = params.metadata_base_uri {
            require!(metadata_base_uri.len() <= 160, MhmError::UriTooLong);
            config.metadata_base_uri = metadata_base_uri;
        }
        if let Some(paused) = params.paused {
            config.paused = paused;
        }
        Ok(())
    }

    /// Admin-only: mint an MHM allocation to a designated holder wallet.
    /// Used to seed the initial MHM holder wallets (team / treasury /
    /// liquidity allocations) before mining supply ramps up.
    pub fn admin_mint_mhm(ctx: Context<AdminMintMhm>, amount: u64) -> Result<()> {
        let bump = ctx.accounts.config.bump;
        let signer_seeds: &[&[&[u8]]] = &[&[CONFIG_SEED, &[bump]]];
        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.mhm_mint.to_account_info(),
                    to: ctx.accounts.recipient_mhm_ata.to_account_info(),
                    authority: ctx.accounts.config.to_account_info(),
                },
                signer_seeds,
            ),
            amount,
        )?;
        Ok(())
    }

    /// Genesis hatch, step 1 of 2: pay SOL and commit. This bootstraps the
    /// economy before any MHM circulates; supply is capped by
    /// `genesis_remaining`. The monster's traits are NOT decided here — they
    /// are sealed to a future slot's hash and revealed by `reveal_monster`,
    /// which prevents rarity grinding.
    pub fn commit_hatch_genesis(ctx: Context<CommitHatchGenesis>, monster_id: u64) -> Result<()> {
        let config = &ctx.accounts.config;
        require!(!config.paused, MhmError::GamePaused);
        require!(config.genesis_remaining > 0, MhmError::GenesisSoldOut);
        require!(monster_id == config.monsters_minted, MhmError::Overflow);

        // Pay the genesis price in lamports to the fee wallet.
        if config.genesis_price_lamports > 0 {
            invoke(
                &system_instruction::transfer(
                    &ctx.accounts.payer.key(),
                    &ctx.accounts.fee_wallet.key(),
                    config.genesis_price_lamports,
                ),
                &[
                    ctx.accounts.payer.to_account_info(),
                    ctx.accounts.fee_wallet.to_account_info(),
                ],
            )?;
        }

        let config = &mut ctx.accounts.config;
        config.genesis_remaining -= 1;
        open_pending_mint(
            config,
            &mut ctx.accounts.pending,
            ctx.bumps.pending,
            &ctx.accounts.monster_mint,
            &ctx.accounts.payer,
        )
    }

    /// Buy a monster with MHM, step 1 of 2: pay and commit. `burn_bps` of the
    /// price is burned (deflationary sink); the rest goes to the fee wallet.
    /// Traits are revealed later by `reveal_monster`.
    pub fn commit_buy_monster(ctx: Context<CommitBuyMonster>, monster_id: u64) -> Result<()> {
        let config = &ctx.accounts.config;
        require!(!config.paused, MhmError::GamePaused);
        require!(monster_id == config.monsters_minted, MhmError::Overflow);

        let price = config.monster_price_mhm;
        let burn_amount = (price as u128 * config.burn_bps as u128 / 10_000) as u64;
        let fee_amount = price - burn_amount;

        if burn_amount > 0 {
            token::burn(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Burn {
                        mint: ctx.accounts.mhm_mint.to_account_info(),
                        from: ctx.accounts.payer_mhm_ata.to_account_info(),
                        authority: ctx.accounts.payer.to_account_info(),
                    },
                ),
                burn_amount,
            )?;
        }
        if fee_amount > 0 {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.payer_mhm_ata.to_account_info(),
                        to: ctx.accounts.fee_mhm_ata.to_account_info(),
                        authority: ctx.accounts.payer.to_account_info(),
                    },
                ),
                fee_amount,
            )?;
        }

        open_pending_mint(
            &mut ctx.accounts.config,
            &mut ctx.accounts.pending,
            ctx.bumps.pending,
            &ctx.accounts.monster_mint,
            &ctx.accounts.payer,
        )
    }

    /// Hatch step 2 of 2: reveal a committed monster's traits and mint the NFT.
    ///
    /// Seeds the roll from the hash of the committed `target_slot` (produced
    /// after the commit, so unpredictable at payment time), then rolls rarity
    /// and stats, mints the single NFT to the minter, attaches metadata, and
    /// revokes the mint authority. Only the original minter may reveal (they
    /// receive the NFT). The reveal must land within REVEAL_WINDOW_SLOTS of the
    /// target; miss it and the hatch expires and the payment is forfeit.
    pub fn reveal_monster(ctx: Context<RevealMonster>) -> Result<()> {
        let clock = Clock::get()?;
        let pending = &ctx.accounts.pending;
        require!(
            pending.monster_mint == ctx.accounts.monster_mint.key(),
            MhmError::PendingMintMismatch
        );

        // Seed from the first produced slot at-or-after the target (tolerating
        // skipped slots). The bounded window below keeps this chosen slot from
        // drifting as the SlotHashes buffer ages, so the seed stays fixed once
        // the slot exists — no timing-based grinding.
        let data = ctx.accounts.slot_hashes.try_borrow_data()?;
        let slot_hash = match rng::first_hash_at_or_after(&data, pending.target_slot) {
            Some((_slot, h)) => {
                // A qualifying slot exists; enforce the reveal window so it can
                // still be in the buffer (and thus stable).
                require!(
                    clock.slot <= pending.target_slot + REVEAL_WINDOW_SLOTS,
                    MhmError::RevealExpired
                );
                h
            }
            None => {
                // No produced slot at-or-after the target. Either the target
                // slot has not been produced yet (too early), or everything at
                // or after it aged out unrevealed (expired).
                require!(
                    clock.slot > pending.target_slot,
                    MhmError::RevealTooEarly
                );
                return err!(MhmError::RevealExpired);
            }
        };
        drop(data);

        let seed = rng::reveal_seed(&slot_hash, &pending.minter, pending.monster_id);
        let traits = traits::roll_traits(
            seed,
            &ctx.accounts.config.mining_rate_ranges,
            &ctx.accounts.config.rarity_weights_bps,
        );

        finalize_monster(
            &ctx.accounts.config,
            &mut ctx.accounts.monster,
            ctx.bumps.monster,
            pending.monster_id,
            pending.minter,
            &traits,
            clock.unix_timestamp,
            &ctx.accounts.monster_mint,
            &ctx.accounts.monster_token,
            &ctx.accounts.metadata,
            &ctx.accounts.payer,
            &ctx.accounts.token_program,
            &ctx.accounts.token_metadata_program,
            &ctx.accounts.system_program,
            &ctx.accounts.rent,
        )
    }

    /// Claim everything this monster has mined so far. Mints MHM to the
    /// current NFT holder.
    pub fn claim_mining(ctx: Context<ClaimMining>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let monster = &mut ctx.accounts.monster;
        require!(!monster.in_battle, MhmError::MonsterInBattle);

        monster.settle(now);
        let amount = monster.unclaimed;
        require!(amount > 0, MhmError::NothingToClaim);
        monster.unclaimed = 0;

        let bump = ctx.accounts.config.bump;
        let signer_seeds: &[&[&[u8]]] = &[&[CONFIG_SEED, &[bump]]];
        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.mhm_mint.to_account_info(),
                    to: ctx.accounts.holder_mhm_ata.to_account_info(),
                    authority: ctx.accounts.config.to_account_info(),
                },
                signer_seeds,
            ),
            amount,
        )?;

        emit!(MiningClaimed {
            monster: monster.mint,
            holder: ctx.accounts.holder.key(),
            amount,
        });
        Ok(())
    }

    /// Level up a monster (+1 level, up to MAX_LEVEL), raising its battle
    /// stats. The MHM cost scales with rarity and current level (cheap for low
    /// tiers, steep for high tiers) and is paid to the fee wallet.
    pub fn level_up(ctx: Context<LevelUp>) -> Result<()> {
        let config = &ctx.accounts.config;
        let monster = &mut ctx.accounts.monster;
        require!(!monster.in_battle, MhmError::MonsterInBattle);
        require!(
            monster.level < combat_stats::MAX_LEVEL,
            MhmError::MaxLevelReached
        );

        let cost = combat_stats::level_up_cost(
            config.level_up_base_cost,
            monster.rarity.index(),
            monster.level,
        );
        if cost > 0 {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.holder_mhm_ata.to_account_info(),
                        to: ctx.accounts.fee_mhm_ata.to_account_info(),
                        authority: ctx.accounts.holder.to_account_info(),
                    },
                ),
                cost,
            )?;
        }

        monster.level += 1;
        emit!(MonsterLeveledUp {
            monster: monster.mint,
            holder: ctx.accounts.holder.key(),
            level: monster.level,
            cost,
        });
        Ok(())
    }

    /// Open a battle challenge. HIGH RISK: the monster's entire unclaimed
    /// mining pot goes on the line the moment the challenge is created.
    pub fn create_battle(ctx: Context<CreateBattle>, battle_id: u64) -> Result<()> {
        let config = &mut ctx.accounts.config;
        require!(!config.paused, MhmError::GamePaused);
        require!(battle_id == config.battles_created, MhmError::Overflow);
        config.battles_created += 1;

        let now = Clock::get()?.unix_timestamp;
        let monster = &mut ctx.accounts.monster;
        require!(!monster.in_battle, MhmError::MonsterInBattle);

        // Lock the monster and move its whole mining pot into the battle.
        monster.settle(now);
        let pot = monster.unclaimed;
        monster.unclaimed = 0;
        monster.in_battle = true;
        monster.battle = ctx.accounts.battle.key();

        let battle = &mut ctx.accounts.battle;
        battle.id = battle_id;
        battle.state = BattleState::Open;
        battle.players[0] = ctx.accounts.creator.key();
        battle.monsters[0] = monster.mint;
        battle.pots[0] = pot;
        // Snapshot fighter inputs; effective stats are built at join, once both
        // rarities are known (traits depend on the opponent's rarity).
        battle.rarity[0] = monster.rarity.index() as u8;
        battle.level[0] = monster.level;
        battle.base_hp[0] = monster.max_hp;
        battle.base_power[0] = monster.power;
        battle.base_defense[0] = monster.defense;
        battle.winner = NONE_U8;
        battle.bump = ctx.bumps.battle;

        emit!(BattleCreated {
            battle: battle.key(),
            creator: battle.players[0],
            monster: monster.mint,
            pot,
        });
        Ok(())
    }

    /// Accept an open challenge with your own monster. Turn 1 starts now;
    /// both players have 90 seconds per turn to submit actions.
    pub fn join_battle(ctx: Context<JoinBattle>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let battle = &mut ctx.accounts.battle;
        require!(battle.state == BattleState::Open, MhmError::BattleNotOpen);

        let monster = &mut ctx.accounts.monster;
        require!(!monster.in_battle, MhmError::MonsterInBattle);
        require!(monster.mint != battle.monsters[0], MhmError::CannotBattleSelf);
        // Same wallet on both sides would make turn submission ambiguous
        // (players are identified by wallet).
        require!(
            ctx.accounts.joiner.key() != battle.players[0],
            MhmError::CannotBattleSelf
        );

        monster.settle(now);
        let pot = monster.unclaimed;
        monster.unclaimed = 0;
        monster.in_battle = true;
        monster.battle = battle.key();

        battle.state = BattleState::Active;
        battle.players[1] = ctx.accounts.joiner.key();
        battle.monsters[1] = monster.mint;
        battle.pots[1] = pot;
        battle.rarity[1] = monster.rarity.index() as u8;
        battle.level[1] = monster.level;
        battle.base_hp[1] = monster.max_hp;
        battle.base_power[1] = monster.power;
        battle.base_defense[1] = monster.defense;

        // Build both fighters (each vs the other's rarity) and open the board.
        let f0 = combat_stats::fighter(
            battle.base_hp[0], battle.base_power[0], battle.base_defense[0],
            battle.rarity[0] as usize, battle.level[0], battle.rarity[1] as usize,
        );
        let f1 = combat_stats::fighter(
            battle.base_hp[1], battle.base_power[1], battle.base_defense[1],
            battle.rarity[1] as usize, battle.level[1], battle.rarity[0] as usize,
        );
        battle.board = Combat::new(&f0, &f1, now + TURN_DEADLINE_SECS);

        emit!(BattleJoined {
            battle: battle.key(),
            joiner: battle.players[1],
            monster: monster.mint,
            pot,
        });
        Ok(())
    }

    /// Submit this turn's actions: one CONSUMABLE (DPS / DEF / HP+) and
    /// optionally one SUPPORT (SUPP) to boost it. Pass `support = 255` for
    /// no support. When the second player submits, the turn resolves
    /// immediately in the same transaction.
    pub fn submit_action(ctx: Context<SubmitAction>, consumable: u8, support: u8) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let battle = &mut ctx.accounts.battle;
        require!(battle.state == BattleState::Active, MhmError::BattleNotActive);
        require!(now <= battle.board.deadline, MhmError::TurnDeadlinePassed);

        let side = battle
            .side_of(&ctx.accounts.player.key())
            .ok_or(MhmError::NotABattlePlayer)?;
        require!(!battle.board.pending[side].submitted, MhmError::AlreadySubmitted);

        let consumable_def = get_action(consumable).ok_or(MhmError::UnknownAction)?;
        require!(
            consumable_def.slot == ActionSlot::Consumable,
            MhmError::NotAConsumable
        );
        if support != NONE_U8 {
            let support_def = get_action(support).ok_or(MhmError::UnknownAction)?;
            require!(support_def.slot == ActionSlot::Support, MhmError::NotASupport);
        }

        battle.board.pending[side] = PendingAction { submitted: true, consumable, support };

        if battle.board.pending[0].submitted && battle.board.pending[1].submitted {
            let turn = battle.board.turn;
            let outcome = combat::resolve_turn(&mut battle.board, now);
            emit!(TurnResolved {
                battle: battle.key(),
                turn,
                hp: battle.board.hp,
            });
            if let Some(winner) = combat::outcome_winner(outcome) {
                battle.state = BattleState::Finished;
                battle.winner = winner;
                emit!(BattleFinished { battle: battle.key(), winner });
            }
        }
        Ok(())
    }

    /// If your opponent let the 90 second clock run out without submitting
    /// (and you did submit), claim the win. If BOTH sides went silent,
    /// either player can call this to end the battle as a draw.
    pub fn claim_timeout(ctx: Context<ClaimTimeout>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let battle = &mut ctx.accounts.battle;
        require!(battle.state == BattleState::Active, MhmError::BattleNotActive);
        require!(now > battle.board.deadline, MhmError::DeadlineNotReached);

        let side = battle
            .side_of(&ctx.accounts.player.key())
            .ok_or(MhmError::NotABattlePlayer)?;
        let opponent = 1 - side;

        if battle.board.pending[side].submitted && !battle.board.pending[opponent].submitted {
            battle.winner = side as u8;
        } else if !battle.board.pending[side].submitted && !battle.board.pending[opponent].submitted {
            battle.winner = DRAW;
        } else {
            return err!(MhmError::CallerDidNotSubmit);
        }
        battle.state = BattleState::Finished;
        emit!(BattleFinished { battle: battle.key(), winner: battle.winner });
        Ok(())
    }

    /// Cancel an open challenge nobody has accepted. Returns the pot to the
    /// monster and unlocks it.
    pub fn cancel_battle(ctx: Context<CancelBattle>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let battle = &mut ctx.accounts.battle;
        require!(battle.state == BattleState::Open, MhmError::BattleNotOpen);
        require!(
            battle.players[0] == ctx.accounts.creator.key(),
            MhmError::NotBattleCreator
        );

        let monster = &mut ctx.accounts.monster;
        monster.unclaimed = monster.unclaimed.saturating_add(battle.pots[0]);
        monster.in_battle = false;
        monster.battle = Pubkey::default();
        monster.last_settled_ts = now;

        battle.pots[0] = 0;
        battle.state = BattleState::Cancelled;
        Ok(())
    }

    /// Pay out a finished battle and unlock both monsters. Permissionless
    /// crank — anyone can call it once a winner is decided.
    ///
    /// WINNER TAKES ALL: the loser's entire staked mining pot is minted as
    /// MHM straight to the winning player's wallet, and the winner's own pot
    /// returns to their monster's unclaimed balance. On a draw both pots
    /// simply return home.
    pub fn settle_battle(ctx: Context<SettleBattle>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let battle = &mut ctx.accounts.battle;
        require!(battle.state == BattleState::Finished, MhmError::BattleNotFinished);

        let monster_a = &mut ctx.accounts.monster_a;
        let monster_b = &mut ctx.accounts.monster_b;

        match battle.winner {
            DRAW => {
                monster_a.unclaimed = monster_a.unclaimed.saturating_add(battle.pots[0]);
                monster_b.unclaimed = monster_b.unclaimed.saturating_add(battle.pots[1]);
            }
            w @ (0 | 1) => {
                let winner_side = w as usize;
                let loser_side = 1 - winner_side;
                let winner_ata = ctx
                    .accounts
                    .winner_mhm_ata
                    .as_ref()
                    .ok_or(MhmError::MissingWinnerAccount)?;
                require!(
                    winner_ata.owner == battle.players[winner_side]
                        && winner_ata.mint == ctx.accounts.config.mhm_mint,
                    MhmError::BadWinnerAccount
                );

                // Loot: the loser's whole pot, minus the battle rake, minted
                // straight to the winning player's wallet.
                let pot = battle.pots[loser_side];
                let rake = (pot as u128 * ctx.accounts.config.battle_fee_bps as u128
                    / 10_000) as u64;
                let loot = pot - rake;
                let bump = ctx.accounts.config.bump;
                let signer_seeds: &[&[&[u8]]] = &[&[CONFIG_SEED, &[bump]]];
                if loot > 0 {
                    token::mint_to(
                        CpiContext::new_with_signer(
                            ctx.accounts.token_program.to_account_info(),
                            MintTo {
                                mint: ctx.accounts.mhm_mint.to_account_info(),
                                to: winner_ata.to_account_info(),
                                authority: ctx.accounts.config.to_account_info(),
                            },
                            signer_seeds,
                        ),
                        loot,
                    )?;
                }
                if rake > 0 {
                    let fee_ata = ctx
                        .accounts
                        .fee_mhm_ata
                        .as_ref()
                        .ok_or(MhmError::MissingFeeAccount)?;
                    require!(
                        fee_ata.owner == ctx.accounts.config.fee_wallet
                            && fee_ata.mint == ctx.accounts.config.mhm_mint,
                        MhmError::BadFeeAccount
                    );
                    token::mint_to(
                        CpiContext::new_with_signer(
                            ctx.accounts.token_program.to_account_info(),
                            MintTo {
                                mint: ctx.accounts.mhm_mint.to_account_info(),
                                to: fee_ata.to_account_info(),
                                authority: ctx.accounts.config.to_account_info(),
                            },
                            signer_seeds,
                        ),
                        rake,
                    )?;
                }

                // Winner's own stake goes back onto their monster.
                let (winner_monster, loser_monster) = if winner_side == 0 {
                    (&mut *monster_a, &mut *monster_b)
                } else {
                    (&mut *monster_b, &mut *monster_a)
                };
                winner_monster.unclaimed =
                    winner_monster.unclaimed.saturating_add(battle.pots[winner_side]);
                winner_monster.wins += 1;
                loser_monster.losses += 1;

                emit!(BattleSettled {
                    battle: battle.key(),
                    winner: battle.players[winner_side],
                    loot,
                });
            }
            _ => return err!(MhmError::BattleNotFinished),
        }

        for monster in [&mut *monster_a, &mut *monster_b] {
            monster.in_battle = false;
            monster.battle = Pubkey::default();
            monster.last_settled_ts = now;
        }
        battle.pots = [0, 0];
        battle.state = BattleState::Settled;
        Ok(())
    }

    // ---- NFT-staked Grudge Matches (best of 5, winner takes both NFTs) ----

    /// Open a Grudge Match: escrow your monster's NFT and stake it on a
    /// best-of-5. HIGHEST RISK — lose and your monster is gone for good.
    pub fn create_match(ctx: Context<CreateMatch>, match_id: u64) -> Result<()> {
        let config = &mut ctx.accounts.config;
        require!(!config.paused, MhmError::GamePaused);
        require!(match_id == config.battles_created, MhmError::Overflow);
        config.battles_created += 1;

        // Escrow the challenger's NFT.
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.creator_nft_token.to_account_info(),
                    to: ctx.accounts.escrow_nft_token.to_account_info(),
                    authority: ctx.accounts.creator.to_account_info(),
                },
            ),
            1,
        )?;

        let m = &mut ctx.accounts.grudge_match;
        let monster = &ctx.accounts.monster;
        m.id = match_id;
        m.state = MatchState::Open;
        m.players[0] = ctx.accounts.creator.key();
        m.monsters[0] = monster.mint;
        m.rarity[0] = monster.rarity.index() as u8;
        m.level[0] = monster.level;
        m.base_hp[0] = monster.max_hp;
        m.base_power[0] = monster.power;
        m.base_defense[0] = monster.defense;
        m.game_wins = [0, 0];
        m.games_played = 0;
        m.winner = NONE_U8;
        m.bump = ctx.bumps.grudge_match;

        emit!(MatchCreated {
            grudge_match: m.key(),
            creator: m.players[0],
            monster: monster.mint,
        });
        Ok(())
    }

    /// Accept a Grudge Match: escrow your NFT and start game 1.
    pub fn join_match(ctx: Context<JoinMatch>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let m = &mut ctx.accounts.grudge_match;
        require!(m.state == MatchState::Open, MhmError::MatchNotOpen);
        let monster = &ctx.accounts.monster;
        require!(monster.mint != m.monsters[0], MhmError::CannotMatchSelf);
        require!(
            ctx.accounts.joiner.key() != m.players[0],
            MhmError::CannotMatchSelf
        );

        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.joiner_nft_token.to_account_info(),
                    to: ctx.accounts.escrow_nft_token.to_account_info(),
                    authority: ctx.accounts.joiner.to_account_info(),
                },
            ),
            1,
        )?;

        m.players[1] = ctx.accounts.joiner.key();
        m.monsters[1] = monster.mint;
        m.rarity[1] = monster.rarity.index() as u8;
        m.level[1] = monster.level;
        m.base_hp[1] = monster.max_hp;
        m.base_power[1] = monster.power;
        m.base_defense[1] = monster.defense;

        let f0 = combat_stats::fighter(
            m.base_hp[0], m.base_power[0], m.base_defense[0],
            m.rarity[0] as usize, m.level[0], m.rarity[1] as usize,
        );
        let f1 = combat_stats::fighter(
            m.base_hp[1], m.base_power[1], m.base_defense[1],
            m.rarity[1] as usize, m.level[1], m.rarity[0] as usize,
        );
        m.board = Combat::new(&f0, &f1, now + TURN_DEADLINE_SECS);
        m.state = MatchState::Active;

        emit!(MatchJoined {
            grudge_match: m.key(),
            joiner: m.players[1],
            monster: monster.mint,
        });
        Ok(())
    }

    /// Submit this game's turn actions in a match (same rules as a battle turn).
    /// When a game ends the match tallies it toward the best-of-5.
    pub fn submit_match_action(
        ctx: Context<SubmitMatchAction>,
        consumable: u8,
        support: u8,
    ) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let m = &mut ctx.accounts.grudge_match;
        require!(m.state == MatchState::Active, MhmError::MatchNotActive);
        require!(now <= m.board.deadline, MhmError::TurnDeadlinePassed);

        let side = m
            .side_of(&ctx.accounts.player.key())
            .ok_or(MhmError::NotAMatchPlayer)?;
        require!(!m.board.pending[side].submitted, MhmError::AlreadySubmitted);

        let consumable_def = get_action(consumable).ok_or(MhmError::UnknownAction)?;
        require!(consumable_def.slot == ActionSlot::Consumable, MhmError::NotAConsumable);
        if support != NONE_U8 {
            let support_def = get_action(support).ok_or(MhmError::UnknownAction)?;
            require!(support_def.slot == ActionSlot::Support, MhmError::NotASupport);
        }

        m.board.pending[side] = PendingAction { submitted: true, consumable, support };

        if m.board.pending[0].submitted && m.board.pending[1].submitted {
            let outcome = combat::resolve_turn(&mut m.board, now);
            match outcome {
                state::TurnOutcome::Continue => {}
                state::TurnOutcome::Win(w) => conclude_game(m, Some(w), now),
                state::TurnOutcome::Draw => conclude_game(m, None, now),
            }
            emit!(MatchProgress {
                grudge_match: m.key(),
                game_wins: m.game_wins,
                games_played: m.games_played,
            });
            if m.state == MatchState::Finished {
                emit!(MatchFinished { grudge_match: m.key(), winner: m.winner });
            }
        }
        Ok(())
    }

    /// After the 90s clock lapses, award the current game to whoever submitted
    /// (both silent = a drawn game, replayed). May end the match.
    pub fn claim_match_timeout(ctx: Context<ClaimMatchTimeout>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let m = &mut ctx.accounts.grudge_match;
        require!(m.state == MatchState::Active, MhmError::MatchNotActive);
        require!(now > m.board.deadline, MhmError::DeadlineNotReached);
        m.side_of(&ctx.accounts.player.key())
            .ok_or(MhmError::NotAMatchPlayer)?;

        let a = m.board.pending[0].submitted;
        let b = m.board.pending[1].submitted;
        let game_winner = match (a, b) {
            (true, false) => Some(0u8),
            (false, true) => Some(1u8),
            (false, false) => None,
            // Both submitted would have auto-resolved on the 2nd submission.
            (true, true) => return err!(MhmError::CallerDidNotSubmit),
        };
        conclude_game(m, game_winner, now);
        emit!(MatchProgress {
            grudge_match: m.key(),
            game_wins: m.game_wins,
            games_played: m.games_played,
        });
        if m.state == MatchState::Finished {
            emit!(MatchFinished { grudge_match: m.key(), winner: m.winner });
        }
        Ok(())
    }

    /// Cancel an unaccepted Grudge Match; the escrowed NFT returns to you.
    pub fn cancel_match(ctx: Context<CancelMatch>) -> Result<()> {
        let m = &ctx.accounts.grudge_match;
        require!(m.state == MatchState::Open, MhmError::MatchNotOpen);
        require!(m.players[0] == ctx.accounts.creator.key(), MhmError::NotMatchCreator);

        let id = m.id;
        let bump = m.bump;
        let signer_seeds: &[&[&[u8]]] = &[&[MATCH_SEED, &id.to_le_bytes(), &[bump]]];
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.escrow_nft_token.to_account_info(),
                    to: ctx.accounts.creator_nft_token.to_account_info(),
                    authority: ctx.accounts.grudge_match.to_account_info(),
                },
                signer_seeds,
            ),
            1,
        )?;
        token::close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.escrow_nft_token.to_account_info(),
                destination: ctx.accounts.creator.to_account_info(),
                authority: ctx.accounts.grudge_match.to_account_info(),
            },
            signer_seeds,
        ))?;

        ctx.accounts.grudge_match.state = MatchState::Cancelled;
        Ok(())
    }

    /// Pay out a finished Grudge Match: WINNER TAKES BOTH NFTs. On a drawn
    /// match each monster returns to its original owner. Permissionless crank.
    pub fn settle_match(ctx: Context<SettleMatch>) -> Result<()> {
        let m = &ctx.accounts.grudge_match;
        require!(m.state == MatchState::Finished, MhmError::MatchNotFinished);

        // Decide who receives each NFT.
        let (rec_a, rec_b) = match m.winner {
            0 | 1 => {
                let w = m.players[m.winner as usize];
                (w, w)
            }
            _ => (m.players[0], m.players[1]), // draw: return to owners
        };
        require!(
            ctx.accounts.dest_a.owner == rec_a && ctx.accounts.dest_a.mint == m.monsters[0],
            MhmError::BadMatchRecipient
        );
        require!(
            ctx.accounts.dest_b.owner == rec_b && ctx.accounts.dest_b.mint == m.monsters[1],
            MhmError::BadMatchRecipient
        );

        let id = m.id;
        let bump = m.bump;
        let signer_seeds: &[&[&[u8]]] = &[&[MATCH_SEED, &id.to_le_bytes(), &[bump]]];

        for (escrow, dest, rent_to) in [
            (&ctx.accounts.escrow_a, &ctx.accounts.dest_a, &ctx.accounts.player0),
            (&ctx.accounts.escrow_b, &ctx.accounts.dest_b, &ctx.accounts.player1),
        ] {
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: escrow.to_account_info(),
                        to: dest.to_account_info(),
                        authority: ctx.accounts.grudge_match.to_account_info(),
                    },
                    signer_seeds,
                ),
                1,
            )?;
            token::close_account(CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                CloseAccount {
                    account: escrow.to_account_info(),
                    destination: rent_to.to_account_info(),
                    authority: ctx.accounts.grudge_match.to_account_info(),
                },
                signer_seeds,
            ))?;
        }

        // Persist win/loss records on the monsters.
        if m.winner == 0 || m.winner == 1 {
            let winner_side = m.winner as usize;
            if winner_side == 0 {
                ctx.accounts.monster_a.wins += 1;
                ctx.accounts.monster_b.losses += 1;
            } else {
                ctx.accounts.monster_b.wins += 1;
                ctx.accounts.monster_a.losses += 1;
            }
        }

        ctx.accounts.grudge_match.state = MatchState::Settled;
        emit!(MatchSettled {
            grudge_match: ctx.accounts.grudge_match.key(),
            winner: ctx.accounts.grudge_match.winner,
        });
        Ok(())
    }

    /// List a monster for sale at an MHM price. The NFT moves into a program
    /// escrow until the listing is bought or cancelled. The monster keeps
    /// mining while listed — its unclaimed pot travels to the buyer.
    pub fn list_monster(ctx: Context<ListMonster>, price: u64) -> Result<()> {
        require!(!ctx.accounts.config.paused, MhmError::GamePaused);
        require!(price > 0, MhmError::BadPrice);
        require!(!ctx.accounts.monster.in_battle, MhmError::MonsterInBattle);

        let listing = &mut ctx.accounts.listing;
        listing.seller = ctx.accounts.seller.key();
        listing.monster_mint = ctx.accounts.monster_mint.key();
        listing.price = price;
        listing.created_ts = Clock::get()?.unix_timestamp;
        listing.bump = ctx.bumps.listing;

        // Escrow the NFT.
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.seller_nft_token.to_account_info(),
                    to: ctx.accounts.escrow_nft_token.to_account_info(),
                    authority: ctx.accounts.seller.to_account_info(),
                },
            ),
            1,
        )?;

        emit!(MonsterListed {
            monster: listing.monster_mint,
            seller: listing.seller,
            price,
        });
        Ok(())
    }

    /// Cancel your listing: the NFT returns from escrow to your wallet.
    pub fn cancel_listing(ctx: Context<CancelListing>) -> Result<()> {
        let mint = ctx.accounts.listing.monster_mint;
        let bump = ctx.accounts.listing.bump;
        let signer_seeds: &[&[&[u8]]] = &[&[LISTING_SEED, mint.as_ref(), &[bump]]];

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.escrow_nft_token.to_account_info(),
                    to: ctx.accounts.seller_nft_token.to_account_info(),
                    authority: ctx.accounts.listing.to_account_info(),
                },
                signer_seeds,
            ),
            1,
        )?;
        token::close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.escrow_nft_token.to_account_info(),
                destination: ctx.accounts.seller.to_account_info(),
                authority: ctx.accounts.listing.to_account_info(),
            },
            signer_seeds,
        ))?;

        emit!(ListingCancelled { monster: mint, seller: ctx.accounts.seller.key() });
        Ok(())
    }

    /// Buy a listed monster with MHM. `market_fee_bps` of the price goes to
    /// the fee wallet, the rest to the seller; the NFT (and the monster's
    /// unclaimed mining pot with it) transfers to the buyer.
    pub fn buy_listing(ctx: Context<BuyListing>) -> Result<()> {
        require!(!ctx.accounts.config.paused, MhmError::GamePaused);

        let price = ctx.accounts.listing.price;
        let fee = (price as u128 * ctx.accounts.config.market_fee_bps as u128 / 10_000) as u64;
        let proceeds = price - fee;

        if fee > 0 {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.buyer_mhm_ata.to_account_info(),
                        to: ctx.accounts.fee_mhm_ata.to_account_info(),
                        authority: ctx.accounts.buyer.to_account_info(),
                    },
                ),
                fee,
            )?;
        }
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.buyer_mhm_ata.to_account_info(),
                    to: ctx.accounts.seller_mhm_ata.to_account_info(),
                    authority: ctx.accounts.buyer.to_account_info(),
                },
            ),
            proceeds,
        )?;

        // Release the NFT from escrow to the buyer.
        let mint = ctx.accounts.listing.monster_mint;
        let bump = ctx.accounts.listing.bump;
        let signer_seeds: &[&[&[u8]]] = &[&[LISTING_SEED, mint.as_ref(), &[bump]]];
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.escrow_nft_token.to_account_info(),
                    to: ctx.accounts.buyer_nft_token.to_account_info(),
                    authority: ctx.accounts.listing.to_account_info(),
                },
                signer_seeds,
            ),
            1,
        )?;
        token::close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.escrow_nft_token.to_account_info(),
                destination: ctx.accounts.seller.to_account_info(),
                authority: ctx.accounts.listing.to_account_info(),
            },
            signer_seeds,
        ))?;

        emit!(MonsterSold {
            monster: mint,
            seller: ctx.accounts.listing.seller,
            buyer: ctx.accounts.buyer.key(),
            price,
            fee,
        });
        Ok(())
    }
}

/// Commit step: reserve the monster id and seal the reveal to a future slot.
/// The `monster_mint` account is created (authority = config) but no token is
/// minted yet; traits are decided at reveal.
fn open_pending_mint<'info>(
    config: &mut Account<'info, GameConfig>,
    pending: &mut Account<'info, PendingMint>,
    pending_bump: u8,
    monster_mint: &Account<'info, Mint>,
    payer: &Signer<'info>,
) -> Result<()> {
    let clock = Clock::get()?;
    pending.minter = payer.key();
    pending.monster_mint = monster_mint.key();
    pending.monster_id = config.monsters_minted;
    pending.target_slot = clock.slot + REVEAL_DELAY_SLOTS;
    pending.bump = pending_bump;

    config.monsters_minted += 1;

    emit!(MonsterCommitted {
        monster: monster_mint.key(),
        minter: payer.key(),
        monster_id: pending.monster_id,
        target_slot: pending.target_slot,
    });
    Ok(())
}

/// Reveal step: write the rolled traits into the Monster account, mint the
/// single NFT to the minter, attach Metaplex metadata, and permanently revoke
/// the mint authority so supply is fixed at 1 forever.
#[allow(clippy::too_many_arguments)]
fn finalize_monster<'info>(
    config: &Account<'info, GameConfig>,
    monster: &mut Account<'info, Monster>,
    monster_bump: u8,
    monster_id: u64,
    minter: Pubkey,
    rolled: &traits::RolledTraits,
    now: i64,
    monster_mint: &Account<'info, Mint>,
    monster_token: &Account<'info, TokenAccount>,
    metadata: &UncheckedAccount<'info>,
    payer: &Signer<'info>,
    token_program: &Program<'info, Token>,
    token_metadata_program: &Program<'info, TokenMetadata>,
    system_program: &Program<'info, System>,
    rent: &Sysvar<'info, Rent>,
) -> Result<()> {
    monster.mint = monster_mint.key();
    monster.id = monster_id;
    monster.rarity = rolled.rarity;
    monster.level = 1;
    monster.mining_rate = rolled.mining_rate;
    monster.last_settled_ts = now;
    monster.unclaimed = 0;
    monster.max_hp = rolled.max_hp;
    monster.power = rolled.power;
    monster.defense = rolled.defense;
    monster.wins = 0;
    monster.losses = 0;
    monster.in_battle = false;
    monster.battle = Pubkey::default();
    monster.bump = monster_bump;

    let bump = config.bump;
    let signer_seeds: &[&[&[u8]]] = &[&[CONFIG_SEED, &[bump]]];
    token::mint_to(
        CpiContext::new_with_signer(
            token_program.to_account_info(),
            MintTo {
                mint: monster_mint.to_account_info(),
                to: monster_token.to_account_info(),
                authority: config.to_account_info(),
            },
            signer_seeds,
        ),
        1,
    )?;
    create_metadata_accounts_v3(
        CpiContext::new_with_signer(
            token_metadata_program.to_account_info(),
            CreateMetadataAccountsV3 {
                metadata: metadata.to_account_info(),
                mint: monster_mint.to_account_info(),
                mint_authority: config.to_account_info(),
                payer: payer.to_account_info(),
                update_authority: config.to_account_info(),
                system_program: system_program.to_account_info(),
                rent: rent.to_account_info(),
            },
            signer_seeds,
        ),
        DataV2 {
            name: format!("Mini Hungry Monster #{}", monster_id),
            symbol: "MHM".to_string(),
            uri: format!("{}{}.json", config.metadata_base_uri, rolled.rarity.slug()),
            seller_fee_basis_points: 0,
            creators: None,
            collection: None,
            uses: None,
        },
        true, // is_mutable: allow fixing URIs via a future update
        true, // update_authority (config PDA) is a signer
        None,
    )?;
    token::set_authority(
        CpiContext::new_with_signer(
            token_program.to_account_info(),
            SetAuthority {
                account_or_mint: monster_mint.to_account_info(),
                current_authority: config.to_account_info(),
            },
            signer_seeds,
        ),
        AuthorityType::MintTokens,
        None,
    )?;

    emit!(MonsterMinted {
        monster: monster.mint,
        owner: minter,
        id: monster_id,
        rarity: rolled.rarity,
        mining_rate: rolled.mining_rate,
        max_hp: rolled.max_hp,
        power: rolled.power,
        defense: rolled.defense,
    });
    Ok(())
}

/// Conclude one game of a match: tally the win (draws replay), then either
/// finish the match (someone reached [MATCH_WINS_NEEDED], or the game cap is
/// hit) or reset the board for the next game.
fn conclude_game(m: &mut GrudgeMatch, game_winner_side: Option<u8>, now: i64) {
    m.games_played = m.games_played.saturating_add(1);
    if let Some(side) = game_winner_side {
        if let Some(w) = record_game_win(&mut m.game_wins, side as usize) {
            m.state = MatchState::Finished;
            m.winner = w;
            return;
        }
    }
    if m.games_played >= MAX_MATCH_GAMES {
        m.state = MatchState::Finished;
        m.winner = decide_on_cap(&m.game_wins);
        return;
    }
    m.board.reset_for_next_game(now + TURN_DEADLINE_SECS);
}

// ---------------------------------------------------------------------------
// Instruction params
// ---------------------------------------------------------------------------

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct InitializeParams {
    /// Wallet that receives all fees (genesis SOL, MHM purchase fees, battle rake).
    pub fee_wallet: Pubkey,
    /// Share of MHM monster purchases that is burned, in basis points.
    pub burn_bps: u16,
    /// Rake on battle loot, in basis points.
    pub battle_fee_bps: u16,
    /// Fee on marketplace sales, in basis points.
    pub market_fee_bps: u16,
    /// Base micro-MHM cost unit for leveling up (scaled by rarity + level).
    pub level_up_base_cost: u64,
    /// Base URI for NFT metadata (should end with '/'); "<rarity>.json" is appended.
    pub metadata_base_uri: String,
    pub monster_price_mhm: u64,
    pub genesis_price_lamports: u64,
    pub genesis_remaining: u32,
    pub mining_rate_ranges: [[u64; 2]; NUM_RARITIES],
    pub rarity_weights_bps: [u16; NUM_RARITIES],
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct UpdateConfigParams {
    pub fee_wallet: Option<Pubkey>,
    pub burn_bps: Option<u16>,
    pub battle_fee_bps: Option<u16>,
    pub market_fee_bps: Option<u16>,
    pub level_up_base_cost: Option<u64>,
    pub metadata_base_uri: Option<String>,
    pub monster_price_mhm: Option<u64>,
    pub genesis_price_lamports: Option<u64>,
    pub genesis_remaining: Option<u32>,
    pub mining_rate_ranges: Option<[[u64; 2]; NUM_RARITIES]>,
    pub rarity_weights_bps: Option<[u16; NUM_RARITIES]>,
    pub paused: Option<bool>,
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = admin,
        space = 8 + GameConfig::INIT_SPACE,
        seeds = [CONFIG_SEED],
        bump
    )]
    pub config: Account<'info, GameConfig>,

    #[account(
        init,
        payer = admin,
        seeds = [MHM_MINT_SEED],
        bump,
        mint::decimals = MHM_DECIMALS,
        mint::authority = config,
    )]
    pub mhm_mint: Account<'info, Mint>,

    #[account(mut)]
    pub admin: Signer<'info>,

    /// This program, used to prove the initializer is the deploy authority.
    #[account(constraint = program.programdata_address()? == Some(program_data.key()) @ MhmError::Unauthorized)]
    pub program: Program<'info, program::MhmGame>,

    /// The program's upgrade-authority record. Constraining `initialize` to
    /// the upgrade authority closes the deploy-time front-running window where
    /// an attacker could otherwise call `initialize` first and seize admin.
    #[account(constraint = program_data.upgrade_authority_address == Some(admin.key()) @ MhmError::Unauthorized)]
    pub program_data: Account<'info, ProgramData>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ MhmError::Unauthorized)]
    pub config: Account<'info, GameConfig>,
    pub admin: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(monster_id: u64)]
pub struct CommitHatchGenesis<'info> {
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, GameConfig>,

    #[account(
        init,
        payer = payer,
        seeds = [b"monster-mint", monster_id.to_le_bytes().as_ref()],
        bump,
        mint::decimals = 0,
        mint::authority = config,
    )]
    pub monster_mint: Account<'info, Mint>,

    #[account(
        init,
        payer = payer,
        space = 8 + PendingMint::INIT_SPACE,
        seeds = [PENDING_SEED, monster_id.to_le_bytes().as_ref()],
        bump
    )]
    pub pending: Account<'info, PendingMint>,

    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: receives the genesis SOL payment; enforced to be the configured fee wallet.
    #[account(mut, address = config.fee_wallet)]
    pub fee_wallet: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(monster_id: u64)]
pub struct CommitBuyMonster<'info> {
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, GameConfig>,

    #[account(mut, seeds = [MHM_MINT_SEED], bump, address = config.mhm_mint)]
    pub mhm_mint: Account<'info, Mint>,

    /// Buyer's MHM account; the purchase price is taken from here.
    #[account(
        mut,
        associated_token::mint = mhm_mint,
        associated_token::authority = payer,
    )]
    pub payer_mhm_ata: Account<'info, TokenAccount>,

    /// CHECK: the configured fee wallet (receives the non-burned price share).
    #[account(address = config.fee_wallet)]
    pub fee_wallet: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = mhm_mint,
        associated_token::authority = fee_wallet,
    )]
    pub fee_mhm_ata: Account<'info, TokenAccount>,

    #[account(
        init,
        payer = payer,
        seeds = [b"monster-mint", monster_id.to_le_bytes().as_ref()],
        bump,
        mint::decimals = 0,
        mint::authority = config,
    )]
    pub monster_mint: Account<'info, Mint>,

    #[account(
        init,
        payer = payer,
        space = 8 + PendingMint::INIT_SPACE,
        seeds = [PENDING_SEED, monster_id.to_le_bytes().as_ref()],
        bump
    )]
    pub pending: Account<'info, PendingMint>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct RevealMonster<'info> {
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, GameConfig>,

    /// The committed hatch. Closed at the end, rent back to the minter.
    /// Only the original minter may reveal (they receive the NFT).
    #[account(
        mut,
        close = payer,
        seeds = [PENDING_SEED, pending.monster_id.to_le_bytes().as_ref()],
        bump = pending.bump,
        constraint = pending.minter == payer.key() @ MhmError::NotMonsterHolder,
    )]
    pub pending: Account<'info, PendingMint>,

    #[account(
        mut,
        seeds = [b"monster-mint", pending.monster_id.to_le_bytes().as_ref()],
        bump,
        address = pending.monster_mint,
    )]
    pub monster_mint: Account<'info, Mint>,

    #[account(
        init,
        payer = payer,
        space = 8 + Monster::INIT_SPACE,
        seeds = [MONSTER_SEED, monster_mint.key().as_ref()],
        bump
    )]
    pub monster: Account<'info, Monster>,

    // init_if_needed (not init) so a griefer front-running the deterministic
    // ATA between commit and reveal can't block the reveal; the ATA address is
    // still pinned to (monster_mint, payer).
    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = monster_mint,
        associated_token::authority = payer,
    )]
    pub monster_token: Account<'info, TokenAccount>,

    /// CHECK: created by the token metadata program via CPI; address is the
    /// canonical metadata PDA for the monster mint.
    #[account(
        mut,
        seeds = [b"metadata", token_metadata_program.key().as_ref(), monster_mint.key().as_ref()],
        seeds::program = token_metadata_program.key(),
        bump,
    )]
    pub metadata: UncheckedAccount<'info>,

    /// CHECK: the SlotHashes sysvar, read to seed the roll. Address-checked.
    #[account(address = anchor_lang::solana_program::sysvar::slot_hashes::ID)]
    pub slot_hashes: UncheckedAccount<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub token_metadata_program: Program<'info, TokenMetadata>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct ClaimMining<'info> {
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, GameConfig>,

    #[account(
        mut,
        seeds = [MONSTER_SEED, monster.mint.as_ref()],
        bump = monster.bump
    )]
    pub monster: Account<'info, Monster>,

    /// Proof of ownership: the holder's token account for this monster's NFT.
    #[account(
        constraint = holder_nft_token.mint == monster.mint @ MhmError::NotMonsterHolder,
        constraint = holder_nft_token.owner == holder.key() @ MhmError::NotMonsterHolder,
        constraint = holder_nft_token.amount == 1 @ MhmError::NotMonsterHolder,
    )]
    pub holder_nft_token: Account<'info, TokenAccount>,

    #[account(mut, seeds = [MHM_MINT_SEED], bump, address = config.mhm_mint)]
    pub mhm_mint: Account<'info, Mint>,

    #[account(
        init_if_needed,
        payer = holder,
        associated_token::mint = mhm_mint,
        associated_token::authority = holder,
    )]
    pub holder_mhm_ata: Account<'info, TokenAccount>,

    #[account(mut)]
    pub holder: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct LevelUp<'info> {
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, GameConfig>,

    #[account(
        mut,
        seeds = [MONSTER_SEED, monster.mint.as_ref()],
        bump = monster.bump
    )]
    pub monster: Account<'info, Monster>,

    /// Proof of ownership: the holder's token account for this monster's NFT.
    #[account(
        constraint = holder_nft_token.mint == monster.mint @ MhmError::NotMonsterHolder,
        constraint = holder_nft_token.owner == holder.key() @ MhmError::NotMonsterHolder,
        constraint = holder_nft_token.amount == 1 @ MhmError::NotMonsterHolder,
    )]
    pub holder_nft_token: Account<'info, TokenAccount>,

    #[account(seeds = [MHM_MINT_SEED], bump, address = config.mhm_mint)]
    pub mhm_mint: Account<'info, Mint>,

    /// Holder's MHM account; the level-up cost is taken from here.
    #[account(
        mut,
        associated_token::mint = mhm_mint,
        associated_token::authority = holder,
    )]
    pub holder_mhm_ata: Account<'info, TokenAccount>,

    /// CHECK: the configured fee wallet (receives the level-up fee).
    #[account(address = config.fee_wallet)]
    pub fee_wallet: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = holder,
        associated_token::mint = mhm_mint,
        associated_token::authority = fee_wallet,
    )]
    pub fee_mhm_ata: Account<'info, TokenAccount>,

    #[account(mut)]
    pub holder: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
#[instruction(battle_id: u64)]
pub struct CreateBattle<'info> {
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, GameConfig>,

    #[account(
        init,
        payer = creator,
        space = 8 + Battle::INIT_SPACE,
        seeds = [BATTLE_SEED, battle_id.to_le_bytes().as_ref()],
        bump
    )]
    pub battle: Account<'info, Battle>,

    #[account(
        mut,
        seeds = [MONSTER_SEED, monster.mint.as_ref()],
        bump = monster.bump
    )]
    pub monster: Account<'info, Monster>,

    #[account(
        constraint = creator_nft_token.mint == monster.mint @ MhmError::NotMonsterHolder,
        constraint = creator_nft_token.owner == creator.key() @ MhmError::NotMonsterHolder,
        constraint = creator_nft_token.amount == 1 @ MhmError::NotMonsterHolder,
    )]
    pub creator_nft_token: Account<'info, TokenAccount>,

    #[account(mut)]
    pub creator: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct JoinBattle<'info> {
    #[account(mut)]
    pub battle: Account<'info, Battle>,

    #[account(
        mut,
        seeds = [MONSTER_SEED, monster.mint.as_ref()],
        bump = monster.bump
    )]
    pub monster: Account<'info, Monster>,

    #[account(
        constraint = joiner_nft_token.mint == monster.mint @ MhmError::NotMonsterHolder,
        constraint = joiner_nft_token.owner == joiner.key() @ MhmError::NotMonsterHolder,
        constraint = joiner_nft_token.amount == 1 @ MhmError::NotMonsterHolder,
    )]
    pub joiner_nft_token: Account<'info, TokenAccount>,

    pub joiner: Signer<'info>,
}

#[derive(Accounts)]
pub struct SubmitAction<'info> {
    #[account(mut)]
    pub battle: Account<'info, Battle>,
    pub player: Signer<'info>,
}

#[derive(Accounts)]
pub struct ClaimTimeout<'info> {
    #[account(mut)]
    pub battle: Account<'info, Battle>,
    pub player: Signer<'info>,
}

#[derive(Accounts)]
pub struct CancelBattle<'info> {
    #[account(mut)]
    pub battle: Account<'info, Battle>,

    #[account(
        mut,
        seeds = [MONSTER_SEED, monster.mint.as_ref()],
        bump = monster.bump,
        constraint = monster.mint == battle.monsters[0] @ MhmError::MonsterNotInBattle
    )]
    pub monster: Account<'info, Monster>,

    pub creator: Signer<'info>,
}

#[derive(Accounts)]
pub struct SettleBattle<'info> {
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, GameConfig>,

    #[account(mut)]
    pub battle: Account<'info, Battle>,

    #[account(
        mut,
        seeds = [MONSTER_SEED, monster_a.mint.as_ref()],
        bump = monster_a.bump,
        constraint = monster_a.mint == battle.monsters[0] @ MhmError::MonsterNotInBattle
    )]
    pub monster_a: Account<'info, Monster>,

    #[account(
        mut,
        seeds = [MONSTER_SEED, monster_b.mint.as_ref()],
        bump = monster_b.bump,
        constraint = monster_b.mint == battle.monsters[1] @ MhmError::MonsterNotInBattle
    )]
    pub monster_b: Account<'info, Monster>,

    #[account(mut, seeds = [MHM_MINT_SEED], bump, address = config.mhm_mint)]
    pub mhm_mint: Account<'info, Mint>,

    /// The winning player's MHM token account (required unless the battle
    /// was a draw). Loot is minted here.
    #[account(mut)]
    pub winner_mhm_ata: Option<Account<'info, TokenAccount>>,

    /// The fee wallet's MHM token account (required when a battle rake is
    /// configured and there is a winner). The rake is minted here.
    #[account(mut)]
    pub fee_mhm_ata: Option<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct AdminMintMhm<'info> {
    #[account(seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ MhmError::Unauthorized)]
    pub config: Account<'info, GameConfig>,

    #[account(mut, seeds = [MHM_MINT_SEED], bump, address = config.mhm_mint)]
    pub mhm_mint: Account<'info, Mint>,

    /// CHECK: the holder wallet receiving the MHM allocation; the admin
    /// signs off on the destination.
    pub recipient: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = admin,
        associated_token::mint = mhm_mint,
        associated_token::authority = recipient,
    )]
    pub recipient_mhm_ata: Account<'info, TokenAccount>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
#[instruction(match_id: u64)]
pub struct CreateMatch<'info> {
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, GameConfig>,

    #[account(
        init,
        payer = creator,
        space = 8 + GrudgeMatch::INIT_SPACE,
        seeds = [MATCH_SEED, match_id.to_le_bytes().as_ref()],
        bump
    )]
    pub grudge_match: Account<'info, GrudgeMatch>,

    pub monster_mint: Account<'info, Mint>,

    #[account(
        seeds = [MONSTER_SEED, monster_mint.key().as_ref()],
        bump = monster.bump
    )]
    pub monster: Account<'info, Monster>,

    #[account(
        mut,
        constraint = creator_nft_token.mint == monster_mint.key() @ MhmError::NotMonsterHolder,
        constraint = creator_nft_token.owner == creator.key() @ MhmError::NotMonsterHolder,
        constraint = creator_nft_token.amount == 1 @ MhmError::NotMonsterHolder,
    )]
    pub creator_nft_token: Account<'info, TokenAccount>,

    #[account(
        init,
        payer = creator,
        associated_token::mint = monster_mint,
        associated_token::authority = grudge_match,
    )]
    pub escrow_nft_token: Account<'info, TokenAccount>,

    #[account(mut)]
    pub creator: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct JoinMatch<'info> {
    #[account(mut)]
    pub grudge_match: Account<'info, GrudgeMatch>,

    pub monster_mint: Account<'info, Mint>,

    #[account(
        seeds = [MONSTER_SEED, monster_mint.key().as_ref()],
        bump = monster.bump
    )]
    pub monster: Account<'info, Monster>,

    #[account(
        mut,
        constraint = joiner_nft_token.mint == monster_mint.key() @ MhmError::NotMonsterHolder,
        constraint = joiner_nft_token.owner == joiner.key() @ MhmError::NotMonsterHolder,
        constraint = joiner_nft_token.amount == 1 @ MhmError::NotMonsterHolder,
    )]
    pub joiner_nft_token: Account<'info, TokenAccount>,

    #[account(
        init,
        payer = joiner,
        associated_token::mint = monster_mint,
        associated_token::authority = grudge_match,
    )]
    pub escrow_nft_token: Account<'info, TokenAccount>,

    #[account(mut)]
    pub joiner: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct SubmitMatchAction<'info> {
    #[account(mut)]
    pub grudge_match: Account<'info, GrudgeMatch>,
    pub player: Signer<'info>,
}

#[derive(Accounts)]
pub struct ClaimMatchTimeout<'info> {
    #[account(mut)]
    pub grudge_match: Account<'info, GrudgeMatch>,
    pub player: Signer<'info>,
}

#[derive(Accounts)]
pub struct CancelMatch<'info> {
    #[account(mut, close = creator)]
    pub grudge_match: Account<'info, GrudgeMatch>,

    #[account(address = grudge_match.monsters[0])]
    pub monster_mint: Account<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = monster_mint,
        associated_token::authority = grudge_match,
    )]
    pub escrow_nft_token: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = creator,
        associated_token::mint = monster_mint,
        associated_token::authority = creator,
    )]
    pub creator_nft_token: Account<'info, TokenAccount>,

    #[account(mut)]
    pub creator: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct SettleMatch<'info> {
    #[account(mut, close = payer)]
    pub grudge_match: Account<'info, GrudgeMatch>,

    #[account(address = grudge_match.monsters[0])]
    pub monster_mint_a: Account<'info, Mint>,
    #[account(address = grudge_match.monsters[1])]
    pub monster_mint_b: Account<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = monster_mint_a,
        associated_token::authority = grudge_match,
    )]
    pub escrow_a: Account<'info, TokenAccount>,
    #[account(
        mut,
        associated_token::mint = monster_mint_b,
        associated_token::authority = grudge_match,
    )]
    pub escrow_b: Account<'info, TokenAccount>,

    /// Destination NFT accounts; validated in the handler against the winner
    /// (or, on a draw, each monster's original owner).
    #[account(mut)]
    pub dest_a: Account<'info, TokenAccount>,
    #[account(mut)]
    pub dest_b: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [MONSTER_SEED, monster_mint_a.key().as_ref()],
        bump = monster_a.bump,
        constraint = monster_a.mint == grudge_match.monsters[0] @ MhmError::MonsterNotInBattle
    )]
    pub monster_a: Account<'info, Monster>,
    #[account(
        mut,
        seeds = [MONSTER_SEED, monster_mint_b.key().as_ref()],
        bump = monster_b.bump,
        constraint = monster_b.mint == grudge_match.monsters[1] @ MhmError::MonsterNotInBattle
    )]
    pub monster_b: Account<'info, Monster>,

    /// CHECK: escrow-A rent returns here; enforced to be side 0's player.
    #[account(mut, address = grudge_match.players[0])]
    pub player0: UncheckedAccount<'info>,
    /// CHECK: escrow-B rent returns here; enforced to be side 1's player.
    #[account(mut, address = grudge_match.players[1])]
    pub player1: UncheckedAccount<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ListMonster<'info> {
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, GameConfig>,

    #[account(
        seeds = [MONSTER_SEED, monster_mint.key().as_ref()],
        bump = monster.bump
    )]
    pub monster: Account<'info, Monster>,

    pub monster_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = seller_nft_token.mint == monster_mint.key() @ MhmError::NotMonsterHolder,
        constraint = seller_nft_token.owner == seller.key() @ MhmError::NotMonsterHolder,
        constraint = seller_nft_token.amount == 1 @ MhmError::NotMonsterHolder,
    )]
    pub seller_nft_token: Account<'info, TokenAccount>,

    #[account(
        init,
        payer = seller,
        space = 8 + Listing::INIT_SPACE,
        seeds = [LISTING_SEED, monster_mint.key().as_ref()],
        bump
    )]
    pub listing: Account<'info, Listing>,

    /// Escrow token account holding the NFT while listed; owned by the
    /// listing PDA so only the program can release it.
    #[account(
        init,
        payer = seller,
        associated_token::mint = monster_mint,
        associated_token::authority = listing,
    )]
    pub escrow_nft_token: Account<'info, TokenAccount>,

    #[account(mut)]
    pub seller: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct CancelListing<'info> {
    #[account(
        mut,
        close = seller,
        seeds = [LISTING_SEED, listing.monster_mint.as_ref()],
        bump = listing.bump,
        constraint = listing.seller == seller.key() @ MhmError::NotSeller,
    )]
    pub listing: Account<'info, Listing>,

    #[account(address = listing.monster_mint)]
    pub monster_mint: Account<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = monster_mint,
        associated_token::authority = listing,
    )]
    pub escrow_nft_token: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = seller,
        associated_token::mint = monster_mint,
        associated_token::authority = seller,
    )]
    pub seller_nft_token: Account<'info, TokenAccount>,

    #[account(mut)]
    pub seller: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct BuyListing<'info> {
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, GameConfig>,

    #[account(
        mut,
        close = seller,
        seeds = [LISTING_SEED, listing.monster_mint.as_ref()],
        bump = listing.bump,
    )]
    pub listing: Account<'info, Listing>,

    #[account(address = listing.monster_mint)]
    pub monster_mint: Account<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = monster_mint,
        associated_token::authority = listing,
    )]
    pub escrow_nft_token: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = buyer,
        associated_token::mint = monster_mint,
        associated_token::authority = buyer,
    )]
    pub buyer_nft_token: Account<'info, TokenAccount>,

    #[account(seeds = [MHM_MINT_SEED], bump, address = config.mhm_mint)]
    pub mhm_mint: Account<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = mhm_mint,
        associated_token::authority = buyer,
    )]
    pub buyer_mhm_ata: Account<'info, TokenAccount>,

    /// CHECK: the seller; receives sale proceeds and reclaimed rent.
    /// Enforced to match the listing.
    #[account(mut, address = listing.seller)]
    pub seller: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = buyer,
        associated_token::mint = mhm_mint,
        associated_token::authority = seller,
    )]
    pub seller_mhm_ata: Account<'info, TokenAccount>,

    /// CHECK: the configured fee wallet.
    #[account(address = config.fee_wallet)]
    pub fee_wallet: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = buyer,
        associated_token::mint = mhm_mint,
        associated_token::authority = fee_wallet,
    )]
    pub fee_mhm_ata: Account<'info, TokenAccount>,

    #[account(mut)]
    pub buyer: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[event]
pub struct MonsterListed {
    pub monster: Pubkey,
    pub seller: Pubkey,
    pub price: u64,
}

#[event]
pub struct ListingCancelled {
    pub monster: Pubkey,
    pub seller: Pubkey,
}

#[event]
pub struct MonsterSold {
    pub monster: Pubkey,
    pub seller: Pubkey,
    pub buyer: Pubkey,
    pub price: u64,
    pub fee: u64,
}

#[event]
pub struct MonsterCommitted {
    pub monster: Pubkey,
    pub minter: Pubkey,
    pub monster_id: u64,
    pub target_slot: u64,
}

#[event]
pub struct MonsterMinted {
    pub monster: Pubkey,
    pub owner: Pubkey,
    pub id: u64,
    pub rarity: Rarity,
    pub mining_rate: u64,
    pub max_hp: u32,
    pub power: u32,
    pub defense: u32,
}

#[event]
pub struct MiningClaimed {
    pub monster: Pubkey,
    pub holder: Pubkey,
    pub amount: u64,
}

#[event]
pub struct MonsterLeveledUp {
    pub monster: Pubkey,
    pub holder: Pubkey,
    pub level: u16,
    pub cost: u64,
}

#[event]
pub struct BattleCreated {
    pub battle: Pubkey,
    pub creator: Pubkey,
    pub monster: Pubkey,
    pub pot: u64,
}

#[event]
pub struct BattleJoined {
    pub battle: Pubkey,
    pub joiner: Pubkey,
    pub monster: Pubkey,
    pub pot: u64,
}

#[event]
pub struct TurnResolved {
    pub battle: Pubkey,
    pub turn: u16,
    pub hp: [u32; 2],
}

#[event]
pub struct BattleFinished {
    pub battle: Pubkey,
    /// 0 or 1 = winning side index, 2 = draw.
    pub winner: u8,
}

#[event]
pub struct BattleSettled {
    pub battle: Pubkey,
    pub winner: Pubkey,
    pub loot: u64,
}

#[event]
pub struct MatchCreated {
    pub grudge_match: Pubkey,
    pub creator: Pubkey,
    pub monster: Pubkey,
}

#[event]
pub struct MatchJoined {
    pub grudge_match: Pubkey,
    pub joiner: Pubkey,
    pub monster: Pubkey,
}

#[event]
pub struct MatchProgress {
    pub grudge_match: Pubkey,
    pub game_wins: [u8; 2],
    pub games_played: u8,
}

#[event]
pub struct MatchFinished {
    pub grudge_match: Pubkey,
    pub winner: u8,
}

#[event]
pub struct MatchSettled {
    pub grudge_match: Pubkey,
    pub winner: u8,
}
