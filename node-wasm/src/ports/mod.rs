use wasm_bindgen::prelude::*;

use crate::commands::{NodeCommand, WorkerResponse};
use crate::error::{Context, Error, Result};
use crate::ports::client::Client;
use crate::ports::server::Server;

mod client;
mod common;
mod server;

pub(crate) type WorkerServer = Server<NodeCommand, WorkerResponse>;

pub struct WorkerClient {
    client: Client<NodeCommand, WorkerResponse>,
}

impl WorkerClient {
    pub fn new(object: JsValue) -> Result<Self> {
        Ok(WorkerClient {
            client: Client::start(object)?,
        })
    }

    pub(crate) async fn add_connection_to_worker(&self, port: JsValue) -> Result<()> {
        let response = self.client.send(NodeCommand::InternalPing, Some(port))?;

        let worker_response = response
            .await
            .context("Response oneshot dropped, should not happen")?;

        if !worker_response.is_internal_pong() {
            Err(Error::new(&format!(
                "invalid response, expected InternalPing got {worker_response:?}"
            )))
        } else {
            Ok(())
        }
    }

    pub(crate) async fn exec(&self, command: NodeCommand) -> Result<WorkerResponse> {
        let response = self.client.send(command, None)?;
        response
            .await
            .context("Response oneshot dropped, should not happen")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use wasm_bindgen_futures::spawn_local;
    use wasm_bindgen_test::wasm_bindgen_test;
    use web_sys::MessageChannel;

    use crate::ports::server::ServerConnection;

    #[wasm_bindgen_test]
    async fn worker_client_server() {
        let channel0 = MessageChannel::new().unwrap();
        let mut server = WorkerServer::new();
        let port_channel = server.get_port_channel();

        spawn_local(async move {
            let (request, responder) = server.recv().await.unwrap();
            assert!(matches!(request, NodeCommand::IsRunning));
            responder.send(WorkerResponse::IsRunning(false)).unwrap();

            let (request, responder) = server.recv().await.unwrap();
            assert!(matches!(request, NodeCommand::InternalPing));
            responder.send(WorkerResponse::InternalPong).unwrap();

            let (request, responder) = server.recv().await.unwrap();
            assert!(matches!(request, NodeCommand::IsRunning));
            responder.send(WorkerResponse::IsRunning(true)).unwrap();
        });

        port_channel.send(channel0.port1().into()).unwrap();
        let client0 = WorkerClient::new(channel0.port2().into()).unwrap();

        let response = client0.exec(NodeCommand::IsRunning).await.unwrap();
        assert!(matches!(response, WorkerResponse::IsRunning(false)));

        let channel1 = MessageChannel::new().unwrap();
        client0
            .add_connection_to_worker(channel1.port1().into())
            .await
            .unwrap();
        let client1 = WorkerClient::new(channel1.port2().into()).unwrap();

        let response = client1.exec(NodeCommand::IsRunning).await.unwrap();
        assert!(matches!(response, WorkerResponse::IsRunning(true)));
    }

    #[wasm_bindgen_test]
    async fn smoke_test() {
        let channel = MessageChannel::new().unwrap();
        let client = Client::<i32, i32>::start(channel.port1().into()).unwrap();

        let (request_tx, mut request_rx) = mpsc::unbounded_channel();
        let (port_tx, _) = mpsc::unbounded_channel();
        let _server_connection =
            ServerConnection::<i32, i32>::start(channel.port2().into(), request_tx, port_tx)
                .unwrap();

        let response = client.send(42, None).unwrap();

        let (request, responder) = request_rx.recv().await.expect("failedd to recv");
        assert_eq!(request, 42);
        responder.send(43).unwrap();

        assert_eq!(response.await.unwrap(), 43);
    }

    #[wasm_bindgen_test]
    async fn response_channel_dropped() {
        let channel = MessageChannel::new().unwrap();
        let client = Client::<i32, i32>::start(channel.port1().into()).unwrap();

        let (request_tx, mut request_rx) = mpsc::unbounded_channel();
        let (port_tx, _) = mpsc::unbounded_channel();
        let _server_connection =
            ServerConnection::<i32, i32>::start(channel.port2().into(), request_tx, port_tx)
                .unwrap();

        let response = client.send(42, None).unwrap();

        let (request, responder) = request_rx.recv().await.expect("failedd to recv");
        assert_eq!(request, 42);
        drop(responder);

        assert_eq!(response.await, None);
    }

    #[wasm_bindgen_test]
    async fn multiple_channels() {
        let channel = MessageChannel::new().unwrap();
        let client = Client::<i32, String>::start(channel.port1().into()).unwrap();

        let (request_tx, mut request_rx) = mpsc::unbounded_channel();
        let (port_tx, _) = mpsc::unbounded_channel();
        let _server_connection =
            ServerConnection::<i32, String>::start(channel.port2().into(), request_tx, port_tx)
                .unwrap();

        let mut responses = [
            Some(client.send(0, None).unwrap()),
            Some(client.send(1, None).unwrap()),
            Some(client.send(2, None).unwrap()),
            Some(client.send(3, None).unwrap()),
            Some(client.send(4, None).unwrap()),
            Some(client.send(5, None).unwrap()),
        ];

        for i in 0..=5 {
            let (request, responder) = request_rx.recv().await.unwrap();
            assert_eq!(i, request);
            responder.send(format!("R:{request}")).unwrap();
        }

        for i in (0..=5).rev() {
            assert_eq!(
                responses[i].take().unwrap().await.unwrap(),
                format!("R:{i}")
            );
        }
    }

    #[wasm_bindgen_test]
    async fn client_server() {
        let channel = MessageChannel::new().unwrap();
        let mut server = Server::<i32, i32>::new();
        let port_channel = server.get_port_channel();
        port_channel.send(channel.port2().into()).unwrap();

        let client = Client::<i32, i32>::start(channel.port1().into()).unwrap();

        let response = client.send(1, None).unwrap();

        let (request, responder) = server.recv().await.unwrap();
        assert_eq!(request, 1);
        responder.send(2).unwrap();

        assert_eq!(response.await.unwrap(), 2);
    }
}
