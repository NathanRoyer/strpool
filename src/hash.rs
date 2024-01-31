use cityhasher::hash_with_seed;

const SEED: &'static [u8] = include_bytes!(concat!(env!("OUT_DIR"), "/seed.dat"));

static SEED_U64: u64 = u64::from_ne_bytes([
    SEED[0], SEED[1], SEED[2], SEED[3],
    SEED[4], SEED[5], SEED[6], SEED[7],
]);

pub(crate) fn hash_str(string: &str) -> u64 {
    hash_with_seed(string, SEED_U64)
}
