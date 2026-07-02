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

## Known limitations before mainnet

- **Randomness** — mint rolls in `rng.rs` derive from clock/slot/minter and
  are deterministically computable, so a wrapper program can grind for rare
  rolls by aborting unfavorable atomic transactions. Replace with a VRF
  (e.g. Switchboard On-Demand) before mainnet. This is the single most
  important pre-mainnet change.
- **No external audit** — this code has not been independently audited.
- **Metadata hosting** — artwork/metadata is placeholder content served from
  GitHub; move to permanent storage (Arweave/IPFS) for mainnet.

## Reporting

Found something? Open a private security advisory on the GitHub repository
rather than a public issue.
