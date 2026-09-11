use super::*;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;

#[tokio::test]
async fn short_frames_do_not_reach_transport_or_ingress_packet_counts() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut peer = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (socket, _) = listener.accept().await.unwrap();
    let (reader, _writer) = socket.into_split();
    let (tx, mut events) = mpsc::channel(1);
    let ingress = IngressControl::new();
    let rxb = Arc::new(AtomicU64::new(0));
    let mut task = tokio::spawn(backbone_read_loop(
        reader,
        7,
        tx,
        Arc::new(AtomicBool::new(true)),
        rxb.clone(),
        ingress.clone(),
        564,
    ));
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        let mut wire = Vec::new();
        for size in 0..=rns_wire::constants::HEADER_MINSIZE {
            wire.extend(hdlc::frame(&vec![hdlc::FLAG; size]));
        }
        // Short frames cannot occupy the single transport slot before this
        // boundary frame, including under fragmented HDLC input.
        let valid = vec![hdlc::ESC; rns_wire::constants::HEADER_MINSIZE + 1];
        wire.extend(hdlc::frame(&valid));
        for chunk in wire.chunks(7) {
            peer.write_all(chunk).await.unwrap();
        }
        let Some(TransportMessage::Inbound(packet)) = events.recv().await else {
            panic!("expected the first frame above the strict minimum")
        };
        assert_eq!(packet.raw.as_ref(), valid);
        assert_eq!(ingress.snapshot(7, false).packets, 1);
        assert_eq!(rxb.load(Ordering::Relaxed), wire.len() as u64);
        peer.shutdown().await.unwrap();
        (&mut task).await.unwrap();
        assert!(events.recv().await.is_none());
    })
    .await;
    if result.is_err() {
        task.abort();
        let _ = task.await;
    }
    result.unwrap();
}

#[tokio::test]
async fn reader_mtu_allowance_accepts_boundary_rejects_overflow_and_resyncs() {
    for ifac_size in [None, Some(0), Some(1), Some(16), Some(64)] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = BackboneClientConfig::new(
            "receive-mtu",
            "127.0.0.1",
            listener.local_addr().unwrap().port(),
        );
        config.bitrate = 100_000_000;
        config.receive_ifac_size = ifac_size;
        config.max_reconnect_tries = Some(1);
        let (tx, mut events) = mpsc::channel(8);
        let handle = spawn_backbone_client(config, 78, tx).await.unwrap();
        let mut task = handle.read_task;
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            let (mut peer, _) = listener.accept().await.unwrap();
            let limit = handle.mtu as usize + ifac_size.unwrap_or(64);
            let valid = vec![hdlc::FLAG; limit];
            peer.write_all(&hdlc::frame(&valid)).await.unwrap();
            let Some(TransportMessage::Inbound(packet)) = events.recv().await else {
                panic!("boundary frame")
            };
            assert_eq!(packet.raw.as_ref(), valid);
            peer.write_all(&hdlc::frame(&vec![hdlc::ESC; limit + 1]))
                .await
                .unwrap();
            peer.write_all(&hdlc::frame(b"resynchronised payload"))
                .await
                .unwrap();
            let Some(TransportMessage::Inbound(packet)) = events.recv().await else {
                panic!("resynchronised frame")
            };
            assert_eq!(packet.raw.as_ref(), b"resynchronised payload");
            peer.shutdown().await.unwrap();
            assert!(matches!(
                events.recv().await,
                Some(TransportMessage::DeregisterInterface { id: 78 })
            ));
            (&mut task).await.unwrap();
        })
        .await;
        if result.is_err() {
            task.abort();
            let _ = task.await;
        }
        result.unwrap();
    }
}

#[tokio::test]
async fn invalid_receive_ifac_size_is_rejected_before_spawn() {
    let mut client = BackboneClientConfig::new("invalid", "127.0.0.1", 1);
    client.receive_ifac_size = Some(65);
    let (tx, _) = mpsc::channel(1);
    assert!(spawn_backbone_client(client, 1, tx.clone()).await.is_err());
    let mut server = BackboneServerConfig::new("invalid", "127.0.0.1", 0);
    server.receive_ifac_size = Some(usize::MAX);
    let (handles, _) = mpsc::channel(1);
    assert!(
        spawn_backbone_server(server, 1, Arc::new(AtomicU64::new(2)), tx, handles)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn listener_children_inherit_configured_bitrate_and_mtu() {
    for bitrate in [62_500, 100_000_000, 200_000_000, 1_000_000_000] {
        let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = reservation.local_addr().unwrap().port();
        drop(reservation);
        let mut config = BackboneServerConfig::new("mtu-parent", "127.0.0.1", port);
        config.bitrate = bitrate;
        config.receive_ifac_size = Some(16);
        config.fast_flap.enabled = false;
        let (tx, mut events) = mpsc::channel(8);
        let (handles_tx, mut handles) = mpsc::channel(8);
        let parent = spawn_backbone_server(config, 1, Arc::new(AtomicU64::new(2)), tx, handles_tx)
            .await
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            let mut peer = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            let child = handles.recv().await.unwrap();
            assert_eq!(parent.bitrate, bitrate);
            assert_eq!(parent.mtu, mtu_for_bitrate(bitrate));
            assert_eq!((child.bitrate, child.mtu), (parent.bitrate, parent.mtu));
            assert_eq!(
                child.diagnostics.as_ref().unwrap().link_mtu(),
                Some(child.mtu)
            );
            assert!(
                child
                    .diagnostics
                    .as_ref()
                    .unwrap()
                    .dataplane_ingress()
                    .is_some()
            );
            assert_eq!(
                parent.diagnostics.as_ref().unwrap().link_mtu(),
                Some(parent.mtu)
            );
            assert!(
                parent
                    .diagnostics
                    .as_ref()
                    .unwrap()
                    .blocked_ip_list()
                    .is_some()
            );
            assert_eq!(child.parent_id, Some(parent.id));
            let limit = child.mtu as usize + 16;
            peer.write_all(&hdlc::frame(&vec![hdlc::ESC; limit + 1]))
                .await
                .unwrap();
            let valid = vec![hdlc::FLAG; limit];
            peer.write_all(&hdlc::frame(&valid)).await.unwrap();
            let Some(TransportMessage::Inbound(packet)) = events.recv().await else {
                panic!("child must reject overflow and accept exact IFAC boundary")
            };
            assert_eq!(packet.raw.as_ref(), valid);
            drop(peer);
            assert!(matches!(
                events.recv().await,
                Some(TransportMessage::DeregisterInterface { id: 2 })
            ));
            child.read_task.await.unwrap();
        })
        .await;
        parent.read_task.abort();
        let _ = parent.read_task.await;
        result.unwrap();
    }
}

