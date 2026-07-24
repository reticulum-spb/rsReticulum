use std::time::Duration;

use rns_identity::destination::{DestType, ProofStrategy};
use rns_identity::identity::Identity;
use rns_runtime::application::{RegisteredDestination, await_path, send_packet};

const APP_NAME: &str = "example_utilities.echo.request";

#[tokio::main]
async fn main() -> rns_examples::ExampleResult {
    let config = rns_examples::option("--config");
    let (runtime, _shutdown) = rns_examples::runtime(config.as_deref()).await?;
    if let Some(value) = rns_examples::option("--destination") {
        let hash = rns_examples::parse_hash(&value)?;
        await_path(&runtime, hash, Duration::from_secs(30)).await?;
        let receipt = send_packet(
            &runtime,
            hash,
            APP_NAME,
            DestType::Single,
            &rns_crypto::random::random_bytes(32),
            Some(Duration::from_secs(12)),
        )
        .await?
        .ok_or("missing packet receipt")?;
        println!(
            "Valid reply from <{}>, round-trip time {:.3} ms",
            hex::encode(hash),
            receipt.rtt.as_secs_f64() * 1000.0
        );
        return Ok(());
    }

    let mut destination = RegisteredDestination::single(runtime, Identity::new(), APP_NAME).await?;
    destination.set_proof_strategy(ProofStrategy::ProveAll);
    println!(
        "Echo server <{}> running; hit enter to announce",
        destination.hex_hash()
    );
    loop {
        tokio::select! {
            line = rns_examples::read_line() => {
                line?;
                destination.announce(None).await?;
            }
            packet = destination.recv() => {
                let packet = packet?;
                println!("Received packet {}, proof sent", hex::encode(&packet.packet_hash[..8]));
            }
        }
    }
}
