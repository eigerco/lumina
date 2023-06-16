use anyhow::Result;

fn main() -> Result<()> {
    prost_build::Config::new()
        .include_file("mod.rs")
        .compile_protos(&["vendor/tendermint/types/types.proto"], &["vendor"])?;

    Ok(())
}
