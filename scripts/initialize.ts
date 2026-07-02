/**
 * One-time game initialization + initial MHM allocations.
 *
 * Usage:
 *   1. cp scripts/config.example.json scripts/config.json
 *   2. Fill in feeWallet and initialMhmHolders with YOUR wallet addresses.
 *   3. ANCHOR_PROVIDER_URL=<rpc> ANCHOR_WALLET=<admin keypair> npm run initialize
 *
 * The ANCHOR_WALLET keypair becomes the game admin: it can tune the economy
 * (update_config) and mint further MHM allocations (admin_mint_mhm).
 */
import * as anchor from "@coral-xyz/anchor";
import { BN, Program } from "@coral-xyz/anchor";
import { PublicKey, SystemProgram, SYSVAR_RENT_PUBKEY } from "@solana/web3.js";
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import * as fs from "fs";
import * as path from "path";
import { MhmGame } from "../target/types/mhm_game";

const MICRO = 1_000_000; // 1 MHM = 1_000_000 micro-MHM (6 decimals)

function loadConfig() {
  const p = path.join(__dirname, "config.json");
  if (!fs.existsSync(p)) {
    throw new Error(
      "scripts/config.json not found. Copy scripts/config.example.json and fill in your wallet addresses."
    );
  }
  return JSON.parse(fs.readFileSync(p, "utf-8"));
}

function rateRange(range: [number, number]): [BN, BN] {
  // whole MHM/hour -> micro-MHM/hour
  return [new BN(Math.round(range[0] * MICRO)), new BN(Math.round(range[1] * MICRO))];
}

async function main() {
  const cfg = loadConfig();
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.MhmGame as Program<MhmGame>;

  const feeWallet = new PublicKey(cfg.feeWallet);
  const [configPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("config")],
    program.programId
  );
  const BPF_LOADER_UPGRADEABLE = new PublicKey(
    "BPFLoaderUpgradeab1e11111111111111111111111"
  );
  const [programData] = PublicKey.findProgramAddressSync(
    [program.programId.toBuffer()],
    BPF_LOADER_UPGRADEABLE
  );
  const [mhmMint] = PublicKey.findProgramAddressSync(
    [Buffer.from("mhm-mint")],
    program.programId
  );

  const r = cfg.miningRateRangesMhmPerHour;
  console.log(`Initializing MINI-HUNGRY-MONSTERS as admin ${provider.wallet.publicKey}`);
  console.log(`  fee wallet: ${feeWallet.toBase58()}`);

  await program.methods
    .initialize({
      feeWallet,
      burnBps: cfg.burnBps,
      battleFeeBps: cfg.battleFeeBps,
      marketFeeBps: cfg.marketFeeBps,
      metadataBaseUri: cfg.metadataBaseUri,
      monsterPriceMhm: new BN(cfg.monsterPriceMhm),
      genesisPriceLamports: new BN(cfg.genesisPriceLamports),
      genesisRemaining: cfg.genesisRemaining,
      miningRateRanges: [
        rateRange(r.standard),
        rateRange(r.rare),
        rateRange(r.epic),
        rateRange(r.legendary),
        rateRange(r.unique),
      ],
      rarityWeightsBps: [
        cfg.rarityWeightsBps.standard,
        cfg.rarityWeightsBps.rare,
        cfg.rarityWeightsBps.epic,
        cfg.rarityWeightsBps.legendary,
        cfg.rarityWeightsBps.unique,
      ],
    })
    .accounts({
      config: configPda,
      mhmMint,
      admin: provider.wallet.publicKey,
      program: program.programId,
      programData,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      rent: SYSVAR_RENT_PUBKEY,
    })
    .rpc();
  console.log(`  config: ${configPda.toBase58()}`);
  console.log(`  MHM mint: ${mhmMint.toBase58()}`);

  for (const holder of cfg.initialMhmHolders ?? []) {
    if (!holder.address || holder.address.startsWith("REPLACE")) continue;
    const recipient = new PublicKey(holder.address);
    const ata = getAssociatedTokenAddressSync(mhmMint, recipient);
    const amount = new BN(holder.amountMhm).mul(new BN(MICRO));
    await program.methods
      .adminMintMhm(amount)
      .accounts({
        config: configPda,
        mhmMint,
        recipient,
        recipientMhmAta: ata,
        admin: provider.wallet.publicKey,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      })
      .rpc();
    console.log(`  minted ${holder.amountMhm} MHM to holder ${recipient.toBase58()}`);
  }

  console.log("Done. Mining speeds and prices can be tuned later with update_config.");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
