use super::*;

fn fixture(identity: &rns_identity::identity::Identity) -> (RecentAnnounce, Vec<u8>) {
    let app = "lxmf.delivery";
    let dest = rns_identity::destination::Destination::hash_from_name_and_identity(
        app,
        Some(&identity.hash),
    );
    let payload = rns_identity::announce::AnnounceData::create(identity, app, Some(b"peer"), None)
        .unwrap()
        .pack();
    let header = rns_wire::header::PacketHeader {
        flags: rns_wire::flags::PacketFlags {
            header_type: rns_wire::flags::HeaderType::Header1,
            context_flag: false,
            transport_type: rns_wire::flags::TransportType::Broadcast,
            destination_type: rns_wire::flags::DestinationType::Single,
            packet_type: rns_wire::flags::PacketType::Announce,
        },
        hops: 1,
        transport_id: None,
        destination_hash: dest,
        context: rns_wire::context::PacketContext::None,
    };
    let mut raw = header.pack().expect("locally constructed header");
    raw.extend_from_slice(&payload);
    let hash = rns_wire::hash::packet_hash(&raw, header.flags.header_type);
    (
        RecentAnnounce {
            dest_hash: dest,
            hops: 1,
            app_data: Some(b"peer".to_vec()),
            timestamp: 100.25,
            public_key: Some(identity.get_public_key()),
            ratchet: None,
            packet_hash: Some(hash),
            is_path_response: false,
            retained: false,
            last_used: None,
            name_hash: rns_identity::name_hash::name_hash(app),
        },
        raw,
    )
}

fn apply(store: &mut dyn TransportStorage, mutations: Vec<Mutation>) {
    assert!(matches!(
        store.execute(Request::Apply(mutations)).unwrap(),
        Reply::Applied
    ));
}
fn get(store: &mut dyn TransportStorage, dest: DestinationHash) -> Option<RecentAnnounce> {
    match store.execute(Request::Announce(dest)).unwrap() {
        Reply::Announce(a) => a,
        r => panic!("{r:?}"),
    }
}
fn packet(store: &mut dyn TransportStorage, hash: PacketHash) -> Option<Vec<u8>> {
    match store.execute(Request::Packet(hash)).unwrap() {
        Reply::Packet(a) => a,
        r => panic!("{r:?}"),
    }
}
fn stats(store: &mut dyn TransportStorage) -> StorageStats {
    match store.execute(Request::Stats).unwrap() {
        Reply::Stats(s) => s,
        r => panic!("{r:?}"),
    }
}
fn removed(store: &mut dyn TransportStorage, request: Request, expected: usize) {
    assert!(matches!(store.execute(request).unwrap(), Reply::Removed(n) if n == expected));
}

