//! High-throughput HDLC-over-TCP backbone. Server accepts many peers
//! (each its own [`InterfaceHandle`]); client auto-reconnects.
//! Per-peer tuning: TCP keepalive + TCP_USER_TIMEOUT + NODELAY + large
//! buffers. Inbound deframer is capped (vs Python's unbounded) to avoid
//! malformed-peer memory blow-up.

use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use crate::backbone_flap::{FastFlapConfig, FastFlapProtection, FastFlapTable};
use crate::backbone_flow::{
    EVALUATE_INTERVAL, EgressController, EgressDecision, EgressSample, HIGH_WATERMARK,
};
use crate::hdlc;
use crate::socket_tuning::{iface_addr_for, set_keepalive_tuned, set_socket_buffers};
use crate::traits::{InterfaceDirection, InterfaceHandle, InterfaceId, InterfaceMode};
use rns_transport::backbone_ingress::{IngressControl, RECEIVE_BUFFER};
use rns_transport::messages::{InboundPacket, TransportMessage};
use rns_transport::tx_queue::{OutboundFrame, TxAccounting, TxLease, byte_channel};

/// Absolute Backbone MTU ceiling and SO_SNDBUF target (kernel clamps).
/// Actual handle MTU follows bitrate; SO_RCVBUF uses dataplane ingress tuning.
pub const HW_MTU: u32 = 1_048_576;

/// Listener-side bitrate guess advertised on the parent handle.
pub const BITRATE_GUESS: u64 = 100_000_000;

/// Per-peer guess (100 Mbps) — drives [`crate::traits::optimise_mtu`] → 32 KiB MTU.
pub const CHILD_BITRATE_GUESS: u64 = 100_000_000;

pub const RECONNECT_WAIT: u64 = 5;
pub const INITIAL_CONNECT_TIMEOUT: u64 = 5;

pub const TCP_PROBE_AFTER: u32 = 5;
pub const TCP_PROBE_INTERVAL: u32 = 2;
pub const TCP_PROBES: u32 = 12;
/// Linux TCP_USER_TIMEOUT — drops stuck conns when peer goes silent without RST.
pub const TCP_USER_TIMEOUT: u32 = 24;

const TX_CHANNEL_DEPTH: usize = 1024;

#[derive(Debug, Clone)]
pub struct BackboneServerConfig {
    /// Known wire IFAC bytes (0 when disabled); None retains a 64-byte allowance.
    pub receive_ifac_size: Option<usize>,
    pub bitrate: u64,
    pub fast_flap: FastFlapConfig,
    /// None selects Python-compatible process-wide IP history.
    pub fast_flap_table: Option<Arc<FastFlapTable>>,
    pub name: String,
    pub listen_ip: String,
    pub listen_port: u16,
    pub prefer_ipv6: bool,
    pub mode: InterfaceMode,
    /// Optional kernel ifname; binds to its current IP (falls back to `listen_ip`).
    pub device: Option<String>,
}

impl BackboneServerConfig {
    pub fn new(name: &str, ip: &str, port: u16) -> Self {
        Self {
            receive_ifac_size: None,
            bitrate: BITRATE_GUESS,
            fast_flap: FastFlapConfig::default(),
            fast_flap_table: None,
            name: name.to_string(),
            listen_ip: ip.to_string(),
            listen_port: port,
            prefer_ipv6: false,
            mode: InterfaceMode::Full,
            device: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BackboneClientConfig {
    /// Known wire IFAC bytes (0 when disabled); None retains a 64-byte allowance.
    pub receive_ifac_size: Option<usize>,
    pub bitrate: u64,
    pub name: String,
    pub target_host: String,
    pub target_port: u16,
    pub prefer_ipv6: bool,
    pub connect_timeout_secs: u64,
    pub max_reconnect_tries: Option<usize>,
    pub mode: InterfaceMode,
}

impl BackboneClientConfig {
    pub fn new(name: &str, host: &str, port: u16) -> Self {
        Self {
            receive_ifac_size: None,
            bitrate: CHILD_BITRATE_GUESS,
            name: name.to_string(),
            target_host: host.to_string(),
            target_port: port,
            prefer_ipv6: false,
            connect_timeout_secs: INITIAL_CONNECT_TIMEOUT,
            max_reconnect_tries: None,
            mode: InterfaceMode::Full,
        }
    }
}

fn receive_limit(bitrate: u64, ifac_size: Option<usize>) -> std::io::Result<u32> {
    let size = ifac_size.unwrap_or(64);
    if size > 64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Backbone receive IFAC size must be 0..=64 bytes",
        ));
    }
    Ok(mtu_for_bitrate(bitrate) + size as u32)
}

