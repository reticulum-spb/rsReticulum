//! Actor-owned equivalent of Python 1.5.2 `Transport.InboundQueues`.
//!
//! Scheduling is strict priority, not round robin: sustained data traffic may
//! starve the lower classes, as in Python. Control messages must use a separate
//! actor admission path. This container does not classify or authenticate raw
//! packets, provide async wakeups, or apply Backbone dataplane throttling.

use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum TrafficClass {
    Data = 0,
    Announce = 1,
    PathRequest = 2,
    IngressLimited = 3,
}

impl TrafficClass {
    pub const ALL: [Self; 4] = [
        Self::Data,
        Self::Announce,
        Self::PathRequest,
        Self::IngressLimited,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboundQueueLimits([usize; 4]);

impl Default for InboundQueueLimits {
    fn default() -> Self {
        Self([1024, 128, 128, 8])
    }
}

impl InboundQueueLimits {
    /// Configuration accepts positive queue sizes only. No eager allocation is
    /// performed even for large configured limits. Reject total-size overflow.
    pub fn new(sizes: [usize; 4]) -> Option<Self> {
        if sizes.contains(&0)
            || sizes
                .iter()
                .try_fold(0usize, |sum, size| sum.checked_add(*size))
                .is_none()
        {
            None
        } else {
            Some(Self(sizes))
        }
    }

    pub fn sizes(self) -> [usize; 4] {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboundQueueSnapshot {
    pub total: usize,
    pub heights: [usize; 4],
    pub dropped: [u64; 4],
}

/// Single-owner storage: snapshot reads and queue mutations are serialized by
/// the actor, with no extra mutex or unbounded intermediate buffer.
#[derive(Debug)]
pub struct InboundQueues<T> {
    queues: [VecDeque<T>; 4],
    limits: InboundQueueLimits,
    dropped: [u64; 4],
}

impl<T> InboundQueues<T> {
    pub fn new(limits: InboundQueueLimits) -> Self {
        Self {
            queues: std::array::from_fn(|_| VecDeque::new()),
            limits,
            dropped: [0; 4],
        }
    }

    /// Drop-tail admission. An overflow returns ownership to the caller;
    /// existing packets and other classes are untouched.
    pub fn try_push(&mut self, class: TrafficClass, item: T) -> Result<(), T> {
        let index = class as usize;
        if self.queues[index].len() >= self.limits.0[index] {
            self.dropped[index] = self.dropped[index].saturating_add(1);
            return Err(item);
        }
        self.queues[index].push_back(item);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<T> {
        self.queues.iter_mut().find_map(VecDeque::pop_front)
    }

    /// Remove stale packets on interface deregistration. Administrative removal
    /// does not increment overflow counters. FIFO order is preserved.
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        for queue in &mut self.queues {
            queue.retain(|item| keep(item));
        }
    }

    pub fn snapshot(&self) -> InboundQueueSnapshot {
        let heights = std::array::from_fn(|i| self.queues[i].len());
        InboundQueueSnapshot {
            total: heights.iter().sum(),
            heights,
            dropped: self.dropped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_limit_validation() {
        assert_eq!(InboundQueueLimits::default().sizes(), [1024, 128, 128, 8]);
        assert!(InboundQueueLimits::new([1; 4]).is_some());
        for i in 0..4 {
            let mut sizes = [1; 4];
            sizes[i] = 0;
            assert!(InboundQueueLimits::new(sizes).is_none());
        }
        assert!(InboundQueueLimits::new([usize::MAX; 4]).is_none());
    }

    #[test]
    fn strict_priority_and_fifo_with_independent_drop_tail() {
        let mut queues = InboundQueues::new(InboundQueueLimits::new([2; 4]).unwrap());
        for class in TrafficClass::ALL.into_iter().rev() {
            for item in 0..2 {
                assert_eq!(queues.try_push(class, (class, item)), Ok(()));
            }
            assert_eq!(queues.try_push(class, (class, 2)), Err((class, 2)));
        }
        assert_eq!(
            queues.snapshot(),
            InboundQueueSnapshot {
                total: 8,
                heights: [2; 4],
                dropped: [1; 4]
            }
        );
        for class in TrafficClass::ALL {
            for item in 0..2 {
                assert_eq!(queues.pop(), Some((class, item)));
            }
        }
        assert_eq!(queues.pop(), None);
        assert_eq!(
            queues.snapshot(),
            InboundQueueSnapshot {
                total: 0,
                heights: [0; 4],
                dropped: [1; 4]
            }
        );
    }

    #[test]
    fn newly_arrived_data_preempts_waiting_lower_classes() {
        let mut queues = InboundQueues::new(InboundQueueLimits::default());
        queues.try_push(TrafficClass::IngressLimited, 1).unwrap();
        queues.try_push(TrafficClass::PathRequest, 2).unwrap();
        queues.try_push(TrafficClass::Announce, 3).unwrap();
        for value in 4..100 {
            queues.try_push(TrafficClass::Data, value).unwrap();
            assert_eq!(queues.pop(), Some(value));
        }
        assert_eq!(
            [queues.pop(), queues.pop(), queues.pop()],
            [Some(3), Some(2), Some(1)]
        );
    }

    #[test]
    fn interface_removal_preserves_order_and_does_not_count_as_overflow() {
        let mut queues = InboundQueues::new(InboundQueueLimits::default());
        for class in TrafficClass::ALL {
            for interface in [1, 2, 3, 2] {
                queues.try_push(class, (class, interface)).unwrap();
            }
        }
        queues.retain(|(_, interface)| *interface != 2);
        assert_eq!(queues.snapshot().heights, [2; 4]);
        assert_eq!(queues.snapshot().dropped, [0; 4]);
        for class in TrafficClass::ALL {
            assert_eq!(queues.pop(), Some((class, 1)));
            assert_eq!(queues.pop(), Some((class, 3)));
        }
    }

    #[test]
    fn sustained_overload_remains_bounded_and_counts_each_rejection() {
        let mut queues = InboundQueues::new(InboundQueueLimits::new([3; 4]).unwrap());
        for value in 0..10_000 {
            for class in TrafficClass::ALL {
                let _ = queues.try_push(class, value);
            }
        }
        assert_eq!(
            queues.snapshot(),
            InboundQueueSnapshot {
                total: 12,
                heights: [3; 4],
                dropped: [9997; 4]
            }
        );
    }
}
