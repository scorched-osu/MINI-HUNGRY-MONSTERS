import { useEffect, useState } from 'react'
import type { PublicKey } from '@solana/web3.js'
import type { Program } from '@coral-xyz/anchor'
import { rarityName } from './catalog'
import { CONSUMABLES, SUPPORTS, NO_SUPPORT } from './catalog'
import {
  cancelMatch,
  claimMatchTimeout,
  createMatch,
  joinMatch,
  settleMatch,
  submitMatchAction,
  type GameConfig,
  type GrudgeMatch,
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

// Best-of-5 pip row.
function Pips({ wins }: { wins: number }) {
  return (
    <span>
      {[0, 1, 2].map((i) => (
        <span key={i}>{i < wins ? '🟢' : '⚪'}</span>
      ))}
    </span>
  )
}

function MatchRoom({
  program,
  me,
  match,
  run,
  busy,
}: {
  program: Program
  me: PublicKey
  match: Keyed<GrudgeMatch>
  run: Run
  busy: boolean
}) {
  const m = match.account
  const board = m.board
  const now = useNow()
  const mySide = m.players.findIndex((p) => p.equals(me))
  const opp = mySide === 0 ? 1 : 0
  const secondsLeft = Math.max(0, board.deadline.toNumber() - now)
  const iSubmitted = mySide >= 0 && board.pending[mySide].submitted
  const oppSubmitted = mySide >= 0 && board.pending[opp].submitted

  const [consumable, setConsumable] = useState<number>(1)
  const [support, setSupport] = useState<number>(NO_SUPPORT)

  return (
    <div className="card battleroom grudge">
      <h2>
        🩸 Grudge Match #{m.id.toString()} — best of 5 · game {m.gamesPlayed + 1}
      </h2>
      <div className="fighters">
        {[0, 1].map((side) => (
          <div key={side} className={`fighter ${side === mySide ? 'mine' : ''}`}>
            <h4>
              {side === mySide ? 'YOUR MONSTER' : 'OPPONENT'} · Lv{m.level[side]}{' '}
              <Pips wins={m.gameWins[side]} />
              {board.defBuff[side] > 0 && <span className="buff"> 🛡️+{board.defBuff[side]}</span>}
            </h4>
            <HpBar hp={board.hp[side]} maxHp={board.maxHp[side]} />
            <div className="stats">
              <span>⚔️ {board.power[side]}</span>
              <span>🛡️ {board.defense[side]}</span>
              <span>💨 {board.speed[side]}</span>
            </div>
            <div className="submitted">
              {board.pending[side].submitted ? '✅ action locked in' : '…choosing'}
            </div>
          </div>
        ))}
      </div>

      <div className={`clock ${secondsLeft <= 15 ? 'urgent' : ''}`}>⏱️ {secondsLeft}s</div>

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
            onClick={() =>
              run('Submitting action', () =>
                submitMatchAction(program, me, match.publicKey, consumable, support),
              )
            }
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
          onClick={() => run('Claiming timeout', () => claimMatchTimeout(program, me, match.publicKey))}
        >
          ⏰ Clock expired — resolve this game
        </button>
      )}
    </div>
  )
}

