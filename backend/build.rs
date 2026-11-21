use std::{fs, path::Path};

fn main() {
    // rust-embed needs the folder to exist; an unbuilt frontend simply embeds nothing.
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../frontend/dist");
    fs::create_dir_all(&dist).expect("create frontend/dist");
    println!("cargo:rerun-if-changed=../frontend/dist");
}
