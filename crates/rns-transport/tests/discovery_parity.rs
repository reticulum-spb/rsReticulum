//! Python-generated decoder contracts; regenerate with interop_tests/discovery_reference.py.
use rns_transport::discovery::app_data::decode_info;

#[test]
fn discovery_fields_follow_python_receiver() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/discovery-reference.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let packed = hex::decode(case["packed"].as_str().unwrap()).unwrap();
        let result = decode_info(&packed);
        if !case["accepted"].as_bool().unwrap() {
            assert!(
                result.is_err(),
                "{}: accepted invalid Python input",
                case["case"]
            );
            continue;
        }
        let info = result.unwrap_or_else(|e| panic!("{}: {e}", case["case"]));
        assert_eq!(
            info.name,
            case["name"].as_str().unwrap(),
            "{}",
            case["case"]
        );
        // Serialize the public fields so this test is RED on the old f64 API,
        // and also distinguishes absent coordinates from a real zero coordinate.
        assert_eq!(
            serde_json::json!(info.latitude),
            case["latitude"],
            "{}",
            case["case"]
        );
        assert_eq!(
            serde_json::json!(info.longitude),
            case["longitude"],
            "{}",
            case["case"]
        );
        assert_eq!(
            serde_json::json!(info.height),
            case["height"],
            "{}",
            case["case"]
        );
    }
}

#[test]
fn discovery_operator_address_survives_python_to_rust() {
    use rns_transport::discovery::app_data::encode_info;
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/discovery-reference.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let packed = hex::decode(case["packed"].as_str().unwrap()).unwrap();
        let result = decode_info(&packed);
        if !case["accepted"].as_bool().unwrap() {
            assert!(
                result.is_err(),
                "{}: accepted invalid Python input",
                case["case"]
            );
            continue;
        }
        let info = result.unwrap();
        let emitted = encode_info(&info).unwrap();
        let map = rmpv::decode::read_value(&mut &emitted.packed[..]).unwrap();
        let address = map
            .as_map()
            .unwrap()
            .iter()
            .find(|(k, _)| k.as_u64() == Some(0xF0))
            .map(|(_, v)| hex::encode(v.as_slice().unwrap()));
        assert_eq!(
            address.as_deref(),
            case["operator_address"].as_str(),
            "{}",
            case["case"]
        );
    }
}

#[test]
#[ignore = "requires Python reference; set PARITY_PYTHON and PYTHONPATH"]
fn discovery_metadata_reaches_python() {
    use rns_transport::discovery::app_data::encode_info;
    use std::io::Write;
    use std::process::{Command, Stdio};
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/discovery-reference.json")).unwrap();
    let emitted: Vec<_> = cases.as_array().unwrap().iter().filter(|case| case["accepted"] == true).map(|case| {
        let info = decode_info(&hex::decode(case["packed"].as_str().unwrap()).unwrap()).unwrap();
        let encoded = encode_info(&info).unwrap();
        serde_json::json!({"packed": hex::encode(encoded.packed), "operator_address": case["operator_address"]})
    }).collect();
    let input = serde_json::json!({"version": env!("CARGO_PKG_VERSION"), "cases": emitted});
    let python =
        std::env::var("PARITY_PYTHON").expect("set PARITY_PYTHON to the reference interpreter");
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../interop_tests/discovery_metadata_reference.py");
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
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}

#[test]
fn discovery_storage_retains_python_operator_address() {
    use rns_transport::discovery::storage::{DiscoveredInterface, DiscoveryStore};
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/discovery-reference.json")).unwrap();
    let case = cases
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["case"] == "operator")
        .unwrap();
    let info = decode_info(&hex::decode(case["packed"].as_str().unwrap()).unwrap()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let path = std::env::temp_dir().join(format!(
        "rns-python-operator-{}-{}",
        std::process::id(),
        now.as_nanos()
    ));
    std::fs::create_dir(&path).unwrap();
    let store = DiscoveryStore::open(&path).unwrap();
    store
        .upsert(DiscoveredInterface {
            info,
            network_id: [0; 16],
            hops: 1,
            stamp_value: 16,
            stamp: vec![0; 32],
            discovered: now.as_secs(),
            last_heard: now.as_secs(),
            heard_count: 1,
            status: None,
        })
        .unwrap();
    drop(store);
    let records = DiscoveryStore::open(&path).unwrap().list(None).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].info.operator_address.map(hex::encode).as_deref(),
        case["operator_address"].as_str()
    );
    std::fs::remove_dir_all(path).unwrap();
}
