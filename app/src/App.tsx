import { useCallback, useEffect, useMemo, useState } from 'react'
import { useAnchorWallet, useConnection, useWallet } from '@solana/wallet-adapter-react'
import { WalletMultiButton } from '@solana/wallet-adapter-react-ui'
import { BN } from '@coral-xyz/anchor'
import { getAccount } from '@solana/spl-token'
import { getAssociatedTokenAddressSync } from '@solana/spl-token'
import {
  claimMining,
  createBattle,
  fetchAllBattles,
  fetchAllListings,
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
  type Keyed,
  type Listing,
  type Monster,
} from './game'
import { MonsterCard } from './MonsterCard'
import { Arena } from './Arena'
import { Market } from './Market'

type Tab = 'monsters' | 'hatchery' | 'arena' | 'market'

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
  const [mhmBalance, setMhmBalance] = useState<BN>(new BN(0))
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState<string>('')

  const refresh = useCallback(async () => {
    if (!program || !wallet.publicKey) return
    try {
      const [cfg, all, mine, fights, sales] = await Promise.all([
        fetchConfig(program),
        fetchAllMonsters(program),
        fetchHeldMints(connection, wallet.publicKey),
        fetchAllBattles(program),
        fetchAllListings(program),
      ])
      setConfig(cfg)
      setMonsters(all)
      setHeld(mine)
      setBattles(fights)
      setListings(sales)
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
      </nav>

      {status && <div className="status">{status}</div>}
      {!config && <div className="card">Game not initialized on this cluster yet.</div>}

      {tab === 'monsters' && (
        <section className="grid">
          {myMonsters.length === 0 && (
            <div className="card">No monsters yet — visit the Hatchery!</div>
          )}
          {myMonsters.map((m) => (
            <MonsterCard
              key={m.publicKey.toBase58()}
              monster={m}
              busy={busy}
              onClaim={() => run('Claiming MHM', () => claimMining(program!, wallet.publicKey!, m))}
              onChallenge={
                config
                  ? () => run('Creating battle', () => createBattle(program!, wallet.publicKey!, config, m))
                  : undefined
              }
            />
          ))}
        </section>
      )}

      {tab === 'hatchery' && config && (
        <section className="hatchery">
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
    </div>
  )
}
