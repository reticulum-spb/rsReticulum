//! Optional object-tree snapshots. No allocator replacement or packet contents.
use allocative::{Allocative, Key, Visitor};
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hash};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Coverage is deliberately written alongside every snapshot.
pub const COVERAGE: &str = "Scope: TransportActor and actor-owned SQLite state, not the whole process.\n\
HashMap/HashSet capacity bytes are estimates: capacity * size_of(entry), excluding control bytes and allocator overhead.\n\
Bytes visible_payload_len is logical length, not backing allocation size; shared slices may be counted repeatedly and static bytes may not be heap allocated.\n\
Excluded: Tokio channel contents/tasks, prepared inbound identity internals, SQLite admission message contents, in-flight storage jobs and worker/SQLite C allocations, driver diagnostics/TX queues, receipt callback captures, channel/waiter pointees, resource proof cache, blackholed shared snapshot, and other runtime/link/protocol/plugin state.\n\
Inline space for opaque fields remains included. This is an object-size estimate, not RSS or a leak verdict.\n";

/// Account for accessible capacity without depending on std's private bucket layout.
pub(crate) fn visit_map<K: Allocative + Eq + Hash, V: Allocative, S: BuildHasher>(
    map: &HashMap<K, V, S>,
    visitor: &mut Visitor<'_>,
) {
    visit_map_with(map, visitor, |value, visitor| value.visit(visitor));
}

