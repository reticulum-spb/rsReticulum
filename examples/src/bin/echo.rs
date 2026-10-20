use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use clap::Parser;
use rns_identity::destination::{DestType, Destination, Direction, ProofStrategy};
use rns_identity::identity::Identity;
use rns_runtime::application::{InboundPacket, RegisteredDestination, await_path, send_packet};
use rns_runtime::reticulum::ReticulumHandle;
use rns_transport::messages::{TransportQuery, TransportQueryResponse};

const APP_NAME: &str = "example_utilities.echo.request";

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
struct EchoConfig {
    announce_interval: u64,
    app_name: String,
}

impl Default for EchoConfig {
    fn default() -> Self {
        Self {
            announce_interval: 300,
            app_name: APP_NAME.to_owned(),
        }
    }
}

impl EchoConfig {
    fn load(dir: &Path, create: bool) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let path = dir.join("config.yaml");
        let config: Self = match std::fs::read_to_string(&path) {
            Ok(contents) => serde_saphyr::from_str(&contents)
                .map_err(|error| format!("{}: {error}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let config = Self::default();
                if create {
                    std::fs::create_dir_all(dir)?;
                    std::fs::write(&path, serde_saphyr::to_string(&config)?)?;
                }
                config
            }
            Err(error) => return Err(error.into()),
        };
        if config.announce_interval == 0
            || tokio::time::Instant::now()
                .checked_add(Duration::from_secs(config.announce_interval))
                .is_none()
        {
            return Err(format!(
                "{}: announce_interval must be a positive, representable interval in seconds",
                path.display()
            )
            .into());
        }
        if config.app_name.trim().is_empty() {
            return Err(format!("{}: app_name must not be empty", path.display()).into());
        }
        Ok(config)
    }
}

#[derive(Parser)]
#[command(about = "Reticulum echo server and client")]
struct Args {
    /// Reticulum configuration directory
    #[arg(long)]
    config: Option<String>,

    /// Send an echo request to this destination hash (server mode if omitted)
    #[arg(long, value_name = "HASH")]
    destination: Option<String>,

    /// Print the saved echo server destination address and exit
    #[arg(long, conflicts_with = "destination")]
    identity: bool,

    /// Echo directory for config.yaml, identity and requests.log [default: ~/.rsEcho]
    #[arg(long, value_name = "PATH")]
    data_dir: Option<PathBuf>,
}

fn default_data_dir() -> Result<PathBuf, std::io::Error> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(".rsEcho"))
        .ok_or_else(|| std::io::Error::other("Cannot find home directory; use --data-dir"))
}

fn server_storage(
    dir: &Path,
) -> Result<(Identity, File), Box<dyn std::error::Error + Send + Sync>> {
    std::fs::create_dir_all(dir)?;
    let identity_path = dir.join("identity");
    let identity = match std::fs::metadata(&identity_path) {
        Ok(_) => Identity::from_file(&identity_path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let identity = Identity::new();
            identity.to_file(&identity_path)?;
            identity
        }
        Err(error) => return Err(error.into()),
    };
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("requests.log"))?;
    Ok((identity, log))
}

fn file_logger(log: File) -> impl tracing::Subscriber + Send + Sync {
    // Use the same default timestamp and event formatter as rnsd --service.
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(Mutex::new(log))
        .finish()
}

#[derive(Default)]
struct Signal {
    rssi: Option<f64>,
    snr: Option<f64>,
}

impl std::fmt::Display for Signal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(rssi) = self.rssi {
            write!(f, ", RSSI {rssi:.1} dBm")?;
        }
        if let Some(snr) = self.snr {
            write!(f, ", SNR {snr:.1} dB")?;
        }
        Ok(())
    }
}

async fn packet_signal(runtime: &ReticulumHandle, packet_hash: [u8; 32]) -> Signal {
    let (rssi, snr) = tokio::join!(
        runtime.query_control(TransportQuery::GetPacketRssi { packet_hash }),
        runtime.query_control(TransportQuery::GetPacketSnr { packet_hash }),
    );
    let value = |response| match response {
        Some(TransportQueryResponse::FloatResult(value)) => value.filter(|v| v.is_finite()),
        _ => None,
    };
    Signal {
        rssi: value(rssi),
        snr: value(snr),
    }
}

fn log_request(packet: &InboundPacket, signal: &Signal) {
    tracing::info!(
        packet = %hex::encode(packet.packet_hash),
        interface = packet.interface_id,
        bytes = packet.data.len(),
        proof = "sent",
        rssi_dbm = signal.rssi,
        snr_db = signal.snr,
        "Received echo request"
    );
}

