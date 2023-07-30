use core::hash::{BuildHasher, Hash, Hasher};
use ahash::RandomState;

static SEED: &'static [u8] = include_bytes!(concat!(env!("OUT_DIR"), "/seed.dat"));

macro_rules! seed {
    ($i:literal) => ( [
        SEED[$i + 0], SEED[$i + 1], SEED[$i + 2], SEED[$i + 3],
        SEED[$i + 4], SEED[$i + 5], SEED[$i + 6], SEED[$i + 7],
    ] )
}

static GEN: RandomState = RandomState::with_seeds(
    u64::from_ne_bytes(seed!( 0)), u64::from_ne_bytes(seed!( 8)),
    u64::from_ne_bytes(seed!(16)), u64::from_ne_bytes(seed!(24)),
);

pub(crate) fn hash_str(string: &str) -> u64 {
    let mut hasher = GEN.build_hasher();
    string.hash(&mut hasher);
    hasher.finish()
}
