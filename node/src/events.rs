use std::fmt;
use std::panic::Location;
use std::time::Duration;

use instant::SystemTime;
use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Debug, thiserror::Error)]
pub enum RecvError {
    #[error("Channel closed")]
    Closed,
}

#[derive(Debug, thiserror::Error)]
pub enum TryRecvError {
    #[error("Channel empty")]
    Empty,
    #[error("Channel closed")]
    Closed,
}

#[derive(Debug)]
pub struct EventSubscription {
    tx: broadcast::Sender<NodeEventInfo>,
}

#[derive(Debug, Clone)]
pub struct EventSender {
    tx: broadcast::Sender<NodeEventInfo>,
}

#[derive(Debug)]
pub struct EventReceiver {
    rx: broadcast::Receiver<NodeEventInfo>,
}

impl EventSubscription {
    pub fn new() -> EventSubscription {
        let (tx, _) = broadcast::channel(32);
        EventSubscription { tx }
    }

    pub(crate) fn sender(&self) -> EventSender {
        EventSender {
            tx: self.tx.clone(),
        }
    }

    pub fn subscribe(&self) -> EventReceiver {
        EventReceiver {
            rx: self.tx.subscribe(),
        }
    }
}

impl EventSender {
    pub fn send(&self, event: NodeEvent) {
        let time = SystemTime::now();
        let location: &'static Location<'static> = Location::caller();

        // Error is produced if there aren't any subscribers. Since this is
        // a valid case, we ignore the error.
        let _ = self.tx.send(NodeEventInfo {
            event,
            time,
            file_path: location.file(),
            file_line: location.line(),
        });
    }
}

impl EventReceiver {
    pub async fn recv(&mut self) -> Result<NodeEventInfo, RecvError> {
        loop {
            match self.rx.recv().await {
                Ok(val) => return Ok(val),
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Slow consumer. We will receive a message on the next call.
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return Err(RecvError::Closed),
            }
        }
    }

    pub fn try_recv(&mut self) -> Result<NodeEventInfo, TryRecvError> {
        loop {
            match self.rx.try_recv() {
                Ok(val) => return Ok(val),
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    // Slow consumer. We will receive a message on the next call.
                    continue;
                }
                Err(broadcast::error::TryRecvError::Empty) => return Err(TryRecvError::Empty),
                Err(broadcast::error::TryRecvError::Closed) => return Err(TryRecvError::Closed),
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeEventInfo {
    pub event: NodeEvent,
    #[cfg_attr(
        target_arch = "wasm32",
        serde(serialize_with = "serialize_system_time")
    )]
    pub time: SystemTime,
    pub file_path: &'static str,
    pub file_line: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum NodeEvent {
    SamplingStarted {
        height: u64,
        square_width: u16,
        shares: Vec<(u16, u16)>,
    },
    ShareSamplingResult {
        height: u64,
        square_width: u16,
        row: u16,
        column: u16,
        accepted: bool,
    },
    SamplingFinished {
        height: u64,
        accepted: bool,
        took: Duration,
    },
    FatalDaserError {
        error: String,
    },
}

impl fmt::Display for NodeEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NodeEvent::SamplingStarted {
                height,
                square_width,
                shares,
            } => {
                write!(f, "Sampling for {height} block started. Square: {square_width}x{square_width}, Shares: {shares:?}.")
            }
            NodeEvent::ShareSamplingResult {
                height,
                row,
                column,
                accepted,
                ..
            } => {
                let acc = if *accepted { "accepted" } else { "rejected" };
                write!(
                    f,
                    "Sampling for share [{row}, {column}] of {height} block was {acc}."
                )
            }
            NodeEvent::SamplingFinished {
                height,
                accepted,
                took,
            } => {
                let acc = if *accepted { "accepted" } else { "rejected" };
                write!(
                    f,
                    "Sampling for {height} block finished and {acc}. Took {took:?}."
                )
            }
            NodeEvent::FatalDaserError { error } => {
                write!(f, "Daser stopped because of a fatal error: {error}")
            }
        }
    }
}
