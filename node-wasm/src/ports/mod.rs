use wasm_bindgen::prelude::*;

use serde::de::DeserializeOwned;
use serde_wasm_bindgen::from_value;
use tokio::sync::mpsc;
use web_sys::{MessageChannel, MessageEvent};

use crate::commands::CheckableResponseExt;
use crate::commands::{Command, ManagementCommand, NodeCommand, NodeSubscription, WorkerResponse};
use crate::error::{Context, Error, Result};
use crate::ports::client::Client;
use crate::ports::server::Server;

mod client;
mod common;
mod server;

pub(crate) use common::{MessagePortLike, PayloadWithContext, Port};

pub(crate) type WorkerServer = Server; //<NodeCommand, WorkerResponse>;

pub struct WorkerClient {
    client: Client<Command, WorkerResponse>,
}

impl WorkerClient {
    pub fn new(object: JsValue) -> Result<Self> {
        Ok(WorkerClient {
            client: Client::start(object)?,
        })
    }

    pub(crate) async fn add_connection_to_worker(&self, port: MessagePortLike) -> Result<()> {
        let command = Command::Meta(ManagementCommand::ConnectPort);
        let response = self.client.send(command, Some(port))?;

        let worker_response = response
            .await
            .context("Response oneshot dropped, should not happen")?;

        if !worker_response.is_port_connected() {
            Err(Error::new(&format!(
                "invalid response, expected InternalPing got {worker_response:?}"
            )))
        } else {
            Ok(())
        }
    }

    pub(crate) async fn mgmt_command(&self, command: ManagementCommand) -> Result<WorkerResponse> {
        let command = Command::Meta(command);
        let response = self.client.send(command, None)?;
        response
            .await
            .context("Response oneshot dropped, should not happen")
    }

    pub(crate) async fn exec(&self, command: NodeCommand) -> Result<WorkerResponse> {
        let command = Command::Node(command);
        let response = self.client.send(command, None)?;
        response
            .await
            .context("Response oneshot dropped, should not happen")
    }

    pub(crate) async fn subscribe<T: DeserializeOwned + 'static>(
        &self,
        subscription: NodeSubscription,
    ) -> Result<(mpsc::UnboundedReceiver<T>, Port)> {
        let (server_port, client_port, subscription_stream) = self.prepare_subscription_port()?;
        let command = Command::Subscribe(subscription);

        let response = self.client.send(command, Some(server_port))?;
        response
            .await
            .context("Response oneshot dropped, should not happen")?
            .into_subscribed()
            .check_variant()??;
        Ok((subscription_stream, client_port))
    }

    fn prepare_subscription_port<T: DeserializeOwned + 'static>(
        &self,
    ) -> Result<(MessagePortLike, Port, mpsc::UnboundedReceiver<T>)> {
        let (tx, rx) = mpsc::unbounded_channel();
        let channel = MessageChannel::new()?;

        let server_port = JsValue::from(channel.port1()).into();

        let client_port = Port::new(
            channel.port2().into(),
            move |ev: MessageEvent| -> Result<()> {
                web_sys::console::info_1(&ev);
                let item: T = from_value(ev.data()).context("could not deserialize message")?;
                tx.send(item)
                    .context("forwarding subscription item failed")?;
                Ok(())
            },
        )?;

        Ok((server_port, client_port, rx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use wasm_bindgen_futures::spawn_local;
    use wasm_bindgen_test::wasm_bindgen_test;
    use web_sys::MessageChannel;

    use lumina_utils::time::sleep;

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

            // otherwise server is dropped too soon and last message does not make it
            sleep(Duration::from_millis(100)).await;
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
