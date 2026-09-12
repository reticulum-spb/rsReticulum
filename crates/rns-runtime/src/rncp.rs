//! Wire-compatible with Python `RNS.Utilities.rncp`. Supports `-l` (listen),
//! send (default), and `-F` / `-f` (fetch serve / client).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio::sync::mpsc;
use tokio::time::timeout;

use rns_crypto::ed25519::Ed25519PublicKey;
use rns_crypto::sha::truncated_hash;
use rns_identity::destination::{DestType, Destination, Direction};
use rns_identity::identity::Identity;
use rns_link::encryption::link_encrypt;
use rns_link::link::{CloseReason, Link};
use rns_protocol::resource::{
    MAX_EFFICIENT_SIZE, OutboundResource, OutboundTransfer, TransferAction,
};
use rns_transport::link_messages::DestinationEvent;
use rns_transport::messages::{
    AnnounceRpcEntry, OutboundRequest, TransportMessage, TransportQuery, TransportQueryResponse,
};
use tokio::sync::oneshot;

use crate::link_manager::{LinkManager, RequestOutcome, ResourceCompletion};

/// Python `REQ_FETCH_NOT_ALLOWED` sentinel (rncp.py:70).
pub const REQ_FETCH_NOT_ALLOWED: u8 = 0xF0;

pub const FETCH_PATH_NAME: &str = "fetch_file";

pub const DEFAULT_RNCP_APP_NAME: &str = "rncp.receive";

pub fn default_rncp_app_name() -> &'static str {
    DEFAULT_RNCP_APP_NAME
}

#[derive(Debug, thiserror::Error)]
pub enum RncpError {
    #[error("transport channel unavailable")]
    TransportUnavailable,
    #[error("timed out waiting for {0}")]
    Timeout(&'static str),
    #[error("remote destination identity has not been announced")]
    PathTimeout,
    #[error("announce carried no public key")]
    NoIdentity,
    #[error("link handshake failed: {0}")]
    HandshakeFailed(String),
    #[error("link proof validation failed: {0}")]
    ProofInvalid(String),
    #[error("link crypto error: {0}")]
    LinkCrypto(String),
    #[error("resource build failed: {0}")]
    ResourceCreate(String),
    #[error("resource transfer failed: {0}")]
    ResourceFailed(String),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("local identity has no signing key")]
    NoSigningKey,
    #[error("sender not in allow-list")]
    Denied,
}

#[derive(Debug, Clone)]
pub struct RncpOutcome {
    pub file_name: String,
    pub bytes: usize,
    pub link_rtt: Duration,
    pub duration: Duration,
}

/// Resource-level transfer estimate, not radio/interface wire accounting.
/// Encoded bytes include compression/encryption/metadata, exclude packet
/// headers and retransmissions, and accumulate across resource segments.
#[derive(Debug, Clone, Copy, Default)]
pub struct RncpTransferProgress {
    pub progress: f32,
    pub encoded_bytes: f64,
    pub elapsed: Duration,
}

struct TransferReporter {
    tx: Option<tokio::sync::watch::Sender<RncpTransferProgress>>,
    started: Option<Instant>,
    segments: std::collections::HashMap<[u8; 32], f64>,
}

impl TransferReporter {
    fn new(tx: Option<tokio::sync::watch::Sender<RncpTransferProgress>>) -> Self {
        Self {
            tx,
            started: None,
            segments: Default::default(),
        }
    }

    fn update(&mut self, hash: [u8; 32], size: usize, fraction: f64, progress: f32) {
        let Some(tx) = self.tx.as_ref() else { return };
        let started = *self.started.get_or_insert_with(Instant::now);
        let bytes = size as f64 * fraction.clamp(0.0, 1.0);
        let previous = self.segments.entry(hash).or_default();
        *previous = previous.max(bytes);
        tx.send_replace(RncpTransferProgress {
            progress: progress.clamp(0.0, 1.0),
            encoded_bytes: self.segments.values().sum(),
            elapsed: started.elapsed(),
        });
    }
}

pub struct RncpSendRequest<'a> {
    pub transport_tx: mpsc::Sender<TransportMessage>,
    pub identity: Identity,
    pub dest_hash: [u8; 16],
    pub file_name: &'a str,
    pub data: Vec<u8>,
    pub auto_compress: bool,
    pub overall_timeout: Duration,
    pub path_wait: Duration,
    pub progress_tx: Option<mpsc::Sender<f32>>,
}

/// Known-length reader source; read sequentially from its current position.
pub struct RncpSendReaderRequest<'a, R> {
    pub transport_tx: mpsc::Sender<TransportMessage>,
    pub identity: Identity,
    pub dest_hash: [u8; 16],
    pub file_name: &'a str,
    pub reader: R,
    pub data_size: usize,
    pub auto_compress: bool,
    pub overall_timeout: Duration,
    pub path_wait: Duration,
    pub progress_tx: Option<mpsc::Sender<f32>>,
}

#[derive(Debug, Clone)]
pub enum RncpEvent {
    LinkEstablished {
        link_id: [u8; 16],
    },
    /// Zero `identity_hash` = remote never identified; only accepted under `allow_all`.
    SenderIdentified {
        link_id: [u8; 16],
        identity_hash: [u8; 16],
    },
    SenderDenied {
        link_id: [u8; 16],
        identity_hash: [u8; 16],
    },
    Completed {
        link_id: [u8; 16],
        file_name: String,
        saved_path: PathBuf,
        bytes: usize,
    },
    WriteFailed {
        link_id: [u8; 16],
        file_name: String,
        reason: String,
    },
    FetchServing {
        link_id: [u8; 16],
        file_name: String,
        bytes: usize,
    },
    FetchDenied {
        link_id: [u8; 16],
        reason: String,
    },
}

/// Resolves a fetch path under the jail root (Python rncp.py:177-183).
/// Absolute paths replace the jail prefix; relatives join onto it; canonical
/// result must stay inside `jail_canonical`.
fn resolve_fetch_path(jail: Option<&Path>, data: &[u8]) -> Result<PathBuf, String> {
    let path_str = std::str::from_utf8(data).map_err(|_| "non-utf8 path".to_string())?;
    if path_str.is_empty() {
        return Err("empty path".into());
    }
    match jail {
        Some(jail_dir) => {
            let jail_canonical =
                std::fs::canonicalize(jail_dir).map_err(|e| format!("jail canonicalize: {e}"))?;
            // canonicalize resolves symlinks (macOS `/var` → `/private/var`)
            // before the containment check.
            let requested = Path::new(path_str);
            let combined = if requested.is_absolute() {
                requested.to_path_buf()
            } else {
                jail_canonical.join(path_str)
            };
            let canonical =
                std::fs::canonicalize(&combined).map_err(|e| format!("not found: {e}"))?;
            if !canonical.starts_with(&jail_canonical) {
                return Err("outside fetch jail".into());
            }
            Ok(canonical)
        }
        None => std::fs::canonicalize(path_str).map_err(|e| format!("not found: {e}")),
    }
}

pub struct RncpListenerConfig {
    pub identity: Identity,
    pub app_name: String,
    pub save_dir: PathBuf,
    /// Python `allow_all` / `-n`.
    pub allow_all: bool,
    /// Ignored when `allow_all` is set.
    pub allowed: Vec<[u8; 16]>,
    pub overwrite: bool,
    /// Python `-F` / `--allow-fetch`.
    pub allow_fetch: bool,
    /// Python `-j` / `--jail`.
    pub fetch_jail: Option<PathBuf>,
    /// Python default on; `--no-compress` disables.
    pub fetch_auto_compress: bool,
}

impl Default for RncpListenerConfig {
    fn default() -> Self {
        Self {
            identity: Identity::new(),
            app_name: DEFAULT_RNCP_APP_NAME.to_string(),
            save_dir: PathBuf::from("."),
            allow_all: false,
            allowed: Vec::new(),
            overwrite: false,
            allow_fetch: false,
            fetch_jail: None,
            fetch_auto_compress: true,
        }
    }
}

pub struct RncpListenerHandle {
    pub destination_hash: [u8; 16],
    task: tokio::task::JoinHandle<()>,
}