#[tokio::test]
async fn client_metadata_uses_configured_bitrate_before_connect() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = BackboneClientConfig::new(
        "mtu-client",
        "127.0.0.1",
        listener.local_addr().unwrap().port(),
    );
    config.bitrate = 200_000_000;
    config.max_reconnect_tries = Some(1);
    let (tx, _events) = mpsc::channel(8);
    let handle = spawn_backbone_client(config, 1, tx).await.unwrap();
    assert_eq!((handle.bitrate, handle.mtu), (200_000_000, 65_536));
    handle.read_task.abort();
    let _ = handle.read_task.await;
}

#[tokio::test]
async fn full_transport_fin_and_reset_cancel_unsent_frame() {
    for reset in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut peer = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let (reader, _writer) = stream.into_split();
        let (tx, mut events) = mpsc::channel(1);
        tx.send(TransportMessage::Shutdown).await.unwrap(); // inert channel filler
        let online = Arc::new(AtomicBool::new(true));
        let ingress = IngressControl::new();
        let mut task = tokio::spawn(backbone_read_loop(
            reader,
            7,
            tx,
            online.clone(),
            Arc::new(AtomicU64::new(0)),
            ingress.clone(),
            HW_MTU,
        ));
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            peer.write_all(&hdlc::frame(b"unsent payload padding~}"))
                .await
                .unwrap();
            while ingress.snapshot(7, false).packets == 0 {
                tokio::task::yield_now().await;
            }
            assert_eq!(events.len(), 1);
            assert!(!task.is_finished());
            if reset {
                socket2::SockRef::from(&peer)
                    .set_linger(Some(Duration::ZERO))
                    .unwrap();
                drop(peer);
            } else {
                peer.shutdown().await.unwrap();
            }
            (&mut task).await.unwrap();
            assert!(!online.load(Ordering::SeqCst));
            assert!(matches!(
                events.recv().await,
                Some(TransportMessage::Shutdown)
            ));
            assert!(
                events.recv().await.is_none(),
                "unsent frame must not leak after close"
            );
            assert_eq!(ingress.snapshot(7, false).packets, 0);
        })
        .await;
        if result.is_err() {
            task.abort();
            let _ = task.await;
        }
        result.unwrap();
    }
}

#[tokio::test]
async fn full_transport_recovers_without_losing_live_peer_frames() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut peer = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (stream, _) = listener.accept().await.unwrap();
    let (reader, _writer) = stream.into_split();
    let (tx, mut events) = mpsc::channel(1);
    tx.send(TransportMessage::Shutdown).await.unwrap();
    let ingress = IngressControl::new();
    let mut task = tokio::spawn(backbone_read_loop(
        reader,
        7,
        tx,
        Arc::new(AtomicBool::new(true)),
        Arc::new(AtomicU64::new(0)),
        ingress.clone(),
        HW_MTU,
    ));
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        let mut wire = hdlc::frame(b"first payload padding~}");
        wire.extend(hdlc::frame(b"second payload padding"));
        peer.write_all(&wire).await.unwrap();
        while ingress.snapshot(7, false).packets == 0 {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !task.is_finished(),
            "live reader waits for channel capacity"
        );
        assert!(matches!(
            events.recv().await,
            Some(TransportMessage::Shutdown)
        ));
        for payload in [
            b"first payload padding~}".as_slice(),
            b"second payload padding".as_slice(),
        ] {
            let Some(TransportMessage::Inbound(packet)) = events.recv().await else {
                panic!("expected frame")
            };
            assert_eq!(packet.raw.as_ref(), payload);
        }
        peer.shutdown().await.unwrap();
        (&mut task).await.unwrap();
        assert!(events.recv().await.is_none());
    })
    .await;
    if result.is_err() {
        task.abort();
        let _ = task.await;
    }
    result.unwrap();
}

#[tokio::test]
async fn full_transport_client_closes_before_deregistration_can_enqueue() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = BackboneClientConfig::new(
        "full-transport",
        "127.0.0.1",
        listener.local_addr().unwrap().port(),
    );
    config.max_reconnect_tries = Some(1);
    let (tx, mut events) = mpsc::channel(1);
    tx.send(TransportMessage::Shutdown).await.unwrap();
    let handle = spawn_backbone_client(config, 77, tx).await.unwrap();
    let mut task = handle.read_task;
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        let (mut peer, _) = listener.accept().await.unwrap();
        while !handle.online.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        peer.write_all(&hdlc::frame(b"blocked payload padding"))
            .await
            .unwrap();
        let ingress = handle
            .diagnostics
            .as_ref()
            .unwrap()
            .dataplane_ingress()
            .unwrap();
        while ingress.snapshot(77, false).packets == 0 {
            tokio::task::yield_now().await;
        }
        peer.shutdown().await.unwrap();
        while handle.online.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            events.len(),
            1,
            "transport remains full throughout socket teardown"
        );
        let mut tail = Vec::new();
        peer.read_to_end(&mut tail).await.unwrap();
        assert!(tail.is_empty());
        assert!(
            !task.is_finished(),
            "deregistration still awaits the shared channel"
        );
        assert!(matches!(
            events.recv().await,
            Some(TransportMessage::Shutdown)
        ));
        assert!(matches!(
            events.recv().await,
            Some(TransportMessage::DeregisterInterface { id: 77 })
        ));
        (&mut task).await.unwrap();
        assert!(handle.tx.is_closed());
    })
    .await;
    if result.is_err() {
        task.abort();
        let _ = task.await;
    }
    result.unwrap();
}

