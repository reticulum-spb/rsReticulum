//! Explicit opt-in integration with the Python reference (no Rust/Python mocks).
use rns_runtime::config::{Config, ReticulumConfig};
use rns_transport::discovery::announcer::DiscoveryInterfaceConfig;
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires the Python reference environment; set PARITY_PYTHON and PYTHONPATH"]
fn discovery_stamp_defaults_match_python_receiver() {
    let producer = DiscoveryInterfaceConfig::backbone("Parity".into(), "127.0.0.1".into(), 4242);
    let mut explicit_producer = producer.clone();
    explicit_producer.stamp_value = 15;
    let explicit = Config::parse(
        "reticulum:\n  required_discovery_value: 15\ninterfaces: []\n",
        "parity.yaml",
    )
    .unwrap();
    let settings = serde_json::json!({
        "producer": producer.stamp_value,
        "receiver": ReticulumConfig::default().required_discovery_value,
        "explicit_producer": explicit_producer.stamp_value,
        "explicit_receiver": explicit.reticulum.required_discovery_value,
    });
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../interop_tests/discovery_stamp_reference.py");
    let python = std::env::var("PARITY_PYTHON")
        .expect("set PARITY_PYTHON to the reference environment's interpreter");
    let mut child = Command::new(python)
        .arg(script)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(settings.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "Python rejected Rust settings: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}
