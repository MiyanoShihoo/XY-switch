use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../..").canonicalize().unwrap();
    let source = root.join("src-tauri/src");
    let privacy_path = source.join("privacy.rs");
    let privacy = fs::read_to_string(&privacy_path).unwrap();
    let core = privacy.split_once("\npub fn load(")
        .expect("privacy core extraction boundary changed; inspect before continuing").0
        .replace("use crate::database::Database;\n", "")
        .replace("use tauri::State;\n", "")
        .replace("#[path = \"privacy_stream.rs\"]\npub(crate) mod response_stream;\n", "");
    assert!(!core.contains("tauri::") && !core.contains("Database"));
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(output.join("privacy_core.rs"), core).unwrap();
    let types = source.join("plugins/types.rs");
    let validation = source.join("plugins/validation.rs");
    let market = source.join("marketplace/mod.rs");
    let service = format!(
        "pub mod plugins {{\n#[path = {types:?}] pub mod types;\n#[path = {validation:?}] pub mod validation;\npub use types::*;\npub use validation::{{validate_package, package_digest, canonical_package_bytes, signing_payload, withdrawal_payload}};\n}}\n#[allow(dead_code)] pub mod privacy {{ include!(concat!(env!(\"OUT_DIR\"), \"/privacy_core.rs\")); }}\n#[path = {market:?}] pub mod marketplace;\n",
        types = types.to_string_lossy(), validation = validation.to_string_lossy(),
        market = market.to_string_lossy(),
    );
    fs::write(output.join("service.rs"), service).unwrap();
    for path in [privacy_path, types, validation, market, source.join("bin/xy-marketplace.rs")] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}
