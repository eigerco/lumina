//! Build script which makes sure that the wasm target directory exists and allows configuration

use anyhow::Result;
use serde::Deserialize;
use std::fs::create_dir_all;
use std::path::PathBuf;

const WASM_NODE_OUTPUT_DIR: &str = "../node-wasm/pkg";

fn default_root() -> PathBuf {
    PathBuf::from(WASM_NODE_OUTPUT_DIR)
}

#[derive(Default, Deserialize)]
struct Config {
    #[serde(default = "default_root")]
    wasm_node_root: PathBuf,
}

fn main() -> Result<()> {
    let env = envy::from_env::<Config>()?;

    if !env.wasm_node_root.exists() {
        create_dir_all(&env.wasm_node_root)?;
    }

    println!(
        "cargo:rustc-env=WASM_NODE_OUT_DIR={}",
        env.wasm_node_root.display()
    );

    Ok(())
}
