# MINI-HUNGRY-MONSTERS 👾🍖

An idle NFT mining + turn-based battle game on Solana.

Your MINI-HUNGRY-MONSTER NFTs passively **mine MHM coin** (mini-hungry-monster
coin) just by sitting in your wallet. Spend MHM to hatch more monsters — or
put your monster's entire unclaimed mining pot on the line in a
**winner-take-all battle**.

## How it works

### 🪙 MHM coin
- SPL token with 6 decimals, minted only by the game program (the mint
  authority is a program PDA).
- Earned by holding monster NFTs (idle mining) and by winning battles.
- Spent on hatching new monsters (a configurable share of every purchase is
  **burned**, the rest goes to the fee wallet).

### 🥚 Monsters
Real SPL NFTs (supply 1, decimals 0) — trade them on any marketplace; whoever
holds the token owns the monster and its mining pot. Each monster rolls:

| Rarity | Odds (default) | Mining speed* |
|---|---|---|
| STANDARD | 60% | 0.1 – 1 MHM/hr |
| RARE | 25% | 1 – 3 MHM/hr |
| EPIC | 10% | 3 – 7 MHM/hr |
| LEGENDARY | 4% | 7 – 13 MHM/hr |
| UNIQUE | 1% | 13+ MHM/hr |

*Placeholder values — mining speeds, odds and prices live in an on-chain
config the admin can tune at any time without redeploying.

### ⛏️ Idle mining
Every monster accrues MHM continuously based on its rolled mining speed.
`claim_mining` mints everything accrued to the current NFT holder. Unclaimed
MHM stays banked on the monster… which is exactly what you wager in battle.

### ⚔️ Battles — HIGH RISK, high reward
- Creating/joining a battle locks your monster and stakes its **entire
  unclaimed mining pot**.
- Turn-based combat with a **90-second clock** per turn to submit actions.
  Miss the deadline while your opponent submitted? They can claim the win.
- Each turn you pick **one CONSUMABLE** action and optionally **one SUPPORT**:

| Slot | Type | Actions | Effect |
|---|---|---|---|
| CONSUMABLE | DPS | Nibble / Chomp / Devour | Deal damage (60/100/150% of POWER) |
| CONSUMABLE | DEF | Harden Shell / Iron Belly | +25 def for 2 turns / +50 def for 1 turn |
| CONSUMABLE | HP+ | Snack / Feast | Heal 20% / 40% of max HP |
| SUPPORT | SUPP | PWR+ / GUARD+ / MEND+ | +50% to a matching DPS / DEF / HP+ consumable |

A support only amplifies a consumable of its matching type — pairing MEND+
with an attack does nothing.

- **When you win, you receive the loser's entire mining pot** (minus a small
  configurable rake to the fee wallet), minted straight to your wallet.

## Repository layout

```
programs/mhm-game/     Anchor on-chain program (token, NFTs, mining, battles)
  src/state.rs         Accounts: GameConfig, Monster, Battle
  src/actions.rs       The action catalog (CONSUMABLE / SUPPORT slots)
  src/combat.rs        Turn resolution engine + unit tests
  src/rng.rs           Mint-roll randomness (swap for a VRF before mainnet!)
app/                   Web client (Vite + React + wallet adapter)
tests/                 Anchor integration tests (localnet)
scripts/               initialize.ts + config for fee wallet & MHM allocations
docs/                  Game design + tokenomics notes
```

## Getting started

Prereqs: Rust, [Solana CLI](https://docs.solanalabs.com/cli/install),
[Anchor 0.31.1](https://www.anchor-lang.com/docs/installation), Node 18+.

```bash
npm install
anchor build          # build the program + IDL
anchor test           # spins up a localnet and runs tests/mhm-game.ts
cargo test -p mhm-game --lib   # combat engine unit tests (no validator needed)
```

### Deploying & initializing

```bash
cp scripts/config.example.json scripts/config.json
# edit scripts/config.json: set feeWallet + initialMhmHolders to YOUR addresses
anchor deploy --provider.cluster devnet
ANCHOR_PROVIDER_URL=https://api.devnet.solana.com \
ANCHOR_WALLET=~/.config/solana/id.json \
npm run initialize
```

- `feeWallet` receives **all fees**: genesis SOL payments, the non-burned
  share of MHM monster purchases, and the battle rake.
- `initialMhmHolders` each receive their MHM allocation at launch via the
  admin-only `admin_mint_mhm` instruction.

### Playing (web client)

```bash
cd app
npm install
VITE_RPC_URL=https://api.devnet.solana.com npm run dev   # defaults to localnet
```

Connect Phantom or Solflare, hatch a monster, watch it mine, and hit the
Arena tab to battle. If you change the on-chain program, regenerate the IDL
the app uses: `anchor idl build --program-name mhm_game -o app/src/idl/mhm_game.json`.

## ⚠️ Before mainnet

- **Randomness**: mint rolls use clock/slot-derived pseudo-randomness — fine
  for devnet, grindable on mainnet. Swap `rng.rs` for Switchboard VRF.
- **Metadata**: monsters are plain SPL NFTs; add Metaplex Token Metadata for
  images/marketplace display.
- **Audit**: this program moves value and has not been audited.

See [docs/GAME_DESIGN.md](docs/GAME_DESIGN.md) and
[docs/TOKENOMICS.md](docs/TOKENOMICS.md) for the full design.
