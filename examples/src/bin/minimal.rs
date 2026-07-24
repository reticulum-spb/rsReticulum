use rns_identity::destination::ProofStrategy;
use rns_identity::identity::Identity;
use rns_runtime::application::RegisteredDestination;

#[tokio::main]
async fn main() -> rns_examples::ExampleResult {
    let config = rns_examples::option("--config");
    let (runtime, _shutdown) = rns_examples::runtime(config.as_deref()).await?;
    let mut destination =
        RegisteredDestination::single(runtime, Identity::new(), "example_utilities.minimalsample")
            .await?;
    destination.set_proof_strategy(ProofStrategy::ProveAll);
    println!(
        "Minimal example <{}> running, hit enter to manually send an announce (Ctrl-C to quit)",
        destination.hex_hash()
    );
    loop {
        rns_examples::read_line().await?;
        destination.announce(None).await?;
        println!("Sent announce from <{}>", destination.hex_hash());
    }
}
