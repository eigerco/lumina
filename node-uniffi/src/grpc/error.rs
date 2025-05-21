use celestia_types::UniffiError;

pub type Result<T, E = GrpcError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum GrpcError {
    #[error("grpc error: {msg}")]
    GrpcError { msg: String },

    #[error("uniffi conversion error: {msg}")]
    UniffiError { msg: String },
}

impl From<celestia_grpc::Error> for GrpcError {
    fn from(value: celestia_grpc::Error) -> Self {
        GrpcError::GrpcError {
            msg: value.to_string(),
        }
    }
}

impl From<UniffiError> for GrpcError {
    fn from(value: UniffiError) -> Self {
        GrpcError::GrpcError {
            msg: value.to_string(),
        }
    }
}
