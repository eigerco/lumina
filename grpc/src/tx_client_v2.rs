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
//! #     ) -> Result<std::collections::HashMap<u64, celestia_grpc::tx_client_v2::TxStatus<u64>>> {
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
use tendermint_proto::google::protobuf::Any;
use tokio::sync::{Mutex, Notify, mpsc, oneshot};
use tokio::task::JoinSet;
use tokio::time;

use crate::{Error, Result, TxConfig};
/// Identifier for a submission/confirmation node.
pub type NodeId = String;
/// Result for submission calls: either a server TxId or a submission failure.
pub type TxSubmitResult<T> = Result<T, SubmitFailure>;
/// Result for confirmation calls.
pub type TxConfirmResult<T> = Result<T>;

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
pub struct Transaction<TxId, ConfirmInfo> {
    /// Transaction sequence for the signer.
    pub sequence: u64,
    /// Signed transaction bytes ready for broadcast.
    pub bytes: Vec<u8>,
    /// One-shot callbacks for submit/confirm acknowledgements.
    pub callbacks: TxCallbacks<TxId, ConfirmInfo>,
}

#[derive(Debug)]
pub struct TxCallbacks<TxId, ConfirmInfo> {
    /// Resolves when submission succeeds or fails.
    pub on_submit: Option<oneshot::Sender<Result<TxId>>>,
    /// Resolves when the transaction is confirmed or rejected.
    pub on_confirm: Option<oneshot::Sender<Result<ConfirmInfo>>>,
}

impl<TxId, ConfirmInfo> Default for TxCallbacks<TxId, ConfirmInfo> {
    fn default() -> Self {
        Self {
            on_submit: None,
            on_confirm: None,
        }
    }
}

#[derive(Debug)]
pub struct TxHandle<TxId, ConfirmInfo> {
    /// Sequence reserved for this transaction.
    pub sequence: u64,
    /// Receives submit result.
    pub on_submit: oneshot::Receiver<Result<TxId>>,
    /// Receives confirm result.
    pub on_confirm: oneshot::Receiver<Result<ConfirmInfo>>,
}

#[async_trait]
pub trait SignFn<TxId, ConfirmInfo>: Send + Sync {
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
pub struct TransactionManager<TxId, ConfirmInfo> {
    add_tx: mpsc::Sender<Transaction<TxId, ConfirmInfo>>,
    next_sequence: Arc<Mutex<u64>>,
    max_sent: Arc<AtomicU64>,
    signer: Arc<dyn SignFn<TxId, ConfirmInfo>>,
}

impl<TxId, ConfirmInfo> TransactionManager<TxId, ConfirmInfo> {
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
    pub fn max_sent(&self) -> u64 {
        self.max_sent.load(Ordering::Relaxed)
    }
}

#[derive(Debug)]
struct TxIndexEntry<TxId> {
    node_id: NodeId,
    sequence: u64,
    id: TxId,
}

#[derive(Debug, Default)]
struct NodeSubmissionState {
    submitted_seq: u64,
    inflight: bool,
    confirm_inflight: bool,
    submit_delay_until: Option<time::Instant>,
}

#[derive(Debug)]
pub enum TxStatus<ConfirmInfo> {
    /// Submitted, but not yet committed.
    Pending,
    /// Included in a block successfully with confirmation info.
    Confirmed { info: ConfirmInfo },
    /// Rejected by the node (with an expected sequence hint).
    Rejected { expected: u64 },
    /// Removed from mempool; may need resubmission.
    Evicted,
    /// Status could not be determined.
    Unknown,
}

#[derive(Debug)]
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
    NetworkError { err: Error },
    /// Node mempool is full.
    MempoolIsFull,
}

#[derive(Debug)]
pub enum ConfirmFailure {
    /// Server expects a different sequence.
    SequenceMismatch { expected: u64 },
    /// Rejected during recovery with a specific expected sequence.
    Rejected { sequence: u64, expected: u64 },
}

#[async_trait]
pub trait TxServer: Send + Sync {
    type TxId: Clone + Eq + StdHash + Send + Sync + 'static;
    type ConfirmInfo: Send + Sync + 'static;

