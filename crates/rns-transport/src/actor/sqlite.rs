//! Async storage admission. Only messages that need announce state wait for
//! SQL; link/data traffic and lifecycle messages continue on the actor.
use super::*;
use crate::messages::{AnnounceRpcEntry, TransportQuery as Q, TransportQueryResponse as R};
use crate::storage::{self, AnnouncePageQuery, Mutation, Reply, Request, StorageHandle};
use tokio::task::JoinHandle;

const CACHE_ENTRIES: usize = 256;
const CACHE_BYTES: usize = 256 * 1024;
const QUEUE_ENTRIES: usize = 64;
const QUEUE_BYTES: usize = 256 * 1024;
const WRITE_BATCH_ITEMS: usize = 32;
const WRITE_BATCH_BYTES: usize = 64 * 1024;
// Bound a deleting transaction more tightly than read-only keep-set staging.
const GC_PAGE_ENTRIES: usize = 32;
const GC_REST_FACTOR: u32 = 3;

#[derive(Clone, Copy, Default)]
enum GcPhase {
    #[default]
    Idle,
    Clean(Option<[u8; 16]>),
    Collect(Option<[u8; 32]>),
}

#[derive(Debug)]
enum BackgroundResult {
    Sweep,
    Maintenance,
    Cleaned {
        next: Option<[u8; 16]>,
        removed: usize,
        destinations: Vec<[u8; 16]>,
    },
    Collected {
        next: Option<[u8; 32]>,
        removed: usize,
    },
}

pub(super) struct SqliteState {
    worker: StorageHandle,
    pub raw: HashMap<[u8; 32], Vec<u8>>,
    queue: VecDeque<Queued>,
    bytes: usize,
    writes: Vec<Mutation>,
    batch_delay: Duration,
    flush_at: Option<tokio::time::Instant>,
    path_response_flush: bool,
    pub busy: bool,
    order: VecDeque<[u8; 16]>,
    error: Option<String>,
    last_sweep: f64,
    last_vacuum: f64,
    vacuum_interval: f64,
    vacuum_pages: u32,
    gc_phase: GcPhase,
    next_gc: tokio::time::Instant,
    gc_started: Option<std::time::Instant>,
    gc_announces: usize,
    gc_packets: usize,
    gc_pages: u64,
    gc_page_time: Duration,
    metrics: AdmissionMetrics,
}

// Conditions can overlap: count each active trigger instead of assigning an
// arbitrary winner that hides a read barrier behind an expired timer.
#[derive(Clone, Copy)]
enum FlushTrigger {
    Shutdown,
    Background,
    PathResponse,
    Timer,
    ItemLimit,
    ByteLimit,
    Rpc,
    Priority,
    PacketRead,
    NextCapacity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PriorityKind {
    PathRequest,
    PathResponse,
    Lrproof,
    Other,
}

#[derive(Default)]
struct BatchFlushMetrics {
    batches: u64,
    items: u64,
    singletons: u64,
    triggers: [u64; 10],
    priority_kinds: [u64; 4],
    unknown_path_bypasses: u64,
}

impl BatchFlushMetrics {
    fn report(&self) {
        if self.batches == 0 && self.unknown_path_bypasses == 0 {
            return;
        }
        tracing::debug!(
            batches = self.batches,
            items = self.items,
            singletons = self.singletons,
            shutdown = self.triggers[FlushTrigger::Shutdown as usize],
            background = self.triggers[FlushTrigger::Background as usize],
            path_response = self.triggers[FlushTrigger::PathResponse as usize],
            timer = self.triggers[FlushTrigger::Timer as usize],
            item_limit = self.triggers[FlushTrigger::ItemLimit as usize],
            byte_limit = self.triggers[FlushTrigger::ByteLimit as usize],
            rpc = self.triggers[FlushTrigger::Rpc as usize],
            priority = self.triggers[FlushTrigger::Priority as usize],
            priority_path_request = self.priority_kinds[PriorityKind::PathRequest as usize],
            priority_path_response = self.priority_kinds[PriorityKind::PathResponse as usize],
            priority_lrproof = self.priority_kinds[PriorityKind::Lrproof as usize],
            priority_other = self.priority_kinds[PriorityKind::Other as usize],
            unknown_path_bypasses = self.unknown_path_bypasses,
            packet_read = self.triggers[FlushTrigger::PacketRead as usize],
            next_capacity = self.triggers[FlushTrigger::NextCapacity as usize],
            "SQLite batch flush summary"
        );
    }
}

struct AdmissionMetrics {
    since: std::time::Instant,
    admission: storage::metrics::Timing,
    flushes: BatchFlushMetrics,
    hits: u64,
    misses: u64,
    max_queue_entries: usize,
    max_queue_bytes: usize,
    max_staged_bytes: usize,
}

impl Default for AdmissionMetrics {
    fn default() -> Self {
        Self {
            since: std::time::Instant::now(),
            admission: Default::default(),
            flushes: BatchFlushMetrics::default(),
            hits: 0,
            misses: 0,
            max_queue_entries: 0,
            max_queue_bytes: 0,
            max_staged_bytes: 0,
        }
    }
}

impl AdmissionMetrics {
    fn report(&mut self) {
        tracing::debug!(
            cache_hits = self.hits,
            cache_misses = self.misses,
            max_queue_entries = self.max_queue_entries,
            max_queue_bytes = self.max_queue_bytes,
            max_staged_bytes = self.max_staged_bytes,
            "SQLite admission summary"
        );
        self.flushes.report();
        self.admission
            .report("admission", "wait_before_preparation");
        *self = Self::default();
    }
}

struct Queued {
    enqueued: std::time::Instant,
    message: TransportMessage,
    priority: u8,
}

#[derive(Default)]
struct Reads {
    keys: Vec<[u8; 16]>,
    identities: Vec<[u8; 16]>,
    packets: Vec<[u8; 32]>,
    cached: usize,
}
#[derive(Debug)]
struct Prepared {
    message: Option<TransportMessage>,
    entries: Vec<RecentAnnounce>,
    raw: HashMap<[u8; 32], Vec<u8>>,
    invalidate: Invalidation,
}

#[derive(Debug, Default)]
enum Invalidation {
    #[default]
    None,
    All,
    Destinations(Vec<[u8; 16]>),
    Identity([u8; 16]),
}

fn weight(msg: &TransportMessage) -> usize {
    match msg {
        TransportMessage::Inbound(p) => p.raw.len() + 256,
        TransportMessage::AdmittedInbound(p) => p.raw.len().saturating_mul(2).saturating_add(1024),
        TransportMessage::Rpc {
            query: Q::FilterBlackholedDests { dests },
            ..
        } => dests.capacity() * 16 + 256,
        _ => 256,
    }
}

fn reject(msg: TransportMessage, error: &str) {
    if let TransportMessage::Rpc { response_tx, .. } = msg {
        let _ = response_tx.send(R::Error(format!("SQLite storage: {error}")));
    }
}

async fn call(worker: &StorageHandle, req: Request) -> storage::Result<Reply> {
    worker.try_submit(req).map_err(|r| r.error)?.wait().await
}

async fn write(worker: &StorageHandle, writes: Vec<Mutation>) -> storage::Result<()> {
    // The actor batches ordinary announces within a fixed byte/item budget.
    // RPC mutations remain ordered commit barriers; there is no unbounded queue.
    if !writes.is_empty() {
        call(worker, Request::Apply(writes)).await?;
    }
    Ok(())
}

fn dto(a: RecentAnnounce) -> AnnounceRpcEntry {
    AnnounceRpcEntry {
        dest_hash: a.dest_hash,
        hops: a.hops,
        app_data: a.app_data,
        timestamp: a.timestamp,
        public_key: a.public_key,
        ratchet: a.ratchet,
        name_hash: a.name_hash,
        is_path_response: a.is_path_response,
        retained: a.retained,
    }
}

async fn page(
    worker: &StorageHandle,
    q: AnnouncePageQuery,
) -> storage::Result<storage::AnnouncePage> {
    match call(worker, Request::Page(q)).await? {
        Reply::Page(p) => Ok(p),
        _ => Err(storage::StorageError::Invalid("page reply")),
    }
}

impl TransportActor {
    pub(super) fn sqlite_snapshot_ready(&self) -> bool {
        self.sqlite
            .as_ref()
            .is_none_or(|s| !s.busy && s.writes.is_empty() && s.error.is_none())
    }
    /// Own a fresh SQLite namespace. No legacy directory is read or imported.
    /// Remaining path/hashlist snapshots live here until their migration in D.
    pub async fn initialize_sqlite_storage(&mut self, directory: PathBuf) -> storage::Result<()> {
        self.initialize_sqlite_storage_with_options(directory, Default::default())
            .await
    }

    pub async fn initialize_sqlite_storage_with_options(
        &mut self,
        directory: PathBuf,
        options: storage::SqliteOptions,
    ) -> storage::Result<()> {
        self.initialize_sqlite_database_path(directory.join("transport.sqlite"), options)
            .await
    }

    /// Open an explicitly configured database file, matching rsLXMF's
    /// `storage.database_path` semantics. Parent directory is created by the
    /// runtime before this call.
    pub async fn initialize_sqlite_database_path(
        &mut self,
        database_path: PathBuf,
        options: storage::SqliteOptions,
    ) -> storage::Result<()> {
        if self.shared_instance_client_mode {
            return Err(storage::StorageError::NotOwner);
        }
        let directory =
            database_path
                .parent()
                .map(PathBuf::from)
                .ok_or(storage::StorageError::Invalid(
                    "database path has no parent",
                ))?;
        std::fs::create_dir_all(&directory)?;
        // Match rsLXMF: maintenance cannot run more often than once a minute.
        // vacuum_pages=0 remains useful as checkpoint-and-metrics mode.
        let vacuum_interval = options
            .vacuum_interval
            .max(Duration::from_secs(60))
            .as_secs_f64();
        let vacuum_pages = options.vacuum_pages;
        let batch_delay = options.announce_batch_delay;
        let worker =
            StorageHandle::open_sqlite(database_path, storage::StorageRole::Standalone, options)
                .await?;
        self.sqlite = Some(SqliteState {
            worker,
            raw: HashMap::new(),
            queue: VecDeque::new(),
            bytes: 0,
            writes: Vec::with_capacity(WRITE_BATCH_ITEMS),
            batch_delay,
            flush_at: None,
            path_response_flush: false,
            busy: false,
            order: VecDeque::new(),
            error: None,
            last_sweep: 0.0,
            last_vacuum: crate::now_f64(),
            vacuum_interval,
            vacuum_pages,
            gc_phase: GcPhase::Idle,
            next_gc: tokio::time::Instant::now(),
            gc_started: None,
            gc_announces: 0,
            gc_packets: 0,
            gc_pages: 0,
            gc_page_time: Duration::ZERO,
            metrics: AdmissionMetrics::default(),
        });
        tracing::debug!(
            delay_ms = batch_delay.as_millis() as u64,
            max_items = WRITE_BATCH_ITEMS,
            max_bytes = WRITE_BATCH_BYTES,
            "SQLite announce batching configured"
        );
        self.storage_dir = Some(directory);
        // SQLite mode intentionally starts empty. Legacy msgpack files and
        // announce cache files are neither read nor imported; this avoids
        // silently rebuilding the large in-memory tables on first startup.
        self.recent_announces.clear();
        self.path_table.clear();
        self.tunnel_table.clear();
        self.pending_path_entries.clear();
        self.pending_tunnel_entries.clear();
        Ok(())
    }

