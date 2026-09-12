//! Optional byte-accounted interface output. Ordinary drivers retain their
//! existing Tokio channel; managed drivers reserve the complete encoded frame.
use bytes::Bytes;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub enum InterfaceTx {
    Plain(mpsc::Sender<Bytes>),
    Managed(Arc<ManagedTx>),
}

impl From<mpsc::Sender<Bytes>> for InterfaceTx {
    fn from(tx: mpsc::Sender<Bytes>) -> Self {
        Self::Plain(tx)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TxSnapshot {
    pub buffered: u64,
    pub sent: u64,
    pub dropped_frames: u64,
    pub dropped_bytes: u64,
    pub gated: bool,
}

/// Optional byte-level statistics; plain channels cannot report encoded bytes.
#[derive(Debug, Default, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TxDiagnostics {
    pub txbuffered: Option<u64>,
    pub txdrb: Option<u64>,
    pub txstalled: Option<bool>,
    pub tx_queue_frames: Option<usize>,
}

#[derive(Debug, Default)]
pub struct TxAccounting(Mutex<TxSnapshot>);

impl TxAccounting {
    pub fn snapshot(&self) -> TxSnapshot {
        *self.0.lock().unwrap()
    }
    pub fn set_gated(&self, gated: bool) {
        self.0.lock().unwrap().gated = gated;
    }
}

#[derive(Debug)]
pub struct ManagedTx {
    tx: mpsc::Sender<OutboundFrame>,
    accounting: Arc<TxAccounting>,
    limit: u64,
    encoded_len: fn(&[u8]) -> u64,
}

/// A frame's reservation survives queue movement and encoding. Batch writers
/// retain a lease for every encoded segment until written or discarded.
#[derive(Debug)]
pub struct TxLease {
    accounting: Arc<TxAccounting>,
    remaining: Mutex<u64>,
}

impl TxLease {
    pub fn written(&self, bytes: u64) {
        let mut remaining = self.remaining.lock().unwrap();
        assert!(bytes <= *remaining, "TX progress exceeds frame reservation");
        let mut state = self.accounting.0.lock().unwrap();
        *remaining -= bytes;
        state.buffered -= bytes;
        state.sent = state.sent.saturating_add(bytes);
    }
}

impl Drop for TxLease {
    fn drop(&mut self) {
        let remaining = *self.remaining.get_mut().unwrap();
        self.accounting.0.lock().unwrap().buffered -= remaining;
    }
}

#[derive(Debug)]
pub struct OutboundFrame {
    pub raw: Bytes,
    pub lease: Option<Arc<TxLease>>,
}

impl From<Bytes> for OutboundFrame {
    fn from(raw: Bytes) -> Self {
        Self { raw, lease: None }
    }
}

pub fn byte_channel(
    depth: usize,
    limit: u64,
    encoded_len: fn(&[u8]) -> u64,
) -> (
    InterfaceTx,
    mpsc::Receiver<OutboundFrame>,
    Arc<TxAccounting>,
) {
    let (tx, rx) = mpsc::channel(depth);
    let accounting = Arc::new(TxAccounting::default());
    (
        InterfaceTx::Managed(Arc::new(ManagedTx {
            tx,
            accounting: accounting.clone(),
            limit,
            encoded_len,
        })),
        rx,
        accounting,
    )
}

impl InterfaceTx {
    pub fn diagnostics(&self) -> TxDiagnostics {
        let snapshot = self.accounting().map(|accounting| accounting.snapshot());
        TxDiagnostics {
            txbuffered: snapshot.map(|s| s.buffered),
            txdrb: snapshot.map(|s| s.dropped_bytes),
            txstalled: snapshot.map(|s| s.gated),
            tx_queue_frames: Some(self.max_capacity().saturating_sub(self.capacity())),
        }
    }
    pub fn accounting(&self) -> Option<Arc<TxAccounting>> {
        match self {
            Self::Plain(_) => None,
            Self::Managed(q) => Some(q.accounting.clone()),
        }
    }
    pub fn try_send(&self, raw: Bytes) -> Result<(), mpsc::error::TrySendError<Bytes>> {
        let Self::Managed(queue) = self else {
            let Self::Plain(tx) = self else {
                unreachable!()
            };
            return tx.try_send(raw);
        };
        // Lock covers admission + enqueue, so concurrent producers cannot
        // oversubscribe the byte limit. No await occurs while holding it.
        let mut state = queue.accounting.0.lock().unwrap();
        if queue.tx.is_closed() {
            return Err(mpsc::error::TrySendError::Closed(raw));
        }
        let size = (queue.encoded_len)(&raw);
        let total = state.buffered.checked_add(size);
        if state.gated || total.is_none_or(|n| n > queue.limit) {
            state.dropped_frames = state.dropped_frames.saturating_add(1);
            state.dropped_bytes = state.dropped_bytes.saturating_add(size);
            return Err(mpsc::error::TrySendError::Full(raw));
        }
        // Reserve a packet slot before constructing a lease; on failure there
        // is no Drop that could recursively acquire the accounting mutex.
        let permit = match queue.tx.try_reserve() {
            Ok(permit) => permit,
            Err(mpsc::error::TrySendError::Full(_)) => {
                state.dropped_frames = state.dropped_frames.saturating_add(1);
                state.dropped_bytes = state.dropped_bytes.saturating_add(size);
                return Err(mpsc::error::TrySendError::Full(raw));
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(mpsc::error::TrySendError::Closed(raw));
            }
        };
        state.buffered = total.unwrap();
        let frame = OutboundFrame {
            raw,
            lease: Some(Arc::new(TxLease {
                accounting: queue.accounting.clone(),
                remaining: Mutex::new(size),
            })),
        };
        drop(state);
        permit.send(frame);
        Ok(())
    }