#[tokio::main]
async fn main() -> rns_examples::ExampleResult {
    let args = Args::parse();
    let data_dir = match args.data_dir {
        Some(path) => path,
        None => default_data_dir()?,
    };
    let config = EchoConfig::load(&data_dir, !args.identity && args.destination.is_none())?;
    if args.identity {
        let identity = Identity::from_file(&data_dir.join("identity"))?;
        let destination = Destination::new(
            Some(&identity),
            Direction::In,
            DestType::Single,
            &config.app_name,
        )?;
        println!("{}", destination.hex_hash());
        return Ok(());
    }
    if let Some(value) = args.destination {
        let hash = rns_examples::parse_hash(&value)?;
        let (runtime, _shutdown) = rns_examples::runtime(args.config.as_deref()).await?;
        await_path(&runtime, hash, Duration::from_secs(30)).await?;
        let receipt = send_packet(
            &runtime,
            hash,
            &config.app_name,
            DestType::Single,
            &rns_crypto::random::random_bytes(32),
            Some(Duration::from_secs(12)),
        )
        .await?
        .ok_or("missing packet receipt")?;
        let signal = packet_signal(&runtime, receipt.proof_hash).await;
        println!(
            "Valid reply from <{}>, round-trip time {:.3} ms{}",
            hex::encode(hash),
            receipt.rtt.as_secs_f64() * 1000.0,
            signal,
        );
        return Ok(());
    }

    let (identity, log) = server_storage(&data_dir)?;
    tracing::subscriber::set_global_default(file_logger(log))?;
    let (runtime, _shutdown) = rns_examples::runtime(args.config.as_deref()).await?;
    let mut destination =
        RegisteredDestination::single(runtime.clone(), identity, &config.app_name).await?;
    destination.set_proof_strategy(ProofStrategy::ProveAll);
    destination.announce(None).await?;
    let period = Duration::from_secs(config.announce_interval);
    let mut announces = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    announces.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    println!(
        "Echo server <{}> running; announcing every {} seconds",
        destination.hex_hash(),
        config.announce_interval,
    );
    println!("Server data: {}", data_dir.display());
    loop {
        tokio::select! {
            _ = announces.tick() => {
                destination.announce(None).await?;
            }
            packet = destination.recv() => {
                let packet = packet?;
                let signal = packet_signal(&runtime, packet.packet_hash).await;
                log_request(&packet, &signal);
                println!("Received packet {}, proof sent{}", hex::encode(&packet.packet_hash[..8]), signal);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_and_custom_values() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("echo");
        let config = EchoConfig::load(&dir, false).unwrap();
        assert_eq!(config.announce_interval, 300);
        assert_eq!(config.app_name, APP_NAME);
        assert!(!dir.exists());
        EchoConfig::load(&dir, true).unwrap();
        assert!(dir.join("config.yaml").is_file());
        let config = EchoConfig::load(&dir, false).unwrap();
        assert_eq!(config.announce_interval, 300);
        assert_eq!(config.app_name, APP_NAME);
        std::fs::write(
            dir.join("config.yaml"),
            "announce_interval: 15\napp_name: custom.echo.request\n",
        )
        .unwrap();
        let config = EchoConfig::load(&dir, true).unwrap();
        assert_eq!(config.announce_interval, 15);
        assert_eq!(config.app_name, "custom.echo.request");
        std::fs::write(dir.join("config.yaml"), "announce_interval: 60\n").unwrap();
        assert_eq!(EchoConfig::load(&dir, false).unwrap().app_name, APP_NAME);
    }

    #[test]
    fn invalid_config_is_rejected_and_preserved() {
        let temp = tempfile::tempdir().unwrap();
        for contents in [
            "announce_interval: 0\n",
            "announce_interval: -1\n",
            "announce_interval: invalid\n",
            "app_name: ' '\n",
            "announce_interva: 30\n",
            "app_name: [\n",
        ] {
            let path = temp.path().join("config.yaml");
            std::fs::write(&path, contents).unwrap();
            assert!(EchoConfig::load(temp.path(), true).is_err(), "{contents}");
            assert_eq!(std::fs::read_to_string(path).unwrap(), contents);
        }
    }

    #[test]
    fn server_identity_survives_restart_and_log_is_appended() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("echo");
        let (identity, log) = server_storage(&dir).unwrap();
        let packet = InboundPacket {
            data: vec![1, 2, 3],
            raw: vec![],
            interface_id: 42,
            packet_hash: [7; 32],
        };
        tracing::subscriber::with_default(file_logger(log), || {
            log_request(&packet, &Signal::default())
        });
        let (reloaded, log) = server_storage(&dir).unwrap();
        assert_eq!(identity.get_private_key(), reloaded.get_private_key());
        tracing::subscriber::with_default(file_logger(log), || {
            log_request(
                &packet,
                &Signal {
                    rssi: Some(-92.5),
                    snr: Some(0.0),
                },
            )
        });
        let contents = std::fs::read_to_string(dir.join("requests.log")).unwrap();
        assert_eq!(contents.lines().count(), 2);
        assert!(contents.lines().all(|line| {
            line.contains(" INFO ")
                && line.contains("Received echo request")
                && !line.contains('\u{1b}')
                && line.contains(&format!("packet={}", hex::encode(packet.packet_hash)))
                && line.contains("interface=42 bytes=3 proof=\"sent\"")
        }));
        let lines: Vec<_> = contents.lines().collect();
        assert!(!lines[0].contains("rssi_dbm"));
        assert!(!lines[0].contains("snr_db"));
        assert!(lines[1].contains("rssi_dbm=-92.5 snr_db=0"));
        for line in contents.lines() {
            let timestamp = line.split_whitespace().next().unwrap();
            assert_eq!(&timestamp[4..5], "-");
            assert_eq!(&timestamp[7..8], "-");
            assert_eq!(&timestamp[10..11], "T");
            assert!(timestamp.ends_with('Z'));
        }
    }

    #[test]
    fn signal_output_includes_only_available_metrics() {
        assert_eq!(Signal::default().to_string(), "");
        assert_eq!(
            Signal {
                rssi: Some(-90.0),
                snr: None
            }
            .to_string(),
            ", RSSI -90.0 dBm"
        );
        assert_eq!(
            Signal {
                rssi: None,
                snr: Some(0.0)
            }
            .to_string(),
            ", SNR 0.0 dB"
        );
        assert_eq!(
            Signal {
                rssi: Some(-92.5),
                snr: Some(-3.5)
            }
            .to_string(),
            ", RSSI -92.5 dBm, SNR -3.5 dB"
        );
    }

    #[test]
    fn corrupt_identity_is_not_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("identity");
        std::fs::write(&path, b"invalid identity").unwrap();
        assert!(server_storage(temp.path()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"invalid identity");
    }
}
