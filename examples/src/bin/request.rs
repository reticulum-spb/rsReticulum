use std::time::Duration;

use rns_identity::identity::Identity;
use rns_runtime::application::await_path;
use rns_runtime::link_client::LinkSession;
use rns_runtime::link_manager::RequestOutcome;
use rns_runtime::link_session::LinkListener;

const APP_NAME: &str = "example_utilities.requestexample";

#[tokio::main]
async fn main() -> rns_examples::ExampleResult {
    let config = rns_examples::option("--config");
    let (runtime, _shutdown) = rns_examples::runtime(config.as_deref()).await?;
    if let Some(value) = rns_examples::option("--destination") {
        let hash = rns_examples::parse_hash(&value)?;
        await_path(&runtime, hash, Duration::from_secs(30)).await?;
        let mut link =
            LinkSession::open(&runtime, Identity::new(), hash, 1, Duration::from_secs(30)).await?;
        loop {
            println!("Press enter to request random text");
            rns_examples::read_line().await?;
            let response = link
                .request("/random/text", None, Duration::from_secs(30))
                .await?;
            println!("Response: {}", String::from_utf8_lossy(&response));
        }
    }

    let identity = Identity::new();
    let listener = LinkListener::listen_with_request_handler(
        &runtime,
        &identity,
        APP_NAME,
        Some(|_link_id, _path_hash, _data| {
            RequestOutcome::Reply(
                b"Lorem ipsum dolor sit amet, consectetur adipiscing elit.".to_vec(),
            )
        }),
    )
    .await?;
    println!(
        "Request example <{}> running; hit enter to announce",
        hex::encode(listener.destination_hash())
    );
    loop {
        rns_examples::read_line().await?;
        listener.announce().await?;
    }
}
