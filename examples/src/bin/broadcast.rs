use rns_identity::destination::DestType;
use rns_runtime::application::{RegisteredDestination, send_packet};

#[tokio::main]
async fn main() -> rns_examples::ExampleResult {
    let config = rns_examples::option("--config");
    let channel =
        rns_examples::option("--channel").unwrap_or_else(|| "public_information".to_string());
    let app_name = format!("example_utilities.broadcast.{channel}");
    let (runtime, _shutdown) = rns_examples::runtime(config.as_deref()).await?;
    let mut destination = RegisteredDestination::plain(runtime.clone(), &app_name).await?;
    println!(
        "Broadcast example <{}> running, enter text to broadcast",
        destination.hex_hash()
    );
    loop {
        tokio::select! {
            line = rns_examples::read_line() => {
                let line = line?;
                if !line.is_empty() {
                    send_packet(&runtime, destination.hash(), &app_name, DestType::Plain, line.as_bytes(), None).await?;
                }
            }
            packet = destination.recv() => {
                println!("Received data: {}", String::from_utf8_lossy(&packet?.data));
            }
        }
    }
}