    pub(super) fn record_sqlite_announce(&mut self, dest: [u8; 16], raw: &[u8]) {
        let Some(state) = self.sqlite.as_mut() else {
            return;
        };
        let Some(a) = self.recent_announces.get(&dest) else {
            return;
        };
        state
            .flush_at
            .get_or_insert_with(|| tokio::time::Instant::now() + state.batch_delay);
        // Discovery replies must not wait for a batching timer.
        if rns_wire::header::PacketHeader::unpack(raw)
            .is_ok_and(|(h, _)| h.context == rns_wire::context::PacketContext::PathResponse)
        {
            state.flush_at = Some(tokio::time::Instant::now());
            state.path_response_flush = true;
        }
        state.writes.push(Mutation::PutAnnounce {
            announce: a.clone(),
            raw: Some(raw.to_vec()),
        });
        state.metrics.max_staged_bytes = state
            .metrics
            .max_staged_bytes
            .max(storage::mutation_batch_bytes(&state.writes));
        state.order.retain(|d| *d != dest);
        state.order.push_back(dest);
    }

    pub(super) fn enqueue_sqlite(&mut self, msg: TransportMessage) {
        let priority = self.sqlite_packet_priority(&msg);
        let state = self.sqlite.as_mut().unwrap();
        if let Some(error) = &state.error {
            reject(msg, error);
            return;
        }
        let bytes = weight(&msg);
        // Reserve admission for link setup / discovery by evicting lower
        // priority inbound traffic. RPC mutations retain their FIFO ordering.
        while priority > 0
            && (state.queue.len() >= QUEUE_ENTRIES || state.bytes + bytes > QUEUE_BYTES)
        {
            let Some(index) = state.queue.iter().rposition(|queued| {
                matches!(
                    &queued.message,
                    TransportMessage::Inbound(_) | TransportMessage::AdmittedInbound(_)
                ) && queued.priority == 0
            }) else {
                break;
            };
            let removed = state.queue.remove(index).unwrap();
            state.bytes = state.bytes.saturating_sub(weight(&removed.message));
            self.channel_drops += 1;
        }
        if state.queue.len() >= QUEUE_ENTRIES || state.bytes + bytes > QUEUE_BYTES {
            self.channel_drops += 1;
            warn!(drops = self.channel_drops, "SQLite admission queue full");
            reject(msg, "admission queue full");
            return;
        }
        state.bytes += bytes;
        state.metrics.max_queue_bytes = state.metrics.max_queue_bytes.max(state.bytes);
        state.metrics.max_queue_entries =
            state.metrics.max_queue_entries.max(state.queue.len() + 1);
        let index = if priority > 0 {
            state
                .queue
                .iter()
                .position(|queued| queued.priority < priority)
                .unwrap_or(state.queue.len())
        } else {
            state.queue.len()
        };
        state.queue.insert(
            index,
            Queued {
                enqueued: std::time::Instant::now(),
                message: msg,
                priority,
            },
        );
    }

    /// Classify the queued priority message and allow only path requests that
    /// cannot observe pending SQLite state to pass an uncommitted batch.
    fn sqlite_priority_dependency(&self, msg: &TransportMessage) -> (PriorityKind, bool) {
        use rns_wire::{
            context::PacketContext,
            flags::{DestinationType, PacketType},
        };
        let Some((header, payload)) = self.sqlite_header(msg) else {
            return (PriorityKind::Other, false);
        };
        if header.context == PacketContext::Lrproof && header.flags.packet_type == PacketType::Proof
        {
            return (PriorityKind::Lrproof, false);
        }
        if header.context == PacketContext::PathResponse
            && header.flags.packet_type == PacketType::Announce
        {
            return (PriorityKind::PathResponse, false);
        }
        if header.destination_hash != Self::path_request_dest_hash() {
            return (PriorityKind::Other, false);
        }
        let independent = header.flags.packet_type == PacketType::Data
            && header.flags.destination_type == DestinationType::Plain
            && header.context == PacketContext::None
            && payload.len() >= 16
            && {
                let dest: [u8; 16] = payload[..16].try_into().unwrap();
                self.path_table.get(&dest).is_none()
                    && self.sqlite.as_ref().is_some_and(|state| !state.writes.iter().any(|m| {
                        matches!(m, Mutation::PutAnnounce { announce, .. } if announce.dest_hash == dest)
                    }))
            };
        (PriorityKind::PathRequest, independent)
    }

    fn sqlite_packet_priority(&self, msg: &TransportMessage) -> u8 {
        // Reuse IFAC verification for the single-channel actor as well as the
        // already admitted header from the two-channel actor.
        let header = self.sqlite_header(msg).map(|(header, _)| header);
        header.map_or(0, |h| {
            if h.context == rns_wire::context::PacketContext::Lrproof
                && h.flags.packet_type == rns_wire::flags::PacketType::Proof
                && self.link_table.contains(&h.destination_hash)
            {
                2
            } else if (h.context == rns_wire::context::PacketContext::PathResponse
                && h.flags.packet_type == rns_wire::flags::PacketType::Announce)
                || h.destination_hash == Self::path_request_dest_hash()
            {
                1
            } else {
                0
            }
        })
    }

    fn sqlite_header(
        &self,
        msg: &TransportMessage,
    ) -> Option<(rns_wire::header::PacketHeader, Vec<u8>)> {
        if let TransportMessage::AdmittedInbound(p) = msg {
            return Some((p.header.clone(), p.raw[p.data_offset..].to_vec()));
        }
        let TransportMessage::Inbound(p) = msg else {
            return None;
        };
        let raw = if let Some(e) = self.interfaces.get(&p.interface_id) {
            if let Some(key) = &e.ifac_key {
                crate::ifac::ifac_verify(&p.raw, key, e.ifac_size)?
            } else {
                p.raw.to_vec()
            }
        } else {
            p.raw.to_vec()
        };
        let (header, offset) = rns_wire::header::PacketHeader::unpack(&raw).ok()?;
        Some((header, raw[offset..].to_vec()))
    }

    fn sqlite_dependent(&self, msg: &TransportMessage) -> bool {
        match msg {
            TransportMessage::CacheRequest { .. } => true,
            TransportMessage::Rpc { query, .. } => matches!(
                query,
                Q::GetRecentAnnounces
                    | Q::GetAnnouncesPage { .. }
                    | Q::Recall { .. }
                    | Q::DropRecentAnnounces
                    | Q::GetBlackholedIdentities
                    | Q::BlackholeIdentity { .. }
                    | Q::BuildBlackholeManifest { .. }
                    | Q::RetainDestination { .. }
                    | Q::RetainIdentity { .. }
                    | Q::UseDestination { .. }
                    | Q::UnretainDestination { .. }
                    | Q::CleanKnownDestinations
                    | Q::ResolveIdentityHash { .. }
                    | Q::FilterBlackholedDests { .. }
                    | Q::PurgeUnverifiedBlackholes
            ),
            TransportMessage::Inbound(_) | TransportMessage::AdmittedInbound(_) => {
                self.sqlite_header(msg).is_some_and(|(h, _)| {
                    h.flags.packet_type == rns_wire::flags::PacketType::Announce
                        || h.flags.destination_type == rns_wire::flags::DestinationType::Plain
                        || h.context == rns_wire::context::PacketContext::CacheRequest
                        || (h.context == rns_wire::context::PacketContext::Lrproof
                            && !self.sqlite_reads(msg).keys.is_empty())
                })
            }
            _ => false,
        }
    }

    fn sqlite_reads(&self, msg: &TransportMessage) -> Reads {
        let mut reads = Reads::default();
        match msg {
            TransportMessage::Inbound(_) | TransportMessage::AdmittedInbound(_) => {
                if let Some((h, payload)) = self.sqlite_header(msg) {
                    if h.flags.packet_type == rns_wire::flags::PacketType::Announce {
                        reads.keys.push(h.destination_hash);
                    }
                    if h.context == rns_wire::context::PacketContext::Lrproof {
                        if let Some(e) = self.link_table.get(&h.destination_hash) {
                            reads.keys.push(e.destination_hash);
                        }
                    }
                    if h.context == rns_wire::context::PacketContext::CacheRequest
                        && payload.len() == 32
                    {
                        reads.packets.push(payload.as_slice().try_into().unwrap());
                    }
                    if h.destination_hash == Self::path_request_dest_hash() && payload.len() >= 16 {
                        let dest: [u8; 16] = payload[..16].try_into().unwrap();
                        if let Some(hash) = self.path_table.get(&dest).and_then(|p| p.packet_hash) {
                            reads.packets.push(hash);
                        }
                    }
                }
            }
            TransportMessage::CacheRequest { packet_hash, .. } => reads.packets.push(*packet_hash),
            TransportMessage::Rpc { query, .. } => match query {
                Q::Recall { destination_hash } => reads.keys.push(*destination_hash),
                Q::ResolveIdentityHash { input } => {
                    reads.keys.push(*input);
                    reads.identities.push(*input);
                }
                Q::FilterBlackholedDests { dests } => reads.keys.extend(dests),
                Q::BlackholeIdentity { hash, .. } => reads.identities.push(*hash),
                Q::GetBlackholedIdentities
                | Q::BuildBlackholeManifest { .. }
                | Q::PurgeUnverifiedBlackholes => reads.identities.extend(
                    self.blackhole_table
                        .iter_entries()
                        .map(|(id, _)| id.into_bytes()),
                ),
                _ => {}
            },
            _ => {}
        }
        let requested = reads.keys.len();
        reads
            .keys
            .retain(|k| !self.recent_announces.contains_key(k));
        reads.cached = requested - reads.keys.len();
        reads
    }

    fn invalidate_sqlite_cache(&mut self, invalidation: Invalidation) {
        if matches!(invalidation, Invalidation::None) {
            return;
        }
        let state = self.sqlite.as_mut().unwrap();
        self.recent_announces.retain(|key, a| {
            // A foreground announce may have refreshed a row after the GC
            // transaction but before its completion was consumed by the actor.
            let dirty = state.writes.iter().any(|m| {
                matches!(m,
                Mutation::PutAnnounce { announce, .. } if announce.dest_hash == *key)
            });
            dirty
                || !match &invalidation {
                    Invalidation::None => false,
                    Invalidation::All => true,
                    Invalidation::Destinations(keys) => keys.contains(key),
                    Invalidation::Identity(hash) => a
                        .public_key
                        .is_some_and(|pk| rns_crypto::sha::truncated_hash(&pk) == *hash),
                }
        });
        state
            .order
            .retain(|key| self.recent_announces.contains_key(key));
    }

    fn trim_sqlite_cache(&mut self) {
        let state = self.sqlite.as_mut().unwrap();
        let mut bytes: usize = self
            .recent_announces
            .values()
            .map(|a| 256 + a.app_data.as_ref().map_or(0, Vec::capacity))
            .sum();
        while self.recent_announces.len() > CACHE_ENTRIES || bytes > CACHE_BYTES {
            let clean = |key: &[u8; 16]| {
                !state.writes.iter().any(|m| {
                    matches!(m,
                Mutation::PutAnnounce { announce, .. } if announce.dest_hash == *key)
                })
            };
            let key = state
                .order
                .iter()
                .copied()
                .find(clean)
                .or_else(|| self.recent_announces.keys().copied().find(clean));
            let Some(key) = key else { break };
            state.order.retain(|k| *k != key);
            if let Some(a) = self.recent_announces.remove(&key) {
                bytes = bytes.saturating_sub(256 + a.app_data.as_ref().map_or(0, Vec::capacity));
            }
        }
        state.raw.clear();
    }

