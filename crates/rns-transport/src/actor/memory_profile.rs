use super::TransportActor;
use crate::memory_profile::MemorySnapshot;

impl TransportActor {
    /// Traverse actor-owned state without cloning payloads or routing tables.
    /// See `memory_profile::COVERAGE` for opaque/excluded allocations.
    pub fn memory_snapshot(&self) -> MemorySnapshot {
        let mut builder = allocative::FlameGraphBuilder::default();
        builder.visit_root(self);
        let output = builder.finish();
        MemorySnapshot {
            folded: output.flamegraph().write(),
            estimated_bytes: output.flamegraph().total_size(),
            warnings: output.warnings(),
        }
    }

    pub(super) fn record_memory_profile(&mut self) {
        if !self.memory_profile.as_ref().is_some_and(|r| r.ready()) {
            return;
        }
        let started = std::time::Instant::now();
        let snapshot = self.memory_snapshot();
        let elapsed = started.elapsed();
        self.memory_profile
            .as_mut()
            .unwrap()
            .record(snapshot, elapsed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::RecentAnnounce;

    #[test]
    fn snapshot_attributes_nested_payload_and_retained_capacity() {
        let (mut actor, _tx) = TransportActor::new();
        let before = actor.memory_snapshot();
        actor.recent_announces.insert(
            [1; 16],
            RecentAnnounce {
                dest_hash: [1; 16],
                hops: 0,
                app_data: Some(vec![0; 16384]),
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
        let populated = actor.memory_snapshot();
        assert!(populated.estimated_bytes >= before.estimated_bytes + 16384);
        assert!(
            populated
                .folded
                .lines()
                .any(|l| l.contains("recent_announces;") && l.contains("app_data;"))
        );
        assert!(populated.warnings.is_empty(), "{}", populated.warnings);
        actor.recent_announces.clear();
        let cleared = actor.memory_snapshot();
        assert!(cleared.estimated_bytes < populated.estimated_bytes);
        assert!(cleared.estimated_bytes > before.estimated_bytes);
        assert!(
            cleared
                .folded
                .lines()
                .any(|l| l.contains("recent_announces;") && l.contains("unused_capacity"))
        );
        actor.recent_announces.shrink_to_fit();
        assert_eq!(
            before.estimated_bytes,
            actor.memory_snapshot().estimated_bytes
        );
    }

    #[test]
    fn snapshot_covers_route_history_and_waiter_buffers() {
        let (mut actor, _tx) = TransportActor::new();
        let mut path =
            crate::path_table::PathEntry::new(None, 1, 1, crate::constants::InterfaceMode::Full);
        path.random_blobs.push_back([0; 10]);
        actor.path_table.insert([1; 16], path);
        let (sender, _receiver) = tokio::sync::oneshot::channel();
        actor.path_waiters.insert([2; 16], vec![(sender, 0.0)]);
        let profile = actor.memory_snapshot();
        assert!(profile.folded.contains("random_blobs;"));
        assert!(profile.folded.contains("opaque_waiters"));
        assert!(profile.warnings.is_empty(), "{}", profile.warnings);
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn sqlite_snapshot_covers_raw_payload_without_reading_database() {
        let (mut actor, tx) = TransportActor::new();
        actor.memory_profile = None;
        let dir = std::env::temp_dir().join(format!(
            "rns-allocative-test-{}-{}",
            std::process::id(),
            crate::now_f64()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        actor
            .initialize_sqlite_database_path(dir.join("transport.sqlite"), Default::default())
            .await
            .unwrap();
        let before = actor.memory_snapshot();
        actor
            .sqlite
            .as_mut()
            .unwrap()
            .raw
            .insert([0; 32], vec![0; 8192]);
        let after = actor.memory_snapshot();
        assert!(after.estimated_bytes >= before.estimated_bytes + 8192);
        assert!(
            after
                .folded
                .lines()
                .any(|l| l.contains("SqliteState;") && l.contains("raw;"))
        );
        assert!(after.warnings.is_empty(), "{}", after.warnings);
        tx.send(crate::messages::TransportMessage::Shutdown)
            .await
            .unwrap();
        actor.run().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
}
