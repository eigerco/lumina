use std::collections::{HashMap, VecDeque};
use std::hash::Hash as StdHash;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use celestia_types::state::ErrorCode;
use tokio::sync::{Mutex, Notify, oneshot};
use tokio::task::JoinSet;
use tokio::time;

use crate::{Error, Result};
pub type NodeId = String;
pub type TxSubmitResult<T> = Result<T, SubmitFailure>;
pub type TxConfirmResult<T> = Result<T>;

#[derive(Debug)]
pub struct Transaction {
    pub sequence: u64,
    pub bytes: Vec<u8>,
    pub callbacks: TxCallbacks,
}

#[derive(Debug, Default)]
pub struct TxCallbacks {
    pub on_submit: Option<oneshot::Sender<Result<()>>>,
    pub on_confirm: Option<oneshot::Sender<Result<()>>>,
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

#[derive(Debug, Clone, Copy)]
pub enum TxStatus {
    Pending,
    Confirmed,
    Rejected { expected: u64 },
    Evicted,
    Unknown,
}

#[derive(Debug)]
pub enum SubmitFailure {
    SequenceMismatch { expected: u64 },
    InvalidTx { error_code: ErrorCode },
    InsufficientFunds,
    InsufficientFee { expected_fee: u64 },
    NetworkError { err: Error },
    MempoolIsFull,
}

#[derive(Debug)]
pub enum ConfirmFailure {
    SequenceMismatch { expected: u64 },
    Rejected { sequence: u64, expected: u64 },
}

#[async_trait]
pub trait TxServer: Send + Sync {
    type TxId: Clone + Eq + StdHash + Send + Sync + 'static;

