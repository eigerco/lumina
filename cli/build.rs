//! Build script which compiles node-wasm so that lumina-cli can embed it

use anyhow::{bail, Result};
use std::env;
use std::path::PathBuf;
use std::process::Command;

fn find_root() -> Result<PathBuf> {
    let mut current_dir = env::current_dir()?;
    while current_dir.pop() {
        if current_dir.join(".git").exists() {
            return Ok(current_dir);
        }
    }
    bail!("cannot find project root")
}

fn main() -> Result<()> {
    let project_root = find_root()?;
    let wasm_node_path = project_root.join("node-wasm");
    println!("cargo:rerun-if-changed={}", wasm_node_path.display());

    let dest_path = project_root.join("target-wasm");

    let profile_flag = if env::var("PROFILE")? == "release" {
        "--profile"
    } else {
        "--debug"
    };

    let output = Command::new("wasm-pack")
        .args(["build", "--target", "web", "--out-dir"])
        .arg(&dest_path)
        .arg(profile_flag)
        .arg(&wasm_node_path)
        .output()
        .expect("to build wasm files successfully");

    if !output.status.success() {
        panic!(
            "Error while compiling node-wasm:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    println!("cargo:rustc-env=WASM_NODE_OUT_DIR={}", dest_path.display());

    Ok(())
}
