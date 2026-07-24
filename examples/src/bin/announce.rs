use rns_identity::identity::Identity;
use rns_runtime::application::{RegisteredDestination, announce_stream};

const FRUITS: &[&str] = &["Peach", "Quince", "Date", "Tangerine", "Pomelo", "Grape"];
const GASES: &[&str] = &["Helium", "Neon", "Argon", "Krypton", "Xenon", "Radon"];

#[tokio::main]
async fn main() -> rns_examples::ExampleResult {
    let config = rns_examples::option("--config");
    let (runtime, _shutdown) = rns_examples::runtime(config.as_deref()).await?;
    let identity = Identity::new();
    let private = identity
        .get_private_key()
        .ok_or("identity has no private key")?;
    let mut fruits = RegisteredDestination::single(
        runtime.clone(),
        Identity::from_private_key(&*private)?,
        "example_utilities.announcesample.fruits",
    )
    .await?;
    let mut gases = RegisteredDestination::single(
        runtime.clone(),
        identity,
        "example_utilities.announcesample.noble_gases",
    )
    .await?;
    let mut announces =
        announce_stream(&runtime, Some("example_utilities.announcesample.fruits")).await?;
    println!("Announce example running; hit enter to send announces");
    let mut index = 0usize;
    loop {
        tokio::select! {
            line = rns_examples::read_line() => {
                line?;
                let fruit = FRUITS[index % FRUITS.len()];
                let gas = GASES[index % GASES.len()];
                index += 1;
                fruits.announce(Some(fruit.as_bytes())).await?;
                gases.announce(Some(gas.as_bytes())).await?;
                println!("Sent announces from <{}> and <{}>", fruits.hex_hash(), gases.hex_hash());
            }
            Some(event) = announces.recv() => {
                println!("Received announce from <{}>", hex::encode(event.destination_hash));
                if let Some(data) = event.app_data {
                    println!("App data: {}", String::from_utf8_lossy(&data));
                }
            }
        }
    }
}
