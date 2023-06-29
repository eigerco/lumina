use http::header::InvalidHeaderValue;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Token contains invalid characters: {0}")]
    InvalidCharactersInToken(#[from] InvalidHeaderValue),

    #[error(transparent)]
    JsonRpc(#[from] jsonrpsee::core::Error),
}
