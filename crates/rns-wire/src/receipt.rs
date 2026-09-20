//! Tracks sent packets until they are delivered, time out, or are culled.

use std::time::{Duration, Instant};

pub use crate::constants::{EXPL_LENGTH, IMPL_LENGTH};

/// Lifecycle state of a tracked packet.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[cfg_attr(feature = "memory-profile", derive(allocative::Allocative))]
pub enum ReceiptStatus {
    Sent = 0x01,
    Delivered = 0x02,
    Failed = 0x00,
    Culled = 0xFF,
    /// Resource transfer in progress.
    Receiving = 0x06,
}

/// Tracks a sent packet until it is delivered, times out, or is culled.
#[allow(missing_docs)]
#[cfg_attr(feature = "memory-profile", derive(allocative::Allocative))]
pub struct PacketReceipt {
    pub hash: [u8; 32],
    pub truncated_hash: [u8; 16],
    pub status: ReceiptStatus,
    pub sent_at: Instant,
    pub concluded_at: Option<Instant>,
    pub timeout: Option<Duration>,
    pub callbacks: ReceiptCallbacks,
}

/// Optional callbacks invoked when a receipt transitions out of `Sent`.
#[allow(missing_docs)]
#[derive(Default)]
#[cfg_attr(feature = "memory-profile", derive(allocative::Allocative))]
pub struct ReceiptCallbacks {
    #[cfg_attr(feature = "memory-profile", allocative(skip))]
    pub delivery: Option<ReceiptCallback>,
    #[cfg_attr(feature = "memory-profile", allocative(skip))]
    pub timeout: Option<ReceiptCallback>,
}

/// Callback invoked once when a packet receipt reaches a terminal state.
pub type ReceiptCallback = Box<dyn FnOnce(&PacketReceipt) + Send>;

impl PacketReceipt {
    /// Create a new receipt in the `Sent` state, stamping `sent_at` to now.
    pub fn new(hash: [u8; 32], truncated_hash: [u8; 16], timeout: Option<Duration>) -> Self {
        Self {
            hash,
            truncated_hash,
            status: ReceiptStatus::Sent,
            sent_at: Instant::now(),
            concluded_at: None,
            timeout,
            callbacks: ReceiptCallbacks::default(),
        }
    }

    /// Transition to `Delivered` and fire the delivery callback.
    pub fn deliver(&mut self) {
        self.status = ReceiptStatus::Delivered;
        self.concluded_at = Some(Instant::now());
        if let Some(cb) = self.callbacks.delivery.take() {
            cb(self);
        }
    }

    /// Transition to `Failed` and fire the timeout callback.
    pub fn fail(&mut self) {
        self.status = ReceiptStatus::Failed;
        self.concluded_at = Some(Instant::now());
        if let Some(cb) = self.callbacks.timeout.take() {
            cb(self);
        }
    }

    /// Transition to `Culled` without firing any callback.
    pub fn cull(&mut self) {
        self.status = ReceiptStatus::Culled;
        self.concluded_at = Some(Instant::now());
    }

    /// Measured round-trip time, if this receipt has concluded.
    pub fn get_rtt(&self) -> Option<Duration> {
        self.concluded_at.map(|t| t.duration_since(self.sent_at))
    }

    /// Whether the configured timeout (if any) has elapsed since `sent_at`.
    pub fn is_timed_out(&self) -> bool {
        match self.timeout {
            Some(timeout) => self.sent_at.elapsed() > timeout,
            None => false,
        }
    }

    /// Register a callback to run once on delivery.
    pub fn set_delivery_callback(&mut self, cb: impl FnOnce(&PacketReceipt) + Send + 'static) {
        self.callbacks.delivery = Some(Box::new(cb));
    }

    /// Register a callback to run once on timeout.
    pub fn set_timeout_callback(&mut self, cb: impl FnOnce(&PacketReceipt) + Send + 'static) {
        self.callbacks.timeout = Some(Box::new(cb));
    }

    /// Validate a PROOF body and mark delivered on match.
    /// Explicit (96): embedded hash must equal receipt hash. Implicit (64):
    /// signature over the receipt hash. `verify` is Ed25519 verify with the
    /// expected identity key.
    pub fn validate_proof<F>(&mut self, proof: &[u8], verify: F) -> bool
    where
        F: FnOnce(&[u8], &[u8]) -> bool,
    {
        if proof.len() == EXPL_LENGTH {
            let proof_hash = &proof[..32];
            let signature = &proof[32..96];
            if proof_hash == self.hash && verify(signature, &self.hash) {
                self.status = ReceiptStatus::Delivered;
                self.concluded_at = Some(Instant::now());
                if let Some(cb) = self.callbacks.delivery.take() {
                    cb(self);
                }
                return true;
            }
        } else if proof.len() == IMPL_LENGTH {
            let signature = &proof[..64];
            if verify(signature, &self.hash) {
                self.status = ReceiptStatus::Delivered;
                self.concluded_at = Some(Instant::now());
                if let Some(cb) = self.callbacks.delivery.take() {
                    cb(self);
                }
                return true;
            }
        }
        false
    }

    /// If the timeout has elapsed while still in `Sent`, transition to
    /// `Failed` and fire the timeout callback. Returns whether a timeout fired.
    pub fn check_timeout(&mut self) -> bool {
        if self.is_timed_out() && self.status == ReceiptStatus::Sent {
            self.status = ReceiptStatus::Failed;
            self.concluded_at = Some(Instant::now());
            if let Some(cb) = self.callbacks.timeout.take() {
                cb(self);
            }
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let receipt = PacketReceipt::new([0; 32], [0; 16], Some(Duration::from_secs(10)));
        assert_eq!(receipt.status, ReceiptStatus::Sent);
        assert!(receipt.concluded_at.is_none());
        assert!(receipt.get_rtt().is_none());
    }

    #[test]
    fn test_deliver() {
        let mut receipt = PacketReceipt::new([0; 32], [0; 16], None);
        receipt.deliver();
        assert_eq!(receipt.status, ReceiptStatus::Delivered);
        assert!(receipt.concluded_at.is_some());
        assert!(receipt.get_rtt().is_some());
    }

    #[test]
    fn test_fail() {
        let mut receipt = PacketReceipt::new([0; 32], [0; 16], None);
        receipt.fail();
        assert_eq!(receipt.status, ReceiptStatus::Failed);
    }
}