impl RncpListenerHandle {
    pub fn destination_hash(&self) -> [u8; 16] {
        self.destination_hash
    }
    pub async fn shutdown(self) {
        self.task.abort();
        let _ = self.task.await;
    }
}

pub async fn spawn_rncp_listener(
    transport_tx: mpsc::Sender<TransportMessage>,
    cfg: RncpListenerConfig,
    events_tx: mpsc::Sender<RncpEvent>,
) -> Result<RncpListenerHandle, RncpError> {
    let signing_key = cfg
        .identity
        .get_signing_key()
        .ok_or(RncpError::NoSigningKey)?;

    let destination = Destination::new(
        Some(&cfg.identity),
        Direction::In,
        DestType::Single,
        &cfg.app_name,
    )
    .map_err(|e| RncpError::HandshakeFailed(format!("destination: {e:?}")))?;
    let dest_hash = destination.hash;

    let (dest_tx, dest_rx) = mpsc::channel::<DestinationEvent>(256);
    let link_control_tx = dest_tx.clone();
    transport_tx
        .send(TransportMessage::RegisterDestination {
            hash: dest_hash,
            app_name: cfg.app_name.clone(),
            delivery_tx: Some(dest_tx),
        })
        .await
        .map_err(|_| RncpError::TransportUnavailable)?;

    let mut link_mgr = LinkManager::with_destination(
        transport_tx.clone(),
        dest_rx,
        &cfg.identity,
        &cfg.app_name,
        Some(signing_key),
    );

    let (completion_tx, mut completion_rx) = mpsc::channel::<ResourceCompletion>(64);
    link_mgr.set_resource_completion_channel(completion_tx);

    // Gate the allow-list before parts arrive.
    let (identified_tx, mut identified_rx) = mpsc::channel::<([u8; 16], [u8; 16])>(64);
    link_mgr.set_link_identified_channel(identified_tx);

    if !cfg.allow_fetch && !cfg.allow_all {
        let allowed_push: std::collections::HashSet<[u8; 16]> =
            cfg.allowed.iter().copied().collect();
        link_mgr.set_link_identity_gate(move |_link_id, identity_hash| {
            allowed_push.contains(&identity_hash)
        });
    }

    if cfg.allow_fetch {
        let jail = cfg.fetch_jail.clone();
        let auto_compress = cfg.fetch_auto_compress;
        let allowed_fetch: Arc<std::collections::HashSet<[u8; 16]>> =
            Arc::new(cfg.allowed.iter().copied().collect());
        let allow_all_fetch = cfg.allow_all;
        let link_identities = link_mgr.link_identities_handle();
        let fetch_events = events_tx.clone();
        let fetch_path_hash = truncated_hash(FETCH_PATH_NAME.as_bytes());
        link_mgr.set_request_handler_ex(move |link_id, path_hash, data| {
            if path_hash != fetch_path_hash {
                return RequestOutcome::Drop;
            }
            // Allow-all bypasses ident check; else peer must have sent
            // LinkIdentify with an identity in `allowed`.
            if !allow_all_fetch {
                let ident = link_identities
                    .lock()
                    .ok()
                    .and_then(|m| m.get(&link_id).copied());
                match ident {
                    Some(h) if allowed_fetch.contains(&h) => {}
                    _ => {
                        let _ = fetch_events.try_send(RncpEvent::FetchDenied {
                            link_id,
                            reason: "not in allow-list".into(),
                        });
                        return RequestOutcome::Reply(vec![REQ_FETCH_NOT_ALLOWED]);
                    }
                }
            }
            match resolve_fetch_path(jail.as_deref(), &data) {
                Ok(file_path) => match std::fs::File::open(&file_path) {
                    Ok(file) => {
                        let size = match file
                            .metadata()
                            .ok()
                            .filter(|m| m.is_file())
                            .and_then(|m| usize::try_from(m.len()).ok())
                        {
                            Some(size) if size <= rns_protocol::resource::MAX_RESOURCE_SIZE => size,
                            _ => return RequestOutcome::Reply(vec![0xC2]),
                        };
                        let basename = file_path
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let metadata = pack_metadata(&basename);
                        let _ = fetch_events.try_send(RncpEvent::FetchServing {
                            link_id,
                            file_name: basename,
                            bytes: size,
                        });
                        RequestOutcome::ReplyWithFile {
                            ack: vec![0xC3], // msgpack `true`
                            file: Arc::new(file),
                            metadata: Some(metadata),
                            auto_compress,
                        }
                    }
                    Err(_) => RequestOutcome::Reply(vec![0xC2]), // msgpack `false`
                },
                Err(reason) => {
                    let _ = fetch_events.try_send(RncpEvent::FetchDenied { link_id, reason });
                    RequestOutcome::Reply(vec![REQ_FETCH_NOT_ALLOWED])
                }
            }
        });
    }

    let app_name = cfg.app_name.clone();
    let save_dir = cfg.save_dir.clone();
    let allow_all = cfg.allow_all;
    let allowed: std::collections::HashSet<[u8; 16]> = cfg.allowed.iter().copied().collect();
    let overwrite = cfg.overwrite;
    let allow_fetch_outer = cfg.allow_fetch;
    let events_tx_cb = events_tx.clone();
    let link_control_tx_cb = link_control_tx.clone();

    let task = tokio::spawn(async move {
        let lm_task = tokio::spawn(async move { link_mgr.run().await });

        let mut denied_links: std::collections::HashSet<[u8; 16]> = Default::default();

        loop {
            tokio::select! {
                ident = identified_rx.recv() => {
                    let Some((link_id, ident_hash)) = ident else { break };
                    if allow_all || allowed.contains(&ident_hash) {
                        let _ = events_tx_cb
                            .send(RncpEvent::SenderIdentified {
                                link_id,
                                identity_hash: ident_hash,
                            })
                            .await;
                    } else {
                        denied_links.insert(link_id);
                        let _ = events_tx_cb
                            .send(RncpEvent::SenderDenied {
                                link_id,
                                identity_hash: ident_hash,
                            })
                            .await;
                        // Keep the link open under fetch so the handler can
                        // reply REQ_FETCH_NOT_ALLOWED rather than a generic close.
                        if !allow_fetch_outer {
                            let _ = link_control_tx_cb
                                .send(DestinationEvent::LinkClosed { link_id })
                                .await;
                        }
                    }
                }
                completion = completion_rx.recv() => {
                    let Some(evt) = completion else { break };
                    if denied_links.contains(&evt.link_id) {
                        continue;
                    }

                    let file_name = extract_filename(evt.metadata.as_deref())
                        .unwrap_or_else(|| default_file_name(&evt.resource_hash));

                    match save_payload(&save_dir, &file_name, &evt.data, overwrite).await {
                        Ok(path) => {
                            let _ = events_tx_cb
                                .send(RncpEvent::Completed {
                                    link_id: evt.link_id,
                                    file_name,
                                    saved_path: path,
                                    bytes: evt.data.len(),
                                })
                                .await;
                        }
                        Err(e) => {
                            let _ = events_tx_cb
                                .send(RncpEvent::WriteFailed {
                                    link_id: evt.link_id,
                                    file_name,
                                    reason: e,
                                })
                                .await;
                        }
                    }
                }
            }
        }

        let _ = lm_task.await;
    });

    tracing::info!(
        dest = hex::encode(dest_hash),
        app_name = %app_name,
        "rncp listener started"
    );

    Ok(RncpListenerHandle {
        destination_hash: dest_hash,
        task,
    })
}

fn extract_filename(metadata: Option<&[u8]>) -> Option<String> {
    let meta = metadata?;
    // Envelope: msgpack({"name": b"filename"}).
    let map: rmpv::Value = rmpv::decode::read_value(&mut std::io::Cursor::new(meta)).ok()?;
    let entries = match map {
        rmpv::Value::Map(m) => m,
        _ => return None,
    };
    for (k, v) in entries {
        let key = match k {
            rmpv::Value::String(s) => s.into_str()?,
            rmpv::Value::Binary(b) => String::from_utf8(b).ok()?,
            _ => continue,
        };
        if key == "name" {
            let bytes = match v {
                rmpv::Value::Binary(b) => b,
                rmpv::Value::String(s) => s.into_bytes(),
                _ => continue,
            };
            return String::from_utf8(bytes).ok();
        }
    }
    None
}

