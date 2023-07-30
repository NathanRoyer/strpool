use std::{fs, path::Path, env};
use rand::random;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let out_dir = env::var("OUT_DIR").unwrap();

    let rnd_seed_dst = Path::new(&out_dir).join("seed.dat");
    let rnd_seed: [u8; 32] = random();
    fs::write(rnd_seed_dst, &rnd_seed).unwrap();
}
