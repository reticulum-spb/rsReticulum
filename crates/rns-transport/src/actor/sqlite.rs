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

pub(super) struct SqliteState {
    worker: StorageHandle,
    pub raw: HashMap<[u8; 32], Vec<u8>>,
    queue: VecDeque<TransportMessage>,
    bytes: usize,
    writes: Vec<Mutation>,
    pub busy: bool,
    order: VecDeque<[u8; 16]>,
    error: Option<String>,
    last_sweep: f64,
    last_vacuum: f64,
    vacuum_interval: f64,
    vacuum_pages: u32,
}

#[derive(Default)]
struct Reads {
    keys: Vec<[u8; 16]>,
    identities: Vec<[u8; 16]>,
    packets: Vec<[u8; 32]>,
}
#[derive(Debug)]
struct Prepared {
    message: Option<TransportMessage>,
    entries: Vec<RecentAnnounce>,
    raw: HashMap<[u8; 32], Vec<u8>>,
    clear: bool,
}

fn weight(msg: &TransportMessage) -> usize {
    match msg {
        TransportMessage::Inbound(p) => p.raw.len() + 256,
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
    // Actor currently stages at most one accepted announce. RPC bulk mutations
    // are applied page-by-page below; no unbounded write-behind queue exists.
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
        let worker =
            StorageHandle::open_sqlite(database_path, storage::StorageRole::Standalone, options)
                .await?;
        self.sqlite = Some(SqliteState {
            worker,
            raw: HashMap::new(),
            queue: VecDeque::new(),
            bytes: 0,
            writes: Vec::new(),
            busy: false,
            order: VecDeque::new(),
            error: None,
            last_sweep: 0.0,
            last_vacuum: crate::now_f64(),
            vacuum_interval,
            vacuum_pages,
        });
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
        state.writes.push(Mutation::PutAnnounce {
            announce: a.clone(),
            raw: Some(raw.to_vec()),
        });
        state.order.retain(|d| *d != dest);
        state.order.push_back(dest);
    }

    pub(super) fn enqueue_sqlite(&mut self, msg: TransportMessage) {
        let state = self.sqlite.as_mut().unwrap();
        if let Some(error) = &state.error {
            reject(msg, error);
            return;
        }
        let bytes = weight(&msg);
        if state.queue.len() >= QUEUE_ENTRIES || state.bytes + bytes > QUEUE_BYTES {
            self.channel_drops += 1;
            warn!(drops = self.channel_drops, "SQLite admission queue full");
            reject(msg, "admission queue full");
            return;
        }
        state.bytes += bytes;
        state.queue.push_back(msg);
    }

    fn sqlite_header(
        &self,
        p: &crate::messages::InboundPacket,
    ) -> Option<(rns_wire::header::PacketHeader, Vec<u8>)> {
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
            TransportMessage::Inbound(p) => self.sqlite_header(p).is_some_and(|(h, _)| {
                h.flags.packet_type == rns_wire::flags::PacketType::Announce
                    || h.flags.destination_type == rns_wire::flags::DestinationType::Plain
                    || h.context == rns_wire::context::PacketContext::CacheRequest
                    || h.context == rns_wire::context::PacketContext::LinkProof
            }),
            _ => false,
        }
    }

    fn sqlite_reads(&self, msg: &TransportMessage) -> Reads {
        let mut reads = Reads::default();
        match msg {
            TransportMessage::Inbound(p) => {
                if let Some((h, payload)) = self.sqlite_header(p) {
                    if h.flags.packet_type == rns_wire::flags::PacketType::Announce {
                        reads.keys.push(h.destination_hash);
                    }
                    if h.context == rns_wire::context::PacketContext::LinkProof {
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
        reads
            .keys
            .retain(|k| !self.recent_announces.contains_key(k));
        reads
    }

    fn trim_sqlite_cache(&mut self) {
        let state = self.sqlite.as_mut().unwrap();
        let mut bytes: usize = self
            .recent_announces
            .values()
            .map(|a| 256 + a.app_data.as_ref().map_or(0, Vec::capacity))
            .sum();
        while self.recent_announces.len() > CACHE_ENTRIES || bytes > CACHE_BYTES {
            let key = state
                .order
                .pop_front()
                .or_else(|| self.recent_announces.keys().next().copied());
            let Some(key) = key else { break };
            if let Some(a) = self.recent_announces.remove(&key) {
                bytes = bytes.saturating_sub(256 + a.app_data.as_ref().map_or(0, Vec::capacity));
            }
        }
        state.raw.clear();
    }

    pub(super) async fn run_sqlite(mut self) -> Self {
        let mut tick = tokio::time::interval(Duration::from_millis(JOB_INTERVAL_MS));
        let mut job: Option<JoinHandle<storage::Result<Prepared>>> = None;
        let mut stopping = false;
        let mut was_foreground = true;
        loop {
            if job.is_none() {
                let state = self.sqlite.as_mut().unwrap();
                let message = state.queue.pop_front();
                if let Some(msg) = &message {
                    state.bytes = state.bytes.saturating_sub(weight(msg));
                }
                let writes = std::mem::take(&mut state.writes);
                let worker = state.worker.clone();
                if let Some(message) = message {
                    let reads = self.sqlite_reads(&message);
                    job = Some(tokio::spawn(prepare(worker, writes, Some(message), reads)));
                } else if !writes.is_empty() {
                    job = Some(tokio::spawn(prepare(
                        worker,
                        writes,
                        None,
                        Reads::default(),
                    )));
                } else if stopping {
                    break;
                } else if crate::now_f64() - state.last_sweep >= 60.0
                    && state.error.is_none()
                    && !self
                        .routing_save_in_flight
                        .load(std::sync::atomic::Ordering::Acquire)
                {
                    state.last_sweep = crate::now_f64();
                    let keep = self.sqlite_live_packets();
                    let directory = self.storage_dir.clone().unwrap();
                    job = Some(tokio::spawn(async move {
                        sweep(&worker, directory, keep).await?;
                        Ok(Prepared {
                            message: None,
                            entries: Vec::new(),
                            raw: HashMap::new(),
                            clear: true,
                        })
                    }));
                } else if crate::now_f64() - state.last_vacuum >= state.vacuum_interval
                    && state.error.is_none()
                    && !self
                        .routing_save_in_flight
                        .load(std::sync::atomic::Ordering::Acquire)
                {
                    state.last_vacuum = crate::now_f64();
                    let vacuum_pages = state.vacuum_pages;
                    job = Some(tokio::spawn(async move {
                        maintain(&worker, vacuum_pages).await;
                        Ok(Prepared {
                            message: None,
                            entries: Vec::new(),
                            raw: HashMap::new(),
                            clear: false,
                        })
                    }));
                }
            }
            self.sqlite.as_mut().unwrap().busy = job.is_some();
            tokio::select! {
                result=async {job.as_mut().unwrap().await}, if job.is_some()=> {
                    job=None;
                    self.sqlite.as_mut().unwrap().busy=false;
                    match result {
                        Ok(Ok(prepared))=> {
                            if prepared.clear {self.recent_announces.clear();self.sqlite.as_mut().unwrap().order.clear();}
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
                            for msg in state.queue.drain(..) {reject(msg,&error);}
                            state.bytes=0;
                        }
                    }
                }
                msg=self.rx.recv(), if !stopping=>match msg {
                    None|Some(TransportMessage::Shutdown)=>{stopping=true;},
                    Some(TransportMessage::SetStoragePaths {..})=>warn!("cannot change active SQLite storage ownership"),
                    Some(msg)=> {
                        if self.sqlite_dependent(&msg) {self.enqueue_sqlite(msg);} else {self.handle_message(msg);}
                    }
                },
                _=tick.tick(), if !stopping=> {
                    let fg=self.is_foreground.load(std::sync::atomic::Ordering::Relaxed);
                    if fg!=was_foreground { if fg {self.on_resume();} else {self.save_state_async();} was_foreground=fg; }
                    self.on_tick();
                }
            }
        }
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

    fn sqlite_live_packets(&self) -> HashSet<[u8; 32]> {
        let mut keep: HashSet<_> = self
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
                keep.insert(h);
            }
        }
        for t in &self.pending_tunnel_entries {
            for p in &t.paths {
                if let Some(h) = p
                    .packet_hash
                    .as_ref()
                    .and_then(|h| h.as_slice().try_into().ok())
                {
                    keep.insert(h);
                }
            }
        }
        keep
    }
}

async fn maintain(worker: &StorageHandle, vacuum_pages: u32) {
    let started = std::time::Instant::now();
    match call(worker, Request::Maintain { vacuum_pages }).await {
        Ok(Reply::Maintenance(stats)) => tracing::info!(
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
    mut message: Option<TransportMessage>,
    reads: Reads,
) -> storage::Result<Prepared> {
    write(&worker, writes).await?;
    let mut result = Prepared {
        message: None,
        entries: Vec::new(),
        raw: HashMap::new(),
        clear: false,
    };
    // These RPCs are answered directly from the source of truth. Only the
    // existing explicit full-list API materializes the complete response.
    if let Some(TransportMessage::Rpc { query, .. }) = &message {
        if matches!(
            query,
            Q::GetRecentAnnounces
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
            result.clear = true;
            return Ok(result);
        }
    }
    for key in reads.keys {
        if let Reply::Announce(Some(a)) = call(&worker, Request::Announce(key)).await? {
            result.entries.push(a);
        }
    }
    for identity in reads.identities {
        let mut cursor = None;
        loop {
            let p = page(
                &worker,
                AnnouncePageQuery {
                    identity_hash: Some(identity),
                    after: cursor,
                    ..Default::default()
                },
            )
            .await?;
            if p.entries.is_empty() {
                break;
            }
            cursor = p.next;
            result.entries.extend(p.entries);
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
    call(
        worker,
        Request::CleanKnown {
            unused_before: now - UNUSED_DESTINATION_LINGER,
            used_before: now - DESTINATION_TIMEOUT as f64 * 1.25,
            limit: 128,
        },
    )
    .await?;
    Ok(())
}

async fn sweep(
    worker: &StorageHandle,
    directory: PathBuf,
    mut keep: HashSet<[u8; 32]>,
) -> storage::Result<()> {
    // The old *and* new durable snapshot must protect packets. Reading files
    // also protects paths if a prior snapshot write failed. No legacy root is
    // inspected; these files belong solely to the SQLite namespace.
    keep = tokio::task::spawn_blocking(move || -> storage::Result<_> {
        let p = directory.join("path_table.msgpack");
        if p.exists() {
            for e in crate::persistence::load_path_table(&p).map_err(|_| {
                storage::StorageError::Invalid("path snapshot unreadable; refusing GC")
            })? {
                if let Some(h) = e.packet_hash.and_then(|h| h.try_into().ok()) {
                    keep.insert(h);
                }
            }
        }
        let p = directory.join("tunnel_table.msgpack");
        if p.exists() {
            for t in crate::persistence::load_tunnel_table(&p).map_err(|_| {
                storage::StorageError::Invalid("tunnel snapshot unreadable; refusing GC")
            })? {
                for e in t.paths {
                    if let Some(h) = e.packet_hash.and_then(|h| h.try_into().ok()) {
                        keep.insert(h);
                    }
                }
            }
        }
        Ok(keep)
    })
    .await
    .map_err(|_| storage::StorageError::Closed)??;
    let Reply::Generation(generation) = call(worker, Request::BeginSweep).await? else {
        unreachable!()
    };
    let mut chunk = Vec::new();
    for hash in keep {
        chunk.push(hash);
        if chunk.len() == 128 {
            call(
                worker,
                Request::KeepPackets {
                    generation,
                    hashes: std::mem::take(&mut chunk),
                },
            )
            .await?;
        }
    }
    if !chunk.is_empty() {
        call(
            worker,
            Request::KeepPackets {
                generation,
                hashes: chunk,
            },
        )
        .await?;
    }
    call(worker, Request::FinishSweep { generation }).await?;
    clean(worker).await?;
    call(worker, Request::CollectPackets { limit: 128 }).await?;
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
