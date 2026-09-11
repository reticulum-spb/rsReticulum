//! Dataplane ingress policy for Backbone, not yet connected to socket readers.
//! Input order is registration order (Python dict order), not hash-map order.
//! Peers must contain only eligible Backbone connections, not local clients.
//! Announce/path-request ingress control is a separate mechanism.

use std::time::Duration;

pub const INTERVAL: Duration = Duration::from_millis(250);
pub const RECEIVE_BUFFER: usize = 32768;
pub const INTERFACE_HEADROOM: usize = 32;
pub const PENALTY: f64 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Watermarks {
    pub high: usize,
    pub mid: usize,
    pub low: usize,
    pub immediate: usize,
}

impl Watermarks {
    pub fn for_data_capacity(capacity: usize) -> Self {
        // Preserve Python int(float_percentage * dql), including rounding,
        // and InboundQueues' separate minimum immediate-trigger depth.
        let high = ((0.90 * capacity as f64) as usize).max(4);
        Self {
            high,
            mid: ((0.68 * capacity as f64) as usize).max(2),
            low: (0.10 * capacity as f64) as usize,
            immediate: high.max(128),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PeerSample {
    pub id: u64,
    /// Frames delivered by the driver this sampling period (all classes).
    pub packets: u64,
    /// Socket bytes read this sampling period, including framing.
    pub bytes: u64,
    pub gated: bool,
    pub hold_until: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IngressAction {
    Gate { id: u64, hold: Duration },
    Release { id: u64 },
}

pub struct IngressPolicy {
    pub watermarks: Watermarks,
    snapshot: Duration,
}

impl IngressPolicy {
    pub fn new(data_capacity: usize, now: Duration) -> Self {
        Self {
            watermarks: Watermarks::for_data_capacity(data_capacity),
            snapshot: now,
        }
    }

    /// Evaluate one periodic snapshot. A returned action must be applied by
    /// the caller; no gate/hold state is changed optimistically here. Counters
    /// reset every periodic pass, including passes that select no action.
    pub fn periodic(
        &mut self,
        depth: usize,
        now: Duration,
        peers: &mut [PeerSample],
    ) -> Option<IngressAction> {
        let action = if depth > self.watermarks.mid {
            self.select_gate(now, peers, false)
        } else if depth < self.watermarks.low {
            peers
                .iter()
                .find(|p| p.gated && p.hold_until.is_none_or(|until| now >= until))
                .map(|p| IngressAction::Release { id: p.id })
        } else {
            None
        };
        self.snapshot = now;
        for peer in peers {
            peer.bytes = 0;
            peer.packets = 0;
        }
        action
    }

    /// Call BEFORE a DATA queue append, even if that append will overflow.
    /// Immediate evaluation neither releases gates nor resets sample counters.
    pub fn immediate(
        &self,
        depth: usize,
        now: Duration,
        peers: &[PeerSample],
    ) -> Option<IngressAction> {
        if depth < self.watermarks.immediate {
            return None;
        }
        self.select_gate(now, peers, true)
    }

    fn select_gate(
        &self,
        now: Duration,
        peers: &[PeerSample],
        immediate: bool,
    ) -> Option<IngressAction> {
        // Strict greater preserves the first producer on a packet-count tie.
        // Python does not skip an already-gated top producer to gate a runner-up.
        let mut selected: Option<&PeerSample> = None;
        for peer in peers {
            if peer.packets > 0 && selected.is_none_or(|old| peer.packets > old.packets) {
                selected = Some(peer);
            }
        }
        let selected = selected?;
        if selected.gated {
            return None;
        }
        let elapsed = now.saturating_sub(self.snapshot).as_secs_f64();
        let span = if immediate {
            elapsed
        } else {
            elapsed.max(0.001)
        };
        let bytes = peers.iter().map(|p| p.bytes as f64).sum::<f64>();
        // Python immediate raises on zero span/zero available bytes. Safely
        // defer such an invalid sample instead of tearing down the actor.
        if immediate && (span == 0.0 || bytes == 0.0) {
            return None;
        }
        let available = bytes / span;
        let allocation = 1.0 / (peers.len().max(INTERFACE_HEADROOM) as f64 * PENALTY);
        let allocated = available * allocation;
        let denominator = if immediate {
            allocated
        } else {
            allocated.max(1.0)
        };
        let hold = Duration::try_from_secs_f64((available / denominator) * span).ok()?;
        Some(IngressAction::Gate {
            id: selected.id,
            hold,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn peer(id: u64, packets: u64, bytes: u64) -> PeerSample {
        PeerSample {
            id,
            packets,
            bytes,
            gated: false,
            hold_until: None,
        }
    }

    #[test]
    fn watermarks_match_python_including_tiny_queue_trap() {
        assert_eq!(
            Watermarks::for_data_capacity(1024),
            Watermarks {
                high: 921,
                mid: 696,
                low: 102,
                immediate: 921
            }
        );
        assert_eq!(
            Watermarks::for_data_capacity(8),
            Watermarks {
                high: 7,
                mid: 5,
                low: 0,
                immediate: 128
            }
        );
        let mut policy = IngressPolicy::new(8, Duration::ZERO);
        let mut peers = [peer(1, 0, 0)];
        peers[0].gated = true;
        assert_eq!(
            policy.periodic(0, Duration::from_secs(60), &mut peers),
            None,
            "literal Python low=0 cannot release even an empty queue; resolve before integration"
        );
    }

    #[test]
    fn periodic_selects_largest_stable_producer_and_resets_counters() {
        let mut policy = IngressPolicy::new(1024, Duration::ZERO);
        let mut peers = [peer(10, 3, 100), peer(20, 7, 200), peer(30, 7, 300)];
        assert_eq!(
            policy.periodic(697, INTERVAL, &mut peers),
            Some(IngressAction::Gate {
                id: 20,
                hold: Duration::from_secs(12)
            })
        );
        assert!(peers.iter().all(|p| p.packets == 0 && p.bytes == 0));
        assert!(!peers[1].gated, "caller applies successful gate");
    }

    #[test]
    fn strict_boundaries_hold_expiry_and_one_release_per_tick() {
        let mut policy = IngressPolicy::new(1024, Duration::ZERO);
        let mut peers = [peer(1, 10, 100), peer(2, 1, 10)];
        assert_eq!(policy.periodic(696, INTERVAL, &mut peers), None);
        for p in &mut peers {
            p.gated = true;
            p.hold_until = Some(Duration::from_secs(12));
        }
        assert_eq!(
            policy.periodic(101, Duration::from_secs(11), &mut peers),
            None
        );
        assert_eq!(
            policy.periodic(102, Duration::from_secs(12), &mut peers),
            None
        );
        assert_eq!(
            policy.periodic(101, Duration::from_secs(12), &mut peers),
            Some(IngressAction::Release { id: 1 })
        );
    }

    #[test]
    fn gated_top_producer_does_not_fall_through_and_immediate_keeps_samples() {
        let policy = IngressPolicy::new(1024, Duration::ZERO);
        let mut peers = [peer(1, 10, 100), peer(2, 1, 10)];
        assert_eq!(policy.immediate(920, INTERVAL, &peers), None);
        assert_eq!(
            policy.immediate(921, INTERVAL, &peers),
            Some(IngressAction::Gate {
                id: 1,
                hold: Duration::from_secs(12)
            })
        );
        assert_eq!(peers[0].packets, 10);
        peers[0].gated = true;
        assert_eq!(policy.immediate(921, INTERVAL, &peers), None);
    }

    #[test]
    fn low_byte_rate_clamps_periodic_but_not_immediate_hold() {
        let mut policy = IngressPolicy::new(1024, Duration::ZERO);
        let mut peers = [peer(1, 1, 1)];
        assert_eq!(
            policy.immediate(921, INTERVAL, &peers),
            Some(IngressAction::Gate {
                id: 1,
                hold: Duration::from_secs(12)
            })
        );
        assert_eq!(
            policy.periodic(697, INTERVAL, &mut peers),
            Some(IngressAction::Gate {
                id: 1,
                hold: Duration::from_secs(1)
            })
        );
        assert_eq!(policy.immediate(921, INTERVAL, &[peer(1, 1, 1)]), None);
        assert_eq!(
            policy.immediate(921, Duration::from_secs(1), &[peer(1, 1, 0)]),
            None
        );
    }
}
