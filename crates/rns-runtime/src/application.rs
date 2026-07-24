//! Application-facing network orchestration corresponding to Python's
//! `Destination`, `Packet` and `Transport` APIs.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use thiserror::Error;
use tokio::sync::mpsc;

use rns_identity::destination::{
    DestType, Destination, DestinationError, Direction, ProofStrategy,
};
use rns_identity::identity::{Identity, IdentityError};
use rns_transport::link_messages::DestinationEvent;
use rns_transport::messages::{
    AnnounceHandlerEvent, OutboundRequest, TransportMessage, TransportQuery, TransportQueryResponse,
};
use rns_wire::constants::MTU;
use rns_wire::context::PacketContext;
use rns_wire::flags::{DestinationType, HeaderType, PacketFlags, PacketType, TransportType};
use rns_wire::hash::packet_hash_pair;
use rns_wire::header::PacketHeader;

use crate::reticulum::ReticulumHandle;

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("destination: {0}")]
    Destination(#[from] DestinationError),
    #[error("identity: {0}")]
    Identity(#[from] IdentityError),
    #[error("transport channel closed")]
    TransportClosed,
    #[error("transport channel full")]
    TransportFull,
    #[error("destination identity is not known")]
    UnknownIdentity,
    #[error("packet size {size} exceeds MTU {mtu}")]
    MtuExceeded { size: usize, mtu: usize },
    #[error("invalid inbound packet")]
    InvalidPacket,
    #[error("packet decryption failed")]
    DecryptionFailed,
    #[error("network operation timed out")]
    Timeout,
}

#[derive(Debug, Clone)]
pub struct InboundPacket {
    pub data: Vec<u8>,
    pub raw: Vec<u8>,
    pub interface_id: u64,
    pub packet_hash: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct PacketReceipt {
    pub packet_hash: [u8; 32],
    pub rtt: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketSubmission {
    pub packet_hash: [u8; 32],
    pub truncated_hash: [u8; 16],
}

/// Build an announce packet for an application destination without registering
/// it with the runtime.
///
/// This is useful for applications that already own destination registration
/// through a Link manager but still need Reticulum's canonical announce wire
/// format.
pub fn build_announce_packet(
    identity: &Identity,
    app_name: &str,
    app_data: Option<&[u8]>,
    ratchet: Option<&[u8; 32]>,
    path_response: bool,
    tag: Option<&[u8]>,
) -> Result<([u8; 16], Vec<u8>), ApplicationError> {
    let mut destination =
        Destination::new(Some(identity), Direction::In, DestType::Single, app_name)?;
    let hash = destination.hash;
    let packet =
        destination.announce_packet(identity, app_data, ratchet, path_response, tag, now())?;
    Ok((hash, packet))
}

/// A Destination registered with the running Reticulum transport.
pub struct RegisteredDestination {
    runtime: ReticulumHandle,
    destination: Destination,
    identity: Option<Arc<Identity>>,
    event_rx: mpsc::Receiver<DestinationEvent>,
}

impl RegisteredDestination {
    pub async fn single(
        runtime: ReticulumHandle,
        identity: Identity,
        app_name: &str,
    ) -> Result<Self, ApplicationError> {
        Self::register(runtime, Some(identity), DestType::Single, app_name).await
    }

    pub async fn plain(runtime: ReticulumHandle, app_name: &str) -> Result<Self, ApplicationError> {
        Self::register(runtime, None, DestType::Plain, app_name).await
    }

    async fn register(
        runtime: ReticulumHandle,
        identity: Option<Identity>,
        dest_type: DestType,
        app_name: &str,
    ) -> Result<Self, ApplicationError> {
        let identity = identity.map(Arc::new);
        let destination =
            Destination::new(identity.as_deref(), Direction::In, dest_type, app_name)?;
        let (event_tx, event_rx) = mpsc::channel(256);
        runtime
            .transport_tx
            .send(TransportMessage::RegisterDestination {
                hash: destination.hash,
                app_name: app_name.to_string(),
                delivery_tx: Some(event_tx),
            })
            .await
            .map_err(|_| ApplicationError::TransportClosed)?;
        Ok(Self {
            runtime,
            destination,
            identity,
            event_rx,
        })
    }

    pub fn hash(&self) -> [u8; 16] {
        self.destination.hash
    }

    pub fn hex_hash(&self) -> String {
        self.destination.hex_hash()
    }

    pub fn identity(&self) -> Option<&Identity> {
        self.identity.as_deref()
    }

    pub fn set_proof_strategy(&mut self, strategy: ProofStrategy) {
        self.destination.set_proof_strategy(strategy);
    }

    pub fn enable_ratchets(&mut self, enforce: bool) {
        self.destination.enable_ratchets(enforce);
    }

    /// Broadcast an announce through the network.
    pub async fn announce(&mut self, app_data: Option<&[u8]>) -> Result<(), ApplicationError> {
        let identity = self
            .identity
            .as_deref()
            .ok_or(ApplicationError::UnknownIdentity)?;
        let ratchet = self.destination.get_ratchet_for_announce();
        let raw = self.destination.announce_packet(
            identity,
            app_data,
            ratchet.as_ref(),
            false,
            None,
            now(),
        )?;
        self.send_raw(raw, self.hash(), None).await
    }

    /// Receive the next DATA packet, automatically servicing announce
    /// requests and PROVE_ALL receipts while waiting.
    pub async fn recv(&mut self) -> Result<InboundPacket, ApplicationError> {
        loop {
            let event = self
                .event_rx
                .recv()
                .await
                .ok_or(ApplicationError::TransportClosed)?;
            match event {
                DestinationEvent::AnnounceRequested(request) => {
                    let Some(identity) = self.identity.as_deref() else {
                        continue;
                    };
                    let ratchet = self.destination.get_ratchet_for_announce();
                    let raw = self.destination.announce_packet(
                        identity,
                        None,
                        ratchet.as_ref(),
                        request.path_response,
                        request.tag.as_deref(),
                        now(),
                    )?;
                    self.send_raw(raw, self.hash(), request.attached_interface)
                        .await?;
                }
                DestinationEvent::InboundPacket { raw, interface_id } => {
                    let (header, offset) =
                        PacketHeader::unpack(&raw).map_err(|_| ApplicationError::InvalidPacket)?;
                    if header.flags.packet_type != PacketType::Data {
                        continue;
                    }
                    let data = match self.destination.dest_type {
                        DestType::Plain => raw[offset..].to_vec(),
                        DestType::Single => {
                            let identity = self
                                .identity
                                .as_deref()
                                .ok_or(ApplicationError::UnknownIdentity)?;
                            self.destination
                                .decrypt(&raw[offset..], identity)
                                .map_err(|_| ApplicationError::DecryptionFailed)?
                        }
                        _ => continue,
                    };
                    let (packet_hash, _) = packet_hash_pair(&raw, header.flags.header_type);
                    if self.destination.should_prove(&data)
                        && let Some(identity) = self.identity.as_deref()
                    {
                        self.send_proof(identity, packet_hash, interface_id).await?;
                    }
                    return Ok(InboundPacket {
                        data,
                        raw: raw.to_vec(),
                        interface_id,
                        packet_hash,
                    });
                }
                _ => {}
            }
        }
    }

    async fn send_proof(
        &self,
        identity: &Identity,
        packet_hash: [u8; 32],
        interface_id: u64,
    ) -> Result<(), ApplicationError> {
        let proof = identity.prove(&packet_hash, true)?;
        let destination_hash = proof_destination_hash(&packet_hash);
        let header = PacketHeader {
            flags: PacketFlags {
                header_type: HeaderType::Header1,
                context_flag: false,
                transport_type: TransportType::Broadcast,
                destination_type: DestinationType::Single,
                packet_type: PacketType::Proof,
            },
            hops: 0,
            transport_id: None,
            destination_hash,
            context: PacketContext::None,
        };
        let mut raw = header.pack();
        raw.extend_from_slice(&proof);
        self.send_raw(raw, destination_hash, Some(interface_id))
            .await
    }

    async fn send_raw(
        &self,
        raw: Vec<u8>,
        destination_hash: [u8; 16],
        interface_id: Option<u64>,
    ) -> Result<(), ApplicationError> {
        let request = OutboundRequest {
            raw: Bytes::from(raw),
            destination_hash,
        };
        let message = match interface_id {
            Some(interface_id) => TransportMessage::OutboundAttached {
                request,
                interface_id,
            },
            None => TransportMessage::Outbound(request),
        };
        self.runtime
            .transport_tx
            .send(message)
            .await
            .map_err(|_| ApplicationError::TransportClosed)
    }
}

impl Drop for RegisteredDestination {
    fn drop(&mut self) {
        let _ = self
            .runtime
            .transport_tx
            .try_send(TransportMessage::DeregisterDestination { hash: self.hash() });
    }
}

pub async fn await_path(
    runtime: &ReticulumHandle,
    destination_hash: [u8; 16],
    timeout: Duration,
) -> Result<(), ApplicationError> {
    runtime
        .transport_tx
        .send(TransportMessage::RequestPath { destination_hash })
        .await
        .map_err(|_| ApplicationError::TransportClosed)?;
    runtime
        .await_path(destination_hash, timeout)
        .await
        .map_err(|_| ApplicationError::Timeout)
}

pub async fn recall_identity(
    runtime: &ReticulumHandle,
    destination_hash: [u8; 16],
) -> Result<Identity, ApplicationError> {
    let response = runtime
        .query_control(TransportQuery::Recall { destination_hash })
        .await;
    let Some(TransportQueryResponse::Announce(Some(entry))) = response else {
        return Err(ApplicationError::UnknownIdentity);
    };
    let public_key = entry.public_key.ok_or(ApplicationError::UnknownIdentity)?;
    Ok(Identity::from_public_key(&public_key)?)
}

pub async fn announce_stream(
    runtime: &ReticulumHandle,
    aspect_filter: Option<&str>,
) -> Result<mpsc::Receiver<AnnounceHandlerEvent>, ApplicationError> {
    let (tx, rx) = mpsc::channel(256);
    runtime
        .transport_tx
        .send(TransportMessage::RegisterAnnounceHandler {
            aspect_filter: aspect_filter.map(str::to_string),
            receive_path_responses: false,
            callback_tx: tx,
        })
        .await
        .map_err(|_| ApplicationError::TransportClosed)?;
    Ok(rx)
}

/// Send a PLAIN or SINGLE DATA packet, optionally awaiting its proof.
pub async fn send_packet(
    runtime: &ReticulumHandle,
    destination_hash: [u8; 16],
    app_name: &str,
    dest_type: DestType,
    data: &[u8],
    timeout: Option<Duration>,
) -> Result<Option<PacketReceipt>, ApplicationError> {
    let payload = match dest_type {
        DestType::Plain => data.to_vec(),
        DestType::Single => {
            let identity = recall_identity(runtime, destination_hash).await?;
            let destination =
                Destination::new(Some(&identity), Direction::Out, DestType::Single, app_name)?;
            destination.encrypt(data, &identity, None)?
        }
        _ => return Err(ApplicationError::InvalidPacket),
    };
    let destination_type = match dest_type {
        DestType::Plain => DestinationType::Plain,
        DestType::Single => DestinationType::Single,
        _ => return Err(ApplicationError::InvalidPacket),
    };
    let header = PacketHeader {
        flags: PacketFlags {
            header_type: HeaderType::Header1,
            context_flag: false,
            transport_type: TransportType::Broadcast,
            destination_type,
            packet_type: PacketType::Data,
        },
        hops: 0,
        transport_id: None,
        destination_hash,
        context: PacketContext::None,
    };
    let mut raw = header.pack();
    raw.extend_from_slice(&payload);
    if raw.len() > MTU {
        return Err(ApplicationError::MtuExceeded {
            size: raw.len(),
            mtu: MTU,
        });
    }
    let (full_hash, truncated_hash) = packet_hash_pair(&raw, HeaderType::Header1);
    let started = Instant::now();
    let msg_id = hex::encode(full_hash);

    let proof_rx = if let Some(timeout) = timeout {
        let listener = Destination::new(None, Direction::In, DestType::Single, "packet.receipt")?;
        let (tx, rx) = mpsc::channel(16);
        runtime
            .transport_tx
            .send(TransportMessage::RegisterDestination {
                hash: listener.hash,
                app_name: "packet.receipt".into(),
                delivery_tx: Some(tx),
            })
            .await
            .map_err(|_| ApplicationError::TransportClosed)?;
        runtime
            .transport_tx
            .send(TransportMessage::RegisterReceipt {
                truncated_hash,
                full_hash,
                msg_id: msg_id.clone(),
                timeout: Some(timeout),
            })
            .await
            .map_err(|_| ApplicationError::TransportClosed)?;
        Some((listener.hash, rx, timeout))
    } else {
        None
    };

    runtime
        .transport_tx
        .send(TransportMessage::Outbound(OutboundRequest {
            raw: Bytes::from(raw),
            destination_hash,
        }))
        .await
        .map_err(|_| ApplicationError::TransportClosed)?;

    let Some((listener_hash, mut rx, timeout)) = proof_rx else {
        return Ok(None);
    };
    let result = tokio::time::timeout(timeout, async {
        while let Some(event) = rx.recv().await {
            if let DestinationEvent::DeliveryProof {
                msg_id: received,
                rtt,
            } = event
                && received == msg_id
            {
                return Some(rtt.unwrap_or_else(|| started.elapsed()));
            }
        }
        None
    })
    .await
    .ok()
    .flatten();
    let _ = runtime
        .transport_tx
        .send(TransportMessage::DeregisterDestination {
            hash: listener_hash,
        })
        .await;
    result
        .map(|rtt| {
            Some(PacketReceipt {
                packet_hash: full_hash,
                rtt,
            })
        })
        .ok_or(ApplicationError::Timeout)
}

/// Send an application-prepared DATA payload without applying destination
/// encryption a second time.
///
/// This is intended for protocols such as LXMF that define their own
/// destination-encrypted payload representation while still delegating packet
/// framing, MTU validation and transport submission to Reticulum.
pub async fn send_pre_encrypted_packet(
    runtime: &ReticulumHandle,
    destination_hash: [u8; 16],
    payload: &[u8],
) -> Result<PacketSubmission, ApplicationError> {
    let (raw, submission) = build_pre_encrypted_packet(destination_hash, payload)?;
    runtime
        .transport_tx
        .send(TransportMessage::Outbound(OutboundRequest {
            raw,
            destination_hash,
        }))
        .await
        .map_err(|_| ApplicationError::TransportClosed)?;
    Ok(submission)
}

/// Non-blocking variant of [`send_pre_encrypted_packet`] for synchronous
/// application state machines.
pub fn try_send_pre_encrypted_packet(
    runtime: &ReticulumHandle,
    destination_hash: [u8; 16],
    payload: &[u8],
) -> Result<PacketSubmission, ApplicationError> {
    let (raw, submission) = build_pre_encrypted_packet(destination_hash, payload)?;
    runtime
        .transport_tx
        .try_send(TransportMessage::Outbound(OutboundRequest {
            raw,
            destination_hash,
        }))
        .map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => ApplicationError::TransportFull,
            mpsc::error::TrySendError::Closed(_) => ApplicationError::TransportClosed,
        })?;
    Ok(submission)
}

fn build_pre_encrypted_packet(
    destination_hash: [u8; 16],
    payload: &[u8],
) -> Result<(Bytes, PacketSubmission), ApplicationError> {
    let header = PacketHeader {
        flags: PacketFlags {
            header_type: HeaderType::Header1,
            context_flag: false,
            transport_type: TransportType::Broadcast,
            destination_type: DestinationType::Single,
            packet_type: PacketType::Data,
        },
        hops: 0,
        transport_id: None,
        destination_hash,
        context: PacketContext::None,
    };
    let mut raw = header.pack();
    raw.extend_from_slice(payload);
    if raw.len() > MTU {
        return Err(ApplicationError::MtuExceeded {
            size: raw.len(),
            mtu: MTU,
        });
    }
    let (packet_hash, truncated_hash) = packet_hash_pair(&raw, HeaderType::Header1);
    Ok((
        Bytes::from(raw),
        PacketSubmission {
            packet_hash,
            truncated_hash,
        },
    ))
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn proof_destination_hash(packet_hash: &[u8; 32]) -> [u8; 16] {
    let mut destination_hash = [0u8; 16];
    destination_hash.copy_from_slice(&packet_hash[..16]);
    destination_hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_encrypted_packet_uses_single_data_framing() {
        let destination_hash = [0xAC; 16];
        let (raw, submission) =
            build_pre_encrypted_packet(destination_hash, b"LXMF ciphertext").unwrap();
        let (header, offset) = PacketHeader::unpack(&raw).unwrap();

        assert_eq!(header.destination_hash, destination_hash);
        assert_eq!(header.flags.destination_type, DestinationType::Single);
        assert_eq!(header.flags.packet_type, PacketType::Data);
        assert_eq!(&raw[offset..], b"LXMF ciphertext");
        assert_eq!(
            packet_hash_pair(&raw, HeaderType::Header1),
            (submission.packet_hash, submission.truncated_hash)
        );
    }

    #[test]
    fn proof_is_addressed_to_truncated_packet_hash() {
        let raw = [
            0x00, 0x00, // flags, hops
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, //
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, // destination
            0x00, // context
            0xAA, 0xBB, 0xCC, // payload
        ];
        let (full_hash, truncated_hash) = packet_hash_pair(&raw, HeaderType::Header1);

        assert_eq!(proof_destination_hash(&full_hash), truncated_hash);
        assert_ne!(
            proof_destination_hash(&full_hash),
            rns_crypto::sha::truncated_hash(&full_hash),
            "proof destination must not hash the packet hash a second time"
        );
    }

    #[test]
    fn standalone_announce_uses_canonical_destination_hash() {
        let identity = Identity::new();
        let (hash, raw) = build_announce_packet(
            &identity,
            "example.service",
            Some(b"hello"),
            None,
            false,
            None,
        )
        .unwrap();
        let destination = Destination::new(
            Some(&identity),
            Direction::In,
            DestType::Single,
            "example.service",
        )
        .unwrap();
        let (header, _) = PacketHeader::unpack(&raw).unwrap();

        assert_eq!(hash, destination.hash);
        assert_eq!(header.destination_hash, destination.hash);
        assert_eq!(header.flags.packet_type, PacketType::Announce);
    }
}