    async fn submit(&self, tx_bytes: Vec<u8>, sequence: u64) -> TxSubmitResult<Self::TxId>;
    async fn status_batch(
        &self,
        ids: Vec<Self::TxId>,
    ) -> TxConfirmResult<HashMap<Self::TxId, TxStatus>>;
    async fn current_sequence(&self) -> Result<u64>;
}

#[derive(Debug)]
enum TransactionEvent<TxId> {
    Added(Transaction),
    Submitted {
        node_id: NodeId,
        sequence: u64,
        id: TxId,
    },
    Confirmed {
        sequence: u64,
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
        statuses: HashMap<TxId, TxStatus>,
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

struct ConfirmationResult<TxId> {
    node_id: NodeId,
    statuses: HashMap<TxId, TxStatus>,
}

pub struct TransactionManager<S: TxServer> {
    nodes: HashMap<NodeId, Arc<S>>,
    txs: VecDeque<Transaction>,
    tx_index: HashMap<S::TxId, TxIndexEntry<S::TxId>>,
    events: VecDeque<TransactionEvent<S::TxId>>,
    confirmed_sequence: u64,
    node_state: HashMap<NodeId, NodeSubmissionState>,
    new_event: Arc<Notify>,
    new_submit: Arc<Notify>,
    submissions: JoinSet<SubmissionResult<S::TxId>>,
    confirmations: JoinSet<ConfirmationResult<S::TxId>>,
    confirm_ticker: time::Interval,
    confirm_interval: Duration,
    max_status_batch: usize,
}

impl<S: TxServer + 'static> TransactionManager<S> {
    pub fn new(
        nodes: HashMap<NodeId, Arc<S>>,
        confirm_interval: Duration,
        max_status_batch: usize,
    ) -> Self {
        let node_state = nodes
            .keys()
            .map(|node_id| (node_id.clone(), NodeSubmissionState::default()))
            .collect();
        Self {
            nodes,
            txs: VecDeque::new(),
            tx_index: HashMap::new(),
            events: VecDeque::new(),
            confirmed_sequence: 0,
            node_state,
            new_event: Arc::new(Notify::new()),
            new_submit: Arc::new(Notify::new()),
            submissions: JoinSet::new(),
            confirmations: JoinSet::new(),
            confirm_ticker: time::interval(confirm_interval),
            confirm_interval,
            max_status_batch,
        }
    }

    pub async fn add_tx(&mut self, tx: Transaction) -> Result<()> {
        let expected = self
            .confirmed_sequence
            .saturating_add(self.txs.len() as u64)
            .saturating_add(1);
        if tx.sequence != expected {
            return Err(crate::Error::UnexpectedResponseType(format!(
                "tx sequence gap: expected {expected}, got {}",
                tx.sequence
            )));
        }
        self.events.push_back(TransactionEvent::Added(tx));
        self.new_event.notify_one();
        Ok(())
    }

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
                TransactionEvent::Confirmed { sequence, id } => {
                    self.apply_confirmed(sequence, id);
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
                    self.tx_index.insert(
                        id.clone(),
                        TxIndexEntry {
                            node_id,
                            sequence,
                            id,
                        },
                    );
                    if let Some(on_submit) = self.take_on_submit(sequence) {
                        let _ = on_submit.send(Ok(()));
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

    fn apply_confirmed(&mut self, sequence: u64, id: S::TxId) {
        let node_id = self.tx_index.get(&id).map(|entry| entry.node_id.clone());
        if self.tx_index.remove(&id).is_some() {
        }
        if let Some(node_id) = node_id {
            if let Some(state) = self.node_state.get_mut(&node_id) {
                state.submitted_seq = state.submitted_seq.max(sequence);
            }
        }
        while self.confirmed_sequence < sequence {
            let next_sequence = self.confirmed_sequence.saturating_add(1);
            if let Some(tx) = self.take_on_confirm(next_sequence) {
                let _ = tx.send(Ok(()));
            }
            self.txs.pop_front();
            self.confirmed_sequence = self.confirmed_sequence.saturating_add(1);
        }
        self.confirmed_sequence = self.confirmed_sequence.max(sequence);
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
        result: ConfirmationResult<S::TxId>,
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
                TransactionEvent::Confirmed { sequence, id } => {
                    self.apply_confirmed(sequence, id);
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
        statuses: HashMap<S::TxId, TxStatus>,
    ) -> Vec<TransactionEvent<S::TxId>> {
        let mut max_confirm: Option<(u64, S::TxId)> = None;
        let mut min_reject: Option<(u64, u64)> = None;
        let mut min_evict: Option<(u64, S::TxId)> = None;

        for (id, status) in statuses {
            let Some(entry) = self.tx_index.get(&id) else {
                continue;
            };
            let seq = entry.sequence;
            match status {
                TxStatus::Confirmed => {
                    let update = match &max_confirm {
                        Some((max, _)) => seq > *max,
                        None => true,
                    };
                    if update {
                        max_confirm = Some((seq, id));
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

        let confirmed = max_confirm.as_ref().map(|(seq, _)| *seq);
        let mut new_events = Vec::new();
        if let Some((seq, id)) = max_confirm {
            new_events.push(TransactionEvent::Confirmed { sequence: seq, id });
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

    fn take_on_submit(&mut self, sequence: u64) -> Option<oneshot::Sender<Result<()>>> {
        let idx = self.index_for_sequence(sequence)?;
        let tx = self.txs.get_mut(idx)?;
        tx.callbacks.on_submit.take()
    }

    fn take_on_confirm(&mut self, sequence: u64) -> Option<oneshot::Sender<Result<()>>> {
        let idx = self.index_for_sequence(sequence)?;
        let tx = self.txs.get_mut(idx)?;
        tx.callbacks.on_confirm.take()
    }
}

pub struct FakeTxServer<TxId>
where
    TxId: Clone + Eq + StdHash + Send + Sync + 'static,
{
    submit_results: Mutex<VecDeque<TxSubmitResult<TxId>>>,
    status_batches: Mutex<VecDeque<HashMap<TxId, TxStatus>>>,
    current_sequences: Mutex<VecDeque<u64>>,
    submit_delays: Mutex<VecDeque<Duration>>,
    status_delays: Mutex<VecDeque<Duration>>,
    current_sequence_delays: Mutex<VecDeque<Duration>>,
    submit_calls: Mutex<Vec<(Vec<u8>, u64)>>,
    status_calls: Mutex<Vec<Vec<TxId>>>,
    current_sequence_calls: AtomicUsize,
    default_current_sequence: AtomicU64,
    default_submit_delay_ms: AtomicU64,
    default_status_delay_ms: AtomicU64,
    default_current_sequence_delay_ms: AtomicU64,
}

impl<TxId> FakeTxServer<TxId>
where
    TxId: Clone + Eq + StdHash + Send + Sync + 'static,
{
    pub fn new(default_current_sequence: u64) -> Self {
        Self {
            submit_results: Mutex::new(VecDeque::new()),
            status_batches: Mutex::new(VecDeque::new()),
            current_sequences: Mutex::new(VecDeque::new()),
            submit_delays: Mutex::new(VecDeque::new()),
            status_delays: Mutex::new(VecDeque::new()),
            current_sequence_delays: Mutex::new(VecDeque::new()),
            submit_calls: Mutex::new(Vec::new()),
            status_calls: Mutex::new(Vec::new()),
            current_sequence_calls: AtomicUsize::new(0),
            default_current_sequence: AtomicU64::new(default_current_sequence),
            default_submit_delay_ms: AtomicU64::new(0),
            default_status_delay_ms: AtomicU64::new(0),
            default_current_sequence_delay_ms: AtomicU64::new(0),
        }
    }

    pub async fn push_submit_result(&self, result: TxSubmitResult<TxId>) {
        self.submit_results.lock().await.push_back(result);
    }

    pub async fn push_status_batch(&self, statuses: HashMap<TxId, TxStatus>) {
        self.status_batches.lock().await.push_back(statuses);
    }

    pub async fn push_current_sequence(&self, sequence: u64) {
        self.current_sequences.lock().await.push_back(sequence);
    }

    pub async fn push_submit_delay(&self, delay: Duration) {
        self.submit_delays.lock().await.push_back(delay);
    }

    pub async fn push_status_delay(&self, delay: Duration) {
        self.status_delays.lock().await.push_back(delay);
    }

    pub async fn push_current_sequence_delay(&self, delay: Duration) {
        self.current_sequence_delays.lock().await.push_back(delay);
    }

    pub fn set_default_current_sequence(&self, sequence: u64) {
        self.default_current_sequence
            .store(sequence, Ordering::SeqCst);
    }

    pub fn set_default_submit_delay(&self, delay: Duration) {
        self.default_submit_delay_ms
            .store(delay.as_millis() as u64, Ordering::SeqCst);
    }

    pub fn set_default_status_delay(&self, delay: Duration) {
        self.default_status_delay_ms
            .store(delay.as_millis() as u64, Ordering::SeqCst);
    }

    pub fn set_default_current_sequence_delay(&self, delay: Duration) {
        self.default_current_sequence_delay_ms
            .store(delay.as_millis() as u64, Ordering::SeqCst);
    }

    pub async fn take_submit_calls(&self) -> Vec<(Vec<u8>, u64)> {
        self.submit_calls.lock().await.drain(..).collect()
    }

    pub async fn take_status_calls(&self) -> Vec<Vec<TxId>> {
        self.status_calls.lock().await.drain(..).collect()
    }

    pub fn current_sequence_call_count(&self) -> usize {
        self.current_sequence_calls.load(Ordering::SeqCst)
    }

    async fn pop_delay(queue: &Mutex<VecDeque<Duration>>, default_ms: &AtomicU64) -> Duration {
        queue
            .lock()
            .await
            .pop_front()
            .unwrap_or_else(|| Duration::from_millis(default_ms.load(Ordering::SeqCst)))
    }
}

#[async_trait]
impl<TxId> TxServer for FakeTxServer<TxId>
where
    TxId: Clone + Eq + StdHash + Send + Sync + 'static,
{
    type TxId = TxId;

    async fn submit(&self, tx_bytes: Vec<u8>, sequence: u64) -> TxSubmitResult<Self::TxId> {
        let delay = Self::pop_delay(&self.submit_delays, &self.default_submit_delay_ms).await;
        if !delay.is_zero() {
            time::sleep(delay).await;
        }
        self.submit_calls.lock().await.push((tx_bytes, sequence));
        let mut results = self.submit_results.lock().await;
        results.pop_front().unwrap_or_else(|| {
            Err(SubmitFailure::NetworkError {
                err: Error::UnexpectedResponseType("fake submit result missing".to_string()),
            })
        })
    }

    async fn status_batch(
        &self,
        ids: Vec<Self::TxId>,
    ) -> TxConfirmResult<HashMap<Self::TxId, TxStatus>> {
        let delay = Self::pop_delay(&self.status_delays, &self.default_status_delay_ms).await;
        if !delay.is_zero() {
            time::sleep(delay).await;
        }
        self.status_calls.lock().await.push(ids);
        let mut batches = self.status_batches.lock().await;
        Ok(batches.pop_front().unwrap_or_default())
    }

    async fn current_sequence(&self) -> Result<u64> {
        let delay = Self::pop_delay(
            &self.current_sequence_delays,
            &self.default_current_sequence_delay_ms,
        )
        .await;
        if !delay.is_zero() {
            time::sleep(delay).await;
        }
        self.current_sequence_calls.fetch_add(1, Ordering::SeqCst);
        let mut sequences = self.current_sequences.lock().await;
        Ok(sequences
            .pop_front()
            .unwrap_or_else(|| self.default_current_sequence.load(Ordering::SeqCst)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, timeout};

    async fn run_manager(
        mut manager: TransactionManager<FakeTxServer<u64>>,
        shutdown: Arc<Notify>,
    ) -> tokio::task::JoinHandle<Result<()>> {
        tokio::spawn(async move { manager.process(shutdown).await })
    }

    async fn add_tx_and_drain(
        manager: &mut TransactionManager<FakeTxServer<u64>>,
        tx: Transaction,
    ) {
        manager.add_tx(tx).await.expect("add tx");
        let _ = manager
            .process_events(ProcessorState::Submitting)
            .await
            .expect("process events");
    }

    async fn wait_for_submit_calls(
        server: &FakeTxServer<u64>,
        expected_calls: usize,
        timeout_duration: Duration,
    ) -> Vec<(Vec<u8>, u64)> {
        let mut collected = Vec::new();
        let start = time::Instant::now();
        while collected.len() < expected_calls {
            let mut new_calls = server.take_submit_calls().await;
            collected.append(&mut new_calls);
            if collected.len() >= expected_calls {
                break;
            }
            if start.elapsed() > timeout_duration {
                break;
            }
            sleep(Duration::from_millis(5)).await;
        }
        collected
    }

    #[tokio::test]
    async fn submit_callback_after_submit_success() {
        let server = Arc::new(FakeTxServer::<u64>::new(0));
        server.push_submit_result(Ok(1)).await;

        let nodes = HashMap::from([(String::from("node-1"), server.clone())]);
        let mut manager = TransactionManager::new(nodes, Duration::from_millis(200), 10);

        let (submit_tx, submit_rx) = oneshot::channel();
        let (confirm_tx, _confirm_rx) = oneshot::channel();
        let tx = Transaction {
            sequence: 1,
            bytes: vec![1, 2, 3],
            callbacks: TxCallbacks {
                on_submit: Some(submit_tx),
                on_confirm: Some(confirm_tx),
            },
        };
        manager.add_tx(tx).await.expect("add tx");

        let shutdown = Arc::new(Notify::new());
        let handle = run_manager(manager, shutdown.clone()).await;

        let submit_result = timeout(Duration::from_secs(1), submit_rx)
            .await
            .expect("submit timeout")
            .expect("submit recv");
        assert!(submit_result.is_ok());

        shutdown.notify_one();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn confirm_callback_after_status_confirmed() {
        let server = Arc::new(FakeTxServer::<u64>::new(0));
        server.push_submit_result(Ok(42)).await;

        let mut statuses = HashMap::new();
        statuses.insert(42, TxStatus::Confirmed);
        server.push_status_batch(statuses).await;

        let nodes = HashMap::from([(String::from("node-1"), server.clone())]);
        let mut manager = TransactionManager::new(nodes, Duration::from_millis(10), 10);

        let (submit_tx, _submit_rx) = oneshot::channel();
        let (confirm_tx, confirm_rx) = oneshot::channel();
        let tx = Transaction {
            sequence: 1,
            bytes: vec![9, 9, 9],
            callbacks: TxCallbacks {
                on_submit: Some(submit_tx),
                on_confirm: Some(confirm_tx),
            },
        };
        manager.add_tx(tx).await.expect("add tx");

        let shutdown = Arc::new(Notify::new());
        let handle = run_manager(manager, shutdown.clone()).await;

        let confirm_result = timeout(Duration::from_secs(1), confirm_rx)
            .await
            .expect("confirm timeout")
            .expect("confirm recv");
        assert!(confirm_result.is_ok());

        shutdown.notify_one();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn submit_callback_waits_for_submit_delay() {
        let server = Arc::new(FakeTxServer::<u64>::new(0));
        server.set_default_submit_delay(Duration::from_millis(50));
        server.push_submit_result(Ok(7)).await;

        let nodes = HashMap::from([(String::from("node-1"), server.clone())]);
        let mut manager = TransactionManager::new(nodes, Duration::from_millis(10), 10);

        let (submit_tx, submit_rx) = oneshot::channel();
        let (confirm_tx, _confirm_rx) = oneshot::channel();
        let tx = Transaction {
            sequence: 1,
            bytes: vec![4, 5, 6],
            callbacks: TxCallbacks {
                on_submit: Some(submit_tx),
                on_confirm: Some(confirm_tx),
            },
        };
        manager.add_tx(tx).await.expect("add tx");

        let shutdown = Arc::new(Notify::new());
        let handle = run_manager(manager, shutdown.clone()).await;

        let mut submit_rx = submit_rx;
        let early = timeout(Duration::from_millis(10), &mut submit_rx).await;
        assert!(early.is_err());

        let submit_result = timeout(Duration::from_secs(1), &mut submit_rx)
            .await
            .expect("submit timeout")
            .expect("submit recv");
        assert!(submit_result.is_ok());

        shutdown.notify_one();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn max_status_batch_is_capped() {
        let server = Arc::new(FakeTxServer::<u64>::new(0));
        server.push_submit_result(Ok(1)).await;
        server.push_submit_result(Ok(2)).await;
        server.push_submit_result(Ok(3)).await;

        let mut batch_1 = HashMap::new();
        batch_1.insert(1, TxStatus::Confirmed);
        batch_1.insert(2, TxStatus::Confirmed);
        server.push_status_batch(batch_1).await;

        let mut batch_2 = HashMap::new();
        batch_2.insert(3, TxStatus::Confirmed);
        server.push_status_batch(batch_2).await;

        let nodes = HashMap::from([(String::from("node-1"), server.clone())]);
        let mut manager = TransactionManager::new(nodes, Duration::from_millis(10), 2);

        let mut submit_rxs = Vec::new();
        let mut confirm_rxs = Vec::new();
        for seq in 1..=3 {
            let (submit_tx, submit_rx) = oneshot::channel();
            let (confirm_tx, confirm_rx) = oneshot::channel();
            let tx = Transaction {
                sequence: seq,
                bytes: vec![seq as u8],
                callbacks: TxCallbacks {
                    on_submit: Some(submit_tx),
                    on_confirm: Some(confirm_tx),
                },
            };
            add_tx_and_drain(&mut manager, tx).await;
            submit_rxs.push(submit_rx);
            confirm_rxs.push(confirm_rx);
        }

        let shutdown = Arc::new(Notify::new());
        let handle = run_manager(manager, shutdown.clone()).await;

        for rx in submit_rxs {
            let result = timeout(Duration::from_secs(1), rx)
                .await
                .expect("submit timeout")
                .expect("submit recv");
            assert!(result.is_ok());
        }

        for rx in confirm_rxs {
            let result = timeout(Duration::from_secs(1), rx)
                .await
                .expect("confirm timeout")
                .expect("confirm recv");
            assert!(result.is_ok());
        }

        sleep(Duration::from_millis(20)).await;
        let status_calls = server.take_status_calls().await;
        assert!(!status_calls.is_empty());
        for call in status_calls {
            assert!(call.len() <= 2);
        }

        shutdown.notify_one();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn recovery_stops_on_rejected_in_range() {
        let server = Arc::new(FakeTxServer::<u64>::new(0));
        server.push_submit_result(Ok(10)).await;
        server
            .push_submit_result(Err(SubmitFailure::SequenceMismatch { expected: 2 }))
            .await;

        let nodes = HashMap::from([(String::from("node-1"), server.clone())]);
        let mut manager = TransactionManager::new(nodes, Duration::from_millis(10), 10);

        let (submit_tx1, _submit_rx1) = oneshot::channel();
        let (confirm_tx1, confirm_rx1) = oneshot::channel();
        let tx1 = Transaction {
            sequence: 1,
            bytes: vec![1],
            callbacks: TxCallbacks {
                on_submit: Some(submit_tx1),
                on_confirm: Some(confirm_tx1),
            },
        };
        add_tx_and_drain(&mut manager, tx1).await;

        let (submit_tx2, _submit_rx2) = oneshot::channel();
        let (confirm_tx2, _confirm_rx2) = oneshot::channel();
        let tx2 = Transaction {
            sequence: 2,
            bytes: vec![2],
            callbacks: TxCallbacks {
                on_submit: Some(submit_tx2),
                on_confirm: Some(confirm_tx2),
            },
        };
        add_tx_and_drain(&mut manager, tx2).await;

        let shutdown = Arc::new(Notify::new());
        let handle = run_manager(manager, shutdown.clone()).await;

        let _calls = wait_for_submit_calls(&server, 2, Duration::from_secs(1)).await;
        let mut rejected = HashMap::new();
        rejected.insert(10, TxStatus::Rejected { expected: 2 });
        server.push_status_batch(rejected).await;

        let stopped = timeout(Duration::from_secs(2), handle)
            .await
            .expect("manager stop")
            .expect("manager join");
        assert!(stopped.is_ok());

        let confirm_result = timeout(Duration::from_secs(1), confirm_rx1)
            .await
            .expect("confirm timeout");
        assert!(confirm_result.is_err());

        shutdown.notify_one();
    }

    #[tokio::test]
    async fn recovery_evicted_then_confirmed() {
        let server = Arc::new(FakeTxServer::<u64>::new(0));
        server.push_submit_result(Ok(11)).await;
        server
            .push_submit_result(Err(SubmitFailure::SequenceMismatch { expected: 2 }))
            .await;

        let nodes = HashMap::from([(String::from("node-1"), server.clone())]);
        let mut manager = TransactionManager::new(nodes, Duration::from_millis(200), 10);

        let (submit_tx1, _submit_rx1) = oneshot::channel();
        let (confirm_tx1, confirm_rx1) = oneshot::channel();
        let tx1 = Transaction {
            sequence: 1,
            bytes: vec![1],
            callbacks: TxCallbacks {
                on_submit: Some(submit_tx1),
                on_confirm: Some(confirm_tx1),
            },
        };
        add_tx_and_drain(&mut manager, tx1).await;

        let (submit_tx2, _submit_rx2) = oneshot::channel();
        let (confirm_tx2, _confirm_rx2) = oneshot::channel();
        let tx2 = Transaction {
            sequence: 2,
            bytes: vec![2],
            callbacks: TxCallbacks {
                on_submit: Some(submit_tx2),
                on_confirm: Some(confirm_tx2),
            },
        };
        add_tx_and_drain(&mut manager, tx2).await;

        let shutdown = Arc::new(Notify::new());
        let handle = run_manager(manager, shutdown.clone()).await;

        let _calls = wait_for_submit_calls(&server, 2, Duration::from_secs(1)).await;
        let mut evicted = HashMap::new();
        evicted.insert(11, TxStatus::Evicted);
        server.push_status_batch(evicted).await;

        let mut confirmed = HashMap::new();
        confirmed.insert(11, TxStatus::Confirmed);
        server.push_status_batch(confirmed).await;

        let confirm_result = timeout(Duration::from_secs(2), confirm_rx1)
            .await
            .expect("confirm timeout")
            .expect("confirm recv");
        assert!(confirm_result.is_ok());

        shutdown.notify_one();
        let _ = handle.await;
    }
}
