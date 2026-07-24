use std::time::Duration;

use rns_identity::identity::Identity;
use rns_runtime::application::await_path;
use rns_runtime::link_client::LinkSession;
use rns_runtime::link_session::{LinkListener, LinkListenerEvent};

const APP_NAME: &str = "example_utilities.resourceexample";
const RESOURCE_SIZE: usize = 32 * 1024 * 1024;

#[tokio::main]
async fn main() -> rns_examples::ExampleResult {
    let config = rns_examples::option("--config");
    let (runtime, _shutdown) = rns_examples::runtime(config.as_deref()).await?;
    if let Some(value) = rns_examples::option("--destination") {
        let hash = rns_examples::parse_hash(&value)?;
        await_path(&runtime, hash, Duration::from_secs(30)).await?;
        let mut link =
            LinkSession::open(&runtime, Identity::new(), hash, 1, Duration::from_secs(30)).await?;
        println!("Link established; hit enter to send a Resource, or type quit");
        loop {
            let command = rns_examples::read_line().await?;
            if matches!(command.trim(), "quit" | "q" | "exit") {
                link.close().await?;
                return Ok(());
            }
            let data = rns_crypto::random::random_bytes(RESOURCE_SIZE);
            let metadata = rmp_serde::to_vec(&(
                "They looked up",
                vec![1u8, 2, 3, 4],
                rns_crypto::random::random_bytes(16),
            ))?;
            println!(
                "Sending {} bytes; first 32 bytes: {}",
                data.len(),
                hex::encode(&data[..32])
            );
            let resource_hash = link
                .send_resource_with_metadata(data, Some(metadata), false, Duration::from_secs(600))
                .await?;
            println!(
                "Resource <{}> sent successfully",
                hex::encode(resource_hash)
            );
        }
    }

    let identity = Identity::new();
    let mut listener = LinkListener::listen(&runtime, &identity, APP_NAME).await?;
    println!(
        "Resource example <{}> running; hit enter to announce",
        hex::encode(listener.destination_hash())
    );
    loop {
        tokio::select! {
            line = rns_examples::read_line() => { line?; listener.announce().await?; }
            Some(event) = listener.next() => {
                match event {
                    LinkListenerEvent::Established { .. } => println!("Client connected"),
                    LinkListenerEvent::Resource(resource) => {
                        println!(
                            "Resource <{}> received: {} bytes; metadata: {}; first 32 bytes: {}",
                            hex::encode(resource.resource_hash),
                            resource.data.len(),
                            resource.metadata.as_ref().map(hex::encode).unwrap_or_else(|| "none".into()),
                            hex::encode(&resource.data[..resource.data.len().min(32)])
                        );
                    }
                    LinkListenerEvent::Closed { .. } => println!("Client disconnected"),
                    _ => {}
                }
            }
        }
    }
}
