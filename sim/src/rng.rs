//! The game's RNG: MT19937 (FUN_004fb640 seed, FUN_004fb6a0 draw → genrand_int32 & 0x7fffffff), global
//! instance 0x686840 via FUN_004fce50. In TAS mode the oracle hook reseeds it before random rule effects
//! (see play/hook.c `reseed_key`), so the sim can reproduce them: seed = tas_mix(S ^ site, btick).

/// `mt` holds the seeded (untwisted) state while `lazy`: only `init` words are filled in, and the first 227
/// outputs come straight from them (the in-place twist of word i < 227 reads words i, i+1, i+397, none of them
/// twisted yet), so a seed that is drawn from a few times skips most of the 624-word init and the full twist.
/// Outputs are identical to the eager generator.
pub struct Mt { mt: [u32; 624], idx: usize, init: usize, lazy: bool }

impl Mt {
    pub fn new(seed: u32) -> Mt {
        let mut mt = [0u32; 624];
        mt[0] = if seed == 0 { 0x1105 } else { seed };
        Mt { mt, idx: 0, init: 1, lazy: true }
    }
    #[inline]
    fn init_to(&mut self, n: usize) {
        while self.init < n { let i = self.init; self.mt[i] = (self.mt[i - 1] ^ (self.mt[i - 1] >> 30)).wrapping_mul(0x6c078965).wrapping_add(i as u32); self.init += 1; }
    }
    #[inline]
    fn twisted(&self, i: usize) -> u32 {
        let y = (self.mt[i] & 0x8000_0000) | (self.mt[(i + 1) % 624] & 0x7fff_ffff);
        self.mt[(i + 397) % 624] ^ (y >> 1) ^ if y & 1 != 0 { 0x9908_b0df } else { 0 }
    }
    /// FUN_004fce50 Rand(): non-negative int
    pub fn rand(&mut self) -> i32 {
        let mut y;
        if self.lazy && self.idx < 227 {
            self.init_to(self.idx + 398);
            y = self.twisted(self.idx);
            self.idx += 1;
        } else {
            if self.lazy {
                // switch to the eager state: finish the seed, twist in place, continue at the same index
                self.init_to(624);
                for i in 0..624 { self.mt[i] = self.twisted(i); }
                self.lazy = false;
            }
            if self.idx >= 624 {
                for i in 0..624 { self.mt[i] = self.twisted(i); }
                self.idx = 0;
            }
            y = self.mt[self.idx];
            self.idx += 1;
        }
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        (y & 0x7fff_ffff) as i32
    }
}

/// hook.c tas_mix
pub fn tas_mix(a: u32, b: u32) -> u32 {
    let mut s = a.wrapping_mul(0x9E3779B1) ^ b.wrapping_mul(0x85EBCA6B).wrapping_add(0x68E31DA4);
    s ^= s >> 15; s = s.wrapping_mul(0x2C1B3C6D); s ^= s >> 12;
    if s == 0 { 1 } else { s }
}

/// RNG state at a keyed reseed site.
pub fn keyed(seed: u32, site: u32, btick: i32) -> Mt { Mt::new(tas_mix(seed ^ site, btick as u32)) }

pub const SITE_SPOOKY: u32 = 0x5B00;
pub const SITE_LUCKY: u32 = 0x5B01;
/// Fire-time LogicMgr+0x2c/+0x30 rolls (FUN_0046ac70), keyed by Board+0x1c4 at fire.
pub const SITE_SHOT: u32 = 0x5B03;

#[cfg(test)]
mod tests {
    /// the eager MT19937 the lazy `Mt` replaced
    fn eager(seed: u32, n: usize) -> Vec<i32> {
        let mut mt = [0u32; 624];
        mt[0] = if seed == 0 { 0x1105 } else { seed };
        for i in 1..624 { mt[i] = (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(0x6c078965).wrapping_add(i as u32); }
        let mut idx = 624;
        (0..n).map(|_| {
            if idx >= 624 {
                for i in 0..624 {
                    let y = (mt[i] & 0x8000_0000) | (mt[(i + 1) % 624] & 0x7fff_ffff);
                    mt[i] = mt[(i + 397) % 624] ^ (y >> 1) ^ if y & 1 != 0 { 0x9908_b0df } else { 0 };
                }
                idx = 0;
            }
            let mut y = mt[idx]; idx += 1;
            y ^= y >> 11; y ^= (y << 7) & 0x9d2c_5680; y ^= (y << 15) & 0xefc6_0000; y ^= y >> 18;
            (y & 0x7fff_ffff) as i32
        }).collect()
    }

    #[test]
    fn lazy_matches_eager() {
        for seed in [0u32, 1, 5489, 0x1105, 0xdead_beef, u32::MAX].into_iter().chain((0..200).map(|k| super::tas_mix(k, k * 7 + 3))) {
            for n in [1usize, 2, 226, 227, 228, 623, 624, 625, 1300, 2000] {
                let mut m = super::Mt::new(seed);
                let got: Vec<i32> = (0..n).map(|_| m.rand()).collect();
                assert_eq!(got, eager(seed, n), "seed {seed} n {n}");
            }
        }
    }
}
