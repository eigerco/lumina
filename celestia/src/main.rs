use anyhow::Result;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
use crate::wasm as imp;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
use crate::native as imp;

fn main() -> Result<()> {
    imp::run()
}
