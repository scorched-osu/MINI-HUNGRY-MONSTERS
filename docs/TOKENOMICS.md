# MHM Coin — Tokenomics (working draft)

MHM ("mini-hungry-monster coin") is an SPL token with **6 decimals**. The
only mint authority is the game's config PDA — supply can never be minted
outside the program's rules.

## Supply sources (faucets)

| Source | Mechanism |
|---|---|
| Idle mining | Monsters accrue MHM by rarity-rolled speed; holders claim it |
| Battle loot | Loser's staked pot is minted to the winner (minus rake) |
| Initial allocations | Admin-only `admin_mint_mhm` for designated holder wallets at launch |

## Supply sinks

| Sink | Mechanism |
|---|---|
| Monster purchases | `burn_bps` share of every MHM purchase is burned (default 70%) |

## Fee flows

All fees route to a single configurable **fee wallet** (set at initialization,
changeable by the admin):

| Fee | Default | Notes |
|---|---|---|
| Genesis hatch | SOL price per hatch | Bootstraps the economy before MHM circulates; capped supply |
| Monster purchase | 30% of MHM price (the non-burned share) | Transferred, not minted |
| Battle rake | 2.5% of battle loot | Minted to the fee wallet on settlement |
| Marketplace sale | 2% of the sale price | Transferred from the buyer's payment |

## Mining speeds — PLACEHOLDER

Final numbers are still to be discussed. Current defaults (whole MHM/hour):

| Tier | Range |
|---|---|
| STANDARD | 0.1 – 1 |
| RARE | 1 – 3 |
| EPIC | 3 – 7 |
| LEGENDARY | 7 – 13 |
| UNIQUE | 13 – 21 |

Every number on this page — ranges, odds, prices, burn %, rake — lives in the
on-chain `GameConfig` and can be retuned by the admin via `update_config`
without redeploying. Balance levers to watch:

- **Emission vs burn**: mining is a perpetual faucet; the monster-purchase
  burn is the main sink. If emission outpaces demand for new monsters, MHM
  inflates — tune ranges/prices so a STANDARD monster roughly pays itself
  back over a target horizon (e.g. 2–4 weeks).
- **Battle velocity**: battles don't create net supply (the pot was accrued
  anyway) but they concentrate it and burn a rake — more battling is
  deflationary and should be encouraged.
- **Genesis supply**: SOL-priced hatches are capped; once sold out, the only
  way in is MHM (bought from existing players or won in battle).
