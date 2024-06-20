/// Alias for a `Result` with the error type [`celestia_rpc::Error`].
///
/// [`celestia_rpc::Error`]: crate::Error
pub type Result<T> = std::result::Result<T, Error>;

/// Representation of all the errors that can occur when interacting with [`celestia_rpc`].
///
/// [`celestia_rpc`]: crate
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Invalid characters in the auth token.
    #[cfg(not(target_arch = "wasm32"))]
    #[error("Token contains invalid characters: {0}")]
    InvalidCharactersInToken(#[from] http::header::InvalidHeaderValue),

    /// Protocol specified in connection string is not supported.
    #[error("Protocol not supported or missing: {0}")]
    ProtocolNotSupported(String),

    /// Error propagated from the [`jsonrpsee`].
    #[error(transparent)]
    JsonRpc(#[from] jsonrpsee::core::ClientError),
}