fn keepalive_durations() -> (Duration, Duration, u32, Duration) {
    (
        Duration::from_secs(TCP_PROBE_AFTER as u64),
        Duration::from_secs(TCP_PROBE_INTERVAL as u64),
        TCP_PROBES,
        Duration::from_secs(TCP_USER_TIMEOUT as u64),
    )
}

fn tune_stream(stream: &TcpStream) {
    let _ = stream.set_nodelay(true);
    let (idle, intvl, retries, user_timeout) = keepalive_durations();
    set_keepalive_tuned(stream, idle, intvl, retries, user_timeout);
    set_socket_buffers(stream, HW_MTU as usize);
    let _ = socket2::SockRef::from(stream).set_recv_buffer_size(RECEIVE_BUFFER);
}

fn mtu_for_bitrate(bitrate: u64) -> u32 {
    crate::traits::optimise_mtu(bitrate)
        .map(|m| m.min(HW_MTU))
        .unwrap_or(rns_wire::constants::MTU as u32)
}

// Readable payload remains unread while gated or awaiting transport capacity.
// Polling is bounded because
// readiness stays asserted for unread data; awaiting ready alone would spin.
const BLOCKED_CLOSE_POLL: Duration = Duration::from_millis(50);

async fn wait_socket_closed(reader: &tokio::net::tcp::OwnedReadHalf) {
    loop {
        match reader
            .ready(tokio::io::Interest::READABLE | tokio::io::Interest::ERROR)
            .await
        {
            Ok(ready) if ready.is_read_closed() || ready.is_error() => return,
            Err(_) => return,
            _ => {}
        }
        tokio::time::sleep(BLOCKED_CLOSE_POLL).await;
    }
}

async fn wait_ingress_or_closed(
    reader: &tokio::net::tcp::OwnedReadHalf,
    ingress: &IngressControl,
) -> bool {
    tokio::select! {
        biased;
        _ = ingress.wait_open() => true,
        _ = wait_socket_closed(reader) => false,
    }
}

async fn backbone_read_loop(
    mut reader: tokio::net::tcp::OwnedReadHalf,
    interface_id: InterfaceId,
    transport_tx: mpsc::Sender<TransportMessage>,
    online: Arc<AtomicBool>,
    rxb: Arc<AtomicU64>,
    ingress: Arc<IngressControl>,
    max_decoded_size: u32,
) {
    struct ResetIngress(Arc<IngressControl>);
    impl Drop for ResetIngress {
        fn drop(&mut self) {
            self.0.reset();
        }
    }
    let _reset = ResetIngress(ingress.clone());
    let mut deframer = hdlc::HdlcDeframer::with_max_decoded_size(max_decoded_size as usize);
    // Large read buffer amortises syscalls independently of frame boundaries.
    let mut buf = vec![0u8; 65536];
    'receive: loop {
        if !wait_ingress_or_closed(&reader, &ingress).await {
            tracing::info!(interface_id, "backbone peer closed while ingress gated");
            break;
        }
        match reader.read(&mut buf).await {
            Ok(0) => {
                tracing::info!(interface_id, "backbone read: EOF");
                break;
            }
            Ok(n) => {
                rxb.fetch_add(n as u64, Ordering::Relaxed);
                ingress.received_bytes(n);
                let rejected = deframer.oversized_frames();
                let frames = deframer.feed(&buf[..n]);
                if deframer.oversized_frames() != rejected {
                    tracing::debug!(
                        interface_id,
                        max_decoded_size,
                        dropped = deframer.oversized_frames() - rejected,
                        "backbone oversized HDLC frames rejected"
                    );
                }
                for frame in frames {
                    if frame.is_empty() {
                        continue;
                    }
                    if !wait_ingress_or_closed(&reader, &ingress).await {
                        tracing::info!(
                            interface_id,
                            "backbone peer closed with gated frames pending"
                        );
                        break 'receive;
                    }
                    ingress.received_frame();
                    let msg = TransportMessage::Inbound(InboundPacket {
                        raw: Bytes::from(frame),
                        interface_id,
                        rssi: None,
                        snr: None,
                        q: None,
                    });
                    // Preserve ready delivery, including buffered frames before
                    // normal FIN. If admission is blocked, closure cancels the
                    // unsent frame instead of retaining a dead reader forever.
                    let sent = tokio::select! {
                        biased;
                        result = transport_tx.send(msg) => result,
                        _ = wait_socket_closed(&reader) => {
                            tracing::info!(interface_id, "backbone peer closed while transport admission blocked");
                            break 'receive;
                        }
                    };
                    if sent.is_err() {
                        tracing::warn!(interface_id, "transport channel closed");
                        online.store(false, Ordering::SeqCst);
                        return;
                    }
                }
            }
            Err(e) => {
                tracing::warn!(interface_id, error = %e, "backbone read error");
                break;
            }
        }
    }
    online.store(false, Ordering::SeqCst);
}

