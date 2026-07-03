//! Pseudo-randomness for monster rolls.
//!
//! SECURITY NOTE: this derives entropy from the clock, the minter and a
//! counter. It is good enough for a devnet prototype but IS predictable and
//! grindable by a determined attacker. Before mainnet, swap this for a real
//! VRF (e.g. Switchboard On-Demand randomness) — the call site in
//! `mint_monster_common` is the single place to change.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::keccak;

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
