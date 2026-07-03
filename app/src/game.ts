// Thin client over the on-chain mhm-game program.
//
// The IDL (src/idl/mhm_game.json) is generated from the Rust program with
// `anchor idl build` / `anchor build` — regenerate it whenever the program
// changes.

import { AnchorProvider, BN, Program } from '@coral-xyz/anchor'
import type { Idl, Wallet } from '@coral-xyz/anchor'
import type { AnchorWallet } from '@solana/wallet-adapter-react'
import {
  PublicKey,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
  SYSVAR_SLOT_HASHES_PUBKEY,
  type Connection,
  type TransactionInstruction,
} from '@solana/web3.js'
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountIdempotentInstruction,
  getAssociatedTokenAddressSync,
} from '@solana/spl-token'
import idl from './idl/mhm_game.json'

export const PROGRAM_ID = new PublicKey((idl as Idl).address)
export const MICRO = 1_000_000 // 1 MHM = 1_000_000 micro-MHM
export const TOKEN_METADATA_PROGRAM_ID = new PublicKey(
  'metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s',
)

// ---------- account shapes (as decoded by Anchor) ----------

export interface GameConfig {
  admin: PublicKey
  mhmMint: PublicKey
  feeWallet: PublicKey
  burnBps: number
  battleFeeBps: number
  marketFeeBps: number
  levelUpBaseCost: BN
  monsterPriceMhm: BN
  genesisPriceLamports: BN
  genesisRemaining: number
  miningRateRanges: BN[][]
  rarityWeightsBps: number[]
  monstersMinted: BN
  battlesCreated: BN
  paused: boolean
}

export interface Monster {
  mint: PublicKey
  id: BN
  rarity: Record<string, unknown>
  level: number
  miningRate: BN
  lastSettledTs: BN
  unclaimed: BN
  maxHp: number
  power: number
  defense: number
  wins: number
  losses: number
  inBattle: boolean
  battle: PublicKey
}

export interface Combat {
  hp: number[]
  maxHp: number[]
  power: number[]
  defense: number[]
  speed: number[]
  dmgBonusBps: number[]
  defensePierceBps: number[]
  alwaysFirst: boolean[]
  defBuff: number[]
  defBuffTurns: number[]
  pending: { submitted: boolean; consumable: number; support: number }[]
  turn: number
  deadline: BN
}

export interface Battle {
  id: BN
  state: Record<string, unknown>
  players: PublicKey[]
  monsters: PublicKey[]
  pots: BN[]
  rarity: number[]
  level: number[]
  baseHp: number[]
  basePower: number[]
  baseDefense: number[]
  board: Combat
  winner: number
}

export interface Listing {
  seller: PublicKey
  monsterMint: PublicKey
  price: BN
  createdTs: BN
}

export interface GrudgeMatch {
  id: BN
  state: Record<string, unknown>
  players: PublicKey[]
  monsters: PublicKey[]
  rarity: number[]
  level: number[]
  baseHp: number[]
  basePower: number[]
  baseDefense: number[]
  gameWins: number[]
  gamesPlayed: number
  board: Combat
  winner: number
}

export interface Keyed<T> {
  publicKey: PublicKey
  account: T
}

// ---------- PDAs ----------

const le8 = (n: BN | number) => new BN(n).toArrayLike(Buffer, 'le', 8)

export const configPda = () =>
  PublicKey.findProgramAddressSync([Buffer.from('config')], PROGRAM_ID)[0]
export const mhmMintPda = () =>
  PublicKey.findProgramAddressSync([Buffer.from('mhm-mint')], PROGRAM_ID)[0]
export const monsterMintPda = (id: BN | number) =>
  PublicKey.findProgramAddressSync([Buffer.from('monster-mint'), le8(id)], PROGRAM_ID)[0]
export const monsterPda = (mint: PublicKey) =>
  PublicKey.findProgramAddressSync([Buffer.from('monster'), mint.toBuffer()], PROGRAM_ID)[0]
export const battlePda = (id: BN | number) =>
  PublicKey.findProgramAddressSync([Buffer.from('battle'), le8(id)], PROGRAM_ID)[0]
export const matchPda = (id: BN | number) =>
  PublicKey.findProgramAddressSync([Buffer.from('match'), le8(id)], PROGRAM_ID)[0]
