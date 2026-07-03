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

Rolled at hatch time via a **commit–reveal** scheme (`commit_hatch_genesis` /
`commit_buy_monster` then `reveal_monster`): payment is taken at commit and the
roll is seeded from the hash of a slot a few slots in the future, so the
outcome can't be predicted at payment time or ground out by aborting an atomic
transaction. Traits:

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

## Hatching (commit–reveal)

Two steps, defeating rarity grinding:

1. **Commit** (`commit_hatch_genesis` for SOL / `commit_buy_monster` for MHM) —
   takes payment, reserves the monster id + mint, and records
   `target_slot = current_slot + 2`. No traits yet.
2. **Reveal** (`reveal_monster`) — once `target_slot` exists, seeds the roll
   from `keccak(slot_hash(target_slot), minter, monster_id)`, rolls rarity and
   stats, mints the NFT to the minter, attaches metadata, revokes the mint
   authority, and closes the pending record (rent back to the minter).

Because the seed slot's hash doesn't exist at commit time, the outcome is
unpredictable when you pay; because it's fixed once produced, a paid commit
can't be re-rolled (abandoning it just forfeits payment). The reveal seeds from
the first *produced* slot at-or-after `target_slot`, so a skipped target slot
(a few percent of Solana slots) still reveals from the next real slot instead
of forfeiting. Reveal must land within `REVEAL_WINDOW_SLOTS` (256, ≈1.5–2 min)
of the target — comfortably under the SlotHashes buffer so the seed slot can't
drift as entries age — else the hatch expires. See `docs/MAINNET_CHECKLIST.md`
for the note on upgrading to a VRF for the strongest
(leader-collusion-resistant) guarantee.

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

## Marketplace

Monsters are plain SPL NFTs, so they trade anywhere — but the built-in
marketplace prices them in MHM and escrows the NFT on-chain:

1. `list_monster(price)` — the NFT moves into a program escrow (a token
   account owned by the listing PDA). One live listing per monster. A monster
   locked in a battle cannot be listed, and a listed monster cannot battle
   (its owner no longer holds the token).
2. `buy_listing` — buyer pays the MHM price: `market_fee_bps` (default 2%) to
   the fee wallet, the rest to the seller. The NFT — along with the monster's
   unclaimed mining pot, which keeps accruing while listed — transfers to the
   buyer.
3. `cancel_listing` — seller reclaims the NFT and the escrow rent.

## Roadmap ideas

- Metaplex metadata + art per rarity tier.
- VRF randomness (Switchboard) for mint rolls.
- More MHM sinks: consumable battle items, monster upgrades/evolution, name
  changes, breeding.
- Matchmaking with pot-size brackets so whales can't snipe tiny pots.
- Seasons/leaderboards keyed off the win/loss counters already on-chain.
