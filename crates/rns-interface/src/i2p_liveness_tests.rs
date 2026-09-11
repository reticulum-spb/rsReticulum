use super::*;
use std::time::Duration;

#[tokio::test]
async fn writer_failure_ends_connection_while_reader_is_idle() {
    let (reader, _read_peer) = tokio::io::duplex(64);
    let (writer, write_peer) = tokio::io::duplex(64);
    drop(write_peer);
    let (transport, _events) = mpsc::channel(1);
    let (tx, rx) = mpsc::channel(1);
    tx.send(Bytes::from_static(b"write must fail"))
        .await
        .unwrap();
    let online = AtomicBool::new(true);
    let txb = AtomicU64::new(0);
    tokio::time::timeout(
        Duration::from_secs(1),
        i2p_connection(
            reader,
            writer,
            &[],
            1,
            &transport,
            &online,
            &AtomicU64::new(0),
            &txb,
            rx,
        ),
    )
    .await
    .expect("writer error must cancel idle reader immediately");
    assert!(!online.load(Ordering::SeqCst));
    assert_eq!(txb.load(Ordering::Relaxed), 0);
    assert!(tx.is_closed());
}

#[tokio::test(start_paused = true)]
async fn watchdog_uses_strict_timeout_and_received_bytes_renew_it() {
    let last_read = Arc::new(LastRead::new(tokio::time::Instant::now()));
    let state = last_read.clone();
    let task = tokio::spawn(async move { i2p_watchdog(&state, 1).await });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(110)).await;
    tokio::task::yield_now().await;
    assert!(
        !task.is_finished(),
        "110 seconds is not strictly greater than 110"
    );
    *last_read.lock().unwrap() = tokio::time::Instant::now();
    tokio::time::advance(Duration::from_secs(110)).await;
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    tokio::time::advance(Duration::from_secs(1)).await;
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn probes_repeat_after_idle_and_data_resets_idle_clock() {
    let (writer, mut peer) = tokio::io::duplex(4096);
    let (tx, rx) = mpsc::channel(8);
    let txb = Arc::new(AtomicU64::new(0));
    let counter = txb.clone();
    let task = tokio::spawn(async move { i2p_write_loop(writer, rx, &counter).await });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(10)).await;
    tokio::task::yield_now().await;
    // No probe at exactly ten seconds: the next data frame is first on wire.
    tx.send(Bytes::from_static(b"data~}")).await.unwrap();
    let expected = hdlc::frame(b"data~}");
    let mut actual = vec![0; expected.len()];
    peer.read_exact(&mut actual).await.unwrap();
    assert_eq!(actual, expected);
    for elapsed in [11, 1] {
        tokio::time::advance(Duration::from_secs(elapsed)).await;
        let mut probe = [0; 2];
        peer.read_exact(&mut probe).await.unwrap();
        assert_eq!(probe, [hdlc::FLAG; 2]);
    }
    assert_eq!(txb.load(Ordering::Relaxed), expected.len() as u64);
    drop(tx);
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn empty_probes_refresh_reader_without_transport_delivery() {
    let (reader, mut peer) = tokio::io::duplex(4096);
    let (tx, mut events) = mpsc::channel(1);
    let online = Arc::new(AtomicBool::new(true));
    let rxb = Arc::new(AtomicU64::new(0));
    let last = Arc::new(LastRead::new(tokio::time::Instant::now()));
    let (state, counter, stamp) = (online.clone(), rxb.clone(), last.clone());
    let task = tokio::spawn(async move {
        i2p_read_loop(reader, &[hdlc::FLAG; 2], 1, &tx, &state, &counter, &stamp).await;
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(100)).await;
    // Split a probe across reads, followed by actual payload.
    peer.write_all(&[hdlc::FLAG]).await.unwrap();
    tokio::task::yield_now().await;
    peer.write_all(&[hdlc::FLAG]).await.unwrap();
    tokio::task::yield_now().await;
    assert_eq!(*last.lock().unwrap(), tokio::time::Instant::now());
    assert!(matches!(
        events.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    peer.write_all(&hdlc::frame(b"payload~}")).await.unwrap();
    let Some(TransportMessage::Inbound(packet)) = events.recv().await else {
        panic!("payload")
    };
    assert_eq!(packet.raw.as_ref(), b"payload~}");
    assert_eq!(
        rxb.load(Ordering::Relaxed),
        4 + hdlc::frame(b"payload~}").len() as u64
    );
    peer.shutdown().await.unwrap();
    task.await.unwrap();
    assert!(!online.load(Ordering::SeqCst));
}

#[tokio::test(start_paused = true)]
async fn watchdog_cancels_blocked_admission_and_writer() {
    let (socket, mut peer) = tokio::io::duplex(1);
    let (reader, writer) = tokio::io::split(socket);
    let (transport, mut events) = mpsc::channel(1);
    transport.send(TransportMessage::Shutdown).await.unwrap();
    let (tx, rx) = mpsc::channel(1);
    tx.send(Bytes::from_static(b"blocked socket output"))
        .await
        .unwrap();
    let online = Arc::new(AtomicBool::new(true));
    let state = online.clone();
    let task = tokio::spawn(async move {
        i2p_connection(
            reader,
            writer,
            &hdlc::frame(b"blocked admission"),
            1,
            &transport,
            &state,
            &AtomicU64::new(0),
            &AtomicU64::new(0),
            rx,
        )
        .await;
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(110)).await;
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    tokio::time::advance(Duration::from_secs(1)).await;
    task.await.unwrap();
    assert!(!online.load(Ordering::SeqCst));
    assert!(tx.is_closed());
    assert!(matches!(
        events.recv().await,
        Some(TransportMessage::Shutdown)
    ));
    assert!(events.recv().await.is_none());
    let mut tail = Vec::new();
    peer.read_to_end(&mut tail).await.unwrap();
    assert!(tail.len() <= 1, "blocked writer must be dropped");
}

#[tokio::test]
async fn connection_abort_closes_writer_and_marks_offline() {
    let (socket, mut peer) = tokio::io::duplex(64);
    let (reader, writer) = tokio::io::split(socket);
    let (transport, _events) = mpsc::channel(1);
    let (_tx, rx) = mpsc::channel(1);
    let online = Arc::new(AtomicBool::new(true));
    let state = online.clone();
    let task = tokio::spawn(async move {
        i2p_connection(
            reader,
            writer,
            &[],
            1,
            &transport,
            &state,
            &AtomicU64::new(0),
            &AtomicU64::new(0),
            rx,
        )
        .await;
    });
    tokio::task::yield_now().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(!online.load(Ordering::SeqCst));
    assert_eq!(peer.read(&mut [0; 1]).await.unwrap(), 0);
}