// Python TransmitBuffer.COALESCE_TARGET. Unlike the Python chunk queue, only
// one bounded encoded batch is held here; the existing mpsc provides backlog.
const TX_COALESCE_TARGET: usize = 65536;
const TX_COALESCE_FRAMES: usize = 64;
// Python 1.5.2 DP_EC_DEAD_TIME. Only pending socket output is timed,
// never an idle connection waiting for the next application frame.
const TX_DEAD_TIME: Duration = crate::backbone_flow::DEAD_TIME;

#[cfg(test)]
#[path = "backbone_tx_tests.rs"]
mod tx_tests;

struct TxFrame {
    raw: Bytes,
    lease: Option<Arc<TxLease>>,
    offset: usize,
    started: bool,
}

impl TxFrame {
    fn new(frame: impl Into<OutboundFrame>) -> Self {
        let frame = frame.into();
        Self {
            raw: frame.raw,
            lease: frame.lease,
            offset: 0,
            started: false,
        }
    }

    /// Append as much as fits, retaining state even across an escaped byte
    /// boundary or a final delimiter. No frame-sized encoded allocation.
    fn append(&mut self, out: &mut Vec<u8>) -> bool {
        if !self.started {
            if out.len() == TX_COALESCE_TARGET {
                return false;
            }
            out.push(hdlc::FLAG);
            self.started = true;
        }
        while self.offset < self.raw.len() {
            // Copy ordinary runs together instead of pushing each byte. Limit
            // the scan to this batch so a large frame is never rescanned.
            let available = TX_COALESCE_TARGET - out.len();
            let end = self.raw.len().min(self.offset + available);
            let run = &self.raw[self.offset..end];
            let plain = run
                .iter()
                .position(|&byte| byte == hdlc::FLAG || byte == hdlc::ESC)
                .unwrap_or(run.len());
            out.extend_from_slice(&run[..plain]);
            self.offset += plain;
            if self.offset == self.raw.len() {
                break;
            }
            if out.len() == TX_COALESCE_TARGET {
                return false;
            }
            let byte = self.raw[self.offset];
            if TX_COALESCE_TARGET - out.len() < 2 {
                return false;
            }
            out.push(hdlc::ESC);
            out.push(byte ^ hdlc::ESC_MASK);
            self.offset += 1;
        }
        if out.len() == TX_COALESCE_TARGET {
            return false;
        }
        out.push(hdlc::FLAG);
        true
    }
}