fn default_file_name(resource_hash: &[u8; 32]) -> String {
    format!("rncp_{}", hex::encode(&resource_hash[..8]))
}

async fn save_payload(
    save_dir: &Path,
    file_name: &str,
    data: &[u8],
    overwrite: bool,
) -> Result<PathBuf, String> {
    // Strip path traversal components.
    let basename = Path::new(file_name)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_name.to_string());
    if basename.is_empty() {
        return Err("empty filename".into());
    }

    tokio::fs::create_dir_all(save_dir)
        .await
        .map_err(|e| format!("create save_dir: {e}"))?;
    let mut path = save_dir.join(&basename);
    if !overwrite && path.exists() {
        let mut i = 1;
        loop {
            let candidate = save_dir.join(format!("{basename}.{i}"));
            if !candidate.exists() {
                path = candidate;
                break;
            }
            i += 1;
        }
    }
    tokio::fs::write(&path, data)
        .await
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
}

fn build_link_close(link: &mut Link) -> Option<Bytes> {
    let link_id = link.link_id;
    let teardown_data = link.teardown(CloseReason::InitiatorClosed)?;
    Some(build_data_packet(
        link_id,
        rns_wire::context::PacketContext::LinkClose,
        &teardown_data,
    ))
}

pub async fn rncp_send_file(request: RncpSendRequest<'_>) -> Result<RncpOutcome, RncpError> {
    let data_size = request.data.len();
    rncp_send_reader(RncpSendReaderRequest {
        transport_tx: request.transport_tx,
        identity: request.identity,
        dest_hash: request.dest_hash,
        file_name: request.file_name,
        reader: std::io::Cursor::new(request.data),
        data_size,
        auto_compress: request.auto_compress,
        overall_timeout: request.overall_timeout,
        path_wait: request.path_wait,
        progress_tx: request.progress_tx,
    })
    .await
}

pub async fn rncp_send_reader<R: tokio::io::AsyncRead + Unpin>(
    request: RncpSendReaderRequest<'_, R>,
) -> Result<RncpOutcome, RncpError> {
    rncp_send_reader_with_stats(request, None).await
}

/// Reader send with optional latest-value Resource transfer statistics.
/// The original request type and fractional progress API are unchanged.
pub async fn rncp_send_reader_with_stats<R: tokio::io::AsyncRead + Unpin>(
    request: RncpSendReaderRequest<'_, R>,
    stats_tx: Option<tokio::sync::watch::Sender<RncpTransferProgress>>,
) -> Result<RncpOutcome, RncpError> {
    let mut stats = TransferReporter::new(stats_tx);
    use tokio::io::AsyncReadExt;
    let RncpSendReaderRequest {
        transport_tx,
        identity,
        dest_hash,
        file_name,
        mut reader,
        data_size: byte_count,
        auto_compress,
        overall_timeout,
        path_wait,
        progress_tx,
    } = request;

    let started = Instant::now();
    let deadline = started + overall_timeout;
    let mut metadata_bytes = Some(pack_metadata(file_name));
    let metadata_size = metadata_bytes
        .as_ref()
        .unwrap()
        .len()
        .checked_add(3)
        .ok_or_else(|| RncpError::ResourceCreate("metadata size overflow".into()))?;
    let total_size = byte_count
        .checked_add(metadata_size)
        .ok_or_else(|| RncpError::ResourceCreate("resource size overflow".into()))?;
    let total_segments = total_size.div_ceil(MAX_EFFICIENT_SIZE).max(1);
    if metadata_size > MAX_EFFICIENT_SIZE
        || byte_count > rns_protocol::resource::MAX_RESOURCE_SIZE
        || total_segments > rns_protocol::resource::MAX_SEGMENTS
    {
        return Err(RncpError::ResourceCreate(
            "source exceeds Resource size limits".into(),
        ));
    }

    let pubkey =
        discover_pubkey(&transport_tx, dest_hash, path_wait.min(remaining(deadline))).await?;

    let (mut link, link_request_data) = Link::new_initiator(dest_hash, 1);
    let link_id = link.link_id;

    // Route LRPROOF / link packets back to this task.
    let (lpkt_tx, mut lpkt_rx) = mpsc::channel::<DestinationEvent>(256);
    transport_tx
        .send(TransportMessage::RegisterDestination {
            hash: link_id,
            app_name: "rncp.sender".to_string(),
            delivery_tx: Some(lpkt_tx),
        })
        .await
        .map_err(|_| RncpError::TransportUnavailable)?;

    let link_req_pkt = build_link_request_packet(dest_hash, &link_request_data);
    transport_tx
        .send(TransportMessage::Outbound(OutboundRequest {
            raw: link_req_pkt,
            destination_hash: dest_hash,
        }))
        .await
        .map_err(|_| RncpError::TransportUnavailable)?;

    let proof_body = match timeout(remaining(deadline), wait_for_proof(&mut lpkt_rx, link_id)).await
    {
        Ok(Ok(body)) => body,
        Ok(Err(e)) => {
            cleanup_destination(&transport_tx, link_id).await;
            return Err(e);
        }
        Err(_) => {
            cleanup_destination(&transport_tx, link_id).await;
            return Err(RncpError::Timeout("link proof"));
        }
    };

    let identity_ed25519_pub: [u8; 32] = pubkey[32..64]
        .try_into()
        .map_err(|_| RncpError::ProofInvalid("remote pub key length".into()))?;
    let verify_key = Ed25519PublicKey::from_bytes(&identity_ed25519_pub)
        .map_err(|e| RncpError::ProofInvalid(format!("verify key: {e}")))?;

    let rtt_data = link
        .validate_proof(&proof_body, &verify_key, &identity_ed25519_pub)
        .map_err(|e| RncpError::ProofInvalid(format!("{e:?}")))?;

    let rtt_pkt = build_data_packet(link_id, rns_wire::context::PacketContext::Lrrtt, &rtt_data);
    transport_tx
        .send(TransportMessage::Outbound(OutboundRequest {
            raw: rtt_pkt,
            destination_hash: link_id,
        }))
        .await
        .map_err(|_| RncpError::TransportUnavailable)?;

    let our_pub = identity.get_public_key();
    let our_priv = identity.get_signing_key().ok_or(RncpError::NoSigningKey)?;
    let ident_data = link
        .identify(&our_pub, &our_priv)
        .map_err(|e| RncpError::LinkCrypto(format!("identify: {e:?}")))?;
    let ident_pkt = build_data_packet(
        link_id,
        rns_wire::context::PacketContext::LinkIdentify,
        &ident_data,
    );
    transport_tx
        .send(TransportMessage::Outbound(OutboundRequest {
            raw: ident_pkt,
            destination_hash: link_id,
        }))
        .await
        .map_err(|_| RncpError::TransportUnavailable)?;

    // Encrypt full payload BEFORE chunking (Resource.py:424).
    let session_keys = link
        .session_keys()
        .ok_or_else(|| RncpError::LinkCrypto("no session keys".into()))?;
    let session_keys_enc = session_keys.clone();
    let encrypt_fn = move |plaintext: &[u8]| -> Vec<u8> {
        link_encrypt(&session_keys_enc, plaintext).unwrap_or_default()
    };

    let link_rtt = link.rtt.unwrap_or(Duration::from_millis(500));
    let mut resource_hash = None;
    let mut total_parts = 0;
    let transfer_result = async {
        let mut consumed = 0;
        for index in 1..=total_segments {
            let capacity = MAX_EFFICIENT_SIZE - if index == 1 { metadata_size } else { 0 };
            let mut chunk = vec![0; (byte_count - consumed).min(capacity)];
            timeout(remaining(deadline), reader.read_exact(&mut chunk))
                .await
                .map_err(|_| RncpError::Timeout("resource source"))?
                .map_err(|error| RncpError::Io(error.to_string()))?;
            let chunk_size = chunk.len();
            let mut resource = OutboundResource::with_options(
                chunk,
                auto_compress,
                metadata_bytes.take(),
                None,
                Some(&encrypt_fn),
            )
            .map_err(|error| RncpError::ResourceCreate(format!("{error:?}")))?;
            let original = *resource_hash.get_or_insert(resource.resource_hash);
            resource.flags.split = total_segments > 1;
            resource.segment_index = index;
            resource.total_segments = total_segments;
            resource.advertisement_data_size = total_size;
            resource.original_hash = (total_segments > 1).then_some(original);
            total_parts += resource.parts.len();
            drive_outbound(OutboundDrive {
                transport_tx: &transport_tx,
                link: &mut link,
                link_id,
                resource,
                lpkt_rx: &mut lpkt_rx,
                progress_tx: progress_tx.clone(),
                stats: &mut stats,
                progress_base: consumed as f32 / byte_count.max(1) as f32,
                progress_span: if byte_count == 0 {
                    1.0
                } else {
                    chunk_size as f32 / byte_count as f32
                },
                deadline,
            })
            .await?;
            consumed += chunk_size;
        }
        Ok::<(), RncpError>(())
    }
    .await;

    if let Some(close_pkt) = build_link_close(&mut link) {
        let _ = transport_tx
            .send(TransportMessage::Outbound(OutboundRequest {
                raw: close_pkt,
                destination_hash: link_id,
            }))
            .await;
    }
    cleanup_destination(&transport_tx, link_id).await;

    transfer_result?;

    tracing::info!(
        dest = hex::encode(dest_hash),
        link_id = hex::encode(link_id),
        resource = hex::encode(&resource_hash.unwrap_or([0; 32])[..8]),
        bytes = byte_count,
        parts = total_parts,
        "rncp send complete"
    );

    Ok(RncpOutcome {
        file_name: file_name.to_string(),
        bytes: byte_count,
        link_rtt,
        duration: started.elapsed(),
    })
}

