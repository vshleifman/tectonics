//! The same tiny deterministic PRNG the TypeScript side uses (mulberry32 seeded
//! from a string), so a seed string produces the same stream in both.

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct Rng {
    state: u32,
}

impl Rng {
    /// Seed from a string, hashing its UTF-16 code units exactly as `makeRng`.
    pub fn new(seed: &str) -> Rng {
        let units: Vec<u16> = seed.encode_utf16().collect();
        let mut h: u32 = 1_779_033_703 ^ units.len() as u32;
        for &unit in &units {
            h = (h ^ unit as u32).wrapping_mul(3_432_918_353);
            h = h.rotate_left(13);
        }
        Rng { state: h }
    }

    /// Next value in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x6d2b_79f5);
        let a = self.state;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        (t ^ (t >> 14)) as f64 / 4_294_967_296.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_typescript_stream() {
        // Draws of `makeRng` from `src/math/rng.ts` for the same seeds.
        let mut rng = Rng::new("bench");
        assert_eq!(rng.next_f64(), 0.5735053720418364);
        assert_eq!(rng.next_f64(), 0.48276105034165084);
        assert_eq!(rng.next_f64(), 0.5790228508412838);
        assert_eq!(Rng::new("tectonics:cell:7").next_f64(), 0.18393687484785914);
    }
}
