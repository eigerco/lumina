use crate::Error;
use crate::tonic::GrpcClient;

use celestia_proto::cosmos::base::tendermint::v1beta1::{
    service_client::ServiceClient as TendermintServiceClient,
    GetNodeInfoRequest,
};


impl GrpcClient {
    async fn get_node_info(&self) -> Result<(), Error> {
        let service_client = TendermintServiceClient::with_interceptor(self.grpc_channel, self.auth_interceptor);
        let request = GetNodeInfoRequest {};

        let node_info = service_client.get_node_info(request).await?;

        node_info
    }
}
