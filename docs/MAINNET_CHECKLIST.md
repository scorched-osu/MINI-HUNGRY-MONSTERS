# Path to Mainnet

Honest status of what stands between this branch and a mainnet launch, who
owns each item, and why. Items are ordered; do them top to bottom.

Legend: ✅ done · 🟡 in progress / partially done · 🔴 blocked on a human
decision or resource that code cannot supply.

## 1. Core game — ✅ done
- MHM SPL token, program-controlled mint authority.
- Monster NFTs (supply-1, mint authority revoked) with Metaplex metadata.
- Idle mining + claim.
- Turn-based battles (90s clock, CONSUMABLE/SUPPORT actions, winner-take-pot).
- On-chain marketplace (list / buy / cancel, escrowed).
- Fee routing to the configured fee wallet; admin launch allocations.
- Combat engine + trait-generation covered by unit tests (`cargo test -p mhm-game --lib`).

## 2. Randomness hardening — 🔴 needs a decision, then implementation + on-chain test
The mint's rarity/stat roll is a **pure function of a 32-byte seed**
(`programs/mhm-game/src/traits.rs`), and the seed is produced at a single
seam (`Roll` in `rng.rs`). Today that seed is clock/slot/minter-derived,
which a wrapper program can predict and grind (mint only when the roll is
rare, abort the atomic transaction otherwise). This is the top pre-mainnet
blocker. Two options — **pick one** (see the open question at the bottom):

- **A. Switchboard On-Demand VRF.** Gold standard. Adds a request/settle
  two-transaction mint and a dependency on Switchboard's on-chain program.
  Requires devnet Switchboard infra to test against.
- **B. On-chain commit–reveal.** Self-contained: `commit` takes payment and
  pins a future slot; `reveal` seeds the roll from that slot's `SlotHashes`
  entry (unknowable at commit time, fixed once the slot exists). No external
  dependency, but still a two-transaction mint.

Either way the change is localized to the seed seam + a two-phase mint flow;
`traits.rs` and its tests are unaffected. **Must be validated with
`anchor test` on a local validator** (not possible in the cloud build
environment — run locally per the README runbook).

## 3. Artwork & metadata hosting — 🔴 needs real assets
Current art is placeholder SVGs in `assets/` served from GitHub raw. For
mainnet:
- Commission/finalize real art per rarity tier (5 images, or per-monster).
- Upload art + metadata JSON to permanent storage (Arweave via Metaplex, or
  IPFS with a pinning service).
- Set `metadataBaseUri` in `scripts/config.json` to that permanent base URI
  (existing monsters update too — metadata is mutable, update authority is
  the program's config PDA).

## 4. Final tokenomics — 🔴 needs your numbers
Placeholders in `scripts/config.json` pending the mining-speed discussion:
- Per-rarity mining ranges (`miningRateRangesMhmPerHour`).
- `monsterPriceMhm`, `genesisPriceLamports`, `genesisRemaining`.
- `burnBps` (70%), `battleFeeBps` (2.5%), `marketFeeBps` (2%).
- Rarity odds (`rarityWeightsBps`).
All are tunable post-launch via `update_config`, but they should be
deliberate before genesis. See `docs/TOKENOMICS.md`.

## 5. Security audit — 🔴 external, required for a value-moving program
An internal review + hardening pass is done (`SECURITY.md`), but code that
mints a token and settles pots should get an independent audit
(e.g. OtterSec, Neodyme, Zellic) before mainnet. This is a human/vendor
engagement, not a code task.

## 6. Devnet dress rehearsal — 🟡 tooling ready, run locally
1. `anchor build && anchor keys sync && anchor build`
2. `anchor deploy --provider.cluster devnet`
3. `anchor idl build --program-name mhm_game -o app/src/idl/mhm_game.json`
4. `... npm run initialize` against devnet.
5. Exercise hatch → mine → claim → battle → settle → list → buy end to end
   (and `anchor test`, which runs the integration suite in `tests/`).
Do this until it's boring. See README "Local playtest".

## 7. Mainnet deploy — 🔴 your call, real funds, irreversible
Only after 2–6 are green:
1. Fund a mainnet deploy keypair with SOL (program deploy + rent; budget a
   few SOL). This keypair becomes the upgrade authority **and** the game
   admin (`initialize` is bound to the upgrade authority).
2. `anchor build && anchor deploy --provider.cluster mainnet`.
3. Regenerate + commit the IDL, set the app's `VITE_RPC_URL` to a mainnet RPC.
4. `npm run initialize` against mainnet with the final config.
5. Consider transferring the upgrade authority to a multisig (e.g. Squads)
   and/or making the program immutable once stable.

This step spends real money and is not reversible — it must be run by you
with a funded authority. Automated agents will not perform it.

---

### Open question for you
Randomness approach for step 2 — **Switchboard VRF (external, battle-tested)**
or **on-chain commit–reveal (self-contained, no dependency)**? This is the
one architectural fork gating the last big piece of engineering.
