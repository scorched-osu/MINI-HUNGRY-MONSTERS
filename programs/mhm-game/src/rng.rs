//! Randomness for monster rolls.
//!
//! The mint uses a **commit–reveal** scheme (see `PendingMint`): the roll seed
//! is derived from the hash of a slot that does not exist yet when payment is
//! taken, so the outcome cannot be predicted or ground out. `Roll::new`
//! (clock-derived) remains only for non-security-critical uses.
//!
//! SECURITY NOTE: for the strongest guarantee (protection even against a
//! colluding leader who could bias a slot hash), swap the commit–reveal seed
//! for a VRF (e.g. Switchboard On-Demand). The seed is consumed in exactly one
//! place — `traits::roll_traits` — so that swap stays localized.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::keccak;

/// Find the entry for the smallest produced slot `>= target_slot` in the raw
/// SlotHashes sysvar data, returning `(slot, hash)`.
///
/// Layout: `u64` entry count, then that many `(slot: u64, hash: [u8; 32])`
/// records ordered newest-slot-first. Using the first slot at-or-after the
/// target (rather than an exact match) tolerates *skipped* slots — a few
/// percent of Solana slots produce no block — so a paid commit whose exact
/// target slot was skipped can still reveal from the next real slot. The
/// caller bounds the reveal window so this chosen slot cannot drift as the
/// buffer ages, keeping the seed stable (and thus un-grindable).
///
/// Returns `None` if no such slot is present (either the target has not been
/// produced yet, or everything at-or-after it aged out of the buffer).
pub fn first_hash_at_or_after(data: &[u8], target_slot: u64) -> Option<(u64, [u8; 32])> {
    if data.len() < 8 {
        return None;
    }
    let count = u64::from_le_bytes(data[0..8].try_into().ok()?) as usize;
    let mut best: Option<(u64, [u8; 32])> = None;
    for i in 0..count {
        let base = 8 + i * 40;
        if base + 40 > data.len() {
            break;
        }
        let slot = u64::from_le_bytes(data[base..base + 8].try_into().ok()?);
        // Entries descend by slot; once below the target, nothing further can
        // qualify, and the last qualifier seen is the smallest slot >= target.
        if slot < target_slot {
            break;
        }
        let mut h = [0u8; 32];
        h.copy_from_slice(&data[base + 8..base + 40]);
        best = Some((slot, h));
    }
    best
}

/// Build the roll seed for a reveal: bind the slot hash to the specific minter
/// and monster id so two hatches revealing off the same slot still differ.
pub fn reveal_seed(slot_hash: &[u8; 32], minter: &Pubkey, monster_id: u64) -> [u8; 32] {
    keccak::hashv(&[slot_hash, minter.as_ref(), &monster_id.to_le_bytes()]).to_bytes()
}

pub struct Roll {
    bytes: [u8; 32],
    cursor: usize,
}

impl Roll {
    /// Build a roll from a raw 32-byte entropy seed. This is the seam a real
    /// randomness source (VRF / commit-reveal) plugs into — everything
    /// downstream (rarity, stats) is a deterministic function of these bytes.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Roll { bytes: seed, cursor: 0 }
    }

    pub fn new(clock: &Clock, minter: &Pubkey, counter: u64) -> Self {
        let hash = keccak::hashv(&[
            &clock.slot.to_le_bytes(),
            &clock.unix_timestamp.to_le_bytes(),
            minter.as_ref(),
            &counter.to_le_bytes(),
        ]);
        Roll::from_seed(hash.to_bytes())
    }

    /// The raw 32-byte entropy seed backing this roll.
    pub fn seed(&self) -> [u8; 32] {
        self.bytes
    }

    fn next_u64(&mut self) -> u64 {
        // Re-hash when the 32 bytes are exhausted.
        if self.cursor + 8 > 32 {
            let hash = keccak::hashv(&[&self.bytes]);
            self.bytes = hash.to_bytes();
            self.cursor = 0;
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&self.bytes[self.cursor..self.cursor + 8]);
        self.cursor += 8;
        u64::from_le_bytes(buf)
    }

    /// Uniform-ish value in [min, max] inclusive.
    pub fn range_u64(&mut self, min: u64, max: u64) -> u64 {
        if max <= min {
            return min;
        }
        min + self.next_u64() % (max - min + 1)
    }

    pub fn range_u32(&mut self, min: u32, max: u32) -> u32 {
        self.range_u64(min as u64, max as u64) as u32
    }

    /// Roll in [0, 10000) for basis-point weighted picks.
    pub fn bps(&mut self) -> u16 {
        (self.next_u64() % 10_000) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_slot_hashes(entries: &[(u64, [u8; 32])]) -> Vec<u8> {
        let mut data = (entries.len() as u64).to_le_bytes().to_vec();
        for (slot, hash) in entries {
            data.extend_from_slice(&slot.to_le_bytes());
            data.extend_from_slice(hash);
        }
        data
    }

    #[test]
    fn picks_first_slot_at_or_after_target_tolerating_skips() {
        // Newest-slot-first, as the runtime stores it. Slot 103 is skipped.
        let entries = [(105u64, [5u8; 32]), (104, [4u8; 32]), (102, [2u8; 32])];
        let data = build_slot_hashes(&entries);
        // Exact hits.
        assert_eq!(first_hash_at_or_after(&data, 104), Some((104, [4u8; 32])));
        assert_eq!(first_hash_at_or_after(&data, 105), Some((105, [5u8; 32])));
        // Target 103 was skipped -> fall forward to the next produced slot, 104.
        assert_eq!(first_hash_at_or_after(&data, 103), Some((104, [4u8; 32])));
        // Target at/under the oldest present slot -> that oldest slot.
        assert_eq!(first_hash_at_or_after(&data, 102), Some((102, [2u8; 32])));
        assert_eq!(first_hash_at_or_after(&data, 100), Some((102, [2u8; 32])));
        // A future slot beyond everything present -> none yet.
        assert_eq!(first_hash_at_or_after(&data, 999), None);
        // Empty buffer.
        assert_eq!(first_hash_at_or_after(&build_slot_hashes(&[]), 5), None);
    }

    #[test]
    fn reveal_seed_varies_by_minter_and_id() {
        let h = [7u8; 32];
        let a = Pubkey::new_from_array([1u8; 32]);
        let b = Pubkey::new_from_array([2u8; 32]);
        assert_ne!(reveal_seed(&h, &a, 0), reveal_seed(&h, &b, 0));
        assert_ne!(reveal_seed(&h, &a, 0), reveal_seed(&h, &a, 1));
        assert_eq!(reveal_seed(&h, &a, 0), reveal_seed(&h, &a, 0));
    }
}