    /// Managed channels reject pressure immediately, like process_outgoing;
    /// plain channels preserve Tokio's waiting send semantics.
    pub async fn send(&self, raw: Bytes) -> Result<(), mpsc::error::SendError<Bytes>> {
        match self {
            Self::Plain(tx) => tx.send(raw).await,
            Self::Managed(_) => self
                .try_send(raw)
                .map_err(|e| mpsc::error::SendError(e.into_inner())),
        }
    }
    pub fn capacity(&self) -> usize {
        match self {
            Self::Plain(tx) => tx.capacity(),
            Self::Managed(q) => q.tx.capacity(),
        }
    }
    pub fn max_capacity(&self) -> usize {
        match self {
            Self::Plain(tx) => tx.max_capacity(),
            Self::Managed(q) => q.tx.max_capacity(),
        }
    }
    pub fn is_closed(&self) -> bool {
        match self {
            Self::Plain(tx) => tx.is_closed(),
            Self::Managed(q) => q.tx.is_closed(),
        }
    }
    pub fn same_channel(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Plain(a), Self::Plain(b)) => a.same_channel(b),
            (Self::Managed(a), Self::Managed(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn size(raw: &[u8]) -> u64 {
        raw.len() as u64 + 2
    }

    #[tokio::test]
    async fn reservations_survive_queue_movement_and_release_partial_or_dropped() {
        let (tx, mut rx, accounting) = byte_channel(2, 10, size);
        tx.try_send(Bytes::from_static(b"12345678")).unwrap();
        assert!(matches!(
            tx.try_send(Bytes::new()),
            Err(mpsc::error::TrySendError::Full(_))
        ));
        let frame = rx.recv().await.unwrap();
        assert_eq!(accounting.snapshot().buffered, 10);
        assert_eq!(tx.diagnostics().txbuffered, Some(10));
        assert_eq!(tx.diagnostics().txdrb, Some(2));
        let (plain, _) = mpsc::channel::<Bytes>(2);
        assert_eq!(InterfaceTx::from(plain).diagnostics().txbuffered, None);
        frame.lease.as_ref().unwrap().written(3);
        assert_eq!(accounting.snapshot().buffered, 7);
        tx.try_send(Bytes::from_static(b"x")).unwrap();
        drop(frame);
        assert_eq!(accounting.snapshot().buffered, 3);
        drop(rx);
        assert_eq!(accounting.snapshot().buffered, 0);
        assert_eq!(accounting.snapshot().sent, 3);
        assert_eq!(accounting.snapshot().dropped_frames, 1);
        assert_eq!(accounting.snapshot().dropped_bytes, 2);
        assert!(matches!(
            tx.try_send(Bytes::new()),
            Err(mpsc::error::TrySendError::Closed(_))
        ));
    }

    #[test]
    fn concurrent_admission_is_atomic_and_slot_failure_does_not_leak() {
        let (tx, rx, accounting) = byte_channel(100, 100, size);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let tx = tx.clone();
                scope.spawn(move || {
                    for _ in 0..100 {
                        let _ = tx.try_send(Bytes::from_static(b"12345678"));
                    }
                });
            }
        });
        assert_eq!(accounting.snapshot().buffered, 100);
        assert_eq!(accounting.snapshot().dropped_frames, 790);
        drop(rx);
        assert_eq!(accounting.snapshot().buffered, 0);
        let (tx, rx, accounting) = byte_channel(1, 100, size);
        tx.try_send(Bytes::new()).unwrap();
        assert!(tx.try_send(Bytes::new()).is_err());
        assert_eq!(accounting.snapshot().buffered, 2);
        assert_eq!(accounting.snapshot().dropped_frames, 1);
        drop(rx);
        assert_eq!(accounting.snapshot().buffered, 0);
    }

    #[tokio::test]
    async fn gating_rejects_without_reserving_and_plain_sender_is_unchanged() {
        let (tx, rx, accounting) = byte_channel(1, 10, size);
        accounting.set_gated(true);
        assert!(tx.send(Bytes::new()).await.is_err());
        assert_eq!(accounting.snapshot().buffered, 0);
        accounting.set_gated(false);
        tx.try_send(Bytes::new()).unwrap();
        assert!(tx.same_channel(&tx.clone()));
        drop(rx);
        let (plain, mut rx) = mpsc::channel(1);
        let plain = InterfaceTx::from(plain);
        assert!(!plain.same_channel(&tx));
        plain.send(Bytes::from_static(b"plain")).await.unwrap();
        assert_eq!(rx.recv().await.unwrap(), b"plain"[..]);
    }
}
