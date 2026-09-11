//! Real TCP/HDLC -> transport actor -> runtime LinkManager MTU regression.
//! Peers use the Rust Link library or (in the ignored interop test) Python Link,
//! not a second full runtime/daemon.
use rns_crypto::ed25519::Ed25519PrivateKey;
use rns_interface::{hdlc, tcp};
use rns_link::link::Link;
use rns_runtime::link_manager::LinkManager;
use rns_transport::actor::TransportActor;
use rns_transport::constants::{InterfaceDirection, InterfaceMode};
use rns_transport::link_messages::DestinationEvent;
use rns_transport::messages::InterfaceEntry;
use rns_transport::messages::{
    OutboundRequest, TransportMessage, TransportQuery, TransportQueryResponse,
};
use rns_wire::context::PacketContext;
use rns_wire::flags::{DestinationType, HeaderType, PacketFlags, PacketType, TransportType};
use rns_wire::header::PacketHeader;
use std::collections::VecDeque;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

struct Tasks(Vec<tokio::task::JoinHandle<()>>);
impl Drop for Tasks {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

fn packet(dest: [u8; 16], kind: PacketType, context: PacketContext, data: &[u8]) -> Vec<u8> {
    let mut raw = PacketHeader {
        flags: PacketFlags {
            header_type: HeaderType::Header1,
            context_flag: false,
            transport_type: TransportType::Broadcast,
            destination_type: if kind == PacketType::LinkRequest {
                DestinationType::Single
            } else {
                DestinationType::Link
            },
            packet_type: kind,
        },
        hops: 0,
        transport_id: None,
        destination_hash: dest,
        context,
    }
    .pack()
    .unwrap();
    raw.extend_from_slice(data);
    raw
}

struct Peer {
    stream: TcpStream,
    decoder: hdlc::HdlcDeframer,
    pending: VecDeque<Vec<u8>>,
}
impl Peer {
    async fn send(&mut self, raw: &[u8]) {
        // Force framing to tolerate arbitrary TCP read boundaries.
        for chunk in hdlc::frame(raw).chunks(997) {
            self.stream.write_all(chunk).await.unwrap();
        }
    }

    async fn receive(&mut self) -> Vec<u8> {
        loop {
            if let Some(frame) = self.pending.pop_front() {
                return frame;
            }
            let mut buffer = [0; 8192];
            let count = self.stream.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0, "TCP peer closed before packet delivery");
            self.pending.extend(self.decoder.feed(&buffer[..count]));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_responder_large_mtu_tcp_roundtrip_and_oversize_recovery() {
    timeout(Duration::from_secs(30), async {
        for (offer, cap, expected) in [
            (32768, 500, 500),
            (32768, 1196, 1196),
            (524288, 262144, 262144),
            (1196, 262144, 1196),
        ] {
            exercise(offer, cap, expected).await;
        }
    })
    .await
    .expect("TCP Link MTU regression timed out");
}

async fn exercise(offer: u32, cap: u32, expected: u32) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut actor, input) = TransportActor::new();
    let mut tasks = Tasks(Vec::new());
    let mut config = tcp::TcpClientConfig::new(
        "mtu-test",
        "127.0.0.1",
        listener.local_addr().unwrap().port(),
    );
    config.fixed_mtu = Some(cap);
    config.receive_ifac_size = Some(0);
    config.max_reconnect_tries = Some(1);
    let handle = tcp::spawn_tcp_client(config, 1, input.clone())
        .await
        .unwrap();
    tasks.0.push(handle.read_task);
    let (stream, _) = listener.accept().await.unwrap();
    let mut peer = Peer {
        stream,
        decoder: hdlc::HdlcDeframer::with_max_decoded_size(cap as usize),
        pending: VecDeque::new(),
    };
    let mut entry = InterfaceEntry::new(
        handle.name,
        InterfaceMode::Full,
        InterfaceDirection::bidirectional(),
        handle.bitrate,
        handle.mtu,
        handle.tx,
    );
    entry.online = Some(handle.online);
    entry.diagnostics = handle.diagnostics;
    actor.interfaces.insert(1, entry);
    let dest = [0xAD; 16];
    let (event_tx, event_rx) = mpsc::channel(16);
    actor.local_destinations.insert(dest);
    actor.destination_channels.insert(dest, event_tx);
    tasks.0.push(tokio::spawn(actor.run()));
    let key = Ed25519PrivateKey::generate();
    let public = key.public_key();
    let mut manager = LinkManager::new(input, event_rx, dest, Some(key));
    let (delivery_tx, mut delivery_rx) = mpsc::channel(4);
    manager.set_link_packet_channel(delivery_tx);