#[tokio::test]
async fn ungated_fin_still_delivers_buffered_complete_frames() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut peer = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (stream, _) = listener.accept().await.unwrap();
    let (reader, _writer) = stream.into_split();
    let wire = hdlc::frame(b"before FIN payload padding~}");
    peer.write_all(&wire).await.unwrap();
    peer.shutdown().await.unwrap();
    let (tx, mut events) = mpsc::channel(8);
    let rxb = Arc::new(AtomicU64::new(0));
    tokio::time::timeout(
        Duration::from_secs(3),
        backbone_read_loop(
            reader,
            7,
            tx,
            Arc::new(AtomicBool::new(true)),
            rxb.clone(),
            IngressControl::new(),
            HW_MTU,
        ),
    )
    .await
    .unwrap();
    let Some(TransportMessage::Inbound(packet)) = events.recv().await else {
        panic!("expected buffered frame")
    };
    assert_eq!(packet.raw.as_ref(), b"before FIN payload padding~}");
    assert_eq!(rxb.load(Ordering::Relaxed), wire.len() as u64);
    assert!(events.recv().await.is_none());
}

#[tokio::test]
async fn gated_peer_keeps_tx_and_other_peer_rx_live() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (tx, mut events) = mpsc::channel(8);
    let mut handles = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        let mut peers = Vec::new();
        for id in [75, 76] {
            let mut config = BackboneClientConfig::new(
                "two-peers",
                "127.0.0.1",
                listener.local_addr().unwrap().port(),
            );
            config.max_reconnect_tries = Some(1);
            handles.push(spawn_backbone_client(config, id, tx.clone()).await.unwrap());
            peers.push(listener.accept().await.unwrap().0);
        }
        while handles
            .iter()
            .any(|handle| !handle.online.load(Ordering::SeqCst))
        {
            tokio::task::yield_now().await;
        }
        handles[0]
            .diagnostics
            .as_ref()
            .unwrap()
            .dataplane_ingress()
            .unwrap()
            .gate(Duration::from_secs(3600));
        peers[0]
            .write_all(&hdlc::frame(b"blocked payload padding"))
            .await
            .unwrap();
        peers[1]
            .write_all(&hdlc::frame(b"active payload padding"))
            .await
            .unwrap();
        let Some(TransportMessage::Inbound(packet)) = events.recv().await else {
            panic!("expected active peer data")
        };
        assert_eq!(
            (packet.interface_id, packet.raw.as_ref()),
            (76, b"active payload padding".as_slice())
        );
        handles[0]
            .tx
            .send(Bytes::from_static(b"outgoing~}"))
            .await
            .unwrap();
        let expected = hdlc::frame(b"outgoing~}");
        let mut received = vec![0; expected.len()];
        peers[0].read_exact(&mut received).await.unwrap();
        assert_eq!(received, expected);
        peers[0].shutdown().await.unwrap();
        assert!(matches!(
            events.recv().await,
            Some(TransportMessage::DeregisterInterface { id: 75 })
        ));
        peers[1]
            .write_all(&hdlc::frame(b"still active payload padding"))
            .await
            .unwrap();
        let Some(TransportMessage::Inbound(packet)) = events.recv().await else {
            panic!("expected surviving peer data")
        };
        assert_eq!(
            (packet.interface_id, packet.raw.as_ref()),
            (76, b"still active payload padding".as_slice())
        );
        assert!(handles[1].online.load(Ordering::SeqCst));
    })
    .await;
    for handle in handles {
        handle.read_task.abort();
        let _ = handle.read_task.await;
    }
    result.unwrap();
}

#[tokio::test]
async fn gated_client_fin_and_reset_disconnect_without_release() {
    for buffered in [false, true] {
        for reset in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut config = BackboneClientConfig::new(
                "gated-close",
                "127.0.0.1",
                listener.local_addr().unwrap().port(),
            );
            config.max_reconnect_tries = Some(1);
            let (tx, mut events) = mpsc::channel(8);
            let handle = spawn_backbone_client(config, 74, tx).await.unwrap();
            let mut task = handle.read_task;
            let result = tokio::time::timeout(Duration::from_secs(3), async {
                let (mut peer, _) = listener.accept().await.unwrap();
                while !handle.online.load(Ordering::SeqCst) {
                    tokio::task::yield_now().await;
                }
                let control = handle
                    .diagnostics
                    .as_ref()
                    .unwrap()
                    .dataplane_ingress()
                    .unwrap();
                control.gate(Duration::from_secs(3600));
                if buffered {
                    peer.write_all(&hdlc::frame(b"must not be delivered~}"))
                        .await
                        .unwrap();
                }
                // Allow the reader to enter its gate, including after an
                // already-started read. No actor is present to release it.
                tokio::time::sleep(Duration::from_millis(100)).await;
                assert!(control.snapshot(74, false).gated);
                assert!(events.try_recv().is_err());
                if reset {
                    socket2::SockRef::from(&peer)
                        .set_linger(Some(Duration::ZERO))
                        .unwrap();
                    drop(peer);
                } else {
                    peer.shutdown().await.unwrap();
                    // Keep the peer's read half alive: FIN alone must suffice.
                }
                assert!(matches!(
                    events.recv().await,
                    Some(TransportMessage::DeregisterInterface { id: 74 })
                ));
                (&mut task).await.unwrap();
                assert!(!handle.online.load(Ordering::SeqCst));
                assert!(handle.tx.is_closed());
                assert!(!control.snapshot(74, false).gated);
                assert_eq!(control.snapshot(74, false).packets, 0);
            })
            .await;
            if result.is_err() {
                task.abort();
                let _ = task.await;
            }
            result.unwrap();
        }
    }
}

