//! Hashes the simulation source into `SIM_SOURCE_HASH`, which a saved world
//! carries so that a build whose code has changed refuses to load it.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("read src")
        .map(|e| e.expect("dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn main() {
    let mut files = Vec::new();
    rust_files(Path::new("src"), &mut files);
    // FNV-1a over each path and its bytes.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            hash = (hash ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for path in &files {
        feed(path.to_string_lossy().as_bytes());
        feed(&fs::read(path).expect("read source"));
    }
    println!("cargo:rustc-env=SIM_SOURCE_HASH={hash}");
    println!("cargo:rerun-if-changed=src");
}
