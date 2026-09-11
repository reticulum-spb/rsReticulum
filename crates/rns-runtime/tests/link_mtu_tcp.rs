//! Real TCP/HDLC -> transport actor -> runtime LinkManager MTU regression.
//! The peer uses the Rust Link library, not a second runtime or Python process.
use rns_crypto::ed25519::Ed25519PrivateKey;
use rns_interface::{hdlc, tcp};
use rns_link::link::Link;
use rns_runtime::link_manager::LinkManager;
use rns_transport::actor::TransportActor;
use rns_transport::constants::{InterfaceDirection, InterfaceMode};
use rns_transport::messages::InterfaceEntry;
use rns_wire::context::PacketContext;
use rns_wire::flags::{DestinationType, HeaderType, PacketFlags, PacketType, TransportType};
use rns_wire::header::PacketHeader;
use std::collections::VecDeque;
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
