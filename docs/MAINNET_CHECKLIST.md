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

## 2. Randomness hardening — 🟡 commit–reveal implemented; needs on-chain test
**Done:** the grindable single-transaction mint is replaced with an on-chain
**commit–reveal** flow (`commit_hatch_genesis` / `commit_buy_monster` then
`reveal_monster`). Payment is taken at commit; the roll is seeded from the hash
of `commit_slot + 2` (via the SlotHashes sysvar), which does not exist at
payment time — so the outcome can't be predicted or ground out by aborting an
atomic transaction, and a paid commit can't be re-rolled. The rarity/stat
derivation is a pure function of the seed (`programs/mhm-game/src/traits.rs`)
with unit tests; the slot-hash parser and seed builder are unit-tested in
`rng.rs`.

**Still to do (yours):**
- **Validate with `anchor test`** on a local validator — the two-phase flow and
  SlotHashes read can only be exercised on-chain, which the cloud build
  environment can't do. The integration test in `tests/mhm-game.ts` already
  drives commit→reveal; run it locally per the README runbook.
- **Optional stronger guarantee:** a slot hash can in principle be biased by a
  colluding block leader. For a high-value launch, swap the reveal seed for a
  VRF (e.g. Switchboard On-Demand). This stays localized — only the seed fed to
  `traits::roll_traits` changes; `traits.rs` and its tests are unaffected.

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

### Decision taken
Randomness (step 2) uses **on-chain commit–reveal** — self-contained, no
external dependency, and it fully removes the free atomic-abort grinding
exploit. If you'd rather have the leader-collusion-resistant guarantee of
**Switchboard VRF**, say so and it's a localized swap at the reveal seed.
