use std::collections::HashMap;

use crate::messages::InterfaceId;

#[derive(Debug, Default, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct PacketStats {
    pub rxpps: f64,
    pub txpps: f64,
}

/// Actor ingress / successful egress admissions on external interfaces only.
pub struct PacketRateCounter {
    pub rx: u64,
    pub tx: u64,
    sampled_at: std::time::Instant,
    stats: PacketStats,
}

impl Default for PacketRateCounter {
    fn default() -> Self {
        Self {
            rx: 0,
            tx: 0,
            sampled_at: std::time::Instant::now(),
            stats: PacketStats::default(),
        }
    }
}

impl PacketRateCounter {
    pub fn sample(&mut self) -> PacketStats {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.sampled_at).as_secs_f64();
        if elapsed >= 1.0 {
            self.stats = PacketStats {
                rxpps: self.rx as f64 / elapsed,
                txpps: self.tx as f64 / elapsed,
            };
            self.rx = 0;
            self.tx = 0;
            self.sampled_at = now;
        }
        self.stats
    }
}

/// Control-plane packet counts and bytes, excluding IFAC and driver framing.
#[derive(Default)]
pub struct ByteRateSampler {
    previous: Option<(std::time::Instant, u64, u64)>,
    pub rates: (u64, u64),
}

impl ByteRateSampler {
    pub fn sample(&mut self, rx: u64, tx: u64) {
        let now = std::time::Instant::now();
        if let Some((previous, old_rx, old_tx)) = self.previous {
            let elapsed = now.duration_since(previous).as_secs_f64();
            if elapsed > 0.0 {
                self.rates = (
                    (rx.saturating_sub(old_rx) as f64 * 8.0 / elapsed) as u64,
                    (tx.saturating_sub(old_tx) as f64 * 8.0 / elapsed) as u64,
                );
            }
        }
        self.previous = Some((now, rx, tx));
    }
}

/// Control-plane packet counts and bytes, excluding IFAC and driver framing.
/// Owned by an interface registration, not by a destination or packet hash.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ControlTraffic {
    pub arxb: u64,
    pub atxb: u64,
    pub arxc: u64,
    pub atxc: u64,
    pub prxb: u64,
    pub ptxb: u64,
    pub prxc: u64,
    pub ptxc: u64,
    /// Last sampled speeds in bits/second, not bytes/second.
    pub arxs: f64,
    pub atxs: f64,
    pub prxs: f64,
    pub ptxs: f64,
}

/// Bounded monotonic sampler. The first observation establishes a baseline,
/// matching Python's transport_traffic_counter initialization.
#[derive(Debug, Default)]
pub(crate) struct ControlTrafficSampler {
    previous: Option<(std::time::Instant, [u64; 4])>,
}

impl ControlTrafficSampler {
    pub(crate) fn sample(&mut self, traffic: &mut ControlTraffic, now: std::time::Instant) {
        let bytes = [traffic.arxb, traffic.atxb, traffic.prxb, traffic.ptxb];
        if let Some((timestamp, previous)) = self.previous {
            let Some(elapsed) = now.checked_duration_since(timestamp) else {
                return;
            };
            if elapsed.is_zero() {
                return;
            }
            let rates = std::array::from_fn::<_, 4, _>(|i| {
                // A reset must never underflow into a huge apparent rate.
                bytes[i].saturating_sub(previous[i]) as f64 * 8.0 / elapsed.as_secs_f64()
            });
            [traffic.arxs, traffic.atxs, traffic.prxs, traffic.ptxs] = rates;
        }
        self.previous = Some((now, bytes));
    }
}

