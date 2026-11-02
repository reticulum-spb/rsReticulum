//! Real outbound TCP client receives signed wire packets from a loopback peer.
//! No routing state is injected. Ingress throttling is disabled (a supported
//! deployment setting) so memory admission must stand on its own.
use rns_identity::{announce::AnnounceData, destination::Destination, identity::Identity};
use rns_transport::{actor::TransportActor, constants::*, messages::*};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signed_announces_over_tcp_have_bounded_routing_state() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut actor, tx) = TransportActor::new();
    actor.is_transport_enabled = true;
    let handle = rns_interface::tcp::spawn_tcp_client(
        rns_interface::tcp::TcpClientConfig::new(
            "external-peer",
            "127.0.0.1",
            listener.local_addr().unwrap().port(),
        ),
        1,
        tx.clone(),
    )
    .await
    .unwrap();
    let mut entry = InterfaceEntry::new(
        handle.name,
        InterfaceMode::Full,
        InterfaceDirection {
            inbound: true,
            outbound: true,
        },
        handle.bitrate,
        handle.mtu,
        handle.tx,
    );
    entry.ingress = rns_transport::ingress::IngressController::disabled();
    tx.send(TransportMessage::RegisterInterface { id: 1, entry })
        .await
        .unwrap();
    let actor_task = tokio::spawn(actor.run());
    let (peer, _) = listener.accept().await.unwrap();
    let (mut reader, mut writer) = peer.into_split();
    let drain = tokio::spawn(async move {
        let mut b = [0; 8192];
        while let Ok(n) = reader.read(&mut b).await {
            if n == 0 {
                break;
            }
        }
    });
    let identity = Identity::new();
    for i in 0..8400 {
        let name = format!("memory.route{i}");
        let dest = Destination::hash_from_name_and_identity(&name, Some(&identity.hash));
        let announce = AnnounceData::create(&identity, &name, None, None).unwrap();
        let mut raw = vec![0x01, 0]; // HEADER_1, SINGLE, ANNOUNCE
        raw.extend_from_slice(&dest);
        raw.push(0);
        raw.extend_from_slice(&announce.pack());
        writer
            .write_all(&rns_interface::hdlc::frame(&raw))
            .await
            .unwrap();
        // Pace below socket/channel admission limits; this tests retained state.
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    let (response_tx, response_rx) = oneshot::channel();
    tx.send(TransportMessage::Rpc {
        query: TransportQuery::GetPathTable,
        response_tx,
    })
    .await
    .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(10), response_rx)
        .await
        .unwrap()
        .unwrap();
    handle.read_task.abort();
    actor_task.abort();
    drain.abort();
    match result {
        TransportQueryResponse::PathTable(paths) => {
            eprintln!(
                "retained paths after 8400 signed network announces: {}",
                paths.len()
            );
            assert!(paths.len() >= 8000, "fixture lost too much traffic");
            assert!(
                paths.len() <= 8192,
                "unbounded route admission: {}",
                paths.len()
            );
        }
        _ => panic!("unexpected response"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stalled_actor_has_a_byte_bound_on_network_ingress() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
    let mut config = rns_interface::tcp::TcpClientConfig::new(
        "large-frames",
        "127.0.0.1",
        listener.local_addr().unwrap().port(),
    );
    config.fixed_mtu = Some(16384);
    let handle = rns_interface::tcp::spawn_tcp_client(config, 9, tx)
        .await
        .unwrap();
    let (mut peer, _) = listener.accept().await.unwrap();
    let frame = rns_interface::hdlc::frame(&vec![0x42; 16384]);
    for _ in 0..1600 {
        peer.write_all(&frame).await.unwrap();
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    let mut bytes = 0;
    while let Ok(TransportMessage::Inbound(packet)) = rx.try_recv() {
        bytes += packet.raw.len();
    }
    handle.read_task.abort();
    eprintln!("queued network payload bytes with stalled actor: {bytes}");
    assert!(bytes >= 1024 * 1024, "fixture received too little traffic");
    assert!(
        bytes <= 8 * 1024 * 1024,
        "packet-count limit permits excessive byte retention"
    );
}