#[tokio::test]
async fn gated_close_does_not_consume_kernel_buffered_payload() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut peer = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (stream, _) = listener.accept().await.unwrap();
    let (reader, _writer) = stream.into_split();
    let ingress = IngressControl::new();
    ingress.gate(Duration::from_secs(3600));
    let online = Arc::new(AtomicBool::new(true));
    let rxb = Arc::new(AtomicU64::new(0));
    let (tx, mut events) = mpsc::channel(8);
    let mut task = tokio::spawn(backbone_read_loop(
        reader,
        7,
        tx,
        online.clone(),
        rxb.clone(),
        ingress.clone(),
        HW_MTU,
    ));
    peer.write_all(&hdlc::frame(b"unread payload padding~}"))
        .await
        .unwrap();
    peer.shutdown().await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), &mut task).await;
    if result.is_err() {
        task.abort();
        let _ = task.await;
    }
    result.unwrap().unwrap();
    assert_eq!(rxb.load(Ordering::Relaxed), 0);
    assert!(!online.load(Ordering::SeqCst));
    assert!(!ingress.snapshot(7, false).gated);
    assert!(events.recv().await.is_none());
}

#[tokio::test]
async fn ingress_gate_pauses_reader_and_release_preserves_frames() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut peer = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (stream, _) = listener.accept().await.unwrap();
    let (reader, _writer) = stream.into_split();
    let ingress = IngressControl::new();
    ingress.gate(Duration::from_secs(12));
    let online = Arc::new(AtomicBool::new(true));
    let rxb = Arc::new(AtomicU64::new(0));
    let (tx, mut rx) = mpsc::channel(8);
    let mut task = tokio::spawn(backbone_read_loop(
        reader,
        7,
        tx,
        online.clone(),
        rxb.clone(),
        ingress.clone(),
        HW_MTU,
    ));
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        peer.write_all(&hdlc::frame(b"first payload padding~}"))
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(30), rx.recv())
                .await
                .is_err()
        );
        assert_eq!(rxb.load(Ordering::Relaxed), 0);
        ingress.release();
        let Some(TransportMessage::Inbound(packet)) = rx.recv().await else {
            panic!("expected frame")
        };
        assert_eq!(packet.raw.as_ref(), b"first payload padding~}");
        ingress.gate(Duration::from_secs(24));
        peer.write_all(&hdlc::frame(b"second payload padding"))
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(30), rx.recv())
                .await
                .is_err()
        );
        ingress.release();
        let Some(TransportMessage::Inbound(packet)) = rx.recv().await else {
            panic!("expected frame")
        };
        assert_eq!(packet.raw.as_ref(), b"second payload padding");
        assert_eq!(ingress.snapshot(7, false).packets, 2);
        drop(peer);
        (&mut task).await.unwrap();
        assert!(!online.load(Ordering::SeqCst));
        assert_eq!(ingress.snapshot(7, false).packets, 0);
    })
    .await;
    if result.is_err() {
        task.abort();
        let _ = task.await;
    }
    result.unwrap();
}

#[tokio::test(start_paused = true)]
async fn abort_releases_all_reservations_and_gate() {
    let (tx, rx, accounting) = byte_channel(4, HIGH_WATERMARK, encoded_len);
    let (sink, _reader) = tokio::io::duplex(1);
    tx.try_send(Bytes::from(vec![42; 200_000])).unwrap();
    tx.try_send(Bytes::from_static(b"queued")).unwrap();
    let task = tokio::spawn(controlled_write_loop(
        sink,
        rx,
        Arc::new(AtomicBool::new(true)),
        Arc::new(AtomicU64::new(0)),
        accounting.clone(),
    ));
    tokio::task::yield_now().await;
    tokio::time::advance(EVALUATE_INTERVAL).await;
    tokio::task::yield_now().await;
    assert!(accounting.snapshot().gated);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(accounting.snapshot().buffered, 0);
    assert!(!accounting.snapshot().gated);
    assert!(tx.is_closed());
}

#[tokio::test]
async fn reconnect_keeps_accounting_and_accepts_new_output() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = BackboneClientConfig::new(
        "managed-reconnect",
        "127.0.0.1",
        listener.local_addr().unwrap().port(),
    );
    let (transport_tx, _events) = mpsc::channel(8);
    let handle = spawn_backbone_client(config, 73, transport_tx)
        .await
        .unwrap();
    let accounting = handle.tx.accounting().unwrap();
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        for payload in [b"first".as_slice(), b"second~}".as_slice()] {
            let (mut peer, _) = listener.accept().await.unwrap();
            handle.tx.try_send(Bytes::copy_from_slice(payload)).unwrap();
            let expected = hdlc::frame(payload);
            let mut received = vec![0; expected.len()];
            peer.read_exact(&mut received).await.unwrap();
            assert_eq!(received, expected);
            assert_eq!(accounting.snapshot().buffered, 0);
            assert!(!accounting.snapshot().gated);
            drop(peer);
        }
    })
    .await;
    handle.read_task.abort();
    let _ = handle.read_task.await;
    assert!(
        handle.tx.is_closed(),
        "abort must not orphan the forwarding receiver"
    );
    assert_eq!(accounting.snapshot().buffered, 0);
    result.unwrap();
}

