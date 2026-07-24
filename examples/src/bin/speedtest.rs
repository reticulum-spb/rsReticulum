use std::collections::HashMap;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rns_identity::identity::Identity;
use rns_runtime::application::await_path;
use rns_runtime::link_client::LinkSession;
use rns_runtime::link_session::{LinkListener, LinkListenerEvent};

const APP_NAME: &str = "example_utilities.speedtest";
const DATA_CAP: usize = 2 * 1024 * 1024;
const WINDOW: usize = 16;

#[tokio::main]
async fn main() -> rns_examples::ExampleResult {
    let config = rns_examples::option("--config");
    let (runtime, _shutdown) = rns_examples::runtime(config.as_deref()).await?;
    if let Some(value) = rns_examples::option("--destination") {
        let hash = rns_examples::parse_hash(&value)?;
        await_path(&runtime, hash, Duration::from_secs(30)).await?;
        let mut link =
            LinkSession::open(&runtime, Identity::new(), hash, 1, Duration::from_secs(30)).await?;
        let data = rns_crypto::random::random_bytes(link.mdu());
        let started = Instant::now();
        let mut sent = 0;
        let mut pending = VecDeque::new();
        while sent < DATA_CAP * 5 / 4 {
            pending.push_back(link.send_tracked(&data).await?);
            sent += data.len();
            if pending.len() >= WINDOW {
                let delivered = link.recv_delivery_proof(Duration::from_secs(30)).await?;
                pending
                    .iter()
                    .position(|hash| *hash == delivered)
                    .map(|index| pending.remove(index))
                    .ok_or("proof for unknown speedtest packet")?;
            }
        }
        while !pending.is_empty() {
            let delivered = link.recv_delivery_proof(Duration::from_secs(30)).await?;
            pending
                .iter()
                .position(|hash| *hash == delivered)
                .map(|index| pending.remove(index))
                .ok_or("proof for unknown speedtest packet")?;
        }
        let elapsed = started.elapsed().as_secs_f64();
        println!(
            "Sent {} bytes in {:.3}s ({:.2} Mbit/s)",
            sent,
            elapsed,
            sent as f64 * 8.0 / elapsed / 1_000_000.0
        );
        return link.close().await.map_err(Into::into);
    }

    let identity = Identity::new();
    let mut listener = LinkListener::listen(&runtime, &identity, APP_NAME).await?;
    let mut transfers: HashMap<[u8; 16], (Instant, usize)> = HashMap::new();
    println!(
        "Speedtest <{}> running; hit enter to announce",
        hex::encode(listener.destination_hash())
    );
    loop {
        tokio::select! {
            line = rns_examples::read_line() => { line?; listener.announce().await?; }
            Some(event) = listener.next() => {
                match event {
                    LinkListenerEvent::Established { link_id } => {
                        transfers.insert(link_id, (Instant::now(), 0));
                        println!("Client connected");
                    }
                    LinkListenerEvent::Packet { link_id, data } => {
                        let transfer = transfers.entry(link_id).or_insert((Instant::now(), 0));
                        transfer.1 += data.len();
                        if transfer.1 >= DATA_CAP * 5 / 4 {
                            let elapsed = transfer.0.elapsed().as_secs_f64();
                            println!(
                                "Received {} bytes in {:.3}s ({:.2} Mbit/s)",
                                transfer.1,
                                elapsed,
                                transfer.1 as f64 * 8.0 / elapsed / 1_000_000.0
                            );
                            listener.close(link_id).await?;
                            transfers.remove(&link_id);
                        }
                    }
                    LinkListenerEvent::Closed { link_id } => {
                        transfers.remove(&link_id);
                        println!("Client disconnected");
                    }
                    _ => {}
                }
            }
        }
    }
}
