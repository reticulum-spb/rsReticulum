//! Initiator-side Link client for `rnstatus -R` / `rnpath -R`. Each `query`
//! does the full handshake (pubkey discovery → link → identify → request →
//! response → close) over its own destination channel.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, timeout};

use rns_crypto::ed25519::Ed25519PublicKey;
use rns_identity::destination::Destination;
use rns_identity::identity::Identity;
use rns_link::link::{CloseReason, Link, LinkAction};
pub use rns_link::{
    constants::{ESTABLISHMENT_TIMEOUT_PER_HOP, KEEPALIVE_DEFAULT},
    link::LinkState,
};
use rns_protocol::channel::{ChannelError, LinkChannel};
use rns_protocol::channel_message::MessageBase;
use rns_protocol::resource::{
    InboundTransfer, MAX_EFFICIENT_SIZE, MultiSegmentInbound, MultiSegmentOutbound,
    OutboundResource, OutboundTransfer, TransferAction,
};
use rns_protocol::resource_adv::ResourceAdvertisement;
use rns_transport::link_messages::DestinationEvent;
use rns_transport::messages::{AnnounceHandlerEvent, OutboundRequest, TransportMessage};
use rns_transport::messages::{TransportQuery, TransportQueryResponse};

use crate::reticulum::ReticulumHandle;

#[derive(Debug, thiserror::Error)]
pub enum LinkClientError {
    #[error("transport channel closed or full")]
    TransportUnavailable,
    #[error("timed out waiting for {0}")]
    Timeout(&'static str),
    #[error("could not discover remote identity public key for destination")]
    PubkeyNotDiscovered,
    #[error("link proof validation failed: {0}")]
    ProofInvalid(String),
    #[error("link establishment failed: {0}")]
    HandshakeFailed(String),
    #[error("local identity has no signing key (cannot identify on link)")]
    NoSigningKey,
    #[error("encryption failure on link: {0}")]
    LinkCrypto(String),
    #[error("unexpected response from remote: {0}")]
    UnexpectedResponse(String),
    #[error("channel: {0}")]
    Channel(#[from] ChannelError),
    #[error("resource: {0}")]
    Resource(String),
}

#[derive(Clone)]
pub struct LinkClient {
    transport_tx: mpsc::Sender<TransportMessage>,
    identity: Arc<Identity>,
}

/// A reusable, established outbound Reticulum Link.
///
/// Unlike [`LinkClient`], this session stays open after one request and exposes
/// packet, identification and request operations to applications.
pub struct LinkSession {
    transport_tx: mpsc::Sender<TransportMessage>,
    identity: Arc<Identity>,
    link: Link,
    event_rx: mpsc::Receiver<DestinationEvent>,
    channel: Option<LinkChannel>,
    channel_packets: Vec<[u8; 32]>,
    pending_packets: VecDeque<Vec<u8>>,
    pending_resource_packets: VecDeque<Bytes>,
}

/// An outbound Link whose identifier and handshake packet have been prepared,
/// but not yet sent to the transport.
///
/// Preparation is synchronous so application state machines can reserve and
/// report the final `link_id` before spawning the asynchronous handshake.
pub struct PreparedLinkSession {
    transport_tx: mpsc::Sender<TransportMessage>,
    identity: Identity,
    destination_hash: [u8; 16],
    public_key: [u8; 64],
    link: Link,
    request_data: Vec<u8>,
}

/// Release temporary link routing state when establishment fails or is cancelled.
struct PendingLinkRegistration {
    tx: mpsc::Sender<TransportMessage>,
    hash: [u8; 16],
    armed: bool,
}

impl Drop for PendingLinkRegistration {
    fn drop(&mut self) {
        if self.armed {
            let message = TransportMessage::DeregisterDestination { hash: self.hash };
            if let Err(mpsc::error::TrySendError::Full(message)) = self.tx.try_send(message) {
                if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                    let tx = self.tx.clone();
                    runtime.spawn(async move {
                        let _ = tx.send(message).await;
                    });
                }
            }
        }
    }
}

/// Command handle for a runtime task that exclusively owns a reusable
/// outbound [`LinkSession`].
#[derive(Clone)]
pub struct LinkSessionHandle {
    link_id: [u8; 16],
    command_tx: mpsc::Sender<LinkSessionCommand>,
    inbound_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Result<Vec<u8>, LinkClientError>>>>,
}

enum LinkSessionCommand {
    Identify {
        result_tx: oneshot::Sender<Result<(), LinkClientError>>,
    },
    SendPacket {
        data: Vec<u8>,
        result_tx: oneshot::Sender<Result<(), LinkClientError>>,
    },
    SendPayload {
        data: Vec<u8>,
        auto_compress: bool,
        deadline: Duration,
        result_tx: oneshot::Sender<Result<LinkPayloadSendReceipt, LinkClientError>>,
    },
    ReceiveResource {
        deadline: Duration,
        result_tx: oneshot::Sender<Result<ReceivedResource, LinkClientError>>,
    },
    ReceiveResourceFile {
        max_size: usize,
        expires: Instant,
        result_tx: oneshot::Sender<Result<ReceivedFileResource, LinkClientError>>,
    },
    SendResource {
        data: Vec<u8>,
        auto_compress: bool,
        deadline: Duration,
        result_tx: oneshot::Sender<Result<[u8; 32], LinkClientError>>,
    },
    Close {
        result_tx: oneshot::Sender<Result<(), LinkClientError>>,
    },
}

#[derive(Debug, Clone)]
pub struct ReceivedResource {
    pub data: Vec<u8>,
    pub metadata: Option<Vec<u8>>,
    pub resource_hash: [u8; 32],
}

/// A verified Resource backed by an anonymous temporary file, positioned at zero.
/// Closing the last file handle removes its temporary storage.
#[derive(Debug)]
pub struct ReceivedFileResource {
    pub file: tokio::fs::File,
    pub data_size: usize,
    pub metadata: Option<Vec<u8>>,
    pub resource_hash: [u8; 32],
}

/// Wire interpretation of metadata-bearing Resource responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResourceResponseMode {
    /// Existing Rust contract: decode `[request_id, data]`, even with metadata.
    #[default]
    Packed,
    /// Python contract: Resources with metadata contain raw file bytes.
    /// Resources without metadata and packet responses still use the envelope.
    PythonFile,
}

/// Response payload and optional Resource metadata returned by a Link request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkResponse {
    pub data: Vec<u8>,
    pub metadata: Option<Vec<u8>>,
}

/// Proof-backed result of sending an application payload over a Link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkPayloadSendReceipt {
    Packet {
        link_id: [u8; 16],
        packet_hash: [u8; 32],
    },
    Resource {
        link_id: [u8; 16],
        resource_hash: [u8; 32],
    },
}

impl PreparedLinkSession {
    pub fn id(&self) -> [u8; 16] {
        self.link.link_id
    }

    /// Register the prepared Link, send its request and validate LRPROOF.
    pub async fn establish(mut self, deadline: Duration) -> Result<LinkSession, LinkClientError> {
        let link_id = self.id();
        let (event_tx, mut event_rx) = mpsc::channel(256);
        send_transport(
            &self.transport_tx,
            TransportMessage::RegisterDestination {
                hash: link_id,
                app_name: "application.link".into(),
                delivery_tx: Some(event_tx),
            },
        )
        .await?;
        let mut registration = PendingLinkRegistration {
            tx: self.transport_tx.clone(),
            hash: link_id,
            armed: true,
        };
        send_transport(
            &self.transport_tx,
            TransportMessage::Outbound(OutboundRequest {
                raw: build_link_request_packet(self.destination_hash, &self.request_data),
                destination_hash: self.destination_hash,
            }),
        )
        .await?;

        let rtt_data = wait_for_proof(
            &self.transport_tx,
            &mut event_rx,
            &mut self.link,
            &self.public_key,
            deadline,
        )
        .await?;
        send_transport(
            &self.transport_tx,
            TransportMessage::Outbound(OutboundRequest {
                raw: build_data_packet(link_id, rns_wire::context::PacketContext::Lrrtt, &rtt_data),
                destination_hash: link_id,
            }),
        )
        .await?;
        registration.armed = false;
        Ok(LinkSession {
            transport_tx: self.transport_tx,
            identity: Arc::new(self.identity),
            link: self.link,
            event_rx,
            channel: None,
            channel_packets: Vec::new(),
            pending_packets: VecDeque::new(),
            pending_resource_packets: VecDeque::new(),
        })
    }

    /// Spawn a task that establishes and then exclusively owns this session.
    ///
    /// The returned handle is available immediately and retains the prepared
    /// `link_id`; commands wait behind establishment in the task queue.
    pub fn spawn(self, deadline: Duration) -> LinkSessionHandle {
        let link_id = self.id();
        let (command_tx, command_rx) = mpsc::channel(64);
        let (inbound_tx, inbound_rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let Ok(session) = self.establish(deadline).await else {
                return;
            };
            run_established_link_session(session, command_rx, inbound_tx).await;
        });
        LinkSessionHandle {
            link_id,
            command_tx,
            inbound_rx: Arc::new(tokio::sync::Mutex::new(inbound_rx)),
        }
    }
}

impl LinkSessionHandle {
    pub fn id(&self) -> [u8; 16] {
        self.link_id
    }

    pub async fn identify(&self) -> Result<(), LinkClientError> {
        let (result_tx, result_rx) = oneshot::channel();
        self.send_command(LinkSessionCommand::Identify { result_tx })
            .await?;
        recv_command_result(result_rx).await
    }

    /// Send one Link packet without waiting for its delivery proof.
    ///
    /// This matches Reticulum applications whose protocol provides its own
    /// acknowledgement, such as RRC's `JOINED` and `PARTED` envelopes.
    pub async fn send_packet(&self, data: Vec<u8>) -> Result<(), LinkClientError> {
        let (result_tx, result_rx) = oneshot::channel();
        self.send_command(LinkSessionCommand::SendPacket { data, result_tx })
            .await?;
        recv_command_result(result_rx).await
    }

    pub async fn send_payload(
        &self,
        data: Vec<u8>,
        auto_compress: bool,
        deadline: Duration,
    ) -> Result<LinkPayloadSendReceipt, LinkClientError> {
        let (result_tx, result_rx) = oneshot::channel();
        self.send_command(LinkSessionCommand::SendPayload {
            data,
            auto_compress,
            deadline,
            result_tx,
        })
        .await?;
        recv_command_result(result_rx).await
    }

    pub async fn recv(&self) -> Result<Vec<u8>, LinkClientError> {
        self.inbound_rx
            .lock()
            .await
            .recv()
            .await
            .ok_or_else(|| LinkClientError::HandshakeFailed("Link session task stopped".into()))?
    }

    pub async fn recv_resource(
        &self,
        deadline: Duration,
    ) -> Result<ReceivedResource, LinkClientError> {
        let (result_tx, result_rx) = oneshot::channel();
        self.send_command(LinkSessionCommand::ReceiveResource {
            deadline,
            result_tx,
        })
        .await?;
        recv_command_result(result_rx).await
    }

    pub async fn send_resource(
        &self,
        data: Vec<u8>,
        auto_compress: bool,
        deadline: Duration,
    ) -> Result<[u8; 32], LinkClientError> {
        let (result_tx, result_rx) = oneshot::channel();
        self.send_command(LinkSessionCommand::SendResource {
            data,
            auto_compress,
            deadline,
            result_tx,
        })
        .await?;
        recv_command_result(result_rx).await
    }

    /// Receive into an anonymous temporary file through the session worker.
    /// `max_size` includes wire metadata. The deadline also covers waiting for
    /// establishment/earlier commands and queue capacity. A cancelled queued
    /// call is skipped; an already-started receive finishes or reaches its
    /// original deadline, then drops the file if its caller is gone.
    pub async fn recv_resource_file(
        &self,
        max_size: usize,
        deadline: Duration,
    ) -> Result<ReceivedFileResource, LinkClientError> {
        let expires = Instant::now() + deadline;
        let operation = async {
            let (result_tx, result_rx) = oneshot::channel();
            self.send_command(LinkSessionCommand::ReceiveResourceFile {
                max_size,
                expires,
                result_tx,
            })
            .await?;
            recv_command_result(result_rx).await
        };
        timeout(deadline, operation)
            .await
            .map_err(|_| LinkClientError::Timeout("resource file command"))?
    }

    pub async fn close(&self) -> Result<(), LinkClientError> {
        let (result_tx, result_rx) = oneshot::channel();
        self.send_command(LinkSessionCommand::Close { result_tx })
            .await?;
        recv_command_result(result_rx).await
    }

    async fn send_command(&self, command: LinkSessionCommand) -> Result<(), LinkClientError> {
        self.command_tx
            .send(command)
            .await
            .map_err(|_| LinkClientError::HandshakeFailed("Link session task stopped".into()))
    }
}

async fn recv_command_result<T>(
    receiver: oneshot::Receiver<Result<T, LinkClientError>>,
) -> Result<T, LinkClientError> {
    receiver
        .await
        .map_err(|_| LinkClientError::HandshakeFailed("Link session task stopped".into()))?
}

async fn run_established_link_session(
    mut session: LinkSession,
    mut command_rx: mpsc::Receiver<LinkSessionCommand>,
    inbound_tx: mpsc::Sender<Result<Vec<u8>, LinkClientError>>,
) {
    loop {
        tokio::select! {
            command = command_rx.recv() => {
                let Some(command) = command else {
                    break;
                };
                match command {
                    LinkSessionCommand::Identify { result_tx } => {
                        let _ = result_tx.send(session.identify().await);
                    }
                    LinkSessionCommand::SendPacket { data, result_tx } => {
                        let _ = result_tx.send(session.send(&data).await);
                    }
                    LinkSessionCommand::SendPayload {
                        data,
                        auto_compress,
                        deadline,
                        result_tx,
                    } => {
                        let _ = result_tx
                            .send(session.send_payload(data, auto_compress, deadline).await);
                    }
                    LinkSessionCommand::ReceiveResource {
                        deadline,
                        result_tx,
                    } => {
                        let _ = result_tx.send(session.recv_resource(deadline).await);
                    }
                    LinkSessionCommand::ReceiveResourceFile {
                        max_size, expires, result_tx,
                    } => {
                        if result_tx.is_closed() {
                            continue;
                        }
                        let result = match time_remaining(expires) {
                            Ok(remaining) => session.recv_resource_file(max_size, remaining).await,
                            Err(error) => Err(error),
                        };
                        let _ = result_tx.send(result);
                    }
                    LinkSessionCommand::SendResource {
                        data,
                        auto_compress,
                        deadline,
                        result_tx,
                    } => {
                        let _ = result_tx
                            .send(session.send_resource(data, auto_compress, deadline).await);
                    }
                    LinkSessionCommand::Close { result_tx } => {
                        let result = session.close().await;
                        let _ = result_tx.send(result);
                        break;
                    }
                }
            }
            packet = session.recv() => {
                let closed = packet.is_err();
                if inbound_tx.send(packet).await.is_err() || closed {
                    break;
                }
            }
        }
    }
}

impl LinkSession {
    /// Prepare an outbound Link without performing any transport I/O.
    pub fn prepare_with_public_key(
        runtime: &ReticulumHandle,
        identity: Identity,
        destination_hash: [u8; 16],
        public_key: [u8; 64],
        hops: u8,
    ) -> PreparedLinkSession {
        Self::prepare_on_transport(
            runtime.transport_tx.clone(),
            identity,
            destination_hash,
            public_key,
            hops,
        )
    }

    fn prepare_on_transport(
        transport_tx: mpsc::Sender<TransportMessage>,
        identity: Identity,
        destination_hash: [u8; 16],
        public_key: [u8; 64],
        hops: u8,
    ) -> PreparedLinkSession {
        Self::prepare_on_transport_with_mtu(
            transport_tx,
            identity,
            destination_hash,
            public_key,
            hops,
            rns_wire::constants::MTU as u32,
        )
    }

    fn prepare_on_transport_with_mtu(
        transport_tx: mpsc::Sender<TransportMessage>,
        identity: Identity,
        destination_hash: [u8; 16],
        public_key: [u8; 64],
        hops: u8,
        mtu: u32,
    ) -> PreparedLinkSession {
        let (link, request_data) = Link::new_initiator_with_mtu(destination_hash, hops, mtu);
        PreparedLinkSession {
            transport_tx,
            identity,
            destination_hash,
            public_key,
            link,
            request_data,
        }
    }

    /// Establish and validate a Link to a destination already learned through
    /// an announce. The local identity is not sent until [`identify`](Self::identify)
    /// is called.
    pub async fn open(
        runtime: &ReticulumHandle,
        identity: Identity,
        destination_hash: [u8; 16],
        hops: u8,
        deadline: Duration,
    ) -> Result<Self, LinkClientError> {
        let public_key = match runtime
            .query_control(TransportQuery::Recall { destination_hash })
            .await
        {
            Some(TransportQueryResponse::Announce(Some(entry))) => entry.public_key,
            _ => None,
        }
        .ok_or(LinkClientError::PubkeyNotDiscovered)?;

        Self::open_with_public_key(
            runtime,
            identity,
            destination_hash,
            public_key,
            hops,
            deadline,
        )
        .await
    }

