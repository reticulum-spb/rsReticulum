use super::*;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;

struct Writer {
    bytes: Arc<std::sync::Mutex<Vec<u8>>>,
    calls: Arc<AtomicU64>,
    max_write: usize,
    fail_after: Option<usize>,
    interrupt_once: bool,
    write_zero: bool,
}

impl AsyncWrite for Writer {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.interrupt_once {
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
        write_zero: false,
    }
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
        let mut cursor = TxFrame::new(payload.into());
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
    let (tx, rx) = mpsc::channel(128);
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
    let (tx, rx) = mpsc::channel(4);
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

#[tokio::test]
async fn partial_writes_errors_and_zero_count_only_accepted_bytes() {
    for zero in [false, true] {
        let (tx, rx) = mpsc::channel(2);
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
