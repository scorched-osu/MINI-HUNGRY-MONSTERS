import * as anchor from "@coral-xyz/anchor";
import { Program, BN } from "@coral-xyz/anchor";
import {
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
  Transaction,
} from "@solana/web3.js";
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountIdempotentInstruction,
  getAssociatedTokenAddressSync,
  getAccount,
} from "@solana/spl-token";
import { assert } from "chai";
import { MhmGame } from "../target/types/mhm_game";

// Action catalog ids (see programs/mhm-game/src/actions.rs)
const CHOMP = 1;
const DEVOUR = 2;
const HARDEN = 3;
const SNACK = 5;
const PWR_PLUS = 7;
const NO_SUPPORT = 255;

const TOKEN_METADATA_PROGRAM_ID = new PublicKey(
  "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s"
);
const metadataPda = (mint: PublicKey) =>
  PublicKey.findProgramAddressSync(
    [Buffer.from("metadata"), TOKEN_METADATA_PROGRAM_ID.toBuffer(), mint.toBuffer()],
    TOKEN_METADATA_PROGRAM_ID
  )[0];

describe("mini-hungry-monsters", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.MhmGame as Program<MhmGame>;
  const admin = provider.wallet as anchor.Wallet;

  const feeWallet = Keypair.generate();
  const playerA = Keypair.generate();
  const playerB = Keypair.generate();

  const [configPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("config")],
    program.programId
  );
  const [mhmMint] = PublicKey.findProgramAddressSync(
    [Buffer.from("mhm-mint")],
    program.programId
  );

  const monsterMintPda = (id: number) =>
    PublicKey.findProgramAddressSync(
      [Buffer.from("monster-mint"), new BN(id).toArrayLike(Buffer, "le", 8)],
      program.programId
    )[0];
  const monsterPda = (mint: PublicKey) =>
    PublicKey.findProgramAddressSync(
      [Buffer.from("monster"), mint.toBuffer()],
      program.programId
    )[0];
  const battlePda = (id: number) =>
    PublicKey.findProgramAddressSync(
      [Buffer.from("battle"), new BN(id).toArrayLike(Buffer, "le", 8)],
      program.programId
    )[0];

  // Mining rates are cranked absurdly high (micro-MHM/hour) so a few seconds
  // of test time accrues a meaningful pot.
  const RATES: Array<[BN, BN]> = [
    [new BN(3_600_000_000), new BN(7_200_000_000)], // Standard: 1000-2000 MHM/s
    [new BN(7_200_000_000), new BN(10_800_000_000)],
    [new BN(10_800_000_000), new BN(14_400_000_000)],
    [new BN(14_400_000_000), new BN(18_000_000_000)],
    [new BN(18_000_000_000), new BN(21_600_000_000)],
  ];

  const pendingPda = (id: number) =>
    PublicKey.findProgramAddressSync(
      [Buffer.from("pending"), new BN(id).toArrayLike(Buffer, "le", 8)],
      program.programId
    )[0];

  // Commit -> wait for the reveal slot -> reveal.
  async function hatch(payer: Keypair): Promise<{
    mint: PublicKey;
    monster: PublicKey;
    token: PublicKey;
  }> {
    const config = await program.account.gameConfig.fetch(configPda);
    const id = config.monstersMinted.toNumber();
    const mint = monsterMintPda(id);
    const monster = monsterPda(mint);
    const token = getAssociatedTokenAddressSync(mint, payer.publicKey);

    await program.methods
      .commitHatchGenesis(new BN(id))
      .accounts({
        config: configPda,
        monsterMint: mint,
        pending: pendingPda(id),
        payer: payer.publicKey,
        feeWallet: feeWallet.publicKey,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        rent: SYSVAR_RENT_PUBKEY,
      })
      .signers([payer])
      .rpc();

    // Give the target slot (commit_slot + REVEAL_DELAY_SLOTS) time to appear.
    for (let i = 0; i < 20; i++) {
      try {
        await program.methods
          .revealMonster()
          .accounts({
            config: configPda,
            pending: pendingPda(id),
            monsterMint: mint,
            monster,
            monsterToken: token,
            metadata: metadataPda(mint),
            slotHashes: anchor.web3.SYSVAR_SLOT_HASHES_PUBKEY,
            payer: payer.publicKey,
            systemProgram: SystemProgram.programId,
            tokenProgram: TOKEN_PROGRAM_ID,
            tokenMetadataProgram: TOKEN_METADATA_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
            rent: SYSVAR_RENT_PUBKEY,
          })
          .signers([payer])
          .rpc();
        return { mint, monster, token };
      } catch (e) {
        if (String(e).includes("RevealTooEarly")) {
          await new Promise((r) => setTimeout(r, 500));
          continue;
        }
        throw e;
      }
    }
    throw new Error("reveal never became available");
  }

  let monsterA: Awaited<ReturnType<typeof hatch>>;
  let monsterB: Awaited<ReturnType<typeof hatch>>;

  before(async () => {
    for (const kp of [playerA, playerB]) {
      const sig = await provider.connection.requestAirdrop(
        kp.publicKey,
        5 * LAMPORTS_PER_SOL
      );
      await provider.connection.confirmTransaction(sig);
    }
  });

  it("initializes the game and MHM mint", async () => {
    await program.methods
      .initialize({
        feeWallet: feeWallet.publicKey,
        burnBps: 7000, // 70% of monster purchases burned, 30% to fee wallet
        battleFeeBps: 250, // 2.5% rake on battle loot
        marketFeeBps: 200, // 2% marketplace fee
        metadataBaseUri:
          "https://raw.githubusercontent.com/scorched-osu/MINI-HUNGRY-MONSTERS/main/assets/metadata/",
        monsterPriceMhm: new BN(100_000_000), // 100 MHM
        genesisPriceLamports: new BN(0.1 * LAMPORTS_PER_SOL),
        genesisRemaining: 1000,
        miningRateRanges: RATES,
        rarityWeightsBps: [6000, 2500, 1000, 400, 100],
      })
      .accounts({
        config: configPda,
        mhmMint,
        admin: admin.publicKey,
        program: program.programId,
        programData: PublicKey.findProgramAddressSync(
          [program.programId.toBuffer()],
          new PublicKey("BPFLoaderUpgradeab1e11111111111111111111111")
        )[0],
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        rent: SYSVAR_RENT_PUBKEY,
      })
      .rpc();

    const config = await program.account.gameConfig.fetch(configPda);
    assert.equal(config.admin.toBase58(), admin.publicKey.toBase58());
    assert.equal(config.feeWallet.toBase58(), feeWallet.publicKey.toBase58());
  });

  it("hatches genesis monsters (SOL fee goes to the fee wallet)", async () => {
    const feeBefore = await provider.connection.getBalance(feeWallet.publicKey);
    monsterA = await hatch(playerA);
    monsterB = await hatch(playerB);
    const feeAfter = await provider.connection.getBalance(feeWallet.publicKey);
    assert.equal(feeAfter - feeBefore, 0.2 * LAMPORTS_PER_SOL);

    const a = await program.account.monster.fetch(monsterA.monster);
    assert.isAtLeast(a.maxHp, 100);
    assert.isAtLeast(a.miningRate.toNumber(), RATES[0][0].toNumber());

    // The NFT is a real SPL token in the player's wallet.
    const tokenAcc = await getAccount(provider.connection, monsterA.token);
    assert.equal(tokenAcc.amount, 1n);
  });

  it("mines MHM over time and lets the holder claim it", async () => {
    await new Promise((r) => setTimeout(r, 3000));
    const holderAta = getAssociatedTokenAddressSync(mhmMint, playerA.publicKey);
    await program.methods
      .claimMining()
      .accounts({
        config: configPda,
        monster: monsterA.monster,
        holderNftToken: monsterA.token,
        mhmMint,
        holderMhmAta: holderAta,
        holder: playerA.publicKey,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      })
      .signers([playerA])
      .rpc();
    const bal = await getAccount(provider.connection, holderAta);
    assert.isTrue(bal.amount > 0n, "holder should have claimed mined MHM");
  });

  it("runs a full high-risk battle: winner takes the loser's pot", async () => {
    // Let both monsters accrue a pot to stake.
    await new Promise((r) => setTimeout(r, 2000));

    const battle = battlePda(0);
    await program.methods
      .createBattle(new BN(0))
      .accounts({
        config: configPda,
        battle,
        monster: monsterA.monster,
        creatorNftToken: monsterA.token,
        creator: playerA.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([playerA])
      .rpc();

    await program.methods
      .joinBattle()
      .accounts({
        battle,
        monster: monsterB.monster,
        joinerNftToken: monsterB.token,
        joiner: playerB.publicKey,
      })
      .signers([playerB])
      .rpc();

    // Fight until someone drops. A picks aggressive turns, B mixes it up.
    let state = await program.account.battle.fetch(battle);
    let round = 0;
    while ("active" in state.state && round < 40) {
      const bMove = round % 3 === 0 ? [HARDEN, NO_SUPPORT] : round % 3 === 1 ? [SNACK, NO_SUPPORT] : [CHOMP, NO_SUPPORT];
      await program.methods
        .submitAction(DEVOUR, PWR_PLUS)
        .accounts({ battle, player: playerA.publicKey })
        .signers([playerA])
        .rpc();
      await program.methods
        .submitAction(bMove[0], bMove[1])
        .accounts({ battle, player: playerB.publicKey })
        .signers([playerB])
        .rpc();
      state = await program.account.battle.fetch(battle);
      round++;
    }
    assert.isTrue("finished" in state.state, "battle should finish");
    assert.notEqual(state.winner, 255);

    const winnerIsA = state.winner === 0;
    const winnerKey = winnerIsA ? playerA.publicKey : playerB.publicKey;
    const loserPot = state.pots[winnerIsA ? 1 : 0];
    const winnerAta = getAssociatedTokenAddressSync(mhmMint, winnerKey);
    const feeAta = getAssociatedTokenAddressSync(mhmMint, feeWallet.publicKey);

    // Make sure the winner + fee MHM token accounts exist before settling.
    await provider.sendAndConfirm(
      new Transaction().add(
        createAssociatedTokenAccountIdempotentInstruction(
          admin.publicKey,
          winnerAta,
          winnerKey,
          mhmMint
        ),
        createAssociatedTokenAccountIdempotentInstruction(
          admin.publicKey,
          feeAta,
          feeWallet.publicKey,
          mhmMint
        )
      )
    );

    // Draws pay nothing extra; only assert loot flow when there is a winner.
    if (state.winner === 0 || state.winner === 1) {
      const before = await getAccount(provider.connection, winnerAta).then(
        (a) => a.amount,
        () => 0n
      );
      await program.methods
        .settleBattle()
        .accounts({
          config: configPda,
          battle,
          monsterA: monsterA.monster,
          monsterB: monsterB.monster,
          mhmMint,
          winnerMhmAta: winnerAta,
          feeMhmAta: feeAta,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc();
      const after = (await getAccount(provider.connection, winnerAta)).amount;
      const rake = (BigInt(loserPot.toString()) * 250n) / 10000n;
      assert.equal(
        after - before,
        BigInt(loserPot.toString()) - rake,
        "winner should receive the loser's whole pot minus the rake"
      );
    }

    // Both monsters are unlocked and mining again.
    const a = await program.account.monster.fetch(monsterA.monster);
    const b = await program.account.monster.fetch(monsterB.monster);
    assert.isFalse(a.inBattle);
    assert.isFalse(b.inBattle);
  });

  it("lists a monster for MHM and sells it on the marketplace", async () => {
    const price = new BN(50_000_000); // 50 MHM
    const [listing] = PublicKey.findProgramAddressSync(
      [Buffer.from("listing"), monsterB.mint.toBuffer()],
      program.programId
    );
    const escrow = getAssociatedTokenAddressSync(monsterB.mint, listing, true);

    await program.methods
      .listMonster(price)
      .accounts({
        config: configPda,
        monster: monsterB.monster,
        monsterMint: monsterB.mint,
        sellerNftToken: monsterB.token,
        listing,
        escrowNftToken: escrow,
        seller: playerB.publicKey,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      })
      .signers([playerB])
      .rpc();

    // NFT is escrowed: seller no longer holds it.
    const sellerNft = await getAccount(provider.connection, monsterB.token);
    assert.equal(sellerNft.amount, 0n);

    const buyerNft = getAssociatedTokenAddressSync(monsterB.mint, playerA.publicKey);
    const sellerMhm = getAssociatedTokenAddressSync(mhmMint, playerB.publicKey);
    const sellerBefore = await getAccount(provider.connection, sellerMhm).then(
      (acc) => acc.amount,
      () => 0n
    );

    await program.methods
      .buyListing()
      .accounts({
        config: configPda,
        listing,
        monsterMint: monsterB.mint,
        escrowNftToken: escrow,
        buyerNftToken: buyerNft,
        mhmMint,
        buyerMhmAta: getAssociatedTokenAddressSync(mhmMint, playerA.publicKey),
        seller: playerB.publicKey,
        sellerMhmAta: sellerMhm,
        feeWallet: feeWallet.publicKey,
        feeMhmAta: getAssociatedTokenAddressSync(mhmMint, feeWallet.publicKey),
        buyer: playerA.publicKey,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      })
      .signers([playerA])
      .rpc();

    // Buyer holds the NFT; seller received the price minus the 2% fee.
    const bought = await getAccount(provider.connection, buyerNft);
    assert.equal(bought.amount, 1n);
    const sellerAfter = (await getAccount(provider.connection, sellerMhm)).amount;
    const fee = (BigInt(price.toString()) * 200n) / 10000n;
    assert.equal(sellerAfter - sellerBefore, BigInt(price.toString()) - fee);
  });
});