fn pack_metadata(file_name: &str) -> Vec<u8> {
    let entries = vec![(
        rmpv::Value::String(rmpv::Utf8String::from("name")),
        rmpv::Value::Binary(file_name.as_bytes().to_vec()),
    )];
    let mut buf = Vec::new();
    let _ = rmpv::encode::write_value(&mut buf, &rmpv::Value::Map(entries));
    buf
}

struct OutboundDrive<'a> {
    stats: &'a mut TransferReporter,
    transport_tx: &'a mpsc::Sender<TransportMessage>,
    link: &'a mut Link,
    link_id: [u8; 16],
    resource: OutboundResource,
    lpkt_rx: &'a mut mpsc::Receiver<DestinationEvent>,
    progress_tx: Option<mpsc::Sender<f32>>,
    progress_base: f32,
    progress_span: f32,
    deadline: Instant,
}

async fn drive_outbound(request: OutboundDrive<'_>) -> Result<(), RncpError> {
    let OutboundDrive {
        stats,
        transport_tx,
        link,
        link_id,
        resource,
        lpkt_rx,
        progress_tx,
        progress_base,
        progress_span,
        deadline,
    } = request;

    let resource_hash = resource.resource_hash;
    let future = async {
        let rtt = link.rtt.unwrap_or(Duration::from_millis(500));
        let mut transfer = OutboundTransfer::from_prebuilt(resource, rtt);
        let total = transfer.resource.parts.len();
        let encoded_size = transfer.resource.total_size;
        stats.update(resource_hash, encoded_size, 0.0, progress_base);
        if let Some(ref tx) = progress_tx {
            let _ = tx.try_send(if total == 0 {
                progress_base + progress_span
            } else {
                progress_base
            });
        }

        send_transfer_action(transport_tx, link, link_id, transfer.tick()).await?;
        loop {
            if Instant::now() >= deadline {
                let _ = crate::link_client::send_link_data(
                    transport_tx,
                    link,
                    link_id,
                    rns_wire::context::PacketContext::ResourceIcl,
                    &transfer.resource.resource_hash,
                    true,
                );
                return Err(RncpError::Timeout("resource proof"));
            }

            let action = transfer.check_sender_timeout(link_id);
            let cancelled = matches!(action, TransferAction::SendCancel(_, _));
            send_transfer_action(transport_tx, link, link_id, action).await?;
            if cancelled {
                return Err(RncpError::ResourceFailed("sender watchdog expired".into()));
            }

            let recv_timeout = remaining(deadline).min(Duration::from_secs(1));
            let ev = match tokio::time::timeout(recv_timeout, lpkt_rx.recv()).await {
                Ok(Some(ev)) => ev,
                Ok(None) => return Err(RncpError::ResourceFailed("link channel closed".into())),
                Err(_) => continue,
            };

            let DestinationEvent::InboundPacket { raw, .. } = ev else {
                continue;
            };
            let (header, off) = match rns_wire::header::PacketHeader::unpack(&raw) {
                Ok(h) => h,
                Err(_) => continue,
            };
            if header.destination_hash != link_id {
                continue;
            }
            let body = &raw[off..];
            match header.context {
                rns_wire::context::PacketContext::Keepalive
                    if body.first() == Some(&rns_link::constants::KEEPALIVE_REQUEST) =>
                {
                    send_keepalive_response(transport_tx, link_id).await?;
                }
                rns_wire::context::PacketContext::ResourceReq => {
                    let Ok(plaintext) = link.decrypt(body) else {
                        continue;
                    };
                    let packet_hash = rns_wire::hash::packet_hash(&raw, header.flags.header_type);
                    for action in transfer.handle_request_packet(packet_hash, &plaintext) {
                        let cancelled = matches!(action, TransferAction::SendCancel(_, _));
                        send_transfer_action(transport_tx, link, link_id, action).await?;
                        if cancelled {
                            return Err(RncpError::ResourceFailed(
                                "invalid resource request".into(),
                            ));
                        }
                    }
                    let frac = transfer.progress();
                    let progress = progress_base + progress_span * frac as f32;
                    stats.update(resource_hash, encoded_size, frac, progress);
                    if let Some(ref tx) = progress_tx {
                        let _ = tx.try_send(progress);
                    }
                }
                rns_wire::context::PacketContext::ResourcePrf => {
                    // PROOF+RESOURCE_PRF plaintext = resource_hash(32) || proof(32) (Packet.py:195-197).
                    if body.len() < 64 {
                        continue;
                    }
                    if !transfer.handle_proof(body) {
                        return Err(RncpError::ResourceFailed("proof mismatch".into()));
                    }
                    stats.update(
                        resource_hash,
                        encoded_size,
                        1.0,
                        progress_base + progress_span,
                    );
                    if let Some(ref tx) = progress_tx {
                        let _ = tx.try_send(progress_base + progress_span);
                    }
                    return Ok(());
                }
                rns_wire::context::PacketContext::ResourceRcl => {
                    if link.decrypt(body).ok().is_some_and(|data| {
                        data.get(..32) == Some(transfer.resource.resource_hash.as_slice())
                    }) {
                        return Err(RncpError::ResourceFailed("receiver cancelled".into()));
                    }
                }
                rns_wire::context::PacketContext::LinkClose if link.receive_teardown(body) => {
                    return Err(RncpError::ResourceFailed("link closed before proof".into()));
                }
                _ => {}
            }
        }
    };
    let result = tokio::time::timeout(remaining(deadline), future).await;
    if result.is_err() {
        let _ = crate::link_client::send_link_data(
            transport_tx,
            link,
            link_id,
            rns_wire::context::PacketContext::ResourceIcl,
            &resource_hash,
            true,
        );
    }
    result.map_err(|_| RncpError::Timeout("resource proof"))?
}

async fn send_keepalive_response(
    transport_tx: &mpsc::Sender<TransportMessage>,
    link_id: [u8; 16],
) -> Result<(), RncpError> {
    let raw = build_data_packet(
        link_id,
        rns_wire::context::PacketContext::Keepalive,
        &[rns_link::constants::KEEPALIVE_RESPONSE],
    );
    transport_tx
        .send(TransportMessage::Outbound(OutboundRequest {
            raw,
            destination_hash: link_id,
        }))
        .await
        .map_err(|_| RncpError::TransportUnavailable)
}

