use std::collections::HashMap;
use std::time::Duration;

use rns_identity::identity::Identity;
use rns_runtime::application::await_path;
use rns_runtime::link_client::LinkSession;
use rns_runtime::link_session::{LinkListener, LinkListenerEvent};

const APP_NAME: &str = "example_utilities.identifyexample";

#[tokio::main]
async fn main() -> rns_examples::ExampleResult {
    let config = rns_examples::option("--config");
    let (runtime, _shutdown) = rns_examples::runtime(config.as_deref()).await?;
    if let Some(value) = rns_examples::option("--destination") {
        let hash = rns_examples::parse_hash(&value)?;
        await_path(&runtime, hash, Duration::from_secs(30)).await?;
        let identity = Identity::new();
        println!("Client identity <{}>", hex::encode(identity.hash));
        let mut link =
            LinkSession::open(&runtime, identity, hash, 1, Duration::from_secs(30)).await?;
        link.identify().await?;
        println!("Link established and identity sent; enter text, or quit");
        loop {
            tokio::select! {
                line = rns_examples::read_line() => {
                    let line = line?;
                    if line == "quit" { link.close().await?; return Ok(()); }
                    link.send(line.as_bytes()).await?;
                }
                packet = link.recv() => println!("Received: {}", String::from_utf8_lossy(&packet?)),
            }
        }
    }

    let identity = Identity::new();
    let mut listener = LinkListener::listen(&runtime, &identity, APP_NAME).await?;
    let mut peers = HashMap::new();
    println!(
        "Link identification example <{}> running; hit enter to announce",
        hex::encode(listener.destination_hash())
    );
    loop {
        tokio::select! {
            line = rns_examples::read_line() => { line?; listener.announce().await?; }
            Some(event) = listener.next() => match event {
                LinkListenerEvent::Identified { link_id, identity_hash } => {
                    peers.insert(link_id, identity_hash);
                    println!("Remote identified as <{}>", hex::encode(identity_hash));
                }
                LinkListenerEvent::Packet { link_id, data } => {
                    let peer = peers.get(&link_id).map(hex::encode).unwrap_or_else(|| "unidentified peer".into());
                    let text = String::from_utf8_lossy(&data);
                    println!("Received data from {peer}: {text}");
                    listener.send(link_id, format!("I received \"{text}\" over the link from {peer}").into_bytes()).await?;
                }
                LinkListenerEvent::Closed { link_id } => { peers.remove(&link_id); println!("Client disconnected"); }
                _ => {}
            }
        }
    }
}
