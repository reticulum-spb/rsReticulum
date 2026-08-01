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
        send_transport(
            &self.transport_tx,
            TransportMessage::Outbound(OutboundRequest {
                raw: build_link_request_packet(self.destination_hash, &self.request_data),
                destination_hash: self.destination_hash,
            }),
        )
        .await?;

        let proof_data = wait_for_proof(&mut event_rx, link_id, deadline).await?;
        let ed25519_bytes: [u8; 32] = self.public_key[32..]
            .try_into()
            .map_err(|_| LinkClientError::ProofInvalid("invalid public key".into()))?;
        let verify_key = Ed25519PublicKey::from_bytes(&ed25519_bytes)
            .map_err(|error| LinkClientError::ProofInvalid(error.to_string()))?;
        let rtt_data = self
            .link
            .validate_proof(&proof_data, &verify_key, &ed25519_bytes)
            .map_err(|error| LinkClientError::ProofInvalid(format!("{error:?}")))?;
        send_transport(
            &self.transport_tx,
            TransportMessage::Outbound(OutboundRequest {
                raw: build_data_packet(link_id, rns_wire::context::PacketContext::Lrrtt, &rtt_data),
                destination_hash: link_id,
            }),
        )
        .await?;
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
        let (link, request_data) = Link::new_initiator(destination_hash, hops);
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
        Self::prepare_with_public_key(runtime, identity, destination_hash, public_key, hops)
            .establish(deadline)
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
            let DestinationEvent::InboundPacket { raw, .. } = event else {
                continue;
            };
            let (header, offset) = match rns_wire::header::PacketHeader::unpack(&raw) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if header.destination_hash == link_id
                && header.flags.packet_type == rns_wire::flags::PacketType::Data
                && header.context == rns_wire::context::PacketContext::None
            {
                let packet = self
                    .link
                    .decrypt(&raw[offset..])
                    .map_err(|error| LinkClientError::LinkCrypto(format!("packet: {error:?}")))?;
                self.prove_application_packet(&raw, header.flags.header_type)
                    .await?;
                self.pending_packets.push_back(packet);
                continue;
            }
            if header.destination_hash != link_id
                || header.flags.packet_type != rns_wire::flags::PacketType::Proof
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

    /// Send a request while rejecting a response before Resource assembly
    /// exceeds the application-provided payload limit.
    pub async fn request_with_metadata_limit(
        &mut self,
        path: &str,
        data: Option<&[u8]>,
        deadline: Duration,
        max_response_bytes: usize,
    ) -> Result<LinkResponse, LinkClientError> {
        let (encrypted, request_id) = self
            .link
            .request(path, data, deadline)
            .map_err(|error| LinkClientError::LinkCrypto(format!("request: {error:?}")))?;
        let packet = build_data_packet(
            self.id(),
            rns_wire::context::PacketContext::Request,
            &encrypted,
        );
        let packet_request_id =
            rns_wire::hash::truncated_packet_hash(&packet, rns_wire::flags::HeaderType::Header1);
        self.link
            .update_pending_request_id(&request_id, packet_request_id);
        send_transport(
            &self.transport_tx,
            TransportMessage::Outbound(OutboundRequest {
                raw: packet,
                destination_hash: self.id(),
            }),
        )
        .await?;
        let link_id = self.link.link_id;
        wait_for_response(
            &self.transport_tx,
            &mut self.event_rx,
            &mut self.link,
            link_id,
            packet_request_id,
            deadline,
            max_response_bytes,
            None,
        )
        .await
    }

    pub fn channel_ready(&self) -> bool {
        self.channel
            .as_ref()
            .is_none_or(LinkChannel::is_ready_to_send)
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
        let link_id = self.id();
        let future = async {
            let mut transfers: HashMap<[u8; 32], InboundTransfer> = HashMap::new();
            let mut segment_info: HashMap<[u8; 32], ([u8; 32], usize, usize)> = HashMap::new();
            let mut multi: Option<MultiSegmentInbound> = None;
            loop {
                let raw = if let Some(raw) = self.pending_resource_packets.pop_front() {
                    raw
                } else {
                    loop {
                        match self.event_rx.recv().await {
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
                        let length = adv.random_hash.len().min(random_hash.len());
                        random_hash[..length].copy_from_slice(&adv.random_hash[..length]);
                        let mut transfer = InboundTransfer::from_advertisement(
                            adv.num_parts,
                            adv.transfer_size,
                            adv.data_size,
                            random_hash,
                            adv.resource_hash,
                            adv.flags,
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
                        if adv.total_segments > 1 && multi.is_none() {
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
                            // Looked up before completing: metadata is only
                            // physically embedded in segment 1's payload, so
                            // `complete()` must know whether this is that
                            // segment before it decides whether to strip it.
                            let (original_hash, segment_index, total_segments) = segment_info
                                .get(&resource_hash)
                                .copied()
                                .unwrap_or((resource_hash, 1, 1));
                            let (data, proof) = transfer
                                .complete(Some(&decrypt), segment_index == 1)
                                .map_err(|error| {
                                    LinkClientError::UnexpectedResponse(format!(
                                        "resource completion: {error:?}"
                                    ))
                                })?;
                            let metadata = transfer.resource.metadata.clone();
                            send_link_proof(&self.transport_tx, link_id, &proof)?;
                            segment_info.remove(&resource_hash);
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
                        if let Some(transfer) = transfers.get_mut(&resource_hash)
                            && let TransferAction::SendRequest(request) =
                                transfer.hashmap_update(segment, &hashmap)
                        {
                            send_link_data(
                                &self.transport_tx,
                                &self.link,
                                link_id,
                                rns_wire::context::PacketContext::ResourceReq,
                                &request,
                                true,
                            )?;
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
        timeout(deadline, future)
            .await
            .map_err(|_| LinkClientError::Timeout("resource"))?
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
        for resource in resources {
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
            while let Some(event) = self.event_rx.recv().await {
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
                        return Err(LinkClientError::Resource(
                            "resource rejected by receiver".into(),
                        ));
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
        timeout(time_remaining(deadline)?, future)
            .await
            .map_err(|_| LinkClientError::Timeout("resource proof"))??;
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
        Ok(())
    }
}

impl Drop for LinkSession {
    fn drop(&mut self) {
        let _ = self
            .transport_tx
            .try_send(TransportMessage::DeregisterDestination { hash: self.id() });
    }
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

    /// Public key from the transport's announce cache, or `None` if this
    /// destination has never been heard from.
    ///
    /// `Recall` is a point lookup into the same cache `GetRecentAnnounces`
    /// dumps in full — see its docs on [`TransportQuery`].
    async fn recall_pubkey(&self, dest_hash: [u8; 16]) -> Option<[u8; 64]> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.transport_tx
            .send(TransportMessage::Rpc {
                query: TransportQuery::Recall {
                    destination_hash: dest_hash,
                },
                response_tx: resp_tx,
            })
            .await
            .ok()?;
        match resp_rx.await.ok()? {
            TransportQueryResponse::Announce(Some(entry)) => entry.public_key,
            _ => None,
        }
    }

    /// Ask the network for a path and wait for the answering announce to
    /// carry the destination's public key.
    ///
    /// Only reached when the announce cache has no entry: an announce is
    /// dispatched to the handlers registered *at the moment it arrives*, so
    /// a query that registers after the answering announce has already been
    /// dispatched waits out its full timeout for an announce nothing will
    /// re-send. Consulting the cache first is what keeps back-to-back
    /// queries to the same destination from alternating success/timeout.
    async fn discover_pubkey(
        &self,
        app_name: &str,
        dest_hash: [u8; 16],
        deadline: Instant,
    ) -> Result<[u8; 64], LinkClientError> {
        // Register the handler before the path request so the answering
        // announce (carrying the pubkey) is observed.
        let (ann_tx, mut ann_rx) = mpsc::channel::<AnnounceHandlerEvent>(64);
        self.send_msg(TransportMessage::RegisterAnnounceHandler {
            aspect_filter: Some(app_name.to_string()),
            receive_path_responses: true,
            callback_tx: ann_tx.clone(),
        })
        .await?;

        // Every exit past this point must deregister. Returning `?` straight
        // out of the path request or the wait -- the timeout case, i.e. the
        // common one -- would leave the registration behind, and the actor
        // only reaps it once the channel closes.
        let outcome = async {
            self.send_msg(TransportMessage::RequestPath {
                destination_hash: dest_hash,
            })
            .await?;
            wait_for_pubkey(&mut ann_rx, dest_hash, time_remaining(deadline)?).await
        }
        .await;

        let _ = self
            .transport_tx
            .try_send(TransportMessage::DeregisterAnnounceHandler {
                callback_tx: ann_tx,
            });
        outcome
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
        self.query_inner(
            remote_transport_hash,
            app_name,
            path,
            payload,
            hops,
            overall_timeout,
            None,
        )
        .await
    }

    /// Same as [`Self::query`], but sends `(bytes_received, total_bytes)` on
    /// `progress` as resource segments arrive. `total_bytes` is only known
    /// once the reply's `ResourceAdv` advertisement is seen, so a small
    /// (inline, non-resource) response never sends anything at all.
    pub async fn query_with_progress(
        &self,
        remote_transport_hash: [u8; 16],
        app_name: &str,
        path: &str,
        payload: Vec<u8>,
        hops: u8,
        overall_timeout: Duration,
        progress: mpsc::UnboundedSender<(usize, usize)>,
    ) -> Result<Vec<u8>, LinkClientError> {
        self.query_inner(
            remote_transport_hash,
            app_name,
            path,
            payload,
            hops,
            overall_timeout,
            Some(progress),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn query_inner(
        &self,
        remote_transport_hash: [u8; 16],
        app_name: &str,
        path: &str,
        payload: Vec<u8>,
        hops: u8,
        overall_timeout: Duration,
        progress: Option<mpsc::UnboundedSender<(usize, usize)>>,
    ) -> Result<Vec<u8>, LinkClientError> {
        let started = Instant::now();
        let deadline = started + overall_timeout;

        let dest_hash =
            Destination::hash_from_name_and_identity(app_name, Some(&remote_transport_hash));

        let pubkey = match self.recall_pubkey(dest_hash).await {
            Some(pubkey) => pubkey,
            None => self.discover_pubkey(app_name, dest_hash, deadline).await?,
        };

        let (mut link, request_data) = Link::new_initiator(dest_hash, hops);
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

        // The destination is registered above; every exit from here on must
        // deregister it. Returning `?` straight out of the handshake -- the
        // timeout path, i.e. what an unreachable or slow node does -- used to
        // skip the teardown entirely, leaking one registration per attempt.
        let response = async {
            let req_pkt = build_link_request_packet(dest_hash, &request_data);
            self.send_msg(TransportMessage::Outbound(OutboundRequest {
                raw: req_pkt,
                destination_hash: dest_hash,
            }))
            .await?;

            let proof_data = wait_for_proof(&mut dest_rx, link_id, time_remaining(deadline)?).await?;

            let identity_ed25519_pub: [u8; 32] = pubkey[32..64].try_into().map_err(|_| {
                LinkClientError::ProofInvalid("remote public key is not 64 bytes".into())
            })?;
            let identity_verify_key = Ed25519PublicKey::from_bytes(&identity_ed25519_pub)
                .map_err(|e| LinkClientError::ProofInvalid(format!("verify key: {e}")))?;

            let rtt_data = link
                .validate_proof(&proof_data, &identity_verify_key, &identity_ed25519_pub)
                .map_err(|e| LinkClientError::ProofInvalid(format!("{e:?}")))?;

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

            wait_for_response(
                &self.transport_tx,
                &mut dest_rx,
                &mut link,
                link_id,
                packet_request_id,
                time_remaining(deadline)?,
                usize::MAX,
                progress.as_ref(),
            )
            .await
            .map(|response| response.data)
        }
        .await;

        // Tear down even on failure so the remote doesn't keep link state.
        let _ = self.send_close(&mut link).await;
        let _ = self
            .transport_tx
            .try_send(TransportMessage::DeregisterDestination { hash: link_id });

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

/// Whether a resource advertised on this link answers our outstanding request.
///
/// A response resource names the request it answers and must name ours.
/// Implementations that reply with a plain application resource instead carry
/// no correlation id at all -- but the link exists solely for the one request
/// still outstanding, so such a resource is that request's answer.
fn resource_answers_request(
    is_response: bool,
    adv_request_id: Option<&[u8]>,
    request_id: &[u8; 16],
) -> bool {
    if is_response {
        adv_request_id == Some(request_id.as_slice())
    } else {
        adv_request_id.is_none()
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
    rx: &mut mpsc::Receiver<DestinationEvent>,
    link_id: [u8; 16],
    deadline: Duration,
) -> Result<Vec<u8>, LinkClientError> {
    let fut = async {
        while let Some(ev) = rx.recv().await {
            match ev {
                DestinationEvent::LinkClosed { link_id: closed_id } if closed_id == link_id => {
                    return Err(LinkClientError::HandshakeFailed("link closed".into()));
                }
                DestinationEvent::InboundPacket { raw, .. } => {
                    let (header, data_offset) = match rns_wire::header::PacketHeader::unpack(&raw) {
                        Ok(h) => h,
                        Err(_) => continue,
                    };
                    let is_proof = header.flags.packet_type == rns_wire::flags::PacketType::Proof
                        && header.destination_hash == link_id;
                    if is_proof && raw.len() > data_offset {
                        return Ok(raw[data_offset..].to_vec());
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

#[allow(clippy::too_many_arguments)]
async fn wait_for_response(
    transport_tx: &mpsc::Sender<TransportMessage>,
    rx: &mut mpsc::Receiver<DestinationEvent>,
    link: &mut Link,
    link_id: [u8; 16],
    request_id: [u8; 16],
    deadline: Duration,
    max_response_bytes: usize,
    progress: Option<&mpsc::UnboundedSender<(usize, usize)>>,
) -> Result<LinkResponse, LinkClientError> {
    // Throttled so a multi-thousand-packet transfer doesn't flood the
    // receiver with one update per wire packet (each ~350-500 bytes).
    const PROGRESS_MIN_INTERVAL: Duration = Duration::from_millis(150);
    let mut bytes_received = 0usize;
    let mut last_progress_emit: Option<Instant> = None;

    let fut = async {
        let mut inbound_resources: HashMap<[u8; 32], InboundTransfer> = HashMap::new();
        let mut segment_info: HashMap<[u8; 32], ([u8; 32], usize, usize)> = HashMap::new();
        let mut multi: Option<MultiSegmentInbound> = None;
        let mut advertised_bytes = 0usize;

        while let Some(ev) = rx.recv().await {
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

                            // A response resource names the request it answers,
                            // and must name ours. Implementations that reply
                            // with a plain application resource instead (no
                            // response flag, no request id) carry no way to
                            // correlate -- but this link exists solely for the
                            // one request still outstanding, so a resource
                            // arriving on it is that request's answer.
                            if !resource_answers_request(
                                adv.flags.is_response,
                                adv.request_id.as_deref(),
                                &request_id,
                            ) {
                                continue;
                            }
                            if !inbound_resources.contains_key(&adv.resource_hash) {
                                advertised_bytes = advertised_bytes.saturating_add(adv.data_size);
                                if advertised_bytes > max_response_bytes {
                                    return Err(LinkClientError::UnexpectedResponse(
                                        "resource response exceeds application size limit".into(),
                                    ));
                                }
                            }

                            let mut random_hash = [0u8; rns_protocol::resource::RANDOM_HASH_SIZE];
                            let copy_len = adv.random_hash.len().min(random_hash.len());
                            random_hash[..copy_len].copy_from_slice(&adv.random_hash[..copy_len]);

                            let rtt = link.rtt.unwrap_or(Duration::from_millis(500));
                            let mut transfer = InboundTransfer::from_advertisement(
                                adv.num_parts,
                                adv.transfer_size,
                                adv.data_size,
                                random_hash,
                                adv.resource_hash,
                                adv.flags,
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

                            // Approximate, not exact: counts the wire packet
                            // once per arrival rather than crediting whichever
                            // candidate transfer actually consumed it, which is
                            // indistinguishable from here without changing
                            // InboundTransfer's return type. Close enough for a
                            // progress bar; a query has one outstanding
                            // resource in the overwhelming majority of cases.
                            if let Some(tx) = progress {
                                bytes_received = bytes_received.saturating_add(body.len());
                                let now = Instant::now();
                                let due = last_progress_emit
                                    .is_none_or(|last| now - last >= PROGRESS_MIN_INTERVAL);
                                if due && advertised_bytes > 0 {
                                    last_progress_emit = Some(now);
                                    let _ = tx.send((bytes_received.min(advertised_bytes), advertised_bytes));
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
                                // Looked up before completing, same reason as
                                // the other resource-response path above:
                                // metadata is only embedded in segment 1's
                                // payload, so `complete()` needs to know
                                // whether this is that segment first.
                                let is_first_segment = segment_info
                                    .get(&rh)
                                    .map(|&(_, segment_index, _)| segment_index == 1)
                                    .unwrap_or(true);
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
                                    let (assembled, proof) = transfer
                                        .complete(Some(&decrypt_fn), is_first_segment)
                                        .map_err(|e| {
                                            LinkClientError::UnexpectedResponse(format!(
                                                "resource assemble: {e:?}"
                                            ))
                                        })?;
                                    (assembled, proof, transfer.resource.metadata.clone())
                                };

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
                                match link.handle_response_plaintext(&response_payload) {
                                    Ok((id, response_data)) => {
                                        if id == request_id {
                                            if let Some(tx) = progress {
                                                let _ = tx.send((advertised_bytes, advertised_bytes));
                                            }
                                            return Ok(LinkResponse {
                                                data: response_data,
                                                metadata,
                                            });
                                        }
                                    }
                                    // Not the msgpack `[request_id, data]`
                                    // envelope Python RNS wraps large responses
                                    // in. Implementations that send the payload
                                    // raw are answering the same request, so the
                                    // bytes are the response as they stand --
                                    // failing here would reject every such peer.
                                    Err(_) => {
                                        if let Some(tx) = progress {
                                            let _ = tx.send((advertised_bytes, advertised_bytes));
                                        }
                                        return Ok(LinkResponse {
                                            data: response_payload,
                                            metadata,
                                        });
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
                                }
                                _ => {}
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
    timeout(deadline, fut)
        .await
        .map_err(|_| LinkClientError::Timeout("response"))?
}

fn send_link_data(
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

fn send_link_proof(
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
    let mut raw = header.pack();
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
    let mut raw = header.pack();
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
    let mut raw = header.pack();
    raw.extend_from_slice(body);
    Bytes::from(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rns_transport::messages::AnnounceRpcEntry;

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

    /// An announce is delivered only to the handlers registered when it
    /// arrives, so a query that waits for a fresh one after the answering
    /// announce has already been dispatched hangs until its own timeout.
    /// A destination already in the announce cache must not wait at all.
    #[tokio::test]
    async fn query_uses_cached_pubkey_without_waiting_for_an_announce() {
        let app_name = "nomadnetwork.node";
        let remote = [0xAB; 16];
        let dest_hash = Destination::hash_from_name_and_identity(app_name, Some(&remote));

        let (transport_tx, mut transport_rx) = mpsc::channel(32);
        let transport = tokio::spawn(async move {
            let mut seen = Vec::new();
            while let Some(msg) = transport_rx.recv().await {
                seen.push(rns_transport::messages::msg_variant_name(&msg));
                if let TransportMessage::Rpc { response_tx, .. } = msg {
                    let _ = response_tx.send(TransportQueryResponse::Announce(Some(
                        AnnounceRpcEntry {
                            dest_hash,
                            hops: 1,
                            app_data: None,
                            timestamp: 0.0,
                            public_key: Some([0x11; 64]),
                            ratchet: None,
                            name_hash: [0; 10],
                            is_path_response: false,
                            retained: false,
                        },
                    )));
                }
            }
            seen
        });

        // Fails at the handshake (nothing answers the link request); the
        // pubkey stage is what's under test.
        let _ = LinkClient::new(transport_tx, Identity::new())
            .query(
                remote,
                app_name,
                "/page/index.mu",
                Vec::new(),
                1,
                Duration::from_millis(250),
            )
            .await;

        let seen = transport.await.unwrap();
        assert!(
            !seen.contains(&"RegisterAnnounceHandler"),
            "cached pubkey should skip announce discovery, saw: {seen:?}"
        );
        assert!(
            !seen.contains(&"RequestPath"),
            "cached pubkey should skip the path request, saw: {seen:?}"
        );
        // It got far enough to attempt the link, i.e. it really used the key.
        assert!(seen.contains(&"RegisterDestination"), "saw: {seen:?}");
    }

    /// The timeout path is the common one for an unreachable destination,
    /// and it used to return without deregistering -- leaving a live
    /// registration the actor only reaps once the channel happens to close.
    #[tokio::test]
    async fn query_deregisters_its_announce_handler_when_discovery_times_out() {
        let (transport_tx, mut transport_rx) = mpsc::channel(32);
        let transport = tokio::spawn(async move {
            let mut seen = Vec::new();
            while let Some(msg) = transport_rx.recv().await {
                seen.push(rns_transport::messages::msg_variant_name(&msg));
                // Never heard of it, and no announce will ever arrive.
                if let TransportMessage::Rpc { response_tx, .. } = msg {
                    let _ = response_tx.send(TransportQueryResponse::Announce(None));
                }
            }
            seen
        });

        let result = LinkClient::new(transport_tx, Identity::new())
            .query(
                [0xAB; 16],
                "nomadnetwork.node",
                "/page/index.mu",
                Vec::new(),
                1,
                Duration::from_millis(150),
            )
            .await;

        assert!(matches!(result, Err(LinkClientError::Timeout(_))));
        let seen = transport.await.unwrap();
        assert!(seen.contains(&"RegisterAnnounceHandler"), "saw: {seen:?}");
        assert!(
            seen.contains(&"DeregisterAnnounceHandler"),
            "handler leaked on the timeout path, saw: {seen:?}"
        );
    }

    /// Peers that answer with a plain application resource (nomad-core does)
    /// set no response flag and no request id. Requiring both meant the
    /// advertisement was skipped, no ResourceReq was ever sent, and the link
    /// sat idle until it was torn down as stale.
    #[test]
    fn plain_resource_without_a_request_id_answers_the_outstanding_request() {
        let ours = [0x11u8; 16];
        let theirs = [0x22u8; 16];

        // Enveloped response resources still must name our request.
        assert!(resource_answers_request(true, Some(&ours), &ours));
        assert!(!resource_answers_request(true, Some(&theirs), &ours));
        assert!(!resource_answers_request(true, None, &ours));

        // A plain resource carries no id and is taken as the answer.
        assert!(resource_answers_request(false, None, &ours));
        // ...but one that names a *different* request is not ours.
        assert!(!resource_answers_request(false, Some(&theirs), &ours));
    }

    /// Every failed navigation used to leave a destination registered in the
    /// transport actor: RegisterDestination happens before the handshake, and
    /// every `?` between there and the teardown skipped the cleanup. A browser
    /// doing repeated fetches leaks one per failure.
    #[tokio::test]
    async fn failed_query_still_deregisters_its_destination() {
        let (transport_tx, mut transport_rx) = mpsc::channel(4096);
        let transport = tokio::spawn(async move {
            let (mut reg, mut dereg) = (0usize, 0usize);
            while let Some(msg) = transport_rx.recv().await {
                match msg {
                    TransportMessage::RegisterDestination { .. } => reg += 1,
                    TransportMessage::DeregisterDestination { .. } => dereg += 1,
                    TransportMessage::Rpc { response_tx, .. } => {
                        // Known peer, so discovery is skipped and the query
                        // proceeds straight to the handshake, which nothing
                        // answers -- the common slow/unreachable-node path.
                        let _ = response_tx.send(TransportQueryResponse::Announce(Some(
                            AnnounceRpcEntry {
                                dest_hash: [0; 16],
                                hops: 1,
                                app_data: None,
                                timestamp: 0.0,
                                public_key: Some([0x11; 64]),
                                ratchet: None,
                                name_hash: [0; 10],
                                is_path_response: false,
                                retained: false,
                            },
                        )));
                    }
                    _ => {}
                }
            }
            (reg, dereg)
        });

        let client = LinkClient::new(transport_tx, Identity::new());
        const NAVIGATIONS: usize = 30;
        for _ in 0..NAVIGATIONS {
            let _ = client
                .query(
                    [0xAB; 16],
                    "nomadnetwork.node",
                    "/page/index.mu",
                    Vec::new(),
                    1,
                    Duration::from_millis(20),
                )
                .await;
        }
        drop(client);

        let (reg, dereg) = transport.await.unwrap();
        assert_eq!(reg, NAVIGATIONS, "expected one registration per navigation");
        assert_eq!(
            dereg, reg,
            "leaked {} destination registrations over {NAVIGATIONS} failed navigations",
            reg - dereg
        );
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
                let mut raw = header.pack();
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