async fn send_transfer_action(
    transport_tx: &mpsc::Sender<TransportMessage>,
    link: &Link,
    link_id: [u8; 16],
    action: TransferAction,
) -> Result<(), RncpError> {
    if let TransferAction::SendProof(proof) = action {
        let raw = build_resource_proof_packet(link_id, &proof);
        transport_tx
            .send(TransportMessage::Outbound(OutboundRequest {
                raw,
                destination_hash: link_id,
            }))
            .await
            .map_err(|_| RncpError::TransportUnavailable)?;
        return Ok(());
    }

    let (context, body) = match action {
        TransferAction::QueryProof(hash) => (
            rns_wire::context::PacketContext::CacheRequest,
            Bytes::copy_from_slice(&hash),
        ),
        TransferAction::SendAdvertisement(adv) => {
            let encrypted = link
                .encrypt(&adv)
                .map_err(|e| RncpError::LinkCrypto(format!("adv encrypt: {e:?}")))?;
            (
                rns_wire::context::PacketContext::ResourceAdv,
                Bytes::from(encrypted),
            )
        }
        TransferAction::SendPart(_, part) => (
            rns_wire::context::PacketContext::Resource,
            Bytes::from(part),
        ),
        TransferAction::SendHmu(hmu) => {
            let encrypted = link
                .encrypt(&hmu)
                .map_err(|e| RncpError::LinkCrypto(format!("hmu encrypt: {e:?}")))?;
            (
                rns_wire::context::PacketContext::ResourceHmu,
                Bytes::from(encrypted),
            )
        }
        TransferAction::SendRequest(req) => {
            let encrypted = link
                .encrypt(&req)
                .map_err(|e| RncpError::LinkCrypto(format!("request encrypt: {e:?}")))?;
            (
                rns_wire::context::PacketContext::ResourceReq,
                Bytes::from(encrypted),
            )
        }
        TransferAction::SendCancel(cancel_type, resource_hash) => {
            let encrypted = link
                .encrypt(&resource_hash)
                .map_err(|e| RncpError::LinkCrypto(format!("cancel encrypt: {e:?}")))?;
            let context = match cancel_type {
                rns_protocol::resource::CancelType::Icl => {
                    rns_wire::context::PacketContext::ResourceIcl
                }
                rns_protocol::resource::CancelType::Rcl => {
                    rns_wire::context::PacketContext::ResourceRcl
                }
            };
            (context, Bytes::from(encrypted))
        }
        TransferAction::SendProof(_) => unreachable!("proof handled before data-packet match"),
        TransferAction::None | TransferAction::Complete => return Ok(()),
        TransferAction::Failed(reason) => return Err(RncpError::ResourceFailed(reason)),
    };
    let raw = build_data_packet(link_id, context, &body);
    transport_tx
        .send(TransportMessage::Outbound(OutboundRequest {
            raw,
            destination_hash: link_id,
        }))
        .await
        .map_err(|_| RncpError::TransportUnavailable)
}

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

async fn discover_pubkey(
    transport_tx: &mpsc::Sender<TransportMessage>,
    dest_hash: [u8; 16],
    path_wait: Duration,
) -> Result<[u8; 64], RncpError> {
    if let Some(pk) = lookup_pubkey(transport_tx, dest_hash).await? {
        return Ok(pk);
    }

    transport_tx
        .send(TransportMessage::RequestPath {
            destination_hash: dest_hash,
        })
        .await
        .map_err(|_| RncpError::TransportUnavailable)?;
    let (await_tx, await_rx) = oneshot::channel();
    transport_tx
        .send(TransportMessage::AwaitPath {
            dest: dest_hash,
            reply: await_tx,
        })
        .await
        .map_err(|_| RncpError::TransportUnavailable)?;
    let found = timeout(path_wait, await_rx)
        .await
        .map_err(|_| RncpError::PathTimeout)?
        .unwrap_or(false);
    if !found {
        return Err(RncpError::PathTimeout);
    }

    lookup_pubkey(transport_tx, dest_hash)
        .await?
        .ok_or(RncpError::NoIdentity)
}

/// `Ok(None)` = peer not in announce cache.
async fn lookup_pubkey(
    transport_tx: &mpsc::Sender<TransportMessage>,
    dest_hash: [u8; 16],
) -> Result<Option<[u8; 64]>, RncpError> {
    let (resp_tx, resp_rx) = oneshot::channel();
    transport_tx
        .send(TransportMessage::Rpc {
            query: TransportQuery::GetRecentAnnounces,
            response_tx: resp_tx,
        })
        .await
        .map_err(|_| RncpError::TransportUnavailable)?;
    let resp = resp_rx.await.map_err(|_| RncpError::TransportUnavailable)?;
    let announces: Vec<AnnounceRpcEntry> = match resp {
        TransportQueryResponse::Announces(v) => v,
        _ => Vec::new(),
    };
    Ok(announces
        .into_iter()
        .find(|a| a.dest_hash == dest_hash)
        .and_then(|a| a.public_key))
}

async fn wait_for_proof(
    rx: &mut mpsc::Receiver<DestinationEvent>,
    link_id: [u8; 16],
) -> Result<Vec<u8>, RncpError> {
    while let Some(ev) = rx.recv().await {
        let DestinationEvent::InboundPacket { raw, .. } = ev else {
            continue;
        };
        let Ok((header, off)) = rns_wire::header::PacketHeader::unpack(&raw) else {
            continue;
        };
        if header.flags.packet_type == rns_wire::flags::PacketType::Proof
            && header.destination_hash == link_id
            && raw.len() > off
        {
            return Ok(raw[off..].to_vec());
        }
    }
    Err(RncpError::HandshakeFailed("channel closed".into()))
}

async fn cleanup_destination(transport_tx: &mpsc::Sender<TransportMessage>, link_id: [u8; 16]) {
    let _ = transport_tx.try_send(TransportMessage::DeregisterDestination { hash: link_id });
}

fn build_link_request_packet(dest_hash: [u8; 16], request_data: &[u8]) -> Bytes {
    let header = rns_wire::header::PacketHeader {
        flags: rns_wire::flags::PacketFlags {
            header_type: rns_wire::flags::HeaderType::Header1,
            context_flag: false,
            transport_type: rns_wire::flags::TransportType::Broadcast,
            destination_type: rns_wire::flags::DestinationType::Single,
            packet_type: rns_wire::flags::PacketType::LinkRequest,
        },
        hops: 0,
        transport_id: None,
        destination_hash: dest_hash,
        context: rns_wire::context::PacketContext::None,
    };
    let mut raw = header.pack().expect("locally constructed header");
    raw.extend_from_slice(request_data);
    Bytes::from(raw)
}

fn build_data_packet(
    link_id: [u8; 16],
    context: rns_wire::context::PacketContext,
    body: &[u8],
) -> Bytes {
    let header = rns_wire::header::PacketHeader {
        flags: rns_wire::flags::PacketFlags {
            header_type: rns_wire::flags::HeaderType::Header1,
            context_flag: false,
            transport_type: rns_wire::flags::TransportType::Broadcast,
            destination_type: rns_wire::flags::DestinationType::Link,
            packet_type: rns_wire::flags::PacketType::Data,
        },
        hops: 0,
        transport_id: None,
        destination_hash: link_id,
        context,
    };
    let mut raw = header.pack().expect("locally constructed header");
    raw.extend_from_slice(body);
    Bytes::from(raw)
}

fn build_resource_proof_packet(link_id: [u8; 16], proof: &[u8]) -> Bytes {
    let header = rns_wire::header::PacketHeader {
        flags: rns_wire::flags::PacketFlags {
            header_type: rns_wire::flags::HeaderType::Header1,
            context_flag: false,
            transport_type: rns_wire::flags::TransportType::Broadcast,
            destination_type: rns_wire::flags::DestinationType::Link,
            packet_type: rns_wire::flags::PacketType::Proof,
        },
        hops: 0,
        transport_id: None,
        destination_hash: link_id,
        context: rns_wire::context::PacketContext::ResourcePrf,
    };
    let mut raw = header.pack().expect("locally constructed header");
    raw.extend_from_slice(proof);
    Bytes::from(raw)
}

#[derive(Debug, Clone)]
pub struct RncpFetchOutcome {
    pub file_name: String,
    pub saved_path: PathBuf,
    pub bytes: usize,
    pub link_rtt: Duration,
    pub duration: Duration,
}

