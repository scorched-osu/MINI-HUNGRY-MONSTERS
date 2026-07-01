import { useEffect, useState } from 'react'
import type { PublicKey } from '@solana/web3.js'
import type { Program } from '@coral-xyz/anchor'
import { CONSUMABLES, SUPPORTS, NO_SUPPORT } from './catalog'
import {
  cancelBattle,
  claimTimeout,
  formatMhm,
  joinBattle,
  settleBattle,
  submitAction,
  type Battle,
  type GameConfig,
  type Keyed,
  type Monster,
} from './game'

type Run = (label: string, fn: () => Promise<unknown>) => Promise<void>

function useNow() {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))
  useEffect(() => {
    const t = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1000)
    return () => clearInterval(t)
  }, [])
  return now
}

function HpBar({ hp, maxHp }: { hp: number; maxHp: number }) {
  const pct = Math.max(0, Math.min(100, (hp / maxHp) * 100))
  return (
    <div className="hpbar">
      <div className="hpfill" style={{ width: `${pct}%` }} />
      <span>
        {hp}/{maxHp}
      </span>
    </div>
  )
}

function BattleRoom({
  program,
  me,
  battle,
  run,
  busy,
}: {
  program: Program
  me: PublicKey
  battle: Keyed<Battle>
  run: Run
  busy: boolean
}) {
  const b = battle.account
  const now = useNow()
  const mySide = b.players.findIndex((p) => p.equals(me))
  const opp = mySide === 0 ? 1 : 0
  const secondsLeft = Math.max(0, b.deadline.toNumber() - now)
  const iSubmitted = mySide >= 0 && b.pending[mySide].submitted
  const oppSubmitted = mySide >= 0 && b.pending[opp].submitted

  const [consumable, setConsumable] = useState<number>(1)
  const [support, setSupport] = useState<number>(NO_SUPPORT)

  return (
    <div className="card battleroom">
      <h2>
        ⚔️ Battle #{b.id.toString()} — turn {b.turn}
      </h2>
      <div className="fighters">
        {[0, 1].map((side) => (
          <div key={side} className={`fighter ${side === mySide ? 'mine' : ''}`}>
            <h4>
              {side === mySide ? 'YOUR MONSTER' : 'OPPONENT'}{' '}
              {b.defBuff[side] > 0 && <span className="buff">🛡️+{b.defBuff[side]}</span>}
            </h4>
            <HpBar hp={b.hp[side]} maxHp={b.maxHp[side]} />
            <div className="stats">
              <span>⚔️ {b.power[side]}</span>
              <span>🛡️ {b.defense[side]}</span>
              <span>💰 {formatMhm(b.pots[side])} MHM at stake</span>
            </div>
            <div className="submitted">{b.pending[side].submitted ? '✅ action locked in' : '…choosing'}</div>
          </div>
        ))}
      </div>

      <div className={`clock ${secondsLeft <= 15 ? 'urgent' : ''}`}>
        ⏱️ {secondsLeft}s to submit
      </div>

      {mySide >= 0 && !iSubmitted && secondsLeft > 0 && (
        <div className="picker">
          <div>
            <h4>CONSUMABLE</h4>
            <div className="cards">
              {CONSUMABLES.map((a) => (
                <button
                  key={a.id}
                  className={`action ${a.type.replace('+', 'P')} ${consumable === a.id ? 'sel' : ''}`}
                  onClick={() => setConsumable(a.id)}
                >
                  <b>{a.name}</b>
                  <i>{a.type}</i>
                  <small>{a.blurb}</small>
                </button>
              ))}
            </div>
          </div>
          <div>
            <h4>SUPPORT (optional)</h4>
            <div className="cards">
              <button
                className={`action SUPP ${support === NO_SUPPORT ? 'sel' : ''}`}
                onClick={() => setSupport(NO_SUPPORT)}
              >
                <b>None</b>
                <small>no support</small>
              </button>
              {SUPPORTS.map((a) => (
                <button
                  key={a.id}
                  className={`action SUPP ${support === a.id ? 'sel' : ''}`}
                  onClick={() => setSupport(a.id)}
                >
                  <b>{a.name}</b>
                  <i>{a.type}</i>
                  <small>{a.blurb}</small>
                </button>
              ))}
            </div>
          </div>
          <button
            className="danger big"
            disabled={busy}
            onClick={() => run('Submitting action', () => submitAction(program, me, battle.publicKey, consumable, support))}
          >
            LOCK IN
          </button>
        </div>
      )}

      {mySide >= 0 && iSubmitted && !oppSubmitted && <p>Waiting for your opponent…</p>}

      {mySide >= 0 && secondsLeft === 0 && (iSubmitted || !oppSubmitted) && (
        <button
          className="danger"
          disabled={busy}
          onClick={() => run('Claiming timeout', () => claimTimeout(program, me, battle.publicKey))}
        >
          ⏰ Opponent timed out — claim {iSubmitted && !oppSubmitted ? 'the WIN' : 'a draw'}
        </button>
      )}
    </div>
  )
}

