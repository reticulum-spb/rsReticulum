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