    pub(super) async fn run_sqlite(mut self) -> Self {
        let mut tick = tokio::time::interval(Duration::from_millis(JOB_INTERVAL_MS));
        let mut ingress_tick = tokio::time::interval(crate::backbone_ingress::INTERVAL);
        ingress_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut job: Option<JoinHandle<storage::Result<Prepared>>> = None;
        // A sweep is many worker operations (and filesystem reads). Keeping it
        // separate lets packet preparation run between those operations. The
        // single storage worker still serializes SQL and committed mutations.
        let mut background: Option<JoinHandle<storage::Result<BackgroundResult>>> = None;
        let mut background_started = std::time::Instant::now();
        let mut invalidate_after_job = Vec::new();
        let mut stopping = false;
        let mut was_foreground = true;
        let mut interface_open = true;
        let mut control_open = self.control_rx.is_some();
        loop {
            let metrics = &mut self.sqlite.as_mut().unwrap().metrics;
            if metrics.since.elapsed() >= Duration::from_secs(60) {
                metrics.report();
            }
            if job.is_none() {
                // A prepared read may have relied on a cache hit. Invalidate
                // only after its message has consumed that entry.
                if !invalidate_after_job.is_empty() {
                    self.invalidate_sqlite_cache(Invalidation::Destinations(std::mem::take(
                        &mut invalidate_after_job,
                    )));
                }
                // Ordinary announces and independent requests for unknown paths
                // can pass staged writes. Reads of stored packets remain barriers.
                let state = self.sqlite.as_ref().unwrap();
                let staged_bytes = storage::mutation_batch_bytes(&state.writes);
                let priority_dependency = state
                    .queue
                    .front()
                    .filter(|q| q.priority != 0)
                    .map(|q| self.sqlite_priority_dependency(&q.message));
                let independent_path_request =
                    priority_dependency.is_some_and(|(_, independent)| independent);
                let next_barrier = state.queue.front().and_then(|q| {
                    if matches!(q.message, TransportMessage::Rpc { .. }) {
                        Some(FlushTrigger::Rpc)
                    } else if q.priority != 0 {
                        if independent_path_request {
                            None
                        } else {
                            Some(FlushTrigger::Priority)
                        }
                    } else if !self.sqlite_header(&q.message).is_some_and(|(h, _)| {
                        h.flags.packet_type == rns_wire::flags::PacketType::Announce
                    }) {
                        Some(FlushTrigger::PacketRead)
                    } else if state.writes.len() >= WRITE_BATCH_ITEMS
                        || staged_bytes + weight(&q.message) * 2 > WRITE_BATCH_BYTES
                    {
                        Some(FlushTrigger::NextCapacity)
                    } else {
                        None
                    }
                });
                let state = self.sqlite.as_mut().unwrap();
                let deadline = state
                    .flush_at
                    .is_some_and(|at| tokio::time::Instant::now() >= at);
                let item_limit = state.writes.len() >= WRITE_BATCH_ITEMS;
                let byte_limit = staged_bytes >= WRITE_BATCH_BYTES;
                let flush = !state.writes.is_empty()
                    && (stopping
                        || background.is_some()
                        || deadline
                        || item_limit
                        || byte_limit
                        || next_barrier.is_some());
                if flush {
                    let metrics = &mut state.metrics.flushes;
                    metrics.batches += 1;
                    metrics.items += state.writes.len() as u64;
                    metrics.singletons += u64::from(state.writes.len() == 1);
                    for (trigger, active) in [
                        (FlushTrigger::Shutdown, stopping),
                        (FlushTrigger::Background, background.is_some()),
                        (
                            FlushTrigger::PathResponse,
                            deadline && state.path_response_flush,
                        ),
                        (FlushTrigger::Timer, deadline && !state.path_response_flush),
                        (FlushTrigger::ItemLimit, item_limit),
                        (FlushTrigger::ByteLimit, byte_limit),
                    ] {
                        metrics.triggers[trigger as usize] += u64::from(active);
                    }
                    if let Some(trigger) = next_barrier {
                        metrics.triggers[trigger as usize] += 1;
                        if matches!(trigger, FlushTrigger::Priority) {
                            let (kind, _) = priority_dependency.unwrap();
                            metrics.priority_kinds[kind as usize] += 1;
                        }
                    }
                } else if !state.writes.is_empty() && independent_path_request {
                    state.metrics.flushes.unknown_path_bypasses += 1;
                }
                let message = state.queue.pop_front().map(|queued| {
                    let msg = queued.message;
                    state.bytes = state.bytes.saturating_sub(weight(&msg));
                    let admission = queued.enqueued.elapsed();
                    state.metrics.admission.record(admission);
                    let admission_ms = admission.as_millis() as u64;
                    if admission_ms >= 100 {
                        debug!(
                            admission_ms,
                            message = crate::messages::msg_variant_name(&msg),
                            priority = queued.priority,
                            queue_entries = state.queue.len(),
                            "SQLite admission delayed"
                        );
                    }
                    msg
                });
                let writes = if flush {
                    state.flush_at = None;
                    state.path_response_flush = false;
                    std::mem::replace(&mut state.writes, Vec::with_capacity(WRITE_BATCH_ITEMS))
                } else {
                    Vec::new()
                };
                let worker = state.worker.clone();
                if let Some(message) = message {
                    let reads = self.sqlite_reads(&message);
                    let metrics = &mut self.sqlite.as_mut().unwrap().metrics;
                    metrics.hits += reads.cached as u64;
                    metrics.misses += reads.keys.len() as u64;
                    job = Some(tokio::spawn(prepare(worker, writes, Some(message), reads)));
                } else if !writes.is_empty() {
                    job = Some(tokio::spawn(prepare(
                        worker,
                        writes,
                        None,
                        Reads::default(),
                    )));
                } else if !state.writes.is_empty() {
                    // Wait for input or the flush deadline. Background work and
                    // route snapshots cannot observe this uncommitted batch.
                } else if stopping && background.is_none() {
                    break;
                } else if !stopping
                    && background.is_none()
                    && crate::now_f64() - state.last_sweep >= 60.0
                    && state.error.is_none()
                    && !self
                        .routing_save_in_flight
                        .load(std::sync::atomic::Ordering::Acquire)
                {
                    state.last_sweep = crate::now_f64();
                    let keep = self.sqlite_live_packets();
                    let directory = self.storage_dir.clone().unwrap();
                    background = Some(tokio::spawn(async move {
                        let started = std::time::Instant::now();
                        let result = sweep(&worker, directory, keep).await;
                        tracing::debug!(
                            duration_ms = started.elapsed().as_millis() as u64,
                            failed = result.is_err(),
                            "SQLite sweep completed"
                        );
                        result?;
                        Ok(BackgroundResult::Sweep)
                    }));
                } else if !stopping
                    && background.is_none()
                    && crate::now_f64() - state.last_vacuum >= state.vacuum_interval
                    && state.error.is_none()
                    && !self
                        .routing_save_in_flight
                        .load(std::sync::atomic::Ordering::Acquire)
                {
                    state.last_vacuum = crate::now_f64();
                    let vacuum_pages = state.vacuum_pages;
                    background = Some(tokio::spawn(async move {
                        maintain(&worker, vacuum_pages).await;
                        Ok(BackgroundResult::Maintenance)
                    }));
                } else if !stopping
                    && background.is_none()
                    && state.error.is_none()
                    && !matches!(state.gc_phase, GcPhase::Idle)
                    && tokio::time::Instant::now() >= state.next_gc
                    && self.inbound_queues.snapshot().total == 0
                    && self.rx.is_empty()
                    && self.control_rx.as_ref().is_none_or(|rx| rx.is_empty())
                {
                    // No foreground job, staged writes or queued input remains.
                    // An arrival can wait for at most this already-started page;
                    // no second GC page is queued behind it.
                    let phase = state.gc_phase;
                    background_started = std::time::Instant::now();
                    background = Some(tokio::spawn(gc_page(worker, phase)));
                }
            }
            self.sqlite.as_mut().unwrap().busy = job.is_some() || background.is_some();
            let flush_at = self.sqlite.as_ref().unwrap().flush_at;
            tokio::select! {
                _ = tokio::time::sleep_until(flush_at.unwrap_or_else(tokio::time::Instant::now)),
                    if job.is_none() && flush_at.is_some() => {},
                _ = tokio::time::sleep_until(self.sqlite.as_ref().unwrap().next_gc),
                    if !stopping && background.is_none()
                        && !matches!(self.sqlite.as_ref().unwrap().gc_phase, GcPhase::Idle)
                        && self.sqlite.as_ref().unwrap().next_gc > tokio::time::Instant::now() => {},
                _ = ingress_tick.tick(), if !stopping => self.evaluate_dataplane_ingress(false),
                _=std::future::ready(()), if !stopping && self.inbound_queues.snapshot().total > 0 => {
                    let packet = self.inbound_queues.pop().unwrap();
                    let msg = TransportMessage::AdmittedInbound(packet);
                    if self.sqlite_dependent(&msg) {self.enqueue_sqlite(msg);} else {self.handle_message(msg);}
                },
                result=async {job.as_mut().unwrap().await}, if job.is_some()=> {
                    job=None;
                    self.sqlite.as_mut().unwrap().busy=background.is_some();
                    match result {
                        Ok(Ok(prepared))=> {
                            self.invalidate_sqlite_cache(prepared.invalidate);
                            for a in prepared.entries {
                                let state=self.sqlite.as_mut().unwrap();
                                state.order.retain(|d|*d!=a.dest_hash);state.order.push_back(a.dest_hash);
                                self.recent_announces.insert(a.dest_hash,a);
                            }
                            self.sqlite.as_mut().unwrap().raw=prepared.raw;
                            if let Some(msg)=prepared.message {self.handle_message(msg);}
                            self.trim_sqlite_cache();
                        }
                        error=> {
                            let error=format!("{error:?}");
                            tracing::error!(%error,"SQLite storage failed; announce admission suspended");
                            let state=self.sqlite.as_mut().unwrap();
                            state.error=Some(error.clone());
                            for queued in state.queue.drain(..) {reject(queued.message,&error);}
                            state.bytes=0;
                        }
                    }
                }
                result=async {background.as_mut().unwrap().await}, if background.is_some()=> {
                    background=None;
                    self.sqlite.as_mut().unwrap().busy=job.is_some();
                    match result {
                        Ok(Ok(result)) => {
                            let state = self.sqlite.as_mut().unwrap();
                            if matches!(&result, BackgroundResult::Cleaned { .. } | BackgroundResult::Collected { .. }) {
                                state.gc_pages += 1;
                                state.gc_page_time += background_started.elapsed();
                            }
                            match result {
                                BackgroundResult::Sweep => {
                                    state.last_sweep = crate::now_f64();
                                    // Refreshing pins must not restart a partially
                                    // completed GC scan at the first key.
                                    if matches!(state.gc_phase, GcPhase::Idle) {
                                        state.gc_phase = GcPhase::Clean(None);
                                        state.gc_started = Some(std::time::Instant::now());
                                        state.gc_announces = 0;
                                        state.gc_packets = 0;
                                        state.gc_pages = 0;
                                        state.gc_page_time = Duration::ZERO;
                                    }
                                }
                                BackgroundResult::Maintenance => {}
                                BackgroundResult::Cleaned { next, removed, destinations } => {
                                    state.gc_announces += removed;
                                    invalidate_after_job = destinations;
                                    state.gc_phase = match next {
                                        Some(key) => GcPhase::Clean(Some(key)),
                                        None => GcPhase::Collect(None),
                                    };
                                    state.next_gc = gc_resume_at(background_started.elapsed());
                                }
                                BackgroundResult::Collected { next, removed } => {
                                    state.gc_packets += removed;
                                    state.gc_phase = match next {
                                        Some(key) => GcPhase::Collect(Some(key)),
                                        None => GcPhase::Idle,
                                    };
                                    state.next_gc = gc_resume_at(background_started.elapsed());
                                    if matches!(state.gc_phase, GcPhase::Idle) {
                                        tracing::debug!(
                                            duration_ms = state.gc_started.take().map_or(0, |t| t.elapsed().as_millis() as u64),
                                            pages = state.gc_pages,
                                            page_time_ms = state.gc_page_time.as_millis() as u64,
                                            removed_announces = state.gc_announces,
                                            removed_packets = state.gc_packets,
                                            "SQLite garbage collection completed"
                                        );
                                    }
                                }
                            }
                        }
                        error => {
                            let error=format!("{error:?}");
                            tracing::error!(%error,"SQLite background storage failed; announce admission suspended");
                            let state=self.sqlite.as_mut().unwrap();
                            state.error=Some(error.clone());
                            for queued in state.queue.drain(..) {reject(queued.message,&error);}
                            state.bytes=0;
                        }
                    }
                }
                msg=async { self.control_rx.as_mut().unwrap().recv().await }, if !stopping && control_open=>match msg {
                    None=>{control_open=false; stopping=!interface_open && self.inbound_queues.snapshot().total == 0;},
                    Some(TransportMessage::Shutdown)=>{stopping=true;},
                    Some(TransportMessage::SetStoragePaths {..})=>warn!("cannot change active SQLite storage ownership"),
                    Some(msg)=> {
                        self.accept_sqlite_message(msg);
                    }
                },
                msg=self.rx.recv(), if !stopping && interface_open=>match msg {
                    None=>{interface_open=false; stopping=!control_open && self.inbound_queues.snapshot().total == 0;},
                    Some(TransportMessage::Shutdown)=>{stopping=true;},
                    Some(TransportMessage::SetStoragePaths {..})=>warn!("cannot change active SQLite storage ownership"),
                    Some(msg)=> {
                        self.accept_sqlite_message(msg);
                    }
                },
                _=tick.tick(), if !stopping=> {
                    let fg=self.is_foreground.load(std::sync::atomic::Ordering::Relaxed);
                    if fg!=was_foreground { if fg {self.on_resume();} else {self.save_state_async();} was_foreground=fg; }
                    self.on_tick();
                }
            }
            if !interface_open && !control_open && self.inbound_queues.snapshot().total == 0 {
                stopping = true;
            }
        }
        self.sqlite.as_mut().unwrap().metrics.report();
        self.on_shutdown();
        let worker = &self.sqlite.as_ref().unwrap().worker;
        match worker.try_shutdown() {
            Ok(p) => {
                if let Err(error) = p.wait().await {
                    tracing::error!(%error,"SQLite shutdown failed");
                }
            }
            Err(e) => tracing::error!(error=%e.error,"SQLite shutdown rejected"),
        }
        self
    }