    /// Submit signed bytes with the given sequence, returning a server TxId.
    async fn submit(&self, tx_bytes: Vec<u8>, sequence: u64) -> TxSubmitResult<Self::TxId>;
    /// Batch status lookup for submitted TxIds.
    async fn status_batch(
        &self,
        ids: Vec<Self::TxId>,
    ) -> TxConfirmResult<HashMap<Self::TxId, TxStatus<Self::ConfirmInfo>>>;
    /// Fetch current sequence for the account (used by some implementations).
    async fn current_sequence(&self) -> Result<u64>;
}

#[derive(Debug)]
enum TransactionEvent<TxId, ConfirmInfo> {
    Added(Transaction<TxId, ConfirmInfo>),
    Submitted {
        node_id: NodeId,
        sequence: u64,
        id: TxId,
    },
    Confirmed {
        sequence: u64,
        info: ConfirmInfo,
        id: TxId,
    },
    Rejected {
        node_id: NodeId,
        sent: u64,
        actual: u64,
    },
    Evicted {
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
        statuses: HashMap<TxId, TxStatus<ConfirmInfo>>,
    },
}

#[derive(Debug)]
enum ProcessorState {
    Recovering { node_id: NodeId, expected: u64 },
    Submitting,
    Stopped(StopReason),
}

#[derive(Debug)]
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

struct SubmissionResult<TxId> {
    node_id: NodeId,
    sequence: u64,
    result: TxSubmitResult<TxId>,
}

struct ConfirmationResult<TxId, ConfirmInfo> {
    node_id: NodeId,
    statuses: HashMap<TxId, TxStatus<ConfirmInfo>>,
}