async fn backbone_write_loop<W: tokio::io::AsyncWrite + Unpin, T: Into<OutboundFrame>>(
    mut writer: W,
    mut rx: mpsc::Receiver<T>,
    online: Arc<AtomicBool>,
    txb: Arc<AtomicU64>,
) {
    let mut pending: Option<TxFrame> = None;
    let mut batch = Vec::with_capacity(TX_COALESCE_TARGET);
    let mut segments = std::collections::VecDeque::new();
    'transmit: loop {
        if pending.is_none() {
            let Some(raw) = rx.recv().await else { break };
            pending = Some(TxFrame::new(raw));
        }
        batch.clear();
        segments.clear();
        for _ in 0..TX_COALESCE_FRAMES {
            let frame = pending.as_mut().unwrap();
            let start = batch.len();
            let complete = frame.append(&mut batch);
            if batch.len() > start {
                segments.push_back((frame.lease.clone(), batch.len() - start));
            }
            if !complete {
                break;
            }
            pending = None;
            if batch.len() == TX_COALESCE_TARGET {
                break;
            }
            // Never wait for another frame: sparse traffic is sent immediately.
            match rx.try_recv() {
                Ok(raw) => pending = Some(TxFrame::new(raw)),
                Err(_) => break,
            }
        }
        let mut sent = 0;
        let mut deadline = tokio::time::Instant::now() + TX_DEAD_TIME;
        while sent < batch.len() {
            // Check explicitly as an always-ready Interrupted writer must not
            // defeat timeout_at's polling of the write future before its timer.
            if tokio::time::Instant::now() >= deadline {
                tracing::warn!("backbone TX made no progress for 12 seconds");
                break 'transmit;
            }
            let result = match tokio::time::timeout_at(deadline, writer.write(&batch[sent..])).await
            {
                Ok(result) => result,
                Err(_) => {
                    tracing::warn!("backbone TX made no progress for 12 seconds");
                    break 'transmit;
                }
            };
            match result {
                Ok(0) => {
                    tracing::warn!("backbone write returned zero");
                    break 'transmit;
                }
                Ok(written) => {
                    sent += written;
                    txb.fetch_add(written as u64, Ordering::Relaxed);
                    let mut credit = written;
                    while credit > 0 {
                        let (lease, remaining) = segments.front_mut().unwrap();
                        let part = credit.min(*remaining);
                        if let Some(lease) = lease {
                            lease.written(part as u64);
                        }
                        *remaining -= part;
                        credit -= part;
                        if *remaining == 0 {
                            segments.pop_front();
                        }
                    }
                    deadline = tokio::time::Instant::now() + TX_DEAD_TIME;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                    tokio::task::yield_now().await;
                    continue;
                }
                Err(error) => {
                    tracing::warn!(%error, "backbone write error");
                    break 'transmit;
                }
            }
        }
        // A ready writer and a permanently full input must still yield to peers.
        tokio::task::yield_now().await;
    }
    online.store(false, Ordering::SeqCst);
}

fn encoded_len(raw: &[u8]) -> u64 {
    raw.len() as u64
        + 2
        + raw
            .iter()
            .filter(|&&b| b == hdlc::FLAG || b == hdlc::ESC)
            .count() as u64
}

async fn controlled_write_loop<W: tokio::io::AsyncWrite + Unpin>(
    writer: W,
    rx: mpsc::Receiver<OutboundFrame>,
    online: Arc<AtomicBool>,
    txb: Arc<AtomicU64>,
    accounting: Arc<TxAccounting>,
) {
    struct ReleaseGate(Arc<TxAccounting>);
    impl Drop for ReleaseGate {
        fn drop(&mut self) {
            self.0.set_gated(false);
        }
    }
    let _release = ReleaseGate(accounting.clone());
    let origin = tokio::time::Instant::now();
    let mut controller = EgressController::new(Duration::ZERO, accounting.snapshot().sent);
    let monitor = async {
        let mut timer = tokio::time::interval_at(origin + EVALUATE_INTERVAL, EVALUATE_INTERVAL);
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            timer.tick().await;
            let snapshot = accounting.snapshot();
            let decision = controller.evaluate(
                origin.elapsed(),
                EgressSample {
                    buffered: snapshot.buffered,
                    sendable: snapshot.buffered,
                    sent: snapshot.sent,
                },
            );
            accounting.set_gated(controller.stalled());
            if decision == EgressDecision::Disconnect {
                tracing::warn!("backbone sampled egress controller requested disconnect");
                break;
            }
        }
    };
    tokio::select! {
        _ = backbone_write_loop(writer, rx, online.clone(), txb) => {},
        _ = monitor => {},
    }
    online.store(false, Ordering::SeqCst);
}

/// `device` takes precedence over `listen_ip` when its lookup succeeds.
fn resolve_listen_addr(config: &BackboneServerConfig) -> String {
    if let Some(name) = config.device.as_deref() {
        match iface_addr_for(name, config.prefer_ipv6) {
            Some(IpAddr::V4(v4)) => return v4.to_string(),
            Some(IpAddr::V6(v6)) => return format!("[{}]", v6),
            None => {
                tracing::warn!(
                    device = %name,
                    listen_ip = %config.listen_ip,
                    "backbone: device lookup failed, falling back to listen_ip",
                );
            }
        }
    }
    config.listen_ip.clone()
}

/// Resolve `host:port` preferring the configured address family.
async fn resolve_target(
    host: &str,
    port: u16,
    prefer_ipv6: bool,
) -> std::io::Result<std::net::SocketAddr> {
    let mut addrs: Vec<std::net::SocketAddr> =
        tokio::net::lookup_host((host, port)).await?.collect();
    if addrs.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            format!("no addresses resolved for {host}:{port}"),
        ));
    }
    // Prefer the requested family; else first resolved.
    let preferred = if prefer_ipv6 {
        addrs.iter().find(|a| a.is_ipv6()).copied()
    } else {
        addrs.iter().find(|a| a.is_ipv4()).copied()
    };
    Ok(preferred.unwrap_or_else(|| addrs.remove(0)))
}

