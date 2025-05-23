use celestia_types::UniffiError;

pub type Result<T, E = GrpcClientError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum GrpcClientError {
    #[error("grpc error: {msg}")]
    GrpcError { msg: String },

    #[error("uniffi conversion error: {msg}")]
    UniffiError { msg: String },
}

impl From<celestia_grpc::Error> for GrpcClientError {
    fn from(value: celestia_grpc::Error) -> Self {
        GrpcClientError::GrpcError {
            msg: value.to_string(),
        }
    }
}

impl From<UniffiError> for GrpcClientError {
    fn from(value: UniffiError) -> Self {
        GrpcClientError::GrpcError {
            msg: value.to_string(),
        }
    }
}