impl ControlTraffic {
    pub fn total(values: impl IntoIterator<Item = Self>) -> Self {
        let mut sum = Self::default();
        for value in values {
            sum.arxb = sum.arxb.saturating_add(value.arxb);
            sum.atxb = sum.atxb.saturating_add(value.atxb);
            sum.arxc = sum.arxc.saturating_add(value.arxc);
            sum.atxc = sum.atxc.saturating_add(value.atxc);
            sum.prxb = sum.prxb.saturating_add(value.prxb);
            sum.ptxb = sum.ptxb.saturating_add(value.ptxb);
            sum.prxc = sum.prxc.saturating_add(value.prxc);
            sum.ptxc = sum.ptxc.saturating_add(value.ptxc);
            sum.arxs += value.arxs;
            sum.atxs += value.atxs;
            sum.prxs += value.prxs;
            sum.ptxs += value.ptxs;
        }
        sum
    }
    pub fn received_announce(&mut self, size: usize) {
        self.arxc = self.arxc.saturating_add(1);
        self.arxb = self.arxb.saturating_add(size as u64);
    }

    pub fn sent_announce(&mut self, size: usize) {
        self.atxc = self.atxc.saturating_add(1);
        self.atxb = self.atxb.saturating_add(size as u64);
    }

    pub fn received_path_request(&mut self, size: usize) {
        self.prxc = self.prxc.saturating_add(1);
        self.prxb = self.prxb.saturating_add(size as u64);
    }

    pub fn sent_path_request(&mut self, size: usize) {
        self.ptxc = self.ptxc.saturating_add(1);
        self.ptxb = self.ptxb.saturating_add(size as u64);
    }
}

#[derive(Debug, Clone, Default)]
pub struct InterfaceTraffic {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    /// Previous sample, retained so `update_speeds` can compute a delta
    /// without keeping a ring buffer of samples.
    pub rx_bytes_prev: u64,
    pub tx_bytes_prev: u64,
    /// Bytes per second, over the last `update_speeds` interval.
    pub rx_speed: f64,
    pub tx_speed: f64,
}

pub struct TrafficCounter {
    interfaces: HashMap<InterfaceId, InterfaceTraffic>,
}

impl TrafficCounter {
    pub fn new() -> Self {
        Self {
            interfaces: HashMap::new(),
        }
    }

    pub fn record_rx(&mut self, interface_id: InterfaceId, bytes: u64) {
        let entry = self.interfaces.entry(interface_id).or_default();
        entry.rx_bytes += bytes;
    }

    pub fn record_tx(&mut self, interface_id: InterfaceId, bytes: u64) {
        let entry = self.interfaces.entry(interface_id).or_default();
        entry.tx_bytes += bytes;
    }

    /// Compute per-second speeds from the delta since the last call. Intended
    /// to run on a 1 Hz tick so the delta equals bytes-per-second directly.
    pub fn update_speeds(&mut self) {
        for entry in self.interfaces.values_mut() {
            entry.rx_speed = (entry.rx_bytes - entry.rx_bytes_prev) as f64;
            entry.tx_speed = (entry.tx_bytes - entry.tx_bytes_prev) as f64;
            entry.rx_bytes_prev = entry.rx_bytes;
            entry.tx_bytes_prev = entry.tx_bytes;
        }
    }

    pub fn get(&self, interface_id: &InterfaceId) -> Option<&InterfaceTraffic> {
        self.interfaces.get(interface_id)
    }

    pub fn all(&self) -> &HashMap<InterfaceId, InterfaceTraffic> {
        &self.interfaces
    }
}

