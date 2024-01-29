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
    let lumina_node_wasm_path = project_root.join("node-wasm");
    println!("cargo:rerun-if-changed={}", lumina_node_wasm_path.display());

    // separate target directory is needed so that the two running cargo builds don't lock
    // eachother
    let dest_path = project_root.join("target-wasm");

    let release_build = env::var("PROFILE")? == "release";
    let wasm_output = format!(
        "{}/wasm32-unknown-unknown/{}/lumina_node_wasm.wasm",
        dest_path.display(),
        if release_build { "release" } else { "debug" }
    );

    let build_cmd = Command::new("cargo")
        .current_dir(project_root)
        .args([
            "build",
            "-p",
            "lumina-node-wasm",
            "--target=wasm32-unknown-unknown",
            "--target-dir",
        ])
        .arg(&dest_path)
        .args(["--profile", if release_build { "release" } else { "dev" }])
        .output()
        .expect("cargo build failed");
    if !build_cmd.status.success() {
        panic!(
            "Error while compiling lumina-node-wasm:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&build_cmd.stdout),
            String::from_utf8_lossy(&build_cmd.stderr)
        );
    }

    let bindgen_cmd = Command::new("wasm-bindgen")
        .args(["--target=web", "--out-dir"])
        .arg(&dest_path)
        .arg(wasm_output)
        .output()
        .expect("wasm-bindgen failed");

    if !bindgen_cmd.status.success() {
        panic!(
            "Error while running bindgen on lumina-node-wasm:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&bindgen_cmd.stdout),
            String::from_utf8_lossy(&bindgen_cmd.stderr)
        );
    }

    println!("cargo:rustc-env=WASM_NODE_OUT_DIR={}", dest_path.display());

    Ok(())
}
