//! Executable counterparts to the Python programs in `Reticulum/Examples`.
//!
//! The examples deliberately use only public crate APIs. Besides being small
//! demonstrations, they are compile-time coverage for the application-facing
//! surface of rsReticulum.

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use rns_identity::destination::{DestType, Destination, Direction, ProofStrategy};
use rns_identity::identity::Identity;
use rns_link::link::Link;
use rns_protocol::buffer::ChannelBuffer;
use rns_protocol::channel::Channel;
use rns_protocol::channel_message::{ChannelMessageError, MessageBase};
use rns_protocol::resource::{InboundResource, OutboundResource};

pub type ExampleResult = Result<(), Box<dyn Error + Send + Sync>>;

const APP_NAME: &str = "example_utilities";

pub async fn runtime(
    config: Option<&str>,
) -> Result<
    (
        rns_runtime::reticulum::ReticulumHandle,
        rns_runtime::lifecycle::ShutdownSignal,
    ),
    Box<dyn Error + Send + Sync>,
> {
    let shutdown = rns_runtime::lifecycle::ShutdownSignal::new();
    let handle = rns_runtime::reticulum::init(
        config,
        None,
        shutdown.clone(),
        Arc::new(AtomicBool::new(true)),
    )
    .await?;
    Ok((handle, shutdown))
}

pub fn option(name: &str) -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == name {
            return args.next();
        }
    }
    None
}

pub fn parse_hash(value: &str) -> Result<[u8; 16], Box<dyn Error + Send + Sync>> {
    let bytes = hex::decode(value)?;
    <[u8; 16]>::try_from(bytes.as_slice())
        .map_err(|_| "destination hash must be 32 hexadecimal characters".into())
}

pub async fn read_line() -> Result<String, Box<dyn Error + Send + Sync>> {
    Ok(tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        Ok::<_, std::io::Error>(line.trim_end().to_string())
    })
    .await??)
}

/// A user-defined Channel message equivalent to `StringMessage` in Channel.py.
#[derive(Debug, Default)]
pub struct StringMessage {
    pub data: String,
}

impl MessageBase for StringMessage {
    fn msg_type(&self) -> u16 {
        0x0101
    }

    fn pack(&self) -> Vec<u8> {
        self.data.as_bytes().to_vec()
    }

    fn unpack(&mut self, raw: &[u8]) -> Result<(), ChannelMessageError> {
        self.data =
            String::from_utf8(raw.to_vec()).map_err(|_| ChannelMessageError::UnpackFailed)?;
        Ok(())
    }
}

fn single_destination(
    aspect: &str,
) -> Result<(Identity, Destination), Box<dyn Error + Send + Sync>> {
    let identity = Identity::new();
    let mut destination = Destination::new(
        Some(&identity),
        Direction::In,
        DestType::Single,
        &format!("{APP_NAME}.{aspect}"),
    )?;
    destination.set_proof_strategy(ProofStrategy::ProveAll);
    Ok((identity, destination))
}

fn destination_demo(aspect: &str) -> ExampleResult {
    let (identity, mut destination) = single_destination(aspect)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs_f64();
    let announce = destination.announce_packet(&identity, None, None, false, None, now)?;
    println!(
        "{} destination <{}>; announce is {} bytes",
        aspect,
        destination.hex_hash(),
        announce.len()
    );
    Ok(())
}

fn channel_demo() -> ExampleResult {
    let mut tx = Channel::new(0.1);
    let mut rx = Channel::new(0.1);
    rx.register_message_type(0x0101)?;
    let raw = tx.send(&StringMessage {
        data: "Hello over a Reticulum Channel".into(),
    })?;
    let delivered = rx.receive(&raw)?;
    let (_, payload) = delivered.first().ok_or("channel delivered no message")?;
    println!("{}", String::from_utf8_lossy(payload));
    Ok(())
}

fn buffer_demo() -> ExampleResult {
    let mut tx = ChannelBuffer::new(7, 64);
    let mut rx = ChannelBuffer::new(7, 64);
    for frame in tx.write(b"Hello through a Reticulum Buffer")? {
        rx.feed_reader(&frame);
    }
    rx.feed_reader(&tx.close_writer());
    let data = rx.read_all().ok_or("buffer did not reach EOF")?;
    println!("{}", String::from_utf8_lossy(&data));
    Ok(())
}

fn resource_demo(label: &str) -> ExampleResult {
    let data = format!("{label}: resource payload").into_bytes();
    let outbound = OutboundResource::new(data.clone(), true, None)?;
    let mut inbound = InboundResource::new(
        outbound.parts.len(),
        outbound.total_size,
        outbound.advertisement_data_size,
        outbound.random_hash,
        outbound.resource_hash,
        outbound.flags,
        outbound.map_hashes.clone(),
    )?;
    for part in outbound.parts {
        inbound.receive_part(part);
    }
    // No segment/multi-segment splitting happens in this demo, so the only
    // thing that decides whether to strip a metadata prefix is whether the
    // resource actually carries one — matching `MultiSegmentInbound::assemble_segment`'s
    // `resource.flags.has_metadata && segment_index == 1`.
    let strip_metadata = inbound.flags.has_metadata;
    let assembled = inbound.assemble(None, strip_metadata)?;
    if assembled != data {
        return Err("resource round-trip mismatch".into());
    }
    println!("{label}: reassembled {} bytes", assembled.len());
    Ok(())
}

fn link_demo(label: &str) -> ExampleResult {
    let (_, destination) = single_destination(label)?;
    let (link, request) = Link::new_initiator(destination.hash, 1);
    println!(
        "{label}: link <{}> request is {} bytes",
        hex::encode(link.link_id),
        request.len()
    );
    Ok(())
}

fn broadcast_demo() -> ExampleResult {
    let destination = Destination::new(
        None,
        Direction::In,
        DestType::Plain,
        &format!("{APP_NAME}.broadcast.public_information"),
    )?;
    println!("broadcast destination <{}>", destination.hex_hash());
    Ok(())
}

/// Run one counterpart selected by its Python example stem.
pub fn run(name: &str) -> ExampleResult {
    match name {
        "Announce" => {
            destination_demo("announcesample.fruits")?;
            destination_demo("announcesample.noble_gases")
        }
        "Broadcast" => broadcast_demo(),
        "Buffer" => buffer_demo(),
        "Channel" => channel_demo(),
        "Echo" => destination_demo("echo.request"),
        "ExampleInterface" => {
            println!("Custom interfaces implement rns_interface::traits::InterfaceHandle");
            Ok(())
        }
        "Filetransfer" => resource_demo("filetransfer"),
        "Identify" => link_demo("identify"),
        "Link" => link_demo("linkexample"),
        "Minimal" => destination_demo("minimalsample"),
        "Ratchets" => {
            let (_, mut destination) = single_destination("ratchets")?;
            destination.enable_ratchets(true);
            println!("ratchets active: {}", destination.ratchets_active());
            Ok(())
        }
        "Request" => link_demo("request"),
        "Resource" => resource_demo("resource"),
        "Speedtest" => resource_demo("speedtest"),
        _ => Err(format!("unknown example {name}").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_counterparts_execute() {
        for name in [
            "Announce",
            "Broadcast",
            "Buffer",
            "Channel",
            "Echo",
            "ExampleInterface",
            "Filetransfer",
            "Identify",
            "Link",
            "Minimal",
            "Ratchets",
            "Request",
            "Resource",
            "Speedtest",
        ] {
            run(name).unwrap_or_else(|error| panic!("{name}: {error}"));
        }
    }
}
