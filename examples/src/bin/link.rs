use std::time::Duration;

use rns_identity::identity::Identity;
use rns_runtime::application::await_path;
use rns_runtime::link_client::LinkSession;
use rns_runtime::link_session::{LinkListener, LinkListenerEvent};

const APP_NAME: &str = "example_utilities.linkexample";

#[tokio::main]
async fn main() -> rns_examples::ExampleResult {
    let config = rns_examples::option("--config");
    let (runtime, _shutdown) = rns_examples::runtime(config.as_deref()).await?;
    if let Some(value) = rns_examples::option("--destination") {
        let hash = rns_examples::parse_hash(&value)?;
        await_path(&runtime, hash, Duration::from_secs(30)).await?;
        let mut link =
            LinkSession::open(&runtime, Identity::new(), hash, 1, Duration::from_secs(30)).await?;
        println!("Link established; enter text, or quit");
        loop {
            tokio::select! {
                line = rns_examples::read_line() => {
                    let line = line?;
                    if line == "quit" { link.close().await?; return Ok(()); }
                    link.send(line.as_bytes()).await?;
                }
                packet = link.recv() => println!("Received data on the link: {}", String::from_utf8_lossy(&packet?)),
            }
        }
    }

    let identity = Identity::new();
    let mut listener = LinkListener::listen(&runtime, &identity, APP_NAME).await?;
    println!(
        "Link example <{}> running; hit enter to announce",
        hex::encode(listener.destination_hash())
    );
    loop {
        tokio::select! {
            line = rns_examples::read_line() => { line?; listener.announce().await?; }
            Some(event) = listener.next() => match event {
                LinkListenerEvent::Established { link_id } => println!("Client connected <{}>", hex::encode(link_id)),
                LinkListenerEvent::Packet { link_id, data } => {
                    let text = String::from_utf8_lossy(&data);
                    println!("Received data on the link: {text}");
                    listener.send(link_id, format!("I received \"{text}\" over the link").into_bytes()).await?;
                }
                LinkListenerEvent::Closed { .. } => println!("Client disconnected"),
                _ => {}
            }
        }
    }
}