impl Default for TrafficCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{ControlTraffic, ControlTrafficSampler};

    #[test]
    fn packet_rates_use_elapsed_time_and_reset_sample() {
        let mut counter = super::PacketRateCounter::default();
        counter.sampled_at -= std::time::Duration::from_secs(2);
        counter.rx = 8;
        counter.tx = 4;
        let stats = counter.sample();
        assert!((stats.rxpps - 4.0).abs() < 0.05);
        assert!((stats.txpps - 2.0).abs() < 0.05);
        assert_eq!((counter.rx, counter.tx), (0, 0));
        assert_eq!(counter.sample().rxpps, stats.rxpps);
        let mut bytes = super::ByteRateSampler::default();
        bytes.previous = Some((
            std::time::Instant::now() - std::time::Duration::from_secs(2),
            100,
            200,
        ));
        bytes.sample(300, 600);
        assert!((799..=800).contains(&bytes.rates.0));
        assert!((1599..=1600).contains(&bytes.rates.1));
    }

    #[test]
    #[ignore = "requires local Python reference checkout"]
    fn control_rates_match_python_transport_expressions() {
        use std::time::{Duration, Instant};
        let mut now = Instant::now();
        let mut traffic = ControlTraffic::default();
        let mut sampler = ControlTrafficSampler::default();
        sampler.sample(&mut traffic, now);
        let mut command = std::process::Command::new(
            std::env::var("RNS_PYTHON_BIN").unwrap_or_else(|_| "/usr/bin/python3.11".into()),
        );
        command.args([
            "-B",
            "-c",
            include_str!("../tests/control_rates_reference.py"),
        ]);
        command.arg(
            std::env::var("RNS_PYTHON_ROOT").unwrap_or_else(|_| "/home/room/src/Reticulum".into()),
        );
        let mut expected = Vec::new();
        for (millis, bytes) in [
            (2500, [500, 250, 125, 25]),
            (1500, [0; 4]),
            (125, [1, 2, 3, 4]),
            (10000, [5000, 6000, 7000, 8000]),
        ] {
            traffic.received_announce(bytes[0]);
            traffic.sent_announce(bytes[1]);
            traffic.received_path_request(bytes[2]);
            traffic.sent_path_request(bytes[3]);
            now += Duration::from_millis(millis);
            sampler.sample(&mut traffic, now);
            expected.extend([traffic.arxs, traffic.atxs, traffic.prxs, traffic.ptxs]);
            command.arg(format!(
                "{},{},{},{},{}",
                millis as f64 / 1000.0,
                bytes[0],
                bytes[1],
                bytes[2],
                bytes[3]
            ));
        }
        let output = command.output().expect("start Python reference");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual: Vec<f64> = std::str::from_utf8(&output.stdout)
            .unwrap()
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn control_rates_use_elapsed_time_and_preserve_baseline_on_invalid_time() {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        let mut sampler = ControlTrafficSampler::default();
        let mut traffic = ControlTraffic {
            arxb: 1000,
            ..Default::default()
        };
        sampler.sample(&mut traffic, now);
        assert_eq!(traffic.arxs, 0.0, "first sample is only a baseline");
        traffic.received_announce(500);
        traffic.sent_announce(250);
        traffic.received_path_request(125);
        traffic.sent_path_request(25);
        sampler.sample(&mut traffic, now);
        sampler.sample(&mut traffic, now - Duration::from_secs(1));
        assert_eq!(traffic.arxs, 0.0);
        sampler.sample(&mut traffic, now + Duration::from_millis(2500));
        assert_eq!(
            [traffic.arxs, traffic.atxs, traffic.prxs, traffic.ptxs],
            [1600.0, 800.0, 400.0, 80.0]
        );
        sampler.sample(&mut traffic, now + Duration::from_secs(4));
        assert_eq!(
            [traffic.arxs, traffic.atxs, traffic.prxs, traffic.ptxs],
            [0.0; 4]
        );
        traffic.arxb = 0; // Defensive handling of a counter reset.
        sampler.sample(&mut traffic, now + Duration::from_secs(5));
        assert_eq!(traffic.arxs, 0.0);
        traffic.received_announce(100);
        sampler.sample(&mut traffic, now + Duration::from_secs(7));
        assert_eq!(traffic.arxs, 400.0);
    }

    #[test]
    fn control_traffic_saturates_each_counter_independently() {
        let mut traffic = ControlTraffic {
            arxb: u64::MAX,
            atxb: u64::MAX,
            arxc: u64::MAX,
            atxc: u64::MAX,
            prxb: u64::MAX,
            ptxb: u64::MAX,
            prxc: u64::MAX,
            ptxc: u64::MAX,
            ..Default::default()
        };
        let before = traffic;
        traffic.received_announce(500);
        traffic.sent_announce(500);
        traffic.received_path_request(51);
        traffic.sent_path_request(51);
        assert_eq!(traffic, before);
    }
}
