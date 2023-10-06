use async_trait::async_trait;

mod exchange;
mod executor;
pub mod node;
pub mod p2p;
pub mod peer_tracker;
pub mod store;
pub mod syncer;
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;
mod utils;

#[async_trait]
pub trait Service: Send + Sync {
    type Args;
    type Command;
    type Error;

    async fn start(args: Self::Args) -> Result<Self, Self::Error>
    where
        Self: Sized;

    async fn stop(&self) -> Result<(), Self::Error>;

    async fn send_command(&self, cmd: Self::Command) -> Result<(), Self::Error>;
}
