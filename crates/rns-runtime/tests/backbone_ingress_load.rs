//! Bounded two-peer TCP load through Backbone drivers and the transport actor.
//! Deliberately tiny DATA queue: drops are expected and reconciled, not hidden.
#![cfg(feature = "full")]

use rns_interface::{
    backbone::{BackboneClientConfig, spawn_backbone_client},
    hdlc,
};
use rns_transport::{
    actor::TransportActor,
    constants::{InterfaceDirection, InterfaceMode},
    inbound_queue::{InboundQueueLimits, InboundQueueStats},
    link_messages::DestinationEvent,
    messages::{InterfaceEntry, TransportMessage, TransportQuery, TransportQueryResponse},
};
use rns_wire::{
    context::PacketContext,
    flags::{DestinationType, HeaderType, PacketFlags, PacketType, TransportType},
    header::PacketHeader,
};
use tokio::{
    io::AsyncWriteExt,
    net::TcpListener,
    sync::mpsc,
    time::{Duration, timeout},
};

struct Tasks(Vec<tokio::task::JoinHandle<()>>);
impl Drop for Tasks {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

fn packet(dest: [u8; 16], peer: u64, sequence: u64) -> Vec<u8> {
    let mut raw = PacketHeader {
        flags: PacketFlags {
            header_type: HeaderType::Header1,
            context_flag: false,
            transport_type: TransportType::Broadcast,
            destination_type: DestinationType::Plain,
            packet_type: PacketType::Data,
        },
        hops: 0,
        transport_id: None,
        destination_hash: dest,
        context: PacketContext::None,
    }
    .pack()
    .unwrap();
    raw.extend_from_slice(&peer.to_be_bytes());
    raw.extend_from_slice(&sequence.to_be_bytes());
    raw.resize(256, 0x55);
    raw
}

async fn stats(control: &mpsc::Sender<TransportMessage>) -> InboundQueueStats {
    timeout(Duration::from_secs(1), async {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        control
            .send(TransportMessage::Rpc {
                query: TransportQuery::GetInboundQueueStats,
                response_tx,
            })
            .await
            .unwrap();
        let TransportQueryResponse::InboundQueueStats(Some(stats)) = response_rx.await.unwrap()
        else {
            panic!("queue statistics unavailable");
        };
        assert_eq!(stats.capacities, [4; 4]);
        assert!(stats.snapshot.heights.iter().all(|height| *height <= 4));
        stats
    })
    .await
    .expect("control query starved behind incoming DATA")
}

fn delivered(event: DestinationEvent, last: &mut [Option<u64>; 2]) -> (u64, u64) {
    let DestinationEvent::InboundPacket { raw, interface_id } = event else {
        panic!("unexpected destination event");
    };
    let (_, offset) = PacketHeader::unpack(&raw).unwrap();
    let peer = u64::from_be_bytes(raw[offset..offset + 8].try_into().unwrap());
    let sequence = u64::from_be_bytes(raw[offset + 8..offset + 16].try_into().unwrap());
    assert_eq!(peer, interface_id);
    assert!((1..=2).contains(&peer));
    assert_eq!(raw.len(), 256);
    assert!(raw[offset + 16..].iter().all(|byte| *byte == 0x55));
    let previous = &mut last[peer as usize - 1];
    assert!(
        previous.is_none_or(|value| sequence > value),
        "duplicate or reordered packet"
    );
    *previous = Some(sequence);
    (peer, sequence)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_backbone_peers_queue_pressure_control_and_recovery() {
    // The policy may retain gates for multiple seconds after the short burst.
    timeout(Duration::from_secs(45), exercise())
        .await
        .expect("Backbone ingress load timed out");
}

async fn exercise() {
    const PER_PEER: u64 = 4096;
    let dest = [0xAB; 16];
    let (mut actor, input, control) = TransportActor::new_with_control_channel_and_queue_limits(
        InboundQueueLimits::new([4; 4]).unwrap(),
    );
    let (delivery_tx, mut deliveries) = mpsc::channel((2 * PER_PEER + 2) as usize);
    actor.local_destinations.insert(dest);
    actor.destination_channels.insert(dest, delivery_tx);
    let mut tasks = Tasks(Vec::new());
    let mut peers = Vec::new();
    for id in 1..=2 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = BackboneClientConfig::new(
            "ingress-load",
            "127.0.0.1",
            listener.local_addr().unwrap().port(),
        );
        config.receive_ifac_size = Some(0);
        config.max_reconnect_tries = Some(1);
        let handle = spawn_backbone_client(config, id, input.clone())
            .await
            .unwrap();
        tasks.0.push(handle.read_task);
        peers.push(listener.accept().await.unwrap().0);
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
        actor.interfaces.insert(id, entry);
    }
    let (first, second) = peers.split_at_mut(1);
    let send = |peer_id| {
        let mut wire = Vec::new();
        for sequence in 0..PER_PEER {
            wire.extend(hdlc::frame(&packet(dest, peer_id, sequence)));
        }
        wire
    };
    let first_wire = send(1);
    let second_wire = send(2);
    let (a, b) = tokio::join!(
        first[0].write_all(&first_wire),
        second[0].write_all(&second_wire)
    );
    a.unwrap();
    b.unwrap();
    // Start with actual driver backpressure, not synthetic queued actor messages.
    timeout(Duration::from_secs(5), async {
        while input.capacity() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("raw ingress channel never filled");
    tasks.0.push(tokio::spawn(actor.run()));
    let mut last = [None; 2];
    let mut counts = [0u64; 2];
    let mut queries = 0;
    let mut max_query = Duration::ZERO;
    let dropped;
    loop {
        while let Ok(event) = deliveries.try_recv() {
            let (peer, sequence) = delivered(event, &mut last);
            assert!(sequence < PER_PEER);
            counts[peer as usize - 1] += 1;
        }
        let start = std::time::Instant::now();
        let state = stats(&control).await;
        max_query = max_query.max(start.elapsed());
        queries += 1;
        assert_eq!(&state.snapshot.dropped[1..], &[0; 3]);
        let reconciled = counts.iter().sum::<u64>() + state.snapshot.dropped[0];
        assert!(reconciled <= 2 * PER_PEER);
        if reconciled == 2 * PER_PEER {
            assert_eq!(state.snapshot.total, 0);
            dropped = state.snapshot.dropped[0];
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(
        dropped > 0,
        "tiny DATA queue should overflow under the prefilled load"
    );
    assert!(
        counts.iter().all(|count| *count > 0),
        "one peer made no progress"
    );
    // Check recovery serially so the two probes do not intentionally overload
    // the tiny queue again. Both existing TCP connections must remain usable.
    for (index, peer) in peers.iter_mut().enumerate() {
        let id = index as u64 + 1;
        peer.write_all(&hdlc::frame(&packet(dest, id, PER_PEER)))
            .await
            .unwrap();
        assert_eq!(
            delivered(deliveries.recv().await.unwrap(), &mut last),
            (id, PER_PEER)
        );
    }
    assert_eq!(stats(&control).await.snapshot.dropped[0], dropped);
    eprintln!(
        "backbone_ingress: sent={} delivered={counts:?} data_dropped={dropped} control_queries={queries} max_control_ms={:.3}; bounded burst, not throughput or fairness benchmark",
        2 * PER_PEER,
        max_query.as_secs_f64() * 1000.0
    );
    control.send(TransportMessage::Shutdown).await.unwrap();
    control.closed().await;
}
