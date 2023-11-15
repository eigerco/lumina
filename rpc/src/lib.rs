mod blob;
pub mod client;
mod error;
mod header;
#[cfg(feature = "p2p")]
pub mod p2p;
mod share;
mod state;

pub use crate::blob::BlobClient;
#[cfg(not(target_arch = "wasm32"))]
pub use crate::client::Client;
pub use crate::error::{Error, Result};
pub use crate::header::HeaderClient;
#[cfg(feature = "p2p")]
pub use crate::p2p::P2PClient;
pub use crate::share::ShareClient;
pub use crate::state::StateClient;

pub mod prelude {
    pub use crate::BlobClient;
    pub use crate::HeaderClient;
    #[cfg(feature = "p2p")]
    pub use crate::P2PClient;
    pub use crate::ShareClient;
    pub use crate::StateClient;
}