fn replacement_contract(store: &mut dyn TransportStorage) {
    let identity = rns_identity::identity::Identity::new();
    let (mut a1, raw1) = fixture(&identity);
    a1.retained = true;
    a1.last_used = Some(200.5);
    let (a2, raw2) = fixture(&identity);
    let h1 = a1.packet_hash.unwrap();
    let h2 = a2.packet_hash.unwrap();
    assert_ne!(h1, h2);
    let dest = a1.dest_hash;
    let owners = [
        PacketOwner::Path(dest),
        PacketOwner::Tunnel {
            tunnel: [7; 32],
            destination: dest,
        },
        PacketOwner::Cache([8; 32]),
    ];
    apply(
        store,
        vec![Mutation::PutAnnounce {
            announce: a1,
            raw: Some(raw1.clone()),
        }],
    );
    apply(
        store,
        owners
            .iter()
            .map(|owner| Mutation::SetReference {
                owner: owner.clone(),
                hash: h1,
            })
            .collect(),
    );
    apply(
        store,
        vec![Mutation::PutAnnounce {
            announce: a2,
            raw: Some(raw2.clone()),
        }],
    );
    let found = get(store, dest).unwrap();
    assert!(found.retained);
    assert_eq!(found.last_used, Some(200.5));
    assert_eq!(found.timestamp, 100.25);
    assert_eq!(found.public_key, Some(identity.get_public_key()));
    assert_eq!(found.app_data.as_deref(), Some(b"peer".as_slice()));
    assert_eq!(found.packet_hash, Some(h2));
    removed(store, Request::CollectPackets { limit: 128 }, 0);
    assert_eq!(packet(store, h1), Some(raw1.clone()));
    assert_eq!(packet(store, h2), Some(raw2));
    let mut forwarded = raw1.clone();
    forwarded[1] = 4;
    apply(
        store,
        vec![Mutation::PutPacket {
            hash: h1,
            raw: forwarded,
        }],
    );
    assert_eq!(
        packet(store, h1),
        Some(raw1.clone()),
        "hash excludes forwarding hops; keep first packet"
    );

    // Failure after an earlier mutation must roll back both metadata and blob.
    let (a3, raw3) = fixture(&identity);
    let h3 = a3.packet_hash.unwrap();
    let before = stats(store);
    assert!(
        store
            .execute(Request::Apply(vec![
                Mutation::PutAnnounce {
                    announce: a3,
                    raw: Some(raw3)
                },
                Mutation::SetReference {
                    owner: PacketOwner::Cache([99; 32]),
                    hash: [99; 32]
                },
            ]))
            .is_err()
    );
    assert_eq!(stats(store), before);
    assert_eq!(get(store, dest).unwrap().packet_hash, Some(h2));
    assert!(packet(store, h3).is_none());

    removed(
        store,
        Request::ExpireAnnounces {
            before: 1000.0,
            limit: 128,
        },
        0,
    );
    apply(
        store,
        vec![Mutation::SetRetained {
            destination: dest,
            retained: false,
        }],
    );
    removed(
        store,
        Request::ExpireAnnounces {
            before: 1000.0,
            limit: 128,
        },
        0,
    ); // old path protects destination
    apply(store, vec![Mutation::RemoveAnnounce(dest)]);
    removed(store, Request::CollectPackets { limit: 128 }, 1); // A2 has no owner
    for (index, owner) in owners.into_iter().enumerate() {
        apply(store, vec![Mutation::RemoveReference(owner)]);
        removed(
            store,
            Request::CollectPackets { limit: 128 },
            usize::from(index == 2),
        );
        if index < 2 {
            assert_eq!(packet(store, h1), Some(raw1.clone()));
        }
    }
    assert_eq!(stats(store), StorageStats::default());
}

