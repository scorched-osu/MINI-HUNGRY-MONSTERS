# Security

MINI-HUNGRY-MONSTERS moves value on-chain (it mints the MHM token, escrows
NFTs, and settles winner-take-all battle pots), so this document records the
security model and the review done so far.

## Trust model

- **MHM mint** — the only mint authority is the `config` PDA. New MHM can be
  created **only** by program instructions: `claim_mining` (against accrued
  idle mining), `settle_battle` (loot + rake), and `admin_mint_mhm`
  (admin-only launch allocations). No other path can mint.
- **Monster NFTs** — supply-1 SPL mints; the mint authority is revoked
  immediately after the single token is minted, so supply is permanently 1.
  Ownership = holding the token, enforced everywhere via
  `amount == 1 && owner == signer` constraints.
- **Admin** — set once at `initialize` and bound to the program's upgrade
  authority (see below). Can tune economy config and mint launch allocations.
  Cannot touch player-held funds or NFTs.
- **Escrows / pots** — marketplace escrows are owned by the listing PDA;
  battle pots live on the `Battle` account. Both are released only by the
  program under its own PDA signature.

## Review status

An internal security pass traced every instruction handler's account
validation against its handler body and every token/lamport flow. Areas
explicitly checked and found sound:

- No double-settle / double-claim: state machines gate on prior state and
  reset it atomically before/after CPIs; balances are zeroed before minting.
- Settlement accounts can't be substituted (self-referential monster seeds +
  `mint == battle.monsters[i]` constraints; winner/fee ATAs owner-checked).
- A monster can't be simultaneously battling and listed.
- `u128 → u64` fee/rake/burn casts cannot overflow (bps ≤ 10 000), and
  `overflow-checks = true` is set for release builds.
- Marketplace escrow authority, seller, and fee-wallet accounts are all
  constrained by address/seeds.
- Web client uses no `dangerouslySetInnerHTML`/`eval`; client-side gating is
  cosmetic and re-enforced on-chain.

`initialize` is bound to the program's upgrade authority
(`program_data.upgrade_authority_address == admin`), closing the deploy-time
window where an attacker could otherwise call it first and seize admin.

The NFT-staked Grudge Match escrow/settle path was reviewed: escrow accounts
are owned by the match PDA and only released under its seeds; `settle_match`
validates each destination against the winner (or, on a draw, the original
owner) so a permissionless crank cannot redirect an NFT. A monster staked in a
match is locked with the same `in_battle` flag as a pot-battle (and its NFT is
escrowed), so it cannot be simultaneously entered into a pot-battle or listed.

## Randomness

Hatching uses **commit–reveal**: `commit_*` takes payment and pins
`target_slot = commit_slot + 2`; `reveal_monster` seeds the roll from
`keccak(slot_hash(target_slot), minter, monster_id)`. The target slot's hash
does not exist at payment time, so the outcome can't be predicted or ground
out by aborting an atomic transaction, and a paid commit can't be re-rolled
(abandoning it forfeits payment). The rarity/stat derivation is a pure,
unit-tested function of the seed.

## Known limitations before mainnet

- **Slot-hash randomness vs VRF** — a block leader can in principle bias the
  slot hash the reveal reads. For a high-value launch, swap the reveal seed
  for a VRF (e.g. Switchboard On-Demand); it's a localized change at the seed
  fed to `traits::roll_traits`.
- **On-chain integration testing** — the commit–reveal flow must be validated
  with `anchor test` on a validator (not runnable in the cloud build env).
- **No external audit** — this code has not been independently audited.
- **Metadata hosting** — artwork/metadata is placeholder content served from
  GitHub; move to permanent storage (Arweave/IPFS) for mainnet.

## Reporting

Found something? Open a private security advisory on the GitHub repository
rather than a public issue.
