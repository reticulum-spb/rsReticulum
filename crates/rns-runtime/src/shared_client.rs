//! Connection lifecycle shared by full and strict-client runtimes.

use crate::lifecycle::ShutdownSignal;
use rns_transport::messages::TransportMessage;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Duration;
use tokio::sync::mpsc;

pub(crate) fn spawn_shared_peer_monitor(
    transport_tx: mpsc::Sender<TransportMessage>,
    interface_id: u64,
    online: Arc<AtomicBool>,
    shutdown: ShutdownSignal,
) {
    tokio::spawn(async move {
        let mut was_online = false;
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                _ = shutdown.wait() => break,
                _ = interval.tick() => {
                    let is_online = online.load(std::sync::atomic::Ordering::SeqCst);
                    if is_online == was_online {
                        continue;
                    }
                    was_online = is_online;
                    let message = if is_online {
                        TransportMessage::SharedConnectionRestored { interface_id }
                    } else {
                        TransportMessage::SharedConnectionLost
                    };
                    if transport_tx.send(message).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn shared_peer_monitor_emits_connection_lifecycle_events() {
        let (tx, mut rx) = mpsc::channel(4);
        let online = Arc::new(AtomicBool::new(false));
        let shutdown = ShutdownSignal::new();

        spawn_shared_peer_monitor(tx, 7, online.clone(), shutdown.clone());
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        assert!(
            rx.try_recv().is_err(),
            "offline initial state should not emit a lost event"
        );

        online.store(true, std::sync::atomic::Ordering::SeqCst);
        let restored = tokio::time::timeout(std::time::Duration::from_millis(300), rx.recv())
            .await
            .expect("restored event timed out")
            .expect("monitor channel closed");
        match restored {
            TransportMessage::SharedConnectionRestored { interface_id } => {
                assert_eq!(interface_id, 7)
            }
            other => panic!("expected SharedConnectionRestored, got {other:?}"),
        }

        online.store(false, std::sync::atomic::Ordering::SeqCst);
        let lost = tokio::time::timeout(std::time::Duration::from_millis(300), rx.recv())
            .await
            .expect("lost event timed out")
            .expect("monitor channel closed");
        match lost {
            TransportMessage::SharedConnectionLost => {}
            other => panic!("expected SharedConnectionLost, got {other:?}"),
        }

        shutdown.trigger();
    }
}
