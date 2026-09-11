//! Python 1.5.2 Backbone fast-flapping policy. The IP history is process-wide,
//! while each listener supplies its own threshold, grace and expiry policy.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub struct FastFlapConfig {
    pub enabled: bool,
    pub threshold_secs: f64,
    pub grace: u64,
    pub block_time_secs: f64,
}

impl Default for FastFlapConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_secs: 20.0,
            grace: 5,
            block_time_secs: 12.0 * 60.0 * 60.0,
        }
    }
}

impl FastFlapConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.threshold_secs.is_finite() || self.threshold_secs < 0.0 {
            return Err("fast_flapping_threshold must be finite and non-negative seconds");
        }
        if !self.block_time_secs.is_finite() || self.block_time_secs < 0.0 {
            return Err(
                "fast_flapping_block_time converted to seconds must be finite and non-negative",
            );
        }
        Ok(())
    }
}

#[derive(Debug)]
struct FlapEntry {
    last_flap: Instant,
    flaps: u64,
}

#[derive(Debug, Default)]
pub struct FastFlapTable(Mutex<HashMap<IpAddr, FlapEntry>>);

static SHARED_TABLE: OnceLock<Arc<FastFlapTable>> = OnceLock::new();

#[derive(Debug)]
pub struct FastFlapProtection {
    config: FastFlapConfig,
    table: Arc<FastFlapTable>,
}

impl FastFlapProtection {
    pub fn new(config: FastFlapConfig) -> Self {
        Self::with_table(
            config,
            SHARED_TABLE
                .get_or_init(|| Arc::new(FastFlapTable::default()))
                .clone(),
        )
    }

    /// Explicit table for isolated runtimes/tests. Defaults share process state,
    /// matching the Python BackboneInterface class-level dictionary.
    pub fn with_table(config: FastFlapConfig, table: Arc<FastFlapTable>) -> Self {
        Self { config, table }
    }

    pub fn is_blocked_at(&self, ip: IpAddr, now: Instant) -> bool {
        self.blocked_ips_at(now).contains(&ip)
    }

    pub fn blocked_ips_at(&self, now: Instant) -> Vec<IpAddr> {
        if !self.config.enabled {
            return Vec::new();
        }
        let mut entries = self.table.0.lock().expect("fast-flapping table poisoned");
        entries.retain(|_, entry| {
            now.saturating_duration_since(entry.last_flap).as_secs_f64()
                <= self.config.block_time_secs
        });
        let mut blocked: Vec<_> = entries
            .iter()
            .filter_map(|(&ip, entry)| (entry.flaps > self.config.grace).then_some(ip))
            .collect();
        blocked.sort_unstable();
        blocked
    }

    pub fn disconnected_at(&self, ip: IpAddr, connected_at: Instant, now: Instant) {
        if !self.config.enabled
            || now.saturating_duration_since(connected_at).as_secs_f64()
                >= self.config.threshold_secs
        {
            return;
        }
        let mut entries = self.table.0.lock().expect("fast-flapping table poisoned");
        let entry = entries.entry(ip).or_insert(FlapEntry {
            last_flap: now,
            flaps: 0,
        });
        entry.last_flap = now;
        entry.flaps = entry.flaps.saturating_add(1);
        if entry.flaps > self.config.grace {
            tracing::warn!(%ip, flaps = entry.flaps, "blocking fast-flapping Backbone IP");
        }
    }
}

impl rns_transport::messages::InterfaceDiagnostics for FastFlapProtection {
    fn blocked_ip_list(&self) -> Option<Vec<String>> {
        Some(
            self.blocked_ips_at(Instant::now())
                .into_iter()
                .map(|ip| ip.to_string())
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn rejected_attempts_do_not_extend_expiry_and_stale_grace_history_expires() {
        let protection = FastFlapProtection::with_table(
            FastFlapConfig {
                grace: 0,
                block_time_secs: 60.0,
                ..Default::default()
            },
            Arc::default(),
        );
        let ip = "127.0.0.1".parse().unwrap();
        let start = Instant::now();
        protection.disconnected_at(ip, start, start);
        for seconds in [1, 20, 59, 60] {
            assert!(protection.is_blocked_at(ip, start + Duration::from_secs(seconds)));
        }
        assert!(!protection.is_blocked_at(ip, start + Duration::from_secs(61)));

        let protection = FastFlapProtection::with_table(
            FastFlapConfig {
                grace: 1,
                block_time_secs: 60.0,
                ..Default::default()
            },
            Arc::default(),
        );
        protection.disconnected_at(ip, start, start);
        assert!(!protection.is_blocked_at(ip, start + Duration::from_secs(61)));
        protection.disconnected_at(
            ip,
            start + Duration::from_secs(61),
            start + Duration::from_secs(61),
        );
        assert!(!protection.is_blocked_at(ip, start + Duration::from_secs(61)));
    }

    #[test]
    fn threshold_grace_and_expiry_are_strict_and_long_connections_do_not_reset() {
        let protection = FastFlapProtection::with_table(
            FastFlapConfig {
                threshold_secs: 20.0,
                grace: 1,
                block_time_secs: 60.0,
                enabled: true,
            },
            Arc::default(),
        );
        let ip = "127.0.0.1".parse().unwrap();
        let start = Instant::now();
        protection.disconnected_at(ip, start, start + Duration::from_secs(20));
        assert!(protection.table.0.lock().unwrap().is_empty());
        protection.disconnected_at(ip, start, start + Duration::from_secs(1));
        assert!(!protection.is_blocked_at(ip, start + Duration::from_secs(1)));
        protection.disconnected_at(ip, start, start + Duration::from_secs(30));
        protection.disconnected_at(
            ip,
            start + Duration::from_secs(30),
            start + Duration::from_secs(31),
        );
        assert_eq!(
            protection.blocked_ips_at(start + Duration::from_secs(31)),
            vec![ip]
        );
        assert!(protection.is_blocked_at(ip, start + Duration::from_secs(91)));
        assert!(!protection.is_blocked_at(ip, start + Duration::from_secs(92)));
        assert!(protection.table.0.lock().unwrap().is_empty());
    }

    #[test]
    fn shared_table_respects_listener_grace_and_disable() {
        let table = Arc::default();
        let enabled = FastFlapProtection::with_table(
            FastFlapConfig {
                grace: 0,
                ..Default::default()
            },
            Arc::clone(&table),
        );
        let disabled = FastFlapProtection::with_table(
            FastFlapConfig {
                enabled: false,
                ..Default::default()
            },
            Arc::clone(&table),
        );
        let lenient = FastFlapProtection::with_table(FastFlapConfig::default(), table);
        let ip = "::1".parse().unwrap();
        let now = Instant::now();
        disabled.disconnected_at(ip, now, now);
        assert!(!enabled.is_blocked_at(ip, now));
        enabled.disconnected_at(ip, now, now);
        assert!(enabled.is_blocked_at(ip, now));
        assert!(!disabled.is_blocked_at(ip, now));
        assert!(!lenient.is_blocked_at(ip, now));
    }
}
