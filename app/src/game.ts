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

export interface Battle {
  id: BN
  state: Record<string, unknown>
  players: PublicKey[]
  monsters: PublicKey[]
  pots: BN[]
  hp: number[]
  maxHp: number[]
  power: number[]
  defense: number[]
  defBuff: number[]
  defBuffTurns: number[]
  pending: { submitted: boolean; consumable: number; support: number }[]
  turn: number
  deadline: BN
  winner: number
}

export interface Listing {
  seller: PublicKey
  monsterMint: PublicKey
  price: BN
  createdTs: BN
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
export const listingPda = (mint: PublicKey) =>
  PublicKey.findProgramAddressSync([Buffer.from('listing'), mint.toBuffer()], PROGRAM_ID)[0]
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

export async function hatchGenesis(program: Program, payer: PublicKey, config: GameConfig) {
  const id = config.monstersMinted
  const mint = monsterMintPda(id)
  return program.methods
    .hatchGenesis(new BN(id))
    .accounts({
      config: configPda(),
      monsterMint: mint,
      monster: monsterPda(mint),
      monsterToken: getAssociatedTokenAddressSync(mint, payer),
      metadata: metadataPda(mint),
      payer,
      feeWallet: config.feeWallet,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      tokenMetadataProgram: TOKEN_METADATA_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      rent: SYSVAR_RENT_PUBKEY,
    })
    .rpc()
}

export async function buyMonster(program: Program, payer: PublicKey, config: GameConfig) {
  const id = config.monstersMinted
  const mint = monsterMintPda(id)
  const mhmMint = mhmMintPda()
  return program.methods
    .buyMonster(new BN(id))
    .accounts({
      config: configPda(),
      mhmMint,
      payerMhmAta: getAssociatedTokenAddressSync(mhmMint, payer),
      feeWallet: config.feeWallet,
      feeMhmAta: getAssociatedTokenAddressSync(mhmMint, config.feeWallet),
      monsterMint: mint,
      monster: monsterPda(mint),
      monsterToken: getAssociatedTokenAddressSync(mint, payer),
      metadata: metadataPda(mint),
      payer,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      tokenMetadataProgram: TOKEN_METADATA_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      rent: SYSVAR_RENT_PUBKEY,
    })
    .rpc()
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
