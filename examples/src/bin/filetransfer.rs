use std::path::PathBuf;
use std::time::Duration;

use rns_identity::identity::Identity;
use rns_runtime::application::await_path;
use rns_runtime::link_client::LinkSession;
use rns_runtime::link_session::{LinkListener, LinkListenerEvent};

const APP_NAME: &str = "example_utilities.filetransfer.server";

#[tokio::main]
async fn main() -> rns_examples::ExampleResult {
    let config = rns_examples::option("--config");
    let (runtime, _shutdown) = rns_examples::runtime(config.as_deref()).await?;
    if let Some(value) = rns_examples::option("--destination") {
        let hash = rns_examples::parse_hash(&value)?;
        await_path(&runtime, hash, Duration::from_secs(30)).await?;
        let mut link =
            LinkSession::open(&runtime, Identity::new(), hash, 1, Duration::from_secs(30)).await?;
        let listing: Vec<String> = rmp_serde::from_slice(&link.recv().await?)?;
        println!("Files available on server:");
        for (index, name) in listing.iter().enumerate() {
            println!("  {}: {}", index + 1, name);
        }
        loop {
            println!("Enter file number, or quit:");
            let command = rns_examples::read_line().await?;
            if matches!(command.trim(), "quit" | "q" | "exit") {
                link.close().await?;
                return Ok(());
            }
            let index: usize = command.trim().parse()?;
            let filename = listing
                .get(index.checked_sub(1).ok_or("invalid file number")?)
                .ok_or("invalid file number")?;
            link.send(filename.as_bytes()).await?;
            let resource = link.recv_resource(Duration::from_secs(600)).await?;
            std::fs::write(filename, &resource.data)?;
            println!("Saved {} bytes to {}", resource.data.len(), filename);
        }
    }

    let root = PathBuf::from(rns_examples::option("--path").unwrap_or_else(|| ".".into()));
    let identity = Identity::new();
    let mut listener = LinkListener::listen(&runtime, &identity, APP_NAME).await?;
    println!(
        "File server <{}> serving {}; hit enter to announce",
        hex::encode(listener.destination_hash()),
        root.display()
    );
    loop {
        tokio::select! {
            line = rns_examples::read_line() => { line?; listener.announce().await?; }
            Some(event) = listener.next() => {
                match event {
                    LinkListenerEvent::Established { link_id } => {
                        let files = list_files(&root)?;
                        listener.send(link_id, rmp_serde::to_vec(&files)?).await?;
                        println!("Client connected, sent file list");
                    }
                    LinkListenerEvent::Packet { link_id, data } => {
                        let requested = String::from_utf8(data)?;
                        if list_files(&root)?.contains(&requested) {
                            listener.send(link_id, std::fs::read(root.join(&requested))?).await?;
                            println!("Sending \"{requested}\"");
                        } else {
                            println!("Client requested an unknown file");
                            listener.close(link_id).await?;
                        }
                    }
                    LinkListenerEvent::Closed { .. } => println!("Client disconnected"),
                    _ => {}
                }
            }
        }
    }
}

fn list_files(root: &std::path::Path) -> std::io::Result<Vec<String>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_file() && !name.starts_with('.') {
            files.push(name);
        }
    }
    files.sort();
    Ok(files)
}