fn pages_contract(store: &mut dyn TransportStorage) {
    let identity = rns_identity::identity::Identity::new();
    let (base, _) = fixture(&identity);
    for n in 0..9u8 {
        let mut a = base.clone();
        a.dest_hash = [n; 16];
        a.packet_hash = None;
        a.app_data = Some(vec![n; MAX_VALUE_BYTES]);
        a.name_hash = [n % 2; 10];
        a.retained = n == 0;
        apply(
            store,
            vec![Mutation::PutAnnounce {
                announce: a,
                raw: None,
            }],
        );
    }
    let mut cursor = None;
    let mut all = Vec::new();
    loop {
        let Reply::Page(page) = store
            .execute(Request::Page(AnnouncePageQuery {
                after: cursor,
                ..Default::default()
            }))
            .unwrap()
        else {
            panic!()
        };
        assert!(page.entries.iter().map(metadata_bytes).sum::<usize>() <= MAX_PAGE_BYTES);
        if page.entries.is_empty() {
            break;
        }
        all.extend(page.entries.iter().map(|a| a.dest_hash));
        cursor = page.next;
    }
    assert_eq!(all, (0..9u8).map(|n| [n; 16]).collect::<Vec<_>>());
    let Reply::Page(page) = store
        .execute(Request::Page(AnnouncePageQuery {
            name_hash: Some([1; 10]),
            limit: 2,
            ..Default::default()
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        page.entries
            .iter()
            .map(|a| a.dest_hash[0])
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
    apply(
        store,
        vec![
            Mutation::Touch {
                destination: [1; 16],
                at: 500.0,
            },
            Mutation::Touch {
                destination: [1; 16],
                at: 150.0,
            },
        ],
    );
    assert_eq!(get(store, [1; 16]).unwrap().last_used, Some(500.0));
    removed(
        store,
        Request::ExpireAnnounces {
            before: 300.0,
            limit: 2,
        },
        2,
    );
    assert!(get(store, [0; 16]).is_some());
    assert!(get(store, [1; 16]).is_some());
    assert_eq!(stats(store).announces, 7);
    assert!(
        store
            .execute(Request::Page(AnnouncePageQuery {
                limit: MAX_PAGE_ITEMS + 1,
                ..Default::default()
            }))
            .is_err()
    );
    assert!(store.execute(Request::CollectPackets { limit: 0 }).is_err());
    assert!(
        store
            .execute(Request::Apply(vec![Mutation::Touch {
                destination: [1; 16],
                at: f64::NAN
            }]))
            .is_err()
    );
}

fn gc_pages_contract(store: &mut dyn TransportStorage) {
    let mut entries: Vec<_> = (0..6)
        .map(|_| fixture(&rns_identity::identity::Identity::new()))
        .collect();
    entries.sort_by_key(|(a, _)| a.dest_hash);
    entries[2].0.retained = true;
    entries[3].0.last_used = Some(1000.0);
    let destinations: Vec<_> = entries.iter().map(|(a, _)| a.dest_hash).collect();
    let hashes: Vec<_> = entries
        .iter()
        .map(|(a, _)| a.packet_hash.unwrap())
        .collect();
    apply(
        store,
        entries
            .into_iter()
            .map(|(announce, raw)| Mutation::PutAnnounce {
                announce,
                raw: Some(raw),
            })
            .collect(),
    );
    apply(
        store,
        vec![Mutation::SetReference {
            owner: PacketOwner::Path(destinations[1]),
            hash: hashes[1],
        }],
    );
    let generation = begin_sweep(store);
    store
        .execute(Request::KeepPackets {
            generation,
            hashes: vec![hashes[0]],
        })
        .unwrap();
    store.execute(Request::FinishSweep { generation }).unwrap();
    let mut after = None;
    for (page, expected) in [0, 0, 2].into_iter().enumerate() {
        let Reply::CleanedPage {
            removed,
            next,
            destinations: deleted,
        } = store
            .execute(Request::CleanKnownPage {
                unused_before: 500.0,
                used_before: 500.0,
                after,
                limit: 2,
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(removed, expected);
        assert_eq!(
            deleted,
            if expected == 0 {
                vec![]
            } else {
                destinations[4..6].to_vec()
            }
        );
        assert_eq!(
            next,
            Some(destinations[page * 2 + 1]),
            "page bounds include protected rows"
        );
        after = next;
    }
    assert!(matches!(
        store
            .execute(Request::CleanKnownPage {
                unused_before: 500.0,
                used_before: 500.0,
                after,
                limit: 2,
            })
            .unwrap(),
        Reply::CleanedPage {
            removed: 0,
            next: None,
            ..
        }
    ));
    let mut after = None;
    let mut total = 0;
    let mut sorted = hashes.clone();
    sorted.sort_unstable();
    for page in 0..3 {
        let Reply::CollectedPage { removed, next } = store
            .execute(Request::CollectPacketsPage { after, limit: 2 })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(next, Some(sorted[page * 2 + 1]));
        total += removed;
        after = next;
    }
    assert_eq!(total, 2);
    assert!(matches!(
        store
            .execute(Request::CollectPacketsPage { after, limit: 2 })
            .unwrap(),
        Reply::CollectedPage {
            removed: 0,
            next: None,
            ..
        }
    ));
    for hash in &hashes[..4] {
        assert!(packet(store, *hash).is_some());
    }
    for hash in &hashes[4..] {
        assert!(packet(store, *hash).is_none());
    }
    assert!(
        store
            .execute(Request::CollectPacketsPage {
                after: None,
                limit: MAX_BATCH_ITEMS + 1
            })
            .is_err()
    );
    assert!(
        store
            .execute(Request::CleanKnownPage {
                unused_before: f64::NAN,
                used_before: 0.0,
                after: None,
                limit: 128
            })
            .is_err()
    );
}

#[test]
fn memory_gc_pages_contract() {
    gc_pages_contract(&mut MemoryTransportStorage::default());
}

fn begin_sweep(store: &mut dyn TransportStorage) -> i64 {
    let Reply::Generation(generation) = store.execute(Request::BeginSweep).unwrap() else {
        panic!()
    };
    generation
}

fn sweep_contract(store: &mut dyn TransportStorage) {
    let (first, raw1) = fixture(&rns_identity::identity::Identity::new());
    let (second, raw2) = fixture(&rns_identity::identity::Identity::new());
    let h1 = first.packet_hash.unwrap();
    let h2 = second.packet_hash.unwrap();
    apply(
        store,
        vec![
            Mutation::PutPacket {
                hash: h1,
                raw: raw1,
            },
            Mutation::PutPacket {
                hash: h2,
                raw: raw2,
            },
        ],
    );
    let g1 = begin_sweep(store);
    store
        .execute(Request::KeepPackets {
            generation: g1,
            hashes: vec![h1],
        })
        .unwrap();
    store
        .execute(Request::FinishSweep { generation: g1 })
        .unwrap();
    let g2 = begin_sweep(store);
    store
        .execute(Request::KeepPackets {
            generation: g2,
            hashes: vec![h2, [0xff; 32]],
        })
        .unwrap();
    // An incomplete sweep protects old pins and every acknowledged new pin.
    removed(store, Request::CollectPackets { limit: 128 }, 0);
    assert!(
        store
            .execute(Request::FinishSweep { generation: g1 })
            .is_err()
    );
    assert!(
        store
            .execute(Request::KeepPackets {
                generation: g1,
                hashes: vec![h1]
            })
            .is_err()
    );
    let g3 = begin_sweep(store);
    removed(store, Request::CollectPackets { limit: 128 }, 0);
    store
        .execute(Request::KeepPackets {
            generation: g3,
            hashes: vec![h2, h2],
        })
        .unwrap();
    store
        .execute(Request::FinishSweep { generation: g3 })
        .unwrap();
    removed(store, Request::CollectPackets { limit: 128 }, 1);
    assert!(packet(store, h1).is_none());
    assert!(packet(store, h2).is_some());
    assert!(
        store
            .execute(Request::FinishSweep { generation: g3 })
            .is_err()
    );
    assert!(
        store
            .execute(Request::KeepPackets {
                generation: g3,
                hashes: vec![h2]
            })
            .is_err()
    );
    let g4 = begin_sweep(store);
    store
        .execute(Request::FinishSweep { generation: g4 })
        .unwrap();
    removed(store, Request::CollectPackets { limit: 128 }, 1);
}

#[test]
fn memory_sweep_contract() {
    sweep_contract(&mut MemoryTransportStorage::default());
}

#[test]
fn memory_contract() {
    replacement_contract(&mut MemoryTransportStorage::default());
    pages_contract(&mut MemoryTransportStorage::default());
}

#[test]
fn request_bounds_reject_spare_allocations_and_invalid_payloads() {
    let mut store = MemoryTransportStorage::default();
    let (a, mut raw) = fixture(&rns_identity::identity::Identity::new());
    raw.reserve(MAX_BATCH_BYTES);
    assert!(matches!(
        store.execute(Request::Apply(vec![Mutation::PutAnnounce {
            announce: a.clone(),
            raw: Some(raw)
        }])),
        Err(StorageError::Invalid(_))
    ));
    let mut oversized = a.clone();
    oversized.packet_hash = None;
    oversized.app_data = Some(vec![0; MAX_VALUE_BYTES + 1]);
    assert!(matches!(
        store.execute(Request::Apply(vec![Mutation::PutAnnounce {
            announce: oversized,
            raw: None
        }])),
        Err(StorageError::Invalid(_))
    ));
    let (_, raw) = fixture(&rns_identity::identity::Identity::new());
    assert!(matches!(
        store.execute(Request::Apply(vec![Mutation::PutPacket {
            hash: a.packet_hash.unwrap(),
            raw
        }])),
        Err(StorageError::Invalid(_))
    ));
    assert_eq!(stats(&mut store), StorageStats::default());
}

#[tokio::test(flavor = "current_thread")]
async fn worker_backpressure_shutdown_and_dropped_reply() {
    let handle = StorageHandle::start(1, || Ok(MemoryTransportStorage::default()))
        .await
        .unwrap();
    let pending = handle.try_submit(Request::Stats).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let rejected = match handle.try_submit(Request::Stats) {
        Err(e) => e,
        Ok(_) => panic!("unconsumed result must hold slot"),
    };
    assert!(matches!(rejected.error, StorageError::Busy));
    assert!(matches!(rejected.request, Some(Request::Stats)));
    pending.wait().await.unwrap();
    let identity = rns_identity::identity::Identity::new();
    let (a, raw) = fixture(&identity);
    let dest = a.dest_hash;
    let pending = handle
        .try_submit(Request::Apply(vec![Mutation::PutAnnounce {
            announce: a,
            raw: Some(raw),
        }]))
        .unwrap();
    drop(pending); // Accepted mutation still runs, even without a waiting caller.
    let mut query = Request::Announce(dest);
    let pending = loop {
        match handle.try_submit(query) {
            Ok(p) => break p,
            Err(e) => {
                assert!(matches!(e.error, StorageError::Busy));
                query = e.request.unwrap();
                tokio::task::yield_now().await;
            }
        }
    };
    assert!(matches!(
        pending.wait().await.unwrap(),
        Reply::Announce(Some(_))
    ));
    handle.try_shutdown().unwrap().wait().await.unwrap();
    assert!(
        matches!(handle.try_submit(Request::Stats),Err(e) if matches!(e.error,StorageError::Closed))
    );
}

#[cfg(feature = "sqlite")]
mod disk {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "rns-sqlite-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> PathBuf {
            self.0.join("transport.sqlite")
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn open(path: &Path) -> SqliteTransportStorage {
        SqliteTransportStorage::open(path, StorageRole::Standalone, Default::default()).unwrap()
    }

    #[test]
    fn sqlite_gc_pages_contract() {
        let t = Temp::new();
        gc_pages_contract(&mut open(&t.path()));
    }

    #[test]
    fn sqlite_sweep_contract_and_interrupted_restart() {
        let t = Temp::new();
        let path = t.path();
        let mut store = open(&path);
        sweep_contract(&mut store);
        let (a, raw) = fixture(&rns_identity::identity::Identity::new());
        let hash = a.packet_hash.unwrap();
        apply(&mut store, vec![Mutation::PutPacket { hash, raw }]);
        let generation = begin_sweep(&mut store);
        store
            .execute(Request::KeepPackets {
                generation,
                hashes: vec![hash],
            })
            .unwrap();
        drop(store);
        let mut store = open(&path);
        assert!(
            store.execute(Request::FinishSweep { generation }).is_err(),
            "lost scratch set cannot complete"
        );
        assert!(
            store
                .execute(Request::KeepPackets {
                    generation,
                    hashes: vec![hash]
                })
                .is_err()
        );
        removed(&mut store, Request::CollectPackets { limit: 128 }, 0);
        assert!(packet(&mut store, hash).is_some());
        let generation = begin_sweep(&mut store);
        store.execute(Request::FinishSweep { generation }).unwrap();
        removed(&mut store, Request::CollectPackets { limit: 128 }, 1);
    }

    #[test]
    fn sqlite_contract() {
        let t = Temp::new();
        replacement_contract(&mut open(&t.path()));
        let t = Temp::new();
        pages_contract(&mut open(&t.path()));
    }

    #[test]
    fn restart_preserves_old_packet_references_and_retention() {
        let t = Temp::new();
        let path = t.path();
        let identity = rns_identity::identity::Identity::new();
        let (a1, raw1) = fixture(&identity);
        let (a2, raw2) = fixture(&identity);
        let dest = a1.dest_hash;
        let h1 = a1.packet_hash.unwrap();
        let h2 = a2.packet_hash.unwrap();
        let owner = PacketOwner::Tunnel {
            tunnel: [3; 32],
            destination: dest,
        };
        {
            let mut s = open(&path);
            apply(
                &mut s,
                vec![
                    Mutation::PutAnnounce {
                        announce: a1,
                        raw: Some(raw1.clone()),
                    },
                    Mutation::SetReference {
                        owner: owner.clone(),
                        hash: h1,
                    },
                ],
            );
            apply(
                &mut s,
                vec![Mutation::PutAnnounce {
                    announce: a2,
                    raw: Some(raw2),
                }],
            );
        }
        let mut s = open(&path);
        assert_eq!(get(&mut s, dest).unwrap().packet_hash, Some(h2));
        removed(&mut s, Request::CollectPackets { limit: 128 }, 0);
        assert_eq!(packet(&mut s, h1), Some(raw1));
        apply(&mut s, vec![Mutation::RemoveReference(owner)]);
        removed(&mut s, Request::CollectPackets { limit: 128 }, 1);
    }

    #[test]
    fn clients_second_owners_foreign_and_newer_schemas_are_rejected() {
        let t = Temp::new();
        let path = t.path();
        assert!(matches!(
            SqliteTransportStorage::open(&path, StorageRole::SharedClient, Default::default()),
            Err(StorageError::NotOwner)
        ));
        assert_eq!(std::fs::read_dir(&t.0).unwrap().count(), 0);
        let s = open(&path);
        #[cfg(unix)]
        {
            let alias = t.0.join("alias.sqlite");
            std::os::unix::fs::symlink(&path, &alias).unwrap();
            assert!(matches!(
                SqliteTransportStorage::open(&alias, StorageRole::Standalone, Default::default()),
                Err(StorageError::AlreadyOwned)
            ));
        }
        assert!(matches!(
            SqliteTransportStorage::open(&path, StorageRole::SharedOwner, Default::default()),
            Err(StorageError::AlreadyOwned)
        ));
        drop(s);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
        drop(conn);
        assert!(matches!(
            SqliteTransportStorage::open(&path, StorageRole::Standalone, Default::default()),
            Err(StorageError::Schema(999))
        ));
        let conn = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            999
        );
        drop(conn);
        let foreign = t.0.join("lxmf.sqlite");
        let conn = rusqlite::Connection::open(&foreign).unwrap();
        conn.execute_batch(
            "CREATE TABLE messages(id INTEGER PRIMARY KEY); INSERT INTO messages VALUES(42);",
        )
        .unwrap();
        drop(conn);
        assert!(matches!(
            SqliteTransportStorage::open(&foreign, StorageRole::Standalone, Default::default()),
            Err(StorageError::ForeignDatabase)
        ));
        let conn = rusqlite::Connection::open(&foreign).unwrap();
        assert_eq!(
            conn.query_row("SELECT id FROM messages", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            42
        );
    }

    #[test]
    fn version_two_database_gets_bounded_cleanup_lookup() {
        let t = Temp::new();
        let path = t.path();
        drop(open(&path));

        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("DROP INDEX packet_blobs_destination", [])
            .unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        drop(conn);

        drop(open(&path));
        let conn = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            3
        );
        let mut statement = conn
            .prepare("SELECT name FROM pragma_index_list('packet_blobs') WHERE name='packet_blobs_destination'")
            .unwrap();
        assert!(statement.exists([]).unwrap());

        let plan: String = conn
            .prepare(
                "EXPLAIN QUERY PLAN SELECT 1 FROM packet_blobs p
                 WHERE p.destination_hash=?1
                   AND EXISTS(SELECT 1 FROM packet_keep k WHERE k.packet_hash=p.packet_hash)",
            )
            .unwrap()
            .query_map([&[0_u8; 16][..]], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n");
        assert!(
            plan.contains("packet_blobs_destination"),
            "cleanup lookup must use the destination index: {plan}"
        );
    }

    #[test]
    fn maintenance_reports_sizes_checkpoints_and_bounds_vacuum() {
        let t = Temp::new();
        let mut store = open(&t.path());
        for n in 0..32_u8 {
            let (mut announce, raw) = fixture(&rns_identity::identity::Identity::new());
            announce.app_data = Some(vec![n; 4096]);
            apply(
                &mut store,
                vec![Mutation::PutAnnounce {
                    announce,
                    raw: Some(raw),
                }],
            );
        }
        removed(&mut store, Request::ClearAnnounces, 32);
        removed(&mut store, Request::CollectPackets { limit: 128 }, 32);

        let Reply::Maintenance(checkpoint_only) = store
            .execute(Request::Maintain { vacuum_pages: 0 })
            .unwrap()
        else {
            panic!("invalid maintenance reply")
        };
        assert!(checkpoint_only.database_bytes > 0);
        assert!(checkpoint_only.page_size > 0);
        assert!(checkpoint_only.page_count > 0);
        assert_eq!(checkpoint_only.vacuumed_pages, 0);
        assert_eq!(checkpoint_only.page_cache_kib, 1024);

        let Reply::Maintenance(maintenance) = store
            .execute(Request::Maintain { vacuum_pages: 4 })
            .unwrap()
        else {
            panic!("invalid maintenance reply")
        };
        assert!(maintenance.vacuumed_pages <= 4);
        assert!(maintenance.free_pages <= checkpoint_only.free_pages);
    }

    #[test]
    fn database_options_and_io_errors() {
        let t = Temp::new();
        let path = t.path();
        let mut s = open(&path);
        let conn = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        assert_eq!(
            conn.pragma_query_value(None, "auto_vacuum", |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        let (a, raw) = fixture(&rns_identity::identity::Identity::new());
        assert!(matches!(
            s.execute(Request::Apply(vec![Mutation::PutAnnounce {
                announce: a,
                raw: Some(raw)
            }])),
            Err(StorageError::Sqlite(_))
        ));
        conn.execute_batch("ROLLBACK").unwrap();
        assert_eq!(stats(&mut s), StorageStats::default());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for entry in std::fs::read_dir(&t.0).unwrap() {
                assert_eq!(
                    entry.unwrap().metadata().unwrap().permissions().mode() & 0o077,
                    0
                );
            }
        }
        drop(conn);
        drop(s);
        let corrupt = t.0.join("corrupt.sqlite");
        std::fs::write(&corrupt, b"not a database").unwrap();
        assert!(
            SqliteTransportStorage::open(&corrupt, StorageRole::Standalone, Default::default())
                .is_err()
        );
        assert_eq!(std::fs::read(corrupt).unwrap(), b"not a database");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn worker_disk_shutdown_releases_owner() {
        let t = Temp::new();
        let path = t.path();
        let handle =
            StorageHandle::open_sqlite(path.clone(), StorageRole::SharedOwner, Default::default())
                .await
                .unwrap();
        let (a, raw) = fixture(&rns_identity::identity::Identity::new());
        let pending = handle
            .try_submit(Request::Apply(vec![Mutation::PutAnnounce {
                announce: a,
                raw: Some(raw),
            }]))
            .unwrap();
        let close = handle.try_shutdown().unwrap();
        close.wait().await.unwrap();
        pending.wait().await.unwrap();
        let mut reopened = open(&path);
        assert_eq!(stats(&mut reopened).announces, 1);
    }

    #[test]
    #[ignore = "subprocess fixture: invoked by abrupt_exit_recovers_wal_and_releases_lock"]
    fn sqlite_crash_fixture() {
        let dir =
            PathBuf::from(std::env::var_os("RNS_SQLITE_CRASH_FIXTURE").expect("fixture directory"));
        let path = dir.join("transport.sqlite");
        let mut s = open(&path);
        let identity = rns_identity::identity::Identity::new();
        let (a1, raw1) = fixture(&identity);
        let (a2, raw2) = fixture(&identity);
        let dest = a1.dest_hash;
        let h1 = a1.packet_hash.unwrap();
        let h2 = a2.packet_hash.unwrap();
        apply(
            &mut s,
            vec![
                Mutation::PutAnnounce {
                    announce: a1,
                    raw: Some(raw1),
                },
                Mutation::SetReference {
                    owner: PacketOwner::Tunnel {
                        tunnel: [3; 32],
                        destination: dest,
                    },
                    hash: h1,
                },
            ],
        );
        apply(
            &mut s,
            vec![Mutation::PutAnnounce {
                announce: a2,
                raw: Some(raw2),
            }],
        );
        let (third, raw3) = fixture(&rns_identity::identity::Identity::new());
        let h3 = third.packet_hash.unwrap();
        apply(
            &mut s,
            vec![Mutation::PutPacket {
                hash: h3,
                raw: raw3,
            }],
        );
        let old_generation = begin_sweep(&mut s);
        s.execute(Request::KeepPackets {
            generation: old_generation,
            hashes: vec![h1],
        })
        .unwrap();
        s.execute(Request::FinishSweep {
            generation: old_generation,
        })
        .unwrap();
        let interrupted_generation = begin_sweep(&mut s);
        s.execute(Request::KeepPackets {
            generation: interrupted_generation,
            hashes: vec![h3],
        })
        .unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("BEGIN IMMEDIATE; UPDATE announces SET app_data=x'00';")
            .unwrap();
        let mut marker = dest.to_vec();
        marker.extend(h1);
        marker.extend(h2);
        marker.extend(h3);
        marker.extend(interrupted_generation.to_be_bytes());
        std::fs::write(dir.join("marker"), marker).unwrap();
        // Skip Rust destructors, connection close and all orderly checkpoints.
        // This models process death, not hardware power-loss durability.
        std::process::exit(0);
    }

    #[test]
    fn abrupt_exit_recovers_wal_and_releases_lock() {
        let t = Temp::new();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "storage::tests::disk::sqlite_crash_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("RNS_SQLITE_CRASH_FIXTURE", &t.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let marker = std::fs::read(t.0.join("marker")).unwrap();
        let dest: [u8; 16] = marker[..16].try_into().unwrap();
        let h1: [u8; 32] = marker[16..48].try_into().unwrap();
        let h2: [u8; 32] = marker[48..80].try_into().unwrap();
        let h3: [u8; 32] = marker[80..112].try_into().unwrap();
        let interrupted_generation = i64::from_be_bytes(marker[112..120].try_into().unwrap());
        let mut s = open(&t.path());
        assert!(
            s.execute(Request::FinishSweep {
                generation: interrupted_generation
            })
            .is_err()
        );
        let a = get(&mut s, dest).unwrap();
        assert_eq!(a.packet_hash, Some(h2));
        assert_eq!(
            a.app_data.as_deref(),
            Some(b"peer".as_slice()),
            "uncommitted edit must roll back"
        );
        removed(&mut s, Request::CollectPackets { limit: 128 }, 0);
        assert!(packet(&mut s, h1).is_some());
        apply(
            &mut s,
            vec![Mutation::RemoveReference(PacketOwner::Tunnel {
                tunnel: [3; 32],
                destination: dest,
            })],
        );
        removed(&mut s, Request::CollectPackets { limit: 128 }, 0);
        assert!(
            packet(&mut s, h1).is_some(),
            "old sweep pin survives process death"
        );
        assert!(
            packet(&mut s, h3).is_some(),
            "acknowledged new pin survives process death"
        );
        let generation = begin_sweep(&mut s);
        s.execute(Request::KeepPackets {
            generation,
            hashes: vec![h3],
        })
        .unwrap();
        s.execute(Request::FinishSweep { generation }).unwrap();
        removed(&mut s, Request::CollectPackets { limit: 128 }, 1);
    }
}