export const listingPda = (mint: PublicKey) =>
  PublicKey.findProgramAddressSync([Buffer.from('listing'), mint.toBuffer()], PROGRAM_ID)[0]
export const pendingPda = (id: BN | number) =>
  PublicKey.findProgramAddressSync([Buffer.from('pending'), le8(id)], PROGRAM_ID)[0]
export const metadataPda = (mint: PublicKey) =>
  PublicKey.findProgramAddressSync(
    [Buffer.from('metadata'), TOKEN_METADATA_PROGRAM_ID.toBuffer(), mint.toBuffer()],
    TOKEN_METADATA_PROGRAM_ID,
  )[0]

// ---------- program ----------

export function getProgram(connection: Connection, wallet: AnchorWallet): Program {
  // AnchorWallet lacks the (unused in-browser) `payer` field of Anchor's
  // NodeWallet type; the provider never touches it when signing via adapter.
  const provider = new AnchorProvider(connection, wallet as Wallet, { commitment: 'confirmed' })
  return new Program(idl as Idl, provider)
}

// Untyped-IDL account namespace: index by name instead of generated types.
interface AccountClient {
  fetchNullable(addr: PublicKey): Promise<unknown>
  all(): Promise<unknown>
}
const accounts = (program: Program) =>
  program.account as unknown as Record<string, AccountClient>

// ---------- reads ----------

export async function fetchConfig(program: Program): Promise<GameConfig | null> {
  return (await accounts(program).gameConfig.fetchNullable(configPda())) as GameConfig | null
}

export async function fetchAllMonsters(program: Program): Promise<Keyed<Monster>[]> {
  return (await accounts(program).monster.all()) as Keyed<Monster>[]
}

export async function fetchAllBattles(program: Program): Promise<Keyed<Battle>[]> {
  return (await accounts(program).battle.all()) as Keyed<Battle>[]
}

export async function fetchAllListings(program: Program): Promise<Keyed<Listing>[]> {
  return (await accounts(program).listing.all()) as Keyed<Listing>[]
}

export async function fetchAllMatches(program: Program): Promise<Keyed<GrudgeMatch>[]> {
  return (await accounts(program).grudgeMatch.all()) as Keyed<GrudgeMatch>[]
}

/** Mints (as base58 strings) of NFTs the wallet holds with amount == 1. */
export async function fetchHeldMints(connection: Connection, owner: PublicKey): Promise<Set<string>> {
  const resp = await connection.getParsedTokenAccountsByOwner(owner, {
    programId: TOKEN_PROGRAM_ID,
  })
  const held = new Set<string>()
  for (const { account } of resp.value) {
    const info = account.data.parsed.info
    if (info.tokenAmount?.amount === '1' && info.tokenAmount?.decimals === 0) {
      held.add(info.mint as string)
    }
  }
  return held
}

/** micro-MHM this monster has mined and not claimed, as of `nowSec`. */
export function pendingMicroMhm(monster: Monster, nowSec: number): BN {
  if (monster.inBattle) return monster.unclaimed
  const elapsed = Math.max(0, nowSec - monster.lastSettledTs.toNumber())
  return monster.unclaimed.add(monster.miningRate.muln(elapsed).divn(3600))
}

export function formatMhm(micro: BN | number): string {
  const v = new BN(micro).toNumber() / MICRO
  return v >= 100 ? v.toFixed(2) : v.toFixed(4)
}

// ---------- writes ----------

// Hatching is commit -> reveal: pay + seal to a future slot, then reveal the
// roll from that slot's hash (see programs/mhm-game/src/rng.rs). The high-level
// `hatch` / `buyMonster` helpers below run both steps and wait for the slot.

export async function commitHatchGenesis(
  program: Program,
  payer: PublicKey,
  config: GameConfig,
): Promise<number> {
  const id = config.monstersMinted.toNumber()
  const mint = monsterMintPda(id)
  await program.methods
    .commitHatchGenesis(new BN(id))
    .accounts({
      config: configPda(),
      monsterMint: mint,
      pending: pendingPda(id),
      payer,
      feeWallet: config.feeWallet,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      rent: SYSVAR_RENT_PUBKEY,
    })
    .rpc()
  return id
}