pub struct TransactionWorker<S: TxServer> {
    nodes: HashMap<NodeId, Arc<S>>,
    txs: VecDeque<Transaction<S::TxId, S::ConfirmInfo>>,
    tx_index: HashMap<S::TxId, TxIndexEntry<S::TxId>>,
    events: VecDeque<TransactionEvent<S::TxId, S::ConfirmInfo>>,
    confirmed_info: HashMap<u64, S::ConfirmInfo>,
    confirmed_sequence: u64,
    next_enqueue_sequence: u64,
    node_state: HashMap<NodeId, NodeSubmissionState>,
    add_tx_rx: mpsc::Receiver<Transaction<S::TxId, S::ConfirmInfo>>,
    new_event: Arc<Notify>,
    new_submit: Arc<Notify>,
    submissions: JoinSet<SubmissionResult<S::TxId>>,
    confirmations: JoinSet<ConfirmationResult<S::TxId, S::ConfirmInfo>>,
    confirm_ticker: time::Interval,
    confirm_interval: Duration,
    max_status_batch: usize,
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
                new_event: Arc::new(Notify::new()),
                new_submit: Arc::new(Notify::new()),
                submissions: JoinSet::new(),
                confirmations: JoinSet::new(),
                confirm_ticker: time::interval(confirm_interval),
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
        self.next_enqueue_sequence += 1;
        self.txs.push_back(tx);
        self.new_submit.notify_one();
        Ok(())
    }

    /// Run the worker loop until shutdown or a terminal error.
    ///
    /// # Notes
    /// - The worker is single-owner; it should be run in a dedicated task.
    /// - On terminal failures it returns `Ok(())` after notifying callbacks.
    pub async fn process(&mut self, shutdown: Arc<Notify>) -> Result<()> {
        let mut state = ProcessorState::Submitting;
        loop {
            state = match state {
                ProcessorState::Recovering { node_id, expected } => {
                    self.run_recovering(&shutdown, node_id, expected).await?
                }
                ProcessorState::Submitting => self.run_submitting(&shutdown).await?,
                ProcessorState::Stopped(_) => break,
            };
        }
        Ok(())
    }

    async fn run_recovering(
        &mut self,
        shutdown: &Notify,
        node_id: NodeId,
        mut expected: u64,
    ) -> Result<ProcessorState> {
        if !self.events.is_empty() {
            let drained = self
                .process_events(ProcessorState::Recovering {
                    node_id: node_id.clone(),
                    expected,
                })
                .await?;
            match drained {
                ProcessorState::Stopped(reason) => {
                    return Ok(ProcessorState::Stopped(reason));
                }
                ProcessorState::Recovering {
                    node_id: _,
                    expected: drained_expected,
                } => {
                    if drained_expected > expected {
                        expected = drained_expected;
                    }
                }
                ProcessorState::Submitting => {}
            }
        }

        loop {
            let start = self.confirmed_sequence.saturating_add(1);
            if expected < start {
                return Ok(ProcessorState::Submitting);
            }

            tokio::select! {
            _ = self.confirm_ticker.tick() => {
                let start = self.confirmed_sequence.saturating_add(1);
                self.spawn_confirmations(Some((start, expected)), Some(&node_id));
            }
            Some(result) = self.confirmations.join_next() => {
                if let Ok(result) = result {
                    if let Some(next_state) =
                        self.handle_recovery_confirmation(&node_id, expected, result)
                    {
                        return Ok(next_state);
                    }
                }
            }
            _ = shutdown.notified() => return Ok(ProcessorState::Stopped(StopReason::Shutdown)),
            }
        }
    }

    async fn run_submitting(&mut self, shutdown: &Notify) -> Result<ProcessorState> {
        loop {
            let next_state = tokio::select! {
                Some(tx) = self.add_tx_rx.recv() => {
                    self.enqueue_tx(tx)?;
                    ProcessorState::Submitting
                }
                _ = self.new_submit.notified() => {
                    self.spawn_submissions();
                    ProcessorState::Submitting
                }
                _ = self.new_event.notified() => self.process_events(ProcessorState::Submitting).await?,
                _ = self.confirm_ticker.tick() => {
                    self.spawn_confirmations(None, None);
                    ProcessorState::Submitting
                }
                Some(result) = self.submissions.join_next() => {
                    if let Ok(result) = result {
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
                        self.new_event.notify_one();
                    }
                    ProcessorState::Submitting
                }
                Some(result) = self.confirmations.join_next() => {
                    if let Ok(result) = result {
                        self.events.push_back(TransactionEvent::StatusBatch {
                            node_id: result.node_id,
                            statuses: result.statuses,
                        });
                        self.new_event.notify_one();
                    }
                    ProcessorState::Submitting
                }
                _ = shutdown.notified() => ProcessorState::Stopped(StopReason::Shutdown),
            };

            match next_state {
                ProcessorState::Submitting => continue,
                ProcessorState::Recovering { .. } | ProcessorState::Stopped(_) => {
                    return Ok(next_state);
                }
            }
        }
    }

    async fn process_events(&mut self, current: ProcessorState) -> Result<ProcessorState> {
        let mut per_node_next: Vec<(NodeId, ProcessorState)> = Vec::new();
        while let Some(event) = self.events.pop_front() {
            match event {
                TransactionEvent::Added(tx) => {
                    self.txs.push_back(tx);
                    self.new_submit.notify_one();
                }
                TransactionEvent::Confirmed { sequence, info, id } => {
                    self.apply_confirmed(sequence, info, id);
                }
                TransactionEvent::Rejected {
                    node_id,
                    sent,
                    actual,
                } => {
                    if sent < actual {
                        per_node_next.push((
                            node_id.clone(),
                            ProcessorState::Recovering {
                                node_id: node_id.clone(),
                                expected: actual,
                            },
                        ));
                    } else {
                        if let Some(state) = self.node_state.get_mut(&node_id) {
                            state.submitted_seq = actual.saturating_sub(1);
                            state.inflight = false;
                        }
                        per_node_next.push((node_id.clone(), ProcessorState::Submitting));
                    }
                }
                TransactionEvent::Evicted {
                    node_id,
                    sequence,
                    id: _,
                } => {
                    if let Some(state) = self.node_state.get_mut(&node_id) {
                        state.submitted_seq = sequence;
                    }
                }
                TransactionEvent::SubmitFailed {
                    node_id,
                    sequence,
                    failure,
                    ..
                } => {
                    if let Some(state) = self.node_state.get_mut(&node_id) {
                        state.inflight = false;
                        state.submitted_seq = sequence.saturating_sub(1);
                        if sequence < self.confirmed_sequence {
                            state.submit_delay_until =
                                Some(time::Instant::now() + self.confirm_interval);
                        }
                    }
                    let next_state = match failure {
                        SubmitFailure::SequenceMismatch { expected } => {
                            if sequence > expected {
                                if let Some(state) = self.node_state.get_mut(&node_id) {
                                    state.submitted_seq = sequence;
                                }
                                ProcessorState::Submitting
                            } else {
                                ProcessorState::Recovering {
                                    node_id: node_id.clone(),
                                    expected,
                                }
                            }
                        }
                        SubmitFailure::MempoolIsFull => ProcessorState::Submitting,
                        SubmitFailure::NetworkError { err: _ } => ProcessorState::Submitting,
                        _ => ProcessorState::Stopped(failure.into()),
                    };
                    per_node_next.push((node_id.clone(), next_state));
                    self.new_submit.notify_one();
                }
                TransactionEvent::Submitted {
                    node_id,
                    sequence,
                    id,
                } => {
                    let state_node_id = node_id.clone();
                    let submit_id = id.clone();
                    self.tx_index.insert(
                        id.clone(),
                        TxIndexEntry {
                            node_id,
                            sequence,
                            id,
                        },
                    );
                    if let Some(on_submit) = self.take_on_submit(sequence) {
                        let _ = on_submit.send(Ok(submit_id));
                    }
                    if let Some(state) = self.node_state.get_mut(&state_node_id) {
                        state.inflight = false;
                    }
                    self.new_submit.notify_one();
                }
                TransactionEvent::StatusBatch { node_id, statuses } => {
                    if let Some(state) = self.node_state.get_mut(&node_id) {
                        state.confirm_inflight = false;
                    }
                    let new_events = self.prepare_status_batch(node_id, statuses);
                    if !new_events.is_empty() {
                        self.events.extend(new_events);
                        self.new_event.notify_one();
                    }
                }
            }
        }
        let mut max_recover: Option<(NodeId, u64)> = None;
        for (_, state) in per_node_next {
            match state {
                ProcessorState::Stopped(reason) => return Ok(ProcessorState::Stopped(reason)),
                ProcessorState::Recovering { node_id, expected } => {
                    let update = match &max_recover {
                        None => true,
                        Some((_, current)) => expected > *current,
                    };
                    if update {
                        max_recover = Some((node_id, expected));
                    }
                }
                ProcessorState::Submitting => {}
            }
        }

        if let Some((node_id, expected)) = max_recover {
            Ok(ProcessorState::Recovering { node_id, expected })
        } else {
            Ok(current)
        }
    }

    fn spawn_submissions(&mut self) {
        if self.nodes.is_empty() {
            return;
        }
        let now = time::Instant::now();
        for (node_id, node) in self.nodes.clone() {
            let target_sequence = {
                let state = self.node_state.entry(node_id.clone()).or_default();
                if state.inflight {
                    continue;
                }
                if let Some(delay_until) = state.submit_delay_until {
                    if now < delay_until {
                        continue;
                    }
                    state.submit_delay_until = None;
                }
                if state.submitted_seq < self.confirmed_sequence {
                    state.submitted_seq = self.confirmed_sequence;
                }
                state.submitted_seq.saturating_add(1)
            };
            let Some(bytes) = self.peek_submission(target_sequence) else {
                continue;
            };
            if let Some(state) = self.node_state.get_mut(&node_id) {
                state.submitted_seq = target_sequence;
                state.inflight = true;
            }
            self.submissions.spawn(async move {
                let result = node.submit(bytes.clone(), target_sequence).await;
                SubmissionResult {
                    node_id,
                    sequence: target_sequence,
                    result,
                }
            });
        }
    }

    fn apply_confirmed(&mut self, sequence: u64, info: S::ConfirmInfo, id: S::TxId) {
        let node_id = self.tx_index.get(&id).map(|entry| entry.node_id.clone());
        if self.tx_index.remove(&id).is_some() {}
        if let Some(node_id) = node_id {
            if let Some(state) = self.node_state.get_mut(&node_id) {
                state.submitted_seq = state.submitted_seq.max(sequence);
            }
        }
        self.confirmed_info.insert(sequence, info);
        loop {
            let next_sequence = self.confirmed_sequence.saturating_add(1);
            let Some(info) = self.confirmed_info.remove(&next_sequence) else {
                break;
            };
            if let Some(tx) = self.take_on_confirm(next_sequence) {
                let _ = tx.send(Ok(info));
            }
            self.txs.pop_front();
            self.confirmed_sequence = self.confirmed_sequence.saturating_add(1);
        }
    }

    fn spawn_confirmations(&mut self, range: Option<(u64, u64)>, node_filter: Option<&NodeId>) {
        if self.tx_index.is_empty() {
            return;
        }
        let mut per_node: HashMap<NodeId, Vec<(u64, S::TxId)>> = HashMap::new();
        for (id, entry) in &self.tx_index {
            if let Some(node_id) = node_filter {
                if &entry.node_id != node_id {
                    continue;
                }
            }
            if let Some((start, end)) = range {
                if entry.sequence < start || entry.sequence > end {
                    continue;
                }
            }
            per_node
                .entry(entry.node_id.clone())
                .or_default()
                .push((entry.sequence, id.clone()));
        }
        for (node_id, mut entries) in per_node {
            let state = self.node_state.entry(node_id.clone()).or_default();
            if state.confirm_inflight {
                continue;
            }
            let Some(node) = self.nodes.get(&node_id).cloned() else {
                continue;
            };
            entries.sort_by_key(|(seq, _)| *seq);
            let ids: Vec<S::TxId> = entries
                .into_iter()
                .take(self.max_status_batch)
                .map(|(_, id)| id)
                .collect();
            if ids.is_empty() {
                continue;
            }
            state.confirm_inflight = true;
            self.confirmations.spawn(async move {
                // TODO: check confirmation handling
                let statuses = node.status_batch(ids).await.unwrap_or_default();
                ConfirmationResult { node_id, statuses }
            });
        }
    }

    fn handle_recovery_confirmation(
        &mut self,
        recovering_node_id: &NodeId,
        expected: u64,
        result: ConfirmationResult<S::TxId, S::ConfirmInfo>,
    ) -> Option<ProcessorState> {
        if let Some(state) = self.node_state.get_mut(&result.node_id) {
            state.confirm_inflight = false;
        }
        if &result.node_id != recovering_node_id {
            return None;
        }

        let start = self.confirmed_sequence.saturating_add(1);
        if expected < start {
            return Some(ProcessorState::Submitting);
        }

        let new_events = self.prepare_status_batch(result.node_id, result.statuses);
        for event in new_events {
            match event {
                TransactionEvent::Confirmed { sequence, info, id } => {
                    self.apply_confirmed(sequence, info, id);
                    if sequence >= expected {
                        return Some(ProcessorState::Submitting);
                    }
                }
                TransactionEvent::Rejected { sent, actual, .. } => {
                    if actual > expected {
                        continue;
                    }
                    return Some(ProcessorState::Stopped(StopReason::ConfirmFailure(
                        ConfirmFailure::Rejected {
                            sequence: sent,
                            expected: actual,
                        },
                    )));
                }
                _ => {}
            }
        }

        None
    }

    fn prepare_status_batch(
        &mut self,
        node_id: NodeId,
        statuses: HashMap<S::TxId, TxStatus<S::ConfirmInfo>>,
    ) -> Vec<TransactionEvent<S::TxId, S::ConfirmInfo>> {
        let mut max_confirm: Option<u64> = None;
        let mut confirms: Vec<(u64, S::TxId, S::ConfirmInfo)> = Vec::new();
        let mut min_reject: Option<(u64, u64)> = None;
        let mut min_evict: Option<(u64, S::TxId)> = None;

        for (id, status) in statuses {
            let Some(entry) = self.tx_index.get(&id) else {
                continue;
            };
            let seq = entry.sequence;
            match status {
                TxStatus::Confirmed { info } => {
                    confirms.push((seq, id, info));
                    let update = match max_confirm {
                        Some(max) => seq > max,
                        None => true,
                    };
                    if update {
                        max_confirm = Some(seq);
                    }
                }
                TxStatus::Rejected { expected } => {
                    let update = match &min_reject {
                        Some((min, _)) => seq < *min,
                        None => true,
                    };
                    if update {
                        min_reject = Some((seq, expected));
                    }
                }
                TxStatus::Evicted => {
                    let update = match &min_evict {
                        Some((min, _)) => seq < *min,
                        None => true,
                    };
                    if update {
                        min_evict = Some((seq, id));
                    }
                }
                TxStatus::Pending | TxStatus::Unknown => {}
            }
        }

        let confirmed = max_confirm;
        let mut new_events = Vec::new();
        if let Some(max_confirm) = max_confirm {
            confirms.sort_by_key(|(seq, _, _)| *seq);
            for (seq, id, info) in confirms {
                new_events.push(TransactionEvent::Confirmed {
                    sequence: seq,
                    id,
                    info,
                });
            }
        }

        if let Some((seq, expected)) = min_reject {
            if confirmed.map_or(true, |confirmed| seq >= confirmed) {
                new_events.push(TransactionEvent::Rejected {
                    node_id: node_id.clone(),
                    sent: seq,
                    actual: expected,
                });
            }
        }

        if let Some((seq, id)) = min_evict {
            if confirmed.map_or(true, |confirmed| seq >= confirmed) {
                new_events.push(TransactionEvent::Evicted {
                    node_id,
                    sequence: seq,
                    id,
                });
            }
        }
        new_events
    }

    fn index_for_sequence(&self, sequence: u64) -> Option<usize> {
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

    fn take_on_confirm(
        &mut self,
        sequence: u64,
    ) -> Option<oneshot::Sender<Result<S::ConfirmInfo>>> {
        let idx = self.index_for_sequence(sequence)?;
        let tx = self.txs.get_mut(idx)?;
        tx.callbacks.on_confirm.take()
    }
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
            reply: oneshot::Sender<TxConfirmResult<HashMap<TestTxId, TxStatus<TestConfirmInfo>>>>,
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
        ) -> TxConfirmResult<HashMap<TestTxId, TxStatus<TestConfirmInfo>>> {
            let (reply, rx) = oneshot::channel();
            self.calls
                .send(ServerCall::StatusBatch { ids, reply })
                .await
                .expect("status batch call");
            rx.await.expect("status batch reply")
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
        shutdown: Arc<Notify>,
        handle: Option<JoinHandle<Result<()>>>,
        confirm_interval: Duration,
        manager: TransactionManager<TestTxId, TestConfirmInfo>,
    }

    impl Harness {
        const MAX_SPINS: usize = 100;

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
                    shutdown: Arc::new(Notify::new()),
                    handle: None,
                    confirm_interval,
                    manager,
                },
                worker,
            )
        }

        fn start(&mut self, mut manager: TransactionWorker<MockTxServer>) {
            let shutdown = self.shutdown.clone();
            self.handle = Some(tokio::spawn(async move { manager.process(shutdown).await }));
        }

        async fn pump(&self) {
            tokio::task::yield_now().await;
        }

        async fn expect_call(&mut self) -> ServerCall {
            for _ in 0..Self::MAX_SPINS {
                match self.calls_rx.try_recv() {
                    Ok(call) => return call,
                    Err(TryRecvError::Empty) => self.pump().await,
                    Err(TryRecvError::Disconnected) => panic!("server call channel closed"),
                }
            }
            panic!("expected server call, got none");
        }

        async fn expect_submit(
            &mut self,
        ) -> (Vec<u8>, u64, oneshot::Sender<TxSubmitResult<TestTxId>>) {
            match self.expect_call().await {
                ServerCall::Submit {
                    bytes,
                    sequence,
                    reply,
                } => (bytes, sequence, reply),
                other => panic!("expected Submit, got {:?}", other),
            }
        }

        async fn expect_status_batch(
            &mut self,
        ) -> (
            Vec<TestTxId>,
            oneshot::Sender<TxConfirmResult<HashMap<TestTxId, TxStatus<TestConfirmInfo>>>>,
        ) {
            match self.expect_call().await {
                ServerCall::StatusBatch { ids, reply } => (ids, reply),
                other => panic!("expected StatusBatch, got {:?}", other),
            }
        }

        fn assert_no_calls(&mut self) {
            match self.calls_rx.try_recv() {
                Ok(call) => panic!("unexpected server call: {:?}", call),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => panic!("server call channel closed"),
            }
        }

        fn assert_no_calls_or_closed(&mut self) {
            match self.calls_rx.try_recv() {
                Ok(call) => panic!("unexpected server call: {:?}", call),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => {}
            }
        }

        async fn tick_confirm(&self) {
            time::advance(self.confirm_interval + Duration::from_nanos(1)).await;
            self.pump().await;
        }

        async fn join(&mut self) -> Result<()> {
            self.handle
                .take()
                .expect("manager started")
                .await
                .expect("manager join")
        }

        async fn shutdown(mut self) {
            self.shutdown.notify_one();
            let _ = self.join().await;
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
        reply.send(Ok(42)).expect("submit reply send");
        harness.pump().await;

        harness.tick_confirm().await;
        let (ids, reply) = harness.expect_status_batch().await;
        assert_eq!(ids, vec![42]);
        reply
            .send(Ok(HashMap::from([(42, TxStatus::Confirmed { info: 123 })])))
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

        reply.send(Ok(7)).expect("submit reply send");
        harness.pump().await;
        let submit_result = submit_rx.await.expect("submit recv");
        assert_eq!(submit_result.expect("submit ok"), 7);

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
        let mut statuses = HashMap::new();
        for (idx, id) in ids.iter().enumerate() {
            statuses.insert(
                *id,
                TxStatus::Confirmed {
                    info: 100 + idx as u64,
                },
            );
        }
        reply.send(Ok(statuses)).expect("status reply send");
        harness.pump().await;

        harness.tick_confirm().await;
        let (ids, reply) = harness.expect_status_batch().await;
        assert!(ids.len() <= 2);
        let mut statuses = HashMap::new();
        for (idx, id) in ids.into_iter().enumerate() {
            statuses.insert(
                id,
                TxStatus::Confirmed {
                    info: 200 + idx as u64,
                },
            );
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
        reply.send(Ok(10)).expect("submit reply send");

        let submit_result = submit_rx1.await.expect("submit recv");
        assert_eq!(submit_result.expect("submit ok"), 10);

        let (_bytes, sequence, reply) = harness.expect_submit().await;
        assert_eq!(sequence, 2);
        reply
            .send(Err(SubmitFailure::SequenceMismatch { expected: 2 }))
            .expect("submit reply send");
        harness.pump().await;

        harness.tick_confirm().await;
        let (ids, reply) = harness.expect_status_batch().await;
        assert_eq!(ids, vec![10]);
        reply
            .send(Ok(HashMap::from([(
                10,
                TxStatus::Rejected { expected: 2 },
            )])))
            .expect("status reply send");

        let stopped = harness.join().await;
        assert!(stopped.is_ok());

        let confirm_result = confirm_rx1.await;
        assert!(confirm_result.is_err());

        harness.assert_no_calls_or_closed();
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
        reply.send(Ok(11)).expect("submit reply send");

        let submit_result = submit_rx1.await.expect("submit recv");
        assert_eq!(submit_result.expect("submit ok"), 11);

        let (_bytes, sequence, reply) = harness.expect_submit().await;
        assert_eq!(sequence, 2);
        reply
            .send(Err(SubmitFailure::SequenceMismatch { expected: 2 }))
            .expect("submit reply send");
        harness.pump().await;

        harness.tick_confirm().await;
        let (ids, reply) = harness.expect_status_batch().await;
        assert_eq!(ids, vec![11]);
        reply
            .send(Ok(HashMap::from([(11, TxStatus::Evicted)])))
            .expect("status reply send");
        harness.pump().await;

        harness.tick_confirm().await;
        let (ids, reply) = harness.expect_status_batch().await;
        assert_eq!(ids, vec![11]);
        reply
            .send(Ok(HashMap::from([(11, TxStatus::Confirmed { info: 456 })])))
            .expect("status reply send");

        let confirm_result = confirm_rx1.await.expect("confirm recv");
        let info = confirm_result.expect("confirm ok");
        assert_eq!(info, 456);

        harness.assert_no_calls();
        harness.shutdown().await;
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_completes_with_pending_submit() {
        let (mut harness, manager_worker) = Harness::new(Duration::from_millis(200), 10, 64);
        let manager = harness.manager.clone();
        let (seq, _submit_rx, _confirm_rx) = add_tx(&manager, vec![1, 2, 3]).await;
        assert_eq!(seq, 1);
        harness.start(manager_worker);

        let (_bytes, sequence, _reply) = harness.expect_submit().await;
        assert_eq!(sequence, 1);

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
        reply.send(Ok(99)).expect("submit reply send");
        harness.pump().await;

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
