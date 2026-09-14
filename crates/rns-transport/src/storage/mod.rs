//! Incremental announce storage, independent of legacy actor snapshots.
//!
//! The SQLite backend is opt-in and runs through `TransportActor`'s bounded
//! admission loop.
//! All SQL runs on one worker thread. Use [`StorageHandle::try_submit`] and
//! handle completion alongside packet traffic; awaiting every lookup in the
//! actor's receive loop would serialize packet processing on disk latency.
//! A successful submission is not a commit: wait for its result. Dropping a
//! pending result does not cancel an accepted operation.

mod memory;
#[cfg(feature = "sqlite")]
mod sqlite;
mod worker;

pub use memory::MemoryTransportStorage;
#[cfg(feature = "sqlite")]
pub use sqlite::{SqliteOptions, SqliteTransportStorage};
pub use worker::{Pending, Rejected, StorageHandle};

use crate::actor::RecentAnnounce;

pub type DestinationHash = [u8; 16];
pub type PacketHash = [u8; 32];

/// Bounds apply before an operation enters the worker queue. Pages have an
/// additional byte budget so large app_data cannot multiply by the row limit.
pub const MAX_VALUE_BYTES: usize = 64 * 1024;
pub const MAX_BATCH_BYTES: usize = 256 * 1024;
pub const MAX_BATCH_ITEMS: usize = 128;
pub const MAX_PAGE_ITEMS: usize = 128;
pub const MAX_PAGE_BYTES: usize = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("shared clients cannot own transport storage")]
    NotOwner,
    #[error("transport storage already has an owner")]
    AlreadyOwned,
    #[error("storage worker has stopped")]
    Closed,
    #[error("storage worker is at its in-flight limit")]
    Busy,
    #[error("invalid storage operation: {0}")]
    Invalid(&'static str),
    #[error("unsupported transport schema: {0}")]
    Schema(i64),
    #[error("not a transport storage database")]
    ForeignDatabase,
    #[error("packet reference has no payload")]
    MissingPacket,
    #[error("storage IO: {0}")]
    Io(#[from] std::io::Error),
    #[cfg(feature = "sqlite")]
    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, StorageError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageRole {
    SharedOwner,
    Standalone,
    SharedClient,
}

/// Owners are stable across restarts, never runtime InterfaceIds. Cache tokens
/// are caller-assigned and must be explicitly released when retention ends.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PacketOwner {
    Path(DestinationHash),
    Tunnel {
        tunnel: [u8; 32],
        destination: DestinationHash,
    },
    Cache([u8; 32]),
}

impl PacketOwner {
    pub(super) fn destination(&self) -> Option<DestinationHash> {
        match self {
            Self::Path(d) | Self::Tunnel { destination: d, .. } => Some(*d),
            Self::Cache(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Mutation {
    /// Inserts metadata and optionally its raw packet in the same transaction.
    /// Re-announcing preserves existing pins and last-used time. Clearing a
    /// pin is a separate explicit mutation. None raw supports legacy metadata;
    /// a non-None packet_hash must already exist or be inserted earlier in this batch.
    PutAnnounce {
        announce: RecentAnnounce,
        raw: Option<Vec<u8>>,
    },
    /// Used to import an older packet still referenced by a path or tunnel.
    PutPacket {
        hash: PacketHash,
        raw: Vec<u8>,
    },
    SetReference {
        owner: PacketOwner,
        hash: PacketHash,
    },
    RemoveReference(PacketOwner),
    RemoveAnnounce(DestinationHash),
    SetRetained {
        destination: DestinationHash,
        retained: bool,
    },
    Touch {
        destination: DestinationHash,
        at: f64,
    },
}

#[derive(Debug, Clone, Default)]
pub struct AnnouncePageQuery {
    pub identity_hash: Option<DestinationHash>,
    /// Exclusive keyset cursor, ordered by destination hash, not OFFSET.
    pub after: Option<DestinationHash>,
    pub name_hash: Option<[u8; 10]>,
    pub retained: Option<bool>,
    /// Zero selects the default (MAX_PAGE_ITEMS).
    pub limit: usize,
}

impl AnnouncePageQuery {
    pub(super) fn limit(&self) -> usize {
        if self.limit == 0 {
            MAX_PAGE_ITEMS
        } else {
            self.limit
        }
    }
    pub(super) fn matches(&self, a: &RecentAnnounce) -> bool {
        self.identity_hash.is_none_or(|id| {
            a.public_key
                .is_some_and(|pk| rns_crypto::sha::truncated_hash(&pk) == id)
        }) && self.after.is_none_or(|d| a.dest_hash > d)
            && self.name_hash.is_none_or(|n| a.name_hash == n)
            && self.retained.is_none_or(|r| a.retained == r)
    }
}

#[derive(Debug)]
pub struct AnnouncePage {
    pub entries: Vec<RecentAnnounce>,
    /// Resume here. An empty page signals EOF; a final non-empty page can
    /// carry a cursor even if the next page is empty. No long read transaction
    /// is held across requests, so concurrent mutations may change later pages.
    pub next: Option<DestinationHash>,
}

#[derive(Debug, Clone)]
pub enum Request {
    ClearAnnounces,
    /// Stage a complete conservative packet keep-set. Interrupted sweeps never
    /// remove references; FinishSweep is issued only after all chunks land.
    BeginSweep,
    KeepPackets {
        generation: i64,
        hashes: Vec<PacketHash>,
    },
    FinishSweep {
        generation: i64,
    },
    CleanKnown {
        unused_before: f64,
        used_before: f64,
        limit: usize,
    },
    Apply(Vec<Mutation>),
    Announce(DestinationHash),
    Packet(PacketHash),
    Page(AnnouncePageQuery),
    /// Caller decides protocol cutoffs; paths/tunnels protect their destination,
    /// pins protect metadata, and last_used protects recently recalled entries.
    ExpireAnnounces {
        before: f64,
        limit: usize,
    },
    /// Collect only blobs with no metadata, path, tunnel or cache owner.
    CollectPackets {
        limit: usize,
    },
    Stats,
    /// Passive WAL checkpoint followed by a bounded incremental vacuum.
    /// A zero page budget keeps checkpointing and metrics enabled.
    Maintain {
        vacuum_pages: u32,
    },
    Checkpoint,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StorageStats {
    pub announces: u64,
    pub packets: u64,
    pub packet_bytes: u64,
    pub references: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StorageMaintenance {
    pub database_bytes: u64,
    pub wal_bytes: u64,
    pub page_size: u64,
    pub page_count: u64,
    pub free_pages: u64,
    pub checkpointed_frames: u64,
    pub remaining_wal_frames: u64,
    pub vacuumed_pages: u64,
    pub page_cache_kib: u64,
}

#[derive(Debug)]
pub enum Reply {
    Generation(i64),
    Applied,
    Announce(Option<RecentAnnounce>),
    Packet(Option<Vec<u8>>),
    Page(AnnouncePage),
    Removed(usize),
    Stats(StorageStats),
    Maintenance(StorageMaintenance),
    /// SQLite PASSIVE checkpoint returns how many WAL frames are uncheckpointed.
    /// Readers can defer progress; this is not reported as a successful truncation.
    Checkpoint {
        remaining_frames: i64,
    },
}

/// Blocking backend contract. Production callers use StorageHandle, never
/// call this on a Tokio packet-processing task. Apply must be all-or-nothing.
pub trait TransportStorage: Send {
    fn execute(&mut self, request: Request) -> Result<Reply>;
}

pub(super) fn metadata_bytes(a: &RecentAnnounce) -> usize {
    256 + a.app_data.as_ref().map_or(0, Vec::len)
}

fn validate_packet(hash: &PacketHash, raw: &[u8]) -> Result<DestinationHash> {
    if raw.len() > MAX_VALUE_BYTES {
        return Err(StorageError::Invalid("packet too large"));
    }
    let (header, _) = rns_wire::header::PacketHeader::unpack(raw)
        .map_err(|_| StorageError::Invalid("invalid packet header"))?;
    if header.flags.packet_type != rns_wire::flags::PacketType::Announce
        || rns_wire::hash::packet_hash(raw, header.flags.header_type) != *hash
    {
        return Err(StorageError::Invalid("announce packet hash mismatch"));
    }
    Ok(header.destination_hash)
}

impl Request {
    /// Operation name only: never log packet bytes, identities or RPC payloads.
    pub(crate) fn operation(&self) -> &'static str {
        match self {
            Self::ClearAnnounces => "clear_announces",
            Self::BeginSweep => "begin_sweep",
            Self::KeepPackets { .. } => "keep_packets",
            Self::FinishSweep { .. } => "finish_sweep",
            Self::CleanKnown { .. } => "clean_known",
            Self::Apply(_) => "apply",
            Self::Announce(_) => "announce",
            Self::Packet(_) => "packet",
            Self::Page(_) => "page",
            Self::ExpireAnnounces { .. } => "expire_announces",
            Self::CollectPackets { .. } => "collect_packets",
            Self::Stats => "stats",
            Self::Maintain { .. } => "maintain",
            Self::Checkpoint => "checkpoint",
        }
    }

    pub(super) fn validate(&self) -> Result<()> {
        match self {
            Self::KeepPackets { hashes, .. } if hashes.capacity() > MAX_BATCH_ITEMS => {
                return Err(StorageError::Invalid("keep-set chunk limit"));
            }
            Self::CleanKnown {
                unused_before,
                used_before,
                limit,
            } if !unused_before.is_finite()
                || !used_before.is_finite()
                || *limit == 0
                || *limit > MAX_BATCH_ITEMS =>
            {
                return Err(StorageError::Invalid("cleanup bounds"));
            }
            Self::Apply(mutations) => {
                if mutations.len() > MAX_BATCH_ITEMS {
                    return Err(StorageError::Invalid("too many batch mutations"));
                }
                // Count allocated capacity, not only length: an almost-empty
                // Vec with a large spare allocation still occupies queue RAM.
                let mut size = mutations.capacity() * std::mem::size_of::<Mutation>();
                if size > MAX_BATCH_BYTES {
                    return Err(StorageError::Invalid("batch byte budget exceeded"));
                }
                for m in mutations {
                    match m {
                        Mutation::PutAnnounce { announce: a, raw } => {
                            if !a.timestamp.is_finite()
                                || a.last_used.is_some_and(|t| !t.is_finite())
                            {
                                return Err(StorageError::Invalid("non-finite announce time"));
                            }
                            if a.app_data
                                .as_ref()
                                .is_some_and(|d| d.len() > MAX_VALUE_BYTES)
                            {
                                return Err(StorageError::Invalid("app_data too large"));
                            }
                            size += a.app_data.as_ref().map_or(0, Vec::capacity);
                            if let Some(raw) = raw {
                                let hash = a
                                    .packet_hash
                                    .ok_or(StorageError::Invalid("raw without hash"))?;
                                if validate_packet(&hash, raw)? != a.dest_hash {
                                    return Err(StorageError::Invalid(
                                        "packet destination mismatch",
                                    ));
                                }
                                size += raw.capacity();
                            }
                        }
                        Mutation::PutPacket { hash, raw } => {
                            validate_packet(hash, raw)?;
                            size += raw.capacity();
                        }
                        Mutation::Touch { at, .. } if !at.is_finite() => {
                            return Err(StorageError::Invalid("non-finite touch time"));
                        }
                        _ => {}
                    }
                    if size > MAX_BATCH_BYTES {
                        return Err(StorageError::Invalid("batch byte budget exceeded"));
                    }
                }
            }
            Self::Page(q) if q.limit() > MAX_PAGE_ITEMS => {
                return Err(StorageError::Invalid("page limit"));
            }
            Self::ExpireAnnounces { before, limit }
                if !before.is_finite() || *limit == 0 || *limit > MAX_BATCH_ITEMS =>
            {
                return Err(StorageError::Invalid("expiry bounds"));
            }
            Self::CollectPackets { limit } if *limit == 0 || *limit > MAX_BATCH_ITEMS => {
                return Err(StorageError::Invalid("GC limit"));
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
