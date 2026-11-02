/// Resolve with bounded blocking admission; keep the caller future alive
/// across deadlines so retry does not duplicate unfinished libc work.
pub(crate) async fn resolve_targets(
    host: &str,
    port: u16,
) -> std::io::Result<Vec<std::net::SocketAddr>> {
    // A timed-out caller must not release admission while libc still runs.
    // Admission is deliberately non-blocking: there is no extra pending queue.
    static RESOLVERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return Ok(vec![std::net::SocketAddr::new(ip, port)]);
    }
    let permit = RESOLVERS.try_acquire().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "DNS resolver capacity exhausted",
        )
    })?;
    let hostname = host.to_owned();
    let addrs = tokio::task::spawn_blocking(move || {
        use std::net::ToSocketAddrs;
        let _permit = permit;
        (hostname.as_str(), port)
            .to_socket_addrs()
            .map(|addresses| addresses.take(64).collect::<Vec<_>>())
    })
    .await
    .map_err(std::io::Error::other)??;
    if addrs.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            format!("no addresses resolved for {host}:{port}"),
        ));
    }
    Ok(addrs)
}

/// Backbone selects one address, preserving its configured family preference.
#[cfg(feature = "full")]
pub(crate) async fn resolve_target(
    host: &str,
    port: u16,
    prefer_ipv6: bool,
) -> std::io::Result<std::net::SocketAddr> {
    let mut addrs = resolve_targets(host, port).await?;
    // Prefer the requested family; else first resolved.
    let preferred = if prefer_ipv6 {
        addrs.iter().find(|a| a.is_ipv6()).copied()
    } else {
        addrs.iter().find(|a| a.is_ipv4()).copied()
    };
    Ok(preferred.unwrap_or_else(|| addrs.remove(0)))
}