    /// Establish a Link when the destination public key is already known
    /// from a previously handled announce.
    pub async fn open_with_public_key(
        runtime: &ReticulumHandle,
        identity: Identity,
        destination_hash: [u8; 16],
        public_key: [u8; 64],
        hops: u8,
        deadline: Duration,
    ) -> Result<Self, LinkClientError> {
        let started = Instant::now();
        let mtu = discover_link_mtu(&runtime.transport_tx, destination_hash, deadline).await;
        let remaining = deadline
            .checked_sub(started.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(LinkClientError::Timeout("MTU discovery"))?;
        Self::prepare_on_transport_with_mtu(
            runtime.transport_tx.clone(),
            identity,
            destination_hash,
            public_key,
            hops,
            mtu,
        )
        .establish(remaining)
        .await
    }

    pub fn id(&self) -> [u8; 16] {
        self.link.link_id
    }

    pub fn rtt(&self) -> Duration {
        self.link.rtt.unwrap_or_default()
    }

    pub fn mdu(&self) -> usize {
        self.link.mdu
    }

    pub fn remote_identity(&self) -> Option<&[u8; 64]> {
        self.link.remote_identity()
    }

    /// Identify the local identity to the remote Link peer.
    pub async fn identify(&mut self) -> Result<(), LinkClientError> {
        let public_key = self.identity.get_public_key();
        let signing_key = self
            .identity
            .get_signing_key()
            .ok_or(LinkClientError::NoSigningKey)?;
        self.identify_with(&public_key, &signing_key).await
    }

    /// Identify with application-owned public/signing keys.
    ///
    /// This supports routers that keep the Reticulum identity outside the
    /// Link session while still reusing the common session implementation.
    pub async fn identify_with(
        &mut self,
        public_key: &[u8; 64],
        signing_key: &rns_crypto::ed25519::Ed25519PrivateKey,
    ) -> Result<(), LinkClientError> {
        let payload = self
            .link
            .identify(public_key, signing_key)
            .map_err(|error| LinkClientError::LinkCrypto(format!("identify: {error:?}")))?;
        self.send_context(rns_wire::context::PacketContext::LinkIdentify, payload)
            .await
    }

    /// Send one encrypted Link packet.
    pub async fn send(&mut self, data: &[u8]) -> Result<(), LinkClientError> {
        self.send_tracked(data).await.map(|_| ())
    }

    /// Send one encrypted Link packet and return the hash addressed by its
    /// delivery proof.
    pub async fn send_tracked(&mut self, data: &[u8]) -> Result<[u8; 32], LinkClientError> {
        let encrypted = self
            .link
            .encrypt(data)
            .map_err(|error| LinkClientError::LinkCrypto(format!("packet: {error:?}")))?;
        let raw = build_data_packet(
            self.id(),
            rns_wire::context::PacketContext::None,
            &encrypted,
        );
        let packet_hash = rns_wire::hash::packet_hash(&raw, rns_wire::flags::HeaderType::Header1);
        send_transport(
            &self.transport_tx,
            TransportMessage::Outbound(OutboundRequest {
                raw,
                destination_hash: self.id(),
            }),
        )
        .await?;
        Ok(packet_hash)
    }

    /// Send an application payload using the appropriate Link representation
    /// and wait for its delivery proof.
    ///
    /// Payloads fitting the Link MDU use a single encrypted Link packet.
    /// Larger payloads use the Resource protocol, including automatic
    /// multi-segment splitting. The session remains open and can be reused
    /// after this operation completes.
    pub async fn send_payload(
        &mut self,
        data: Vec<u8>,
        auto_compress: bool,
        deadline: Duration,
    ) -> Result<LinkPayloadSendReceipt, LinkClientError> {
        let link_id = self.id();
        if data.len() <= self.mdu() {
            let expected_hash = self.send_tracked(&data).await?;
            let expires = Instant::now() + deadline;
            loop {
                let packet_hash = self.recv_delivery_proof(time_remaining(expires)?).await?;
                if packet_hash == expected_hash {
                    return Ok(LinkPayloadSendReceipt::Packet {
                        link_id,
                        packet_hash,
                    });
                }
            }
        }

        let resource_hash = self.send_resource(data, auto_compress, deadline).await?;
        Ok(LinkPayloadSendReceipt::Resource {
            link_id,
            resource_hash,
        })
    }

    /// Wait for the next valid delivery proof for an application Link packet.
    pub async fn recv_delivery_proof(
        &mut self,
        deadline: Duration,
    ) -> Result<[u8; 32], LinkClientError> {
        let link_id = self.id();
        let expires = Instant::now() + deadline;
        loop {
            let event = timeout(time_remaining(expires)?, self.event_rx.recv())
                .await
                .map_err(|_| LinkClientError::Timeout("packet delivery proof"))?
                .ok_or_else(|| {
                    LinkClientError::HandshakeFailed("destination channel closed".into())
                })?;
            let raw = match event {
                DestinationEvent::InboundPacket { raw, .. } => raw,
                DestinationEvent::LinkClosed { link_id: closed_id } if closed_id == link_id => {
                    return Err(LinkClientError::HandshakeFailed("link closed".into()));
                }
                _ => continue,
            };
            let (header, offset) = match rns_wire::header::PacketHeader::unpack(&raw) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if header.destination_hash != link_id {
                continue;
            }
            if header.flags.packet_type == rns_wire::flags::PacketType::Data
                && header.context == rns_wire::context::PacketContext::LinkClose
                && self.link.receive_teardown(&raw[offset..])
            {
                return Err(LinkClientError::HandshakeFailed(
                    "link closed by remote".into(),
                ));
            }
            if header.flags.packet_type == rns_wire::flags::PacketType::Data
                && header.context == rns_wire::context::PacketContext::None
            {
                let packet = self
                    .link
                    .decrypt(&raw[offset..])
                    .map_err(|error| LinkClientError::LinkCrypto(format!("packet: {error:?}")))?;
                self.link.record_inbound();
                self.link.record_rx(raw.len() - offset);
                self.link.keepalive.record_data();
                self.prove_application_packet(&raw, header.flags.header_type)
                    .await?;
                self.pending_packets.push_back(packet);
                continue;
            }
            if header.flags.packet_type != rns_wire::flags::PacketType::Proof
                || !matches!(
                    header.context,
                    rns_wire::context::PacketContext::LinkProof
                        | rns_wire::context::PacketContext::None
                )
            {
                continue;
            }
            let proof = &raw[offset..];
            let Some(packet_hash) = proof.get(..32).and_then(|hash| hash.try_into().ok()) else {
                continue;
            };
            if self.link.validate_packet_proof(&packet_hash, proof) {
                self.link.record_inbound();
                self.link.record_rx(proof.len());
                return Ok(packet_hash);
            }
        }
    }

    /// Receive the next encrypted application packet on this Link.
    pub async fn recv(&mut self) -> Result<Vec<u8>, LinkClientError> {
        if let Some(packet) = self.pending_packets.pop_front() {
            return Ok(packet);
        }
        loop {
            let event = tokio::select! {
                event = self.event_rx.recv() => event,
                _ = sleep(Duration::from_secs_f64(
                    rns_link::constants::WATCHDOG_MAX_SLEEP,
                )) => {
                    self.drive_watchdog().await?;
                    continue;
                }
            };
            let Some(event) = event else {
                break;
            };
            match event {
                DestinationEvent::LinkClosed { link_id } if link_id == self.id() => {
                    return Err(LinkClientError::HandshakeFailed("link closed".into()));
                }
                DestinationEvent::InboundPacket { raw, .. } => {
                    let (header, offset) = match rns_wire::header::PacketHeader::unpack(&raw) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    if header.destination_hash != self.id() {
                        continue;
                    }
                    self.link.record_inbound();
                    self.link.record_rx(raw.len().saturating_sub(offset));
                    if header.context == rns_wire::context::PacketContext::LinkClose
                        && self.link.receive_teardown(&raw[offset..])
                    {
                        return Err(LinkClientError::HandshakeFailed(
                            "link closed by remote".into(),
                        ));
                    }
                    if header.context == rns_wire::context::PacketContext::Keepalive {
                        continue;
                    }
                    if matches!(
                        header.context,
                        rns_wire::context::PacketContext::ResourceAdv
                            | rns_wire::context::PacketContext::Resource
                            | rns_wire::context::PacketContext::ResourceHmu
                    ) {
                        self.pending_resource_packets.push_back(raw);
                        continue;
                    }
                    if header.flags.packet_type == rns_wire::flags::PacketType::Data
                        && header.context == rns_wire::context::PacketContext::None
                    {
                        let packet = self.link.decrypt(&raw[offset..]).map_err(|error| {
                            LinkClientError::LinkCrypto(format!("packet: {error:?}"))
                        })?;
                        self.link.keepalive.record_data();
                        self.prove_application_packet(&raw, header.flags.header_type)
                            .await?;
                        return Ok(packet);
                    }
                }
                _ => {}
            }
        }
        Err(LinkClientError::HandshakeFailed(
            "destination channel closed".into(),
        ))
    }

    async fn drive_watchdog(&mut self) -> Result<(), LinkClientError> {
        match self.link.tick() {
            LinkAction::None => Ok(()),
            LinkAction::SendKeepalive | LinkAction::TransitionedToStale => {
                let raw = build_data_packet(
                    self.id(),
                    rns_wire::context::PacketContext::Keepalive,
                    &[rns_link::constants::KEEPALIVE_REQUEST],
                );
                send_transport(
                    &self.transport_tx,
                    TransportMessage::Outbound(OutboundRequest {
                        raw,
                        destination_hash: self.id(),
                    }),
                )
                .await?;
                self.link.record_tx_keepalive(1);
                Ok(())
            }
            LinkAction::SendTeardownAndClose(data) => {
                if !data.is_empty() {
                    let raw = build_data_packet(
                        self.id(),
                        rns_wire::context::PacketContext::LinkClose,
                        &data,
                    );
                    send_transport(
                        &self.transport_tx,
                        TransportMessage::Outbound(OutboundRequest {
                            raw,
                            destination_hash: self.id(),
                        }),
                    )
                    .await?;
                }
                Err(LinkClientError::Timeout("link keepalive response"))
            }
            LinkAction::Closed(_) => Err(LinkClientError::Timeout("link watchdog")),
        }
    }

    /// Send a request and wait for its response, including Resource responses.
    pub async fn request(
        &mut self,
        path: &str,
        data: Option<&[u8]>,
        deadline: Duration,
    ) -> Result<Vec<u8>, LinkClientError> {
        self.request_with_metadata(path, data, deadline)
            .await
            .map(|response| response.data)
    }

    /// Send a request and retain metadata from a Resource-backed response.
    pub async fn request_with_metadata(
        &mut self,
        path: &str,
        data: Option<&[u8]>,
        deadline: Duration,
    ) -> Result<LinkResponse, LinkClientError> {
        self.request_with_metadata_limit(path, data, deadline, usize::MAX)
            .await
    }

    /// Limit packet response data bytes, or the full advertised uncompressed
    /// Resource size (including envelope and metadata). For split Resources,
    /// each advertisement repeats the total size; it is not summed per segment.
    pub async fn request_with_metadata_limit(
        &mut self,
        path: &str,
        data: Option<&[u8]>,
        deadline: Duration,
        max_response_bytes: usize,
    ) -> Result<LinkResponse, LinkClientError> {
        self.request_with_response_mode(
            path,
            data,
            deadline,
            max_response_bytes,
            ResourceResponseMode::Packed,
        )
        .await
    }

    /// Select the wire interpretation of Resource responses explicitly, without
    /// guessing from file contents. PythonFile returns raw file bytes in
    /// LinkResponse.data, not a file handle; max_response_bytes still bounds
    /// the full Resource size including metadata, before reception.
    pub async fn request_with_response_mode(
        &mut self,
        path: &str,
        data: Option<&[u8]>,
        deadline: Duration,
        max_response_bytes: usize,
        response_mode: ResourceResponseMode,
    ) -> Result<LinkResponse, LinkClientError> {
        let expires = Instant::now() + deadline;
        let (packed, request_id) = self
            .link
            .prepare_request(path, data, deadline)
            .map_err(|error| LinkClientError::LinkCrypto(format!("request: {error:?}")))?;
        let response_id = match self.link.classify_request(packed) {
            rns_link::link::RequestSendMode::Packet(packed) => {
                let encrypted = self
                    .link
                    .encrypt(&packed)
                    .map_err(|error| LinkClientError::LinkCrypto(format!("request: {error:?}")))?;
                let packet = build_data_packet(
                    self.id(),
                    rns_wire::context::PacketContext::Request,
                    &encrypted,
                );
                let id = rns_wire::hash::truncated_packet_hash(
                    &packet,
                    rns_wire::flags::HeaderType::Header1,
                );
                self.link.update_pending_request_id(&request_id, id);
                send_transport(
                    &self.transport_tx,
                    TransportMessage::Outbound(OutboundRequest {
                        raw: packet,
                        destination_hash: self.id(),
                    }),
                )
                .await?;
                id
            }
            rns_link::link::RequestSendMode::Resource(packed) => {
                self.send_resource_inner(
                    packed,
                    None,
                    true,
                    time_remaining(expires)?,
                    Some(request_id),
                )
                .await?;
                request_id
            }
        };
        let link_id = self.link.link_id;
        wait_for_response(
            &self.transport_tx,
            &mut self.event_rx,
            &mut self.link,
            link_id,
            response_id,
            time_remaining(expires)?,
            max_response_bytes,
            response_mode,
        )
        .await
    }

    pub fn channel_ready(&self) -> bool {
        self.channel
            .as_ref()
            .is_none_or(LinkChannel::is_ready_to_send)
    }

    pub fn channel_mdu(&self) -> usize {
        rns_protocol::channel::Channel::channel_mdu(self.link.mdu)
    }

    /// Create a Buffer sized for this link. Send returned frames through
    /// `send_channel`, respecting channel readiness and EOF drain ordering.
    pub fn channel_buffer(
        &self,
        stream_id: u16,
    ) -> Result<rns_protocol::buffer::ChannelBuffer, rns_protocol::stream_data::StreamIdError> {
        rns_protocol::buffer::ChannelBuffer::new(stream_id, self.channel_mdu().saturating_sub(2))
    }

    /// Send a typed message over the Link Channel.
    pub async fn send_channel(&mut self, message: &dyn MessageBase) -> Result<(), LinkClientError> {
        self.ensure_channel()?;
        let link_id = self.id();
        let channel = self.channel.as_mut().expect("channel initialized");
        let prepared = channel.prepare_send_tracked(message)?;
        let raw = build_data_packet(
            link_id,
            rns_wire::context::PacketContext::Channel,
            &prepared.data,
        );
        let packet_hash = rns_wire::hash::packet_hash(&raw, rns_wire::flags::HeaderType::Header1);
        channel.track_outbound_packet_hash(packet_hash, prepared.sequence);
        self.channel_packets.push(packet_hash);
        send_transport(
            &self.transport_tx,
            TransportMessage::Outbound(OutboundRequest {
                raw,
                destination_hash: link_id,
            }),
        )
        .await
    }

    /// Receive the next typed Channel envelope.
    pub async fn recv_channel(&mut self) -> Result<(u16, Vec<u8>), LinkClientError> {
        self.ensure_channel()?;
        loop {
            let event =
                self.event_rx.recv().await.ok_or_else(|| {
                    LinkClientError::HandshakeFailed("link channel closed".into())
                })?;
            let DestinationEvent::InboundPacket { raw, .. } = event else {
                continue;
            };
            let (header, offset) = match rns_wire::header::PacketHeader::unpack(&raw) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if header.destination_hash != self.id() {
                continue;
            }
            let body = &raw[offset..];
            if header.flags.packet_type == rns_wire::flags::PacketType::Proof {
                if let Some(index) = self
                    .channel_packets
                    .iter()
                    .position(|hash| self.link.validate_packet_proof(hash, body))
                {
                    let hash = self.channel_packets.remove(index);
                    self.channel
                        .as_mut()
                        .expect("channel initialized")
                        .delivered_by_packet_hash(&hash, self.link.rtt_secs());
                }
                continue;
            }
            if header.context != rns_wire::context::PacketContext::Channel {
                continue;
            }
            let packet_hash =
                rns_wire::hash::packet_hash(&raw, rns_wire::flags::HeaderType::Header1);
            let proof = self
                .link
                .prove_packet_with_link_key(&packet_hash)
                .map_err(|error| {
                    LinkClientError::LinkCrypto(format!("channel proof: {error:?}"))
                })?;
            send_transport(
                &self.transport_tx,
                TransportMessage::Outbound(OutboundRequest {
                    raw: build_proof_packet(
                        self.id(),
                        rns_wire::context::PacketContext::None,
                        &proof,
                    ),
                    destination_hash: self.id(),
                }),
            )
            .await?;
            let delivered = self
                .channel
                .as_mut()
                .expect("channel initialized")
                .receive_data(body)?;
            if let Some(message) = delivered.into_iter().next() {
                return Ok(message);
            }
        }
    }

    /// Receive and reassemble the next Resource sent over this Link.
    pub async fn recv_resource(
        &mut self,
        deadline: Duration,
    ) -> Result<ReceivedResource, LinkClientError> {
        self.recv_resource_inner(deadline, None, usize::MAX).await
    }

    /// Receive sequential Resource segments into a temporary file without
    /// retaining previously verified segments in RAM. `max_size` includes wire
    /// metadata; advertisements and actual decoded sizes are both checked.
    /// Proofs follow successful writes/flushes, not merely receipt of parts.
    /// The deadline includes file creation and reception. Errors/timeouts drop
    /// the partial file; no application destination path is overwritten.
    pub async fn recv_resource_file(
        &mut self,
        max_size: usize,
        deadline: Duration,
    ) -> Result<ReceivedFileResource, LinkClientError> {
        let expires = Instant::now() + deadline;
        let file = timeout(deadline, tokio::task::spawn_blocking(tempfile::tempfile))
            .await
            .map_err(|_| LinkClientError::Timeout("resource file creation"))?
            .map_err(|e| LinkClientError::Resource(format!("resource file: {e}")))?
            .map_err(|e| LinkClientError::Resource(format!("resource file: {e}")))?;
        let mut file = tokio::fs::File::from_std(file);
        let received = self
            .recv_resource_inner(time_remaining(expires)?, Some(&mut file), max_size)
            .await?;
        // The receiver has flushed and rewound before acknowledging completion.
        let data_size = timeout(time_remaining(expires)?, file.metadata())
            .await
            .map_err(|_| LinkClientError::Timeout("resource file metadata"))?
            .map_err(|e| LinkClientError::Resource(format!("resource file metadata: {e}")))?
            .len() as usize;
        Ok(ReceivedFileResource {
            file,
            data_size,
            metadata: received.metadata,
            resource_hash: received.resource_hash,
        })
    }

    async fn recv_resource_inner(
        &mut self,
        deadline: Duration,
        mut file: Option<&mut tokio::fs::File>,
        max_size: usize,
    ) -> Result<ReceivedResource, LinkClientError> {
        let link_id = self.id();
        let mut transfers: HashMap<[u8; 32], InboundTransfer> = HashMap::new();
        let future = async {
            let mut segment_info: HashMap<[u8; 32], ([u8; 32], usize, usize)> = HashMap::new();
            let mut multi: Option<MultiSegmentInbound> = None;
            let mut file_layout = None;
            let mut next_segment = 1;
            let mut file_bytes = 0usize;
            let mut file_metadata: Option<Vec<u8>> = None;
            let mut watchdog = tokio::time::interval(Duration::from_secs(1));
            watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                let raw = if let Some(raw) = self.pending_resource_packets.pop_front() {
                    raw
                } else {
                    loop {
                        let event = tokio::select! {
                            event = self.event_rx.recv() => event,
                            _ = watchdog.tick() => {
                                retry_response_resources(&self.transport_tx, &self.link, link_id, &mut transfers)?;
                                continue;
                            }
                        };
                        match event {
                            Some(DestinationEvent::InboundPacket { raw, .. }) => break raw,
                            Some(DestinationEvent::LinkClosed { link_id })
                                if link_id == self.id() =>
                            {
                                return Err(LinkClientError::HandshakeFailed(
                                    "link closed during resource".into(),
                                ));
                            }
                            Some(_) => {}
                            None => {
                                return Err(LinkClientError::HandshakeFailed(
                                    "resource channel closed".into(),
                                ));
                            }
                        }
                    }
                };
                let (header, offset) = match rns_wire::header::PacketHeader::unpack(&raw) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                if header.destination_hash != link_id {
                    continue;
                }
                let body = &raw[offset..];
                match header.context {
                    rns_wire::context::PacketContext::None
                        if header.flags.packet_type == rns_wire::flags::PacketType::Data =>
                    {
                        let packet = self.link.decrypt(body).map_err(|error| {
                            LinkClientError::LinkCrypto(format!("packet: {error:?}"))
                        })?;
                        self.prove_application_packet(&raw, header.flags.header_type)
                            .await?;
                        self.pending_packets.push_back(packet);
                    }
                    rns_wire::context::PacketContext::ResourceAdv => {
                        let plaintext = self.link.decrypt(body).map_err(|error| {
                            LinkClientError::LinkCrypto(format!(
                                "resource advertisement: {error:?}"
                            ))
                        })?;
                        let adv = ResourceAdvertisement::unpack(&plaintext).map_err(|error| {
                            LinkClientError::UnexpectedResponse(format!(
                                "resource advertisement: {error}"
                            ))
                        })?;
                        let mut random_hash = [0u8; rns_protocol::resource::RANDOM_HASH_SIZE];
                        if transfers.contains_key(&adv.resource_hash) {
                            continue;
                        }
                        // Guard every receive API before allocating segment slots,
                        // including the legacy byte-returning path.
                        if adv.total_segments == 0
                            || adv.total_segments > rns_protocol::resource::MAX_SEGMENTS
                            || adv.segment_index == 0
                            || adv.segment_index > adv.total_segments
                        {
                            send_link_data(
                                &self.transport_tx,
                                &self.link,
                                link_id,
                                rns_wire::context::PacketContext::ResourceRcl,
                                &adv.resource_hash,
                                true,
                            )?;
                            return Err(LinkClientError::Resource(
                                "invalid Resource segment count/index".into(),
                            ));
                        }
                        if file.is_some() {
                            let layout = (adv.original_hash, adv.total_segments, adv.data_size);
                            if adv.data_size > max_size
                                || adv.total_segments == 0
                                || adv.total_segments > rns_protocol::resource::MAX_SEGMENTS
                                || adv.segment_index > adv.total_segments
                                || adv.segment_index != next_segment
                                || !transfers.is_empty()
                                || file_layout.is_some_and(|expected| expected != layout)
                            {
                                let _ = send_link_data(
                                    &self.transport_tx,
                                    &self.link,
                                    link_id,
                                    rns_wire::context::PacketContext::ResourceRcl,
                                    &adv.resource_hash,
                                    true,
                                );
                                return Err(LinkClientError::Resource(
                                    "file Resource size or segment sequence mismatch".into(),
                                ));
                            }
                            file_layout = Some(layout);
                        }
                        let length = adv.random_hash.len().min(random_hash.len());
                        random_hash[..length].copy_from_slice(&adv.random_hash[..length]);
                        // Python repeats the metadata flag on later segments,
                        // but only the first segment contains the metadata prefix.
                        let mut transfer_flags = adv.flags;
                        if adv.total_segments > 1 && adv.segment_index > 1 {
                            transfer_flags.has_metadata = false;
                        }
                        let mut transfer = InboundTransfer::from_advertisement(
                            adv.num_parts,
                            adv.transfer_size,
                            adv.data_size,
                            random_hash,
                            adv.resource_hash,
                            transfer_flags,
                            adv.get_map_hashes(),
                            self.link.rtt.unwrap_or(Duration::from_millis(500)),
                        )
                        .map_err(|error| {
                            LinkClientError::UnexpectedResponse(format!("resource: {error:?}"))
                        })?;
                        if let TransferAction::SendRequest(request) = transfer.request_next() {
                            send_link_data(
                                &self.transport_tx,
                                &self.link,
                                link_id,
                                rns_wire::context::PacketContext::ResourceReq,
                                &request,
                                true,
                            )?;
                        }
                        segment_info.insert(
                            adv.resource_hash,
                            (adv.original_hash, adv.segment_index, adv.total_segments),
                        );
                        if file.is_none() && adv.total_segments > 1 && multi.is_none() {
                            multi = Some(MultiSegmentInbound::new(
                                adv.total_segments,
                                adv.original_hash,
                            ));
                        }
                        transfers.insert(adv.resource_hash, transfer);
                    }
                    rns_wire::context::PacketContext::Resource => {
                        let Some(resource_hash) = transfers.keys().next().copied() else {
                            continue;
                        };
                        let transfer = transfers.get_mut(&resource_hash).expect("known transfer");
                        let action = transfer.receive_part(body.to_vec());
                        let completed = matches!(action, TransferAction::Complete);
                        match action {
                            TransferAction::SendRequest(request) => send_link_data(
                                &self.transport_tx,
                                &self.link,
                                link_id,
                                rns_wire::context::PacketContext::ResourceReq,
                                &request,
                                true,
                            )?,
                            TransferAction::SendHmu(hmu) => send_link_data(
                                &self.transport_tx,
                                &self.link,
                                link_id,
                                rns_wire::context::PacketContext::ResourceHmu,
                                &hmu,
                                true,
                            )?,
                            TransferAction::Failed(reason) => {
                                return Err(LinkClientError::UnexpectedResponse(reason));
                            }
                            _ => {}
                        }
                        if transfer.resource.is_complete() || completed {
                            let keys = self.link.session_keys().ok_or_else(|| {
                                LinkClientError::LinkCrypto("missing resource keys".into())
                            })?;
                            let decrypt = move |data: &[u8]| {
                                rns_link::encryption::link_decrypt(&keys, data).map_err(|_| {
                                    rns_protocol::resource::ResourceError::DecryptFailed
                                })
                            };
                            let (data, proof) =
                                transfer.complete(Some(&decrypt)).map_err(|error| {
                                    LinkClientError::UnexpectedResponse(format!(
                                        "resource completion: {error:?}"
                                    ))
                                })?;
                            let metadata = transfer.resource.metadata.clone();
                            if let Some(file) = file.as_mut() {
                                use tokio::io::{AsyncSeekExt, AsyncWriteExt};
                                let (original_hash, index, total) = segment_info[&resource_hash];
                                let advertised_size =
                                    file_layout.expect("accepted file advertisement").2;
                                if index == 1 {
                                    file_metadata = metadata;
                                }
                                file_bytes += data.len();
                                let wire_size =
                                    file_bytes + file_metadata.as_ref().map_or(0, |m| m.len() + 3);
                                if wire_size > advertised_size
                                    || wire_size > max_size
                                    || (index == total && wire_size != advertised_size)
                                {
                                    return Err(LinkClientError::Resource(
                                        "decoded file Resource size mismatch".into(),
                                    ));
                                }
                                file.write_all(&data).await.map_err(|e| {
                                    LinkClientError::Resource(format!("resource file write: {e}"))
                                })?;
                                file.flush().await.map_err(|e| {
                                    LinkClientError::Resource(format!("resource file flush: {e}"))
                                })?;
                                if index == total {
                                    file.rewind().await.map_err(|e| {
                                        LinkClientError::Resource(format!(
                                            "resource file rewind: {e}"
                                        ))
                                    })?;
                                }
                                send_link_proof(&self.transport_tx, link_id, &proof)?;
                                transfers.remove(&resource_hash);
                                segment_info.remove(&resource_hash);
                                next_segment += 1;
                                if index == total {
                                    return Ok(ReceivedResource {
                                        data: Vec::new(),
                                        metadata: file_metadata,
                                        resource_hash: if total > 1 {
                                            original_hash
                                        } else {
                                            resource_hash
                                        },
                                    });
                                }
                                continue;
                            }
                            send_link_proof(&self.transport_tx, link_id, &proof)?;
                            let (original_hash, segment_index, total_segments) = segment_info
                                .remove(&resource_hash)
                                .unwrap_or((resource_hash, 1, 1));
                            transfers.remove(&resource_hash);
                            if total_segments > 1 {
                                let coordinator =
                                    multi.as_mut().expect("multi-segment coordinator");
                                coordinator.set_segment_data(segment_index, data).map_err(
                                    |error| {
                                        LinkClientError::UnexpectedResponse(format!(
                                            "resource segment: {error:?}"
                                        ))
                                    },
                                )?;
                                if let Some(metadata) = metadata {
                                    coordinator.set_metadata(metadata);
                                }
                                if coordinator.is_complete() {
                                    let data = coordinator.reassemble().map_err(|error| {
                                        LinkClientError::UnexpectedResponse(format!(
                                            "resource reassembly: {error:?}"
                                        ))
                                    })?;
                                    return Ok(ReceivedResource {
                                        data,
                                        metadata: coordinator.metadata.clone(),
                                        resource_hash: original_hash,
                                    });
                                }
                                continue;
                            }
                            return Ok(ReceivedResource {
                                data,
                                metadata,
                                resource_hash,
                            });
                        }
                    }
                    rns_wire::context::PacketContext::ResourceHmu => {
                        let plaintext = self.link.decrypt(body).map_err(|error| {
                            LinkClientError::LinkCrypto(format!("resource HMU: {error:?}"))
                        })?;
                        let (resource_hash, segment, hashmap) =
                            rns_protocol::resource::parse_hashmap_update(&plaintext).map_err(
                                |error| {
                                    LinkClientError::UnexpectedResponse(format!(
                                        "resource HMU: {error:?}"
                                    ))
                                },
                            )?;
                        if let Some(transfer) = transfers.get_mut(&resource_hash) {
                            match transfer.hashmap_update(segment, &hashmap) {
                                TransferAction::SendRequest(request) => send_link_data(
                                    &self.transport_tx,
                                    &self.link,
                                    link_id,
                                    rns_wire::context::PacketContext::ResourceReq,
                                    &request,
                                    true,
                                )?,
                                TransferAction::SendCancel { .. } | TransferAction::Failed(_) => {
                                    return Err(LinkClientError::Resource(
                                        "resource hashmap cancelled reception".into(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    }
                    rns_wire::context::PacketContext::ResourceIcl => {
                        if let Some(hash) = resource_cancel_hash(&self.link, body)
                            && (transfers.contains_key(&hash)
                                || multi
                                    .as_ref()
                                    .is_some_and(|coordinator| coordinator.original_hash == hash)
                                || file_layout.is_some_and(|layout| layout.0 == hash))
                        {
                            return Err(LinkClientError::Resource(
                                "sender cancelled Resource".into(),
                            ));
                        }
                    }
                    rns_wire::context::PacketContext::LinkClose => {
                        return Err(LinkClientError::HandshakeFailed(
                            "link closed during resource".into(),
                        ));
                    }
                    _ => {}
                }
            }
        };
        let result = timeout(deadline, future)
            .await
            .unwrap_or(Err(LinkClientError::Timeout("resource")));
        if result.is_err() {
            for hash in transfers.keys() {
                let _ = send_link_data(
                    &self.transport_tx,
                    &self.link,
                    link_id,
                    rns_wire::context::PacketContext::ResourceRcl,
                    hash,
                    true,
                );
            }
        }
        result
    }

    /// Send a Resource and wait for its delivery proof.
    pub async fn send_resource(
        &mut self,
        data: Vec<u8>,
        auto_compress: bool,
        deadline: Duration,
    ) -> Result<[u8; 32], LinkClientError> {
        self.send_resource_with_metadata(data, None, auto_compress, deadline)
            .await
    }

    /// Send a Resource with optional metadata and wait for all segment proofs.
    ///
    /// Payloads larger than the efficient single-resource limit are split
    /// automatically and retain one original resource hash across segments.
    pub async fn send_resource_with_metadata(
        &mut self,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        auto_compress: bool,
        deadline: Duration,
    ) -> Result<[u8; 32], LinkClientError> {
        self.send_resource_inner(data, metadata, auto_compress, deadline, None)
            .await
    }

    /// Send an unknown-length stream by spooling it to an anonymous temporary
    /// file before advertising. `max_size` limits source bytes (not metadata),
    /// additionally capped by protocol limits. An oversized source consumes at
    /// most one byte beyond that limit, then fails without advertising.
    ///
    /// The deadline covers spooling and all segment proofs. The spool is closed
    /// automatically on success, error, timeout or future cancellation. This
    /// uses temporary disk space, not a whole-source memory buffer.
    pub async fn send_resource_stream<R: tokio::io::AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
        max_size: usize,
        metadata: Option<Vec<u8>>,
        auto_compress: bool,
        deadline: Duration,
    ) -> Result<[u8; 32], LinkClientError> {
        let metadata_size = metadata
            .as_ref()
            .map_or(Some(0), |m| m.len().checked_add(3))
            .filter(|size| *size <= MAX_EFFICIENT_SIZE)
            .ok_or_else(|| LinkClientError::Resource("stream metadata exceeds limits".into()))?;
        if self.link.session_keys().is_none() {
            return Err(LinkClientError::LinkCrypto("missing resource keys".into()));
        }
        let limit = max_size
            .min(rns_protocol::resource::MAX_RESOURCE_SIZE)
            .min(MAX_EFFICIENT_SIZE * rns_protocol::resource::MAX_SEGMENTS - metadata_size);
        let expires = Instant::now() + deadline;
        let (mut spool, size) = timeout(deadline, spool_resource_stream(reader, limit))
            .await
            .map_err(|_| LinkClientError::Timeout("resource stream source"))??;
        self.send_resource_reader(
            &mut spool,
            size,
            metadata,
            auto_compress,
            time_remaining(expires)?,
        )
        .await
    }

    /// Send exactly `data_size` bytes from a reader, retaining only one segment
    /// at a time. No seek is required. Extra bytes remain unread; premature EOF
    /// is an error. The deadline includes source reads and all segment proofs.
    pub async fn send_resource_reader<R: tokio::io::AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
        data_size: usize,
        mut metadata: Option<Vec<u8>>,
        auto_compress: bool,
        deadline: Duration,
    ) -> Result<[u8; 32], LinkClientError> {
        use tokio::io::AsyncReadExt;
        let metadata_size = metadata
            .as_ref()
            .map_or(Some(0), |m| m.len().checked_add(3))
            .ok_or_else(|| LinkClientError::Resource("metadata size overflow".into()))?;
        let total_size = data_size
            .checked_add(metadata_size)
            .ok_or_else(|| LinkClientError::Resource("resource size overflow".into()))?;
        let segments = total_size.div_ceil(MAX_EFFICIENT_SIZE).max(1);
        if metadata_size > MAX_EFFICIENT_SIZE
            || data_size > rns_protocol::resource::MAX_RESOURCE_SIZE
            || segments > rns_protocol::resource::MAX_SEGMENTS
        {
            return Err(LinkClientError::Resource(
                "reader resource exceeds size limits".into(),
            ));
        }
        let expires = Instant::now() + deadline;
        let keys = self
            .link
            .session_keys()
            .ok_or_else(|| LinkClientError::LinkCrypto("missing resource keys".into()))?;
        let encrypt = |plaintext: &[u8]| {
            rns_link::encryption::link_encrypt(&keys, plaintext)
                .expect("validated Link session keys")
        };
        let mut remaining = data_size;
        let mut original_hash = None;
        let transfer = async {
            for index in 1..=segments {
                let budget = MAX_EFFICIENT_SIZE - if index == 1 { metadata_size } else { 0 };
                let mut chunk = vec![0; remaining.min(budget)];
                reader.read_exact(&mut chunk).await.map_err(|error| {
                    LinkClientError::Resource(format!("resource source: {error}"))
                })?;
                remaining -= chunk.len();
                let mut resource = OutboundResource::with_options(
                    chunk,
                    auto_compress,
                    metadata.take(),
                    None,
                    Some(&encrypt),
                )
                .map_err(|error| LinkClientError::Resource(format!("reader segment: {error:?}")))?;
                // Python's original hash is the FIRST segment hash, not a hash
                // of the entire source; later segments repeat it for routing.
                let original = *original_hash.get_or_insert(resource.resource_hash);
                resource.flags.split = segments > 1;
                resource.segment_index = index;
                resource.total_segments = segments;
                resource.advertisement_data_size = total_size;
                resource.original_hash = (segments > 1).then_some(original);
                let outbound = OutboundTransfer::from_prebuilt(resource, self.rtt());
                self.send_resource_transfer(outbound, expires).await?;
            }
            Ok(original_hash.expect("at least one segment"))
        };
        let result = timeout(deadline, transfer)
            .await
            .unwrap_or(Err(LinkClientError::Timeout("resource reader")));
        if result.is_err()
            && let Some(hash) = original_hash
        {
            let _ = send_link_data(
                &self.transport_tx,
                &self.link,
                self.id(),
                rns_wire::context::PacketContext::ResourceIcl,
                &hash,
                true,
            );
        }
        result
    }

    async fn send_resource_inner(
        &mut self,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        auto_compress: bool,
        deadline: Duration,
        request_id: Option<[u8; 16]>,
    ) -> Result<[u8; 32], LinkClientError> {
        let keys = self
            .link
            .session_keys()
            .ok_or_else(|| LinkClientError::LinkCrypto("missing resource keys".into()))?;
        let encrypt = |plaintext: &[u8]| {
            rns_link::encryption::link_encrypt(&keys, plaintext)
                .unwrap_or_else(|_| plaintext.to_vec())
        };
        let (resource_hash, resources) = if data.len()
            + metadata.as_ref().map_or(0, |m| m.len() + 3)
            <= MAX_EFFICIENT_SIZE
        {
            let resource =
                OutboundResource::with_options(data, auto_compress, metadata, None, Some(&encrypt))
                    .map_err(|error| LinkClientError::Resource(format!("{error:?}")))?;
            (resource.resource_hash, vec![resource])
        } else {
            let resource = MultiSegmentOutbound::with_options(
                data,
                auto_compress,
                metadata,
                None,
                false,
                Some(&encrypt),
            )
            .map_err(|error| LinkClientError::Resource(format!("{error:?}")))?;
            (resource.original_hash, resource.segments)
        };
        let deadline = Instant::now() + deadline;
        for mut resource in resources {
            if let Some(id) = request_id {
                resource.flags.is_request = true;
                resource.request_id = Some(id.to_vec());
            }
            let transfer = OutboundTransfer::from_prebuilt(resource, self.rtt());
            self.send_resource_transfer(transfer, deadline).await?;
        }
        Ok(resource_hash)
    }

    async fn send_resource_transfer(
        &mut self,
        mut transfer: OutboundTransfer,
        deadline: Instant,
    ) -> Result<(), LinkClientError> {
        let resource_hash = transfer.resource.resource_hash;
        let TransferAction::SendAdvertisement(advertisement) = transfer.tick() else {
            return Err(LinkClientError::Resource(
                "resource produced no advertisement".into(),
            ));
        };
        let encrypted = self
            .link
            .encrypt(&advertisement)
            .map_err(|error| LinkClientError::LinkCrypto(format!("resource ADV: {error:?}")))?;
        self.send_context(rns_wire::context::PacketContext::ResourceAdv, encrypted)
            .await?;

        let link_id = self.id();
        let future = async {
            let mut timer = tokio::time::interval(Duration::from_secs(1));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                let event = tokio::select! {
                    _ = timer.tick() => {
                        match transfer.check_sender_timeout(link_id) {
                            TransferAction::QueryProof(hash) => {
                                send_link_data(&self.transport_tx, &self.link, link_id,
                                    rns_wire::context::PacketContext::CacheRequest, &hash, false)?;
                            }
                            TransferAction::SendAdvertisement(payload) => {
                                send_link_data(&self.transport_tx, &self.link, link_id,
                                    rns_wire::context::PacketContext::ResourceAdv, &payload, true)?;
                            }
                            TransferAction::SendCancel(_, hash) => {
                                send_link_data(&self.transport_tx, &self.link, link_id,
                                    rns_wire::context::PacketContext::ResourceIcl, &hash, true)?;
                                return Err(LinkClientError::Resource("resource sender retries exhausted".into()));
                            }
                            _ => {}
                        }
                        continue;
                    }
                    event = self.event_rx.recv() => match event {
                        Some(event) => event,
                        None => break,
                    },
                };
                let DestinationEvent::InboundPacket { raw, .. } = event else {
                    continue;
                };
                let (header, offset) = match rns_wire::header::PacketHeader::unpack(&raw) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                if header.destination_hash != link_id {
                    continue;
                }
                let body = &raw[offset..];
                match header.context {
                    rns_wire::context::PacketContext::ResourceReq => {
                        let plaintext = self.link.decrypt(body).map_err(|error| {
                            LinkClientError::LinkCrypto(format!("resource request: {error:?}"))
                        })?;
                        let packet_hash =
                            rns_wire::hash::packet_hash(&raw, header.flags.header_type);
                        for action in transfer.handle_request_packet(packet_hash, &plaintext) {
                            match action {
                                TransferAction::SendPart(_, part) => {
                                    self.send_context(
                                        rns_wire::context::PacketContext::Resource,
                                        part,
                                    )
                                    .await?;
                                }
                                TransferAction::SendHmu(hmu) => {
                                    let encrypted = self.link.encrypt(&hmu).map_err(|error| {
                                        LinkClientError::LinkCrypto(format!(
                                            "resource HMU: {error:?}"
                                        ))
                                    })?;
                                    self.send_context(
                                        rns_wire::context::PacketContext::ResourceHmu,
                                        encrypted,
                                    )
                                    .await?;
                                }
                                TransferAction::Failed(reason) => {
                                    return Err(LinkClientError::Resource(reason));
                                }
                                TransferAction::SendCancel(cancel_type, hash) => {
                                    let context = match cancel_type {
                                        rns_protocol::resource::CancelType::Icl => {
                                            rns_wire::context::PacketContext::ResourceIcl
                                        }
                                        rns_protocol::resource::CancelType::Rcl => {
                                            rns_wire::context::PacketContext::ResourceRcl
                                        }
                                    };
                                    send_link_data(
                                        &self.transport_tx,
                                        &self.link,
                                        link_id,
                                        context,
                                        &hash,
                                        true,
                                    )?;
                                    return Err(LinkClientError::Resource(
                                        "resource request cancelled transfer".into(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    }
                    rns_wire::context::PacketContext::ResourcePrf => {
                        if transfer.handle_proof(body) {
                            return Ok(resource_hash);
                        }
                    }
                    rns_wire::context::PacketContext::ResourceRcl => {
                        if resource_cancel_hash(&self.link, body) == Some(resource_hash) {
                            return Err(LinkClientError::Resource(
                                "resource rejected by receiver".into(),
                            ));
                        }
                    }
                    rns_wire::context::PacketContext::LinkClose => {
                        return Err(LinkClientError::HandshakeFailed(
                            "link closed during resource send".into(),
                        ));
                    }
                    _ => {}
                }
            }
            Err(LinkClientError::HandshakeFailed(
                "resource channel closed".into(),
            ))
        };
        let result = timeout(deadline.saturating_duration_since(Instant::now()), future).await;
        if result.is_err() {
            let _ = send_link_data(
                &self.transport_tx,
                &self.link,
                link_id,
                rns_wire::context::PacketContext::ResourceIcl,
                &resource_hash,
                true,
            );
        }
        result.map_err(|_| LinkClientError::Timeout("resource proof"))??;
        Ok(())
    }

    pub async fn close(&mut self) -> Result<(), LinkClientError> {
        if let Some(payload) = self.link.teardown(CloseReason::InitiatorClosed) {
            self.send_context(rns_wire::context::PacketContext::LinkClose, payload)
                .await?;
        }
        let _ = self
            .transport_tx
            .send(TransportMessage::DeregisterDestination { hash: self.id() })
            .await;
        Ok(())
    }

    async fn send_context(
        &mut self,
        context: rns_wire::context::PacketContext,
        payload: Vec<u8>,
    ) -> Result<(), LinkClientError> {
        send_transport(
            &self.transport_tx,
            TransportMessage::Outbound(OutboundRequest {
                raw: build_data_packet(self.id(), context, &payload),
                destination_hash: self.id(),
            }),
        )
        .await
    }

    async fn prove_application_packet(
        &mut self,
        raw: &[u8],
        header_type: rns_wire::flags::HeaderType,
    ) -> Result<(), LinkClientError> {
        let packet_hash = rns_wire::hash::packet_hash(raw, header_type);
        let proof = self
            .link
            .prove_packet_with_link_key(&packet_hash)
            .map_err(|error| LinkClientError::LinkCrypto(format!("packet proof: {error:?}")))?;
        send_transport(
            &self.transport_tx,
            TransportMessage::Outbound(OutboundRequest {
                raw: build_proof_packet(
                    self.id(),
                    rns_wire::context::PacketContext::LinkProof,
                    &proof,
                ),
                destination_hash: self.id(),
            }),
        )
        .await
    }

    fn ensure_channel(&mut self) -> Result<(), LinkClientError> {
        if self.channel.is_none() {
            let keys = self
                .link
                .session_keys()
                .ok_or_else(|| LinkClientError::LinkCrypto("missing session keys".into()))?;
            self.channel = Some(LinkChannel::new_encrypted(
                self.id(),
                self.link.rtt_secs(),
                keys,
            ));
            self.link.mark_channel_created();
        }
        self.channel
            .as_mut()
            .expect("channel initialized")
            .set_link_mdu(self.link.mdu);
        Ok(())
    }
}

impl Drop for LinkSession {
    fn drop(&mut self) {
        let _registration = PendingLinkRegistration {
            tx: self.transport_tx.clone(),
            hash: self.id(),
            armed: true,
        };
    }
}

/// Flush before rewinding: Tokio file writes may still be in flight when the
/// copy completes. Anonymous tempfile ownership also cleans up cancelled reads.
async fn spool_resource_stream<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    limit: usize,
) -> Result<(tokio::fs::File, usize), LinkClientError> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
    let file = tokio::task::spawn_blocking(tempfile::tempfile)
        .await
        .map_err(|error| LinkClientError::Resource(format!("create stream spool: {error}")))?
        .map_err(|error| LinkClientError::Resource(format!("create stream spool: {error}")))?;
    let mut spool = tokio::fs::File::from_std(file);
    let copied = tokio::io::copy(&mut reader.take(limit as u64 + 1), &mut spool)
        .await
        .map_err(|error| LinkClientError::Resource(format!("spool resource source: {error}")))?;
    if copied > limit as u64 {
        return Err(LinkClientError::Resource(
            "stream resource exceeds size limits".into(),
        ));
    }
    spool
        .flush()
        .await
        .map_err(|error| LinkClientError::Resource(format!("flush stream spool: {error}")))?;
    spool
        .rewind()
        .await
        .map_err(|error| LinkClientError::Resource(format!("rewind stream spool: {error}")))?;
    Ok((spool, copied as usize))
}

async fn send_transport(
    transport_tx: &mpsc::Sender<TransportMessage>,
    message: TransportMessage,
) -> Result<(), LinkClientError> {
    transport_tx
        .send(message)
        .await
        .map_err(|_| LinkClientError::TransportUnavailable)
}

impl LinkClient {
    pub fn new(transport_tx: mpsc::Sender<TransportMessage>, identity: Identity) -> Self {
        Self {
            transport_tx,
            identity: Arc::new(identity),
        }
    }

    /// Open a Link to `app_name` on `remote_transport_hash`, send one
    /// request, return the response.
    pub async fn query(
        &self,
        remote_transport_hash: [u8; 16],
        app_name: &str,
        path: &str,
        payload: Vec<u8>,
        hops: u8,
        overall_timeout: Duration,
    ) -> Result<Vec<u8>, LinkClientError> {
        let started = Instant::now();
        let deadline = started + overall_timeout;

        let dest_hash =
            Destination::hash_from_name_and_identity(app_name, Some(&remote_transport_hash));

        // Register the handler before the path request so the answering
        // announce (carrying the pubkey) is observed.
        let (ann_tx, mut ann_rx) = mpsc::channel::<AnnounceHandlerEvent>(64);
        self.send_msg(TransportMessage::RegisterAnnounceHandler {
            aspect_filter: Some(app_name.to_string()),
            receive_path_responses: true,
            callback_tx: ann_tx,
        })
        .await?;

        self.send_msg(TransportMessage::RequestPath {
            destination_hash: dest_hash,
        })
        .await?;

        let pubkey = wait_for_pubkey(&mut ann_rx, dest_hash, time_remaining(deadline)?).await?;
        let _ = self
            .transport_tx
            .try_send(TransportMessage::DeregisterAnnounceHandler {
                aspect_filter: Some(app_name.to_string()),
            });

        let mtu = discover_link_mtu(&self.transport_tx, dest_hash, time_remaining(deadline)?).await;
        time_remaining(deadline)?;
        let (mut link, request_data) = Link::new_initiator_with_mtu(dest_hash, hops, mtu);
        let link_id = link.link_id;

        // Register link_id as a destination so inbound LRPROOF / Response
        // packets route back to this task via dest_rx.
        let (dest_tx, mut dest_rx) = mpsc::channel::<DestinationEvent>(128);
        self.send_msg(TransportMessage::RegisterDestination {
            hash: link_id,
            app_name: "rnstatus.linkclient".to_string(),
            delivery_tx: Some(dest_tx),
        })
        .await?;

        let req_pkt = build_link_request_packet(dest_hash, &request_data);
        let _registration = PendingLinkRegistration {
            tx: self.transport_tx.clone(),
            hash: link_id,
            armed: true,
        };
        self.send_msg(TransportMessage::Outbound(OutboundRequest {
            raw: req_pkt,
            destination_hash: dest_hash,
        }))
        .await?;

        let rtt_data = wait_for_proof(
            &self.transport_tx,
            &mut dest_rx,
            &mut link,
            &pubkey,
            time_remaining(deadline)?,
        )
        .await?;

        let rtt_pkt =
            build_data_packet(link_id, rns_wire::context::PacketContext::Lrrtt, &rtt_data);
        self.send_msg(TransportMessage::Outbound(OutboundRequest {
            raw: rtt_pkt,
            destination_hash: link_id,
        }))
        .await?;

        let our_pub = self.identity.get_public_key();
        let our_priv = self
            .identity
            .get_signing_key()
            .ok_or(LinkClientError::NoSigningKey)?;
        let identify_data = link
            .identify(&our_pub, &our_priv)
            .map_err(|e| LinkClientError::LinkCrypto(format!("identify: {e:?}")))?;
        let identify_pkt = build_data_packet(
            link_id,
            rns_wire::context::PacketContext::LinkIdentify,
            &identify_data,
        );
        self.send_msg(TransportMessage::Outbound(OutboundRequest {
            raw: identify_pkt,
            destination_hash: link_id,
        }))
        .await?;

        let req_timeout = Duration::from_secs(5);
        let (encrypted_req, request_id) = link
            .request(path, Some(&payload), req_timeout)
            .map_err(|e| LinkClientError::LinkCrypto(format!("request: {e:?}")))?;
        let request_pkt = build_data_packet(
            link_id,
            rns_wire::context::PacketContext::Request,
            &encrypted_req,
        );
        let packet_request_id = rns_wire::hash::truncated_packet_hash(
            &request_pkt,
            rns_wire::flags::HeaderType::Header1,
        );
        link.update_pending_request_id(&request_id, packet_request_id);
        self.send_msg(TransportMessage::Outbound(OutboundRequest {
            raw: request_pkt,
            destination_hash: link_id,
        }))
        .await?;

        let response = wait_for_response(
            &self.transport_tx,
            &mut dest_rx,
            &mut link,
            link_id,
            packet_request_id,
            time_remaining(deadline)?,
            usize::MAX,
            ResourceResponseMode::Packed,
        )
        .await
        .map(|response| response.data);

        // Tear down even on failure so the remote doesn't keep link state.
        let _ = self.send_close(&mut link).await;
        response
    }

    async fn send_msg(&self, msg: TransportMessage) -> Result<(), LinkClientError> {
        self.transport_tx
            .send(msg)
            .await
            .map_err(|_| LinkClientError::TransportUnavailable)
    }

    async fn send_close(&self, link: &mut Link) -> Result<(), LinkClientError> {
        let link_id = link.link_id;
        let Some(teardown_data) = link.teardown(CloseReason::InitiatorClosed) else {
            return Ok(());
        };
        let close_pkt = build_data_packet(
            link_id,
            rns_wire::context::PacketContext::LinkClose,
            &teardown_data,
        );
        self.send_msg(TransportMessage::Outbound(OutboundRequest {
            raw: close_pkt,
            destination_hash: link_id,
        }))
        .await
    }
}

fn time_remaining(deadline: Instant) -> Result<Duration, LinkClientError> {
    let now = Instant::now();
    if now >= deadline {
        Err(LinkClientError::Timeout("overall query"))
    } else {
        Ok(deadline - now)
    }
}

async fn wait_for_pubkey(
    rx: &mut mpsc::Receiver<AnnounceHandlerEvent>,
    target_dest_hash: [u8; 16],
    deadline: Duration,
) -> Result<[u8; 64], LinkClientError> {
    let fut = async {
        while let Some(ev) = rx.recv().await {
            if ev.destination_hash == target_dest_hash {
                if let Some(pk) = ev.public_key {
                    return Ok(pk);
                }
            }
        }
        Err(LinkClientError::PubkeyNotDiscovered)
    };
    timeout(deadline, fut)
        .await
        .map_err(|_| LinkClientError::Timeout("path/announce discovery"))?
}

async fn wait_for_proof(
    transport_tx: &mpsc::Sender<TransportMessage>,
    rx: &mut mpsc::Receiver<DestinationEvent>,
    link: &mut Link,
    public_key: &[u8; 64],
    deadline: Duration,
) -> Result<Vec<u8>, LinkClientError> {
    let link_id = link.link_id;
    let signing_bytes: [u8; 32] = public_key[32..]
        .try_into()
        .expect("fixed public key length");
    let verify_key = Ed25519PublicKey::from_bytes(&signing_bytes)
        .map_err(|e| LinkClientError::ProofInvalid(e.to_string()))?;
    let fut = async {
        while let Some(ev) = rx.recv().await {
            match ev {
                DestinationEvent::LinkClosed { link_id: closed_id } if closed_id == link_id => {
                    return Err(LinkClientError::HandshakeFailed("link closed".into()));
                }
                DestinationEvent::InboundPacket { raw, interface_id } => {
                    let (header, data_offset) = match rns_wire::header::PacketHeader::unpack(&raw) {
                        Ok(h) => h,
                        Err(_) => continue,
                    };
                    let is_proof = header.flags.packet_type == rns_wire::flags::PacketType::Proof
                        && header.context == rns_wire::context::PacketContext::Lrproof
                        && header.destination_hash == link_id;
                    if is_proof && raw.len() > data_offset {
                        let response = link_transport_query(
                            transport_tx,
                            TransportQuery::NormalizeInboundHops {
                                raw_hops: header.hops,
                                interface_id,
                            },
                        )
                        .await?;
                        let TransportQueryResponse::IntResult(hops) = response else {
                            return Err(LinkClientError::TransportUnavailable);
                        };
                        let hops = u8::try_from(hops)
                            .map_err(|_| LinkClientError::TransportUnavailable)?;
                        let rtt = match link.validate_proof_with_hops(
                            &raw[data_offset..],
                            hops,
                            &verify_key,
                            &signing_bytes,
                        ) {
                            Ok(rtt) => rtt,
                            // Invalid mismatched proofs must not close or poison
                            // a pending link. Continue within the original deadline.
                            Err(_) if link.state == LinkState::Pending => continue,
                            Err(e) => return Err(LinkClientError::ProofInvalid(format!("{e:?}"))),
                        };
                        let confirmed = link_transport_query(
                            transport_tx,
                            TransportQuery::ConfirmLocalLinkProof {
                                link_id,
                                interface_id,
                                dest: link.destination_hash,
                                hops,
                                rebalance: link.rebalanced.is_some(),
                            },
                        )
                        .await?;
                        if !matches!(confirmed, TransportQueryResponse::BoolResult(true)) {
                            return Err(LinkClientError::HandshakeFailed(
                                "link proof binding rejected".into(),
                            ));
                        }
                        return Ok(rtt);
                    }
                }
                _ => {}
            }
        }
        Err(LinkClientError::HandshakeFailed(
            "destination channel closed".into(),
        ))
    };
    timeout(deadline, fut)
        .await
        .map_err(|_| LinkClientError::Timeout("link proof"))?
}

// Only used within wait_for_proof's deadline; these in-process queries are
// deliberately not part of the external control RPC protocol.
async fn discover_link_mtu(
    transport_tx: &mpsc::Sender<TransportMessage>,
    destination: [u8; 16],
    remaining: Duration,
) -> u32 {
    // Bound send and response together, and never consume more than the
    // caller's remaining budget. Unknown/old actors retain base-MTU behaviour.
    match timeout(
        remaining.min(Duration::from_secs(1)),
        link_transport_query(
            transport_tx,
            TransportQuery::GetNextHopMtu { dest: destination },
        ),
    )
    .await
    {
        Ok(Ok(TransportQueryResponse::IntResult(value))) => u32::try_from(value)
            .ok()
            .filter(|mtu| *mtu >= rns_wire::constants::MTU as u32)
            .unwrap_or(rns_wire::constants::MTU as u32),
        _ => rns_wire::constants::MTU as u32,
    }
}

async fn link_transport_query(
    transport_tx: &mpsc::Sender<TransportMessage>,
    query: TransportQuery,
) -> Result<TransportQueryResponse, LinkClientError> {
    let (response_tx, response_rx) = oneshot::channel();
    send_transport(transport_tx, TransportMessage::Rpc { query, response_tx }).await?;
    response_rx
        .await
        .map_err(|_| LinkClientError::TransportUnavailable)
}

async fn wait_for_response(
    transport_tx: &mpsc::Sender<TransportMessage>,
    rx: &mut mpsc::Receiver<DestinationEvent>,
    link: &mut Link,
    link_id: [u8; 16],
    request_id: [u8; 16],
    deadline: Duration,
    max_response_bytes: usize,
    response_mode: ResourceResponseMode,
) -> Result<LinkResponse, LinkClientError> {
    let mut inbound_resources: HashMap<[u8; 32], InboundTransfer> = HashMap::new();
    let fut = async {
        let mut segment_info: HashMap<[u8; 32], ([u8; 32], usize, usize)> = HashMap::new();
        let mut multi: Option<MultiSegmentInbound> = None;
        let mut response_shape = None;
        let mut seen_segments = HashMap::new();
        let mut assembled_bytes = 0usize;
        let mut resource_timer = tokio::time::interval(Duration::from_secs(1));
        resource_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            let ev = tokio::select! {
                _ = resource_timer.tick() => {
                    retry_response_resources(transport_tx, link, link_id, &mut inbound_resources)?;
                    continue;
                }
                event = rx.recv() => match event {
                    Some(event) => event,
                    None => break,
                },
            };
            match ev {
                DestinationEvent::LinkClosed { link_id: closed_id } if closed_id == link_id => {
                    return Err(LinkClientError::HandshakeFailed("link closed".into()));
                }
                DestinationEvent::InboundPacket { raw, .. } => {
                    let (header, data_offset) = match rns_wire::header::PacketHeader::unpack(&raw) {
                        Ok(h) => h,
                        Err(_) => continue,
                    };
                    if header.destination_hash != link_id {
                        continue;
                    }
                    let body = &raw[data_offset..];
                    match header.context {
                        rns_wire::context::PacketContext::Response => {
                            match link.handle_response(body) {
                                Ok((id, response_data)) => {
                                    if id == request_id {
                                        if response_data.len() > max_response_bytes {
                                            return Err(LinkClientError::UnexpectedResponse(
                                                "response exceeds application size limit".into(),
                                            ));
                                        }
                                        return Ok(LinkResponse {
                                            data: response_data,
                                            metadata: None,
                                        });
                                    }
                                }
                                Err(e) => {
                                    return Err(LinkClientError::LinkCrypto(format!(
                                        "response decrypt: {e:?}"
                                    )));
                                }
                            }
                        }
                        rns_wire::context::PacketContext::ResourceAdv => {
                            let plaintext = link.decrypt(body).map_err(|e| {
                                LinkClientError::LinkCrypto(format!(
                                    "resource advertisement decrypt: {e:?}"
                                ))
                            })?;
                            let adv = ResourceAdvertisement::unpack(&plaintext).map_err(|e| {
                                LinkClientError::UnexpectedResponse(format!(
                                    "resource advertisement: {e}"
                                ))
                            })?;

                            if !adv.flags.is_response
                                || adv.request_id.as_deref() != Some(request_id.as_slice())
                            {
                                continue;
                            }
                            // `d` is the complete uncompressed size, repeated on
                            // every segment, including metadata and the envelope.
                            let shape = (adv.original_hash, adv.total_segments, adv.data_size);
                            let invalid = adv.total_segments == 0
                                || adv.total_segments > rns_protocol::resource::MAX_SEGMENTS
                                || adv.segment_index == 0
                                || adv.segment_index > adv.total_segments
                                || response_shape.is_some_and(|previous| previous != shape)
                                || seen_segments
                                    .get(&adv.segment_index)
                                    .is_some_and(|hash| *hash != adv.resource_hash)
                                || seen_segments.iter().any(|(index, hash)| {
                                    *index != adv.segment_index && *hash == adv.resource_hash
                                });
                            if adv.data_size > max_response_bytes || invalid {
                                send_link_data(
                                    transport_tx,
                                    link,
                                    link_id,
                                    rns_wire::context::PacketContext::ResourceRcl,
                                    &adv.resource_hash,
                                    true,
                                )?;
                                return Err(LinkClientError::UnexpectedResponse(
                                    if invalid {
                                        "inconsistent resource response segments"
                                    } else {
                                        "resource response exceeds application size limit"
                                    }
                                    .into(),
                                ));
                            }
                            // Retransmitted advertisements must not reset an
                            // active transfer or count a completed segment twice.
                            if seen_segments.contains_key(&adv.segment_index) {
                                continue;
                            }
                            response_shape = Some(shape);
                            seen_segments.insert(adv.segment_index, adv.resource_hash);

                            let mut random_hash = [0u8; rns_protocol::resource::RANDOM_HASH_SIZE];
                            let copy_len = adv.random_hash.len().min(random_hash.len());
                            random_hash[..copy_len].copy_from_slice(&adv.random_hash[..copy_len]);

                            let rtt = link.rtt.unwrap_or(Duration::from_millis(500));
                            let mut transfer_flags = adv.flags;
                            if adv.total_segments > 1 && adv.segment_index > 1 {
                                // Python repeats the flag but not the metadata prefix.
                                transfer_flags.has_metadata = false;
                            }
                            let mut transfer = InboundTransfer::from_advertisement(
                                adv.num_parts,
                                adv.transfer_size,
                                adv.data_size,
                                random_hash,
                                adv.resource_hash,
                                transfer_flags,
                                adv.get_map_hashes(),
                                rtt,
                            )
                            .map_err(|e| {
                                LinkClientError::UnexpectedResponse(format!(
                                    "resource transfer: {e:?}"
                                ))
                            })?;

                            if let TransferAction::SendRequest(req) = transfer.request_next() {
                                send_link_data(
                                    transport_tx,
                                    link,
                                    link_id,
                                    rns_wire::context::PacketContext::ResourceReq,
                                    &req,
                                    true,
                                )?;
                            }

                            segment_info.insert(
                                adv.resource_hash,
                                (adv.original_hash, adv.segment_index, adv.total_segments),
                            );
                            if adv.total_segments > 1 && multi.is_none() {
                                multi = Some(MultiSegmentInbound::new(
                                    adv.total_segments,
                                    adv.original_hash,
                                ));
                            }
                            inbound_resources.insert(adv.resource_hash, transfer);
                        }
                        rns_wire::context::PacketContext::Resource => {
                            let mut action_to_send = None;
                            let mut completed_rh = None;

                            for (rh, transfer) in &mut inbound_resources {
                                let action = transfer.receive_part(body.to_vec());
                                match action {
                                    TransferAction::SendHmu(_) | TransferAction::SendRequest(_) => {
                                        action_to_send = Some(action);
                                    }
                                    TransferAction::Complete => {
                                        completed_rh = Some(*rh);
                                    }
                                    TransferAction::Failed(reason) => {
                                        return Err(LinkClientError::UnexpectedResponse(format!(
                                            "resource transfer failed: {reason}"
                                        )));
                                    }
                                    _ => {}
                                }

                                if completed_rh.is_none() && transfer.resource.is_complete() {
                                    completed_rh = Some(*rh);
                                }
                                if action_to_send.is_some() || completed_rh.is_some() {
                                    break;
                                }
                            }

                            if let Some(action) = action_to_send {
                                let (context, payload) = match action {
                                    TransferAction::SendHmu(hmu) => {
                                        (rns_wire::context::PacketContext::ResourceHmu, hmu)
                                    }
                                    TransferAction::SendRequest(req) => {
                                        (rns_wire::context::PacketContext::ResourceReq, req)
                                    }
                                    _ => unreachable!(),
                                };
                                send_link_data(
                                    transport_tx,
                                    link,
                                    link_id,
                                    context,
                                    &payload,
                                    true,
                                )?;
                            }

                            if let Some(rh) = completed_rh {
                                let (assembled, proof, metadata) = {
                                    let transfer =
                                        inbound_resources.get_mut(&rh).ok_or_else(|| {
                                            LinkClientError::UnexpectedResponse(
                                                "completed resource disappeared".into(),
                                            )
                                        })?;
                                    let keys = link.session_keys().ok_or_else(|| {
                                        LinkClientError::LinkCrypto(
                                            "resource response missing link keys".into(),
                                        )
                                    })?;
                                    let decrypt_fn = move |data: &[u8]| {
                                        rns_link::encryption::link_decrypt(&keys, data).map_err(
                                            |_| {
                                                rns_protocol::resource::ResourceError::DecryptFailed
                                            },
                                        )
                                    };
                                    let (assembled, proof) =
                                        transfer.complete(Some(&decrypt_fn)).map_err(|e| {
                                            LinkClientError::UnexpectedResponse(format!(
                                                "resource assemble: {e:?}"
                                            ))
                                        })?;
                                    (assembled, proof, transfer.resource.metadata.clone())
                                };

                                assembled_bytes = assembled_bytes
                                    .saturating_add(assembled.len())
                                    .saturating_add(
                                        metadata.as_ref().map_or(0, |m| m.len().saturating_add(3)),
                                    );
                                if assembled_bytes > max_response_bytes {
                                    send_link_data(
                                        transport_tx,
                                        link,
                                        link_id,
                                        rns_wire::context::PacketContext::ResourceRcl,
                                        &rh,
                                        true,
                                    )?;
                                    return Err(LinkClientError::UnexpectedResponse(
                                        "assembled response exceeds application size limit".into(),
                                    ));
                                }
                                send_link_proof(transport_tx, link_id, &proof)?;
                                inbound_resources.remove(&rh);
                                let (_, segment_index, total_segments) =
                                    segment_info.remove(&rh).unwrap_or((rh, 1, 1));
                                let response_payload = if total_segments > 1 {
                                    let coordinator =
                                        multi.as_mut().expect("multi-segment coordinator");
                                    coordinator
                                        .set_segment_data(segment_index, assembled)
                                        .map_err(|error| {
                                            LinkClientError::UnexpectedResponse(format!(
                                                "resource response segment: {error:?}"
                                            ))
                                        })?;
                                    if let Some(metadata) = metadata.clone() {
                                        coordinator.set_metadata(metadata);
                                    }
                                    if !coordinator.is_complete() {
                                        continue;
                                    }
                                    coordinator.reassemble().map_err(|error| {
                                        LinkClientError::UnexpectedResponse(format!(
                                            "resource response reassembly: {error:?}"
                                        ))
                                    })?
                                } else {
                                    assembled
                                };
                                let metadata = multi
                                    .as_ref()
                                    .and_then(|coordinator| coordinator.metadata.clone())
                                    .or(metadata);
                                if response_mode == ResourceResponseMode::PythonFile
                                    && metadata.is_some()
                                {
                                    // The request id was already authenticated/matched
                                    // in every encrypted Resource advertisement (`q`).
                                    return Ok(LinkResponse {
                                        data: response_payload,
                                        metadata,
                                    });
                                }
                                match link.handle_response_plaintext(&response_payload) {
                                    Ok((id, response_data)) => {
                                        if id == request_id {
                                            return Ok(LinkResponse {
                                                data: response_data,
                                                metadata,
                                            });
                                        }
                                    }
                                    Err(e) => {
                                        return Err(LinkClientError::LinkCrypto(format!(
                                            "resource response decode: {e:?}"
                                        )));
                                    }
                                }
                            }
                        }
                        rns_wire::context::PacketContext::ResourceHmu => {
                            let plaintext = link.decrypt(body).map_err(|e| {
                                LinkClientError::LinkCrypto(format!(
                                    "resource hashmap update decrypt: {e:?}"
                                ))
                            })?;
                            let (rh, segment, hashmap) =
                                rns_protocol::resource::parse_hashmap_update(&plaintext).map_err(
                                    |e| {
                                        LinkClientError::UnexpectedResponse(format!(
                                            "resource hashmap update: {e:?}"
                                        ))
                                    },
                                )?;
                            let Some(transfer) = inbound_resources.get_mut(&rh) else {
                                continue;
                            };
                            match transfer.hashmap_update(segment, &hashmap) {
                                TransferAction::SendRequest(req) => {
                                    send_link_data(
                                        transport_tx,
                                        link,
                                        link_id,
                                        rns_wire::context::PacketContext::ResourceReq,
                                        &req,
                                        true,
                                    )?;
                                }
                                // Empty/invalid HMU cancels the transfer (RESOURCE_RCL, 1.3.9).
                                TransferAction::SendCancel(cancel_type, resource_hash) => {
                                    let context = match cancel_type {
                                        rns_protocol::resource::CancelType::Icl => {
                                            rns_wire::context::PacketContext::ResourceIcl
                                        }
                                        rns_protocol::resource::CancelType::Rcl => {
                                            rns_wire::context::PacketContext::ResourceRcl
                                        }
                                    };
                                    send_link_data(
                                        transport_tx,
                                        link,
                                        link_id,
                                        context,
                                        &resource_hash,
                                        true,
                                    )?;
                                    return Err(LinkClientError::Resource(
                                        "resource hashmap update cancelled response".into(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                        rns_wire::context::PacketContext::ResourceIcl => {
                            if let Some(resource_hash) = resource_cancel_hash(link, body)
                                && (inbound_resources.contains_key(&resource_hash)
                                    || multi.as_ref().is_some_and(|coordinator| {
                                        coordinator.original_hash == resource_hash
                                    }))
                            {
                                send_link_data(
                                    transport_tx,
                                    link,
                                    link_id,
                                    rns_wire::context::PacketContext::ResourceRcl,
                                    &resource_hash,
                                    true,
                                )?;
                                // Returning drops all active segments and the
                                // response coordinator, not merely this segment.
                                return Err(LinkClientError::Resource(
                                    "resource response cancelled by sender".into(),
                                ));
                            }
                        }
                        rns_wire::context::PacketContext::LinkClose
                            if link.receive_teardown(body) =>
                        {
                            return Err(LinkClientError::HandshakeFailed(
                                "link closed by remote".into(),
                            ));
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Err(LinkClientError::HandshakeFailed(
            "destination channel closed".into(),
        ))
    };
    let result = timeout(deadline, fut).await;
    if result.is_err() {
        for hash in inbound_resources.keys() {
            let _ = send_link_data(
                transport_tx,
                link,
                link_id,
                rns_wire::context::PacketContext::ResourceRcl,
                hash,
                true,
            );
        }
    }
    result.map_err(|_| LinkClientError::Timeout("response"))?
}

/// Drive the existing receive watchdog without extending the request deadline.
fn retry_response_resources(
    transport_tx: &mpsc::Sender<TransportMessage>,
    link: &Link,
    link_id: [u8; 16],
    resources: &mut HashMap<[u8; 32], InboundTransfer>,
) -> Result<(), LinkClientError> {
    for transfer in resources.values_mut() {
        match transfer.check_timeout() {
            TransferAction::SendRequest(payload) => send_link_data(
                transport_tx,
                link,
                link_id,
                rns_wire::context::PacketContext::ResourceReq,
                &payload,
                true,
            )?,
            TransferAction::Failed(reason) => {
                // The caller drops the complete response coordinator on error.
                // Cancel every active segment, not only the exhausted one.
                for hash in resources.keys() {
                    let _ = send_link_data(
                        transport_tx,
                        link,
                        link_id,
                        rns_wire::context::PacketContext::ResourceRcl,
                        hash,
                        true,
                    );
                }
                return Err(LinkClientError::Resource(reason));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Cancel controls are encrypted and identify one transfer by a full hash.
fn resource_cancel_hash(link: &Link, body: &[u8]) -> Option<[u8; 32]> {
    let plaintext = link.decrypt(body).ok()?;
    plaintext.get(..32)?.try_into().ok()
}

pub(crate) fn send_link_data(
    transport_tx: &mpsc::Sender<TransportMessage>,
    link: &Link,
    link_id: [u8; 16],
    context: rns_wire::context::PacketContext,
    body: &[u8],
    encrypt: bool,
) -> Result<(), LinkClientError> {
    let payload = if encrypt {
        link.encrypt(body)
            .map_err(|e| LinkClientError::LinkCrypto(format!("resource control: {e:?}")))?
    } else {
        body.to_vec()
    };
    let packet = build_data_packet(link_id, context, &payload);
    transport_tx
        .try_send(TransportMessage::Outbound(OutboundRequest {
            raw: packet,
            destination_hash: link_id,
        }))
        .map_err(|_| LinkClientError::TransportUnavailable)
}

pub(crate) fn send_link_proof(
    transport_tx: &mpsc::Sender<TransportMessage>,
    link_id: [u8; 16],
    proof: &[u8],
) -> Result<(), LinkClientError> {
    let packet = build_proof_packet(
        link_id,
        rns_wire::context::PacketContext::ResourcePrf,
        proof,
    );
    transport_tx
        .try_send(TransportMessage::Outbound(OutboundRequest {
            raw: packet,
            destination_hash: link_id,
        }))
        .map_err(|_| LinkClientError::TransportUnavailable)
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

fn build_proof_packet(
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
            packet_type: rns_wire::flags::PacketType::Proof,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn delivery_proof_recovers_stale_link_and_observes_close() {
        use rns_wire::context::PacketContext;
        for remote_close in [false, true] {
            let key = rns_crypto::ed25519::Ed25519PrivateKey::generate();
            let public = key.public_key();
            let (mut link, request) = Link::new_initiator([7; 16], 1);
            let (mut peer, proof) = Link::new_responder(&request, &key, [7; 16], 1).unwrap();
            let rtt = link
                .validate_proof(&proof, &public, &public.to_bytes())
                .unwrap();
            peer.receive_rtt_packet(&rtt).unwrap();
            let link_id = link.link_id;
            link.state = LinkState::Stale;
            let (tx, _output) = mpsc::channel(8);
            let (events, rx) = mpsc::channel(8);
            let mut session = LinkSession {
                transport_tx: tx,
                identity: Arc::new(Identity::new()),
                link,
                event_rx: rx,
                channel: None,
                channel_packets: Vec::new(),
                pending_packets: VecDeque::new(),
                pending_resource_packets: VecDeque::new(),
            };
            // Unrelated close events and unauthenticated teardown must not end the wait.
            events
                .send(DestinationEvent::LinkClosed { link_id: [0; 16] })
                .await
                .unwrap();
            events
                .send(DestinationEvent::InboundPacket {
                    raw: build_data_packet(link_id, PacketContext::LinkClose, &[0; 32]),
                    interface_id: 0,
                })
                .await
                .unwrap();
            let hash = [42; 32];
            events
                .send(DestinationEvent::InboundPacket {
                    raw: build_proof_packet(
                        link_id,
                        PacketContext::LinkProof,
                        &peer.prove_packet_with_link_key(&hash).unwrap(),
                    ),
                    interface_id: 0,
                })
                .await
                .unwrap();
            assert_eq!(
                session
                    .recv_delivery_proof(Duration::from_secs(1))
                    .await
                    .unwrap(),
                hash
            );
            assert_eq!(session.link.state, LinkState::Active);
            let closed = if remote_close {
                DestinationEvent::InboundPacket {
                    raw: build_data_packet(
                        link_id,
                        PacketContext::LinkClose,
                        &peer.teardown(CloseReason::DestinationClosed).unwrap(),
                    ),
                    interface_id: 0,
                }
            } else {
                DestinationEvent::LinkClosed { link_id }
            };
            events.send(closed).await.unwrap();
            assert!(
                matches!(session.recv_delivery_proof(Duration::from_secs(1)).await,
                Err(LinkClientError::HandshakeFailed(message)) if message.contains("link closed"))
            );
        }
    }

    #[tokio::test]
    async fn resource_file_handle_receives_segments_and_rejects_oversize() {
        use rns_wire::context::PacketContext;
        use tokio::io::AsyncReadExt;
        let key = rns_crypto::ed25519::Ed25519PrivateKey::generate();
        let public = key.public_key();
        let (mut link, request) = Link::new_initiator([7; 16], 1);
        let (peer, proof) = Link::new_responder(&request, &key, [7; 16], 1).unwrap();
        link.validate_proof(&proof, &public, &public.to_bytes())
            .unwrap();
        let link_id = link.link_id;
        let (tx, mut output) = mpsc::channel(8);
        let (events, rx) = mpsc::channel(8);
        let session = LinkSession {
            transport_tx: tx,
            identity: Arc::new(Identity::new()),
            link,
            event_rx: rx,
            channel: None,
            channel_packets: Vec::new(),
            pending_packets: VecDeque::new(),
            pending_resource_packets: VecDeque::new(),
        };
        let (command_tx, command_rx) = mpsc::channel(4);
        let (inbound_tx, inbound_rx) = mpsc::channel(4);
        let handle = LinkSessionHandle {
            link_id,
            command_tx,
            inbound_rx: Arc::new(tokio::sync::Mutex::new(inbound_rx)),
        };
        let worker = tokio::spawn(run_established_link_session(
            session, command_rx, inbound_tx,
        ));
        // A command that expired in the queue must not start receiving.
        let (result_tx, result_rx) = oneshot::channel();
        handle
            .send_command(LinkSessionCommand::ReceiveResourceFile {
                max_size: 13,
                expires: Instant::now() - Duration::from_secs(1),
                result_tx,
            })
            .await
            .unwrap();
        assert!(matches!(
            recv_command_result(result_rx).await,
            Err(LinkClientError::Timeout(_))
        ));
        assert!(output.try_recv().is_err());
        let sending = async {
            let mut original = None;
            let mut first_adv = None;
            for (index, data) in [(1, b"first".as_slice()), (2, b"tail".as_slice())] {
                let encrypt = |data: &[u8]| peer.encrypt(data).unwrap();
                let mut resource = OutboundResource::with_options(
                    data.to_vec(),
                    false,
                    (index == 1).then_some(vec![0x80]),
                    None,
                    Some(&encrypt),
                )
                .unwrap();
                let root = *original.get_or_insert(resource.resource_hash);
                resource.flags.split = true;
                // Python retains this flag even on segments without the prefix.
                resource.flags.has_metadata = true;
                resource.segment_index = index;
                resource.total_segments = 2;
                resource.original_hash = Some(root);
                resource.advertisement_data_size = 13;
                let mut transfer =
                    OutboundTransfer::from_prebuilt(resource, Duration::from_millis(500));
                let TransferAction::SendAdvertisement(adv) = transfer.tick() else {
                    panic!("ADV")
                };
                let raw = build_data_packet(
                    link_id,
                    PacketContext::ResourceAdv,
                    &peer.encrypt(&adv).unwrap(),
                );
                first_adv.get_or_insert(raw.clone());
                events
                    .send(DestinationEvent::InboundPacket {
                        raw,
                        interface_id: 0,
                    })
                    .await
                    .unwrap();
                let TransportMessage::Outbound(request) = output.recv().await.unwrap() else {
                    panic!("REQ")
                };
                let (header, _) = rns_wire::header::PacketHeader::unpack(&request.raw).unwrap();
                assert_eq!(header.context, PacketContext::ResourceReq);
                assert_eq!(transfer.resource.parts.len(), 1);
                events
                    .send(DestinationEvent::InboundPacket {
                        raw: build_data_packet(
                            link_id,
                            PacketContext::Resource,
                            &transfer.resource.parts[0],
                        ),
                        interface_id: 0,
                    })
                    .await
                    .unwrap();
                let TransportMessage::Outbound(proof) = output.recv().await.unwrap() else {
                    panic!("proof")
                };
                let (header, offset) = rns_wire::header::PacketHeader::unpack(&proof.raw).unwrap();
                assert_eq!(header.context, PacketContext::ResourcePrf);
                assert!(transfer.resource.validate_proof(&proof.raw[offset..]));
            }
            (original.unwrap(), first_adv.unwrap())
        };
        let (received, (root, adv)) = tokio::join!(
            handle.recv_resource_file(13, Duration::from_secs(2)),
            sending,
        );
        let mut received = received.unwrap();
        assert_eq!(received.resource_hash, root);
        assert_eq!(received.metadata, Some(vec![0x80]));
        assert_eq!(received.data_size, 9);
        let mut bytes = Vec::new();
        received.file.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, b"firsttail");
        events
            .send(DestinationEvent::InboundPacket {
                raw: adv,
                interface_id: 0,
            })
            .await
            .unwrap();
        assert!(matches!(
            handle.recv_resource_file(12, Duration::from_secs(1)).await,
            Err(LinkClientError::Resource(_))
        ));
        let TransportMessage::Outbound(cancel) = output.recv().await.unwrap() else {
            panic!("RCL")
        };
        let (header, _) = rns_wire::header::PacketHeader::unpack(&cancel.raw).unwrap();
        assert_eq!(header.context, PacketContext::ResourceRcl);
        handle.close().await.unwrap();
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn resource_stream_spool_flushes_rewinds_and_bounds_input() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut writer, mut reader) = tokio::io::duplex(4);
        let writing = async {
            writer.write_all(b"stream data").await.unwrap();
            writer.shutdown().await.unwrap();
        };
        let (prepared, ()) = tokio::join!(spool_resource_stream(&mut reader, 11), writing);
        let (mut spool, size) = prepared.unwrap();
        assert_eq!(size, 11);
        let mut restored = Vec::new();
        spool.read_to_end(&mut restored).await.unwrap();
        assert_eq!(restored, b"stream data");

        let mut excessive = b"123456".as_slice();
        assert!(matches!(
            spool_resource_stream(&mut excessive, 3).await,
            Err(LinkClientError::Resource(_))
        ));
        assert_eq!(excessive, b"56", "only one excess byte is consumed");
        let (_, size) = spool_resource_stream(&mut tokio::io::empty(), 0)
            .await
            .unwrap();
        assert_eq!(size, 0);
    }

    #[tokio::test]
    async fn reader_resource_preserves_segment_size_and_source_position() {
        use rns_wire::context::PacketContext;
        let key = rns_crypto::ed25519::Ed25519PrivateKey::generate();
        let public = key.public_key();
        let (mut link, request) = Link::new_initiator([7; 16], 1);
        let (peer, proof) = Link::new_responder(&request, &key, [7; 16], 1).unwrap();
        link.validate_proof(&proof, &public, &public.to_bytes())
            .unwrap();
        let link_id = link.link_id;
        let (tx, mut output) = mpsc::channel(8);
        let (events, rx) = mpsc::channel(8);
        let mut session = LinkSession {
            transport_tx: tx,
            identity: Arc::new(Identity::new()),
            link,
            event_rx: rx,
            channel: None,
            channel_packets: Vec::new(),
            pending_packets: VecDeque::new(),
            pending_resource_packets: VecDeque::new(),
        };
        let size = MAX_EFFICIENT_SIZE + 7;
        let data = vec![42; size + 1];
        let mut reader = data.as_slice();
        let sending =
            session.send_resource_reader(&mut reader, size, None, true, Duration::from_secs(2));
        let receiving = async {
            let mut original = None;
            for (index, bytes) in [(1, MAX_EFFICIENT_SIZE), (2, 7)] {
                let TransportMessage::Outbound(packet) = output.recv().await.unwrap() else {
                    panic!("ADV")
                };
                let (header, offset) = rns_wire::header::PacketHeader::unpack(&packet.raw).unwrap();
                assert_eq!(header.context, PacketContext::ResourceAdv);
                let adv =
                    ResourceAdvertisement::unpack(&peer.decrypt(&packet.raw[offset..]).unwrap())
                        .unwrap();
                let first = *original.get_or_insert(adv.resource_hash);
                assert_eq!(adv.original_hash, first);
                assert_eq!(adv.data_size, size);
                assert_eq!((adv.segment_index, adv.total_segments), (index, 2));
                assert!(adv.flags.split);
                // Admission/source test: the fixture already knows the source
                // bytes and provides a valid proof without transferring parts.
                let expected = rns_protocol::resource::compute_expected_proof(
                    &vec![42; bytes],
                    &adv.resource_hash,
                );
                let mut proof = adv.resource_hash.to_vec();
                proof.extend_from_slice(&expected);
                events
                    .send(DestinationEvent::InboundPacket {
                        raw: build_proof_packet(link_id, PacketContext::ResourcePrf, &proof),
                        interface_id: 0,
                    })
                    .await
                    .unwrap();
            }
            original.unwrap()
        };
        let (result, original) = tokio::join!(sending, receiving);
        assert_eq!(result.unwrap(), original);
        assert_eq!(reader.len(), 1, "read exactly the declared length");
        let mut short = b"abc".as_slice();
        assert!(matches!(
            session
                .send_resource_reader(&mut short, 4, None, false, Duration::from_secs(1))
                .await,
            Err(LinkClientError::Resource(_))
        ));
        assert!(
            output.try_recv().is_err(),
            "early EOF must not advertise a partial segment"
        );
        let mut excessive = b"123456".as_slice();
        assert!(matches!(
            session
                .send_resource_stream(&mut excessive, 3, None, false, Duration::from_secs(1))
                .await,
            Err(LinkClientError::Resource(_))
        ));
        assert_eq!(excessive, b"56");
        assert!(
            output.try_recv().is_err(),
            "oversized stream must not advertise"
        );
    }

    #[test]
    fn response_resource_timer_retries_then_cancels() {
        use rns_wire::context::PacketContext;
        let key = rns_crypto::ed25519::Ed25519PrivateKey::generate();
        let public = key.public_key();
        let (mut link, request) = Link::new_initiator([7; 16], 1);
        let (peer, proof) = Link::new_responder(&request, &key, [7; 16], 1).unwrap();
        link.validate_proof(&proof, &public, &public.to_bytes())
            .unwrap();
        let hash = [0xa5; 32];
        let mut transfer = InboundTransfer::from_advertisement(
            1,
            64,
            64,
            [0xb6; 4],
            hash,
            rns_protocol::resource::ResourceFlags::default(),
            vec![[0xc7; 4]],
            Duration::from_millis(500),
        )
        .unwrap();
        transfer.request_next();
        let mut resources = HashMap::from([(hash, transfer)]);
        let (tx, mut rx) = mpsc::channel(8);
        for (retries, context) in [
            (1, PacketContext::ResourceReq),
            (0, PacketContext::ResourceRcl),
        ] {
            let transfer = resources.get_mut(&hash).unwrap();
            transfer.retries_left = retries;
            transfer.last_activity = Instant::now() - Duration::from_secs(60);
            let result = retry_response_resources(&tx, &link, link.link_id, &mut resources);
            assert_eq!(result.is_err(), retries == 0);
            let TransportMessage::Outbound(packet) = rx.try_recv().unwrap() else {
                panic!("outbound")
            };
            let (header, offset) = rns_wire::header::PacketHeader::unpack(&packet.raw).unwrap();
            assert_eq!(header.context, context);
            let plaintext = peer.decrypt(&packet.raw[offset..]).unwrap();
            if retries == 0 {
                assert_eq!(plaintext, hash);
            } else {
                assert_eq!(&plaintext[1..33], &hash);
            }
        }
        assert_eq!(link.state, LinkState::Active);
    }

    #[tokio::test]
    async fn malformed_resource_request_sends_cancel_without_waiting() {
        use rns_wire::context::PacketContext;
        let key = rns_crypto::ed25519::Ed25519PrivateKey::generate();
        let public = key.public_key();
        let (mut link, request) = Link::new_initiator([7; 16], 1);
        let (mut peer, proof) = Link::new_responder(&request, &key, [7; 16], 1).unwrap();
        let rtt = link
            .validate_proof(&proof, &public, &public.to_bytes())
            .unwrap();
        peer.receive_rtt_packet(&rtt).unwrap();
        let link_id = link.link_id;
        let transfer =
            OutboundTransfer::new(vec![42; 2000], false, Duration::from_millis(10)).unwrap();
        let hash = transfer.resource.resource_hash;
        let mut invalid_request = vec![rns_protocol::resource::HASHMAP_IS_EXHAUSTED];
        invalid_request.extend_from_slice(&transfer.resource.map_hashes[0]);
        invalid_request.extend_from_slice(&hash);
        let (tx, mut outbound) = mpsc::channel(8);
        let (events, rx) = mpsc::channel(8);
        events
            .send(DestinationEvent::InboundPacket {
                raw: build_data_packet(
                    link_id,
                    PacketContext::ResourceReq,
                    &peer.encrypt(&invalid_request).unwrap(),
                ),
                interface_id: 0,
            })
            .await
            .unwrap();
        let mut session = LinkSession {
            transport_tx: tx,
            identity: Arc::new(Identity::new()),
            link,
            event_rx: rx,
            channel: None,
            channel_packets: Vec::new(),
            pending_packets: VecDeque::new(),
            pending_resource_packets: VecDeque::new(),
        };
        let result = session
            .send_resource_transfer(transfer, Instant::now() + Duration::from_secs(1))
            .await;
        assert!(
            matches!(result, Err(LinkClientError::Resource(_))),
            "{result:?}"
        );
        for context in [PacketContext::ResourceAdv, PacketContext::ResourceIcl] {
            let TransportMessage::Outbound(packet) = outbound.try_recv().unwrap() else {
                panic!("outbound")
            };
            let (header, offset) = rns_wire::header::PacketHeader::unpack(&packet.raw).unwrap();
            assert_eq!(header.context, context);
            if context == PacketContext::ResourceIcl {
                assert_eq!(peer.decrypt(&packet.raw[offset..]).unwrap(), hash);
            }
        }
        assert_eq!(session.link.state, LinkState::Active);
    }

    #[tokio::test]
    async fn response_split_metadata_flag_and_receive_segment_cap() {
        use rns_wire::context::PacketContext;
        for response_mode in [
            ResourceResponseMode::Packed,
            ResourceResponseMode::PythonFile,
        ] {
            let key = rns_crypto::ed25519::Ed25519PrivateKey::generate();
            let public = key.public_key();
            let (mut client, request) = Link::new_initiator([7; 16], 1);
            let (server, proof) = Link::new_responder(&request, &key, [7; 16], 1).unwrap();
            client
                .validate_proof(&proof, &public, &public.to_bytes())
                .unwrap();
            let link_id = client.link_id;
            let request_id = [8; 16];
            let payload = Link::pack_response(&request_id, b"reply").unwrap();
            let (tx, mut output) = mpsc::channel(8);
            let (events, mut rx) = mpsc::channel(8);
            let mut original = None;
            let mut resources = Vec::new();
            for (index, data) in [(1, &payload[..5]), (2, &payload[5..])] {
                let encrypt = |data: &[u8]| server.encrypt(data).unwrap();
                let mut resource = OutboundResource::with_options(
                    data.to_vec(),
                    false,
                    (index == 1).then_some(vec![0x80]),
                    None,
                    Some(&encrypt),
                )
                .unwrap();
                let root = *original.get_or_insert(resource.resource_hash);
                resource.flags.split = true;
                resource.flags.is_response = true;
                resource.flags.has_metadata = true;
                resource.request_id = Some(request_id.to_vec());
                resource.original_hash = Some(root);
                resource.segment_index = index;
                resource.total_segments = 2;
                resource.advertisement_data_size = payload.len() + 4;
                let mut transfer =
                    OutboundTransfer::from_prebuilt(resource, Duration::from_millis(500));
                let TransferAction::SendAdvertisement(adv) = transfer.tick() else {
                    panic!("ADV")
                };
                events
                    .send(DestinationEvent::InboundPacket {
                        raw: build_data_packet(
                            link_id,
                            PacketContext::ResourceAdv,
                            &server.encrypt(&adv).unwrap(),
                        ),
                        interface_id: 0,
                    })
                    .await
                    .unwrap();
                assert_eq!(transfer.resource.parts.len(), 1);
                events
                    .send(DestinationEvent::InboundPacket {
                        raw: build_data_packet(
                            link_id,
                            PacketContext::Resource,
                            &transfer.resource.parts[0],
                        ),
                        interface_id: 0,
                    })
                    .await
                    .unwrap();
                resources.push(transfer.resource);
            }
            let response = wait_for_response(
                &tx,
                &mut rx,
                &mut client,
                link_id,
                request_id,
                Duration::from_secs(1),
                payload.len() + 4,
                response_mode,
            )
            .await
            .unwrap();
            match response_mode {
                ResourceResponseMode::Packed => assert_eq!(response.data, b"reply"),
                // File bytes deliberately resemble a valid response envelope: do
                // not guess and strip them in Python mode.
                ResourceResponseMode::PythonFile => assert_eq!(response.data, payload),
            }
            assert_eq!(response.metadata, Some(vec![0x80]));
            for mut resource in resources {
                let _request = output.try_recv().unwrap();
                let TransportMessage::Outbound(proof) = output.try_recv().unwrap() else {
                    panic!("proof")
                };
                let (header, offset) = rns_wire::header::PacketHeader::unpack(&proof.raw).unwrap();
                assert_eq!(header.context, PacketContext::ResourcePrf);
                assert!(resource.validate_proof(&proof.raw[offset..]));
            }
            let mut session = LinkSession {
                transport_tx: tx,
                identity: Arc::new(Identity::new()),
                link: client,
                event_rx: rx,
                channel: None,
                channel_packets: Vec::new(),
                pending_packets: VecDeque::new(),
                pending_resource_packets: VecDeque::new(),
            };
            let mut adv = ResourceAdvertisement::new(
                16,
                32,
                1,
                [3; 32],
                vec![0; 4],
                rns_protocol::resource::ResourceFlags::default(),
                &[[3; 4]],
                rns_wire::constants::ENCRYPTED_MDU,
            );
            adv.total_segments = rns_protocol::resource::MAX_SEGMENTS + 1;
            events
                .send(DestinationEvent::InboundPacket {
                    raw: build_data_packet(
                        link_id,
                        PacketContext::ResourceAdv,
                        &server.encrypt(&adv.pack()).unwrap(),
                    ),
                    interface_id: 0,
                })
                .await
                .unwrap();
            assert!(matches!(
                session.recv_resource(Duration::from_secs(1)).await,
                Err(LinkClientError::Resource(_))
            ));
            let TransportMessage::Outbound(cancel) = output.try_recv().unwrap() else {
                panic!("RCL")
            };
            let (header, _) = rns_wire::header::PacketHeader::unpack(&cancel.raw).unwrap();
            assert_eq!(header.context, PacketContext::ResourceRcl);
        }
    }

    #[tokio::test]
    async fn response_size_limit_uses_total_advertised_size() {
        use rns_wire::context::PacketContext;
        for (limit, cancel) in [(31, false), (32, false), (33, false), (32, true)] {
            let key = rns_crypto::ed25519::Ed25519PrivateKey::generate();
            let public = key.public_key();
            let (mut client, request) = Link::new_initiator([7; 16], 1);
            let (mut server, proof) = Link::new_responder(&request, &key, [7; 16], 1).unwrap();
            let rtt = client
                .validate_proof(&proof, &public, &public.to_bytes())
                .unwrap();
            server.receive_rtt_packet(&rtt).unwrap();
            let link_id = client.link_id;
            assert_eq!(resource_cancel_hash(&client, &[1; 32]), None);
            assert_eq!(
                resource_cancel_hash(&client, &server.encrypt(&[1; 31]).unwrap()),
                None
            );
            let request_id = [8; 16];
            let (tx, mut outbound) = mpsc::channel(8);
            let (events, mut rx) = mpsc::channel(8);
            // Each segment advertises the SAME total d=32. Include a duplicate
            // ADV to verify that it neither resets nor double-counts a transfer.
            for index in [1, 1, 2] {
                let mut adv = ResourceAdvertisement::new(
                    16,
                    32,
                    1,
                    [index as u8; 32],
                    vec![0; 4],
                    rns_protocol::resource::ResourceFlags {
                        is_response: true,
                        split: true,
                        ..Default::default()
                    },
                    &[[index as u8; 4]],
                    rns_wire::constants::ENCRYPTED_MDU,
                );
                adv.original_hash = [1; 32];
                adv.segment_index = index;
                adv.total_segments = 2;
                adv.request_id = Some(request_id.to_vec());
                events
                    .send(DestinationEvent::InboundPacket {
                        raw: build_data_packet(
                            link_id,
                            PacketContext::ResourceAdv,
                            &server.encrypt(&adv.pack()).unwrap(),
                        ),
                        interface_id: 0,
                    })
                    .await
                    .unwrap();
            }
            if cancel {
                for hash in [[9; 32], [2; 32]] {
                    events
                        .send(DestinationEvent::InboundPacket {
                            raw: build_data_packet(
                                link_id,
                                PacketContext::ResourceIcl,
                                &server.encrypt(&hash).unwrap(),
                            ),
                            interface_id: 0,
                        })
                        .await
                        .unwrap();
                }
            }
            events
                .send(DestinationEvent::LinkClosed { link_id })
                .await
                .unwrap();
            let result = wait_for_response(
                &tx,
                &mut rx,
                &mut client,
                link_id,
                request_id,
                Duration::from_secs(1),
                limit,
                ResourceResponseMode::Packed,
            )
            .await;
            if limit < 32 {
                assert!(matches!(
                    result,
                    Err(LinkClientError::UnexpectedResponse(_))
                ));
                let TransportMessage::Outbound(packet) = outbound.try_recv().unwrap() else {
                    panic!("outbound")
                };
                let (header, offset) = rns_wire::header::PacketHeader::unpack(&packet.raw).unwrap();
                assert_eq!(header.context, PacketContext::ResourceRcl);
                assert_eq!(server.decrypt(&packet.raw[offset..]).unwrap(), [1; 32]);
            } else {
                if cancel {
                    assert!(
                        matches!(result, Err(LinkClientError::Resource(_))),
                        "{result:?}"
                    );
                } else {
                    assert!(
                        matches!(result, Err(LinkClientError::HandshakeFailed(_))),
                        "{result:?}"
                    );
                }
                for _ in 0..2 {
                    let TransportMessage::Outbound(packet) = outbound.try_recv().unwrap() else {
                        panic!("outbound")
                    };
                    assert_eq!(
                        rns_wire::header::PacketHeader::unpack(&packet.raw)
                            .unwrap()
                            .0
                            .context,
                        PacketContext::ResourceReq
                    );
                }
            }
            if cancel {
                let TransportMessage::Outbound(packet) = outbound.try_recv().unwrap() else {
                    panic!("outbound")
                };
                let (header, offset) = rns_wire::header::PacketHeader::unpack(&packet.raw).unwrap();
                assert_eq!(header.context, PacketContext::ResourceRcl);
                assert_eq!(server.decrypt(&packet.raw[offset..]).unwrap(), [2; 32]);
            }
            assert!(outbound.try_recv().is_err());
        }
    }

    #[tokio::test]
    async fn discovered_mtu_reaches_prepared_request_with_safe_fallbacks() {
        for (value, expected) in [
            (1196, 1196),
            (262144, 262144),
            (-1, 500),
            (0, 500),
            (499, 500),
        ] {
            let (tx, mut rx) = mpsc::channel(1);
            let actor = tokio::spawn(async move {
                let Some(TransportMessage::Rpc {
                    query: TransportQuery::GetNextHopMtu { dest },
                    response_tx,
                }) = rx.recv().await
                else {
                    panic!("MTU query")
                };
                assert_eq!(dest, [0xab; 16]);
                response_tx
                    .send(TransportQueryResponse::IntResult(value))
                    .unwrap();
            });
            let mtu = discover_link_mtu(&tx, [0xab; 16], Duration::from_secs(1)).await;
            let prepared = LinkSession::prepare_on_transport_with_mtu(
                tx,
                Identity::new(),
                [0xab; 16],
                [0xcd; 64],
                1,
                mtu,
            );
            assert_eq!(prepared.link.mtu, expected);
            assert_eq!(
                rns_link::handshake::LinkRequestData::unpack(&prepared.request_data)
                    .unwrap()
                    .signalling
                    .mtu,
                expected
            );
            actor.await.unwrap();
        }
    }

    #[tokio::test]
    async fn mtu_discovery_bounds_blocked_transport_admission() {
        let (tx, _rx) = mpsc::channel(1);
        tx.send(TransportMessage::Shutdown).await.unwrap();
        let mtu = timeout(
            Duration::from_secs(1),
            discover_link_mtu(&tx, [0xab; 16], Duration::from_millis(10)),
        )
        .await
        .expect("query must bound send as well as response");
        assert_eq!(mtu, 500);
    }

    #[tokio::test]
    async fn failed_establishment_releases_registered_destination() {
        let (tx, mut rx) = mpsc::channel(8);
        let prepared = LinkSession::prepare_on_transport(
            tx,
            Identity::new(),
            [0xad; 16],
            Identity::new().get_public_key(),
            4,
        );
        let link_id = prepared.id();
        let task = tokio::spawn(prepared.establish(Duration::from_millis(20)));
        let TransportMessage::RegisterDestination {
            delivery_tx, hash, ..
        } = rx.recv().await.unwrap()
        else {
            panic!("registration")
        };
        assert_eq!(hash, link_id);
        // Keep the channel open to exercise timeout, not channel-closed cleanup.
        let _delivery_tx = delivery_tx;
        assert!(matches!(
            rx.recv().await.unwrap(),
            TransportMessage::Outbound(_)
        ));
        assert!(matches!(
            task.await.unwrap(),
            Err(LinkClientError::Timeout(_))
        ));
        assert!(
            matches!(rx.recv().await.unwrap(), TransportMessage::DeregisterDestination { hash } if hash == link_id)
        );
    }

    #[tokio::test]
    async fn cancelled_registration_cleanup_survives_full_transport_queue() {
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(TransportMessage::Shutdown).await.unwrap();
        drop(PendingLinkRegistration {
            tx,
            hash: [0xae; 16],
            armed: true,
        });
        assert!(matches!(
            rx.recv().await.unwrap(),
            TransportMessage::Shutdown
        ));
        assert!(
            matches!(timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap(), TransportMessage::DeregisterDestination { hash } if hash == [0xae; 16])
        );
    }

    #[tokio::test]
    #[ignore = "requires the local Python Reticulum reference and python3.11"]
    async fn python_link_rebalance_and_active_route_binding() {
        check_python_link_rebalance(false).await;
    }

    #[tokio::test]
    #[ignore = "requires the local Python Reticulum reference and python3.11"]
    async fn python_legacy_link_rebalance_and_active_route_binding() {
        check_python_link_rebalance(true).await;
    }

    async fn check_python_link_rebalance(legacy: bool) {
        use rns_transport::constants::{InterfaceDirection, InterfaceMode};
        use rns_transport::messages::{InboundPacket, InterfaceEntry};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let reference =
            std::env::var("RNS_REFERENCE").unwrap_or_else(|_| "/home/room/src/Reticulum".into());
        let mut child = tokio::process::Command::new("python3.11")
            .arg("-B")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/link_rebalance_peer.py"
            ))
            .arg(reference)
            .arg(if legacy { "legacy" } else { "modern" })
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
        async fn read_json(
            output: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
        ) -> serde_json::Value {
            let line = timeout(Duration::from_secs(10), output.next_line())
                .await
                .unwrap()
                .unwrap()
                .expect("Python output");
            serde_json::from_str(&line).unwrap()
        }
        async fn write_packet(input: &mut tokio::process::ChildStdin, raw: &[u8]) {
            input
                .write_all(format!("{}\n", hex::encode(raw)).as_bytes())
                .await
                .unwrap();
            input.flush().await.unwrap();
        }
        fn from_hex(value: &serde_json::Value, field: &str) -> Vec<u8> {
            hex::decode(value[field].as_str().unwrap()).unwrap()
        }
        let initial = read_json(&mut output).await;
        let dest: [u8; 16] = from_hex(&initial, "destination").try_into().unwrap();
        let public: [u8; 64] = from_hex(&initial, "public_key").try_into().unwrap();
        let announce = from_hex(&initial, "announce");
        let (actor, tx) = rns_transport::actor::TransportActor::new();
        let actor_task = tokio::spawn(actor.run());
        let (network1, mut rx1) = mpsc::channel(32);
        let (network2, mut rx2) = mpsc::channel(32);
        for (id, network, gravity) in [(1, network1, -10), (2, network2, 10)] {
            let mut entry = InterfaceEntry::new(
                format!("path-{id}"),
                InterfaceMode::Full,
                InterfaceDirection {
                    inbound: true,
                    outbound: true,
                },
                1_000_000,
                500,
                network,
            );
            entry.gravity = gravity;
            tx.send(TransportMessage::RegisterInterface { id, entry })
                .await
                .unwrap();
        }
        let mut first = announce.clone();
        first[1] = 3; // advertised route is four hops; proof arrives at two
        tx.send(TransportMessage::Inbound(InboundPacket {
            raw: first.into(),
            interface_id: 1,
            rssi: None,
            snr: None,
            q: None,
        }))
        .await
        .unwrap();
        let prepared =
            LinkSession::prepare_on_transport(tx.clone(), Identity::new(), dest, public, 4);
        let establishing = tokio::spawn(prepared.establish(Duration::from_secs(10)));
        let request = timeout(Duration::from_secs(5), rx1.recv())
            .await
            .unwrap()
            .unwrap();
        write_packet(&mut input, &request).await;
        let proof = from_hex(&read_json(&mut output).await, "proof");
        let mut forged = proof.clone();
        let (_, offset) = rns_wire::header::PacketHeader::unpack(&proof).unwrap();
        forged[offset] ^= 1;
        for raw in [forged, proof] {
            tx.send(TransportMessage::Inbound(InboundPacket {
                raw: raw.into(),
                interface_id: 1,
                rssi: None,
                snr: None,
                q: None,
            }))
            .await
            .unwrap();
        }
        let mut session = establishing.await.unwrap().unwrap();
        assert_eq!(session.link.expected_hops, Some(2));
        assert!(session.link.rebalanced.is_some());
        let rtt = timeout(Duration::from_secs(5), rx1.recv())
            .await
            .unwrap()
            .unwrap();
        write_packet(&mut input, &rtt).await;
        assert_eq!(read_json(&mut output).await["rtt_ok"], true);

        let mut alternative = announce;
        alternative[1] = 1;
        tx.send(TransportMessage::Inbound(InboundPacket {
            raw: alternative.into(),
            interface_id: 2,
            rssi: None,
            snr: None,
            q: None,
        }))
        .await
        .unwrap();
        let TransportQueryResponse::PathTable(paths) =
            link_transport_query(&tx, TransportQuery::GetPathTable)
                .await
                .unwrap()
        else {
            panic!("path table")
        };
        let path = paths.iter().find(|path| path.hash == dest).unwrap();
        assert_eq!((path.interface_id, path.hops), (2, 2));
        session
            .send(b"still on the established interface")
            .await
            .unwrap();
        let data = timeout(Duration::from_secs(5), rx1.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(rx2.try_recv().is_err());
        write_packet(&mut input, &data).await;
        let reply = from_hex(&read_json(&mut output).await, "reply");
        tx.send(TransportMessage::Inbound(InboundPacket {
            raw: reply.clone().into(),
            interface_id: 2,
            rssi: None,
            snr: None,
            q: None,
        }))
        .await
        .unwrap();
        // A replay on the wrong interface must not poison the dedup filter.
        tx.send(TransportMessage::Inbound(InboundPacket {
            raw: reply.into(),
            interface_id: 1,
            rssi: None,
            snr: None,
            q: None,
        }))
        .await
        .unwrap();
        assert_eq!(
            timeout(Duration::from_secs(5), session.recv())
                .await
                .unwrap()
                .unwrap(),
            b"python reply after gravity change"
        );
        session.close().await.unwrap();
        tx.send(TransportMessage::Shutdown).await.unwrap();
        timeout(Duration::from_secs(5), actor_task)
            .await
            .unwrap()
            .unwrap();
        assert!(
            timeout(Duration::from_secs(5), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }

    #[tokio::test]
    async fn pending_proof_rebalances_only_after_valid_proof_and_uses_transport_hops() {
        let identity = Identity::new();
        let signing = identity.get_signing_key().unwrap();
        let (mut link, request) = Link::new_initiator([0xac; 16], 4);
        let (_, proof) = Link::new_responder(&request, &signing, [0xac; 16], 1).unwrap();
        let (tx, mut transport_rx) = mpsc::channel(8);
        let (events, mut rx) = mpsc::channel(8);
        let mut forged = proof.clone();
        forged[0] ^= 1;
        for body in [forged, proof] {
            events
                .send(DestinationEvent::InboundPacket {
                    raw: build_proof_packet(
                        link.link_id,
                        rns_wire::context::PacketContext::Lrproof,
                        &body,
                    ),
                    interface_id: 7,
                })
                .await
                .unwrap();
        }
        let link_id = link.link_id;
        let worker = tokio::spawn(async move {
            // A bad proof asks only for normalisation; it cannot mutate paths.
            for _ in 0..2 {
                let TransportMessage::Rpc {
                    query:
                        TransportQuery::NormalizeInboundHops {
                            raw_hops: 0,
                            interface_id: 7,
                        },
                    response_tx,
                } = transport_rx.recv().await.unwrap()
                else {
                    panic!("expected hop query")
                };
                response_tx
                    .send(TransportQueryResponse::IntResult(2))
                    .unwrap();
            }
            let TransportMessage::Rpc {
                query:
                    TransportQuery::ConfirmLocalLinkProof {
                        link_id: bound,
                        dest,
                        hops,
                        interface_id,
                        rebalance,
                    },
                response_tx,
            } = transport_rx.recv().await.unwrap()
            else {
                panic!("expected authenticated confirmation")
            };
            assert_eq!(bound, link_id);
            assert_eq!(dest, [0xac; 16]);
            assert_eq!((hops, interface_id, rebalance), (2, 7, true));
            response_tx
                .send(TransportQueryResponse::BoolResult(true))
                .unwrap();
        });
        wait_for_proof(
            &tx,
            &mut rx,
            &mut link,
            &identity.get_public_key(),
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(link.state, LinkState::Active);
        assert_eq!(link.expected_hops, Some(2));
        assert!(link.rebalanced.is_some());
        worker.await.unwrap();
    }

    #[test]
    fn build_link_request_packet_has_link_request_type() {
        let pkt = build_link_request_packet([0xAA; 16], &[0x01, 0x02, 0x03]);
        let (header, _) = rns_wire::header::PacketHeader::unpack(&pkt).unwrap();
        assert_eq!(
            header.flags.packet_type,
            rns_wire::flags::PacketType::LinkRequest
        );
        assert_eq!(header.destination_hash, [0xAA; 16]);
    }

    #[test]
    fn build_data_packet_carries_context() {
        let pkt = build_data_packet([0xBB; 16], rns_wire::context::PacketContext::Lrrtt, &[0x42]);
        let (header, _) = rns_wire::header::PacketHeader::unpack(&pkt).unwrap();
        assert_eq!(header.context, rns_wire::context::PacketContext::Lrrtt);
        assert_eq!(header.flags.packet_type, rns_wire::flags::PacketType::Data);
    }

    #[test]
    fn build_proof_packet_uses_link_proof_type() {
        let packet = build_proof_packet(
            [0xBD; 16],
            rns_wire::context::PacketContext::LinkProof,
            &[0x42],
        );
        let (header, _) = rns_wire::header::PacketHeader::unpack(&packet).unwrap();
        assert_eq!(header.flags.packet_type, rns_wire::flags::PacketType::Proof);
        assert_eq!(header.destination_hash, [0xBD; 16]);
        assert_eq!(header.context, rns_wire::context::PacketContext::LinkProof);
    }

    #[test]
    fn prepared_session_exposes_link_id_without_transport_io() {
        let (transport_tx, mut transport_rx) = mpsc::channel(1);
        let prepared = LinkSession::prepare_on_transport(
            transport_tx,
            Identity::new(),
            [0xBC; 16],
            [0xCD; 64],
            1,
        );

        assert_ne!(prepared.id(), [0; 16]);
        assert!(transport_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn time_remaining_returns_err_after_deadline() {
        let past = Instant::now();
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert!(matches!(
            time_remaining(past),
            Err(LinkClientError::Timeout(_))
        ));
    }

    #[tokio::test]
    async fn send_close_uses_authenticated_teardown_payload() {
        let dest_hash = [0xCC; 16];
        let responder_key = rns_crypto::ed25519::Ed25519PrivateKey::generate();
        let responder_pub = responder_key.public_key();
        let (mut initiator, request_data) = Link::new_initiator(dest_hash, 1);
        let (mut responder, proof_data) =
            Link::new_responder(&request_data, &responder_key, dest_hash, 1).unwrap();
        let rtt_data = initiator
            .validate_proof(&proof_data, &responder_pub, &responder_pub.to_bytes())
            .unwrap();
        responder.receive_rtt_packet(&rtt_data).unwrap();

        let (transport_tx, mut transport_rx) = mpsc::channel(4);
        let client = LinkClient::new(transport_tx, Identity::new());
        client.send_close(&mut initiator).await.unwrap();

        let TransportMessage::Outbound(request) = transport_rx.try_recv().unwrap() else {
            panic!("expected outbound close packet");
        };
        let (header, offset) = rns_wire::header::PacketHeader::unpack(&request.raw).unwrap();
        assert_eq!(header.context, rns_wire::context::PacketContext::LinkClose);
        assert!(responder.receive_teardown(&request.raw[offset..]));
    }

    #[tokio::test]
    async fn reusable_session_watchdog_sends_link_keepalive() {
        let dest_hash = [0xCF; 16];
        let responder_key = rns_crypto::ed25519::Ed25519PrivateKey::generate();
        let responder_pub = responder_key.public_key();
        let (mut initiator, request_data) = Link::new_initiator(dest_hash, 1);
        let (mut responder, proof_data) =
            Link::new_responder(&request_data, &responder_key, dest_hash, 1).unwrap();
        let rtt_data = initiator
            .validate_proof(&proof_data, &responder_pub, &responder_pub.to_bytes())
            .unwrap();
        responder.receive_rtt_packet(&rtt_data).unwrap();
        initiator.keepalive.update_from_rtt(Duration::ZERO);
        initiator.keepalive.last_inbound =
            Instant::now().checked_sub(Duration::from_secs(6)).unwrap();

        let link_id = initiator.link_id;
        let (transport_tx, mut transport_rx) = mpsc::channel(4);
        let (_event_tx, event_rx) = mpsc::channel(4);
        let mut session = LinkSession {
            transport_tx,
            identity: Arc::new(Identity::new()),
            link: initiator,
            event_rx,
            channel: None,
            channel_packets: Vec::new(),
            pending_packets: VecDeque::new(),
            pending_resource_packets: VecDeque::new(),
        };

        session.drive_watchdog().await.unwrap();
        let TransportMessage::Outbound(request) = transport_rx.try_recv().unwrap() else {
            panic!("expected outbound keepalive");
        };
        let (header, offset) = rns_wire::header::PacketHeader::unpack(&request.raw).unwrap();
        assert_eq!(header.destination_hash, link_id);
        assert_eq!(header.context, rns_wire::context::PacketContext::Keepalive);
        assert_eq!(
            &request.raw[offset..],
            &[rns_link::constants::KEEPALIVE_REQUEST]
        );
    }

    #[tokio::test]
    async fn session_worker_identifies_over_established_link() {
        let dest_hash = [0xCE; 16];
        let responder_key = rns_crypto::ed25519::Ed25519PrivateKey::generate();
        let responder_pub = responder_key.public_key();
        let (mut initiator, request_data) = Link::new_initiator(dest_hash, 1);
        let (mut responder, proof_data) =
            Link::new_responder(&request_data, &responder_key, dest_hash, 1).unwrap();
        let rtt_data = initiator
            .validate_proof(&proof_data, &responder_pub, &responder_pub.to_bytes())
            .unwrap();
        responder.receive_rtt_packet(&rtt_data).unwrap();

        let link_id = initiator.link_id;
        let identity = Arc::new(Identity::new());
        let expected_public_key = identity.get_public_key();
        let (transport_tx, mut transport_rx) = mpsc::channel(4);
        let (_event_tx, event_rx) = mpsc::channel(4);
        let session = LinkSession {
            transport_tx,
            identity,
            link: initiator,
            event_rx,
            channel: None,
            channel_packets: Vec::new(),
            pending_packets: VecDeque::new(),
            pending_resource_packets: VecDeque::new(),
        };
        let (command_tx, command_rx) = mpsc::channel(4);
        let (inbound_tx, inbound_rx) = mpsc::channel(4);
        let handle = LinkSessionHandle {
            link_id,
            command_tx,
            inbound_rx: Arc::new(tokio::sync::Mutex::new(inbound_rx)),
        };
        let worker = tokio::spawn(run_established_link_session(
            session, command_rx, inbound_tx,
        ));

        handle.identify().await.unwrap();

        let TransportMessage::Outbound(request) = transport_rx.recv().await.unwrap() else {
            panic!("expected outbound Link identification");
        };
        let (header, offset) = rns_wire::header::PacketHeader::unpack(&request.raw).unwrap();
        assert_eq!(header.destination_hash, link_id);
        assert_eq!(
            header.context,
            rns_wire::context::PacketContext::LinkIdentify
        );
        assert_eq!(
            responder
                .handle_identification(&request.raw[offset..])
                .unwrap(),
            expected_public_key
        );

        handle.close().await.unwrap();
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn session_worker_reuses_link_for_python_style_packet_proofs() {
        let dest_hash = [0xCD; 16];
        let responder_key = rns_crypto::ed25519::Ed25519PrivateKey::generate();
        let responder_pub = responder_key.public_key();
        let (mut initiator, request_data) = Link::new_initiator(dest_hash, 1);
        let (mut responder, proof_data) =
            Link::new_responder(&request_data, &responder_key, dest_hash, 1).unwrap();
        let rtt_data = initiator
            .validate_proof(&proof_data, &responder_pub, &responder_pub.to_bytes())
            .unwrap();
        responder.receive_rtt_packet(&rtt_data).unwrap();

        let link_id = initiator.link_id;
        let (transport_tx, mut transport_rx) = mpsc::channel(4);
        let (event_tx, event_rx) = mpsc::channel(4);
        let session = LinkSession {
            transport_tx,
            identity: Arc::new(Identity::new()),
            link: initiator,
            event_rx,
            channel: None,
            channel_packets: Vec::new(),
            pending_packets: VecDeque::new(),
            pending_resource_packets: VecDeque::new(),
        };
        let (command_tx, command_rx) = mpsc::channel(4);
        let (inbound_tx, inbound_rx) = mpsc::channel(4);
        let handle = LinkSessionHandle {
            link_id,
            command_tx,
            inbound_rx: Arc::new(tokio::sync::Mutex::new(inbound_rx)),
        };
        let worker = tokio::spawn(run_established_link_session(
            session, command_rx, inbound_tx,
        ));

        for payload in [b"first payload".as_slice(), b"second payload".as_slice()] {
            let send = handle.send_payload(payload.to_vec(), true, Duration::from_secs(1));
            let prove = async {
                let TransportMessage::Outbound(request) = transport_rx.recv().await.unwrap() else {
                    panic!("expected outbound Link packet");
                };
                let packet_hash =
                    rns_wire::hash::packet_hash(&request.raw, rns_wire::flags::HeaderType::Header1);
                let proof = responder.prove_packet_with_link_key(&packet_hash).unwrap();
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
                    // Python Link.prove_packet() uses the default context for
                    // application packet proofs.
                    context: rns_wire::context::PacketContext::None,
                };
                let mut raw = header.pack().expect("locally constructed header");
                raw.extend_from_slice(&proof);
                event_tx
                    .send(DestinationEvent::InboundPacket {
                        raw: Bytes::from(raw),
                        interface_id: 0,
                    })
                    .await
                    .unwrap();
                packet_hash
            };

            let (receipt, packet_hash) = tokio::join!(send, prove);
            assert_eq!(
                receipt.unwrap(),
                LinkPayloadSendReceipt::Packet {
                    link_id,
                    packet_hash,
                }
            );
        }

        let inbound_payload = b"backchannel payload";
        let encrypted = responder.encrypt(inbound_payload).unwrap();
        let inbound_raw =
            build_data_packet(link_id, rns_wire::context::PacketContext::None, &encrypted);
        let inbound_hash =
            rns_wire::hash::packet_hash(&inbound_raw, rns_wire::flags::HeaderType::Header1);
        event_tx
            .send(DestinationEvent::InboundPacket {
                raw: inbound_raw,
                interface_id: 0,
            })
            .await
            .unwrap();

        assert_eq!(handle.recv().await.unwrap(), inbound_payload);
        let TransportMessage::Outbound(proof_request) = transport_rx.recv().await.unwrap() else {
            panic!("expected outbound LINKPROOF");
        };
        let (proof_header, proof_offset) =
            rns_wire::header::PacketHeader::unpack(&proof_request.raw).unwrap();
        assert_eq!(
            proof_header.flags.packet_type,
            rns_wire::flags::PacketType::Proof
        );
        assert_eq!(
            proof_header.context,
            rns_wire::context::PacketContext::LinkProof
        );
        assert!(responder.validate_packet_proof(&inbound_hash, &proof_request.raw[proof_offset..]));

        drop(handle);
        worker.await.unwrap();
    }
}