#[tokio::test(start_paused = true)]
async fn managed_backlog_gates_recovers_and_counts_partial_writes() {
    let (tx, rx, accounting) = byte_channel(4, HIGH_WATERMARK, encoded_len);
    let (sink, mut reader) = tokio::io::duplex(7);
    let count = Arc::new(AtomicU64::new(0));
    let task = tokio::spawn(controlled_write_loop(
        sink,
        rx,
        Arc::new(AtomicBool::new(true)),
        count.clone(),
        accounting.clone(),
    ));
    let payload = Bytes::from(vec![hdlc::FLAG; 200_000]);
    tx.try_send(payload.clone()).unwrap();
    tokio::task::yield_now().await;
    assert_eq!(accounting.snapshot().buffered, encoded_len(&payload) - 7);
    assert_eq!(accounting.snapshot().sent, 7);
    tokio::time::advance(EVALUATE_INTERVAL).await;
    tokio::task::yield_now().await;
    assert!(accounting.snapshot().gated);
    assert!(tx.try_send(Bytes::new()).is_err());
    let mut actual = vec![0; encoded_len(&payload) as usize];
    reader.read_exact(&mut actual).await.unwrap();
    assert_eq!(actual, hdlc::frame(&payload));
    assert_eq!(accounting.snapshot().buffered, 0);
    assert_eq!(accounting.snapshot().sent, count.load(Ordering::Relaxed));
    tokio::time::advance(EVALUATE_INTERVAL).await;
    tokio::task::yield_now().await;
    assert!(!accounting.snapshot().gated);
    tx.try_send(Bytes::new()).unwrap();
    drop(tx);
    let mut tail = Vec::new();
    reader.read_to_end(&mut tail).await.unwrap();
    task.await.unwrap();
    assert_eq!(tail, hdlc::frame(&[]));
    assert_eq!(accounting.snapshot().buffered, 0);
}

#[tokio::test]
async fn managed_error_releases_queued_and_encoded_reservations() {
    let (tx, rx, accounting) = byte_channel(4, HIGH_WATERMARK, encoded_len);
    for _ in 0..3 {
        tx.try_send(Bytes::from(vec![hdlc::ESC; 100_000])).unwrap();
    }
    let mut sink = writer(3);
    sink.fail_after = Some(17);
    backbone_write_loop(
        sink,
        rx,
        Arc::new(AtomicBool::new(true)),
        Arc::new(AtomicU64::new(0)),
    )
    .await;
    assert_eq!(accounting.snapshot().buffered, 0);
    assert_eq!(accounting.snapshot().sent, 17);
    assert!(tx.is_closed());
}

#[tokio::test]
#[ignore = "real loopback no-drain deadline; takes at least 12 seconds"]
async fn client_stalled_socket_closes_and_deregisters() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = BackboneClientConfig::new(
        "stalled-client",
        "127.0.0.1",
        listener.local_addr().unwrap().port(),
    );
    config.max_reconnect_tries = Some(1);
    let (transport_tx, mut events) = mpsc::channel(8);
    let handle = spawn_backbone_client(config, 72, transport_tx)
        .await
        .unwrap();
    let mut task = handle.read_task;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        let (mut peer, _) = listener.accept().await.unwrap();
        socket2::SockRef::from(&peer)
            .set_recv_buffer_size(4096)
            .unwrap();
        let payload = Bytes::from(vec![42; HW_MTU as usize - 2]);
        let mut rejected = 0;
        for _ in 0..32 {
            if handle.tx.try_send(payload.clone()).is_err() {
                rejected += 1;
            }
        }
        assert!(
            rejected > 0,
            "full encoded-byte quota rejects excess frames"
        );
        // Do not read or close either peer half. Only the TX no-progress
        // deadline can end the connection; leave the sender alive too.
        assert!(matches!(
            events.recv().await,
            Some(TransportMessage::DeregisterInterface { id: 72 })
        ));
        (&mut task).await.unwrap();
        assert!(!handle.online.load(Ordering::SeqCst));
        assert!(handle.tx.is_closed());
        let sent = handle.txb.unwrap().load(Ordering::Relaxed);
        assert!(sent > 0 && sent < 32 * (u64::from(HW_MTU) + 2));
        // Kernel-accepted bytes may still drain after writer termination.
        socket2::SockRef::from(&peer)
            .set_recv_buffer_size(4 * 1024 * 1024)
            .unwrap();
        let mut received = Vec::new();
        peer.read_to_end(&mut received).await.unwrap();
        assert_eq!(received.len() as u64, sent);
        let frame = hdlc::frame(&payload);
        for (index, byte) in received.into_iter().enumerate() {
            assert_eq!(byte, frame[index % frame.len()]);
        }
    })
    .await;
    if result.is_err() {
        task.abort();
        let _ = task.await;
    }
    result.unwrap();
}

#[tokio::test]
async fn client_writer_completion_closes_silent_peer_and_deregisters() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = BackboneClientConfig::new(
        "writer-completion",
        "127.0.0.1",
        listener.local_addr().unwrap().port(),
    );
    config.max_reconnect_tries = Some(1);
    let (transport_tx, mut events) = mpsc::channel(8);
    let handle = spawn_backbone_client(config, 71, transport_tx)
        .await
        .unwrap();
    let mut task = handle.read_task;
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        let (mut peer, _) = listener.accept().await.unwrap();
        let payload = Bytes::from_static(b"last~}packet");
        handle.tx.send(payload.clone()).await.unwrap();
        drop(handle.tx);
        // Peer keeps its sending half open: reader cannot drive disconnect.
        let mut received = Vec::new();
        peer.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, hdlc::frame(&payload));
        assert!(matches!(
            events.recv().await,
            Some(TransportMessage::DeregisterInterface { id: 71 })
        ));
        (&mut task).await.unwrap();
        assert!(!handle.online.load(Ordering::SeqCst));
        assert_eq!(
            handle.txb.unwrap().load(Ordering::Relaxed),
            received.len() as u64
        );
    })
    .await;
    if result.is_err() {
        task.abort();
        let _ = task.await;
    }
    result.unwrap();
}

struct Writer {
    bytes: Arc<std::sync::Mutex<Vec<u8>>>,
    calls: Arc<AtomicU64>,
    max_write: usize,
    fail_after: Option<usize>,
    interrupt_once: bool,
    interrupt_forever: bool,
    write_zero: bool,
}