/// Spawn a backbone server; each accepted connection becomes an `InterfaceHandle`.
pub async fn spawn_backbone_server(
    config: BackboneServerConfig,
    id: InterfaceId,
    id_gen: Arc<AtomicU64>,
    transport_tx: mpsc::Sender<TransportMessage>,
    handle_tx: mpsc::Sender<InterfaceHandle>,
) -> Result<InterfaceHandle, crate::traits::InterfaceError> {
    let receive_limit = receive_limit(config.bitrate, config.receive_ifac_size)?;
    config
        .fast_flap
        .validate()
        .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?;
    let protection = Arc::new(match config.fast_flap_table.clone() {
        Some(table) => FastFlapProtection::with_table(config.fast_flap.clone(), table),
        None => FastFlapProtection::new(config.fast_flap.clone()),
    });
    let diagnostics = protection.clone();
    let listen_ip = resolve_listen_addr(&config);
    let bind_addr = if listen_ip.starts_with('[') {
        format!("{}:{}", listen_ip, config.listen_port)
    } else if listen_ip.contains(':') && !listen_ip.contains('.') {
        format!("[{}]:{}", listen_ip, config.listen_port)
    } else {
        format!("{}:{}", listen_ip, config.listen_port)
    };
    let listener = TcpListener::bind(&bind_addr).await?;
    let local_addr = listener.local_addr()?;
    tracing::info!(name = %config.name, addr = %local_addr, "backbone server listening");

    let online = Arc::new(AtomicBool::new(true));
    let online2 = online.clone();
    let name = config.name.clone();
    let mode = config.mode;
    let bitrate = config.bitrate;
    let mtu = mtu_for_bitrate(bitrate);

    // Parent listener is inbound-only; drain task warns on stray writes.
    let (tx, mut listener_rx) = mpsc::channel::<Bytes>(1);
    let drain_name = name.clone();
    tokio::spawn(async move {
        while listener_rx.recv().await.is_some() {
            tracing::warn!(
                name = %drain_name,
                "backbone listener tx received unexpected outbound data; dropping",
            );
        }
    });

    let read_task = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    if protection.is_blocked_at(peer.ip(), std::time::Instant::now()) {
                        tracing::debug!(%peer, "rejecting fast-flapping Backbone IP");
                        drop(stream);
                        continue;
                    }
                    let connected_at = std::time::Instant::now();
                    let client_id = id_gen.fetch_add(1, Ordering::SeqCst);
                    let client_name = format!("{}/client_{}", config.name, client_id);
                    tracing::info!(
                        name = %client_name,
                        peer = %peer,
                        "backbone: accepted connection",
                    );

                    tune_stream(&stream);

                    let c_online = Arc::new(AtomicBool::new(true));
                    let c_rxb = Arc::new(AtomicU64::new(0));
                    let c_txb = Arc::new(AtomicU64::new(0));
                    let ingress = IngressControl::new();
                    let reader_ingress = ingress.clone();
                    let (c_tx, c_rx, accounting) =
                        byte_channel(TX_CHANNEL_DEPTH, HIGH_WATERMARK, encoded_len);
                    let (reader, writer) = stream.into_split();

                    let c_online_w = c_online.clone();
                    let c_txb_w = c_txb.clone();

                    let c_online_r = c_online.clone();
                    let c_rxb_r = c_rxb.clone();
                    let transport_tx2 = transport_tx.clone();
                    let dereg_tx = transport_tx.clone();
                    let cname = client_name.clone();
                    let connection_online = c_online.clone();
                    let disconnected = FlapDisconnectGuard {
                        protection: protection.clone(),
                        ip: peer.ip(),
                        connected_at,
                    };
                    let read_handle = tokio::spawn(async move {
                        tokio::select! {
                            _ = backbone_read_loop(reader, client_id, transport_tx2, c_online_r, c_rxb_r, reader_ingress, receive_limit) => {},
                            _ = controlled_write_loop(writer, c_rx, c_online_w, c_txb_w, accounting) => {},
                        }
                        connection_online.store(false, Ordering::SeqCst);
                        // Record before notifying the runtime, so an immediate
                        // reconnect observes the new block. Guard also covers abort.
                        drop(disconnected);
                        tracing::info!(name = %cname, "backbone client disconnected");
                        // Proactive notify so broadcasts don't target dead tx.
                        let _ = dereg_tx
                            .send(TransportMessage::DeregisterInterface { id: client_id })
                            .await;
                    });

                    let handle = InterfaceHandle {
                        id: client_id,
                        parent_id: Some(id),
                        diagnostics: Some(ingress),
                        name: client_name,
                        mode,
                        direction: InterfaceDirection {
                            inbound: true,
                            outbound: true,
                            forward: false,
                            repeat: false,
                        },
                        bitrate,
                        mtu,
                        online: c_online,
                        rxb: Some(c_rxb),
                        txb: Some(c_txb),
                        tx: c_tx,
                        read_task: read_handle,
                    };
                    if handle_tx.send(handle).await.is_err() {
                        tracing::warn!("backbone handle registry closed");
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "backbone accept error");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
        online2.store(false, Ordering::SeqCst);
    });

    Ok(InterfaceHandle {
        id,
        parent_id: None,
        diagnostics: Some(diagnostics),
        name,
        mode,
        direction: InterfaceDirection {
            inbound: true,
            outbound: false,
            forward: false,
            repeat: false,
        },
        bitrate,
        mtu,
        online,
        rxb: Some(Arc::new(AtomicU64::new(0))),
        txb: Some(Arc::new(AtomicU64::new(0))),
        tx: tx.into(),
        read_task,
    })
}

