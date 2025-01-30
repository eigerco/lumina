use lumina_node::NodeError;
use thiserror::Error;

/// Result type alias for LuminaNode operations that can fail with a LuminaError
pub type Result<T, E = LuminaError> = std::result::Result<T, E>;

/// Represents all possible errors that can occur in the LuminaNode.
#[derive(Error, Debug, uniffi::Error)]
pub enum LuminaError {
    /// Error returned when trying to perform operations on a node that isn't running
    #[error("Node is not running")]
    NodeNotRunning,

    /// Error returned when network operations fail
    #[error("Network error: {msg}")]
    Network {
        /// Description of the network error
        msg: String,
    },

    /// Error returned when storage operations fail
    #[error("Storage error: {msg}")]
    Storage {
        /// Description of the storage error
        msg: String,
    },

    /// Error returned when trying to start a node that's already running
    #[error("Node is already running")]
    AlreadyRunning,

    /// Error returned when a hash string is invalid or malformed
    #[error("Invalid hash format: {msg}")]
    InvalidHash {
        /// Description of why the hash is invalid
        msg: String,
    },

    /// Error returned when a header is invalid or malformed
    #[error("Invalid header format: {msg}")]
    InvalidHeader {
        /// Description of why the header is invalid
        msg: String,
    },

    /// Error returned when storage initialization fails
    #[error("Storage initialization failed: {msg}")]
    StorageInit {
        /// Description of why storage initialization failed
        msg: String,
    },
}

impl LuminaError {
    pub fn network(msg: impl Into<String>) -> Self {
        Self::Network { msg: msg.into() }
    }

    pub fn storage(msg: impl Into<String>) -> Self {
        Self::Storage { msg: msg.into() }
    }

    pub fn invalid_hash(msg: impl Into<String>) -> Self {
        Self::InvalidHash { msg: msg.into() }
    }

    pub fn invalid_header(msg: impl Into<String>) -> Self {
        Self::InvalidHeader { msg: msg.into() }
    }

    pub fn storage_init(msg: impl Into<String>) -> Self {
        Self::StorageInit { msg: msg.into() }
    }
}

impl From<NodeError> for LuminaError {
    fn from(error: NodeError) -> Self {
        LuminaError::network(error.to_string())
    }
}

impl From<libp2p::multiaddr::Error> for LuminaError {
    fn from(e: libp2p::multiaddr::Error) -> Self {
        LuminaError::network(format!("Invalid multiaddr: {}", e))
    }
}