export async function commitBuyMonster(
  program: Program,
  payer: PublicKey,
  config: GameConfig,
): Promise<number> {
  const id = config.monstersMinted.toNumber()
  const mint = monsterMintPda(id)
  const mhmMint = mhmMintPda()
  await program.methods
    .commitBuyMonster(new BN(id))
    .accounts({
      config: configPda(),
      mhmMint,
      payerMhmAta: getAssociatedTokenAddressSync(mhmMint, payer),
      feeWallet: config.feeWallet,
      feeMhmAta: getAssociatedTokenAddressSync(mhmMint, config.feeWallet),
      monsterMint: mint,
      pending: pendingPda(id),
      payer,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      rent: SYSVAR_RENT_PUBKEY,
    })
    .rpc()
  return id
}

export async function revealMonster(program: Program, payer: PublicKey, monsterId: number) {
  const mint = monsterMintPda(monsterId)
  return program.methods
    .revealMonster()
    .accounts({
      config: configPda(),
      pending: pendingPda(monsterId),
      monsterMint: mint,
      monster: monsterPda(mint),
      monsterToken: getAssociatedTokenAddressSync(mint, payer),
      metadata: metadataPda(mint),
      slotHashes: SYSVAR_SLOT_HASHES_PUBKEY,
      payer,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      tokenMetadataProgram: TOKEN_METADATA_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      rent: SYSVAR_RENT_PUBKEY,
    })
    .rpc()
}

/** Reveal, retrying while the target slot has not been produced yet. */
async function revealWithRetry(program: Program, payer: PublicKey, monsterId: number) {
  for (let attempt = 0; attempt < 8; attempt++) {
    try {
      return await revealMonster(program, payer, monsterId)
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      if (msg.includes('RevealTooEarly') && attempt < 7) {
        await new Promise((r) => setTimeout(r, 800))
        continue
      }
      throw e
    }
  }
}

/** Full genesis hatch: commit (pay SOL), wait for the reveal slot, reveal. */
export async function hatchGenesis(program: Program, payer: PublicKey, config: GameConfig) {
  const id = await commitHatchGenesis(program, payer, config)
  await new Promise((r) => setTimeout(r, 1200))
  return revealWithRetry(program, payer, id)
}

/** Full MHM purchase: commit (pay MHM), wait for the reveal slot, reveal. */
export async function buyMonster(program: Program, payer: PublicKey, config: GameConfig) {
  const id = await commitBuyMonster(program, payer, config)
  await new Promise((r) => setTimeout(r, 1200))
  return revealWithRetry(program, payer, id)
}

export async function claimMining(program: Program, holder: PublicKey, monster: Keyed<Monster>) {
  const mhmMint = mhmMintPda()
  return program.methods
    .claimMining()
    .accounts({
      config: configPda(),
      monster: monster.publicKey,
      holderNftToken: getAssociatedTokenAddressSync(monster.account.mint, holder),
      mhmMint,
      holderMhmAta: getAssociatedTokenAddressSync(mhmMint, holder),
      holder,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    })
    .rpc()
}

export const MAX_LEVEL = 20
const RARITY_MULT = [1, 2, 4, 8, 16]

/** micro-MHM cost to go from `level` to `level+1` (mirror of on-chain calc). */
export function levelUpCost(config: GameConfig, rarityIdx: number, level: number): BN {
  if (level >= MAX_LEVEL) return new BN(0)
  return config.levelUpBaseCost.muln(RARITY_MULT[rarityIdx] ?? 1).muln(level)
}

export async function levelUp(
  program: Program,
  holder: PublicKey,
  config: GameConfig,
  monster: Keyed<Monster>,
) {
  const mhmMint = mhmMintPda()
  return program.methods
    .levelUp()
    .accounts({
      config: configPda(),
      monster: monster.publicKey,
      holderNftToken: getAssociatedTokenAddressSync(monster.account.mint, holder),
      mhmMint,
      holderMhmAta: getAssociatedTokenAddressSync(mhmMint, holder),
      feeWallet: config.feeWallet,
      feeMhmAta: getAssociatedTokenAddressSync(mhmMint, config.feeWallet),
      holder,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    })
    .rpc()
}

