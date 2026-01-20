//! Transaction manager v2: queueing, submission, confirmation, and recovery logic.
//!
//! # Overview
//! - `TransactionManager` is a front-end that signs and enqueues transactions.
//! - `TransactionWorker` is a background event loop that submits and confirms.
//! - `TxServer` abstracts the submission/confirmation backend (one or more nodes).
//!
//! The worker maintains a contiguous in-order queue keyed by sequence and resolves
//! submit/confirm callbacks as transitions are observed.
//!
//! # Notes
//! - Sequence continuity is enforced at enqueue time; any gap is treated as fatal.
//! - Confirmations are batched per node and reduced to a small set of events.
//! - Recovery runs a narrow confirmation loop for a single node when sequence
//!   mismatch is detected.
//!
//! # Example
//! ```no_run
//! # use std::collections::HashMap;
//! # use std::sync::Arc;
//! # use std::time::Duration;
//! # use celestia_grpc::{Result, TxConfig};
//! # use celestia_grpc::tx_client_v2::{
//! #     TransactionWorker, TxRequest, TxServer, SignFn,
//! # };
//! # struct DummyServer;
//! # #[async_trait::async_trait]
//! # impl TxServer for DummyServer {
//! #     type TxId = u64;
//! #     type ConfirmInfo = u64;
//! #     async fn submit(&self, _b: Vec<u8>, _s: u64) -> Result<u64, _> { unimplemented!() }
//! #     async fn status_batch(
//! #         &self,
//! #         _ids: Vec<u64>,
//! #     ) -> Result<Vec<(u64, celestia_grpc::tx_client_v2::TxStatus<u64>)>> {
//! #         unimplemented!()
//! #     }
//! #     async fn current_sequence(&self) -> Result<u64> { unimplemented!() }
//! # }
//! # struct DummySigner;
//! # #[async_trait::async_trait]
//! # impl SignFn<u64, u64> for DummySigner {
//! #     async fn sign(
//! #         &self,
//! #         _sequence: u64,
//! #         _request: &TxRequest,
//! #         _cfg: &TxConfig,
//! #     ) -> Result<celestia_grpc::tx_client_v2::Transaction<u64, u64>> {
//! #         unimplemented!()
//! #     }
//! # }
//! # async fn docs() -> Result<()> {
//! let nodes = HashMap::from([(String::from("node-1"), Arc::new(DummyServer))]);
//! let signer = Arc::new(DummySigner);
//! let (manager, mut worker) = TransactionWorker::new(
//!     nodes,
//!     Duration::from_secs(1),
//!     16,
//!     signer,
//!     1,
//!     128,
//! );
//! # let _ = manager;
//! # let _ = worker;
//! # Ok(())
//! # }
//! ```
use std::collections::{HashMap, VecDeque};
use std::hash::Hash as StdHash;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use celestia_types::state::ErrorCode;
use futures::future::{BoxFuture, FutureExt};
use futures::stream::{FuturesUnordered, StreamExt};
use lumina_utils::executor::{spawn, spawn_cancellable};
use lumina_utils::time::{self, Interval};
use tendermint_proto::google::protobuf::Any;
use tendermint_proto::types::CanonicalVoteExtension;
use tokio::select;
use tokio::sync::{Mutex, Notify, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::{Error, Result, TxConfig};
/// Identifier for a submission/confirmation node.
pub type NodeId = String;
/// Result for submission calls: either a server TxId or a submission failure.
pub type TxSubmitResult<T> = Result<T, SubmitFailure>;
/// Result for confirmation calls.
pub type TxConfirmResult<T> = Result<T>;

pub trait TxIdT: Clone + std::fmt::Debug {}
impl<T> TxIdT for T where T: Clone + std::fmt::Debug {}

#[derive(Debug, Clone)]
pub enum TxRequest {
    /// Pay-for-blobs transaction.
    Blobs(Vec<celestia_types::Blob>),
    /// Arbitrary protobuf message (already encoded as `Any`).
    Message(Any),
    /// Raw bytes payload (used by tests or external signers).
    RawPayload(Vec<u8>),
}

#[derive(Debug)]
pub struct Transaction<TxId: TxIdT, ConfirmInfo> {
    /// Transaction sequence for the signer.
    pub sequence: u64,
    /// Signed transaction bytes ready for broadcast.
    pub bytes: Vec<u8>,
    /// One-shot callbacks for submit/confirm acknowledgements.
    pub callbacks: TxCallbacks<TxId, ConfirmInfo>,
    /// Id of the transaction
    pub id: TxId,
}

#[derive(Debug)]
pub struct TxCallbacks<TxId: TxIdT, ConfirmInfo> {
    /// Resolves when submission succeeds or fails.
    pub on_submit: Option<oneshot::Sender<Result<TxId>>>,
    /// Resolves when the transaction is confirmed or rejected.
    pub on_confirm: Option<oneshot::Sender<Result<ConfirmInfo>>>,
}

impl<TxId: TxIdT, ConfirmInfo> Default for TxCallbacks<TxId, ConfirmInfo> {
    fn default() -> Self {
        Self {
            on_submit: None,
            on_confirm: None,
        }
    }
}

#[derive(Debug)]
pub struct TxHandle<TxId: TxIdT, ConfirmInfo> {
    /// Sequence reserved for this transaction.
    pub sequence: u64,
    /// Receives submit result.
    pub on_submit: oneshot::Receiver<Result<TxId>>,
    /// Receives confirm result.
    pub on_confirm: oneshot::Receiver<Result<ConfirmInfo>>,
}

#[async_trait]
pub trait SignFn<TxId: TxIdT, ConfirmInfo>: Send + Sync {
    /// Produce a signed `Transaction` for the provided request and sequence.
    ///
    /// # Notes
    /// - The returned `Transaction.sequence` must match the input `sequence`.
    /// - Returning a mismatched sequence causes the caller to fail the enqueue.
    async fn sign(
        &self,
        sequence: u64,
        request: &TxRequest,
        cfg: &TxConfig,
    ) -> Result<Transaction<TxId, ConfirmInfo>>;
}

#[derive(Clone)]
pub struct TransactionManager<TxId: TxIdT, ConfirmInfo> {
    add_tx: mpsc::Sender<Transaction<TxId, ConfirmInfo>>,
    next_sequence: Arc<Mutex<u64>>,
    max_sent: Arc<AtomicU64>,
    signer: Arc<dyn SignFn<TxId, ConfirmInfo>>,
}

impl<TxId: TxIdT, ConfirmInfo> TransactionManager<TxId, ConfirmInfo> {
    /// Sign and enqueue a transaction, returning a handle for submit/confirm.
    ///
    /// # Notes
    /// - Sequence reservation is serialized with a mutex.
    /// - If the queue is full or closed, the sequence is not incremented.
    ///
    /// # Example
    /// ```no_run
    /// # use celestia_grpc::{TxConfig, Result};
    /// # use celestia_grpc::tx_client_v2::{TransactionManager, TxRequest};
    /// # async fn docs(manager: TransactionManager<u64, u64>) -> Result<()> {
    /// let handle = manager
    ///     .add_tx(TxRequest::RawPayload(vec![1, 2, 3]), TxConfig::default())
    ///     .await?;
    /// handle.on_submit.await?;
    /// handle.on_confirm.await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn add_tx(
        &self,
        request: TxRequest,
        cfg: TxConfig,
    ) -> Result<TxHandle<TxId, ConfirmInfo>> {
        let mut sequence = self.next_sequence.lock().await;
        let current = *sequence;
        let mut tx = self.signer.sign(current, &request, &cfg).await?;
        if tx.sequence != current {
            return Err(Error::UnexpectedResponseType(format!(
                "tx sequence mismatch: expected {}, got {}",
                current, tx.sequence
            )));
        }
        let (submit_tx, submit_rx) = oneshot::channel();
        let (confirm_tx, confirm_rx) = oneshot::channel();
        tx.callbacks.on_submit = Some(submit_tx);
        tx.callbacks.on_confirm = Some(confirm_tx);
        match self.add_tx.try_send(tx) {
            Ok(()) => {
                *sequence = sequence.saturating_add(1);
                self.max_sent.fetch_add(1, Ordering::Relaxed);
                Ok(TxHandle {
                    sequence: current,
                    on_submit: submit_rx,
                    on_confirm: confirm_rx,
                })
            }
            Err(mpsc::error::TrySendError::Full(_)) => Err(Error::UnexpectedResponseType(
                "transaction queue full".to_string(),
            )),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(Error::UnexpectedResponseType(
                "transaction manager closed".to_string(),
            )),
        }
    }

    /// Return the maximum sequence successfully enqueued so far.
    #[allow(dead_code)]
    pub fn max_sent(&self) -> u64 {
        self.max_sent.load(Ordering::Relaxed)
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct TxIndexEntry<TxId: TxIdT> {
    node_id: NodeId,
    sequence: u64,
    id: TxId,
}

#[derive(Debug, Default)]
struct NodeSubmissionState {
    submitted_seq: u64,
    inflight: bool,
    confirm_inflight: bool,
    delay: Option<Duration>,
}

#[derive(Debug)]
pub enum TxStatus<ConfirmInfo> {
    /// Submitted, but not yet committed.
    Pending,
    /// Included in a block successfully with confirmation info.
    Confirmed { info: ConfirmInfo },
    /// Rejected by the node with a specific reason.
    Rejected { reason: RejectionReason },
    /// Removed from mempool; may need resubmission.
    Evicted,
    /// Status could not be determined.
    Unknown,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SubmitFailure {
    /// Server expects a different sequence.
    SequenceMismatch { expected: u64 },
    /// Transaction failed validation.
    InvalidTx { error_code: ErrorCode },
    /// Account has insufficient funds.
    InsufficientFunds,
    /// Fee too low for the computed gas price.
    InsufficientFee { expected_fee: u64 },
    /// Transport or RPC error while submitting.
    NetworkError { err: Arc<Error> },
    /// Node mempool is full.
    MempoolIsFull,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ConfirmFailure {
    reason: RejectionReason,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum RejectionReason {
    SequenceMismatch {
        expected: u64,
        node_id: NodeId,
    },
    OtherReason {
        error_code: ErrorCode,
        message: String,
        node_id: NodeId,
    },
}

#[async_trait]
pub trait TxServer: Send + Sync {
    type TxId: TxIdT + Clone + Eq + StdHash + Send + Sync + 'static;
    type ConfirmInfo: Send + Sync + 'static;

    /// Submit signed bytes with the given sequence, returning a server TxId.
    async fn submit(&self, tx_bytes: Vec<u8>, sequence: u64) -> TxSubmitResult<Self::TxId>;
    /// Batch status lookup for submitted TxIds.
    async fn status_batch(
        &self,
        ids: Vec<Self::TxId>,
    ) -> TxConfirmResult<Vec<(Self::TxId, TxStatus<Self::ConfirmInfo>)>>;
    /// Status lookup for submitted TxId.
    async fn status(&self, id: Self::TxId) -> TxConfirmResult<TxStatus<Self::ConfirmInfo>>;
    /// Fetch current sequence for the account (used by some implementations).
    async fn current_sequence(&self) -> Result<u64>;
}

#[derive(Debug)]
enum TransactionEvent<TxId, ConfirmInfo> {
    Submitted {
        node_id: NodeId,
        sequence: u64,
        id: TxId,
    },
    SubmitFailed {
        node_id: NodeId,
        sequence: u64,
        failure: SubmitFailure,
    },
    StatusBatch {
        node_id: NodeId,
        statuses: Vec<(TxId, TxStatus<ConfirmInfo>)>,
    },
    RecoverStatus {
        node_id: NodeId,
        id: TxId,
        status: TxStatus<ConfirmInfo>,
    },
}

#[derive(Debug, Clone)]
enum ProcessorState {
    Recovering { node_id: NodeId, expected: u64 },
    Submitting,
    Stopped(StopReason),
}

impl ProcessorState {
    fn transition(&self, other: ProcessorState) -> ProcessorState {
        match (self, other) {
            (ProcessorState::Stopped(_stop_reason), _) => self.clone(),
            (ProcessorState::Recovering { .. }, ProcessorState::Stopped(stop_reason)) => {
                ProcessorState::Stopped(stop_reason)
            }
            (ProcessorState::Recovering { .. }, _) => self.clone(),
            (_, other) => other,
        }
    }

    #[allow(dead_code)]
    fn is_stopped(&self) -> bool {
        matches!(self, ProcessorState::Stopped(_))
    }

    fn is_recovering(&self) -> bool {
        matches!(self, ProcessorState::Recovering { .. })
    }

    #[allow(dead_code)]
    fn is_submitting(&self) -> bool {
        matches!(self, ProcessorState::Submitting)
    }

    fn equivalent(&self, other: &ProcessorState) -> bool {
        match (self, other) {
            (ProcessorState::Submitting, ProcessorState::Submitting) => true,
            (ProcessorState::Recovering { .. }, ProcessorState::Recovering { .. }) => true,
            (ProcessorState::Stopped(_), ProcessorState::Stopped(_)) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
struct ProcessorStateWithEpoch {
    state: ProcessorState,
    epoch: u64,
}

impl ProcessorStateWithEpoch {
    fn new(state: ProcessorState) -> Self {
        Self { state, epoch: 0 }
    }

    fn epoch(&self) -> u64 {
        self.epoch
    }

    fn as_ref(&self) -> &ProcessorState {
        &self.state
    }

    fn snapshot(&self) -> ProcessorState {
        self.state.clone()
    }

    fn update(&mut self, other: ProcessorState) {
        let next = self.state.transition(other);
        self.set_if_changed(next);
    }

    fn force_update(&mut self, other: ProcessorState) {
        self.set_if_changed(other);
    }

    fn set_if_changed(&mut self, next: ProcessorState) {
        if !self.state.equivalent(&next) {
            self.state = next;
            self.epoch = self.epoch.saturating_add(1);
        }
    }

    fn is_stopped(&self) -> bool {
        self.state.is_stopped()
    }

    fn is_recovering(&self) -> bool {
        self.state.is_recovering()
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum StopReason {
    SubmitFailure(SubmitFailure),
    ConfirmFailure(ConfirmFailure),
    Shutdown,
}

impl From<SubmitFailure> for StopReason {
    fn from(value: SubmitFailure) -> Self {
        StopReason::SubmitFailure(value)
    }
}

impl From<ConfirmFailure> for StopReason {
    fn from(value: ConfirmFailure) -> Self {
        StopReason::ConfirmFailure(value)
    }
}

fn recovery_expected(reason: &RejectionReason) -> Option<u64> {
    match reason {
        RejectionReason::SequenceMismatch { expected, .. } => Some(*expected),
        _ => None,
    }
}

struct SubmissionResult<TxId> {
    node_id: NodeId,
    sequence: u64,
    result: TxSubmitResult<TxId>,
}

enum ConfirmationResponse<TxId, ConfirmInfo> {
    Recovering {
        id: TxId,
        status: TxStatus<ConfirmInfo>,
    },
    Submitting(Vec<(TxId, TxStatus<ConfirmInfo>)>),
}

struct ConfirmationResult<TxId, ConfirmInfo> {
    node_id: NodeId,
    state_epoch: u64,
    response: TxConfirmResult<ConfirmationResponse<TxId, ConfirmInfo>>,
}

pub struct TransactionWorker<S: TxServer> {
    nodes: HashMap<NodeId, Arc<S>>,
    txs: VecDeque<Transaction<S::TxId, S::ConfirmInfo>>,
    tx_index: HashMap<S::TxId, u64>,
    events: VecDeque<TransactionEvent<S::TxId, S::ConfirmInfo>>,
    confirmed_info: HashMap<u64, S::ConfirmInfo>,
    confirmed_sequence: u64,
    next_enqueue_sequence: u64,
    node_state: HashMap<NodeId, NodeSubmissionState>,
    add_tx_rx: mpsc::Receiver<Transaction<S::TxId, S::ConfirmInfo>>,
    new_submit: Arc<Notify>,
    submissions: FuturesUnordered<BoxFuture<'static, Option<SubmissionResult<S::TxId>>>>,
    confirmations:
        FuturesUnordered<BoxFuture<'static, Option<ConfirmationResult<S::TxId, S::ConfirmInfo>>>>,
    confirm_ticker: Interval,
    confirm_interval: Duration,
    max_status_batch: usize,
    state: ProcessorStateWithEpoch,
}

impl<S: TxServer + 'static> TransactionWorker<S> {
    /// Create a manager/worker pair with initial sequence and queue capacity.
    ///
    /// # Notes
    /// - `start_sequence` should be the next sequence to submit for the signer.
    /// - `confirm_interval` drives periodic confirmation polling.
    pub fn new(
        nodes: HashMap<NodeId, Arc<S>>,
        confirm_interval: Duration,
        max_status_batch: usize,
        signer: Arc<dyn SignFn<S::TxId, S::ConfirmInfo>>,
        start_sequence: u64,
        add_tx_capacity: usize,
    ) -> (TransactionManager<S::TxId, S::ConfirmInfo>, Self) {
        let (add_tx_tx, add_tx_rx) = mpsc::channel(add_tx_capacity);
        let next_sequence = Arc::new(Mutex::new(start_sequence));
        let max_sent = Arc::new(AtomicU64::new(0));
        let manager = TransactionManager {
            add_tx: add_tx_tx,
            next_sequence,
            max_sent,
            signer,
        };
        let node_state = nodes
            .keys()
            .map(|node_id| (node_id.clone(), NodeSubmissionState::default()))
            .collect();
        (
            manager,
            TransactionWorker {
                add_tx_rx,
                nodes,
                txs: VecDeque::new(),
                tx_index: HashMap::new(),
                events: VecDeque::new(),
                confirmed_info: HashMap::new(),
                confirmed_sequence: start_sequence.saturating_sub(1),
                next_enqueue_sequence: start_sequence,
                node_state,
                new_submit: Arc::new(Notify::new()),
                submissions: FuturesUnordered::new(),
                confirmations: FuturesUnordered::new(),
                confirm_ticker: Interval::new(confirm_interval),
                state: ProcessorStateWithEpoch::new(ProcessorState::Submitting),
                confirm_interval,
                max_status_batch,
            },
        )
    }

    fn enqueue_tx(&mut self, tx: Transaction<S::TxId, S::ConfirmInfo>) -> Result<()> {
        if tx.sequence != self.next_enqueue_sequence {
            return Err(crate::Error::UnexpectedResponseType(format!(
                "tx sequence gap: expected {}, got {}",
                self.next_enqueue_sequence, tx.sequence
            )));
        }
        let id = tx.id.clone();
        let sequence = tx.sequence;
        self.txs.push_back(tx);
        self.tx_index.insert(id, sequence);
        self.next_enqueue_sequence += 1;
        self.new_submit.notify_one();
        Ok(())
    }

    /// Run the worker loop until shutdown or a terminal error.
    ///
    /// # Notes
    /// - The worker is single-owner; it should be run in a dedicated task.
    /// - On terminal failures it returns `Ok(())` after notifying callbacks.
    pub async fn process(&mut self, shutdown: CancellationToken) -> Result<()> {
        loop {
            while let Some(event) = self.events.pop_front() {
                self.process_event(event).await?;
            }

            select! {
                _ = shutdown.cancelled() => self.state.update(ProcessorState::Stopped(StopReason::Shutdown)),
                res = async {
                    match self.state.snapshot() {
                        ProcessorState::Recovering { node_id, expected } => {
                            self.run_recovering(shutdown.clone(), &node_id, expected).await?;
                        }
                        ProcessorState::Submitting => {
                            self.run_submitting(shutdown.clone()).await?;
                        }
                        _ => (),
                    }
                    Ok::<_, Error>(())
                } => res?,
            }
            if self.state.is_stopped() {
                break;
            }
        }

        // Wait for all in-flight tasks to complete before returning
        self.drain_pending().await;

        Ok(())
    }

    /// Wait for all in-flight submissions and confirmations to complete.
    ///
    /// Call this before dropping the worker if you need to ensure all spawned
    /// tasks have finished. Note: this will block until all pending network
    /// requests complete, so ensure your servers are responsive.
    pub async fn drain_pending(&mut self) {
        while self.submissions.next().await.is_some() {}
        while self.confirmations.next().await.is_some() {}
    }

    async fn run_recovering(
        &mut self,
        token: CancellationToken,
        node_id: &NodeId,
        expected: u64,
    ) -> Result<()> {
        let start = self.confirmed_sequence.saturating_add(1);
        if expected < start {
            self.state.force_update(ProcessorState::Submitting);
            return Ok(());
        }

        select! {
            _ = self.confirm_ticker.tick() => {
                let target = expected.saturating_sub(1);
                if let Some(idx) = self.index_for_sequence(target) {
                    let node_id = node_id.clone();
                    let node = self.nodes.get(&node_id).unwrap().clone();
                    let id = self.txs[idx].id.clone();
                    let state_epoch = self.state.epoch();
                    let tx = push_oneshot(&mut self.confirmations);
                    spawn_cancellable(token, async move {
                        let response = node.status(id.clone()).await.map(|status| {
                            ConfirmationResponse::Recovering {
                                id,
                                status,
                            }
                        });
                        let _ = tx.send(ConfirmationResult {
                            node_id,
                            response,
                            state_epoch,
                        });
                    });
                }
            }
            result = self.confirmations.next() => {
                let Some(Some(result)) = result else {
                    return Ok(());
                };
                if result.state_epoch != self.state.epoch() {
                    return Ok(());
                }
                match result.response {
                    Ok(ConfirmationResponse::Recovering { id, status }) => {
                        self.events.push_back(TransactionEvent::RecoverStatus { node_id: result.node_id, id, status });
                    }
                    Err(_err) => {
                        // TODO: add error handling
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    async fn run_submitting(&mut self, token: CancellationToken) -> Result<()> {
        select! {
            tx = self.add_tx_rx.recv() => {
                if let Some(tx) = tx {
                    self.enqueue_tx(tx)?;
                }
            }
            _ = self.new_submit.notified() => {
                self.spawn_submissions(token.clone());
            }
            _ = self.confirm_ticker.tick() => {
                self.spawn_confirmations(token.clone());
            }
            result = self.submissions.next() => {
                if let Some(Some(result)) = result {
                    self.events.push_back(match result.result {
                        Ok(id) => TransactionEvent::Submitted {
                            node_id: result.node_id,
                            sequence: result.sequence,
                            id,
                        },
                        Err(failure) => TransactionEvent::SubmitFailed {
                            node_id: result.node_id,
                            sequence: result.sequence,
                            failure,
                        },
                    });
                }
            }
            result = self.confirmations.next() => {
                let Some(Some(result)) = result else {
                    return Ok(());
                };
                if result.state_epoch != self.state.epoch() {
                    return Ok(());
                }
                match result.response {
                    Ok(ConfirmationResponse::Submitting(statuses)) => {
                        self.events.push_back(TransactionEvent::StatusBatch {
                            node_id: result.node_id,
                            statuses,
                        });
                    }
                    Err(_err) => {
                        // TODO: add error handling
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    // either confirmation or submission or
    async fn process_event(
        &mut self,
        event: TransactionEvent<S::TxId, S::ConfirmInfo>,
    ) -> Result<()> {
        match event {
            TransactionEvent::SubmitFailed {
                node_id,
                sequence,
                failure,
                ..
            } => {
                let Some(state) = self.node_state.get_mut(&node_id) else {
                    return Ok(());
                };
                state.inflight = false;
                state.submitted_seq = sequence.saturating_sub(1);
                match failure {
                    SubmitFailure::SequenceMismatch { expected } => {
                        if sequence < expected {
                            self.state
                                .update(ProcessorState::Recovering { node_id, expected });
                        } else {
                            if let Some(state) = self.node_state.get_mut(&node_id) {
                                state.submitted_seq = expected.saturating_sub(1);
                            }
                            self.state.update(ProcessorState::Submitting);
                        }
                    }
                    SubmitFailure::MempoolIsFull => {
                        state.delay = Some(self.confirm_interval);
                    }
                    SubmitFailure::NetworkError { err: _ } => {
                        state.delay = Some(self.confirm_interval);
                    }
                    _ => {
                        self.state.update(ProcessorState::Stopped(failure.into()));
                    }
                };
                self.new_submit.notify_one();
            }
            TransactionEvent::Submitted {
                node_id,
                sequence,
                id,
            } => {
                let state_node_id = node_id.clone();
                let submit_id = id.clone();
                // TODO: refactor to make it inside func
                if let Some(on_submit) = self.take_on_submit(sequence) {
                    let _ = on_submit.send(Ok(submit_id));
                }
                if let Some(state) = self.node_state.get_mut(&state_node_id) {
                    state.inflight = false;
                }
                self.new_submit.notify_one();
            }
            TransactionEvent::StatusBatch { node_id, statuses } => {
                let state = self.node_state.get_mut(&node_id).unwrap();
                state.confirm_inflight = false;
                self.process_status_batch(node_id, statuses)?;
            }
            TransactionEvent::RecoverStatus {
                node_id,
                id,
                status,
            } => {
                let state = self.node_state.get_mut(&node_id).unwrap();
                state.confirm_inflight = false;
                self.process_status_recovering(node_id, id, status)?;
            }
        }
        Ok(())
    }

    fn process_status_batch(
        &mut self,
        node_id: NodeId,
        statuses: Vec<(S::TxId, TxStatus<S::ConfirmInfo>)>,
    ) -> Result<()> {
        let mut collected = statuses
            .into_iter()
            .map(|(tx_id, status)| (self.tx_index.get(&tx_id).unwrap().clone(), status))
            .collect::<Vec<_>>();
        collected.sort_by(|first, second| first.0.cmp(&second.0));
        for (cur_seq, status) in collected.into_iter() {
            match status {
                TxStatus::Pending => {
                    continue;
                }
                TxStatus::Evicted => {
                    let node_state = self.node_state.get_mut(&node_id).unwrap();
                    node_state.submitted_seq =
                        node_state.submitted_seq.min(cur_seq.saturating_sub(1));
                    self.new_submit.notify_one();
                    break;
                }
                TxStatus::Rejected { reason } => match reason {
                    RejectionReason::SequenceMismatch {
                        expected,
                        node_id: _,
                    } => {
                        if cur_seq < expected {
                            self.state.update(ProcessorState::Recovering {
                                node_id: node_id.clone(),
                                expected,
                            });
                        } else {
                            let node_state = self.node_state.get_mut(&node_id).unwrap();
                            node_state.submitted_seq = node_state.submitted_seq.min(expected);
                        }
                        break;
                    }
                    other => {
                        self.state
                            .update(ProcessorState::Stopped(StopReason::ConfirmFailure(
                                ConfirmFailure { reason: other },
                            )));
                    }
                },
                TxStatus::Confirmed { info } => {
                    self.update_confirmed_sequence(cur_seq, info)?;
                }
                TxStatus::Unknown => {
                    self.state
                        .update(ProcessorState::Stopped(StopReason::ConfirmFailure(
                            ConfirmFailure {
                                reason: RejectionReason::OtherReason {
                                    error_code: ErrorCode::UnknownRequest,
                                    message: "transaction status unknown".to_string(),
                                    node_id: node_id.clone(),
                                },
                            },
                        )));
                }
            }
        }
        Ok(())
    }

    fn process_status_recovering(
        &mut self,
        node_id: NodeId,
        id: S::TxId,
        status: TxStatus<S::ConfirmInfo>,
    ) -> Result<()> {
        let seq = self.tx_index.get(&id).unwrap();
        match status {
            TxStatus::Confirmed { info } => {
                self.update_confirmed_sequence(*seq, info)?;
            }
            _ => {
                self.state
                    .update(ProcessorState::Stopped(StopReason::ConfirmFailure(
                        ConfirmFailure {
                            reason: RejectionReason::SequenceMismatch {
                                expected: *seq,
                                node_id,
                            },
                        },
                    )));
            }
        }
        Ok(())
    }

    fn update_confirmed_sequence(&mut self, sequence: u64, info: S::ConfirmInfo) -> Result<()> {
        if sequence <= self.confirmed_sequence {
            return Ok(());
        }
        self.confirmed_info.insert(sequence, info);
        for (_, state) in self.node_state.iter_mut() {
            state.submitted_seq = state.submitted_seq.max(sequence);
        }
        self.confirmed_sequence = sequence;
        self.trim_confirmed_queue();
        Ok(())
    }

    fn trim_confirmed_queue(&mut self) {
        while let Some(front) = self.txs.front() {
            if front.sequence > self.confirmed_sequence {
                break;
            }
            let mut tx = self.txs.pop_front().expect("front exists");
            self.tx_index.remove(&tx.id);
            if let Some(info) = self.confirmed_info.remove(&tx.sequence) {
                if let Some(on_confirm) = tx.callbacks.on_confirm.take() {
                    let _ = on_confirm.send(Ok(info));
                }
            }
        }
    }

    fn spawn_submissions(&mut self, token: CancellationToken) {
        if self.nodes.is_empty() {
            return;
        }
        for (node_id, node) in self.nodes.clone() {
            let (target_sequence, delay) = {
                let state = self.node_state.entry(node_id.clone()).or_default();
                if state.inflight {
                    continue;
                }
                let delay = if let Some(delay) = state.delay {
                    state.delay = None;
                    delay
                } else {
                    Duration::from_nanos(0)
                };
                if state.submitted_seq < self.confirmed_sequence {
                    state.submitted_seq = self.confirmed_sequence;
                }
                (state.submitted_seq.saturating_add(1), delay)
            };
            let Some(bytes) = self.peek_submission(target_sequence) else {
                continue;
            };
            if let Some(state) = self.node_state.get_mut(&node_id) {
                state.submitted_seq = target_sequence;
                state.inflight = true;
            }
            let tx = push_oneshot(&mut self.submissions);
            spawn_cancellable(token.clone(), async move {
                time::sleep(delay).await;
                let result = node.submit(bytes.clone(), target_sequence).await;
                let _ = tx.send(SubmissionResult {
                    node_id,
                    sequence: target_sequence,
                    result,
                });
            });
        }
    }

    fn spawn_confirmations(&mut self, token: CancellationToken) {
        if self.tx_index.is_empty() {
            return;
        }
        let state_epoch = self.state.epoch();
        // gathering all the sequences for each node
        for (node_id, state) in self.node_state.iter_mut() {
            if state.confirm_inflight {
                continue;
            }
            let sub_seq = state.submitted_seq;
            let node_id = node_id.clone();
            let total = sub_seq - self.confirmed_sequence;
            let cap = self.max_status_batch.min(total as usize);
            if cap == 0 {
                continue;
            }
            let mut to_send = Vec::with_capacity(cap);
            for i in 0..cap {
                to_send.push(self.txs[i].id.clone());
            }
            let node = self.nodes.get(&node_id).unwrap().clone();
            let tx = push_oneshot(&mut self.confirmations);
            spawn_cancellable(token.clone(), async move {
                let response = node
                    .status_batch(to_send)
                    .await
                    .map(|status| ConfirmationResponse::Submitting(status));
                let _ = tx.send(ConfirmationResult {
                    state_epoch,
                    node_id,
                    response,
                });
            });
        }
    }

    fn index_for_sequence(&self, sequence: u64) -> Option<usize> {
        // checking the first not confirmed sequence
        let start = self.confirmed_sequence.saturating_add(1);
        if sequence < start {
            return None;
        }
        let offset = sequence - start;
        if offset >= self.txs.len() as u64 {
            return None;
        }
        Some(offset as usize)
    }

    fn peek_submission(&mut self, sequence: u64) -> Option<Vec<u8>> {
        let idx = self.index_for_sequence(sequence)?;
        let tx = self.txs.get_mut(idx)?;
        Some(tx.bytes.clone())
    }

    fn take_on_submit(&mut self, sequence: u64) -> Option<oneshot::Sender<Result<S::TxId>>> {
        let idx = self.index_for_sequence(sequence)?;
        let tx = self.txs.get_mut(idx)?;
        tx.callbacks.on_submit.take()
    }

    #[allow(dead_code)]
    fn take_on_confirm(
        &mut self,
        sequence: u64,
    ) -> Option<oneshot::Sender<Result<S::ConfirmInfo>>> {
        let idx = self.index_for_sequence(sequence)?;
        let tx = self.txs.get_mut(idx)?;
        tx.callbacks.on_confirm.take()
    }
}

fn push_oneshot<T: 'static + Send>(
    unordered: &mut FuturesUnordered<BoxFuture<'static, Option<T>>>,
) -> oneshot::Sender<T> {
    let (tx, rx) = oneshot::channel();
    unordered.push(async move { rx.await.ok() }.boxed());
    tx
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use tokio::sync::mpsc;
    use tokio::sync::mpsc::error::TryRecvError;
    use tokio::task::JoinHandle;

    type TestTxId = u64;
    type TestConfirmInfo = u64;

    #[derive(Debug)]
    enum ServerCall {
        Submit {
            bytes: Vec<u8>,
            sequence: u64,
            reply: oneshot::Sender<TxSubmitResult<TestTxId>>,
        },
        StatusBatch {
            ids: Vec<TestTxId>,
            reply: oneshot::Sender<TxConfirmResult<Vec<(TestTxId, TxStatus<TestConfirmInfo>)>>>,
        },
        CurrentSequence {
            reply: oneshot::Sender<Result<u64>>,
        },
    }

    #[derive(Debug)]
    struct MockTxServer {
        calls: mpsc::Sender<ServerCall>,
    }

    impl MockTxServer {
        fn new(calls: mpsc::Sender<ServerCall>) -> Self {
            Self { calls }
        }
    }

    #[derive(Default)]
    struct TestSigner;

    #[async_trait]
    impl SignFn<TestTxId, TestConfirmInfo> for TestSigner {
        async fn sign(
            &self,
            sequence: u64,
            request: &TxRequest,
            _cfg: &TxConfig,
        ) -> Result<Transaction<TestTxId, TestConfirmInfo>> {
            let bytes = match request {
                TxRequest::RawPayload(bytes) => bytes.clone(),
                TxRequest::Message(_) | TxRequest::Blobs(_) => Vec::new(),
            };
            Ok(Transaction {
                sequence,
                bytes,
                callbacks: TxCallbacks::default(),
                id: sequence,
            })
        }
    }

    #[async_trait]
    impl TxServer for MockTxServer {
        type TxId = TestTxId;
        type ConfirmInfo = TestConfirmInfo;

        async fn submit(&self, tx_bytes: Vec<u8>, sequence: u64) -> TxSubmitResult<TestTxId> {
            let (reply, rx) = oneshot::channel();
            self.calls
                .send(ServerCall::Submit {
                    bytes: tx_bytes,
                    sequence,
                    reply,
                })
                .await
                .expect("submit call");
            rx.await.expect("submit reply")
        }

        async fn status_batch(
            &self,
            ids: Vec<TestTxId>,
        ) -> TxConfirmResult<Vec<(TestTxId, TxStatus<TestConfirmInfo>)>> {
            let (reply, rx) = oneshot::channel();
            self.calls
                .send(ServerCall::StatusBatch { ids, reply })
                .await
                .expect("status batch call");
            rx.await.expect("status batch reply")
        }

        async fn status(&self, id: Self::TxId) -> TxConfirmResult<TxStatus<Self::ConfirmInfo>> {
            todo!()
        }

        async fn current_sequence(&self) -> Result<u64> {
            let (reply, rx) = oneshot::channel();
            self.calls
                .send(ServerCall::CurrentSequence { reply })
                .await
                .expect("current sequence call");
            rx.await.expect("current sequence reply")
        }
    }

    struct Harness {
        calls_rx: mpsc::Receiver<ServerCall>,
        shutdown: CancellationToken,
        handle: Option<JoinHandle<Result<()>>>,
        confirm_interval: Duration,
        manager: TransactionManager<TestTxId, TestConfirmInfo>,
    }

    impl Harness {
        /// Maximum spins for polling operations.
        const MAX_SPINS: usize = 1000;

        fn new(
            confirm_interval: Duration,
            max_status_batch: usize,
            add_tx_capacity: usize,
        ) -> (Self, TransactionWorker<MockTxServer>) {
            let (calls_tx, calls_rx) = mpsc::channel(64);
            let server = Arc::new(MockTxServer::new(calls_tx));
            let nodes = HashMap::from([(String::from("node-1"), server)]);
            let signer = Arc::new(TestSigner::default());
            let (manager, worker) = TransactionWorker::new(
                nodes,
                confirm_interval,
                max_status_batch,
                signer,
                1,
                add_tx_capacity,
            );
            (
                Self {
                    calls_rx,
                    shutdown: CancellationToken::new(),
                    handle: None,
                    confirm_interval,
                    manager,
                },
                worker,
            )
        }

        fn start(&mut self, mut manager: TransactionWorker<MockTxServer>) {
            let shutdown = self.shutdown.clone();

            // CRITICAL FIX: Inject sentinel futures to prevent busy-loop.
            //
            // Problem: When FuturesUnordered is empty, next() returns Ready(None) immediately.
            // This causes the worker's select! to complete without yielding, creating a busy
            // loop that starves spawned tasks from ever running.
            //
            // Solution: Add sentinel futures that stay Pending until shutdown. This makes
            // next() return Pending (not Ready(None)) when there's no real work, allowing
            // the worker to yield and let spawned tasks run.
            let shutdown_for_submissions = shutdown.clone();
            manager.submissions.push(
                async move {
                    shutdown_for_submissions.cancelled().await;
                    None
                }
                .boxed(),
            );

            let shutdown_for_confirmations = shutdown.clone();
            manager.confirmations.push(
                async move {
                    shutdown_for_confirmations.cancelled().await;
                    None
                }
                .boxed(),
            );

            self.handle = Some(tokio::spawn(async move { manager.process(shutdown).await }));
        }

        async fn pump(&self) {
            // Yield to let other tasks run
            tokio::task::yield_now().await;
        }

        async fn expect_call(&mut self) -> ServerCall {
            // Poll for server calls, yielding between attempts to let worker make progress
            for _ in 0..Self::MAX_SPINS {
                match self.calls_rx.try_recv() {
                    Ok(call) => return call,
                    Err(TryRecvError::Disconnected) => panic!("server call channel closed"),
                    Err(TryRecvError::Empty) => self.pump().await,
                }
            }
            panic!(
                "expected server call, got none after {} spins",
                Self::MAX_SPINS
            );
        }

        async fn expect_submit(
            &mut self,
        ) -> (Vec<u8>, u64, oneshot::Sender<TxSubmitResult<TestTxId>>) {
            loop {
                match self.expect_call().await {
                    ServerCall::Submit {
                        bytes,
                        sequence,
                        reply,
                    } => return (bytes, sequence, reply),
                    ServerCall::StatusBatch { ids, reply } => {
                        let statuses = ids.into_iter().map(|id| (id, TxStatus::Pending)).collect();
                        let _ = reply.send(Ok(statuses));
                        self.pump().await;
                    }
                    other => panic!("expected Submit, got {:?}", other),
                }
            }
        }

        async fn expect_status_batch(
            &mut self,
        ) -> (
            Vec<TestTxId>,
            oneshot::Sender<TxConfirmResult<Vec<(TestTxId, TxStatus<TestConfirmInfo>)>>>,
        ) {
            loop {
                match self.expect_call().await {
                    ServerCall::StatusBatch { ids, reply } => return (ids, reply),
                    ServerCall::Submit {
                        bytes: _,
                        sequence,
                        reply,
                    } => {
                        let _ = reply.send(Ok(sequence));
                        self.pump().await;
                    }
                    other => panic!("expected StatusBatch, got {:?}", other),
                }
            }
        }

        fn assert_no_calls(&mut self) {
            match self.calls_rx.try_recv() {
                Ok(call) => panic!("unexpected server call: {:?}", call),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => panic!("server call channel closed"),
            }
        }

        async fn drain_status_batches_pending(&mut self) {
            // Drain any pending calls, yielding to let worker process them
            for _ in 0..Self::MAX_SPINS {
                match self.calls_rx.try_recv() {
                    Ok(ServerCall::StatusBatch { ids, reply }) => {
                        let statuses = ids.into_iter().map(|id| (id, TxStatus::Pending)).collect();
                        let _ = reply.send(Ok(statuses));
                        self.pump().await;
                    }
                    Ok(ServerCall::Submit {
                        sequence, reply, ..
                    }) => {
                        let _ = reply.send(Ok(sequence));
                        self.pump().await;
                    }
                    Ok(ServerCall::CurrentSequence { reply }) => {
                        let _ = reply.send(Ok(1));
                        self.pump().await;
                    }
                    Err(TryRecvError::Empty) => {
                        self.pump().await;
                    }
                    Err(TryRecvError::Disconnected) => break,
                }
            }
        }

        fn assert_no_calls_or_closed(&mut self) {
            match self.calls_rx.try_recv() {
                Ok(call) => panic!("unexpected server call: {:?}", call),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => {}
            }
        }

        async fn tick_confirm(&self) {
            // Advance mock time to trigger confirm ticker
            tokio::time::advance(self.confirm_interval + Duration::from_nanos(1)).await;
            self.pump().await;
        }

        async fn shutdown(mut self) {
            self.shutdown.cancel();

            let mut handle = self.handle.take().expect("manager started");

            // Use select to respond to mock calls while waiting for worker to finish
            loop {
                select! {
                    biased;
                    result = &mut handle => {
                        let _ = result;
                        break;
                    }
                    call = self.calls_rx.recv() => {
                        match call {
                            Some(ServerCall::Submit { reply, sequence, .. }) => {
                                let _ = reply.send(Ok(sequence));
                            }
                            Some(ServerCall::StatusBatch { ids, reply }) => {
                                let statuses =
                                    ids.into_iter().map(|id| (id, TxStatus::Pending)).collect();
                                let _ = reply.send(Ok(statuses));
                            }
                            Some(ServerCall::CurrentSequence { reply }) => {
                                let _ = reply.send(Ok(1));
                            }
                            None => break, // Channel closed
                        }
                    }
                }
            }
        }
    }

    async fn add_tx(
        manager: &TransactionManager<TestTxId, TestConfirmInfo>,
        bytes: Vec<u8>,
    ) -> (
        u64,
        oneshot::Receiver<Result<TestTxId>>,
        oneshot::Receiver<Result<TestConfirmInfo>>,
    ) {
        let handle = manager
            .add_tx(TxRequest::RawPayload(bytes), TxConfig::default())
            .await
            .expect("add tx");
        (handle.sequence, handle.on_submit, handle.on_confirm)
    }

    #[tokio::test(start_paused = true)]
    async fn submit_callback_after_submit_success() {
        let (mut harness, manager_worker) = Harness::new(Duration::from_millis(200), 10, 64);
        let manager = harness.manager.clone();
        let (seq, submit_rx, _confirm_rx) = add_tx(&manager, vec![1, 2, 3]).await;
        assert_eq!(seq, 1);
        harness.start(manager_worker);

        let (_bytes, sequence, reply) = harness.expect_submit().await;
        assert_eq!(sequence, 1);
        reply.send(Ok(1)).expect("submit reply send");
        harness.pump().await;

        let submit_result = submit_rx.await.expect("submit recv");
        assert_eq!(submit_result.expect("submit ok"), 1);

        harness.drain_status_batches_pending().await;
        harness.assert_no_calls();
        harness.shutdown().await;
    }

    #[tokio::test(start_paused = true)]
    async fn confirm_callback_after_status_confirmed() {
        let (mut harness, manager_worker) = Harness::new(Duration::from_millis(10), 10, 64);
        let manager = harness.manager.clone();
        let (seq, _submit_rx, confirm_rx) = add_tx(&manager, vec![9, 9, 9]).await;
        assert_eq!(seq, 1);
        harness.start(manager_worker);

        let (_bytes, sequence, reply) = harness.expect_submit().await;
        assert_eq!(sequence, 1);
        reply.send(Ok(sequence)).expect("submit reply send");
        harness.pump().await;

        harness.drain_status_batches_pending().await;
        harness.tick_confirm().await;
        let (ids, reply) = harness.expect_status_batch().await;
        assert_eq!(ids, vec![sequence]);
        reply
            .send(Ok(vec![(sequence, TxStatus::Confirmed { info: 123 })]))
            .expect("status reply send");

        let confirm_result = confirm_rx.await.expect("confirm recv");
        let info = confirm_result.expect("confirm ok");
        assert_eq!(info, 123);

        harness.assert_no_calls();
        harness.shutdown().await;
    }

    #[tokio::test(start_paused = true)]
    async fn submit_callback_waits_for_submit_delay() {
        let (mut harness, manager_worker) = Harness::new(Duration::from_millis(10), 10, 64);
        let manager = harness.manager.clone();
        let (seq, mut submit_rx, _confirm_rx) = add_tx(&manager, vec![4, 5, 6]).await;
        assert_eq!(seq, 1);
        harness.start(manager_worker);

        let (_bytes, sequence, reply) = harness.expect_submit().await;
        assert_eq!(sequence, 1);

        tokio::select! {
            _ = &mut submit_rx => panic!("submit resolved before reply"),
            _ = harness.pump() => {}
        }

        reply.send(Ok(sequence)).expect("submit reply send");
        harness.pump().await;
        let submit_result = submit_rx.await.expect("submit recv");
        assert_eq!(submit_result.expect("submit ok"), sequence);

        harness.drain_status_batches_pending().await;
        harness.assert_no_calls();
        harness.shutdown().await;
    }

    #[tokio::test(start_paused = true)]
    async fn max_status_batch_is_capped() {
        let (mut harness, manager_worker) = Harness::new(Duration::from_millis(10), 2, 64);
        let manager = harness.manager.clone();
        let mut submit_rxs = Vec::new();
        let mut confirm_rxs = Vec::new();
        for seq in 1..=3 {
            let (added_seq, submit_rx, confirm_rx) = add_tx(&manager, vec![seq as u8]).await;
            assert_eq!(added_seq, seq);
            submit_rxs.push(submit_rx);
            confirm_rxs.push(confirm_rx);
        }
        harness.start(manager_worker);

        for seq in 1..=3 {
            let (_bytes, sequence, reply) = harness.expect_submit().await;
            assert_eq!(sequence, seq);
            reply.send(Ok(seq)).expect("submit reply send");
            harness.pump().await;
        }

        for rx in submit_rxs {
            let submit_result = rx.await.expect("submit recv");
            assert!(submit_result.is_ok());
        }

        harness.tick_confirm().await;
        let (ids, reply) = harness.expect_status_batch().await;
        assert!(ids.len() <= 2);
        let mut statuses = Vec::with_capacity(ids.len());
        for (idx, id) in ids.iter().enumerate() {
            statuses.push((
                *id,
                TxStatus::Confirmed {
                    info: 100 + idx as u64,
                },
            ));
        }
        reply.send(Ok(statuses)).expect("status reply send");
        harness.pump().await;

        harness.tick_confirm().await;
        let (ids, reply) = harness.expect_status_batch().await;
        assert!(ids.len() <= 2);
        let mut statuses = Vec::with_capacity(ids.len());
        for (idx, id) in ids.into_iter().enumerate() {
            statuses.push((
                id,
                TxStatus::Confirmed {
                    info: 200 + idx as u64,
                },
            ));
        }
        reply.send(Ok(statuses)).expect("status reply send");

        for rx in confirm_rxs {
            let result = rx.await.expect("confirm recv");
            assert!(result.is_ok());
        }

        harness.assert_no_calls();
        harness.shutdown().await;
    }

    #[tokio::test(start_paused = true)]
    async fn recovery_stops_on_rejected_in_range() {
        let (mut harness, manager_worker) = Harness::new(Duration::from_millis(200), 10, 64);
        let manager = harness.manager.clone();
        let (seq1, submit_rx1, confirm_rx1) = add_tx(&manager, vec![1]).await;
        let (seq2, _submit_rx2, _confirm_rx2) = add_tx(&manager, vec![2]).await;
        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);
        harness.start(manager_worker);

        let (_bytes, sequence, reply) = harness.expect_submit().await;
        assert_eq!(sequence, 1);
        reply.send(Ok(sequence)).expect("submit reply send");
        let first_id = sequence;

        let submit_result = submit_rx1.await.expect("submit recv");
        assert_eq!(submit_result.expect("submit ok"), first_id);

        let (_bytes, sequence, reply) = harness.expect_submit().await;
        assert_eq!(sequence, 2);
        reply
            .send(Err(SubmitFailure::SequenceMismatch { expected: 2 }))
            .expect("submit reply send");
        harness.pump().await;

        harness.tick_confirm().await;
        let (ids, reply) = harness.expect_status_batch().await;
        assert!(ids.contains(&first_id));
        let statuses = ids
            .into_iter()
            .map(|id| {
                if id == first_id {
                    (
                        id,
                        TxStatus::Rejected {
                            reason: RejectionReason::SequenceMismatch {
                                expected: 2,
                                node_id: "node-1".to_string(),
                            },
                        },
                    )
                } else {
                    (id, TxStatus::Pending)
                }
            })
            .collect();
        reply.send(Ok(statuses)).expect("status reply send");

        harness.assert_no_calls_or_closed();
        harness.shutdown().await;

        let confirm_result = confirm_rx1.await;
        assert!(confirm_result.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn recovery_evicted_then_confirmed() {
        let (mut harness, manager_worker) = Harness::new(Duration::from_millis(200), 10, 64);
        let manager = harness.manager.clone();
        let (seq1, submit_rx1, confirm_rx1) = add_tx(&manager, vec![1]).await;
        let (seq2, _submit_rx2, _confirm_rx2) = add_tx(&manager, vec![2]).await;
        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);
        harness.start(manager_worker);

        let (_bytes, sequence, reply) = harness.expect_submit().await;
        assert_eq!(sequence, 1);
        reply.send(Ok(sequence)).expect("submit reply send");
        let first_id = sequence;

        let submit_result = submit_rx1.await.expect("submit recv");
        assert_eq!(submit_result.expect("submit ok"), first_id);

        let (_bytes, sequence, reply) = harness.expect_submit().await;
        assert_eq!(sequence, 2);
        reply
            .send(Err(SubmitFailure::SequenceMismatch { expected: 2 }))
            .expect("submit reply send");
        harness.pump().await;

        harness.tick_confirm().await;
        let (ids, reply) = harness.expect_status_batch().await;
        assert!(ids.contains(&first_id));
        let statuses = ids
            .into_iter()
            .map(|id| {
                if id == first_id {
                    (id, TxStatus::Evicted)
                } else {
                    (id, TxStatus::Pending)
                }
            })
            .collect();
        reply.send(Ok(statuses)).expect("status reply send");
        harness.pump().await;

        harness.tick_confirm().await;
        let (ids, reply) = harness.expect_status_batch().await;
        assert!(ids.contains(&first_id));
        let statuses = ids
            .into_iter()
            .map(|id| {
                if id == first_id {
                    (id, TxStatus::Confirmed { info: 456 })
                } else {
                    (id, TxStatus::Pending)
                }
            })
            .collect();
        reply.send(Ok(statuses)).expect("status reply send");

        let confirm_result = confirm_rx1.await.expect("confirm recv");
        let info = confirm_result.expect("confirm ok");
        assert_eq!(info, 456);

        harness.drain_status_batches_pending().await;
        harness.shutdown().await;
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_completes_with_pending_submit() {
        let (mut harness, manager_worker) = Harness::new(Duration::from_millis(200), 10, 64);
        let manager = harness.manager.clone();
        let (seq, _submit_rx, _confirm_rx) = add_tx(&manager, vec![1, 2, 3]).await;
        assert_eq!(seq, 1);
        harness.start(manager_worker);

        let (_bytes, sequence, reply) = harness.expect_submit().await;
        assert_eq!(sequence, 1);
        // Must reply so the spawned task can complete during drain_pending
        reply.send(Ok(sequence)).expect("send reply");

        harness.drain_status_batches_pending().await;
        harness.shutdown().await;
    }

    #[tokio::test(start_paused = true)]
    async fn add_tx_from_another_task() {
        let (mut harness, manager_worker) = Harness::new(Duration::from_millis(200), 10, 64);
        let manager = harness.manager.clone();

        harness.start(manager_worker);

        let handle = tokio::spawn(async move {
            let tx_handle = manager
                .add_tx(TxRequest::RawPayload(vec![7, 8, 9]), TxConfig::default())
                .await
                .expect("enqueue tx");
            let _ = tx_handle.on_submit.await;
            tx_handle.sequence
        });

        let (_bytes, seq, reply) = harness.expect_submit().await;
        assert_eq!(seq, 1);
        reply.send(Ok(seq)).expect("submit reply send");
        harness.pump().await;

        harness.drain_status_batches_pending().await;
        let queued_sequence = handle.await.expect("enqueue task");
        assert_eq!(queued_sequence, 1);
        harness.shutdown().await;
    }

    #[tokio::test]
    async fn add_tx_queue_full_does_not_increment() {
        let nodes = HashMap::<NodeId, Arc<MockTxServer>>::new();
        let signer = Arc::new(TestSigner::default());
        let (manager, mut worker) =
            TransactionWorker::new(nodes, Duration::from_millis(10), 10, signer, 1, 1);

        let seq1 = manager
            .add_tx(TxRequest::RawPayload(vec![1]), TxConfig::default())
            .await
            .expect("first add")
            .sequence;
        assert_eq!(seq1, 1);

        let err = manager
            .add_tx(TxRequest::RawPayload(vec![2]), TxConfig::default())
            .await
            .expect_err("second add should fail");
        assert!(matches!(err, Error::UnexpectedResponseType(_)));
        assert_eq!(manager.max_sent(), 1);

        let _drained = worker.add_tx_rx.try_recv().expect("drain queued tx");
        let seq2 = manager
            .add_tx(TxRequest::RawPayload(vec![3]), TxConfig::default())
            .await
            .expect("third add")
            .sequence;
        assert_eq!(seq2, 2);
        assert_eq!(manager.max_sent(), 2);
    }

    #[tokio::test]
    async fn signer_sequence_mismatch_rejects_without_increment() {
        struct BadSigner {
            seen: Arc<AtomicU64>,
        }

        #[async_trait]
        impl SignFn<TestTxId, TestConfirmInfo> for BadSigner {
            async fn sign(
                &self,
                sequence: u64,
                request: &TxRequest,
                _cfg: &TxConfig,
            ) -> Result<Transaction<TestTxId, TestConfirmInfo>> {
                self.seen.store(sequence, Ordering::SeqCst);
                let bytes = match request {
                    TxRequest::RawPayload(bytes) => bytes.clone(),
                    TxRequest::Message(_) | TxRequest::Blobs(_) => Vec::new(),
                };
                Ok(Transaction {
                    sequence: sequence.saturating_add(1),
                    bytes,
                    callbacks: TxCallbacks::default(),
                    id: sequence.saturating_add(1),
                })
            }
        }

        let seen = Arc::new(AtomicU64::new(0));
        let signer = Arc::new(BadSigner { seen: seen.clone() });
        let nodes = HashMap::<NodeId, Arc<MockTxServer>>::new();
        let (manager, mut worker) =
            TransactionWorker::new(nodes, Duration::from_millis(10), 10, signer, 1, 4);

        let err = manager
            .add_tx(TxRequest::RawPayload(vec![1]), TxConfig::default())
            .await
            .expect_err("bad signer");
        assert!(matches!(err, Error::UnexpectedResponseType(_)));
        assert_eq!(seen.load(Ordering::SeqCst), 1);
        assert_eq!(manager.max_sent(), 0);
        assert!(matches!(
            worker.add_tx_rx.try_recv(),
            Err(TryRecvError::Empty)
        ));

        let err = manager
            .add_tx(TxRequest::RawPayload(vec![2]), TxConfig::default())
            .await
            .expect_err("bad signer again");
        assert!(matches!(err, Error::UnexpectedResponseType(_)));
        assert_eq!(seen.load(Ordering::SeqCst), 1);
        assert_eq!(manager.max_sent(), 0);
    }

    #[tokio::test]
    async fn add_tx_closed_channel_returns_error() {
        let nodes = HashMap::<NodeId, Arc<MockTxServer>>::new();
        let signer = Arc::new(TestSigner::default());
        let (manager, _worker) =
            TransactionWorker::new(nodes, Duration::from_millis(10), 10, signer, 1, 4);

        drop(_worker);

        let err = manager
            .add_tx(TxRequest::RawPayload(vec![1]), TxConfig::default())
            .await
            .expect_err("closed manager");
        assert!(matches!(err, Error::UnexpectedResponseType(_)));
        assert_eq!(manager.max_sent(), 0);
    }

    #[tokio::test]
    async fn add_tx_concurrent_sequences_are_monotonic() {
        let nodes = HashMap::<NodeId, Arc<MockTxServer>>::new();
        let signer = Arc::new(TestSigner::default());
        let (manager, _worker) =
            TransactionWorker::new(nodes, Duration::from_millis(10), 10, signer, 1, 128);

        let mut handles = Vec::new();
        for _ in 0..20 {
            let manager = manager.clone();
            handles.push(tokio::spawn(async move {
                manager
                    .add_tx(TxRequest::RawPayload(vec![1, 2, 3]), TxConfig::default())
                    .await
                    .map(|handle| handle.sequence)
            }));
        }

        let mut sequences = Vec::new();
        for handle in handles {
            sequences.push(handle.await.expect("task").expect("add tx"));
        }
        sequences.sort_unstable();
        assert_eq!(sequences, (1..=20).collect::<Vec<u64>>());
        assert_eq!(manager.max_sent(), 20);
    }
}
