//! Process-wide byte admission. A reservation belongs to the Bytes allocation,
//! so slicing/cloning and moving between queues cannot release it prematurely.
use bytes::Bytes;
use std::sync::atomic::{AtomicUsize, Ordering};

pub static INGRESS: Budget = Budget::new(8 * 1024 * 1024);
pub static EGRESS: Budget = Budget::new(16 * 1024 * 1024);

pub struct Budget {
    used: AtomicUsize,
    limit: usize,
    pub dropped: AtomicUsize,
}
impl Budget {
    pub const fn new(limit: usize) -> Self {
        Self {
            used: AtomicUsize::new(0),
            limit,
            dropped: AtomicUsize::new(0),
        }
    }
    pub fn copy(&'static self, raw: &[u8]) -> Option<Bytes> {
        // Include allocation/queue bookkeeping, including zero-length frames.
        let size = raw.len().checked_add(128)?;
        if self
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |used| {
                used.checked_add(size).filter(|n| *n <= self.limit)
            })
            .is_err()
        {
            let dropped = self.dropped.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
            if dropped.is_power_of_two() {
                tracing::warn!(
                    limit_bytes = self.limit,
                    dropped,
                    "packet memory budget exhausted"
                );
            }
            return None;
        }
        Some(Bytes::from_owner(Allocation {
            data: raw.to_vec().into_boxed_slice(),
            budget: self,
            size,
        }))
    }
}
struct Allocation {
    data: Box<[u8]>,
    budget: &'static Budget,
    size: usize,
}
impl AsRef<[u8]> for Allocation {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}
impl Drop for Allocation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.size, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reservation_survives_slice_and_last_clone() {
        static B: Budget = Budget::new(256);
        let a = B.copy(&[7; 128]).unwrap();
        let b = a.slice(0..1);
        drop(a);
        assert!(B.copy(&[1]).is_none());
        drop(b);
        assert!(B.copy(&[2; 128]).is_some());
    }
}
