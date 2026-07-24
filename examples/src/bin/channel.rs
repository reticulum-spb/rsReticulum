use std::time::Duration;

use rns_identity::identity::Identity;
use rns_runtime::application::await_path;
use rns_runtime::link_client::LinkSession;
use rns_runtime::link_session::{LinkListener, LinkListenerEvent};

const APP_NAME: &str = "example_utilities.channel";

#[tokio::main]
async fn main() -> rns_examples::ExampleResult {
    let config = rns_examples::option("--config");
    let (runtime, _shutdown) = rns_examples::runtime(config.as_deref()).await?;
    if let Some(value) = rns_examples::option("--destination") {
        let hash = rns_examples::parse_hash(&value)?;
        await_path(&runtime, hash, Duration::from_secs(30)).await?;
        let mut link =
            LinkSession::open(&runtime, Identity::new(), hash, 1, Duration::from_secs(30)).await?;
        println!("Channel established; enter text, or quit");
        loop {
            tokio::select! {
                line = rns_examples::read_line() => {
                    let line = line?;
                    if line == "quit" { link.close().await?; return Ok(()); }
                    link.send_channel(&rns_examples::StringMessage { data: line }).await?;
                }
                message = link.recv_channel() => {
                    let (_, data) = message?;
                    println!("Received data on channel: {}", String::from_utf8_lossy(&data));
                }
            }
        }
    }

    let identity = Identity::new();
    let mut listener = LinkListener::listen(&runtime, &identity, APP_NAME).await?;
    println!(
        "Channel example <{}> running; hit enter to announce",
        hex::encode(listener.destination_hash())
    );
    loop {
        tokio::select! {
            line = rns_examples::read_line() => { line?; listener.announce().await?; }
            Some(event) = listener.next() => if let LinkListenerEvent::Channel(message) = event {
                let text = String::from_utf8_lossy(&message.payload);
                println!("Received data on channel: {text}");
                listener.send_channel(
                    message.link_id,
                    Box::new(rns_examples::StringMessage { data: format!("I received \"{text}\"") }),
                ).await?;
            }
        }
    }
}
