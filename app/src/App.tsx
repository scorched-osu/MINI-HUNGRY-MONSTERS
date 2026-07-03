import { useCallback, useEffect, useMemo, useState } from 'react'
import { useAnchorWallet, useConnection, useWallet } from '@solana/wallet-adapter-react'
import { WalletMultiButton } from '@solana/wallet-adapter-react-ui'
import { BN } from '@coral-xyz/anchor'
import { getAccount } from '@solana/spl-token'
import { getAssociatedTokenAddressSync } from '@solana/spl-token'
import {
  claimMining,
  createBattle,
  levelUp,
  levelUpCost,
  MAX_LEVEL,
  fetchAllBattles,
  fetchAllListings,
  fetchAllMatches,
  fetchAllMonsters,
  fetchConfig,
  fetchHeldMints,
  formatMhm,
  getProgram,
  hatchGenesis,
  buyMonster,
  mhmMintPda,
  type Battle,
  type GameConfig,
  type GrudgeMatch,
  type Keyed,
  type Listing,
  type Monster,
} from './game'
import { MonsterCard } from './MonsterCard'
import { Arena } from './Arena'
import { Market } from './Market'
import { Grudge } from './Grudge'
import { RARITY_NAMES, rarityName } from './catalog'

type Tab = 'monsters' | 'hatchery' | 'arena' | 'market' | 'grudge'

