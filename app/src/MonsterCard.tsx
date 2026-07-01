import { useEffect, useState } from 'react'
import { rarityName, RARITY_NAMES } from './catalog'
import { formatMhm, pendingMicroMhm, type Keyed, type Monster } from './game'

const EMOJI = ['👾', '🐙', '🐲', '🦖', '👹']

export function MonsterCard({
  monster,
  busy,
  onClaim,
  onChallenge,
}: {
  monster: Keyed<Monster>
  busy: boolean
  onClaim: () => void
  onChallenge?: () => void
}) {
  const m = monster.account
  const rarity = rarityName(m.rarity)
  const rarityIdx = Math.max(0, RARITY_NAMES.indexOf(rarity as (typeof RARITY_NAMES)[number]))

  // Live-ticking pending MHM counter.
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))
  useEffect(() => {
    const t = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1000)
    return () => clearInterval(t)
  }, [])
  const pending = pendingMicroMhm(m, now)

  return (
    <div className={`card monster rar-border${rarityIdx}`}>
      <div className="monster-head">
        <span className="monster-emoji">{EMOJI[rarityIdx]}</span>
        <div>
          <h3>MHM #{m.id.toString()}</h3>
          <span className={`rar rar${rarityIdx}`}>{rarity}</span>
        </div>
      </div>
      <div className="stats">
        <span>❤️ {m.maxHp}</span>
        <span>⚔️ {m.power}</span>
        <span>🛡️ {m.defense}</span>
        <span>
          🏆 {m.wins}–{m.losses}
        </span>
      </div>
      <div className="mining">
        <div className="rate">⛏️ {formatMhm(m.miningRate)} MHM/hr</div>
        <div className="pending">
          {m.inBattle ? '⚔️ pot locked in battle' : `${formatMhm(pending)} MHM unclaimed`}
        </div>
      </div>
      <div className="row">
        <button disabled={busy || m.inBattle || pending.isZero()} onClick={onClaim}>
          Claim
        </button>
        {onChallenge && (
          <button className="danger" disabled={busy || m.inBattle} onClick={onChallenge}>
            Open battle (stakes pot!)
          </button>
        )}
      </div>
    </div>
  )
}
