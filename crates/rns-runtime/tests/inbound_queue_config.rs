//! Run under both default/full and --no-default-features --features client.
use rns_runtime::{config::Config, reticulum::ReticulumConfig};
use rns_transport::inbound_queue::InboundQueueLimits;

#[test]
fn inbound_queue_yaml_roundtrip_and_runtime_limits() {
    for (yaml, expected) in [
        ("{}", [1024, 128, 128, 8]),
        (
            "reticulum:\n  qlen_in_data: 7\n  qlen_in_announce: 5\n  qlen_in_pr: 3\n  qlen_in_il: 1\n",
            [7, 5, 3, 1],
        ),
        ("reticulum:\n  qlen_in_pr: 17\n", [1024, 128, 17, 8]),
    ] {
        let typed = Config::parse(yaml, "config.yaml").unwrap();
        let serialized = typed.to_yaml().unwrap();
        let roundtrip = Config::parse(&serialized, "config.yaml").unwrap();
        assert_eq!(roundtrip, typed);
        let normalized = roundtrip.to_runtime_config().unwrap();
        let runtime = ReticulumConfig::try_from_config(&normalized).unwrap();
        assert_eq!(runtime.inbound_queue_limits.sizes(), expected);
    }
    assert_eq!(
        ReticulumConfig::default().inbound_queue_limits,
        InboundQueueLimits::default()
    );
}

#[test]
fn inbound_queue_yaml_rejects_invalid_sizes_and_sum_overflow() {
    for key in [
        "qlen_in_data",
        "qlen_in_announce",
        "qlen_in_pr",
        "qlen_in_il",
    ] {
        for value in [
            "0",
            "-1",
            "1.5",
            "true",
            "null",
            "[]",
            "18446744073709551616",
        ] {
            let yaml = format!("reticulum:\n  {key}: {value}\n");
            assert!(
                Config::parse(&yaml, "config.yaml").is_err(),
                "accepted {yaml}"
            );
        }
        let yaml = format!("reticulum:\n  {key}: {}\n", usize::MAX);
        assert!(
            Config::parse(&yaml, "config.yaml").is_err(),
            "accepted {yaml}"
        );
    }
    // Very large valid limits need no eager allocation.
    let yaml = format!(
        "reticulum:\n  qlen_in_data: {}\n  qlen_in_announce: 1\n  qlen_in_pr: 1\n  qlen_in_il: 1\n",
        usize::MAX - 3
    );
    let normalized = Config::parse(&yaml, "config.yaml")
        .unwrap()
        .to_runtime_config()
        .unwrap();
    assert_eq!(
        ReticulumConfig::try_from_config(&normalized)
            .unwrap()
            .inbound_queue_limits
            .sizes(),
        [usize::MAX - 3, 1, 1, 1]
    );
    let limits = ReticulumConfig::try_from_config(&normalized)
        .unwrap()
        .inbound_queue_limits;
    let (_actor, input, control) =
        rns_transport::actor::TransportActor::new_with_control_channel_and_queue_limits(limits);
    assert_eq!(input.max_capacity(), 4096);
    assert_eq!(control.max_capacity(), 256);
}

#[test]
fn inbound_queue_normalized_config_also_validates_limits() {
    // Embedders can modify the normalized config without going through YAML.
    for key in [
        "qlen_in_data",
        "qlen_in_announce",
        "qlen_in_pr",
        "qlen_in_il",
    ] {
        for value in [
            "0".to_owned(),
            "-1".into(),
            "bad".into(),
            usize::MAX.to_string(),
        ] {
            let mut normalized = Config::parse("{}", "config.yaml")
                .unwrap()
                .to_runtime_config()
                .unwrap();
            normalized.ensure_section("reticulum").set(key, &value);
            assert!(
                ReticulumConfig::try_from_config(&normalized).is_err(),
                "accepted {key}={value}"
            );
        }
        let mut normalized = Config::parse("{}", "config.yaml")
            .unwrap()
            .to_runtime_config()
            .unwrap();
        normalized
            .ensure_section("reticulum")
            .set_list(key, vec!["1".into()]);
        assert!(ReticulumConfig::try_from_config(&normalized).is_err());
        normalized.ensure_section("reticulum").remove(key);
        assert_eq!(
            ReticulumConfig::try_from_config(&normalized)
                .unwrap()
                .inbound_queue_limits,
            InboundQueueLimits::default()
        );
    }
}
