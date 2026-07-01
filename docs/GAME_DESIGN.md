# MINI-HUNGRY-MONSTERS — Game Design

## Core loop

```
hatch monster ──> monster idles & mines MHM ──> claim MHM ──> buy more monsters
                        │                                          ▲
                        └──> stake pot in battle ──(win)── loot ───┘
                                       │
                                     (lose)──> pot gone
```

## Monsters

Each monster is an SPL NFT (supply 1) with an on-chain `Monster` account
(PDA seeded by the mint) holding its game state. Ownership = holding the
token, so monsters are freely tradable on any SPL marketplace.

Rolled at mint time (pseudo-random; VRF planned for mainnet):

- **Rarity** — weighted roll, default 60/25/10/4/1% for
  STANDARD / RARE / EPIC / LEGENDARY / UNIQUE.
- **Mining speed** — uniform roll inside the rarity's configured range.
- **Battle stats** — per-rarity base + random bonus:

| Rarity | Max HP | Power | Defense |
|---|---|---|---|
| Standard | 100–120 | 50–65 | 20–30 |
| Rare | 130–150 | 65–80 | 30–40 |
| Epic | 170–190 | 85–100 | 45–55 |
| Legendary | 220–240 | 110–125 | 65–75 |
| Unique | 300–320 | 150–165 | 90–100 |

## Idle mining

- Accrual: `mining_rate (micro-MHM/hr) × elapsed_seconds / 3600`, computed
  lazily — no cranks needed.
- `claim_mining` requires holding the NFT and mints the full accrued amount
  to the holder. Claiming is blocked while the monster is locked in battle.
- Mining pauses while a monster is in a battle (the pot is at stake instead).

## Battles

**High risk, high reward.** The moment a monster enters a battle its entire
unclaimed mining pot becomes the stake.

### Flow

1. `create_battle` — challenger locks monster + pot; challenge sits open.
2. `join_battle` — opponent locks their monster + pot; turn 1 starts,
   90-second action clock begins.
3. `submit_action(consumable, support)` — each player, every turn. The second
   submission auto-resolves the turn in the same transaction.
4. `claim_timeout` — if the clock expires: the side that submitted beats the
   side that didn't; if neither submitted, either player can end it as a draw.
5. `settle_battle` — permissionless payout crank:
   - Winner: loser's pot (minus `battle_fee_bps` rake to the fee wallet) is
     minted to the winner's wallet; winner's own pot returns to their monster.
   - Draw: both pots return to their monsters.
   - Both monsters unlock and resume mining.
6. `cancel_battle` — creator can withdraw an unaccepted challenge.

### Turn resolution order

1. DEF consumables apply their defense buff (so they defend *this* turn).
2. HP+ consumables heal (capped at max HP).
3. DPS consumables deal damage **simultaneously**.
4. Buff durations tick down.

Damage formula: `raw = POWER × magnitude% × support%`, then mitigation
`dealt = raw × 100 / (100 + DEF + def_buff)`, minimum 1.

### End conditions

- HP reaches 0 → other side wins; both at 0 same turn → draw.
- Turn 30 cap → higher remaining HP percentage wins; equal → draw.

### Action catalog

| id | Name | Slot | Type | Effect |
|---|---|---|---|---|
| 0 | Nibble | CONSUMABLE | DPS | 60% POWER damage |
| 1 | Chomp | CONSUMABLE | DPS | 100% POWER damage |
| 2 | Devour | CONSUMABLE | DPS | 150% POWER damage |
| 3 | Harden Shell | CONSUMABLE | DEF | +25 defense, 2 turns |
| 4 | Iron Belly | CONSUMABLE | DEF | +50 defense, 1 turn |
| 5 | Snack | CONSUMABLE | HP+ | heal 20% max HP |
| 6 | Feast | CONSUMABLE | HP+ | heal 40% max HP |
| 7 | PWR+ | SUPPORT | SUPP | +50% to a DPS consumable |
| 8 | GUARD+ | SUPPORT | SUPP | +50% to a DEF consumable |
| 9 | MEND+ | SUPPORT | SUPP | +50% to an HP+ consumable |

Supports only amplify a consumable of their matching type; a mismatched
pairing is wasted. The catalog is a program constant today; a future version
can move it to config or per-monster movesets/card inventories.

## Roadmap ideas

- Metaplex metadata + art per rarity tier.
- VRF randomness (Switchboard) for mint rolls.
- Marketplace escrow for monster-for-MHM listings (today: any SPL marketplace).
- More MHM sinks: consumable battle items, monster upgrades/evolution, name
  changes, breeding.
- Matchmaking with pot-size brackets so whales can't snipe tiny pots.
- Seasons/leaderboards keyed off the win/loss counters already on-chain.