export function Grudge({
  program,
  me,
  config,
  matches,
  myMonsters,
  busy,
  run,
}: {
  program: Program
  me: PublicKey
  config: GameConfig
  matches: Keyed<GrudgeMatch>[]
  myMonsters: Keyed<Monster>[]
  busy: boolean
  run: Run
}) {
  const [joinWith, setJoinWith] = useState<string>('')
  const [challengeWith, setChallengeWith] = useState<string>('')

  const active = matches.filter(
    (m) => 'active' in m.account.state && m.account.players.some((p) => p.equals(me)),
  )
  const open = matches.filter((m) => 'open' in m.account.state)
  const finished = matches.filter((m) => 'finished' in m.account.state)
  // A monster must be in the wallet (not escrowed) and idle to stake.
  const stakeable = myMonsters.filter((m) => !m.account.inBattle)
  const chosenChallenge = stakeable.find((m) => m.publicKey.toBase58() === challengeWith)
  const chosenJoin = stakeable.find((m) => m.publicKey.toBase58() === joinWith)

  return (
    <section>
      <div className="card grudge-warn">
        <h2>🩸 Grudge Matches — winner takes the NFT</h2>
        <p className="muted">
          Both fighters escrow their monster NFT and play a best-of-5. Win and you
          walk away with your opponent's monster; lose and yours is gone forever.
          Level up and pick your matchups wisely.
        </p>
      </div>

      {active.map((m) => (
        <MatchRoom key={m.publicKey.toBase58()} program={program} me={me} match={m} run={run} busy={busy} />
      ))}

      {finished.length > 0 && (
        <div className="card">
          <h2>🏁 Awaiting payout</h2>
          {finished.map((m) => {
            const w = m.account.winner
            const iWon = (w === 0 || w === 1) && m.account.players[w].equals(me)
            const label =
              w === 2 ? 'Draw — monsters returned' : iWon ? 'YOU WON — claim both NFTs!' : 'Decided'
            return (
              <div key={m.publicKey.toBase58()} className="row spread">
                <span>
                  Match #{m.account.id.toString()} — {m.account.gameWins[0]}–{m.account.gameWins[1]} — {label}
                </span>
                <button disabled={busy} onClick={() => run('Settling match', () => settleMatch(program, me, m))}>
                  Settle
                </button>
              </div>
            )
          })}
        </div>
      )}

      <div className="card">
        <h2>⚔️ Issue a challenge</h2>
        <div className="row">
          <select value={challengeWith} onChange={(e) => setChallengeWith(e.target.value)}>
            <option value="">stake a monster…</option>
            {stakeable.map((m) => (
              <option key={m.publicKey.toBase58()} value={m.publicKey.toBase58()}>
                MHM #{m.account.id.toString()} · {rarityName(m.account.rarity)} · Lv{m.account.level}
              </option>
            ))}
          </select>
          <button
            className="danger"
            disabled={busy || !chosenChallenge}
            onClick={() =>
              chosenChallenge &&
              run('Creating match', () => createMatch(program, me, config, chosenChallenge))
            }
          >
            Stake NFT & open match
          </button>
        </div>
      </div>

      <div className="card">
        <h2>🥊 Open grudge matches</h2>
        {open.length === 0 && <p>None open. Issue a challenge above.</p>}
        {open.length > 0 && (
          <div className="row">
            <label>Join with:</label>
            <select value={joinWith} onChange={(e) => setJoinWith(e.target.value)}>
              <option value="">stake your monster</option>
              {stakeable.map((m) => (
                <option key={m.publicKey.toBase58()} value={m.publicKey.toBase58()}>
                  MHM #{m.account.id.toString()} · {rarityName(m.account.rarity)} · Lv{m.account.level}
                </option>
              ))}
            </select>
          </div>
        )}
        {open.map((m) => {
          const isMine = m.account.players[0].equals(me)
          return (
            <div key={m.publicKey.toBase58()} className="row spread">
              <span>
                Match #{m.account.id.toString()} — Lv{m.account.level[0]} ❤️{m.account.baseHp[0]} ⚔️
                {m.account.basePower[0]} 🛡️{m.account.baseDefense[0]}
              </span>
              {isMine ? (
                <button disabled={busy} onClick={() => run('Cancelling', () => cancelMatch(program, me, m))}>
                  Cancel
                </button>
              ) : (
                <button
                  className="danger"
                  disabled={busy || !chosenJoin}
                  onClick={() => chosenJoin && run('Joining match', () => joinMatch(program, me, m, chosenJoin))}
                >
                  Accept (stake your NFT!)
                </button>
              )}
            </div>
          )
        })}
      </div>
    </section>
  )
}
