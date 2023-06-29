mod blob;
pub mod client;
mod error;
mod header;
mod share;

pub use error::{Error, Result};

pub mod prelude {
    pub use celestia_proto::share::p2p::shrex::nd::Proof as RawProof;
    pub use celestia_types::nmt::Namespace;
    pub use celestia_types::Blob;

    pub use crate::blob::BlobClient;
    pub use crate::header::HeaderClient;
    pub use crate::share::ShareClient;
}
