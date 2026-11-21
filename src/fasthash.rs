//! A fast hash for the runtime's own maps and fingerprints (node keys, glyph and instance ids, cache
//! keys: none of them chosen by an attacker), several times faster than the standard library's SipHash
//! on short keys, which costs most in wasm. Each word is mixed in by a folded multiply (the 128-bit
//! product of the state and the word with a constant, its halves xored), as foldhash and ahash do, so a
//! difference anywhere in a word reaches every bit of the state.
use std::hash::{BuildHasherDefault, Hasher};

const K: u64 = 0x517c_c1b7_2722_0a95;
const SEED: u64 = 0x243f_6a88_85a3_08d3;

/// The hasher (`Default` and `new` start it at the same seed).
#[derive(Clone, Copy, Debug)]
pub struct FastHasher { hash: u64 }
impl Default for FastHasher { fn default() -> Self { Self { hash: SEED } } }
impl FastHasher {
    pub fn new() -> Self { Self::default() }
    #[inline]
    fn add(&mut self, w: u64) {
        let p = ((self.hash ^ w) as u128).wrapping_mul(K as u128);
        self.hash = (p as u64) ^ ((p >> 64) as u64);
    }
}
impl Hasher for FastHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut c = bytes.chunks_exact(8);
        for w in &mut c { self.add(u64::from_le_bytes(w.try_into().expect("8 bytes"))); }
        let r = c.remainder();
        if !r.is_empty() {
            let mut b = [0u8; 8];
            b[..r.len()].copy_from_slice(r);
            // Then its length, as a word of its own: "a\0" and "a" differ.
            self.add(u64::from_le_bytes(b));
            self.add(r.len() as u64);
        }
    }
    #[inline] fn write_u8(&mut self, i: u8) { self.add(i as u64); }
    #[inline] fn write_u16(&mut self, i: u16) { self.add(i as u64); }
    #[inline] fn write_u32(&mut self, i: u32) { self.add(i as u64); }
    #[inline] fn write_u64(&mut self, i: u64) { self.add(i); }
    #[inline] fn write_usize(&mut self, i: usize) { self.add(i as u64); }
    #[inline] fn finish(&self) -> u64 { self.hash }
}
/// Builds `FastHasher`s for a map or set.
pub type Build = BuildHasherDefault<FastHasher>;
/// A `HashMap` hashed with `FastHasher` (made with `default()`, not `new()`).
pub type HashMap<K, V> = std::collections::HashMap<K, V, Build>;
/// A `HashSet` hashed with `FastHasher`.
pub type HashSet<K> = std::collections::HashSet<K, Build>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::Hash;
    fn h(v: impl Hash) -> u64 { let mut s = FastHasher::new(); v.hash(&mut s); s.finish() }
    #[test]
    fn keys_that_differ_hash_apart_and_spread_over_the_buckets() {
        let mut seen: std::collections::HashSet<u64> = Default::default();
        for pre in ["doc/button/sec", "row", "a/b/c/d/e/f/g/h/", ""] {
            for i in 0..50_000 { let k = format!("{pre}{i}"); assert!(seen.insert(h(k.as_str())), "{k}"); }
        }
        for i in 1..64 { assert!(seen.insert(h("a".repeat(i).as_str())), "{i} a's"); }
        assert_ne!(h((1u32, 2u32)), h((2u32, 1u32)));
        assert_eq!(h("same"), h("same"));
        // The low bits (the bucket) and the top 7 (the tag) of row keys are even.
        let mut low = [0u32; 256];
        let mut top = [0u32; 128];
        for i in 0..64_000 { let v = h(format!("row{i}").as_str()); low[(v & 255) as usize] += 1; top[(v >> 57) as usize] += 1; }
        assert!(low.iter().all(|&n| (150..=350).contains(&n)), "{:?}", low.iter().min().zip(low.iter().max()));
        assert!(top.iter().all(|&n| (350..=650).contains(&n)), "{:?}", top.iter().min().zip(top.iter().max()));
    }
}
