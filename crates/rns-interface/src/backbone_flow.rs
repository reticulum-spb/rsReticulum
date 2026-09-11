//! Python 1.5.2 Backbone egress policy, independent of socket/queue ownership.
//!
//! Not yet wired into the driver: callers must supply a complete encoded-byte
//! backlog, including queued frames, rather than only the current write batch.
//! This is separate from announce/path-request ingress and egress control.

use std::time::Duration;

pub const EVALUATE_INTERVAL: Duration = Duration::from_secs(1);
pub const MID_WATERMARK: u64 = 128 * 1024;
pub const HIGH_WATERMARK: u64 = 4 * 1024 * 1024;
pub const STALL_TICKS: u32 = 3;
pub const MAX_ETA: f64 = 10.0;
pub const RELEASE_ETA: f64 = 5.0;
pub const DEAD_TIME: Duration = Duration::from_secs(12);

/// One snapshot of a single connection's transmit buffer. Byte values include
/// HDLC framing/escaping. `sent` is cumulative successful socket writes.
#[derive(Debug, Clone, Copy)]
pub struct EgressSample {
    pub buffered: u64,
    pub sendable: u64,
    pub sent: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressDecision {
    Open,
    Gated,
    Disconnect,
}

/// Sampled once per second, like Python `_dp_ec_evaluate`. Create a new state
/// for each connection; timestamps are monotonic elapsed time from one origin.
#[derive(Debug)]
pub struct EgressController {
    previous_sent: u64,
    last_drain: Duration,
    zero_ticks: u32,
    stalled: bool,
}

impl EgressController {
    pub fn new(now: Duration, sent: u64) -> Self {
        Self {
            previous_sent: sent,
            last_drain: now,
            zero_ticks: 0,
            stalled: false,
        }
    }

    pub fn stalled(&self) -> bool {
        self.stalled
    }

    /// `Disconnect` is terminal: the caller must tear down the connection,
    /// not continue sampling this instance. No socket side effects occur here.
    pub fn evaluate(&mut self, now: Duration, sample: EgressSample) -> EgressDecision {
        let drained = sample.sent.saturating_sub(self.previous_sent);
        self.previous_sent = sample.sent;
        if sample.buffered == 0 || sample.sendable == 0 {
            self.zero_ticks = 0;
            self.last_drain = now;
            self.stalled = false;
            return EgressDecision::Open;
        }
        // Python checks this BEFORE applying progress in the current sample.
        if now.saturating_sub(self.last_drain) >= DEAD_TIME {
            return EgressDecision::Disconnect;
        }
        if drained > 0 {
            self.last_drain = now;
            self.zero_ticks = 0;
            // Deliberately use the fixed evaluation interval, not wall-clock
            // delta: this preserves Python's ETA under delayed scheduling.
            let rate = drained as f64 / EVALUATE_INTERVAL.as_secs_f64();
            let eta = sample.buffered as f64 / rate;
            if sample.buffered > MID_WATERMARK && eta > MAX_ETA {
                self.stalled = true;
            } else if eta < RELEASE_ETA || sample.buffered <= MID_WATERMARK {
                self.stalled = false;
            }
        } else if sample.buffered > MID_WATERMARK {
            self.zero_ticks = self.zero_ticks.saturating_add(1);
            if self.zero_ticks >= STALL_TICKS {
                self.stalled = true;
            }
        } else {
            self.zero_ticks = 0;
            self.stalled = false;
        }
        if self.stalled {
            EgressDecision::Gated
        } else {
            EgressDecision::Open
        }
    }

    /// Python process_outgoing + TransmitBuffer.append admission predicate.
    /// This does not reserve space: the eventual queue owner must perform
    /// this check and append atomically, and account for rejected frames.
    pub fn permits_frame(&self, buffered: u64, encoded_len: u64, limit: u64) -> bool {
        !self.stalled
            && buffered
                .checked_add(encoded_len)
                .is_some_and(|n| n <= limit)
    }
}

#[cfg(test)]
#[path = "backbone_flow_tests.rs"]
mod tests;