export default function App() {
  const { connection } = useConnection()
  const wallet = useWallet()
  const anchorWallet = useAnchorWallet()

  const program = useMemo(
    () => (anchorWallet ? getProgram(connection, anchorWallet) : null),
    [connection, anchorWallet],
  )

  const [tab, setTab] = useState<Tab>('monsters')
  const [config, setConfig] = useState<GameConfig | null>(null)
  const [monsters, setMonsters] = useState<Keyed<Monster>[]>([])
  const [held, setHeld] = useState<Set<string>>(new Set())
  const [battles, setBattles] = useState<Keyed<Battle>[]>([])
  const [listings, setListings] = useState<Keyed<Listing>[]>([])
  const [matches, setMatches] = useState<Keyed<GrudgeMatch>[]>([])
  const [mhmBalance, setMhmBalance] = useState<BN>(new BN(0))
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState<string>('')

  const refresh = useCallback(async () => {
    if (!program || !wallet.publicKey) return
    try {
      const [cfg, all, mine, fights, sales, grudges] = await Promise.all([
        fetchConfig(program),
        fetchAllMonsters(program),
        fetchHeldMints(connection, wallet.publicKey),
        fetchAllBattles(program),
        fetchAllListings(program),
        fetchAllMatches(program),
      ])
      setConfig(cfg)
      setMonsters(all)
      setHeld(mine)
      setBattles(fights)
      setListings(sales)
      setMatches(grudges)
      try {
        const ata = getAssociatedTokenAddressSync(mhmMintPda(), wallet.publicKey)
        const acc = await getAccount(connection, ata)
        setMhmBalance(new BN(acc.amount.toString()))
      } catch {
        setMhmBalance(new BN(0))
      }
    } catch (e) {
      console.error(e)
    }
  }, [program, connection, wallet.publicKey])

  useEffect(() => {
    refresh()
    const t = setInterval(refresh, 8000)
    return () => clearInterval(t)
  }, [refresh])

  const run = useCallback(
    async (label: string, fn: () => Promise<unknown>) => {
      setBusy(true)
      setStatus(`${label}…`)
      try {
        await fn()
        setStatus(`${label} ✓`)
        await refresh()
      } catch (e) {
        console.error(e)
        setStatus(`${label} failed: ${e instanceof Error ? e.message : String(e)}`)
      } finally {
        setBusy(false)
      }
    },
    [refresh],
  )

  const myMonsters = monsters.filter((m) => held.has(m.account.mint.toBase58()))

  if (!wallet.connected) {
    return (
      <div className="shell">
        <header className="topbar">
          <h1>MINI‑HUNGRY‑MONSTERS</h1>
          <WalletMultiButton />
        </header>
        <div className="hero">
          <p className="big">👾 Your monsters mine MHM while you sleep.</p>
          <p>Hatch them. Trade them. Or bet their whole mining pot in battle.</p>
          <WalletMultiButton />
        </div>
      </div>
    )
  }

  return (
    <div className="shell">
      <header className="topbar">
        <h1>MINI‑HUNGRY‑MONSTERS</h1>
        <div className="walletrow">
          <span className="pill mhm">🪙 {formatMhm(mhmBalance)} MHM</span>
          <WalletMultiButton />
        </div>
      </header>

      <nav className="tabs">
        <button className={tab === 'monsters' ? 'on' : ''} onClick={() => setTab('monsters')}>
          My Monsters ({myMonsters.length})
        </button>
        <button className={tab === 'hatchery' ? 'on' : ''} onClick={() => setTab('hatchery')}>
          Hatchery
        </button>
        <button className={tab === 'arena' ? 'on' : ''} onClick={() => setTab('arena')}>
          Arena ({battles.filter((b) => 'open' in b.account.state).length} open)
        </button>
        <button className={tab === 'market' ? 'on' : ''} onClick={() => setTab('market')}>
          Market ({listings.length})
        </button>
        <button className={tab === 'grudge' ? 'on' : ''} onClick={() => setTab('grudge')}>
          🩸 Grudge ({matches.filter((m) => 'open' in m.account.state).length})
        </button>
      </nav>

      {status && <div className="status">{status}</div>}
      {!config && <div className="card">Game not initialized on this cluster yet.</div>}

      {tab === 'monsters' && (
        <section className="grid">
          {myMonsters.length === 0 && (
            <div className="card">No monsters yet — visit the Hatchery!</div>
          )}
          {myMonsters.map((m) => {
            const rarityIdx = Math.max(0, RARITY_NAMES.indexOf(rarityName(m.account.rarity) as (typeof RARITY_NAMES)[number]))
            const atMax = m.account.level >= MAX_LEVEL
            return (
              <MonsterCard
                key={m.publicKey.toBase58()}
                monster={m}
                busy={busy}
                atMaxLevel={atMax}
                levelCost={config ? formatMhm(levelUpCost(config, rarityIdx, m.account.level)) : undefined}
                onClaim={() => run('Claiming MHM', () => claimMining(program!, wallet.publicKey!, m))}
                onLevelUp={
                  config && !atMax
                    ? () => run('Leveling up', () => levelUp(program!, wallet.publicKey!, config, m))
                    : undefined
                }
                onChallenge={
                  config
                    ? () => run('Creating battle', () => createBattle(program!, wallet.publicKey!, config, m))
                    : undefined
                }
              />
            )
          })}
        </section>
      )}

      {tab === 'hatchery' && config && (
        <section className="hatchery">
          <div className="card odds" style={{ gridColumn: '1 / -1' }}>
            <p className="muted">
              🎲 Hatching is a two-step commit → reveal: your monster's rarity is
              sealed to a future block and revealed a moment later, so it can't be
              predicted or grinded. Approve both transactions; the wait is brief.
            </p>
          </div>
          <div className="card">
            <h2>🥚 Genesis Hatch — pay SOL</h2>
            <p>
              {(config.genesisPriceLamports.toNumber() / 1e9).toFixed(3)} SOL ·{' '}
              {config.genesisRemaining} left
            </p>
            <button
              disabled={busy || config.genesisRemaining === 0}
              onClick={() => run('Hatching', () => hatchGenesis(program!, wallet.publicKey!, config))}
            >
              Hatch with SOL
            </button>
          </div>
          <div className="card">
            <h2>🪙 Buy with MHM</h2>
            <p>
              {formatMhm(config.monsterPriceMhm)} MHM ({config.burnBps / 100}% burned)
            </p>
            <button
              disabled={busy || mhmBalance.lt(config.monsterPriceMhm)}
              onClick={() => run('Buying', () => buyMonster(program!, wallet.publicKey!, config))}
            >
              Buy with MHM
            </button>
          </div>
          <div className="card odds">
            <h2>Rarity odds</h2>
            <ul>
              {['STANDARD', 'RARE', 'EPIC', 'LEGENDARY', 'UNIQUE'].map((r, i) => (
                <li key={r}>
                  <span className={`rar rar${i}`}>{r}</span> {config.rarityWeightsBps[i] / 100}% ·{' '}
                  {formatMhm(config.miningRateRanges[i][0])}–{formatMhm(config.miningRateRanges[i][1])} MHM/hr
                </li>
              ))}
            </ul>
          </div>
        </section>
      )}

      {tab === 'arena' && config && program && wallet.publicKey && (
        <Arena
          program={program}
          me={wallet.publicKey}
          config={config}
          battles={battles}
          myMonsters={myMonsters}
          busy={busy}
          run={run}
        />
      )}

      {tab === 'market' && config && program && wallet.publicKey && (
        <Market
          program={program}
          me={wallet.publicKey}
          config={config}
          listings={listings}
          monsters={monsters}
          myMonsters={myMonsters}
          busy={busy}
          run={run}
        />
      )}

      {tab === 'grudge' && config && program && wallet.publicKey && (
        <Grudge
          program={program}
          me={wallet.publicKey}
          config={config}
          matches={matches}
          myMonsters={myMonsters}
          busy={busy}
          run={run}
        />
      )}
    </div>
  )
}
