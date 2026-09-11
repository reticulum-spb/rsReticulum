use rns_transport::backbone_ingress::{IngressAction, IngressPolicy, PeerSample, Watermarks};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
#[ignore = "requires local Python 1.5.2 checkout; RNS_PYTHON_ROOT/RNS_PYTHON_BIN override defaults"]
fn ingress_arithmetic_matches_python_152() {
    let mut input = String::new();
    let mut expected = Vec::new();
    for capacity in 1..=2048 {
        let w = Watermarks::for_data_capacity(capacity);
        input.push_str(&format!("w {capacity}\n"));
        expected.push(format!("w {} {} {} {}", w.high, w.mid, w.low, w.immediate));
    }
    for count in [1, 31, 32, 33, 64] {
        for bytes in [1, 100, 1_000_000] {
            for span in [0.001, 0.25, 1.0, 3.5] {
                for kind in ['p', 'i'] {
                    let mut peers = (0..count)
                        .map(|id| PeerSample {
                            id: id as u64,
                            packets: u64::from(id == 0),
                            bytes: if id == 0 { bytes } else { 0 },
                            gated: false,
                            hold_until: None,
                        })
                        .collect::<Vec<_>>();
                    let mut policy = IngressPolicy::new(1024, Duration::ZERO);
                    let now = Duration::from_secs_f64(span);
                    let action = if kind == 'i' {
                        policy.immediate(921, now, &peers)
                    } else {
                        policy.periodic(697, now, &mut peers)
                    };
                    let Some(IngressAction::Gate { hold, .. }) = action else {
                        panic!("expected gate")
                    };
                    input.push_str(&format!("h {kind} {count} {bytes} {span}\n"));
                    expected.push(format!("h {}", hold.as_secs_f64()));
                }
            }
        }
    }
    let mut child = Command::new(
        std::env::var("RNS_PYTHON_BIN").unwrap_or_else(|_| "/usr/bin/python3.11".into()),
    )
    .arg("-B")
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/backbone_ingress_reference.py"
    ))
    .arg(std::env::var("RNS_PYTHON_ROOT").unwrap_or_else(|_| "/home/room/src/Reticulum".into()))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).unwrap();
    assert_eq!(actual.lines().count(), expected.len());
    for (actual, expected) in actual.lines().zip(expected.iter()) {
        if actual.starts_with("w ") {
            assert_eq!(actual, expected);
        } else {
            let actual: f64 = actual[2..].parse().unwrap();
            let expected: f64 = expected[2..].parse().unwrap();
            assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
        }
    }
    eprintln!("2048 watermark sets and 120 hold calculations match Python source");
}