impl AsyncWrite for Writer {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.interrupt_once || self.interrupt_forever {
            self.interrupt_once = false;
            return Poll::Ready(Err(io::ErrorKind::Interrupted.into()));
        }
        let mut bytes = self.bytes.lock().unwrap();
        let remaining = self
            .fail_after
            .unwrap_or(usize::MAX)
            .saturating_sub(bytes.len());
        if remaining == 0 {
            return Poll::Ready(if self.write_zero {
                Ok(0)
            } else {
                Err(io::ErrorKind::BrokenPipe.into())
            });
        }
        self.calls.fetch_add(1, Ordering::Relaxed);
        let n = data.len().min(self.max_write).min(remaining);
        bytes.extend_from_slice(&data[..n]);
        Poll::Ready(Ok(n))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

fn writer(max_write: usize) -> Writer {
    Writer {
        bytes: Default::default(),
        calls: Default::default(),
        max_write,
        fail_after: None,
        interrupt_once: false,
        interrupt_forever: false,
        write_zero: false,
    }
}

#[tokio::test(start_paused = true)]
async fn interruptions_neither_reset_deadline_nor_starve_other_tasks() {
    let (tx, rx) = mpsc::channel(1);
    tx.send(Bytes::from_static(b"packet")).await.unwrap();
    let mut sink = writer(1);
    sink.interrupt_forever = true;
    let count = Arc::new(AtomicU64::new(0));
    let online = Arc::new(AtomicBool::new(true));
    let task = tokio::spawn(backbone_write_loop(sink, rx, online.clone(), count.clone()));
    tokio::task::yield_now().await;
    tokio::time::advance(TX_DEAD_TIME).await;
    task.await.unwrap();
    assert!(!online.load(Ordering::SeqCst));
    assert_eq!(count.load(Ordering::Relaxed), 0);
}

#[tokio::test(start_paused = true)]
async fn stalled_output_expires_but_idle_connection_survives() {
    let (tx, rx) = mpsc::channel(2);
    let (sink, mut reader) = tokio::io::duplex(1);
    let count = Arc::new(AtomicU64::new(0));
    let online = Arc::new(AtomicBool::new(true));
    let task = tokio::spawn(backbone_write_loop(sink, rx, online.clone(), count.clone()));
    tokio::task::yield_now().await;
    tokio::time::advance(TX_DEAD_TIME * 3).await;
    assert!(!task.is_finished(), "idle is not stalled");
    tx.send(Bytes::from_static(b"packet")).await.unwrap();
    tokio::task::yield_now().await;
    assert_eq!(count.load(Ordering::Relaxed), 1);
    tokio::time::advance(TX_DEAD_TIME - Duration::from_millis(1)).await;
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    tokio::time::advance(Duration::from_millis(1)).await;
    task.await.unwrap();
    assert!(!online.load(Ordering::SeqCst));
    assert_eq!(count.load(Ordering::Relaxed), 1);
    assert!(tx.send(Bytes::from_static(b"late")).await.is_err());
    let mut received = Vec::new();
    reader.read_to_end(&mut received).await.unwrap();
    assert_eq!(received, [hdlc::FLAG]);
}

#[tokio::test(start_paused = true)]
async fn partial_progress_renews_deadline_and_slow_reader_can_finish() {
    let (tx, rx) = mpsc::channel(1);
    let (sink, mut reader) = tokio::io::duplex(1);
    let count = Arc::new(AtomicU64::new(0));
    let online = Arc::new(AtomicBool::new(true));
    let task = tokio::spawn(backbone_write_loop(sink, rx, online.clone(), count.clone()));
    tx.send(Bytes::from_static(b"abc")).await.unwrap();
    let expected = hdlc::frame(b"abc");
    let mut received = Vec::new();
    for index in 0..expected.len() {
        tokio::task::yield_now().await;
        assert_eq!(count.load(Ordering::Relaxed), (index + 1) as u64);
        tokio::time::advance(Duration::from_secs(11)).await;
        assert!(!task.is_finished());
        received.push(reader.read_u8().await.unwrap());
    }
    assert_eq!(received, expected);
    assert!(online.load(Ordering::SeqCst));
    drop(tx);
    task.await.unwrap();
}

#[test]
fn encoded_chunks_are_bounded_and_match_legacy_hdlc() {
    for payload in [
        vec![],
        vec![0x7E, 0x7D, 0, 255],
        vec![0; 65534],
        vec![0x7E; 65536],
        vec![0x7D; 1_048_576],
    ] {
        let expected = hdlc::frame(&payload);
        let mut cursor = TxFrame::new(Bytes::from(payload));
        let mut actual = Vec::new();
        loop {
            let mut chunk = Vec::with_capacity(TX_COALESCE_TARGET);
            let complete = cursor.append(&mut chunk);
            assert!(!chunk.is_empty());
            assert!(chunk.len() <= TX_COALESCE_TARGET);
            actual.extend(chunk);
            if complete {
                break;
            }
        }
        assert_eq!(actual, expected);
    }
}

#[tokio::test]
async fn ready_frames_coalesce_without_changing_wire_bytes() {
    let (tx, rx) = mpsc::channel::<Bytes>(128);
    let mut expected = Vec::new();
    for index in 0..128u8 {
        let payload = vec![index, hdlc::FLAG, hdlc::ESC];
        expected.extend(hdlc::frame(&payload));
        tx.try_send(payload.into()).unwrap();
    }
    drop(tx);
    let sink = writer(usize::MAX);
    let bytes = sink.bytes.clone();
    let calls = sink.calls.clone();
    let online = Arc::new(AtomicBool::new(true));
    let count = Arc::new(AtomicU64::new(0));
    backbone_write_loop(sink, rx, online.clone(), count.clone()).await;
    assert_eq!(*bytes.lock().unwrap(), expected);
    assert_eq!(count.load(Ordering::Relaxed), expected.len() as u64);
    assert_eq!(
        calls.load(Ordering::Relaxed),
        2,
        "128 ready frames in two bounded batches"
    );
    assert!(!online.load(Ordering::SeqCst));
}

#[tokio::test]
async fn fragmented_writes_preserve_large_and_small_frame_order() {
    let (tx, rx) = mpsc::channel::<Bytes>(4);
    let mut expected = Vec::new();
    for payload in [
        vec![hdlc::FLAG; 100000],
        vec![],
        vec![hdlc::ESC; 65536],
        vec![42; 500],
    ] {
        expected.extend(hdlc::frame(&payload));
        tx.try_send(payload.into()).unwrap();
    }
    drop(tx);
    let sink = writer(137);
    let bytes = sink.bytes.clone();
    let count = Arc::new(AtomicU64::new(0));
    backbone_write_loop(sink, rx, Arc::new(AtomicBool::new(true)), count.clone()).await;
    assert_eq!(*bytes.lock().unwrap(), expected);
    assert_eq!(count.load(Ordering::Relaxed), expected.len() as u64);
}

#[tokio::test]
#[ignore = "local in-memory batching comparison, not a network throughput benchmark"]
async fn compare_coalesced_and_legacy_writes() {
    let payload = Bytes::from(vec![0x55; 500]);
    let mut legacy = writer(usize::MAX);
    let start = std::time::Instant::now();
    for _ in 0..8192 {
        legacy.write_all(&hdlc::frame(&payload)).await.unwrap();
    }
    let legacy_elapsed = start.elapsed();
    let (tx, rx) = mpsc::channel(8192);
    for _ in 0..8192 {
        tx.try_send(payload.clone()).unwrap();
    }
    drop(tx);
    let sink = writer(usize::MAX);
    let bytes = sink.bytes.clone();
    let calls = sink.calls.clone();
    let start = std::time::Instant::now();
    backbone_write_loop(
        sink,
        rx,
        Arc::new(AtomicBool::new(true)),
        Arc::new(AtomicU64::new(0)),
    )
    .await;
    let elapsed = start.elapsed();
    assert_eq!(*bytes.lock().unwrap(), *legacy.bytes.lock().unwrap());
    assert_eq!(calls.load(Ordering::Relaxed), 128);
    eprintln!(
        "8192 x 500-byte frames: legacy {:?}, {} writes; coalesced {:?}, {} writes; in-memory sink only",
        legacy_elapsed,
        legacy.calls.load(Ordering::Relaxed),
        elapsed,
        calls.load(Ordering::Relaxed)
    );
}

/// Run alone with --ignored --exact --nocapture for meaningful process RSS.
/// This measures the current driver; it is not a historical before/after result.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "local two-peer TCP load measurement; holds one reader until egress gates"]
async fn measure_two_peer_tcp_isolation_and_recovery() {
    struct Tasks(Vec<tokio::task::JoinHandle<()>>);
    impl Drop for Tasks {
        fn drop(&mut self) {
            for task in &self.0 {
                task.abort();
            }
        }
    }
    fn rss_kib() -> Option<u64> {
        std::fs::read_to_string("/proc/self/status")
            .ok()?
            .lines()
            .find(|line| line.starts_with("VmRSS:"))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }
    async fn drain(
        peer: &mut TcpStream,
        sequences: &[u64],
        size: usize,
        origin: std::time::Instant,
    ) -> Vec<u128> {
        let mut decoder = hdlc::HdlcDeframer::with_max_decoded_size(size);
        let mut received = 0;
        let mut latency = Vec::with_capacity(sequences.len());
        let mut buffer = [0; 65536];
        while received < sequences.len() {
            let count = peer.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0, "peer closed before accepted frames drained");
            for frame in decoder.feed(&buffer[..count]) {
                assert!(received < sequences.len(), "unexpected extra frame");
                assert_eq!(frame.len(), size);
                assert_eq!(
                    u64::from_be_bytes(frame[..8].try_into().unwrap()),
                    sequences[received]
                );
                assert!(frame[16..].iter().all(|byte| *byte == 0x55));
                let sent_ns = u64::from_be_bytes(frame[8..16].try_into().unwrap());
                latency.push(
                    origin
                        .elapsed()
                        .as_nanos()
                        .saturating_sub(u128::from(sent_ns)),
                );
                received += 1;
            }
        }
        latency
    }
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut tasks = Tasks(Vec::new());
        let mut handles = Vec::new();
        let mut peers = Vec::new();
        let (transport_tx, _events) = mpsc::channel(16);
        for id in 1..=2 {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut config = BackboneClientConfig::new("two-peer-load", "127.0.0.1", listener.local_addr().unwrap().port());
            config.max_reconnect_tries = Some(1);
            config.receive_ifac_size = Some(0);
            let handle = spawn_backbone_client(config, id, transport_tx.clone()).await.unwrap();
            let (peer, _) = listener.accept().await.unwrap();
            while !handle.online.load(Ordering::SeqCst) { tokio::task::yield_now().await; }
            tasks.0.push(handle.read_task);
            handles.push((handle.tx, handle.online));
            peers.push(peer);
        }
        let mut slow = peers.remove(0);
        let mut fast = peers.remove(0);
        socket2::SockRef::from(&slow).set_recv_buffer_size(4096).unwrap();
        let slow_accounting = handles[0].0.accounting().unwrap();
        let fast_accounting = handles[1].0.accounting().unwrap();
        let origin = std::time::Instant::now();
        let rss_start = rss_kib();
        let mut accepted = Vec::new();
        let mut rejected = 0u64;
        let mut high_buffered = 0;
        const SLOW_SIZE: usize = 16384;
        const ATTEMPTS: u64 = 4096;
        for sequence in 0..ATTEMPTS {
            let mut payload = vec![0x55; SLOW_SIZE];
            payload[..8].copy_from_slice(&sequence.to_be_bytes());
            payload[8..16].copy_from_slice(&(origin.elapsed().as_nanos() as u64).to_be_bytes());
            match handles[0].0.try_send(payload.into()) {
                Ok(()) => accepted.push(sequence),
                Err(mpsc::error::TrySendError::Full(_)) => rejected += 1,
                Err(error) => panic!("slow peer disconnected: {error}"),
            }
            let snapshot = slow_accounting.snapshot();
            high_buffered = high_buffered.max(snapshot.buffered);
            assert!(snapshot.buffered <= HIGH_WATERMARK);
        }
        assert!(rejected > 0 && !accepted.is_empty());
        tokio::time::timeout(Duration::from_secs(6), async {
            while !slow_accounting.snapshot().gated {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.expect("stopped reader did not trigger egress gate");
        assert!(slow_accounting.snapshot().buffered > 0);
        let rss_gated = rss_kib();
        const FAST_SIZE: usize = 4096;
        const FAST_COUNT: u64 = 256;
        let fast_start = std::time::Instant::now();
        for sequence in 0..FAST_COUNT {
            let mut payload = vec![0x55; FAST_SIZE];
            payload[..8].copy_from_slice(&sequence.to_be_bytes());
            payload[8..16].copy_from_slice(&(origin.elapsed().as_nanos() as u64).to_be_bytes());
            handles[1].0.try_send(payload.into()).unwrap();
        }
        let mut latency = tokio::time::timeout(Duration::from_secs(3),
            drain(&mut fast, &(0..FAST_COUNT).collect::<Vec<_>>(), FAST_SIZE, origin)).await.unwrap();
        let fast_elapsed = fast_start.elapsed();
        assert!(slow_accounting.snapshot().gated, "slow peer remains unread");
        assert_eq!(fast_accounting.snapshot().dropped_frames, 0);
        assert!(handles.iter().all(|(_, online)| online.load(Ordering::SeqCst)));
        // Resume the slow peer and require every admitted frame, in order.
        socket2::SockRef::from(&slow).set_recv_buffer_size(4 * 1024 * 1024).unwrap();
        let recovery_start = std::time::Instant::now();
        let slow_latency = drain(&mut slow, &accepted, SLOW_SIZE, origin).await;
        let recovery = recovery_start.elapsed();
        tokio::time::timeout(Duration::from_secs(3), async {
            while slow_accounting.snapshot().gated || slow_accounting.snapshot().buffered != 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.expect("egress gate did not release after drain");
        let slow_snapshot = slow_accounting.snapshot();
        assert_eq!(slow_snapshot.dropped_frames, rejected);
        assert_eq!(accepted.len() as u64 + rejected, ATTEMPTS);
        let mut probe = vec![0x55; SLOW_SIZE];
        probe[..8].copy_from_slice(&ATTEMPTS.to_be_bytes());
        probe[8..16].copy_from_slice(&(origin.elapsed().as_nanos() as u64).to_be_bytes());
        handles[0].0.try_send(probe.into()).expect("admission must resume after gate release");
        drain(&mut slow, &[ATTEMPTS], SLOW_SIZE, origin).await;
        assert_eq!(slow_accounting.snapshot().dropped_frames, rejected);
        latency.sort_unstable();
        eprintln!("two_peer_tcp: slow_attempts={ATTEMPTS} slow_accepted={} slow_dropped={rejected} slow_high_buffered={high_buffered} limit={HIGH_WATERMARK} slow_recovery_ms={:.3} slow_max_latency_ms={:.3} fast_frames={FAST_COUNT} fast_payload_MiB_s={:.3} fast_p50_ms={:.3} fast_p99_ms={:.3} fast_dropped=0 rss_start_kib={rss_start:?} rss_gated_kib={rss_gated:?} rss_end_kib={:?}; RSS checkpoints are process-wide, not peak or a memory bound", accepted.len(), recovery.as_secs_f64()*1000.0, *slow_latency.iter().max().unwrap() as f64/1e6, (FAST_COUNT as f64*FAST_SIZE as f64/1048576.0)/fast_elapsed.as_secs_f64(), latency[latency.len()/2] as f64/1e6, latency[(latency.len()-1)*99/100] as f64/1e6, rss_kib());
    }).await.expect("two-peer load test timed out");
}

#[tokio::test]
async fn partial_writes_errors_and_zero_count_only_accepted_bytes() {
    for zero in [false, true] {
        let (tx, rx) = mpsc::channel::<Bytes>(2);
        let payload = vec![hdlc::FLAG; 100];
        tx.try_send(payload.clone().into()).unwrap();
        drop(tx);
        let mut sink = writer(3);
        sink.interrupt_once = true;
        sink.fail_after = Some(17);
        sink.write_zero = zero;
        let bytes = sink.bytes.clone();
        let online = Arc::new(AtomicBool::new(true));
        let count = Arc::new(AtomicU64::new(0));
        backbone_write_loop(sink, rx, online.clone(), count.clone()).await;
        assert_eq!(*bytes.lock().unwrap(), hdlc::frame(&payload)[..17]);
        assert_eq!(count.load(Ordering::Relaxed), 17);
        assert!(!online.load(Ordering::SeqCst));
    }
}

#[tokio::test]
async fn stopped_reader_resumes_and_sparse_frames_do_not_wait_for_batch() {
    let (tx, rx) = mpsc::channel(2);
    let (sink, mut reader) = tokio::io::duplex(7);
    let count = Arc::new(AtomicU64::new(0));
    let online = Arc::new(AtomicBool::new(true));
    let task = tokio::spawn(backbone_write_loop(sink, rx, online.clone(), count.clone()));
    let first = Bytes::from_static(b"first~}packet");
    tx.send(first.clone()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while count.load(Ordering::Relaxed) < 7 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        count.load(Ordering::Relaxed),
        7,
        "blocked writer does not pre-count the frame"
    );
    let mut received = vec![0; hdlc::frame(&first).len()];
    tokio::time::timeout(Duration::from_secs(2), reader.read_exact(&mut received))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received, hdlc::frame(&first));
    tx.send(Bytes::from_static(b"second")).await.unwrap();
    drop(tx);
    let mut tail = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_to_end(&mut tail))
        .await
        .unwrap()
        .unwrap();
    task.await.unwrap();
    assert_eq!(tail, hdlc::frame(b"second"));
    assert_eq!(
        count.load(Ordering::Relaxed),
        (received.len() + tail.len()) as u64
    );
    assert!(!online.load(Ordering::SeqCst));
}
