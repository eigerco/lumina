use crate::utils::client::{env_or, token_from_env};
use crate::AuthLevel;

use anyhow::Result;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::Interceptor;
use tonic::transport::Channel;
use tonic::{Request, Status};

use celestia_rpc::tonic::GrpcClient;

const CELESTIA_GRPC_URL: &str = "http://localhost:9090";

#[derive(Clone)]
pub struct TestAuthInterceptor {
    token: Option<MetadataValue<Ascii>>,
}

impl Interceptor for TestAuthInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        if let Some(token) = &self.token {
            request
                .metadata_mut()
                .insert("authorization", token.clone());
        }
        Ok(request)
    }
}

impl TestAuthInterceptor {
    pub fn new(bearer_token: Option<String>) -> Result<TestAuthInterceptor> {
        let token = bearer_token.map(|token| token.parse()).transpose()?;
        Ok(Self { token })
    }
}

pub async fn new_test_client(auth_level: AuthLevel) -> Result<GrpcClient<TestAuthInterceptor>> {
    let _ = dotenvy::dotenv();
    let url = env_or("CELESTIA_GRPC_URL", CELESTIA_GRPC_URL);
    let grpc_channel = Channel::from_shared(url)?.connect().await?;

    let token = token_from_env(auth_level)?;
    let auth_interceptor = TestAuthInterceptor::new(token)?;
    Ok(GrpcClient::new(grpc_channel, auth_interceptor))
}
