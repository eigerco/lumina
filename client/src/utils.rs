use crate::{Error, Result};

/// Executes a one-shot RPC call with transparent, automatic retries on network failure.
///
/// If the underlying RPC call fails with a transport error (indicating a disconnection),
/// this macro will wait for the `RpcConnectionManager` to re-establish a connection
/// and then retry the operation.
#[macro_export]
macro_rules! exec_rpc {
    ($self:ident, |$client:ident| $expr:expr) => {
        loop {
            let guard = $self.inner.rpc_manager.client_lock.read().await;
            let $client = match guard.as_ref() {
                Some(client) => client,
                None => {
                    drop(guard);
                    $self.inner.rpc_manager.notify_new_client.notified().await;
                    continue;
                }
            };

            match $expr.await {
                Ok(value) => break Ok(value),
                Err(celestia_rpc::Error::JsonRpc(jsonrpsee_core::ClientError::Transport(e))) => {
                    log::warn!(
                        "RPC call failed with a transport error: {e}. Waiting for reconnect to retry..."
                    );
                    drop(guard);
                    $self.inner.rpc_manager.notify_new_client.notified().await;
                    continue;
                }
                Err(e) => {
                    break Err(Error::from(e));
                }
            }
        }
    };
}

pub(crate) fn height_i64(height: u64) -> Result<i64> {
    height.try_into().map_err(|_| Error::InvalidHeight(height))
}
