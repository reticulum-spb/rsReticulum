//! Application-facing inbound Link listener built on [`crate::link_manager`].

use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use rns_identity::identity::Identity;
use rns_protocol::channel_message::MessageBase;

use crate::link_manager::{
    ChannelSendError, ChannelSendReceipt, LinkChannelMessage, LinkManager, LinkManagerCommand,
    LinkPayloadSendReceipt, LinkSendError, RequestOutcome, ResourceCompletion,
    register_destination,
};
use crate::reticulum::ReticulumHandle;

#[derive(Debug, thiserror::Error)]
pub enum LinkListenerError {
    #[error("identity has no signing key")]
    NoSigningKey,
    #[error("link manager stopped")]
    ManagerStopped,
    #[error("send: {0}")]
    Send(#[from] LinkSendError),
    #[error("channel: {0}")]
    Channel(#[from] ChannelSendError),
}

/// Events exposed by an inbound Link listener.
#[derive(Debug)]
pub enum LinkListenerEvent {
    Established {
        link_id: [u8; 16],
    },
    Identified {
        link_id: [u8; 16],
        identity_hash: [u8; 16],
    },
    Packet {
        link_id: [u8; 16],
        data: Vec<u8>,
    },
    Resource(ResourceCompletion),
    Channel(LinkChannelMessage),
    Closed {
        link_id: [u8; 16],
    },
}

/// A network-backed inbound SINGLE destination accepting Links.
pub struct LinkListener {
    destination_hash: [u8; 16],
    command_tx: mpsc::Sender<LinkManagerCommand>,
    event_rx: mpsc::Receiver<LinkListenerEvent>,
}

impl LinkListener {
    pub async fn listen(
        runtime: &ReticulumHandle,
        identity: &Identity,
        app_name: &str,
    ) -> Result<Self, LinkListenerError> {
        Self::listen_with_request_handler(runtime, identity, app_name, None::<fn(_, _, _) -> _>)
            .await
    }

    pub async fn listen_with_request_handler<F>(
        runtime: &ReticulumHandle,
        identity: &Identity,
        app_name: &str,
        handler: Option<F>,
    ) -> Result<Self, LinkListenerError>
    where
        F: Fn([u8; 16], [u8; 16], Vec<u8>) -> RequestOutcome + Send + 'static,
    {
        let signing_key = identity
            .get_signing_key()
            .ok_or(LinkListenerError::NoSigningKey)?;
        let destination_hash = rns_identity::destination::Destination::hash_from_name_and_identity(
            app_name,
            Some(&identity.hash),
        );
        let destination_rx =
            register_destination(&runtime.transport_tx, destination_hash, app_name);
        let mut manager = LinkManager::with_destination(
            runtime.transport_tx.clone(),
            destination_rx,
            identity,
            app_name,
            Some(signing_key),
        );
        if let Some(handler) = handler {
            manager.set_request_handler_ex(handler);
        }

        let (established_tx, mut established_rx) = mpsc::channel(64);
        let (identified_tx, mut identified_rx) = mpsc::channel(64);
        let (packet_tx, mut packet_rx) = mpsc::channel(256);
        let (resource_tx, mut resource_rx) = mpsc::channel(64);
        let (channel_tx, mut channel_rx) = mpsc::channel(256);
        let (closed_tx, mut closed_rx) = mpsc::channel(64);
        manager.set_link_established_channel(established_tx);
        manager.set_link_identified_channel(identified_tx);
        manager.set_link_packet_channel(packet_tx);
        manager.set_resource_completion_channel(resource_tx);
        manager.set_channel_message_channel(channel_tx);
        manager.set_link_closed_channel(closed_tx);

        let (command_tx, command_rx) = mpsc::channel(256);
        let manager_shutdown = runtime.shutdown.clone();
        let drain_coordinator = runtime.drain_coordinator.clone();
        tokio::spawn(
            drain_coordinator.run_registered(manager.run_with_commands_until_shutdown(
                command_rx,
                manager_shutdown,
                crate::lifecycle::LINK_MANAGER_DRAIN_GRACE,
            )),
        );

        let (event_tx, event_rx) = mpsc::channel(256);
        tokio::spawn(async move {
            loop {
                let event = tokio::select! {
                    value = established_rx.recv() => value.map(|link_id| LinkListenerEvent::Established { link_id }),
                    value = identified_rx.recv() => value.map(|(link_id, identity_hash)| LinkListenerEvent::Identified { link_id, identity_hash }),
                    value = packet_rx.recv() => value.map(|(data, link_id)| LinkListenerEvent::Packet { link_id, data }),
                    value = resource_rx.recv() => value.map(LinkListenerEvent::Resource),
                    value = channel_rx.recv() => value.map(LinkListenerEvent::Channel),
                    value = closed_rx.recv() => value.map(|link_id| LinkListenerEvent::Closed { link_id }),
                };
                let Some(event) = event else { break };
                if event_tx.send(event).await.is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            destination_hash,
            command_tx,
            event_rx,
        })
    }

    pub fn destination_hash(&self) -> [u8; 16] {
        self.destination_hash
    }

    pub async fn announce(&self) -> Result<(), LinkListenerError> {
        self.announce_with_app_data(None).await
    }

    pub async fn announce_with_app_data(
        &self,
        app_data: Option<&[u8]>,
    ) -> Result<(), LinkListenerError> {
        self.command_tx
            .send(LinkManagerCommand::Announce {
                app_data: app_data.map(<[u8]>::to_vec),
            })
            .await
            .map_err(|_| LinkListenerError::ManagerStopped)
    }

    pub async fn next(&mut self) -> Option<LinkListenerEvent> {
        self.event_rx.recv().await
    }

    pub async fn send(
        &self,
        link_id: [u8; 16],
        data: Vec<u8>,
    ) -> Result<LinkPayloadSendReceipt, LinkListenerError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(LinkManagerCommand::SendLinkPayload {
                link_id,
                payload: data,
                auto_compress: true,
                result_tx: Some(tx),
            })
            .await
            .map_err(|_| LinkListenerError::ManagerStopped)?;
        Ok(rx.await.map_err(|_| LinkListenerError::ManagerStopped)??)
    }

    pub async fn send_channel(
        &self,
        link_id: [u8; 16],
        message: Box<dyn MessageBase>,
    ) -> Result<ChannelSendReceipt, LinkListenerError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(LinkManagerCommand::SendChannelMessage {
                link_id,
                message,
                result_tx: Some(tx),
            })
            .await
            .map_err(|_| LinkListenerError::ManagerStopped)?;
        Ok(rx.await.map_err(|_| LinkListenerError::ManagerStopped)??)
    }

    pub async fn close(&self, link_id: [u8; 16]) -> Result<(), LinkListenerError> {
        self.command_tx
            .send(LinkManagerCommand::CloseLink {
                link_id,
                reason: rns_link::link::CloseReason::DestinationClosed,
                send_teardown: true,
            })
            .await
            .map_err(|_| LinkListenerError::ManagerStopped)
    }
}

impl Drop for LinkListener {
    fn drop(&mut self) {
        let _ = self.command_tx.try_send(LinkManagerCommand::Shutdown);
    }
}

/// Default timeout used by the Python Link examples.
pub const DEFAULT_LINK_TIMEOUT: Duration = Duration::from_secs(30);
