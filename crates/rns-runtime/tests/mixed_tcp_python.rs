use rns_transport::actor::TransportActor;
use rns_transport::constants::{IC_BURST_MIN_SAMPLES, InterfaceDirection, InterfaceMode};
use rns_transport::inbound_queue::InboundQueueLimits;
use rns_transport::messages::{
    InterfaceEntry, TransportMessage, TransportQuery, TransportQueryResponse,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Python reference and local TCP sockets"]
async fn python_mixed_tcp_load_and_disconnect() {
    timeout(Duration::from_secs(45), exercise(false))
        .await
        .unwrap();
}

#[cfg(feature = "sqlite")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Python reference and local TCP sockets"]
async fn python_mixed_tcp_load_and_disconnect_sqlite() {
    timeout(Duration::from_secs(45), exercise(true))
        .await
        .unwrap();
}

async fn query(
    tx: &mpsc::Sender<TransportMessage>,
    query: TransportQuery,
) -> TransportQueryResponse {
    let (response_tx, rx) = tokio::sync::oneshot::channel();
    tx.send(TransportMessage::Rpc { query, response_tx })
        .await
        .unwrap();
    timeout(Duration::from_secs(10), rx).await.unwrap().unwrap()
}

async fn exercise(sqlite: bool) {
    let mut child = tokio::process::Command::new(
        std::env::var("RNS_PYTHON_BIN").unwrap_or_else(|_| "/usr/bin/python3.11".into()),
    )
    .args(["-B", "-c", include_str!("mixed_tcp_peer.py")])
    .arg(std::env::var("RNS_PYTHON_ROOT").unwrap_or_else(|_| "/home/room/src/Reticulum".into()))
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::inherit())
    .kill_on_drop(true)
    .spawn()
    .unwrap();
    let mut peer_input = child.stdin.take().unwrap();
    let mut peer_output = BufReader::new(child.stdout.take().unwrap()).lines();
    let hello: serde_json::Value = serde_json::from_str(
        &timeout(Duration::from_secs(10), peer_output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let port = hello["port"].as_u64().unwrap() as u16;
    let data_dest: [u8; 16] = hex::decode(hello["data"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let ann_dest: [u8; 16] = hex::decode(hello["announce"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let (mut actor, input, control) = TransportActor::new_with_control_channel_and_queue_limits(
        InboundQueueLimits::new([4; 4]).unwrap(),
    );
    let directory =
        std::env::temp_dir().join(format!("rns-mixed-tcp-{}-{port}", std::process::id()));
    #[cfg(feature = "sqlite")]
    if sqlite {
        actor
            .initialize_sqlite_storage(directory.clone())
            .await
            .unwrap();
    }
    #[cfg(not(feature = "sqlite"))]
    assert!(!sqlite);
    let (delivery_tx, mut delivery_rx) = mpsc::channel(64);
    actor.local_destinations.insert(data_dest);
    actor.destination_channels.insert(data_dest, delivery_tx);
    let (callback_tx, mut callback_rx) = mpsc::channel(64);
    control
        .send(TransportMessage::RegisterAnnounceHandler {
            aspect_filter: None,
            receive_path_responses: true,
            callback_tx,
        })
        .await
        .unwrap();
    let mut drivers = Vec::new();
    for id in 1..=2 {
        let mut config = rns_interface::tcp::TcpClientConfig::new("mixed-tcp", "127.0.0.1", port);
        config.max_reconnect_tries = Some(1);
        let handle = rns_interface::tcp::spawn_tcp_client(config, id, input.clone())
            .await
            .unwrap();
        // Ensure accept order matches the normal / ingress-limited endpoints.
        timeout(Duration::from_secs(10), async {
            while !handle.online.load(std::sync::atomic::Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let mut entry = InterfaceEntry::new(
            handle.name,
            InterfaceMode::Full,
            InterfaceDirection::bidirectional(),
            handle.bitrate,
            handle.mtu,
            handle.tx,
        );
        entry.online = Some(handle.online);
        entry.rxb = handle.rxb;
        entry.txb = handle.txb;
        if id == 1 {
            entry.ingress = rns_transport::ingress::IngressController::disabled();
        } else {
            for _ in 0..IC_BURST_MIN_SAMPLES {
                entry.ingress.received_path_request();
            }
        }
        actor.interfaces.insert(id, entry);
        drivers.push(handle.read_task);
    }
    let task = tokio::spawn(actor.run());
    // Control FIFO barrier installs the callback before the peer starts.
    query(&control, TransportQuery::GetInterfaceStats).await;
    peer_input.write_all(b"start\n").await.unwrap();
    timeout(Duration::from_secs(15), async {
        loop {
            let TransportQueryResponse::InterfaceStats(stats) =
                query(&control, TransportQuery::GetInterfaceStats).await
            else {
                panic!()
            };
            if stats.len() == 2
                && stats.iter().all(|e| e.control_traffic.prxc >= 8)
                && stats.iter().any(|e| e.control_traffic.arxc >= 8)
            {
                assert!(stats.iter().all(|e| e.rx_bytes > 0));
                assert!(stats.iter().find(|e| e.id == 2).unwrap().pr_burst_active);
                break;
            }
            tokio::task::yield_now().await;
        }
        let Some(rns_transport::link_messages::DestinationEvent::InboundPacket {
            raw,
            interface_id: 1,
        }) = delivery_rx.recv().await
        else {
            panic!()
        };
        let (_, offset) = rns_wire::header::PacketHeader::unpack(&raw).unwrap();
        assert!(
            raw[offset..].starts_with(b"mixed~}"),
            "HDLC escape roundtrip"
        );
        assert_eq!(callback_rx.recv().await.unwrap().destination_hash, ann_dest);
    })
    .await
    .unwrap();
    for _ in 0..8 {
        let TransportQueryResponse::InboundQueueStats(Some(stats)) =
            query(&control, TransportQuery::GetInboundQueueStats).await
        else {
            panic!()
        };
        assert_eq!(stats.capacities, [4; 4]);
        assert!(stats.snapshot.heights.iter().all(|n| *n <= 4));
        assert_eq!(
            stats.snapshot.total,
            stats.snapshot.heights.iter().sum::<usize>()
        );
    }
    let TransportQueryResponse::PathTable(paths) =
        query(&control, TransportQuery::GetPathTable).await
    else {
        panic!()
    };
    assert!(
        paths
            .iter()
            .any(|path| path.hash == ann_dest && path.interface_id == 1)
    );
    peer_input.write_all(b"disconnect\n").await.unwrap();
    let closed: serde_json::Value = serde_json::from_str(
        &timeout(Duration::from_secs(10), peer_output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(closed["closed"], true);
    assert!(
        closed["batches"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| (1..=256).contains(&n.as_u64().unwrap()))
    );
    assert!(
        timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    for driver in drivers {
        timeout(Duration::from_secs(15), driver)
            .await
            .unwrap()
            .unwrap();
    }
    timeout(Duration::from_secs(15), async {
        loop {
            let TransportQueryResponse::InterfaceStats(stats) =
                query(&control, TransportQuery::GetInterfaceStats).await
            else {
                panic!()
            };
            if stats.is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let TransportQueryResponse::InboundQueueStats(Some(stats)) =
        query(&control, TransportQuery::GetInboundQueueStats).await
    else {
        panic!()
    };
    assert_eq!(
        stats.snapshot.total, 0,
        "disconnected interfaces leave no admitted backlog"
    );
    let TransportQueryResponse::PathTable(paths) =
        query(&control, TransportQuery::GetPathTable).await
    else {
        panic!()
    };
    assert!(
        paths.is_empty(),
        "routes through disconnected peers must be removed"
    );
    control.send(TransportMessage::Shutdown).await.unwrap();
    timeout(Duration::from_secs(15), task)
        .await
        .unwrap()
        .unwrap();
    if sqlite {
        std::fs::remove_dir_all(directory).unwrap();
    }
}
