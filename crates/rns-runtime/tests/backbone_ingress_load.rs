//! Bounded multi-peer TCP load through Backbone drivers and the transport actor.
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

fn delivered(event: DestinationEvent, last: &mut [Option<u64>]) -> (u64, u64) {
    let DestinationEvent::InboundPacket { raw, interface_id } = event else {
        panic!("unexpected destination event");
    };
    let (_, offset) = PacketHeader::unpack(&raw).unwrap();
    let peer = u64::from_be_bytes(raw[offset..offset + 8].try_into().unwrap());
    let sequence = u64::from_be_bytes(raw[offset + 8..offset + 16].try_into().unwrap());
    assert_eq!(peer, interface_id);
    assert!((1..=last.len() as u64).contains(&peer));
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
#[ignore = "four-peer repeated ingress load; policy holds can take tens of seconds"]
async fn four_backbone_peers_repeated_pressure_and_progress() {
    timeout(Duration::from_secs(180), repeated_pressure())
        .await
        .expect("repeated Backbone pressure timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "transport actor churn/soak; defaults to 64 rounds with one-second pauses"]
async fn backbone_actor_churn_preserves_progress_and_control() {
    let rounds: u64 = std::env::var("RNS_BACKBONE_CHURN_ROUNDS")
        .map(|value| {
            value
                .parse()
                .expect("RNS_BACKBONE_CHURN_ROUNDS must be an integer")
        })
        .unwrap_or(64);
    assert!((1..=10000).contains(&rounds));
    timeout(Duration::from_secs(rounds * 20 + 30), actor_churn(rounds))
        .await
        .expect("Backbone actor churn timed out");
}

async fn interface_ids(control: &mpsc::Sender<TransportMessage>) -> Vec<u64> {
    timeout(Duration::from_secs(1), async {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        control
            .send(TransportMessage::Rpc {
                query: TransportQuery::GetInterfaceStats,
                response_tx,
            })
            .await
            .unwrap();
        let TransportQueryResponse::InterfaceStats(entries) = response_rx.await.unwrap() else {
            panic!("interface statistics unavailable");
        };
        let mut ids: Vec<_> = entries.into_iter().map(|entry| entry.id).collect();
        ids.sort_unstable();
        ids
    })
    .await
    .expect("interface query starved during churn")
}

async fn actor_churn(rounds: u64) {
    const PEERS: usize = 4;
    const BURST: u64 = 128;
    let dest = [0xCD; 16];
    let (mut actor, input, control) = TransportActor::new_with_control_channel_and_queue_limits(
        InboundQueueLimits::new([4; 4]).unwrap(),
    );
    let (delivery_tx, mut deliveries) = mpsc::channel(PEERS * BURST as usize + PEERS);
    actor.local_destinations.insert(dest);
    actor.destination_channels.insert(dest, delivery_tx);
    let mut actor_task = Tasks(vec![tokio::spawn(actor.run())]);
    let started = std::time::Instant::now();
    let mut last = [None; PEERS];
    let mut counts = [0u64; PEERS];
    let mut offered = 0u64;
    let mut drops = 0u64;
    let mut queries = 0u64;
    let mut max_query = Duration::ZERO;
    for round in 0..rounds {
        let mut drivers = Tasks(Vec::new());
        let mut peers = Vec::new();
        for id in 1..=PEERS as u64 {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut config = BackboneClientConfig::new(
                "actor-churn",
                "127.0.0.1",
                listener.local_addr().unwrap().port(),
            );
            config.receive_ifac_size = Some(0);
            config.max_reconnect_tries = Some(1);
            let handle = spawn_backbone_client(config, id, input.clone())
                .await
                .unwrap();
            drivers.0.push(handle.read_task);
            peers.push(listener.accept().await.unwrap().0);
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
            control
                .send(TransportMessage::RegisterInterface { id, entry })
                .await
                .unwrap();
        }
        assert_eq!(interface_ids(&control).await, [1, 2, 3, 4]);
        let base = round * (2 * BURST + 1);
        for phase in 0..2 {
            let before = counts;
            let active = if phase == 0 { PEERS } else { 2 };
            let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(active));
            let mut producers = tokio::task::JoinSet::new();
            for (index, mut peer) in peers.drain(..).enumerate() {
                let barrier = barrier.clone();
                producers.spawn(async move {
                    if phase == 0 || index >= 2 {
                        let mut wire = Vec::new();
                        for sequence in base + phase * BURST..base + (phase + 1) * BURST {
                            wire.extend(hdlc::frame(&packet(dest, index as u64 + 1, sequence)));
                        }
                        barrier.wait().await;
                        peer.write_all(&wire).await.unwrap();
                    }
                    (index, peer)
                });
            }
            offered += active as u64 * BURST;
            let mut returned = Vec::new();
            // Interleave joining producers, bounded delivery work and RPC probes.
            loop {
                while let Some(result) = producers.try_join_next() {
                    returned.push(result.unwrap());
                }
                for _ in 0..512 {
                    let Ok(event) = deliveries.try_recv() else {
                        break;
                    };
                    let (id, sequence) = delivered(event, &mut last);
                    assert!((base + phase * BURST..base + (phase + 1) * BURST).contains(&sequence));
                    assert!(phase == 0 || id >= 3);
                    counts[id as usize - 1] += 1;
                }
                let probe = std::time::Instant::now();
                let state = stats(&control).await;
                queries += 1;
                max_query = max_query.max(probe.elapsed());
                assert_eq!(&state.snapshot.dropped[1..], &[0; 3]);
                assert!(state.snapshot.dropped[0] >= drops);
                drops = state.snapshot.dropped[0];
                let accounted = counts.iter().sum::<u64>() + drops;
                assert!(accounted <= offered);
                if producers.is_empty() && accounted == offered {
                    assert_eq!(state.snapshot.total, 0);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            returned.sort_by_key(|(index, _)| *index);
            peers = returned.into_iter().map(|(_, peer)| peer).collect();
            for index in if phase == 0 { 0..PEERS } else { 2..PEERS } {
                assert!(
                    counts[index] > before[index],
                    "peer made no progress in round {round}, phase {phase}"
                );
            }
            if phase == 0 {
                // Account all earlier input before FIN: no unobservable in-flight loss
                // is mislabelled as an inbound class-queue drop. Driver deregistration
                // now races the surviving peers' next burst on the running actor.
                for peer in &mut peers[..2] {
                    peer.shutdown().await.unwrap();
                }
            }
        }
        timeout(Duration::from_secs(3), async {
            while interface_ids(&control).await != [3, 4] {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("departed interfaces retained during survivor traffic");
        for (index, peer) in peers.iter_mut().enumerate().skip(2) {
            let id = index as u64 + 1;
            peer.write_all(&hdlc::frame(&packet(dest, id, base + 2 * BURST)))
                .await
                .unwrap();
            assert_eq!(
                delivered(deliveries.recv().await.unwrap(), &mut last),
                (id, base + 2 * BURST)
            );
        }
        assert_eq!(stats(&control).await.snapshot.dropped[0], drops);
        for peer in &mut peers[2..] {
            peer.shutdown().await.unwrap();
        }
        timeout(Duration::from_secs(3), async {
            for task in &mut drivers.0 {
                task.await.unwrap();
            }
            while !interface_ids(&control).await.is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("drivers/interfaces survived end of churn round");
        assert!(deliveries.try_recv().is_err());
        drop(peers);
        drop(drivers);
        if round % 8 == 7 || round + 1 == rounds {
            eprintln!(
                "backbone_actor_churn: rounds={} offered={offered} delivered={counts:?} drops={drops} queries={queries} max_control_ms={:.3} elapsed_s={:.3}; all four interfaces removed before ID reuse",
                round + 1,
                max_query.as_secs_f64() * 1000.0,
                started.elapsed().as_secs_f64()
            );
        }
        if round + 1 < rounds {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    assert_eq!(counts.iter().sum::<u64>() + drops, rounds * 6 * BURST);
    timeout(Duration::from_secs(3), async {
        control.send(TransportMessage::Shutdown).await.unwrap();
        control.closed().await;
        (&mut actor_task.0[0]).await.unwrap();
    })
    .await
    .expect("actor shutdown stalled after churn");
}

async fn repeated_pressure() {
    const PEERS: usize = 4;
    const ROUNDS: u64 = 100;
    const PER_ROUND: u64 = 128;
    let dest = [0xBC; 16];
    let (mut actor, input, control) = TransportActor::new_with_control_channel_and_queue_limits(
        InboundQueueLimits::new([4; 4]).unwrap(),
    );
    let (delivery_tx, mut deliveries) = mpsc::channel(PEERS * PER_ROUND as usize + PEERS);
    actor.local_destinations.insert(dest);
    actor.destination_channels.insert(dest, delivery_tx);
    let mut tasks = Tasks(Vec::new());
    let mut peers = Vec::new();
    for id in 1..=PEERS as u64 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = BackboneClientConfig::new(
            "repeated-ingress",
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
    tasks.0.push(tokio::spawn(actor.run()));
    let started = std::time::Instant::now();
    let mut last = [None; PEERS];
    let mut counts = [0u64; PEERS];
    let mut min_round = [u64::MAX; PEERS];
    let mut queries = 0;
    let mut max_query = Duration::ZERO;
    let mut drops = 0;
    for round in 0..ROUNDS {
        let before = counts;
        // Independent producers share a start barrier. The bounded JoinSet is
        // dropped/aborted on cancellation; sockets return after each burst.
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(PEERS));
        let mut producers = tokio::task::JoinSet::new();
        for (index, mut peer) in peers.drain(..).enumerate() {
            let barrier = barrier.clone();
            producers.spawn(async move {
                let mut wire = Vec::new();
                for sequence in round * PER_ROUND..(round + 1) * PER_ROUND {
                    wire.extend(hdlc::frame(&packet(dest, index as u64 + 1, sequence)));
                }
                barrier.wait().await;
                peer.write_all(&wire).await.unwrap();
                (index, peer)
            });
        }
        let mut returned = Vec::new();
        while let Some(result) = producers.join_next().await {
            returned.push(result.unwrap());
        }
        returned.sort_by_key(|(index, _)| *index);
        peers = returned.into_iter().map(|(_, peer)| peer).collect();
        loop {
            while let Ok(event) = deliveries.try_recv() {
                let (peer, sequence) = delivered(event, &mut last);
                assert!((round * PER_ROUND..(round + 1) * PER_ROUND).contains(&sequence));
                counts[peer as usize - 1] += 1;
            }
            let start = std::time::Instant::now();
            let state = stats(&control).await;
            queries += 1;
            max_query = max_query.max(start.elapsed());
            assert_eq!(&state.snapshot.dropped[1..], &[0; 3]);
            assert!(state.snapshot.dropped[0] >= drops);
            drops = state.snapshot.dropped[0];
            let total = counts.iter().sum::<u64>() + drops;
            let expected = (round + 1) * PER_ROUND * PEERS as u64;
            assert!(total <= expected);
            if total == expected {
                assert_eq!(state.snapshot.total, 0);
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        for index in 0..PEERS {
            let progress = counts[index] - before[index];
            assert!(
                progress > 0,
                "peer {} made no progress in round {round}",
                index + 1
            );
            min_round[index] = min_round[index].min(progress);
        }
        if round % 25 == 24 {
            eprintln!(
                "backbone_repeated: rounds={} delivered={counts:?} drops={drops} elapsed_s={:.3}",
                round + 1,
                started.elapsed().as_secs_f64()
            );
        }
        if round + 1 < ROUNDS {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    assert!(drops > 0);
    for (index, peer) in peers.iter_mut().enumerate() {
        let id = index as u64 + 1;
        peer.write_all(&hdlc::frame(&packet(dest, id, ROUNDS * PER_ROUND)))
            .await
            .unwrap();
        assert_eq!(
            delivered(deliveries.recv().await.unwrap(), &mut last),
            (id, ROUNDS * PER_ROUND)
        );
    }
    assert_eq!(stats(&control).await.snapshot.dropped[0], drops);
    eprintln!(
        "backbone_repeated_complete: peers={PEERS} rounds={ROUNDS} offered={} delivered={counts:?} min_round_delivered={min_round:?} drops={drops} control_queries={queries} max_control_ms={:.3} elapsed_s={:.3}; round barriers and 250ms pauses, not continuous saturation or a fairness guarantee",
        PEERS as u64 * ROUNDS * PER_ROUND,
        max_query.as_secs_f64() * 1000.0,
        started.elapsed().as_secs_f64()
    );
    control.send(TransportMessage::Shutdown).await.unwrap();
    control.closed().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "independent asymmetric Backbone streams; backpressure can extend runtime"]
async fn asymmetric_backbone_streams_accounting_and_recovery() {
    timeout(Duration::from_secs(120), asymmetric_streams())
        .await
        .expect("asymmetric Backbone load timed out");
}

async fn asymmetric_streams() {
    const BATCHES: u64 = 500;
    const SIZES: [u64; 4] = [64, 16, 4, 1];
    const TOTAL: u64 = BATCHES * (64 + 16 + 4 + 1);
    let dest = [0xCD; 16];
    let (mut actor, input, control) = TransportActor::new_with_control_channel_and_queue_limits(
        InboundQueueLimits::new([4; 4]).unwrap(),
    );
    // The fixture cannot add application-channel loss even if delivery reading
    // pauses during a control query. This is a finite workload, not unbounded RAM.
    let (delivery_tx, mut deliveries) = mpsc::channel(TOTAL as usize + SIZES.len());
    actor.local_destinations.insert(dest);
    actor.destination_channels.insert(dest, delivery_tx);
    let mut tasks = Tasks(Vec::new());
    let mut sockets = Vec::new();
    for id in 1..=SIZES.len() as u64 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = BackboneClientConfig::new(
            "asymmetric-ingress",
            "127.0.0.1",
            listener.local_addr().unwrap().port(),
        );
        config.receive_ifac_size = Some(0);
        config.max_reconnect_tries = Some(1);
        let handle = spawn_backbone_client(config, id, input.clone())
            .await
            .unwrap();
        tasks.0.push(handle.read_task);
        sockets.push(listener.accept().await.unwrap().0);
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
    tasks.0.push(tokio::spawn(actor.run()));
    let started = std::time::Instant::now();
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(SIZES.len()));
    let mut producers = tokio::task::JoinSet::new();
    for (index, mut socket) in sockets.into_iter().enumerate() {
        let barrier = barrier.clone();
        producers.spawn(async move {
            barrier.wait().await;
            let mut timer = tokio::time::interval(Duration::from_millis(20));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            for batch in 0..BATCHES {
                timer.tick().await;
                let mut wire = Vec::new();
                for sequence in batch * SIZES[index]..(batch + 1) * SIZES[index] {
                    wire.extend(hdlc::frame(&packet(dest, index as u64 + 1, sequence)));
                }
                socket.write_all(&wire).await.unwrap();
            }
            (index, socket, started.elapsed())
        });
    }
    let mut returned = Vec::new();
    let mut last = [None; 4];
    let mut counts = [0u64; 4];
    let mut progress_at = [Duration::ZERO; 4];
    let mut max_gap = [Duration::ZERO; 4];
    let mut query_max = Duration::ZERO;
    let mut queries = 0;
    let mut drops = 0;
    let mut next_report = Duration::from_secs(5);
    loop {
        // Bound work between control probes even when the delivery queue is full.
        for _ in 0..512 {
            let Ok(event) = deliveries.try_recv() else {
                break;
            };
            let (peer, sequence) = delivered(event, &mut last);
            let index = peer as usize - 1;
            assert!(sequence < BATCHES * SIZES[index]);
            counts[index] += 1;
            let now = started.elapsed();
            max_gap[index] = max_gap[index].max(now - progress_at[index]);
            progress_at[index] = now;
        }
        while let Some(result) = producers.try_join_next() {
            returned.push(result.unwrap());
        }
        let query_started = std::time::Instant::now();
        let state = stats(&control).await;
        query_max = query_max.max(query_started.elapsed());
        queries += 1;
        assert_eq!(&state.snapshot.dropped[1..], &[0; 3]);
        assert!(state.snapshot.dropped[0] >= drops);
        drops = state.snapshot.dropped[0];
        let accounted = counts.iter().sum::<u64>() + drops;
        assert!(accounted <= TOTAL);
        if producers.is_empty() && accounted == TOTAL {
            assert_eq!(state.snapshot.total, 0);
            break;
        }
        if started.elapsed() >= next_report {
            eprintln!(
                "backbone_asymmetric: delivered={counts:?} drops={drops} producers_remaining={} elapsed_s={:.3}",
                producers.len(),
                started.elapsed().as_secs_f64()
            );
            next_report = started.elapsed() + Duration::from_secs(5);
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(counts.iter().all(|count| *count > 0));
    // Report starvation symptoms without treating policy hold durations as bugs
    // or claiming equal-share fairness for deliberately unequal offered rates.
    returned.sort_by_key(|(index, _, _)| *index);
    let sender_elapsed: Vec<_> = returned
        .iter()
        .map(|(_, _, elapsed)| elapsed.as_secs_f64())
        .collect();
    for (index, mut socket, _) in returned {
        let sequence = BATCHES * SIZES[index];
        socket
            .write_all(&hdlc::frame(&packet(dest, index as u64 + 1, sequence)))
            .await
            .unwrap();
        assert_eq!(
            delivered(deliveries.recv().await.unwrap(), &mut last),
            (index as u64 + 1, sequence)
        );
    }
    assert_eq!(stats(&control).await.snapshot.dropped[0], drops);
    eprintln!(
        "backbone_asymmetric_complete: offered={:?} delivered={counts:?} drops={drops} observed_delivery_gap_s={:?} sender_elapsed_s={sender_elapsed:?} control_queries={queries} max_control_ms={:.3} elapsed_s={:.3}; independent paced streams, not equal-share fairness or guaranteed line-rate saturation",
        SIZES.map(|size| size * BATCHES),
        max_gap.map(|gap| gap.as_secs_f64()),
        query_max.as_secs_f64() * 1000.0,
        started.elapsed().as_secs_f64()
    );
    control.send(TransportMessage::Shutdown).await.unwrap();
    control.closed().await;
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
