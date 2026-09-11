use std::collections::HashMap;

use crate::messages::InterfaceId;

/// Control-plane packet counts and bytes, excluding IFAC and driver framing.
/// Owned by an interface registration, not by a destination or packet hash.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
}

impl ControlTraffic {
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
    use super::ControlTraffic;

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
        };
        let before = traffic;
        traffic.received_announce(500);
        traffic.sent_announce(500);
        traffic.received_path_request(51);
        traffic.sent_path_request(51);
        assert_eq!(traffic, before);
    }
}
