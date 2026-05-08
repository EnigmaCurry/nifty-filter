use std::path::Path;

fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("LICENSE.md");

    // Walk up from CARGO_MANIFEST_DIR to find LICENSE.md
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut dir = Path::new(&manifest_dir).to_path_buf();
    loop {
        let candidate = dir.join("LICENSE.md");
        if candidate.exists() {
            std::fs::copy(&candidate, &dest).expect("failed to copy LICENSE.md");
            println!("cargo:rerun-if-changed={}", candidate.display());
            return;
        }
        if !dir.pop() {
            break;
        }
    }
    panic!("LICENSE.md not found in any parent directory of {}", manifest_dir);
}