export function Arena({
  program,
  me,
  config,
  battles,
  myMonsters,
  busy,
  run,
}: {
  program: Program
  me: PublicKey
  config: GameConfig
  battles: Keyed<Battle>[]
  myMonsters: Keyed<Monster>[]
  busy: boolean
  run: Run
}) {
  const [joinWith, setJoinWith] = useState<string>('')

  const active = battles.filter(
    (b) => 'active' in b.account.state && b.account.players.some((p) => p.equals(me)),
  )
  const open = battles.filter((b) => 'open' in b.account.state)
  const finished = battles.filter((b) => 'finished' in b.account.state)
  const idleMine = myMonsters.filter((m) => !m.account.inBattle)

  return (
    <section>
      {active.map((b) => (
        <BattleRoom key={b.publicKey.toBase58()} program={program} me={me} battle={b} run={run} busy={busy} />
      ))}

      {finished.length > 0 && (
        <div className="card">
          <h2>🏁 Awaiting payout</h2>
          {finished.map((b) => {
            const winner = b.account.winner
            const label =
              winner === 2
                ? 'Draw — return both pots'
                : b.account.players[winner].equals(me)
                  ? `YOU WON ${formatMhm(b.account.pots[winner === 0 ? 1 : 0])} MHM — collect!`
                  : 'Battle decided'
            return (
              <div key={b.publicKey.toBase58()} className="row spread">
                <span>
                  Battle #{b.account.id.toString()} — {label}
                </span>
                <button
                  disabled={busy}
                  onClick={() => run('Settling battle', () => settleBattle(program, me, config, b))}
                >
                  Settle
                </button>
              </div>
            )
          })}
        </div>
      )}

      <div className="card">
        <h2>🥊 Open challenges</h2>
        {open.length === 0 && <p>No open battles. Create one from a monster card — if you dare.</p>}
        {open.length > 0 && (
          <div className="row">
            <label>Join with:</label>
            <select value={joinWith} onChange={(e) => setJoinWith(e.target.value)}>
              <option value="">pick your monster</option>
              {idleMine.map((m) => (
                <option key={m.publicKey.toBase58()} value={m.publicKey.toBase58()}>
                  MHM #{m.account.id.toString()} (❤️{m.account.maxHp} ⚔️{m.account.power})
                </option>
              ))}
            </select>
          </div>
        )}
        {open.map((b) => {
          const isMine = b.account.players[0].equals(me)
          const chosen = idleMine.find((m) => m.publicKey.toBase58() === joinWith)
          return (
            <div key={b.publicKey.toBase58()} className="row spread">
              <span>
                Battle #{b.account.id.toString()} — pot {formatMhm(b.account.pots[0])} MHM —{' '}
                ❤️{b.account.maxHp[0]} ⚔️{b.account.power[0]} 🛡️{b.account.defense[0]}
              </span>
              {isMine ? (
                <button disabled={busy} onClick={() => run('Cancelling', () => cancelBattle(program, me, b))}>
                  Cancel
                </button>
              ) : (
                <button
                  className="danger"
                  disabled={busy || !chosen}
                  onClick={() => chosen && run('Joining battle', () => joinBattle(program, me, b.publicKey, chosen))}
                >
                  Fight (stakes your pot!)
                </button>
              )}
            </div>
          )
        })}
      </div>
    </section>
  )
}