struct FlapDisconnectGuard {
    protection: Arc<FastFlapProtection>,
    ip: IpAddr,
    connected_at: std::time::Instant,
}

impl Drop for FlapDisconnectGuard {
    fn drop(&mut self) {
        self.protection
            .disconnected_at(self.ip, self.connected_at, std::time::Instant::now());
    }
}

pub async fn spawn_backbone_client(
    config: BackboneClientConfig,
    id: InterfaceId,
    transport_tx: mpsc::Sender<TransportMessage>,
) -> Result<InterfaceHandle, crate::traits::InterfaceError> {
    let receive_limit = receive_limit(config.bitrate, config.receive_ifac_size)?;
    let online = Arc::new(AtomicBool::new(false));
    let online2 = online.clone();
    let (tx, rx, accounting) = byte_channel(TX_CHANNEL_DEPTH, HIGH_WATERMARK, encoded_len);
    let name = config.name.clone();
    let mode = config.mode;
    let rx = Arc::new(tokio::sync::Mutex::new(rx));
    let bitrate = config.bitrate;
    let mtu = mtu_for_bitrate(bitrate);

    let shared_rxb = Arc::new(AtomicU64::new(0));
    let shared_txb = Arc::new(AtomicU64::new(0));
    let ingress = IngressControl::new();
    let reader_ingress = ingress.clone();
    let task_rxb = shared_rxb.clone();
    let task_txb = shared_txb.clone();

    let read_task = tokio::spawn(async move {
        let max_tries = config.max_reconnect_tries;
        let mut tries: usize = 0;

        loop {
            let target = match tokio::time::timeout(
                Duration::from_secs(config.connect_timeout_secs),
                resolve_target(&config.target_host, config.target_port, config.prefer_ipv6),
            )
            .await
            {
                Ok(Ok(addr)) => addr,
                Ok(Err(e)) => {
                    tracing::warn!(name = %config.name, error = %e, "backbone resolve failed");
                    if let Some(max) = max_tries {
                        tries += 1;
                        if tries >= max {
                            let _ = transport_tx
                                .send(TransportMessage::DeregisterInterface { id })
                                .await;
                            return;
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(RECONNECT_WAIT)).await;
                    continue;
                }
                Err(_) => {
                    tracing::warn!(name = %config.name, "backbone resolve timed out");
                    if let Some(max) = max_tries {
                        tries += 1;
                        if tries >= max {
                            let _ = transport_tx
                                .send(TransportMessage::DeregisterInterface { id })
                                .await;
                            return;
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(RECONNECT_WAIT)).await;
                    continue;
                }
            };

            let stream = match tokio::time::timeout(
                Duration::from_secs(config.connect_timeout_secs),
                TcpStream::connect(target),
            )
            .await
            {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    tracing::warn!(name = %config.name, error = %e, "backbone connect failed");
                    if let Some(max) = max_tries {
                        tries += 1;
                        if tries >= max {
                            let _ = transport_tx
                                .send(TransportMessage::DeregisterInterface { id })
                                .await;
                            return;
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(RECONNECT_WAIT)).await;
                    continue;
                }
                Err(_) => {
                    tracing::warn!(name = %config.name, "backbone connect timed out");
                    if let Some(max) = max_tries {
                        tries += 1;
                        if tries >= max {
                            let _ = transport_tx
                                .send(TransportMessage::DeregisterInterface { id })
                                .await;
                            return;
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(RECONNECT_WAIT)).await;
                    continue;
                }
            };

            tune_stream(&stream);
            reader_ingress.reset();
            online2.store(true, Ordering::SeqCst);
            tries = 0;

            let c_online = Arc::new(AtomicBool::new(true));
            let (reader, writer) = stream.into_split();

            let (conn_tx, conn_rx) = mpsc::channel::<OutboundFrame>(TX_CHANNEL_DEPTH);
            let c_online_w = c_online.clone();
            let c_txb = task_txb.clone();

            let rx_ref = rx.clone();
            let c_online_r = c_online.clone();
            let c_rxb = task_rxb.clone();
            // Either half ending must close the whole connection. In particular,
            // a TX deadline must not leave us waiting on a silent peer's reader.
            {
                // Keep forwarding in this task so cancellation cannot orphan
                // a receiver and retain queued reservations indefinitely.
                let forward = async move {
                    let mut guard = rx_ref.lock().await;
                    while let Some(data) = guard.recv().await {
                        if conn_tx.send(data).await.is_err() {
                            break;
                        }
                    }
                };
                let reading = backbone_read_loop(
                    reader,
                    id,
                    transport_tx.clone(),
                    c_online_r,
                    c_rxb,
                    reader_ingress.clone(),
                    receive_limit,
                );
                let writing =
                    controlled_write_loop(writer, conn_rx, c_online_w, c_txb, accounting.clone());
                tokio::pin!(forward, reading, writing);
                let mut forwarded = false;
                loop {
                    tokio::select! {
                        _ = &mut reading => break,
                        _ = &mut writing => break,
                        _ = &mut forward, if !forwarded => { forwarded = true; },
                    }
                }
            }

            online2.store(false, Ordering::SeqCst);

            if let Some(max) = max_tries {
                tries += 1;
                if tries >= max {
                    let _ = transport_tx
                        .send(TransportMessage::DeregisterInterface { id })
                        .await;
                    return;
                }
            }
            tracing::info!(
                name = %config.name,
                "backbone: reconnecting in {}s",
                RECONNECT_WAIT,
            );
            tokio::time::sleep(Duration::from_secs(RECONNECT_WAIT)).await;
        }
    });

    Ok(InterfaceHandle {
        id,
        parent_id: None,
        diagnostics: Some(ingress),
        name,
        mode,
        direction: InterfaceDirection {
            inbound: true,
            outbound: true,
            forward: false,
            repeat: false,
        },
        bitrate,
        mtu,
        online,
        rxb: Some(shared_rxb),
        txb: Some(shared_txb),
        tx: tx.into(),
        read_task,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fast_flapping_loopback_grace_rejection_and_disabled() {
        for enabled in [true, false] {
            let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = reservation.local_addr().unwrap().port();
            drop(reservation);
            let mut config = BackboneServerConfig::new("flap-test", "127.0.0.1", port);
            config.fast_flap = FastFlapConfig {
                enabled,
                grace: 1,
                ..Default::default()
            };
            config.fast_flap_table = Some(Arc::default());
            let (transport_tx, mut events) = mpsc::channel(16);
            let (handle_tx, mut children) = mpsc::channel(16);
            let parent = spawn_backbone_server(
                config,
                1,
                Arc::new(AtomicU64::new(2)),
                transport_tx,
                handle_tx,
            )
            .await
            .unwrap();
            let diagnostics = parent.diagnostics.as_ref().unwrap();
            for count in 1..=2 {
                let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                let child = tokio::time::timeout(Duration::from_secs(2), children.recv())
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(child.parent_id, Some(1));
                drop(stream);
                let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
                    .await
                    .unwrap()
                    .unwrap();
                assert!(
                    matches!(event, TransportMessage::DeregisterInterface { id } if id == child.id)
                );
                assert_eq!(
                    diagnostics.blocked_ip_list().unwrap(),
                    if enabled && count == 2 {
                        vec!["127.0.0.1".to_owned()]
                    } else {
                        vec![]
                    }
                );
                child.read_task.await.unwrap();
            }
            let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            if enabled {
                let mut byte = [0];
                assert_eq!(
                    tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte))
                        .await
                        .unwrap()
                        .unwrap(),
                    0
                );
                assert!(children.try_recv().is_err());
            } else {
                let child = tokio::time::timeout(Duration::from_secs(2), children.recv())
                    .await
                    .unwrap()
                    .unwrap();
                drop(stream);
                tokio::time::timeout(Duration::from_secs(2), child.read_task)
                    .await
                    .unwrap()
                    .unwrap();
                assert!(diagnostics.blocked_ip_list().unwrap().is_empty());
            }
            parent.read_task.abort();
            let _ = parent.read_task.await;
        }
    }

    #[test]
    fn test_backbone_server_config() {
        let cfg = BackboneServerConfig::new("backbone0", "0.0.0.0", 4243);
        assert_eq!(cfg.listen_port, 4243);
        assert!(!cfg.prefer_ipv6);
        assert!(cfg.device.is_none());
    }

    #[test]
    fn test_backbone_client_config() {
        let cfg = BackboneClientConfig::new("bb-client", "10.0.0.1", 4243);
        assert_eq!(cfg.target_host, "10.0.0.1");
        assert_eq!(cfg.connect_timeout_secs, INITIAL_CONNECT_TIMEOUT);
    }

    #[test]
    fn test_constants() {
        assert_eq!(HW_MTU, 1_048_576);
        assert_eq!(BITRATE_GUESS, 100_000_000);
        assert_eq!(CHILD_BITRATE_GUESS, 100_000_000);
        assert_eq!(RECONNECT_WAIT, 5);
    }

    #[test]
    fn test_backbone_server_config_mode() {
        let cfg = BackboneServerConfig::new("bb-srv", "0.0.0.0", 4243);
        assert_eq!(cfg.mode, InterfaceMode::Full);
        assert_eq!(cfg.listen_ip, "0.0.0.0");
        assert_eq!(cfg.name, "bb-srv");
    }

    #[test]
    fn test_backbone_client_config_defaults() {
        let cfg = BackboneClientConfig::new("bb-cli", "192.168.1.1", 4243);
        assert_eq!(cfg.target_host, "192.168.1.1");
        assert_eq!(cfg.target_port, 4243);
        assert_eq!(cfg.mode, InterfaceMode::Full);
        assert!(cfg.max_reconnect_tries.is_none());
    }

    #[test]
    fn test_backbone_config_ipv6() {
        let cfg = BackboneServerConfig::new("bb-v6", "::", 4243);
        assert!(!cfg.prefer_ipv6);
        let mut cfg = cfg;
        cfg.prefer_ipv6 = true;
        assert!(cfg.prefer_ipv6);
    }

    #[test]
    fn test_child_mtu_uses_100mbps_curve() {
        let mtu = mtu_for_bitrate(CHILD_BITRATE_GUESS);
        assert_eq!(mtu, 32_768);
        assert!(mtu <= HW_MTU);
        assert!(mtu >= rns_wire::constants::MTU as u32 / 2);
    }

    #[tokio::test]
    async fn test_backbone_max_reconnect_dereg() {
        // Connect attempts to a port nobody is listening on; with
        // max_reconnect_tries = Some(1) we get one connect attempt, one
        // failure, then DeregisterInterface and exit.
        let mut cfg = BackboneClientConfig::new("bb-dereg", "127.0.0.1", 1);
        cfg.connect_timeout_secs = 1;
        cfg.max_reconnect_tries = Some(1);

        let (tx, mut rx) = mpsc::channel::<TransportMessage>(8);
        let handle = spawn_backbone_client(cfg, 99, tx).await.unwrap();
        // Wait for the read_task to finish — guarded by a generous timeout
        // to absorb the one RECONNECT_WAIT sleep + connect attempt.
        let dereg = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                match rx.recv().await {
                    Some(TransportMessage::DeregisterInterface { id }) => return Some(id),
                    Some(_) => continue,
                    None => return None,
                }
            }
        })
        .await
        .ok()
        .flatten();
        assert_eq!(dereg, Some(99));
        // Drop the handle (aborts read_task if still alive).
        drop(handle);
    }

    #[tokio::test]
    async fn test_resolve_target_loopback_v4() {
        let addr = resolve_target("127.0.0.1", 1234, false).await.unwrap();
        assert!(addr.is_ipv4());
        assert_eq!(addr.port(), 1234);
    }
}
