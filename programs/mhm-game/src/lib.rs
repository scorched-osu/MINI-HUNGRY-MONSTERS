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
use anchor_spl::token::spl_token::instruction::AuthorityType;
use anchor_spl::token::{self, Burn, Mint, MintTo, SetAuthority, Token, TokenAccount, Transfer};

pub mod actions;
pub mod combat;
pub mod errors;
pub mod rng;
pub mod state;

use actions::{get_action, ActionSlot};
use errors::MhmError;
use rng::Roll;
use state::*;

declare_id!("594wvdBsGrU6g6LswPmsx7tk1Cc7fgrpDAxvF8CEWq9K");

/// Base battle stats per rarity tier (Standard..Unique); a random bonus is
/// rolled on top at mint time.
const BASE_MAX_HP: [u32; NUM_RARITIES] = [100, 130, 170, 220, 300];
const BASE_POWER: [u32; NUM_RARITIES] = [50, 65, 85, 110, 150];
const BASE_DEFENSE: [u32; NUM_RARITIES] = [20, 30, 45, 65, 90];
const HP_ROLL: u32 = 20;
const POWER_ROLL: u32 = 15;
const DEFENSE_ROLL: u32 = 10;

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

        let config = &mut ctx.accounts.config;
        config.admin = ctx.accounts.admin.key();
        config.mhm_mint = ctx.accounts.mhm_mint.key();
        config.fee_wallet = params.fee_wallet;
        config.burn_bps = params.burn_bps;
        config.battle_fee_bps = params.battle_fee_bps;
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

    /// Genesis hatch: buy a monster for SOL. This bootstraps the economy
    /// before any MHM circulates; supply is capped by `genesis_remaining`.
    pub fn hatch_genesis(ctx: Context<HatchGenesis>, monster_id: u64) -> Result<()> {
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

        ctx.accounts.config.genesis_remaining -= 1;
        mint_monster(
            &mut ctx.accounts.config,
            &mut ctx.accounts.monster,
            ctx.bumps.monster,
            &ctx.accounts.monster_mint,
            &ctx.accounts.monster_token,
            &ctx.accounts.payer,
            &ctx.accounts.token_program,
        )
    }

    /// Buy a monster with MHM coin. `burn_bps` of the price is burned
    /// (deflationary sink); the rest goes to the fee wallet.
    pub fn buy_monster(ctx: Context<BuyMonster>, monster_id: u64) -> Result<()> {
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

        mint_monster(
            &mut ctx.accounts.config,
            &mut ctx.accounts.monster,
            ctx.bumps.monster,
            &ctx.accounts.monster_mint,
            &ctx.accounts.monster_token,
            &ctx.accounts.payer,
            &ctx.accounts.token_program,
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
        battle.hp[0] = monster.max_hp;
        battle.max_hp[0] = monster.max_hp;
        battle.power[0] = monster.power;
        battle.defense[0] = monster.defense;
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

        monster.settle(now);
        let pot = monster.unclaimed;
        monster.unclaimed = 0;
        monster.in_battle = true;
        monster.battle = battle.key();

        battle.state = BattleState::Active;
        battle.players[1] = ctx.accounts.joiner.key();
        battle.monsters[1] = monster.mint;
        battle.pots[1] = pot;
        battle.hp[1] = monster.max_hp;
        battle.max_hp[1] = monster.max_hp;
        battle.power[1] = monster.power;
        battle.defense[1] = monster.defense;
        battle.turn = 1;
        battle.deadline = now + TURN_DEADLINE_SECS;

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
        require!(now <= battle.deadline, MhmError::TurnDeadlinePassed);

        let side = battle
            .side_of(&ctx.accounts.player.key())
            .ok_or(MhmError::NotABattlePlayer)?;
        require!(!battle.pending[side].submitted, MhmError::AlreadySubmitted);

        let consumable_def = get_action(consumable).ok_or(MhmError::UnknownAction)?;
        require!(
            consumable_def.slot == ActionSlot::Consumable,
            MhmError::NotAConsumable
        );
        if support != NONE_U8 {
            let support_def = get_action(support).ok_or(MhmError::UnknownAction)?;
            require!(support_def.slot == ActionSlot::Support, MhmError::NotASupport);
        }

        battle.pending[side] = PendingAction { submitted: true, consumable, support };

        if battle.pending[0].submitted && battle.pending[1].submitted {
            let turn = battle.turn;
            combat::resolve_turn(battle, now);
            emit!(TurnResolved {
                battle: battle.key(),
                turn,
                hp: battle.hp,
            });
            if battle.state == BattleState::Finished {
                emit!(BattleFinished { battle: battle.key(), winner: battle.winner });
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
        require!(now > battle.deadline, MhmError::DeadlineNotReached);

        let side = battle
            .side_of(&ctx.accounts.player.key())
            .ok_or(MhmError::NotABattlePlayer)?;
        let opponent = 1 - side;

        if battle.pending[side].submitted && !battle.pending[opponent].submitted {
            battle.winner = side as u8;
        } else if !battle.pending[side].submitted && !battle.pending[opponent].submitted {
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
}

/// Roll rarity + stats and mint the NFT to the payer. Shared by genesis
/// hatches and MHM purchases.
fn mint_monster<'info>(
    config: &mut Account<'info, GameConfig>,
    monster: &mut Account<'info, Monster>,
    monster_bump: u8,
    monster_mint: &Account<'info, Mint>,
    monster_token: &Account<'info, TokenAccount>,
    payer: &Signer<'info>,
    token_program: &Program<'info, Token>,
) -> Result<()> {
    let clock = Clock::get()?;
    let mut roll = Roll::new(&clock, &payer.key(), config.monsters_minted);

    // Weighted rarity roll.
    let pick = roll.bps();
    let mut cumulative: u16 = 0;
    let mut rarity_index = NUM_RARITIES - 1;
    for (i, weight) in config.rarity_weights_bps.iter().enumerate() {
        cumulative = cumulative.saturating_add(*weight);
        if pick < cumulative {
            rarity_index = i;
            break;
        }
    }
    let rarity = Rarity::from_index(rarity_index);

    // Mining speed from the tier's configured range.
    let range = config.mining_rate_ranges[rarity_index];
    let mining_rate = roll.range_u64(range[0], range[1]);

    monster.mint = monster_mint.key();
    monster.id = config.monsters_minted;
    monster.rarity = rarity;
    monster.mining_rate = mining_rate;
    monster.last_settled_ts = clock.unix_timestamp;
    monster.unclaimed = 0;
    monster.max_hp = BASE_MAX_HP[rarity_index] + roll.range_u32(0, HP_ROLL);
    monster.power = BASE_POWER[rarity_index] + roll.range_u32(0, POWER_ROLL);
    monster.defense = BASE_DEFENSE[rarity_index] + roll.range_u32(0, DEFENSE_ROLL);
    monster.wins = 0;
    monster.losses = 0;
    monster.in_battle = false;
    monster.battle = Pubkey::default();
    monster.bump = monster_bump;

    config.monsters_minted += 1;

    // Mint exactly 1 NFT token to the payer, then permanently revoke the
    // mint authority so supply is fixed at 1 forever.
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
        owner: payer.key(),
        id: monster.id,
        rarity: monster.rarity,
        mining_rate,
        max_hp: monster.max_hp,
        power: monster.power,
        defense: monster.defense,
    });
    Ok(())
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
pub struct HatchGenesis<'info> {
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
        space = 8 + Monster::INIT_SPACE,
        seeds = [MONSTER_SEED, monster_mint.key().as_ref()],
        bump
    )]
    pub monster: Account<'info, Monster>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = monster_mint,
        associated_token::authority = payer,
    )]
    pub monster_token: Account<'info, TokenAccount>,

    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: receives the genesis SOL payment; enforced to be the configured fee wallet.
    #[account(mut, address = config.fee_wallet)]
    pub fee_wallet: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(monster_id: u64)]
pub struct BuyMonster<'info> {
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
        space = 8 + Monster::INIT_SPACE,
        seeds = [MONSTER_SEED, monster_mint.key().as_ref()],
        bump
    )]
    pub monster: Account<'info, Monster>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = monster_mint,
        associated_token::authority = payer,
    )]
    pub monster_token: Account<'info, TokenAccount>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
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

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

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
