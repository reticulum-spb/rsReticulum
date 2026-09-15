//! Bounded, infrequent Linux process memory sampling, independent of SQLite.
use std::io::{self, Read};
use std::time::Duration;

use crate::lifecycle::ShutdownSignal;

fn parse_kib(status: &[u8], key: &[u8]) -> Option<u64> {
    // Only complete lines count. Other fields may contain non-UTF-8 bytes.
    status.split_inclusive(|b| *b == b'\n').find_map(|line| {
        if !line.ends_with(b"\n") {
            return None;
        }
        let value = std::str::from_utf8(line.strip_prefix(key)?).ok()?;
        let mut fields = value.split_ascii_whitespace();
        let number = fields.next()?.parse().ok()?;
        (fields.next() == Some("kB") && fields.next().is_none()).then_some(number)
    })
}

fn sample() -> io::Result<(u64, Option<u64>)> {
    // Never allocate in proportion to /proc contents (e.g. a huge Groups field).
    let mut buffer = [0u8; 16384];
    let mut file = std::fs::File::open("/proc/self/status")?;
    let mut len = 0;
    while len < buffer.len() {
        let count = file.read(&mut buffer[len..])?;
        if count == 0 {
            break;
        }
        len += count;
    }
    let status = &buffer[..len];
    let rss = parse_kib(status, b"VmRSS:")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "VmRSS missing or invalid"))?;
    Ok((rss, parse_kib(status, b"VmHWM:")))
}

pub(crate) async fn run(seconds: u32, shutdown: ShutdownSignal) {
    if seconds == 0 {
        return;
    }
    let period = Duration::from_secs(u64::from(seconds));
    let mut timer = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = shutdown.wait() => return,
            _ = timer.tick() => {
                // Skip filesystem work entirely when INFO is filtered out.
                if !tracing::enabled!(tracing::Level::INFO) {
                    continue;
                }
                match tokio::task::spawn_blocking(sample).await {
                    Ok(Ok((rss_kib, peak_rss_kib))) => tracing::info!(rss_kib, peak_rss_kib, "process memory"),
                    result => tracing::debug!(?result, "process memory sample unavailable"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_complete_memory_fields_with_correct_units() {
        let status = b"Name:\t\xff\nVmHWM:\t2048 kB\nVmRSS:\t1024 kB\n";
        assert_eq!(parse_kib(status, b"VmRSS:"), Some(1024));
        assert_eq!(parse_kib(status, b"VmHWM:"), Some(2048));
        for invalid in [
            b"VmRSS: 123 kB".as_slice(),
            b"VmRSS: 123 MB\n",
            b"VmRSS: -1 kB\n",
            b"Name: test\n",
        ] {
            assert_eq!(parse_kib(invalid, b"VmRSS:"), None);
        }
    }
}