pub struct RncpFetchRequest<'a> {
    pub transport_tx: mpsc::Sender<TransportMessage>,
    pub identity: Identity,
    pub dest_hash: [u8; 16],
    pub remote_path: &'a str,
    pub save_dir: &'a Path,
    pub overwrite: bool,
    pub overall_timeout: Duration,
    pub path_wait: Duration,
    pub progress_tx: Option<mpsc::Sender<f32>>,
}

/// Fetches a remote file via `rncp --fetch`. Wire flow:
/// link + identify → REQUEST `fetch_file` → ack byte (`0xC3` ok, `0xC2` not
/// found, [`REQ_FETCH_NOT_ALLOWED`] denied) → resource → write to `save_dir`.
pub async fn rncp_fetch_file(request: RncpFetchRequest<'_>) -> Result<RncpFetchOutcome, RncpError> {
    rncp_fetch_file_with_stats(request, None).await
}

/// Fetch with optional Resource-level statistics, preserving the legacy API.
pub async fn rncp_fetch_file_with_stats(
    request: RncpFetchRequest<'_>,
    stats_tx: Option<tokio::sync::watch::Sender<RncpTransferProgress>>,
) -> Result<RncpFetchOutcome, RncpError> {
    let mut stats = TransferReporter::new(stats_tx);
    use rns_protocol::resource::{
        InboundTransfer, MAX_SEGMENTS, MultiSegmentInbound, TransferAction,
    };

    let RncpFetchRequest {
        transport_tx,
        identity,
        dest_hash,
        remote_path,
        save_dir,
        overwrite,
        overall_timeout,
        path_wait,
        progress_tx,
    } = request;

    let started = Instant::now();
    let deadline = started + overall_timeout;

    let pubkey =
        discover_pubkey(&transport_tx, dest_hash, path_wait.min(remaining(deadline))).await?;

    let (mut link, link_request_data) = Link::new_initiator(dest_hash, 1);
    let link_id = link.link_id;

    let (lpkt_tx, mut lpkt_rx) = mpsc::channel::<DestinationEvent>(256);
    transport_tx
        .send(TransportMessage::RegisterDestination {
            hash: link_id,
            app_name: "rncp.fetch".to_string(),
            delivery_tx: Some(lpkt_tx),
        })
        .await
        .map_err(|_| RncpError::TransportUnavailable)?;

    let link_req_pkt = build_link_request_packet(dest_hash, &link_request_data);
    transport_tx
        .send(TransportMessage::Outbound(OutboundRequest {
            raw: link_req_pkt,
            destination_hash: dest_hash,
        }))
        .await
        .map_err(|_| RncpError::TransportUnavailable)?;

    let proof_body = match timeout(remaining(deadline), wait_for_proof(&mut lpkt_rx, link_id)).await
    {
        Ok(Ok(body)) => body,
        Ok(Err(e)) => {
            cleanup_destination(&transport_tx, link_id).await;
            return Err(e);
        }
        Err(_) => {
            cleanup_destination(&transport_tx, link_id).await;
            return Err(RncpError::Timeout("link proof"));
        }
    };

    let identity_ed25519_pub: [u8; 32] = pubkey[32..64]
        .try_into()
        .map_err(|_| RncpError::ProofInvalid("remote pub key length".into()))?;
    let verify_key = Ed25519PublicKey::from_bytes(&identity_ed25519_pub)
        .map_err(|e| RncpError::ProofInvalid(format!("verify key: {e}")))?;

    let rtt_data = link
        .validate_proof(&proof_body, &verify_key, &identity_ed25519_pub)
        .map_err(|e| RncpError::ProofInvalid(format!("{e:?}")))?;

    let rtt_pkt = build_data_packet(link_id, rns_wire::context::PacketContext::Lrrtt, &rtt_data);
    transport_tx
        .send(TransportMessage::Outbound(OutboundRequest {
            raw: rtt_pkt,
            destination_hash: link_id,
        }))
        .await
        .map_err(|_| RncpError::TransportUnavailable)?;

    let our_pub = identity.get_public_key();
    let our_priv = identity.get_signing_key().ok_or(RncpError::NoSigningKey)?;
    let ident_data = link
        .identify(&our_pub, &our_priv)
        .map_err(|e| RncpError::LinkCrypto(format!("identify: {e:?}")))?;
    let ident_pkt = build_data_packet(
        link_id,
        rns_wire::context::PacketContext::LinkIdentify,
        &ident_data,
    );
    transport_tx
        .send(TransportMessage::Outbound(OutboundRequest {
            raw: ident_pkt,
            destination_hash: link_id,
        }))
        .await
        .map_err(|_| RncpError::TransportUnavailable)?;

    let (req_encrypted, _req_id) = link
        .request_str(FETCH_PATH_NAME, remote_path, remaining(deadline))
        .map_err(|e| RncpError::LinkCrypto(format!("request: {e:?}")))?;
    let req_pkt = build_data_packet(
        link_id,
        rns_wire::context::PacketContext::Request,
        &req_encrypted,
    );
    let packet_request_id =
        rns_wire::hash::truncated_packet_hash(&req_pkt, rns_wire::flags::HeaderType::Header1);
    link.update_pending_request_id(&_req_id, packet_request_id);
    transport_tx
        .send(TransportMessage::Outbound(OutboundRequest {
            raw: req_pkt,
            destination_hash: link_id,
        }))
        .await
        .map_err(|_| RncpError::TransportUnavailable)?;

    let session_keys = link
        .session_keys()
        .ok_or_else(|| RncpError::LinkCrypto("no session keys".into()))?;
    let session_keys_dec = session_keys.clone();
    let decrypt_fn =
        move |ciphertext: &[u8]| -> Result<Vec<u8>, rns_protocol::resource::ResourceError> {
            rns_link::encryption::link_decrypt(&session_keys_dec, ciphertext)
                .map_err(|_| rns_protocol::resource::ResourceError::DecryptFailed)
        };

    let link_rtt = link.rtt.unwrap_or(Duration::from_millis(500));
    let mut ack_seen = false;
    let mut transfers: std::collections::HashMap<[u8; 32], InboundTransfer> =
        std::collections::HashMap::new();
    let mut split_resources: std::collections::HashMap<[u8; 32], MultiSegmentInbound> =
        std::collections::HashMap::new();
    let mut segment_routes: std::collections::HashMap<[u8; 32], ([u8; 32], usize)> =
        std::collections::HashMap::new();

    let result: Result<(String, Vec<u8>), RncpError> = loop {
        if Instant::now() >= deadline {
            break Err(RncpError::Timeout("fetch"));
        }
        let recv_timeout = remaining(deadline).min(Duration::from_secs(5));
        let ev = match tokio::time::timeout(recv_timeout, lpkt_rx.recv()).await {
            Ok(Some(ev)) => ev,
            Ok(None) => break Err(RncpError::ResourceFailed("link channel closed".into())),
            Err(_) => continue,
        };

        let DestinationEvent::InboundPacket { raw, .. } = ev else {
            continue;
        };
        let (header, off) = match rns_wire::header::PacketHeader::unpack(&raw) {
            Ok(h) => h,
            Err(_) => continue,
        };
        if header.destination_hash != link_id {
            continue;
        }
        let body = &raw[off..];
        match header.context {
            rns_wire::context::PacketContext::Keepalive
                if body.first() == Some(&rns_link::constants::KEEPALIVE_REQUEST) =>
            {
                send_keepalive_response(&transport_tx, link_id).await?;
            }
            rns_wire::context::PacketContext::Response => {
                let Ok((_req_id, resp)) = link.handle_response(body) else {
                    continue;
                };
                match resp.first().copied() {
                    Some(0xC3) => ack_seen = true,
                    Some(0xC2) => break Err(RncpError::ResourceFailed("remote: not found".into())),
                    Some(REQ_FETCH_NOT_ALLOWED) => break Err(RncpError::Denied),
                    _ => break Err(RncpError::ResourceFailed("unknown ack".into())),
                }
            }
            rns_wire::context::PacketContext::ResourceAdv => {
                if !ack_seen {
                    // Some implementations race ADV with ack; treat ADV as implicit consent.
                    ack_seen = true;
                }
                let Ok(plaintext) = link.decrypt(body) else {
                    continue;
                };
                let Ok(adv) = rns_protocol::resource_adv::ResourceAdvertisement::unpack(&plaintext)
                else {
                    continue;
                };
                if adv.total_segments > 1 {
                    if adv.total_segments > MAX_SEGMENTS
                        || adv.segment_index == 0
                        || adv.segment_index > adv.total_segments
                    {
                        break Err(RncpError::ResourceFailed(
                            "invalid split-resource advertisement".into(),
                        ));
                    }
                    let coord = split_resources.entry(adv.original_hash).or_insert_with(|| {
                        MultiSegmentInbound::new(adv.total_segments, adv.original_hash)
                    });
                    if coord.total_segments != adv.total_segments {
                        break Err(RncpError::ResourceFailed(
                            "split-resource total_segments changed".into(),
                        ));
                    }
                    segment_routes
                        .insert(adv.resource_hash, (adv.original_hash, adv.segment_index));
                }
                let map_hashes = adv.get_map_hashes();
                let mut transfer_flags = adv.flags;
                if adv.total_segments > 1 && adv.segment_index > 1 {
                    transfer_flags.has_metadata = false;
                }
                let mut rh = [0u8; rns_protocol::resource::RANDOM_HASH_SIZE];
                let copy_len = adv.random_hash.len().min(rh.len());
                rh[..copy_len].copy_from_slice(&adv.random_hash[..copy_len]);
                let Ok(mut t) = InboundTransfer::from_advertisement(
                    adv.num_parts,
                    adv.transfer_size,
                    adv.data_size,
                    rh,
                    adv.resource_hash,
                    transfer_flags,
                    map_hashes,
                    link_rtt,
                ) else {
                    continue;
                };
                if let TransferAction::SendRequest(req_data) = t.request_next() {
                    if let Ok(encrypted) = link.encrypt(&req_data) {
                        let req_raw = build_data_packet(
                            link_id,
                            rns_wire::context::PacketContext::ResourceReq,
                            &encrypted,
                        );
                        transport_tx
                            .send(TransportMessage::Outbound(OutboundRequest {
                                raw: req_raw,
                                destination_hash: link_id,
                            }))
                            .await
                            .map_err(|_| RncpError::TransportUnavailable)?;
                    }
                }
                transfers.insert(adv.resource_hash, t);
                stats.update(
                    adv.resource_hash,
                    adv.transfer_size,
                    0.0,
                    (adv.segment_index.saturating_sub(1)) as f32 / adv.total_segments.max(1) as f32,
                );
            }
            rns_wire::context::PacketContext::Resource => {
                let mut resource_action_to_send = None;
                let mut completed_rh = None;
                for (rh, t) in &mut transfers {
                    let action = t.receive_part(body.to_vec());
                    let fraction = t.resource.progress();
                    let progress = segment_routes
                        .get(rh)
                        .and_then(|(original, index)| {
                            split_resources.get(original).map(|coord| {
                                ((*index - 1) as f64 + fraction) / coord.total_segments as f64
                            })
                        })
                        .unwrap_or(fraction) as f32;
                    stats.update(*rh, t.resource.total_size, fraction, progress);
                    if let Some(ref tx) = progress_tx {
                        let _ = tx.try_send(progress);
                    }
                    match action {
                        TransferAction::SendHmu(_) | TransferAction::SendRequest(_) => {
                            resource_action_to_send = Some(action);
                        }
                        TransferAction::Complete => {
                            completed_rh = Some(*rh);
                        }
                        _ => {}
                    }
                    if completed_rh.is_none() && t.resource.is_complete() {
                        completed_rh = Some(*rh);
                    }
                    if resource_action_to_send.is_some() || completed_rh.is_some() {
                        break;
                    }
                }

                if let Some(action) = resource_action_to_send {
                    let (context, payload) = match action {
                        TransferAction::SendHmu(hmu) => {
                            (rns_wire::context::PacketContext::ResourceHmu, hmu)
                        }
                        TransferAction::SendRequest(req) => {
                            (rns_wire::context::PacketContext::ResourceReq, req)
                        }
                        _ => unreachable!(),
                    };
                    if let Ok(encrypted) = link.encrypt(&payload) {
                        let raw = build_data_packet(link_id, context, &encrypted);
                        transport_tx
                            .send(TransportMessage::Outbound(OutboundRequest {
                                raw,
                                destination_hash: link_id,
                            }))
                            .await
                            .map_err(|_| RncpError::TransportUnavailable)?;
                    }
                }

                if let Some(rh) = completed_rh {
                    let (payload, proof, metadata, resource_hash) = match transfers.get_mut(&rh) {
                        Some(t) => match t.complete(Some(&decrypt_fn)) {
                            Ok((payload, proof)) => (
                                payload,
                                proof,
                                t.resource.metadata.clone(),
                                t.resource.resource_hash,
                            ),
                            Err(e) => {
                                break Err(RncpError::ResourceFailed(format!(
                                    "resource assemble/proof failed: {e:?}; parts={}/{} map_hashes={} total_size={} data_size={} flags={:?}",
                                    t.resource.received_count(),
                                    t.resource.total_parts,
                                    t.resource.map_hashes.len(),
                                    t.resource.total_size,
                                    t.resource.data_size,
                                    t.resource.flags,
                                )));
                            }
                        },
                        None => {
                            break Err(RncpError::ResourceFailed(
                                "resource assemble/proof failed: transfer missing".into(),
                            ));
                        }
                    };
                    let prf_raw = build_resource_proof_packet(link_id, &proof);
                    let _ = transport_tx
                        .send(TransportMessage::Outbound(OutboundRequest {
                            raw: prf_raw,
                            destination_hash: link_id,
                        }))
                        .await;

                    transfers.remove(&rh);

                    if let Some((original_hash, segment_index)) = segment_routes.remove(&rh) {
                        let Some(coord) = split_resources.get_mut(&original_hash) else {
                            break Err(RncpError::ResourceFailed(
                                "split-resource coordinator missing".into(),
                            ));
                        };
                        if let Some(meta) = metadata {
                            coord.set_metadata(meta);
                        }
                        if let Err(e) = coord.set_segment_data(segment_index, payload) {
                            break Err(RncpError::ResourceFailed(format!(
                                "split segment rejected: {e:?}"
                            )));
                        }
                        if !coord.is_complete() {
                            if let Some(ref tx) = progress_tx {
                                let _ = tx.try_send(coord.progress() as f32);
                            }
                            continue;
                        }

                        let payload = match coord.reassemble() {
                            Ok(p) => p,
                            Err(e) => {
                                break Err(RncpError::ResourceFailed(format!(
                                    "split reassemble: {e:?}"
                                )));
                            }
                        };
                        let meta_bytes = coord.metadata.clone();
                        let file_name = extract_filename(meta_bytes.as_deref())
                            .unwrap_or_else(|| default_file_name(&original_hash));
                        split_resources.remove(&original_hash);
                        if let Some(ref tx) = progress_tx {
                            let _ = tx.try_send(1.0);
                        }
                        break Ok((file_name, payload));
                    }

                    let file_name = extract_filename(metadata.as_deref())
                        .unwrap_or_else(|| default_file_name(&resource_hash));
                    if let Some(ref tx) = progress_tx {
                        let _ = tx.try_send(1.0);
                    }
                    break Ok((file_name, payload));
                }
            }
            rns_wire::context::PacketContext::ResourceHmu => {
                let Ok(plaintext) = link.decrypt(body) else {
                    continue;
                };
                let Ok((rh, segment, hashmap)) =
                    rns_protocol::resource::parse_hashmap_update(&plaintext)
                else {
                    continue;
                };
                let Some(t) = transfers.get_mut(&rh) else {
                    continue;
                };
                if let TransferAction::SendRequest(req) = t.hashmap_update(segment, &hashmap) {
                    if let Ok(encrypted) = link.encrypt(&req) {
                        let req_raw = build_data_packet(
                            link_id,
                            rns_wire::context::PacketContext::ResourceReq,
                            &encrypted,
                        );
                        transport_tx
                            .send(TransportMessage::Outbound(OutboundRequest {
                                raw: req_raw,
                                destination_hash: link_id,
                            }))
                            .await
                            .map_err(|_| RncpError::TransportUnavailable)?;
                    }
                }
            }
            rns_wire::context::PacketContext::LinkClose if link.receive_teardown(body) => {
                break Err(RncpError::ResourceFailed("link closed".into()));
            }
            _ => {}
        }
    };

    if let Some(close_pkt) = build_link_close(&mut link) {
        let _ = transport_tx
            .send(TransportMessage::Outbound(OutboundRequest {
                raw: close_pkt,
                destination_hash: link_id,
            }))
            .await;
    }
    cleanup_destination(&transport_tx, link_id).await;

    let (file_name, payload) = result?;
    let bytes = payload.len();
    let saved_path = save_payload(save_dir, &file_name, &payload, overwrite)
        .await
        .map_err(RncpError::Io)?;

    tracing::info!(
        dest = hex::encode(dest_hash),
        link_id = hex::encode(link_id),
        bytes = bytes,
        file_name = %file_name,
        "rncp fetch complete"
    );

    Ok(RncpFetchOutcome {
        file_name,
        saved_path,
        bytes,
        link_rtt,
        duration: started.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reader_size_is_validated_before_connecting() {
        let (tx, mut rx) = mpsc::channel(1);
        let result = rncp_send_reader(RncpSendReaderRequest {
            transport_tx: tx,
            identity: Identity::new(),
            dest_hash: [7; 16],
            file_name: "too-large.bin",
            reader: tokio::io::empty(),
            data_size: rns_protocol::resource::MAX_RESOURCE_SIZE + 1,
            auto_compress: false,
            overall_timeout: Duration::from_secs(1),
            path_wait: Duration::from_secs(1),
            progress_tx: None,
        })
        .await;
        assert!(matches!(result, Err(RncpError::ResourceCreate(_))));
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn resource_sender_waits_for_requests() {
        use rns_wire::context::PacketContext;
        let key = rns_crypto::ed25519::Ed25519PrivateKey::generate();
        let public = key.public_key();
        let (mut link, request) = Link::new_initiator([7; 16], 1);
        let (peer, proof) = Link::new_responder(&request, &key, [7; 16], 1).unwrap();
        link.validate_proof(&proof, &public, &public.to_bytes())
            .unwrap();
        let link_id = link.link_id;
        let resource = OutboundResource::new(vec![42; 2000], false, None).unwrap();
        let encoded_size = resource.total_size;
        let (stats_tx, stats_rx) = tokio::sync::watch::channel(RncpTransferProgress::default());
        let parts = resource.parts.clone();
        let mut request = vec![rns_protocol::resource::HASHMAP_IS_NOT_EXHAUSTED];
        request.extend_from_slice(&resource.resource_hash);
        for hash in &resource.map_hashes {
            request.extend_from_slice(hash);
        }
        let mut proof = resource.resource_hash.to_vec();
        proof.extend_from_slice(&resource.expected_proof);
        let (tx, mut output) = mpsc::channel(32);
        let (events, mut rx) = mpsc::channel(8);
        let task = async move {
            drive_outbound(OutboundDrive {
                stats: &mut TransferReporter::new(Some(stats_tx)),
                transport_tx: &tx,
                link: &mut link,
                link_id,
                resource,
                lpkt_rx: &mut rx,
                progress_tx: None,
                progress_base: 0.0,
                progress_span: 1.0,
                deadline: Instant::now() + Duration::from_secs(1),
            })
            .await
        };
        let peer_task = async {
            let TransportMessage::Outbound(adv) = output.recv().await.unwrap() else {
                panic!("ADV")
            };
            assert_eq!(
                rns_wire::header::PacketHeader::unpack(&adv.raw)
                    .unwrap()
                    .0
                    .context,
                PacketContext::ResourceAdv
            );
            tokio::task::yield_now().await;
            assert!(output.try_recv().is_err(), "no unsolicited parts");
            events
                .send(DestinationEvent::InboundPacket {
                    raw: build_data_packet(
                        link_id,
                        PacketContext::ResourceReq,
                        &peer.encrypt(&request).unwrap(),
                    ),
                    interface_id: 0,
                })
                .await
                .unwrap();
            for part in parts {
                let TransportMessage::Outbound(packet) = output.recv().await.unwrap() else {
                    panic!("part")
                };
                let (header, offset) = rns_wire::header::PacketHeader::unpack(&packet.raw).unwrap();
                assert_eq!(header.context, PacketContext::Resource);
                assert_eq!(&packet.raw[offset..], part.as_slice());
            }
            let raw = build_data_packet(link_id, PacketContext::ResourcePrf, &proof);
            let (mut header, _) = rns_wire::header::PacketHeader::unpack(&raw).unwrap();
            header.flags.packet_type = rns_wire::flags::PacketType::Proof;
            let mut raw = header.pack().unwrap();
            raw.extend_from_slice(&proof);
            events
                .send(DestinationEvent::InboundPacket {
                    raw: Bytes::from(raw),
                    interface_id: 0,
                })
                .await
                .unwrap();
        };
        let (result, ()) = tokio::join!(task, peer_task);
        result.unwrap();
        assert_eq!(stats_rx.borrow().encoded_bytes, encoded_size as f64);
        assert_eq!(stats_rx.borrow().progress, 1.0);
    }

    #[test]
    fn physical_progress_uses_encoded_size_and_accumulates_segments_once() {
        let resource = OutboundResource::new(vec![42; 8000], true, None).unwrap();
        assert!(resource.total_size < resource.data.len());
        let size = resource.total_size;
        let (tx, rx) = tokio::sync::watch::channel(RncpTransferProgress::default());
        let mut reporter = TransferReporter::new(Some(tx));
        reporter.update([1; 32], size, 0.5, 0.25);
        assert_eq!(rx.borrow().encoded_bytes, size as f64 / 2.0);
        reporter.update([1; 32], size, 0.5, 0.25); // duplicate part/progress
        reporter.update([1; 32], size, 1.0, 0.5);
        reporter.update([2; 32], 200, 0.5, 0.75);
        assert_eq!(rx.borrow().encoded_bytes, size as f64 + 100.0);
        reporter.update([1; 32], size, 0.0, 0.75); // repeated ADV cannot reset bytes
        assert_eq!(rx.borrow().encoded_bytes, size as f64 + 100.0);
    }

    #[test]
    fn metadata_roundtrip() {
        let packed = pack_metadata("hello.txt");
        let parsed = extract_filename(Some(&packed)).unwrap();
        assert_eq!(parsed, "hello.txt");
    }

    #[test]
    fn default_file_name_uses_resource_hash_prefix() {
        let rh = [0xAB; 32];
        let name = default_file_name(&rh);
        assert!(name.starts_with("rncp_"));
        assert_eq!(name.len(), "rncp_".len() + 16);
    }

    #[test]
    fn default_app_name_matches_python() {
        assert_eq!(default_rncp_app_name(), "rncp.receive");
    }

    #[test]
    fn resource_proof_packet_uses_proof_type() {
        let raw = build_resource_proof_packet([0x11; 16], &[0x22; 64]);
        let (header, off) = rns_wire::header::PacketHeader::unpack(&raw).unwrap();
        assert_eq!(header.flags.packet_type, rns_wire::flags::PacketType::Proof);
        assert_eq!(
            header.context,
            rns_wire::context::PacketContext::ResourcePrf
        );
        assert_eq!(&raw[off..], &[0x22; 64]);
    }

    #[test]
    fn link_close_packet_uses_authenticated_teardown_payload() {
        let dest_hash = [0x33; 16];
        let responder_key = rns_crypto::ed25519::Ed25519PrivateKey::generate();
        let responder_pub = responder_key.public_key();
        let (mut initiator, request_data) = Link::new_initiator(dest_hash, 1);
        let (mut responder, proof_data) =
            Link::new_responder(&request_data, &responder_key, dest_hash, 1).unwrap();
        let rtt_data = initiator
            .validate_proof(&proof_data, &responder_pub, &responder_pub.to_bytes())
            .unwrap();
        responder.receive_rtt_packet(&rtt_data).unwrap();

        let raw = build_link_close(&mut initiator).expect("active link emits teardown");
        let (header, offset) = rns_wire::header::PacketHeader::unpack(&raw).unwrap();
        assert_eq!(header.context, rns_wire::context::PacketContext::LinkClose);
        assert!(responder.receive_teardown(&raw[offset..]));
    }
}
