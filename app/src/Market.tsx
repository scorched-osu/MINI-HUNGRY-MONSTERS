import { useState } from 'react'
import type { PublicKey } from '@solana/web3.js'
import type { Program } from '@coral-xyz/anchor'
import { BN } from '@coral-xyz/anchor'
import { rarityName } from './catalog'
import {
  buyListing,
  cancelListing,
  formatMhm,
  listMonster,
  MICRO,
  type GameConfig,
  type Keyed,
  type Listing,
  type Monster,
} from './game'

type Run = (label: string, fn: () => Promise<unknown>) => Promise<void>

export function Market({
  program,
  me,
  config,
  listings,
  monsters,
  myMonsters,
  busy,
  run,
}: {
  program: Program
  me: PublicKey
  config: GameConfig
  listings: Keyed<Listing>[]
  monsters: Keyed<Monster>[]
  myMonsters: Keyed<Monster>[]
  busy: boolean
  run: Run
}) {
  const [sellMonster, setSellMonster] = useState('')
  const [priceMhm, setPriceMhm] = useState('')
  const sellable = myMonsters.filter((m) => !m.account.inBattle)
  const chosen = sellable.find((m) => m.publicKey.toBase58() === sellMonster)
  const price = Number(priceMhm)

  const byMint = (mint: PublicKey) => monsters.find((m) => m.account.mint.equals(mint))

  return (
    <section>
      <div className="card">
        <h2>🏷️ Sell a monster</h2>
        <p className="muted">
          Marketplace fee: {config.marketFeeBps / 100}% — the monster's unclaimed mining pot
          transfers to the buyer with it.
        </p>
        <div className="row">
          <select value={sellMonster} onChange={(e) => setSellMonster(e.target.value)}>
            <option value="">pick your monster</option>
            {sellable.map((m) => (
              <option key={m.publicKey.toBase58()} value={m.publicKey.toBase58()}>
                MHM #{m.account.id.toString()} ({rarityName(m.account.rarity)})
              </option>
            ))}
          </select>
          <input
            type="number"
            min="0"
            placeholder="price in MHM"
            value={priceMhm}
            onChange={(e) => setPriceMhm(e.target.value)}
          />
          <button
            disabled={busy || !chosen || !(price > 0)}
            onClick={() =>
              chosen &&
              run('Listing monster', () =>
                listMonster(program, me, chosen, new BN(Math.round(price * MICRO))),
              )
            }
          >
            List for sale
          </button>
        </div>
      </div>

      <div className="card">
        <h2>🛒 Listings</h2>
        {listings.length === 0 && <p>Nothing for sale right now.</p>}
        {listings.map((l) => {
          const m = byMint(l.account.monsterMint)
          const isMine = l.account.seller.equals(me)
          return (
            <div key={l.publicKey.toBase58()} className="row spread">
              <span>
                {m ? (
                  <>
                    MHM #{m.account.id.toString()} — {rarityName(m.account.rarity)} · ❤️
                    {m.account.maxHp} ⚔️{m.account.power} 🛡️{m.account.defense} · ⛏️
                    {formatMhm(m.account.miningRate)}/hr · pot {formatMhm(m.account.unclaimed)} MHM
                  </>
                ) : (
                  l.account.monsterMint.toBase58().slice(0, 8)
                )}
                {' — '}
                <b>{formatMhm(l.account.price)} MHM</b>
              </span>
              {isMine ? (
                <button
                  disabled={busy}
                  onClick={() => run('Delisting', () => cancelListing(program, me, l))}
                >
                  Delist
                </button>
              ) : (
                <button
                  disabled={busy}
                  onClick={() => run('Buying monster', () => buyListing(program, me, config, l))}
                >
                  Buy
                </button>
              )}
            </div>
          )
        })}
      </div>
    </section>
  )
}