    fn accept_sqlite_message(&mut self, msg: TransportMessage) {
        if matches!(&msg, TransportMessage::Inbound(_)) && self.control_rx.is_some() {
            self.handle_message(msg);
        } else if self.sqlite_dependent(&msg) {
            self.enqueue_sqlite(msg);
        } else {
            self.handle_message(msg);
        }
    }

    fn sqlite_live_packets(&self) -> Vec<[u8; 32]> {
        let mut keep: Vec<_> = self
            .path_table
            .iter()
            .filter_map(|(_, p)| p.packet_hash)
            .collect();
        for (_, t) in self.tunnel_table.iter() {
            keep.extend(t.tunnel_paths.values().filter_map(|p| p.packet_hash));
        }
        for p in &self.pending_path_entries {
            if let Some(h) = p
                .packet_hash
                .as_ref()
                .and_then(|h| h.as_slice().try_into().ok())
            {
                keep.push(h);
            }
        }
        for t in &self.pending_tunnel_entries {
            for p in &t.paths {
                if let Some(h) = p
                    .packet_hash
                    .as_ref()
                    .and_then(|h| h.as_slice().try_into().ok())
                {
                    keep.push(h);
                }
            }
        }
        keep
    }
}

async fn maintain(worker: &StorageHandle, vacuum_pages: u32) {
    let started = std::time::Instant::now();
    match call(worker, Request::Maintain { vacuum_pages }).await {
        Ok(Reply::Maintenance(stats)) => tracing::debug!(
            database_bytes = stats.database_bytes,
            wal_bytes = stats.wal_bytes,
            page_size = stats.page_size,
            page_count = stats.page_count,
            free_pages = stats.free_pages,
            checkpointed_frames = stats.checkpointed_frames,
            remaining_wal_frames = stats.remaining_wal_frames,
            vacuumed_pages = stats.vacuumed_pages,
            page_cache_kib = stats.page_cache_kib,
            duration_ms = started.elapsed().as_millis(),
            "SQLite maintenance completed"
        ),
        Ok(_) => tracing::warn!("SQLite maintenance returned an invalid reply"),
        Err(error) => tracing::warn!(%error, "SQLite maintenance failed"),
    }
}

async fn prepare(
    worker: StorageHandle,
    writes: Vec<Mutation>,
    message: Option<TransportMessage>,
    reads: Reads,
) -> storage::Result<Prepared> {
    let started = std::time::Instant::now();
    let message_kind = message
        .as_ref()
        .map(crate::messages::msg_variant_name)
        .unwrap_or("writes");
    let result = prepare_inner(worker, writes, message, reads).await;
    let preparation_ms = started.elapsed().as_millis() as u64;
    if preparation_ms >= 100 {
        debug!(
            preparation_ms,
            message = message_kind,
            failed = result.is_err(),
            "SQLite preparation delayed"
        );
    }
    result
}

async fn prepare_inner(
    worker: StorageHandle,
    writes: Vec<Mutation>,
    mut message: Option<TransportMessage>,
    reads: Reads,
) -> storage::Result<Prepared> {
    write(&worker, writes).await?;
    let mut result = Prepared {
        message: None,
        entries: Vec::new(),
        raw: HashMap::new(),
        invalidate: Invalidation::None,
    };
    // These RPCs are answered directly from the source of truth. Only the
    // existing explicit full-list API materializes the complete response.
    if let Some(TransportMessage::Rpc { query, .. }) = &message {
        if matches!(
            query,
            Q::GetRecentAnnounces
                | Q::GetAnnouncesPage { .. }
                | Q::DropRecentAnnounces
                | Q::RetainDestination { .. }
                | Q::RetainIdentity { .. }
                | Q::UseDestination { .. }
                | Q::UnretainDestination { .. }
                | Q::CleanKnownDestinations
        ) {
            let TransportMessage::Rpc { query, response_tx } = message.take().unwrap() else {
                unreachable!()
            };
            let invalidation = match &query {
                Q::GetRecentAnnounces | Q::GetAnnouncesPage { .. } => Invalidation::None,
                Q::RetainDestination { dest }
                | Q::UnretainDestination { dest }
                | Q::UseDestination { dest } => Invalidation::Destinations(vec![*dest]),
                Q::RetainIdentity { identity_hash } => Invalidation::Identity(*identity_hash),
                _ => Invalidation::All,
            };
            let response = direct_query(&worker, query).await;
            match response {
                Ok(response) => {
                    let _ = response_tx.send(response);
                }
                Err(error) => {
                    let _ = response_tx.send(R::Error(error.to_string()));
                    return Err(error);
                }
            }
            result.invalidate = invalidation;
            return Ok(result);
        }
    }
    for key in reads.keys {
        if let Reply::Announce(Some(a)) = call(&worker, Request::Announce(key)).await? {
            result.entries.push(a);
        }
    }
    let all_identity_destinations = matches!(
        &message,
        Some(TransportMessage::Rpc {
            query: Q::BlackholeIdentity { .. },
            ..
        })
    );
    for identity in reads.identities {
        let mut cursor = None;
        loop {
            let p = page(
                &worker,
                AnnouncePageQuery {
                    identity_hash: Some(identity),
                    after: cursor,
                    limit: if all_identity_destinations {
                        storage::MAX_PAGE_ITEMS
                    } else {
                        1
                    },
                    ..Default::default()
                },
            )
            .await?;
            if p.entries.is_empty() {
                break;
            }
            cursor = p.next;
            result.entries.extend(p.entries);
            if !all_identity_destinations {
                break;
            }
        }
    }
    for hash in reads.packets {
        if let Reply::Packet(Some(raw)) = call(&worker, Request::Packet(hash)).await? {
            if let Ok((h, _)) = rns_wire::header::PacketHeader::unpack(&raw) {
                if let Reply::Announce(Some(a)) =
                    call(&worker, Request::Announce(h.destination_hash)).await?
                {
                    result.entries.push(a);
                }
            }
            result.raw.insert(hash, raw);
        }
    }
    result.message = message;
    Ok(result)
}

async fn direct_query(worker: &StorageHandle, query: Q) -> storage::Result<R> {
    Ok(match query {
        Q::GetAnnouncesPage { after, limit } => {
            if limit > storage::MAX_PAGE_ITEMS {
                return Ok(R::Error("announce page limit exceeded".into()));
            }
            let p = page(
                worker,
                AnnouncePageQuery {
                    after,
                    limit,
                    ..Default::default()
                },
            )
            .await?;
            R::Announces(p.entries.into_iter().map(dto).collect())
        }
        Q::GetRecentAnnounces => {
            let mut entries = Vec::new();
            let mut cursor = None;
            loop {
                let p = page(
                    worker,
                    AnnouncePageQuery {
                        after: cursor,
                        ..Default::default()
                    },
                )
                .await?;
                if p.entries.is_empty() {
                    break;
                }
                cursor = p.next;
                entries.extend(p.entries.into_iter().map(dto));
            }
            entries.sort_by(|a, b| b.timestamp.total_cmp(&a.timestamp));
            R::Announces(entries)
        }
        Q::DropRecentAnnounces => match call(worker, Request::ClearAnnounces).await? {
            Reply::Removed(n) => R::IntResult(n as i64),
            _ => unreachable!(),
        },
        Q::RetainIdentity { identity_hash } => {
            let mut cursor = None;
            let mut found = false;
            loop {
                let p = page(
                    worker,
                    AnnouncePageQuery {
                        identity_hash: Some(identity_hash),
                        after: cursor,
                        ..Default::default()
                    },
                )
                .await?;
                if p.entries.is_empty() {
                    break;
                }
                cursor = p.next;
                found = true;
                write(
                    worker,
                    p.entries
                        .into_iter()
                        .map(|a| Mutation::SetRetained {
                            destination: a.dest_hash,
                            retained: true,
                        })
                        .collect(),
                )
                .await?;
            }
            R::BoolResult(found)
        }
        Q::RetainDestination { dest }
        | Q::UnretainDestination { dest }
        | Q::UseDestination { dest } => {
            let found = matches!(
                call(worker, Request::Announce(dest)).await?,
                Reply::Announce(Some(_))
            );
            if found {
                let m = match query {
                    Q::UseDestination { .. } => Mutation::Touch {
                        destination: dest,
                        at: crate::now_f64(),
                    },
                    _ => Mutation::SetRetained {
                        destination: dest,
                        retained: matches!(query, Q::RetainDestination { .. }),
                    },
                };
                write(worker, vec![m]).await?;
            }
            R::BoolResult(found)
        }
        Q::CleanKnownDestinations => {
            // The periodic keep-set sweep supplies both live and durable paths.
            // Conservative between sweeps: retaining an extra entry is safe.
            clean(worker).await?;
            match call(worker, Request::Stats).await? {
                Reply::Stats(s) => R::IntResult(s.announces as i64),
                _ => unreachable!(),
            }
        }
        _ => unreachable!(),
    })
}

async fn clean(worker: &StorageHandle) -> storage::Result<()> {
    let now = crate::now_f64();
    let mut after = None;
    loop {
        let Reply::CleanedPage { next, .. } = call(
            worker,
            Request::CleanKnownPage {
                unused_before: now - UNUSED_DESTINATION_LINGER,
                used_before: now - DESTINATION_TIMEOUT as f64 * 1.25,
                after,
                limit: storage::MAX_BATCH_ITEMS,
            },
        )
        .await?
        else {
            return Err(storage::StorageError::Invalid("cleanup page reply"));
        };
        let Some(next) = next else { break };
        after = Some(next);
    }
    Ok(())
}

fn gc_resume_at(elapsed: Duration) -> tokio::time::Instant {
    // At most roughly 25% of worker time for a GC backlog, even while idle.
    // Input pressure can defer it further. A single SQL operation cannot be
    // preempted, hence the smaller page bound above.
    tokio::time::Instant::now() + elapsed.saturating_mul(GC_REST_FACTOR)
}

async fn gc_page(worker: StorageHandle, phase: GcPhase) -> storage::Result<BackgroundResult> {
    match phase {
        GcPhase::Clean(after) => {
            let now = crate::now_f64();
            match call(
                &worker,
                Request::CleanKnownPage {
                    unused_before: now - UNUSED_DESTINATION_LINGER,
                    used_before: now - DESTINATION_TIMEOUT as f64 * 1.25,
                    after,
                    limit: GC_PAGE_ENTRIES,
                },
            )
            .await?
            {
                Reply::CleanedPage {
                    next,
                    removed,
                    destinations,
                } => Ok(BackgroundResult::Cleaned {
                    next,
                    removed,
                    destinations,
                }),
                _ => Err(storage::StorageError::Invalid("cleanup page reply")),
            }
        }
        GcPhase::Collect(after) => match call(
            &worker,
            Request::CollectPacketsPage {
                after,
                limit: GC_PAGE_ENTRIES,
            },
        )
        .await?
        {
            Reply::CollectedPage { next, removed } => {
                Ok(BackgroundResult::Collected { next, removed })
            }
            _ => Err(storage::StorageError::Invalid("collection page reply")),
        },
        GcPhase::Idle => Err(storage::StorageError::Invalid("idle GC page")),
    }
}

async fn sweep(
    worker: &StorageHandle,
    directory: PathBuf,
    mut keep: Vec<[u8; 32]>,
) -> storage::Result<()> {
    let Reply::Generation(generation) = call(worker, Request::BeginSweep).await? else {
        unreachable!()
    };
    // This compact snapshot preserves the actor's view while it continues to
    // mutate routes. Do not hold a second hash set or decoded disk snapshot.
    keep.sort_unstable();
    keep.dedup();
    for chunk in keep.chunks(storage::MAX_BATCH_ITEMS) {
        call(
            worker,
            Request::KeepPackets {
                generation,
                hashes: chunk.to_vec(),
            },
        )
        .await?;
    }
    drop(keep);
    let owned_worker = worker.clone();
    tokio::task::spawn_blocking(move || -> storage::Result<()> {
        let runtime = tokio::runtime::Handle::current();
        let mut chunk = Vec::with_capacity(storage::MAX_BATCH_ITEMS);
        let flush = |chunk: &mut Vec<[u8; 32]>| -> storage::Result<()> {
            if chunk.is_empty() {
                return Ok(());
            }
            chunk.sort_unstable();
            chunk.dedup();
            let hashes = std::mem::replace(chunk, Vec::with_capacity(storage::MAX_BATCH_ITEMS));
            runtime.block_on(call(
                &owned_worker,
                Request::KeepPackets { generation, hashes },
            ))?;
            Ok(())
        };
        for (file, kind) in [
            ("path_table.msgpack", storage::snapshot::Kind::Paths),
            ("tunnel_table.msgpack", storage::snapshot::Kind::Tunnels),
        ] {
            let path = directory.join(file);
            // Missing snapshots are normal on first start. Other IO errors
            // must abort the sweep, preserving all old and newly staged pins.
            match storage::snapshot::visit(&path, kind, &mut |hash| {
                chunk.push(hash);
                if chunk.len() == storage::MAX_BATCH_ITEMS {
                    flush(&mut chunk)?;
                }
                Ok(())
            }) {
                Err(storage::StorageError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
                result => result?,
            }
        }
        flush(&mut chunk)
    })
    .await
    .map_err(|_| storage::StorageError::Closed)??;
    call(worker, Request::FinishSweep { generation }).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{make_test_interface, make_valid_announce};
    use super::*;

    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!(
            "rns-actor-sqlite-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
    async fn query(tx: &mpsc::Sender<TransportMessage>, query: Q) -> R {
        let (response_tx, rx) = tokio::sync::oneshot::channel();
        tx.send(TransportMessage::Rpc { query, response_tx })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), rx)
            .await
            .unwrap()
            .unwrap()
    }
    fn inbound(raw: Bytes) -> TransportMessage {
        TransportMessage::Inbound(crate::messages::InboundPacket {
            raw,
            interface_id: 1,
            rssi: None,
            snr: None,
            q: None,
        })
    }

    type Batches = std::sync::Arc<std::sync::Mutex<Vec<(usize, usize)>>>;

    #[test]
    fn memory_announce_pages_obey_byte_budget_and_key_order() {
        let (mut actor, _tx) = TransportActor::new();
        for n in (1..=20).rev() {
            let key = [n; 16];
            actor.recent_announces.insert(
                key,
                RecentAnnounce {
                    dest_hash: key,
                    hops: 1,
                    app_data: Some(vec![0; 32 * 1024]),
                    timestamp: 0.0,
                    public_key: None,
                    ratchet: None,
                    packet_hash: None,
                    is_path_response: false,
                    retained: false,
                    last_used: None,
                    name_hash: [0; 10],
                },
            );
        }
        let mut after = None;
        let mut count = 0;
        loop {
            let R::Announces(entries) =
                actor.handle_query(Q::GetAnnouncesPage { after, limit: 128 })
            else {
                panic!()
            };
            if entries.is_empty() {
                break;
            }
            assert!(
                entries.len() <= 7,
                "byte budget must apply before the row limit"
            );
            for entry in entries {
                assert!(after.is_none_or(|key| entry.dest_hash > key));
                after = Some(entry.dest_hash);
                count += 1;
            }
        }
        assert_eq!(count, 20);
    }

    #[tokio::test]
    async fn corrupt_streamed_snapshot_preserves_old_and_new_pins() {
        let (actor, _tx, _, dir) = batch_actor(false).await;
        let worker = &actor.sqlite.as_ref().unwrap().worker;
        let mut hashes = Vec::new();
        for _ in 0..2 {
            let (raw, _) = make_valid_announce("lxmf.delivery", 1);
            let (header, _) = rns_wire::header::PacketHeader::unpack(&raw).unwrap();
            let hash = rns_wire::hash::packet_hash(&raw, header.flags.header_type);
            write(
                worker,
                vec![Mutation::PutPacket {
                    hash,
                    raw: raw.to_vec(),
                }],
            )
            .await
            .unwrap();
            hashes.push(hash);
        }
        sweep(worker, dir.clone(), vec![hashes[0]]).await.unwrap();
        // Root array and entries are readable; the version field is truncated.
        std::fs::write(dir.join("path_table.msgpack"), [0x92, 0x90]).unwrap();
        assert!(sweep(worker, dir.clone(), vec![hashes[1]]).await.is_err());
        assert!(matches!(
            call(worker, Request::CollectPackets { limit: 128 })
                .await
                .unwrap(),
            Reply::Removed(0)
        ));
        for hash in hashes {
            assert!(matches!(
                call(worker, Request::Packet(hash)).await.unwrap(),
                Reply::Packet(Some(_))
            ));
        }
        worker.try_shutdown().unwrap().wait().await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    async fn batch_actor(
        fail: bool,
    ) -> (
        TransportActor,
        mpsc::Sender<TransportMessage>,
        Batches,
        PathBuf,
    ) {
        struct Observed {
            db: storage::MemoryTransportStorage,
            batches: Batches,
            fail: bool,
        }
        impl storage::TransportStorage for Observed {
            fn execute(&mut self, request: Request) -> storage::Result<Reply> {
                if let Request::Apply(mutations) = &request {
                    self.batches
                        .lock()
                        .unwrap()
                        .push((mutations.len(), storage::mutation_batch_bytes(mutations)));
                    if self.fail {
                        return Err(storage::StorageError::Invalid("injected commit failure"));
                    }
                }
                self.db.execute(request)
            }
        }
        let dir = temp();
        let (mut actor, tx) = TransportActor::new();
        actor.initialize_sqlite_storage(dir.clone()).await.unwrap();
        actor
            .sqlite
            .as_ref()
            .unwrap()
            .worker
            .try_shutdown()
            .unwrap()
            .wait()
            .await
            .unwrap();
        let batches = Batches::default();
        let observed = batches.clone();
        actor.sqlite.as_mut().unwrap().worker = StorageHandle::start(8, move || {
            Ok(Observed {
                db: Default::default(),
                batches: observed,
                fail,
            })
        })
        .await
        .unwrap();
        actor.sqlite.as_mut().unwrap().last_sweep = crate::now_f64();
        let (mut interface, _) = make_test_interface("batch");
        interface.ingress = crate::ingress::IngressController::disabled();
        actor.interfaces.insert(1, interface);
        (actor, tx, batches, dir)
    }

    fn path_request_message(dest: [u8; 16]) -> TransportMessage {
        use rns_wire::flags::*;
        let header = rns_wire::header::PacketHeader {
            flags: PacketFlags {
                header_type: HeaderType::Header1,
                context_flag: false,
                transport_type: TransportType::Broadcast,
                destination_type: DestinationType::Plain,
                packet_type: PacketType::Data,
            },
            hops: 0,
            transport_id: None,
            destination_hash: TransportActor::path_request_dest_hash(),
            context: rns_wire::context::PacketContext::None,
        };
        let mut raw = header.pack().unwrap();
        raw.extend_from_slice(&dest);
        raw.extend_from_slice(&[0xBA; 16]);
        inbound(Bytes::from(raw))
    }

    #[tokio::test]
    async fn unknown_path_request_forwards_without_flushing_unrelated_announces() {
        for fail in [false, true] {
            let (mut actor, tx, batches, dir) = batch_actor(fail).await;
            actor.is_transport_enabled = true;
            actor.sqlite.as_mut().unwrap().batch_delay = Duration::from_secs(10);
            let (raw, dest) = make_valid_announce("lxmf.delivery", 1);
            actor.handle_message(inbound(raw));
            assert_eq!(actor.sqlite.as_ref().unwrap().writes.len(), 1);
            let unknown = [0xF3; 16];
            assert_eq!(
                actor.sqlite_priority_dependency(&path_request_message(unknown)),
                (PriorityKind::PathRequest, true)
            );
            assert_eq!(
                actor.sqlite_priority_dependency(&path_request_message(dest)),
                (PriorityKind::PathRequest, false)
            );
            let (mut outgoing, mut output) = make_test_interface("path-request-output");
            outgoing.ingress = crate::ingress::IngressController::disabled();
            actor.interfaces.insert(2, outgoing);
            let task = tokio::spawn(actor.run_sqlite());
            tx.send(path_request_message(unknown)).await.unwrap();
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let raw = output.recv().await.unwrap();
                    let (header, offset) = rns_wire::header::PacketHeader::unpack(&raw).unwrap();
                    if header.destination_hash == TransportActor::path_request_dest_hash() {
                        assert_eq!(&raw[offset..offset + 16], &unknown);
                        break;
                    }
                }
            })
            .await
            .unwrap();
            assert!(
                batches.lock().unwrap().is_empty(),
                "unknown-path forwarding must not commit unrelated data, even if commit would fail"
            );
            // A path request for the staged destination must still commit first.
            tx.send(path_request_message(dest)).await.unwrap();
            tokio::time::timeout(Duration::from_secs(2), async {
                while batches.lock().unwrap().is_empty() {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
            .await
            .unwrap();
            let recalled = query(
                &tx,
                Q::Recall {
                    destination_hash: dest,
                },
            )
            .await;
            assert!(if fail {
                matches!(recalled, R::Error(_))
            } else {
                matches!(recalled, R::Announce(Some(_)))
            });
            assert_eq!(batches.lock().unwrap().len(), 1);
            tx.send(TransportMessage::Shutdown).await.unwrap();
            let actor = task.await.unwrap();
            assert_eq!(actor.sqlite_snapshot_ready(), !fail);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn configured_batch_window_starts_at_first_write_without_sliding() {
        let dir = temp();
        let (mut actor, _tx) = TransportActor::new();
        actor
            .initialize_sqlite_storage_with_options(
                dir.clone(),
                storage::SqliteOptions {
                    announce_batch_delay: Duration::from_millis(10000),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let (raw, dest) = make_valid_announce("lxmf.delivery", 1);
        let before = tokio::time::Instant::now();
        actor.handle_message(inbound(raw.clone()));
        let after = tokio::time::Instant::now();
        let deadline = actor.sqlite.as_ref().unwrap().flush_at.unwrap();
        assert!(deadline >= before + Duration::from_millis(10000));
        assert!(deadline <= after + Duration::from_millis(10000));
        actor.record_sqlite_announce(dest, &raw);
        assert_eq!(actor.sqlite.as_ref().unwrap().flush_at, Some(deadline));
        actor
            .sqlite
            .as_ref()
            .unwrap()
            .worker
            .try_shutdown()
            .unwrap()
            .wait()
            .await
            .unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn announce_batch_is_bounded_and_rpc_observes_committed_rows() {
        let (mut actor, tx, batches, dir) = batch_actor(false).await;
        // Make the item limit deterministic even on a very slow debug runner.
        actor.sqlite.as_mut().unwrap().flush_at =
            Some(tokio::time::Instant::now() + Duration::from_secs(3600));
        let mut destinations = Vec::new();
        for _ in 0..WRITE_BATCH_ITEMS {
            let (raw, dest) = make_valid_announce("lxmf.delivery", 1);
            destinations.push(dest);
            actor.enqueue_sqlite(inbound(raw));
        }
        let task = tokio::spawn(actor.run_sqlite());
        assert!(matches!(
            query(
                &tx,
                Q::RetainDestination {
                    dest: destinations[0]
                }
            )
            .await,
            R::BoolResult(true)
        ));
        let recorded = batches.lock().unwrap().clone();
        assert_eq!(recorded[0].0, WRITE_BATCH_ITEMS);
        assert_eq!(
            recorded[1].0, 1,
            "RPC pin is committed after the announce batch"
        );
        assert!(
            recorded
                .iter()
                .all(|(n, bytes)| *n <= WRITE_BATCH_ITEMS && *bytes <= WRITE_BATCH_BYTES)
        );
        for dest in destinations {
            assert!(matches!(
                query(
                    &tx,
                    Q::Recall {
                        destination_hash: dest
                    }
                )
                .await,
                R::Announce(Some(_))
            ));
        }
        tx.send(TransportMessage::Shutdown).await.unwrap();
        let actor = task.await.unwrap();
        assert!(actor.sqlite_snapshot_ready());
        assert!(actor.sqlite.as_ref().unwrap().writes.is_empty());
        assert_eq!(actor.channel_drops, 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn lone_announce_flushes_on_timer_and_failed_batch_blocks_snapshot() {
        for fail in [false, true] {
            let (mut actor, tx, batches, dir) = batch_actor(fail).await;
            let (raw, dest) = make_valid_announce("lxmf.delivery", 1);
            actor.enqueue_sqlite(inbound(raw));
            let task = tokio::spawn(actor.run_sqlite());
            // No RPC or shutdown is available to force this flush.
            tokio::time::timeout(Duration::from_secs(2), async {
                while batches.lock().unwrap().is_empty() {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
            .await
            .unwrap();
            let recalled = query(
                &tx,
                Q::Recall {
                    destination_hash: dest,
                },
            )
            .await;
            if fail {
                assert!(matches!(recalled, R::Error(_)));
            } else {
                assert!(matches!(recalled, R::Announce(Some(_))));
            }
            tx.send(TransportMessage::Shutdown).await.unwrap();
            let actor = task.await.unwrap();
            assert_eq!(actor.sqlite_snapshot_ready(), !fail);
            assert_eq!(actor.sqlite.as_ref().unwrap().error.is_some(), fail);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn read_only_rpc_keeps_cache_and_gc_invalidation_preserves_dirty_rows() {
        let (mut actor, _tx, _, dir) = batch_actor(false).await;
        let (raw, dest) = make_valid_announce("lxmf.delivery", 1);
        actor.handle_message(inbound(raw));
        actor.invalidate_sqlite_cache(Invalidation::Destinations(vec![dest]));
        assert!(actor.recent_announces.contains_key(&dest));
        let writes = std::mem::take(&mut actor.sqlite.as_mut().unwrap().writes);
        let worker = actor.sqlite.as_ref().unwrap().worker.clone();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let prepared = prepare_inner(
            worker.clone(),
            writes,
            Some(TransportMessage::Rpc {
                query: Q::GetRecentAnnounces,
                response_tx,
            }),
            Reads::default(),
        )
        .await
        .unwrap();
        assert!(matches!(response_rx.await.unwrap(), R::Announces(entries) if entries.len() == 1));
        actor.invalidate_sqlite_cache(prepared.invalidate);
        assert!(actor.recent_announces.contains_key(&dest));
        actor.invalidate_sqlite_cache(Invalidation::Destinations(vec![dest]));
        assert!(!actor.recent_announces.contains_key(&dest));
        worker.try_shutdown().unwrap().wait().await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lrproof_loads_destination_identity_before_transit_validation() {
        let (mut actor, _tx) = TransportActor::new();
        let link_id = [0xA1; 16];
        let destination_hash = [0xB2; 16];
        actor.link_table.insert(
            link_id,
            crate::link_table::LinkEntry {
                timestamp: crate::now_f64(),
                next_hop: None,
                interface_id: 1,
                remaining_hops: 1,
                destination_hash,
                established: false,
                validated: false,
                proof_timeout: crate::now_f64() + 60.0,
                receiving_interface: 2,
                taken_hops: 0,
            },
        );
        let flags = rns_wire::flags::PacketFlags {
            header_type: rns_wire::flags::HeaderType::Header1,
            context_flag: false,
            transport_type: rns_wire::flags::TransportType::Broadcast,
            destination_type: rns_wire::flags::DestinationType::Link,
            packet_type: rns_wire::flags::PacketType::Proof,
        };
        let header = rns_wire::header::PacketHeader {
            flags,
            hops: 1,
            transport_id: None,
            destination_hash: link_id,
            context: rns_wire::context::PacketContext::Lrproof,
        };
        let mut raw = header.pack().expect("locally constructed header");
        raw.extend_from_slice(&[0; 96]);
        let message = inbound(Bytes::from(raw));

        assert!(actor.sqlite_dependent(&message));
        assert_eq!(actor.sqlite_reads(&message).keys, vec![destination_hash]);
    }

    /// Stop the first sweep operation without tying the test to disk speed.
    struct PausedSweep {
        db: storage::MemoryTransportStorage,
        started: Option<tokio::sync::oneshot::Sender<()>>,
        release: std::sync::mpsc::Receiver<()>,
        operations: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
    }
    impl storage::TransportStorage for PausedSweep {
        fn execute(&mut self, request: Request) -> storage::Result<Reply> {
            self.operations.lock().unwrap().push(request.operation());
            if matches!(request, Request::BeginSweep) {
                if let Some(started) = self.started.take() {
                    let _ = started.send(());
                    self.release.recv_timeout(Duration::from_secs(5)).unwrap();
                }
            }
            self.db.execute(request)
        }
    }

    #[tokio::test]
    async fn link_proofs_and_path_responses_during_sweep() {
        use super::super::tests::{insert_announce_for, make_lrproof_packet};
        for cached in [true, false] {
            let dir = temp();
            let (mut actor, tx) = TransportActor::new();
            actor.initialize_sqlite_storage(dir.clone()).await.unwrap();
            actor.is_transport_enabled = true;
            let (entry, mut output) = make_test_interface("initiator");
            actor.interfaces.insert(1, entry);
            let (mut entry, _) = make_test_interface("destination");
            entry.ingress = crate::ingress::IngressController::disabled();
            actor.interfaces.insert(2, entry);
            let identity = rns_identity::identity::Identity::new();
            let destination = [0xcc; 16];
            insert_announce_for(&mut actor, destination, &identity);
            let announce = actor.recent_announces[&destination].clone();
            if !cached {
                actor.recent_announces.clear();
            }
            for link in [[0x77; 16], [0x78; 16]] {
                actor.link_table.insert(
                    link,
                    crate::link_table::LinkEntry {
                        timestamp: crate::now_f64(),
                        next_hop: None,
                        interface_id: 2,
                        remaining_hops: 1,
                        destination_hash: destination,
                        established: false,
                        validated: false,
                        proof_timeout: crate::now_f64() + 120.0,
                        receiving_interface: 1,
                        taken_hops: 0,
                    },
                );
            }
            actor
                .sqlite
                .as_ref()
                .unwrap()
                .worker
                .try_shutdown()
                .unwrap()
                .wait()
                .await
                .unwrap();
            let (started, ready) = tokio::sync::oneshot::channel();
            let (release, wait) = std::sync::mpsc::channel();
            let operations = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let observed = operations.clone();
            let worker = StorageHandle::start(8, move || {
                use storage::TransportStorage;
                let mut db = storage::MemoryTransportStorage::default();
                db.execute(Request::Apply(vec![Mutation::PutAnnounce {
                    announce,
                    raw: None,
                }]))?;
                Ok(PausedSweep {
                    db,
                    started: Some(started),
                    release: wait,
                    operations: observed,
                })
            })
            .await
            .unwrap();
            actor.sqlite.as_mut().unwrap().worker = worker.clone();
            let task = tokio::spawn(actor.run_sqlite());
            tokio::time::timeout(Duration::from_secs(2), ready)
                .await
                .unwrap()
                .unwrap();
            let packet = |raw| {
                TransportMessage::Inbound(crate::messages::InboundPacket {
                    raw,
                    interface_id: 2,
                    rssi: None,
                    snr: None,
                    q: None,
                })
            };
            let mut invalid = make_lrproof_packet([0x78; 16], 0, &identity, None).to_vec();
            *invalid.last_mut().unwrap() ^= 1;
            tx.send(packet(Bytes::from(invalid))).await.unwrap();
            tx.send(packet(make_lrproof_packet([0x77; 16], 0, &identity, None)))
                .await
                .unwrap();
            let before_release = crate::now_f64();
            // An actual signed path-response announce must also pass admission.
            let (raw, dest) = make_valid_announce("lxmf.delivery", 1);
            let (mut header, offset) = rns_wire::header::PacketHeader::unpack(&raw).unwrap();
            header.context = rns_wire::context::PacketContext::PathResponse;
            let mut response = header.pack().unwrap();
            response.extend_from_slice(&raw[offset..]);
            tx.send(packet(Bytes::from(response))).await.unwrap();
            if cached {
                // Both signature checks run while the storage operation remains paused.
                let raw = tokio::time::timeout(Duration::from_millis(500), output.recv())
                    .await
                    .unwrap()
                    .unwrap();
                let (header, _) = rns_wire::header::PacketHeader::unpack(&raw).unwrap();
                assert_eq!(header.destination_hash, [0x77; 16]);
                assert!(output.try_recv().is_err());
                // The path response also starts its lookup before sweep completion.
                tokio::time::timeout(Duration::from_millis(500), async {
                    while worker.available_slots() > 6 {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                release.send(()).unwrap();
            } else {
                // Ensure the miss is submitted while the sweep is still paused.
                // Before the fix, admission waits for the entire sweep here.
                tokio::time::timeout(Duration::from_millis(500), async {
                    while worker.available_slots() > 6 {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                release.send(()).unwrap();
                let raw = tokio::time::timeout(Duration::from_secs(2), output.recv())
                    .await
                    .unwrap()
                    .unwrap();
                let (header, _) = rns_wire::header::PacketHeader::unpack(&raw).unwrap();
                assert_eq!(header.destination_hash, [0x77; 16]);
                assert!(output.try_recv().is_err());
            }
            assert!(matches!(
                query(
                    &tx,
                    Q::Recall {
                        destination_hash: dest
                    }
                )
                .await,
                R::Announce(Some(_))
            ));
            tx.send(TransportMessage::Shutdown).await.unwrap();
            let actor = task.await.unwrap();
            assert!(actor.sqlite.as_ref().unwrap().error.is_none());
            assert!(
                actor.sqlite.as_ref().unwrap().last_sweep >= before_release,
                "cooldown must start at completion, not before the stalled sweep"
            );
            assert!(actor.link_table.get(&[0x77; 16]).unwrap().validated);
            assert!(
                !actor
                    .link_table
                    .get(&[0x78; 16])
                    .is_some_and(|e| e.validated)
            );
            {
                let operations = operations.lock().unwrap();
                assert!(
                    operations.iter().position(|op| *op == "announce").unwrap()
                        < operations
                            .iter()
                            .position(|op| *op == "finish_sweep")
                            .unwrap()
                );
            }
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn discovery_and_proof_admission_survive_announce_queue_overflow() {
        let dir = temp();
        let (mut actor, _tx) = TransportActor::new();
        actor.initialize_sqlite_storage(dir.clone()).await.unwrap();
        let (raw, _) = make_valid_announce("lxmf.delivery", 1);
        for _ in 0..QUEUE_ENTRIES {
            actor.enqueue_sqlite(inbound(raw.clone()));
        }
        let (mut header, offset) = rns_wire::header::PacketHeader::unpack(&raw).unwrap();
        header.context = rns_wire::context::PacketContext::PathResponse;
        let mut response = header.pack().unwrap();
        response.extend_from_slice(&raw[offset..]);
        actor.enqueue_sqlite(inbound(Bytes::from(response)));
        actor.link_table.insert(
            [7; 16],
            crate::link_table::LinkEntry {
                timestamp: crate::now_f64(),
                next_hop: None,
                interface_id: 1,
                remaining_hops: 1,
                destination_hash: [8; 16],
                established: false,
                validated: false,
                proof_timeout: crate::now_f64() + 120.0,
                receiving_interface: 2,
                taken_hops: 0,
            },
        );
        let identity = rns_identity::identity::Identity::new();
        actor.enqueue_sqlite(inbound(super::super::tests::make_lrproof_packet(
            [7; 16], 0, &identity, None,
        )));
        let state = actor.sqlite.as_ref().unwrap();
        assert_eq!(state.queue.len(), QUEUE_ENTRIES);
        assert!(state.bytes <= QUEUE_BYTES);
        assert_eq!(state.queue[0].priority, 2);
        assert_eq!(state.queue[1].priority, 1);
        assert_eq!(actor.channel_drops, 2);
        state.worker.try_shutdown().unwrap().wait().await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn four_peer_announce_and_link_load_during_large_sqlite_sweep() {
        use super::super::tests::{insert_announce_for, make_lrproof_packet};
        let (dir, store, hashes) = tokio::task::spawn_blocking(|| storage::populated_store(22_000))
            .await
            .unwrap();
        drop(store);
        let (mut actor, tx) = TransportActor::new();
        actor.initialize_sqlite_storage(dir.clone()).await.unwrap();
        actor.is_transport_enabled = true;
        let mut outbound_receivers = Vec::new();
        let mut peers = Vec::new();
        for peer in 0..4u8 {
            let interface = u64::from(peer) + 1;
            let origin = interface + 100;
            let (mut entry, rx) = make_test_interface("peer");
            entry.ingress = crate::ingress::IngressController::disabled();
            actor.interfaces.insert(interface, entry);
            outbound_receivers.push(rx);
            let (entry, output) = make_test_interface("initiator");
            actor.interfaces.insert(origin, entry);
            let identity = rns_identity::identity::Identity::new();
            let destination = [0xe0 + peer; 16];
            let link = [0xa0 + peer; 16];
            insert_announce_for(&mut actor, destination, &identity);
            let mut announce = actor.recent_announces.remove(&destination).unwrap();
            announce.retained = true;
            call(
                &actor.sqlite.as_ref().unwrap().worker,
                Request::Apply(vec![Mutation::PutAnnounce {
                    announce,
                    raw: None,
                }]),
            )
            .await
            .unwrap();
            actor.link_table.insert(
                link,
                crate::link_table::LinkEntry {
                    timestamp: crate::now_f64(),
                    next_hop: None,
                    interface_id: interface,
                    remaining_hops: 1,
                    destination_hash: destination,
                    established: false,
                    validated: false,
                    proof_timeout: crate::now_f64() + 30.0,
                    receiving_interface: origin,
                    taken_hops: 0,
                },
            );
            peers.push((interface, identity, link, output));
        }
        for hash in hashes {
            let mut path = crate::path_table::PathEntry::new(None, 1, 1, InterfaceMode::Full);
            path.packet_hash = Some(hash);
            actor.path_table.insert(
                rns_wire::types::DestHash::new(hash[..16].try_into().unwrap()),
                path,
            );
        }
        let started = std::time::Instant::now();
        let task = tokio::spawn(actor.run_sqlite());
        let mut producers = Vec::new();
        for (interface, identity, link, mut output) in peers {
            let tx = tx.clone();
            producers.push(tokio::spawn(async move {
                let packet = |raw| {
                    TransportMessage::Inbound(crate::messages::InboundPacket {
                        raw,
                        interface_id: interface,
                        rssi: None,
                        snr: None,
                        q: None,
                    })
                };
                let mut max_recall = Duration::ZERO;
                let mut proof_latency = Duration::ZERO;
                for round in 0..32 {
                    let (mut raw, destination_hash) = make_valid_announce("lxmf.delivery", 1);
                    if round % 8 == 0 {
                        let (mut header, offset) =
                            rns_wire::header::PacketHeader::unpack(&raw).unwrap();
                        header.context = rns_wire::context::PacketContext::PathResponse;
                        let mut response = header.pack().unwrap();
                        response.extend_from_slice(&raw[offset..]);
                        raw = Bytes::from(response);
                    }
                    let sent = std::time::Instant::now();
                    tx.send(packet(raw)).await.unwrap();
                    assert!(matches!(
                        query(&tx, Q::Recall { destination_hash }).await,
                        R::Announce(Some(_))
                    ));
                    max_recall = max_recall.max(sent.elapsed());
                    if round == 2 {
                        let sent = std::time::Instant::now();
                        tx.send(packet(make_lrproof_packet(link, 0, &identity, None)))
                            .await
                            .unwrap();
                        tokio::time::timeout(Duration::from_secs(5), async {
                            loop {
                                let raw = output.recv().await.expect("interface remains open");
                                let (header, _) =
                                    rns_wire::header::PacketHeader::unpack(&raw).unwrap();
                                if header.destination_hash == link
                                    && header.context == rns_wire::context::PacketContext::Lrproof
                                {
                                    break;
                                }
                            }
                        })
                        .await
                        .expect("proof must make progress during sweep");
                        proof_latency = sent.elapsed();
                    }
                }
                (max_recall, proof_latency)
            }));
        }
        for producer in producers {
            let (max_recall, proof_latency) = producer.await.unwrap();
            eprintln!(
                "peer: max announce/recall={}ms, cold proof={}ms",
                max_recall.as_millis(),
                proof_latency.as_millis()
            );
        }
        tx.send(TransportMessage::Shutdown).await.unwrap();
        let actor = task.await.unwrap();
        assert!(actor.sqlite.as_ref().unwrap().error.is_none());
        assert_eq!(actor.channel_drops, 0);
        assert!(actor.recent_announces.len() <= CACHE_ENTRIES);
        eprintln!(
            "four peers, 128 announces, four cold proofs, 22000 pins: {}ms",
            started.elapsed().as_millis()
        );
        drop(outbound_receivers);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn obsolete_backlog_yields_to_four_peers_and_finishes_incrementally() {
        exercise_obsolete_backlog(false).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_does_not_wait_for_obsolete_backlog() {
        exercise_obsolete_backlog(true).await;
    }

    async fn exercise_obsolete_backlog(stop_early: bool) {
        struct SlowGc {
            db: storage::SqliteTransportStorage,
            started: Option<tokio::sync::oneshot::Sender<()>>,
            release: std::sync::mpsc::Receiver<()>,
            finished: Option<tokio::sync::oneshot::Sender<()>>,
            applies: usize,
            pages: std::sync::Arc<
                std::sync::Mutex<Vec<(std::time::Instant, std::time::Instant, usize)>>,
            >,
        }
        impl storage::TransportStorage for SlowGc {
            fn execute(&mut self, request: Request) -> storage::Result<Reply> {
                let limit = match &request {
                    Request::CleanKnownPage { limit, .. }
                    | Request::CollectPacketsPage { limit, .. } => Some(*limit),
                    _ => None,
                };
                let started = std::time::Instant::now();
                if let Some(limit) = limit {
                    assert!(limit <= GC_PAGE_ENTRIES);
                    if let Some(ready) = self.started.take() {
                        let _ = ready.send(());
                        self.release.recv_timeout(Duration::from_secs(5)).unwrap();
                    }
                    // Model a slow device independently of the host SSD.
                    std::thread::sleep(Duration::from_millis(limit as u64 * 4));
                }
                if let Request::Apply(mutations) = &request {
                    self.applies += mutations.len();
                }
                let result = self.db.execute(request);
                if limit.is_some() {
                    self.pages.lock().unwrap().push((
                        started,
                        std::time::Instant::now(),
                        self.applies,
                    ));
                }
                if matches!(&result, Ok(Reply::CollectedPage { next: None, .. })) {
                    if let Some(done) = self.finished.take() {
                        let _ = done.send(());
                    }
                }
                result
            }
        }
        let (dir, db) = tokio::task::spawn_blocking(|| storage::obsolete_store(96))
            .await
            .unwrap();
        drop(db);
        let (mut actor, tx) = TransportActor::new();
        actor.initialize_sqlite_storage(dir.clone()).await.unwrap();
        actor
            .sqlite
            .as_ref()
            .unwrap()
            .worker
            .try_shutdown()
            .unwrap()
            .wait()
            .await
            .unwrap();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let (finished, done) = tokio::sync::oneshot::channel();
        let pages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = pages.clone();
        let database_path = dir.join("transport.sqlite");
        let worker = StorageHandle::start(8, move || {
            Ok(SlowGc {
                db: storage::SqliteTransportStorage::open(
                    &database_path,
                    storage::StorageRole::Standalone,
                    Default::default(),
                )?,
                started: Some(started),
                release: wait,
                finished: Some(finished),
                applies: 0,
                pages: observed,
            })
        })
        .await
        .unwrap();
        actor.sqlite.as_mut().unwrap().worker = worker.clone();
        let mut interface_receivers = Vec::new();
        let mut streams = Vec::new();
        for interface in 1..=4 {
            let (mut entry, rx) = make_test_interface("peer");
            entry.ingress = crate::ingress::IngressController::disabled();
            actor.interfaces.insert(interface, entry);
            interface_receivers.push(rx);
            streams.push((
                interface,
                (0..8)
                    .map(|_| make_valid_announce("lxmf.delivery", 1))
                    .collect::<Vec<_>>(),
            ));
        }
        let task = tokio::spawn(actor.run_sqlite());
        tokio::time::timeout(Duration::from_secs(5), ready)
            .await
            .unwrap()
            .unwrap();
        let mut producers = Vec::new();
        for (interface, packets) in streams {
            let tx = tx.clone();
            producers.push(tokio::spawn(async move {
                let destinations: Vec<_> = packets.iter().map(|(_, dest)| *dest).collect();
                for (raw, _) in packets {
                    tx.send(TransportMessage::Inbound(crate::messages::InboundPacket {
                        raw,
                        interface_id: interface,
                        rssi: None,
                        snr: None,
                        q: None,
                    }))
                    .await
                    .unwrap();
                }
                for destination_hash in destinations {
                    assert!(matches!(
                        query(&tx, Q::Recall { destination_hash }).await,
                        R::Announce(Some(_))
                    ));
                }
                std::time::Instant::now()
            }));
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while worker.available_slots() > 6 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let released = std::time::Instant::now();
        release.send(()).unwrap();
        let mut last_peer = released;
        for producer in producers {
            last_peer = last_peer.max(producer.await.unwrap());
        }
        if !stop_early {
            tokio::time::timeout(Duration::from_secs(15), done)
                .await
                .unwrap()
                .unwrap();
        }
        tx.send(TransportMessage::Shutdown).await.unwrap();
        let actor = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("shutdown must not drain the remaining GC backlog")
            .unwrap();
        assert_eq!(actor.channel_drops, 0);
        let state = actor.sqlite.as_ref().unwrap();
        assert!(state.error.is_none());
        assert!(
            actor.sqlite_snapshot_ready(),
            "paused GC must not block final route persistence"
        );
        if stop_early {
            assert!(!matches!(state.gc_phase, GcPhase::Idle));
            assert!(state.gc_announces < 96);
            std::fs::remove_dir_all(dir).unwrap();
            return;
        }
        assert!(matches!(state.gc_phase, GcPhase::Idle));
        assert_eq!(state.gc_announces, 96);
        assert_eq!(state.gc_packets, 96);
        let pages = pages.lock().unwrap();
        assert!(pages.len() > 2);
        // The entire accepted burst is served before scheduling another page.
        assert!(
            pages[1].2 >= 32,
            "all accepted announce writes precede the next GC page"
        );
        for pair in pages.windows(2) {
            let work = pair[0].1.duration_since(pair[0].0);
            let rest = pair[1].0.duration_since(pair[0].1);
            assert!(
                rest >= work.saturating_mul(GC_REST_FACTOR),
                "GC did not leave its worker budget to foreground traffic"
            );
        }
        eprintln!(
            "four peers / 32 announces: {}ms after blocked page released; {} GC pages, all 96 obsolete records and packets removed",
            last_peer.duration_since(released).as_millis(),
            pages.len()
        );
        drop(pages);
        drop(interface_receivers);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn maintenance_options_match_lxmf_bounds() {
        let dir = temp();
        std::fs::create_dir_all(&dir).unwrap();
        let (mut actor, _tx) = TransportActor::new();
        actor
            .initialize_sqlite_storage_with_options(
                dir.clone(),
                storage::SqliteOptions {
                    vacuum_interval: Duration::from_secs(1),
                    vacuum_pages: 7,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let state = actor.sqlite.as_ref().unwrap();
        assert_eq!(state.vacuum_interval, 60.0);
        assert_eq!(state.vacuum_pages, 7);
        state.worker.try_shutdown().unwrap().wait().await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn admitted_ifac_announce_drains_through_sqlite_after_inputs_close() {
        let (mut actor, input, control) = TransportActor::new_with_control_channel();
        actor.initialize_sqlite_storage(temp()).await.unwrap();
        super::super::tests::exercise_admitted_drain(actor, input, control).await;
    }

    #[tokio::test]
    async fn all_classes_drain_and_deliver_through_sqlite_after_inputs_close() {
        let dir = temp();
        let (mut actor, input, control) = TransportActor::new_with_control_channel();
        actor.initialize_sqlite_storage(dir.clone()).await.unwrap();
        super::super::tests::exercise_all_class_drain(actor, input, control).await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn mixed_class_load_preserves_sqlite_control_and_shutdown() {
        let dir = temp();
        let (mut actor, input, control) = TransportActor::new_with_control_channel();
        actor.initialize_sqlite_storage(dir.clone()).await.unwrap();
        super::super::tests::exercise_mixed_class_load(actor, input, control).await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn separate_control_channel_survives_sqlite_inbound_flood() {
        let dir = temp();
        let (mut actor, interface_tx, control_tx) = TransportActor::new_with_control_channel();
        actor.initialize_sqlite_storage(dir).await.unwrap();
        super::super::tests::exercise_control_during_flood(actor, interface_tx, control_tx).await;
    }

    #[tokio::test]
    async fn idle_actor_schedules_configured_vacuum() {
        struct ObservedMaintenance {
            db: storage::MemoryTransportStorage,
            observed: Option<tokio::sync::oneshot::Sender<u32>>,
        }
        impl storage::TransportStorage for ObservedMaintenance {
            fn execute(&mut self, request: Request) -> storage::Result<Reply> {
                if let Request::Maintain { vacuum_pages } = &request
                    && let Some(observed) = self.observed.take()
                {
                    let _ = observed.send(*vacuum_pages);
                }
                self.db.execute(request)
            }
        }

        let dir = temp();
        let (mut actor, tx) = TransportActor::new();
        actor.initialize_sqlite_storage(dir.clone()).await.unwrap();
        actor
            .sqlite
            .as_ref()
            .unwrap()
            .worker
            .try_shutdown()
            .unwrap()
            .wait()
            .await
            .unwrap();
        let (observed_tx, observed_rx) = tokio::sync::oneshot::channel();
        let state = actor.sqlite.as_mut().unwrap();
        state.worker = StorageHandle::start(8, move || {
            Ok(ObservedMaintenance {
                db: Default::default(),
                observed: Some(observed_tx),
            })
        })
        .await
        .unwrap();
        let now = crate::now_f64();
        state.last_sweep = now;
        state.last_vacuum = now - 61.0;
        state.vacuum_interval = 60.0;
        state.vacuum_pages = 23;

        let task = tokio::spawn(actor.run_sqlite());
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), observed_rx)
                .await
                .unwrap()
                .unwrap(),
            23
        );
        tx.send(TransportMessage::Shutdown).await.unwrap();
        task.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn bounded_cache_recall_restart_and_no_legacy_import() {
        let dir = temp();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("announce_cache.msgpack"),
            b"old root must not be read",
        )
        .unwrap();
        let namespace = dir.join("sqlite");
        let (mut actor, tx) = TransportActor::new();
        actor
            .initialize_sqlite_storage(namespace.clone())
            .await
            .unwrap();
        let (mut iface, _rx) = make_test_interface("test");
        iface.ingress = crate::ingress::IngressController::disabled();
        actor.handle_message(TransportMessage::RegisterInterface {
            id: 1,
            entry: iface,
        });
        let task = tokio::spawn(actor.run_sqlite());
        let mut first = None;
        for _ in 0..300 {
            let (raw, dest) = make_valid_announce("lxmf.delivery", 1);
            first.get_or_insert(dest);
            tx.send(inbound(raw)).await.unwrap();
            assert!(matches!(
                query(
                    &tx,
                    Q::Recall {
                        destination_hash: dest
                    }
                )
                .await,
                R::Announce(Some(_))
            ));
        }
        tx.send(TransportMessage::Shutdown).await.unwrap();
        let actor = task.await.unwrap();
        assert!(actor.sqlite.as_ref().unwrap().error.is_none());
        assert!(actor.recent_announces.len() <= CACHE_ENTRIES);
        assert!(actor.memory_stats().announce_app_data_bytes <= CACHE_BYTES);
        assert!(!namespace.join("announce_cache.msgpack").exists());
        assert!(!namespace.join("cache/announces").exists());
        assert_eq!(
            std::fs::read(dir.join("announce_cache.msgpack")).unwrap(),
            b"old root must not be read"
        );
        let (mut restored, tx) = TransportActor::new();
        restored.initialize_sqlite_storage(namespace).await.unwrap();
        assert!(
            restored.recent_announces.is_empty(),
            "restart must not restore all metadata"
        );
        let task = tokio::spawn(restored.run_sqlite());
        assert!(matches!(
            query(
                &tx,
                Q::Recall {
                    destination_hash: first.unwrap()
                }
            )
            .await,
            R::Announce(Some(_))
        ));
        let R::Announces(all) = query(&tx, Q::GetRecentAnnounces).await else {
            panic!()
        };
        assert_eq!(all.len(), 300, "full RPC sees database, not just RAM cache");
        let mut after = None;
        let mut paged = Vec::new();
        loop {
            let R::Announces(entries) = query(&tx, Q::GetAnnouncesPage { after, limit: 17 }).await
            else {
                panic!()
            };
            if entries.is_empty() {
                break;
            }
            assert!(entries.len() <= 17);
            for entry in entries {
                assert!(after.is_none_or(|key| entry.dest_hash > key));
                after = Some(entry.dest_hash);
                paged.push(entry.dest_hash);
            }
        }
        assert_eq!(paged.len(), 300);
        assert!(matches!(
            query(
                &tx,
                Q::GetAnnouncesPage {
                    after: None,
                    limit: 129
                }
            )
            .await,
            R::Error(_)
        ));
        assert!(matches!(
            query(
                &tx,
                Q::Recall {
                    destination_hash: first.unwrap()
                }
            )
            .await,
            R::Announce(Some(_))
        ));
        tx.send(TransportMessage::Shutdown).await.unwrap();
        task.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn slow_storage_does_not_block_other_actor_queries() {
        struct Slow {
            db: storage::MemoryTransportStorage,
            started: Option<tokio::sync::oneshot::Sender<()>>,
            release: std::sync::mpsc::Receiver<()>,
        }
        impl storage::TransportStorage for Slow {
            fn execute(&mut self, request: Request) -> storage::Result<Reply> {
                if matches!(request, Request::Announce(_)) {
                    if let Some(started) = self.started.take() {
                        let _ = started.send(());
                        self.release.recv().unwrap();
                    }
                }
                self.db.execute(request)
            }
        }
        let dir = temp();
        let (mut actor, tx) = TransportActor::new();
        actor.initialize_sqlite_storage(dir.clone()).await.unwrap();
        actor
            .sqlite
            .as_ref()
            .unwrap()
            .worker
            .try_shutdown()
            .unwrap()
            .wait()
            .await
            .unwrap();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        actor.sqlite.as_mut().unwrap().worker = StorageHandle::start(8, move || {
            Ok(Slow {
                db: Default::default(),
                started: Some(started),
                release: wait,
            })
        })
        .await
        .unwrap();
        let task = tokio::spawn(actor.run_sqlite());
        let (response_tx, answer) = tokio::sync::oneshot::channel();
        tx.send(TransportMessage::Rpc {
            query: Q::Recall {
                destination_hash: [1; 16],
            },
            response_tx,
        })
        .await
        .unwrap();
        ready.await.unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_millis(200), query(&tx, Q::GetLinkCount))
                .await
                .unwrap(),
            R::IntResult(0)
        ));
        release.send(()).unwrap();
        assert!(matches!(answer.await.unwrap(), R::Announce(None)));
        tx.send(TransportMessage::Shutdown).await.unwrap();
        task.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