export async function createBattle(
  program: Program,
  creator: PublicKey,
  config: GameConfig,
  monster: Keyed<Monster>,
) {
  const id = config.battlesCreated
  return program.methods
    .createBattle(new BN(id))
    .accounts({
      config: configPda(),
      battle: battlePda(id),
      monster: monster.publicKey,
      creatorNftToken: getAssociatedTokenAddressSync(monster.account.mint, creator),
      creator,
      systemProgram: SystemProgram.programId,
    })
    .rpc()
}

export async function joinBattle(
  program: Program,
  joiner: PublicKey,
  battle: PublicKey,
  monster: Keyed<Monster>,
) {
  return program.methods
    .joinBattle()
    .accounts({
      battle,
      monster: monster.publicKey,
      joinerNftToken: getAssociatedTokenAddressSync(monster.account.mint, joiner),
      joiner,
    })
    .rpc()
}

export async function submitAction(
  program: Program,
  player: PublicKey,
  battle: PublicKey,
  consumable: number,
  support: number,
) {
  return program.methods
    .submitAction(consumable, support)
    .accounts({ battle, player })
    .rpc()
}

export async function claimTimeout(program: Program, player: PublicKey, battle: PublicKey) {
  return program.methods.claimTimeout().accounts({ battle, player }).rpc()
}

export async function cancelBattle(
  program: Program,
  creator: PublicKey,
  battle: Keyed<Battle>,
) {
  return program.methods
    .cancelBattle()
    .accounts({
      battle: battle.publicKey,
      monster: monsterPda(battle.account.monsters[0]),
      creator,
    })
    .rpc()
}

export async function listMonster(
  program: Program,
  seller: PublicKey,
  monster: Keyed<Monster>,
  priceMicro: BN,
) {
  const mint = monster.account.mint
  const listing = listingPda(mint)
  return program.methods
    .listMonster(priceMicro)
    .accounts({
      config: configPda(),
      monster: monster.publicKey,
      monsterMint: mint,
      sellerNftToken: getAssociatedTokenAddressSync(mint, seller),
      listing,
      escrowNftToken: getAssociatedTokenAddressSync(mint, listing, true),
      seller,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    })
    .rpc()
}

export async function cancelListing(program: Program, seller: PublicKey, listing: Keyed<Listing>) {
  const mint = listing.account.monsterMint
  return program.methods
    .cancelListing()
    .accounts({
      listing: listing.publicKey,
      monsterMint: mint,
      escrowNftToken: getAssociatedTokenAddressSync(mint, listing.publicKey, true),
      sellerNftToken: getAssociatedTokenAddressSync(mint, seller),
      seller,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    })
    .rpc()
}

export async function buyListing(
  program: Program,
  buyer: PublicKey,
  config: GameConfig,
  listing: Keyed<Listing>,
) {
  const mint = listing.account.monsterMint
  const mhmMint = mhmMintPda()
  return program.methods
    .buyListing()
    .accounts({
      config: configPda(),
      listing: listing.publicKey,
      monsterMint: mint,
      escrowNftToken: getAssociatedTokenAddressSync(mint, listing.publicKey, true),
      buyerNftToken: getAssociatedTokenAddressSync(mint, buyer),
      mhmMint,
      buyerMhmAta: getAssociatedTokenAddressSync(mhmMint, buyer),
      seller: listing.account.seller,
      sellerMhmAta: getAssociatedTokenAddressSync(mhmMint, listing.account.seller),
      feeWallet: config.feeWallet,
      feeMhmAta: getAssociatedTokenAddressSync(mhmMint, config.feeWallet),
      buyer,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    })
    .rpc()
}

export async function settleBattle(
  program: Program,
  payer: PublicKey,
  config: GameConfig,
  battle: Keyed<Battle>,
) {
  const mhmMint = mhmMintPda()
  const isDraw = battle.account.winner === 2
  const winnerWallet = isDraw ? null : battle.account.players[battle.account.winner]
  const winnerAta = winnerWallet ? getAssociatedTokenAddressSync(mhmMint, winnerWallet) : null
  const feeAta = getAssociatedTokenAddressSync(mhmMint, config.feeWallet)

  // Make sure destination token accounts exist before minting into them.
  const pre: TransactionInstruction[] = []
  if (winnerWallet && winnerAta) {
    pre.push(
      createAssociatedTokenAccountIdempotentInstruction(payer, winnerAta, winnerWallet, mhmMint),
      createAssociatedTokenAccountIdempotentInstruction(payer, feeAta, config.feeWallet, mhmMint),
    )
  }

  return program.methods
    .settleBattle()
    .accounts({
      config: configPda(),
      battle: battle.publicKey,
      monsterA: monsterPda(battle.account.monsters[0]),
      monsterB: monsterPda(battle.account.monsters[1]),
      mhmMint,
      winnerMhmAta: winnerAta as PublicKey | null as never,
      feeMhmAta: (winnerAta ? feeAta : null) as PublicKey | null as never,
      tokenProgram: TOKEN_PROGRAM_ID,
    })
    .preInstructions(pre)
    .rpc()
}