    let (mut initiator, request) = Link::new_initiator_with_mtu(dest, 1, offer);
    peer.send(&packet(
        dest,
        PacketType::LinkRequest,
        PacketContext::None,
        &request,
    ))
    .await;
    assert!(manager.step().await);
    let proof = peer.receive().await;
    let (header, offset) = PacketHeader::unpack(&proof).unwrap();
    assert_eq!(header.context, PacketContext::Lrproof);
    assert_eq!(header.destination_hash, initiator.link_id);
    let rtt = initiator
        .validate_proof(&proof[offset..], &public, &public.to_bytes())
        .unwrap();
    assert_eq!(initiator.mtu, expected);
    peer.send(&packet(
        initiator.link_id,
        PacketType::Data,
        PacketContext::Lrrtt,
        &rtt,
    ))
    .await;
    assert!(manager.step().await);
    let responder = manager.get_link(&initiator.link_id).unwrap();
    assert_eq!(responder.mtu, expected);
    assert_eq!(responder.mdu, initiator.mdu);

    // An oversized decoded frame must be discarded, not admitted as application
    // data or leave the deframer unable to accept the following valid packet.
    peer.send(&vec![0x7e; cap as usize + 1]).await;
    let oversized = packet(
        initiator.link_id,
        PacketType::Data,
        PacketContext::None,
        &initiator.encrypt(&vec![0x55; cap as usize]).unwrap(),
    );
    assert!(oversized.len() > cap as usize);
    peer.send(&oversized).await;
    for length in [1, initiator.mdu] {
        let payload: Vec<u8> = (0..length).map(|i| (i % 256) as u8).collect();
        let encrypted = initiator.encrypt(&payload).unwrap();
        let raw = packet(
            initiator.link_id,
            PacketType::Data,
            PacketContext::None,
            &encrypted,
        );
        assert!(raw.len() <= expected as usize);
        peer.send(&raw).await;
        assert!(manager.step().await);
        let (received, link_id) = delivery_rx.recv().await.unwrap();
        assert_eq!(link_id, initiator.link_id);
        assert_eq!(received, payload);
        manager.send_link_packet(&link_id, &payload).unwrap();
        loop {
            let reply = peer.receive().await;
            let (header, offset) = PacketHeader::unpack(&reply).unwrap();
            if header.destination_hash != link_id {
                continue; // Actor may also broadcast control/path-request traffic.
            }
            if header.flags.packet_type == PacketType::Proof {
                continue; // LinkManager also proves incoming application packets.
            }
            assert_eq!(header.flags.packet_type, PacketType::Data);
            assert_eq!(header.context, PacketContext::None);
            assert!(reply.len() <= expected as usize);
            assert_eq!(initiator.decrypt(&reply[offset..]).unwrap(), payload);
            break;
        }
    }
    assert!(delivery_rx.try_recv().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Python reference and local TCP sockets"]
async fn python_initiator_large_mtu_tcp_roundtrip() {
    timeout(Duration::from_secs(45), async {
        for (offer, cap, expected) in [
            (32768, 500, 500),
            (32768, 1196, 1196),
            (524288, 262144, 262144),
            (1196, 262144, 1196),
        ] {
            exercise_python(offer, cap, expected).await;
        }
    })
    .await
    .expect("Python TCP Link MTU regression timed out");
}

async fn exercise_python(offer: u32, cap: u32, expected: u32) {
    use rns_identity::destination::{DestType, Destination, Direction};
    use rns_identity::identity::Identity;
    let mut child = tokio::process::Command::new(
        std::env::var("RNS_PYTHON_BIN").unwrap_or_else(|_| "/usr/bin/python3.11".into()),
    )
    .args(["-B", "-c", include_str!("link_mtu_peer.py")])
    .arg(std::env::var("RNS_PYTHON_ROOT").unwrap_or_else(|_| "/home/room/src/Reticulum".into()))
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::inherit())
    .kill_on_drop(true)
    .spawn()
    .unwrap();
    let mut peer_input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    let hello: serde_json::Value =
        serde_json::from_str(&output.next_line().await.unwrap().unwrap()).unwrap();
    let port = u16::try_from(hello["port"].as_u64().unwrap()).unwrap();
    let identity = Identity::new();
    let dest = Destination::new(Some(&identity), Direction::In, DestType::Single, "test.mtu")
        .unwrap()
        .hash;
    let (mut actor, input) = TransportActor::new();
    let mut tasks = Tasks(Vec::new());
    let mut config = tcp::TcpClientConfig::new("python-mtu", "127.0.0.1", port);
    config.fixed_mtu = Some(cap);
    config.receive_ifac_size = Some(0);
    config.max_reconnect_tries = Some(1);
    let handle = tcp::spawn_tcp_client(config, 1, input.clone())
        .await
        .unwrap();
    tasks.0.push(handle.read_task);
    let mut entry = InterfaceEntry::new(
        handle.name,
        InterfaceMode::Full,
        InterfaceDirection::bidirectional(),
        handle.bitrate,
        handle.mtu,
        handle.tx,
    );
    entry.online = Some(handle.online);
    entry.diagnostics = handle.diagnostics;
    actor.interfaces.insert(1, entry);
    let (event_tx, event_rx) = mpsc::channel(16);
    actor.local_destinations.insert(dest);
    actor.destination_channels.insert(dest, event_tx);
    tasks.0.push(tokio::spawn(actor.run()));
    let mut manager = LinkManager::with_destination(
        input,
        event_rx,
        &identity,
        "test.mtu",
        identity.get_signing_key(),
    );
    let (delivery_tx, mut delivery_rx) = mpsc::channel(4);
    manager.set_link_packet_channel(delivery_tx);
    let config = serde_json::json!({
        "offer": offer, "expected": expected, "destination": hex::encode(dest),
        "public_key": hex::encode(identity.get_public_key()),
    });
    peer_input
        .write_all(format!("{config}\n").as_bytes())
        .await
        .unwrap();
    assert!(manager.step().await); // LINKREQUEST -> signed LRPROOF over TCP.
    assert!(manager.step().await); // Python validates proof and sends LRRTT.
    let report: serde_json::Value =
        serde_json::from_str(&output.next_line().await.unwrap().unwrap()).unwrap();
    let link_id: [u8; 16] = hex::decode(report["link_id"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let link = manager.get_link(&link_id).unwrap();
    assert_eq!(link.mtu, expected);
    assert_eq!(report["mtu"].as_u64(), Some(u64::from(expected)));
    assert_eq!(report["mdu"].as_u64(), Some(link.mdu as u64));
    let mdu = link.mdu;
    for length in [1, mdu] {
        assert!(manager.step().await);
        let (payload, received_link) = delivery_rx.recv().await.unwrap();
        assert_eq!(received_link, link_id);
        assert_eq!(
            payload,
            (0..length).map(|i| (i % 256) as u8).collect::<Vec<_>>()
        );
        manager.send_link_packet(&link_id, &payload).unwrap();
    }
    let report: serde_json::Value =
        serde_json::from_str(&output.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(report["complete"], true);
    assert!(child.wait().await.unwrap().success());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Python reference and local TCP sockets"]
async fn rust_initiator_python_responder_large_mtu_tcp_roundtrip() {
    timeout(Duration::from_secs(45), async {
        for (offer, cap, expected) in [
            (32768, 500, 500),
            (32768, 1196, 1196),
            (524288, 262144, 262144),
            (1196, 262144, 1196),
        ] {
            exercise_python_responder(offer, cap, expected).await;
        }
    })
    .await
    .expect("Python responder TCP MTU regression timed out");
}

async fn receive_link_packet(events: &mut mpsc::Receiver<DestinationEvent>) -> bytes::Bytes {
    loop {
        if let DestinationEvent::InboundPacket { raw, interface_id } = events.recv().await.unwrap()
        {
            assert_eq!(interface_id, 1);
            return raw;
        }
    }
}

async fn exercise_python_responder(offer: u32, cap: u32, expected: u32) {
    let mut child = tokio::process::Command::new(
        std::env::var("RNS_PYTHON_BIN").unwrap_or_else(|_| "/usr/bin/python3.11".into()),
    )
    .args(["-B", "-c", include_str!("link_mtu_peer.py")])
    .arg(std::env::var("RNS_PYTHON_ROOT").unwrap_or_else(|_| "/home/room/src/Reticulum".into()))
    .arg("responder")
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::inherit())
    .kill_on_drop(true)
    .spawn()
    .unwrap();
    let mut peer_input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    let hello: serde_json::Value =
        serde_json::from_str(&output.next_line().await.unwrap().unwrap()).unwrap();
    let port = u16::try_from(hello["port"].as_u64().unwrap()).unwrap();
    let dest: [u8; 16] = hex::decode(hello["destination"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let identity: [u8; 64] = hex::decode(hello["public_key"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let public_bytes: [u8; 32] = identity[32..].try_into().unwrap();
    let public = rns_crypto::ed25519::Ed25519PublicKey::from_bytes(&public_bytes).unwrap();
    let (mut actor, input) = TransportActor::new();
    let mut tasks = Tasks(Vec::new());
    let mut config = tcp::TcpClientConfig::new("python-responder", "127.0.0.1", port);
    config.fixed_mtu = Some(offer);
    config.receive_ifac_size = Some(0);
    config.max_reconnect_tries = Some(1);
    let handle = tcp::spawn_tcp_client(config, 1, input.clone())
        .await
        .unwrap();
    tasks.0.push(handle.read_task);
    // Do not send the first request until the real driver is connected.
    while !handle.online.load(std::sync::atomic::Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    let mut entry = InterfaceEntry::new(
        handle.name,
        InterfaceMode::Full,
        InterfaceDirection::bidirectional(),
        handle.bitrate,
        handle.mtu,
        handle.tx,
    );
    entry.online = Some(handle.online);
    entry.diagnostics = handle.diagnostics;
    actor.interfaces.insert(1, entry);
    // Seed a known direct route; route discovery/announce is not under test.
    actor.path_table.insert(
        dest,
        rns_transport::path_table::PathEntry::new(None, 1, 1, InterfaceMode::Full),
    );
    tasks.0.push(tokio::spawn(actor.run()));
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    input
        .send(TransportMessage::Rpc {
            query: TransportQuery::GetNextHopMtu { dest },
            response_tx,
        })
        .await
        .unwrap();
    let TransportQueryResponse::IntResult(mtu) = response_rx.await.unwrap() else {
        panic!("MTU query response");
    };
    assert_eq!(mtu, i64::from(offer));
    let (mut link, request) = Link::new_initiator_with_mtu(dest, 1, u32::try_from(mtu).unwrap());
    let (event_tx, mut events) = mpsc::channel(16);
    input
        .send(TransportMessage::RegisterDestination {
            hash: link.link_id,
            app_name: "test.mtu.link".into(),
            delivery_tx: Some(event_tx),
        })
        .await
        .unwrap();
    let config = serde_json::json!({"offer": offer, "cap": cap, "expected": expected});
    peer_input
        .write_all(format!("{config}\n").as_bytes())
        .await
        .unwrap();
    input
        .send(TransportMessage::Outbound(OutboundRequest {
            raw: packet(dest, PacketType::LinkRequest, PacketContext::None, &request).into(),
            destination_hash: dest,
        }))
        .await
        .unwrap();
    let proof = receive_link_packet(&mut events).await;
    let (header, offset) = PacketHeader::unpack(&proof).unwrap();
    assert_eq!(header.context, PacketContext::Lrproof);
    assert_eq!(header.destination_hash, link.link_id);
    let rtt = link
        .validate_proof(&proof[offset..], &public, &public_bytes)
        .unwrap();
    assert_eq!(link.mtu, expected);
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    input
        .send(TransportMessage::Rpc {
            query: TransportQuery::ConfirmLocalLinkProof {
                link_id: link.link_id,
                interface_id: 1,
                dest,
                hops: header.hops,
                rebalance: false,
            },
            response_tx,
        })
        .await
        .unwrap();
    assert!(matches!(
        response_rx.await.unwrap(),
        TransportQueryResponse::BoolResult(true)
    ));
    input
        .send(TransportMessage::Outbound(OutboundRequest {
            raw: packet(link.link_id, PacketType::Data, PacketContext::Lrrtt, &rtt).into(),
            destination_hash: link.link_id,
        }))
        .await
        .unwrap();
    let report: serde_json::Value =
        serde_json::from_str(&output.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(report["link_id"], hex::encode(link.link_id));
    assert_eq!(report["mtu"].as_u64(), Some(u64::from(expected)));
    assert_eq!(report["mdu"].as_u64(), Some(link.mdu as u64));
    for length in [1, link.mdu] {
        let payload: Vec<u8> = (0..length).map(|i| (i % 256) as u8).collect();
        let raw = packet(
            link.link_id,
            PacketType::Data,
            PacketContext::None,
            &link.encrypt(&payload).unwrap(),
        );
        assert!(raw.len() <= expected as usize);
        input
            .send(TransportMessage::Outbound(OutboundRequest {
                raw: raw.into(),
                destination_hash: link.link_id,
            }))
            .await
            .unwrap();
        let reply = receive_link_packet(&mut events).await;
        let (header, offset) = PacketHeader::unpack(&reply).unwrap();
        assert_eq!(header.destination_hash, link.link_id);
        assert_eq!(header.flags.packet_type, PacketType::Data);
        assert_eq!(header.context, PacketContext::None);
        assert!(reply.len() <= expected as usize);
        assert_eq!(link.decrypt(&reply[offset..]).unwrap(), payload);
    }
    let report: serde_json::Value =
        serde_json::from_str(&output.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(report["complete"], true);
    assert!(child.wait().await.unwrap().success());
}