pub(crate) fn visit_map_with<K: Allocative + Eq + Hash, V, S: BuildHasher>(
    map: &HashMap<K, V, S>,
    visitor: &mut Visitor<'_>,
    visit_value: impl Fn(&V, &mut Visitor<'_>),
) {
    let mut visitor = visitor.enter_self_sized::<HashMap<K, V, S>>();
    let mut ptr = visitor.enter_unique(Key::new("ptr"), size_of::<usize>());
    let mut data = ptr.enter(
        Key::new("estimated_capacity"),
        map.capacity() * size_of::<(K, V)>(),
    );
    for (key, value) in map {
        let mut entry = data.enter(Key::new("entry"), size_of::<(K, V)>());
        entry.visit_field(Key::new("key"), key);
        entry.visit_field_with(Key::new("value"), size_of::<V>(), |v| visit_value(value, v));
        entry.exit();
    }
    data.visit_simple(
        Key::new("unused_capacity"),
        (map.capacity() - map.len()) * size_of::<(K, V)>(),
    );
    data.exit();
    ptr.exit();
    visitor.exit();
}

pub(crate) fn visit_channel_map<K: Allocative + Eq + Hash, V, S: BuildHasher>(
    map: &HashMap<K, V, S>,
    visitor: &mut Visitor<'_>,
) {
    visit_map_with(map, visitor, |_, v| {
        v.visit_simple(Key::new("opaque_handle"), size_of::<V>())
    });
}

type PathWaiters = HashMap<[u8; 16], Vec<(tokio::sync::oneshot::Sender<bool>, f64)>>;

pub(crate) fn visit_waiters(map: &PathWaiters, visitor: &mut Visitor<'_>) {
    visit_map_with(map, visitor, |waiters, v| {
        let mut v = v.enter_self_sized::<Vec<(tokio::sync::oneshot::Sender<bool>, f64)>>();
        let mut ptr = v.enter_unique(Key::new("ptr"), size_of::<usize>());
        let entry_size = size_of::<(tokio::sync::oneshot::Sender<bool>, f64)>();
        let mut data = ptr.enter(Key::new("capacity"), waiters.capacity() * entry_size);
        data.visit_simple(Key::new("opaque_waiters"), waiters.len() * entry_size);
        data.visit_simple(
            Key::new("unused_capacity"),
            (waiters.capacity() - waiters.len()) * entry_size,
        );
        data.exit();
        ptr.exit();
        v.exit();
    });
}

pub(crate) fn visit_set<T: Allocative + Eq + Hash, S: BuildHasher>(
    set: &HashSet<T, S>,
    visitor: &mut Visitor<'_>,
) {
    let mut visitor = visitor.enter_self_sized::<HashSet<T, S>>();
    let mut ptr = visitor.enter_unique(Key::new("ptr"), size_of::<usize>());
    let mut data = ptr.enter(
        Key::new("estimated_capacity"),
        set.capacity() * size_of::<T>(),
    );
    data.visit_iter(set.iter());
    data.visit_simple(
        Key::new("unused_capacity"),
        (set.capacity() - set.len()) * size_of::<T>(),
    );
    data.exit();
    ptr.exit();
    visitor.exit();
}

pub(crate) fn visit_bytes(bytes: &bytes::Bytes, visitor: &mut Visitor<'_>) {
    let mut visitor = visitor.enter_self_sized::<bytes::Bytes>();
    let mut ptr = visitor.enter_unique(Key::new("ptr"), size_of::<usize>());
    ptr.visit_simple(Key::new("visible_payload_len"), bytes.len());
    ptr.exit();
    visitor.exit();
}

/// A single point-in-time object tree, with explicit accounting limitations.
pub struct MemorySnapshot {
    pub folded: String,
    pub estimated_bytes: usize,
    pub warnings: String,
}

pub(crate) struct Recorder {
    directory: PathBuf,
    interval: Duration,
    next: Instant,
    id: u64,
    sequence: u64,
    writer: Option<tokio::task::JoinHandle<()>>,
}

impl Recorder {
    pub(crate) fn from_env() -> Option<Self> {
        let directory = std::env::var_os("RNS_ALLOCATIVE_DIR")?;
        if directory.is_empty() {
            tracing::warn!("RNS_ALLOCATIVE_DIR is empty; memory profiling disabled");
            return None;
        }
        let seconds = match std::env::var("RNS_ALLOCATIVE_INTERVAL") {
            Ok(value) => match value.parse::<u64>() {
                Ok(seconds @ 1..=86400) => seconds,
                _ => {
                    tracing::warn!(
                        "RNS_ALLOCATIVE_INTERVAL must be 1..=86400 seconds; memory profiling disabled"
                    );
                    return None;
                }
            },
            Err(std::env::VarError::NotPresent) => 300,
            Err(_) => {
                tracing::warn!("Invalid RNS_ALLOCATIVE_INTERVAL; memory profiling disabled");
                return None;
            }
        };
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let directory = PathBuf::from(directory);
        tracing::info!(
            ?directory,
            interval_secs = seconds,
            "allocative memory snapshots enabled"
        );
        Some(Self {
            directory,
            interval: Duration::from_secs(seconds),
            next: Instant::now(),
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            sequence: 0,
            writer: None,
        })
    }

    pub(crate) fn ready(&self) -> bool {
        Instant::now() >= self.next && self.writer.as_ref().is_none_or(|w| w.is_finished())
    }

    pub(crate) fn record(&mut self, snapshot: MemorySnapshot, elapsed: Duration) {
        self.next = Instant::now() + self.interval;
        self.sequence += 1;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let stem = format!(
            "transport-{}-{}-{}-{}",
            std::process::id(),
            self.id,
            timestamp,
            self.sequence
        );
        let directory = self.directory.clone();
        self.writer = Some(tokio::task::spawn_blocking(move || {
            let path = directory.join(format!("{stem}.folded"));
            let metadata = format!(
                "timestamp_unix_ns={timestamp}\nestimated_bytes={}\ncapture_us={}\n{}\nallocative warnings:\n{}",
                snapshot.estimated_bytes,
                elapsed.as_micros(),
                COVERAGE,
                snapshot.warnings
            );
            let result = (|| -> std::io::Result<()> {
                std::fs::create_dir_all(&directory)?;
                std::fs::write(directory.join(format!("{stem}.txt")), metadata)?;
                std::fs::write(&path, snapshot.folded)
            })();
            match result {
                Ok(()) => tracing::info!(
                    ?path,
                    estimated_bytes = snapshot.estimated_bytes,
                    capture_us = elapsed.as_micros() as u64,
                    "allocative memory snapshot written"
                ),
                Err(error) => tracing::warn!(?path, %error, "allocative memory snapshot failed"),
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn recorder_writes_snapshot_and_coverage_and_bounds_pending_work() {
        let directory = std::env::temp_dir().join(format!(
            "rns-profile-recorder-{}-{}",
            std::process::id(),
            crate::now_f64()
        ));
        let mut recorder = Recorder {
            directory: directory.clone(),
            interval: Duration::from_secs(300),
            next: Instant::now(),
            id: 0,
            sequence: 0,
            writer: None,
        };
        assert!(recorder.ready());
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        recorder.writer = Some(tokio::spawn(async move {
            let _ = rx.await;
        }));
        assert!(!recorder.ready());
        tx.send(()).unwrap();
        recorder.writer.take().unwrap().await.unwrap();
        recorder.record(
            MemorySnapshot {
                folded: "TransportActor;example 128\n".into(),
                estimated_bytes: 128,
                warnings: "test diagnostic".into(),
            },
            Duration::from_micros(12),
        );
        recorder.writer.take().unwrap().await.unwrap();
        assert!(!recorder.ready());
        let files: Vec<_> = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(files.len(), 2);
        let folded = files
            .iter()
            .find(|p| p.extension().unwrap() == "folded")
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(folded).unwrap(),
            "TransportActor;example 128\n"
        );
        let metadata = std::fs::read_to_string(folded.with_extension("txt")).unwrap();
        assert!(metadata.contains(COVERAGE));
        assert!(metadata.contains("capture_us=12"));
        assert!(metadata.contains("test diagnostic"));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