// ---------- NFT-staked Grudge Matches ----------

export async function createMatch(
  program: Program,
  creator: PublicKey,
  config: GameConfig,
  monster: Keyed<Monster>,
) {
  const id = config.battlesCreated
  const gm = matchPda(id)
  const mint = monster.account.mint
  return program.methods
    .createMatch(new BN(id))
    .accounts({
      config: configPda(),
      grudgeMatch: gm,
      monsterMint: mint,
      monster: monster.publicKey,
      creatorNftToken: getAssociatedTokenAddressSync(mint, creator),
      escrowNftToken: getAssociatedTokenAddressSync(mint, gm, true),
      creator,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    })
    .rpc()
}

export async function joinMatch(
  program: Program,
  joiner: PublicKey,
  gm: Keyed<GrudgeMatch>,
  monster: Keyed<Monster>,
) {
  const mint = monster.account.mint
  return program.methods
    .joinMatch()
    .accounts({
      grudgeMatch: gm.publicKey,
      monsterMint: mint,
      monster: monster.publicKey,
      joinerNftToken: getAssociatedTokenAddressSync(mint, joiner),
      escrowNftToken: getAssociatedTokenAddressSync(mint, gm.publicKey, true),
      joiner,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    })
    .rpc()
}

export async function submitMatchAction(
  program: Program,
  player: PublicKey,
  gm: PublicKey,
  consumable: number,
  support: number,
) {
  return program.methods
    .submitMatchAction(consumable, support)
    .accounts({ grudgeMatch: gm, player })
    .rpc()
}

export async function claimMatchTimeout(program: Program, player: PublicKey, gm: PublicKey) {
  return program.methods.claimMatchTimeout().accounts({ grudgeMatch: gm, player }).rpc()
}

export async function cancelMatch(program: Program, creator: PublicKey, gm: Keyed<GrudgeMatch>) {
  const mint = gm.account.monsters[0]
  return program.methods
    .cancelMatch()
    .accounts({
      grudgeMatch: gm.publicKey,
      monsterMint: mint,
      escrowNftToken: getAssociatedTokenAddressSync(mint, gm.publicKey, true),
      creatorNftToken: getAssociatedTokenAddressSync(mint, creator),
      creator,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    })
    .rpc()
}

export async function settleMatch(program: Program, payer: PublicKey, gm: Keyed<GrudgeMatch>) {
  const a = gm.account.monsters[0]
  const b = gm.account.monsters[1]
  const isDraw = gm.account.winner === 2
  const recA = isDraw ? gm.account.players[0] : gm.account.players[gm.account.winner]
  const recB = isDraw ? gm.account.players[1] : gm.account.players[gm.account.winner]
  const destA = getAssociatedTokenAddressSync(a, recA)
  const destB = getAssociatedTokenAddressSync(b, recB)
  const pre: TransactionInstruction[] = [
    createAssociatedTokenAccountIdempotentInstruction(payer, destA, recA, a),
    createAssociatedTokenAccountIdempotentInstruction(payer, destB, recB, b),
  ]
  return program.methods
    .settleMatch()
    .accounts({
      grudgeMatch: gm.publicKey,
      monsterMintA: a,
      monsterMintB: b,
      escrowA: getAssociatedTokenAddressSync(a, gm.publicKey, true),
      escrowB: getAssociatedTokenAddressSync(b, gm.publicKey, true),
      destA,
      destB,
      monsterA: monsterPda(a),
      monsterB: monsterPda(b),
      player0: gm.account.players[0],
      player1: gm.account.players[1],
      payer,
      tokenProgram: TOKEN_PROGRAM_ID,
    })
    .preInstructions(pre)
    .rpc()
}
