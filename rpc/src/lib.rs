mod blob;
pub mod client;
mod error;
mod header;
#[cfg(feature = "libp2p")]
pub mod p2p;
mod share;
mod state;

pub use crate::blob::BlobClient;
pub use crate::error::{Error, Result};
pub use crate::header::HeaderClient;
#[cfg(feature = "libp2p")]
pub use crate::p2p::P2PClient;
pub use crate::share::ShareClient;
pub use crate::state::StateClient;

pub mod prelude {
    pub use crate::BlobClient;
    pub use crate::HeaderClient;
    #[cfg(feature = "libp2p")]
    pub use crate::P2PClient;
    pub use crate::ShareClient;
    pub use crate::StateClient;
}
