// UI mirror of the on-chain action catalog (programs/mhm-game/src/actions.rs).

export const NO_SUPPORT = 255

export type ActionType = 'DPS' | 'DEF' | 'HP+' | 'SUPP'

export interface UiAction {
  id: number
  name: string
  slot: 'CONSUMABLE' | 'SUPPORT'
  type: ActionType
  blurb: string
}

export const ACTIONS: UiAction[] = [
  { id: 0, name: 'Nibble', slot: 'CONSUMABLE', type: 'DPS', blurb: '60% POWER damage' },
  { id: 1, name: 'Chomp', slot: 'CONSUMABLE', type: 'DPS', blurb: '100% POWER damage' },
  { id: 2, name: 'Devour', slot: 'CONSUMABLE', type: 'DPS', blurb: '150% POWER damage' },
  { id: 3, name: 'Harden Shell', slot: 'CONSUMABLE', type: 'DEF', blurb: '+25 DEF for 2 turns' },
  { id: 4, name: 'Iron Belly', slot: 'CONSUMABLE', type: 'DEF', blurb: '+50 DEF for 1 turn' },
  { id: 5, name: 'Snack', slot: 'CONSUMABLE', type: 'HP+', blurb: 'Heal 20% max HP' },
  { id: 6, name: 'Feast', slot: 'CONSUMABLE', type: 'HP+', blurb: 'Heal 40% max HP' },
  { id: 7, name: 'PWR+', slot: 'SUPPORT', type: 'SUPP', blurb: '+50% to a DPS action' },
  { id: 8, name: 'GUARD+', slot: 'SUPPORT', type: 'SUPP', blurb: '+50% to a DEF action' },
  { id: 9, name: 'MEND+', slot: 'SUPPORT', type: 'SUPP', blurb: '+50% to an HP+ action' },
]

export const CONSUMABLES = ACTIONS.filter((a) => a.slot === 'CONSUMABLE')
export const SUPPORTS = ACTIONS.filter((a) => a.slot === 'SUPPORT')

export const RARITY_NAMES = ['STANDARD', 'RARE', 'EPIC', 'LEGENDARY', 'UNIQUE'] as const

export function rarityName(rarity: Record<string, unknown>): string {
  const key = Object.keys(rarity)[0] ?? 'standard'
  return key.toUpperCase()
}
