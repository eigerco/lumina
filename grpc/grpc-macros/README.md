Helper crate for grpc_method macro for creating a tonic gRPC client, used by [celestia-grpc](https://docs.rs/celestia-grpc/latest/celestia_grpc/).

# Example

```rust,ignore
use celestia_proto::cosmos::auth::v1beta1::query_client::QueryClient;
use tonic::service::Interceptor;
use tonic::transport::Channel;

pub struct GrpcClient<I>
where
   I: Interceptor,
{
    grpc_channel: Channel,
    auth_interceptor: I,
}

impl<I> GrpcClient<I>
where
    I: Interceptor + Clone,
{
    /// Get auth params
    #[grpc_method(AuthQueryClient::params)]
    async fn get_auth_params(&mut self) -> Result<AuthParams, Error>;
}
```
