//! Fixed-size interval statistics; never retain individual requests or payloads.
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct Timing {
    pub count: u64,
    total_us: u64,
    max_us: u64,
    // Upper bounds 1, 2, 4, ... 262144 ms, then an overflow bucket.
    buckets: [u64; 20],
}

impl Timing {
    pub fn record(&mut self, elapsed: Duration) {
        let us = elapsed.as_micros().min(u64::MAX as u128) as u64;
        self.count = self.count.saturating_add(1);
        self.total_us = self.total_us.saturating_add(us);
        self.max_us = self.max_us.max(us);
        let ms = us.div_ceil(1000).max(1);
        let bucket = (u64::BITS - (ms - 1).leading_zeros()) as usize;
        self.buckets[bucket.min(19)] += 1;
    }

    fn percentile_ms(&self, percent: u64) -> u64 {
        let target = self.count.saturating_mul(percent).div_ceil(100);
        let mut count = 0;
        for (index, n) in self.buckets.iter().enumerate() {
            count += n;
            if count >= target {
                return if index == 19 {
                    self.max_us.div_ceil(1000)
                } else {
                    (1_u64 << index).min(self.max_us.div_ceil(1000))
                };
            }
        }
        self.max_us.div_ceil(1000)
    }

    pub fn report(&mut self, operation: &'static str, phase: &'static str) {
        if self.count == 0 {
            return;
        }
        tracing::info!(
            operation,
            phase,
            count = self.count,
            total_us = self.total_us,
            max_us = self.max_us,
            p50_upper_ms = self.percentile_ms(50),
            p95_upper_ms = self.percentile_ms(95),
            p99_upper_ms = self.percentile_ms(99),
            "transport storage timing summary"
        );
        *self = Self::default();
    }
}

#[derive(Default)]
struct Operation {
    name: Option<&'static str>,
    queue: Timing,
    execution: Timing,
    failed: u64,
}

pub(super) struct WorkerMetrics {
    operations: [Operation; 16],
    since: Instant,
}

impl Default for WorkerMetrics {
    fn default() -> Self {
        Self {
            operations: Default::default(),
            since: Instant::now(),
        }
    }
}

impl WorkerMetrics {
    pub fn record(
        &mut self,
        name: &'static str,
        queue: Duration,
        execution: Duration,
        failed: bool,
    ) {
        let index = self
            .operations
            .iter()
            .position(|o| o.name == Some(name))
            .or_else(|| self.operations.iter().position(|o| o.name.is_none()));
        if let Some(index) = index {
            let op = &mut self.operations[index];
            op.name = Some(name);
            op.queue.record(queue);
            op.execution.record(execution);
            op.failed += u64::from(failed);
        }
        if self.since.elapsed() >= Duration::from_secs(60) {
            self.report();
        }
    }

    fn report(&mut self) {
        for op in &mut self.operations {
            if let Some(name) = op.name {
                if op.execution.count > 0 {
                    tracing::info!(
                        operation = name,
                        failed = op.failed,
                        "transport storage result summary"
                    );
                }
                op.queue.report(name, "queue");
                op.execution.report(name, "execution");
                op.failed = 0;
            }
        }
        self.since = Instant::now();
    }
}

impl Drop for WorkerMetrics {
    fn drop(&mut self) {
        self.report();
    }
}

#[cfg(feature = "sqlite")]
pub(super) struct Transactions {
    since: Instant,
    sql: Timing,
    commit: Timing,
    items: u64,
    bytes: u64,
    max_items: usize,
    max_bytes: usize,
    failures: u64,
    name: &'static str,
}

#[cfg(feature = "sqlite")]
impl Transactions {
    pub fn new(name: &'static str) -> Self {
        Self {
            since: Instant::now(),
            sql: Timing::default(),
            commit: Timing::default(),
            items: 0,
            bytes: 0,
            max_items: 0,
            max_bytes: 0,
            failures: 0,
            name,
        }
    }

    pub fn commit(
        &mut self,
        tx: rusqlite::Transaction<'_>,
        started: Instant,
        items: usize,
        bytes: usize,
    ) -> super::Result<()> {
        self.sql.record(started.elapsed());
        let started = Instant::now();
        let result = tx.commit();
        // Includes SQLite's automatic checkpoint. It is not a pure fsync timer.
        self.commit.record(started.elapsed());
        self.items += items as u64;
        self.bytes += bytes as u64;
        self.max_items = self.max_items.max(items);
        self.max_bytes = self.max_bytes.max(bytes);
        self.failures += u64::from(result.is_err());
        if self.since.elapsed() >= Duration::from_secs(60) {
            self.report();
        }
        Ok(result?)
    }

    fn report(&mut self) {
        if self.commit.count == 0 {
            return;
        }
        tracing::info!(
            operation = self.name,
            transactions = self.commit.count,
            items = self.items,
            allocated_bytes = self.bytes,
            max_items = self.max_items,
            max_allocated_bytes = self.max_bytes,
            commit_failures = self.failures,
            "SQLite transaction summary"
        );
        self.sql.report(self.name, "sql_before_commit");
        self.commit
            .report(self.name, "commit_including_autocheckpoint");
        self.items = 0;
        self.bytes = 0;
        self.max_items = 0;
        self.max_bytes = 0;
        self.failures = 0;
        self.since = Instant::now();
    }
}

#[cfg(feature = "sqlite")]
impl Drop for Transactions {
    fn drop(&mut self) {
        self.report();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_include_fast_operations_and_overflow() {
        let mut timing = Timing::default();
        for _ in 0..95 {
            timing.record(Duration::from_micros(500));
        }
        for _ in 0..4 {
            timing.record(Duration::from_millis(170));
        }
        timing.record(Duration::from_secs(1000));
        assert_eq!(timing.count, 100);
        assert_eq!(timing.percentile_ms(50), 1);
        assert_eq!(timing.percentile_ms(95), 1);
        assert_eq!(timing.percentile_ms(99), 256);
        assert_eq!(timing.percentile_ms(100), 1_000_000);
    }
}
