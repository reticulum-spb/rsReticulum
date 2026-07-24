use std::collections::HashMap;
use std::time::Duration;

use rns_identity::identity::Identity;
use rns_protocol::buffer::{StreamReader, StreamWriter};
use rns_protocol::channel_message::MessageBase;
use rns_protocol::stream_data::StreamDataMessage;
use rns_runtime::application::await_path;
use rns_runtime::link_client::LinkSession;
use rns_runtime::link_session::{LinkListener, LinkListenerEvent};

const APP_NAME: &str = "example_utilities.buffer";
const STREAM_ID: u16 = 1;

#[tokio::main]
async fn main() -> rns_examples::ExampleResult {
    let config = rns_examples::option("--config");
    let (runtime, _shutdown) = rns_examples::runtime(config.as_deref()).await?;
    if let Some(value) = rns_examples::option("--destination") {
        let hash = rns_examples::parse_hash(&value)?;
        await_path(&runtime, hash, Duration::from_secs(30)).await?;
        let mut link =
            LinkSession::open(&runtime, Identity::new(), hash, 1, Duration::from_secs(30)).await?;
        let mut reader = StreamReader::new(STREAM_ID);
        println!("Buffer established; enter text, or quit");
        loop {
            tokio::select! {
                line = rns_examples::read_line() => {
                    let line = line?;
                    if line == "quit" { link.close().await?; return Ok(()); }
                    let mut writer = StreamWriter::new(STREAM_ID, 400);
                    for frame in writer.write(line.as_bytes())? {
                        link.send_channel(&frame).await?;
                    }
                    link.send_channel(&writer.close_simple()).await?;
                }
                message = link.recv_channel() => {
                    let (kind, payload) = message?;
                    if kind == rns_protocol::channel_message::SMT_STREAM_DATA {
                        let mut frame = StreamDataMessage::new(0, Vec::new(), false);
                        frame.unpack(&payload)?;
                        reader.feed(&frame);
                        if reader.is_done() {
                            println!("Received buffer: {}", String::from_utf8_lossy(&reader.read_all().unwrap_or_default()));
                            reader = StreamReader::new(STREAM_ID);
                        }
                    }
                }
            }
        }
    }

    let identity = Identity::new();
    let mut listener = LinkListener::listen(&runtime, &identity, APP_NAME).await?;
    let mut readers: HashMap<[u8; 16], StreamReader> = HashMap::new();
    println!(
        "Buffer example <{}> running; hit enter to announce",
        hex::encode(listener.destination_hash())
    );
    loop {
        tokio::select! {
            line = rns_examples::read_line() => { line?; listener.announce().await?; }
            Some(event) = listener.next() => if let LinkListenerEvent::Channel(message) = event {
                if message.msg_type != rns_protocol::channel_message::SMT_STREAM_DATA { continue; }
                let mut frame = StreamDataMessage::new(0, Vec::new(), false);
                frame.unpack(&message.payload)?;
                let reader = readers.entry(message.link_id).or_insert_with(|| StreamReader::new(STREAM_ID));
                reader.feed(&frame);
                if reader.is_done() {
                    let data = reader.read_all().unwrap_or_default();
                    println!("Received buffer: {}", String::from_utf8_lossy(&data));
                    let mut writer = StreamWriter::new(STREAM_ID, 400);
                    for response in writer.write(b"Buffer received")? {
                        listener.send_channel(message.link_id, Box::new(response)).await?;
                    }
                    listener.send_channel(message.link_id, Box::new(writer.close_simple())).await?;
                    *reader = StreamReader::new(STREAM_ID);
                }
            }
        }
    }
}
