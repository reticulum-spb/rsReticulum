use super::*;

#[test]
#[ignore = "requires local Python 1.5.2 source; RNS_PYTHON_ROOT and RNS_PYTHON_BIN override defaults"]
fn sampled_policy_matches_python_152() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut inputs = Vec::new();
    let mut expected = Vec::new();
    let mut seed = 0x152u64;
    let mut decisions = [0; 3];
    for _ in 0..400 {
        let mut controller = EgressController::new(Duration::ZERO, 0);
        let mut sequence = Vec::new();
        let mut states = Vec::new();
        let mut sent = 0;
        let mut now = 0;
        for _ in 0..40 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            now += [1, 1, 1, 3, 12][(seed % 5) as usize];
            let buffered = [
                0,
                1,
                MID_WATERMARK,
                MID_WATERMARK + 1,
                200_000,
                HIGH_WATERMARK,
            ][((seed >> 8) % 6) as usize];
            let sendable = if seed % 17 == 0 { 0 } else { buffered };
            sent += [0, 0, 0, 19_999, 20_000, 40_000, 40_001][((seed >> 16) % 7) as usize];
            sequence.push([now, buffered, sendable, sent]);
            let decision = controller.evaluate(
                Duration::from_secs(now),
                EgressSample {
                    buffered,
                    sendable,
                    sent,
                },
            );
            decisions[match decision {
                EgressDecision::Open => 0,
                EgressDecision::Gated => 1,
                EgressDecision::Disconnect => 2,
            }] += 1;
            states.push(serde_json::json!([
                format!("{decision:?}"),
                controller.stalled(),
                controller.zero_ticks,
                controller.last_drain.as_secs(),
                controller.previous_sent,
            ]));
            if decision == EgressDecision::Disconnect {
                break;
            }
        }
        inputs.push(sequence);
        expected.push(states);
    }
    assert!(decisions.iter().all(|&count| count > 0));
    let python = std::env::var("RNS_PYTHON_BIN").unwrap_or_else(|_| "/usr/bin/python3.11".into());
    let root =
        std::env::var("RNS_PYTHON_ROOT").unwrap_or_else(|_| "/home/room/src/Reticulum".into());
    let mut child = Command::new(python)
        .arg("-B")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/backbone_flow_reference.py"
        ))
        .arg(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start Python egress oracle");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&inputs).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(actual, serde_json::json!(expected));
    eprintln!("400 Python sequences match: open/gated/disconnect = {decisions:?}");
}

fn sample(buffered: u64, sent: u64) -> EgressSample {
    EgressSample {
        buffered,
        sendable: buffered,
        sent,
    }
}

#[test]
fn eta_hysteresis_has_strict_boundaries() {
    let mut controller = EgressController::new(Duration::ZERO, 0);
    let mut sent = 0;
    // ETA exactly 10 preserves open, >10 gates; exactly 5 and 10
    // preserve gated; <5 releases. Backlog stays above the mid watermark.
    for (tick, drained, expected) in [
        (1, 20_000, EgressDecision::Open),
        (2, 19_999, EgressDecision::Gated),
        (3, 40_000, EgressDecision::Gated),
        (4, 20_000, EgressDecision::Gated),
        (5, 40_001, EgressDecision::Open),
    ] {
        sent += drained;
        assert_eq!(
            controller.evaluate(Duration::from_secs(tick), sample(200_000, sent)),
            expected
        );
    }
}

#[test]
fn three_zero_ticks_gate_and_mid_watermark_releases() {
    let mut controller = EgressController::new(Duration::ZERO, 0);
    for tick in 1..=3 {
        assert_eq!(
            controller.evaluate(Duration::from_secs(tick), sample(MID_WATERMARK + 1, 0)),
            if tick == 3 {
                EgressDecision::Gated
            } else {
                EgressDecision::Open
            }
        );
    }
    assert_eq!(
        controller.evaluate(Duration::from_secs(4), sample(MID_WATERMARK, 0)),
        EgressDecision::Open
    );
    assert_eq!(controller.zero_ticks, 0);
    assert_eq!(
        controller.evaluate(Duration::from_secs(5), sample(MID_WATERMARK + 1, 0)),
        EgressDecision::Open
    );
}

#[test]
fn idle_or_invisible_output_resets_deadline_before_dead_check() {
    for (buffered, sendable) in [(0, 0), (0, 1), (200_000, 0)] {
        let mut controller = EgressController::new(Duration::ZERO, 0);
        controller.stalled = true;
        assert_eq!(
            controller.evaluate(
                Duration::from_secs(30),
                EgressSample {
                    buffered,
                    sendable,
                    sent: 7
                }
            ),
            EgressDecision::Open
        );
        assert_eq!(controller.last_drain, Duration::from_secs(30));
        assert_eq!(controller.previous_sent, 7);
        assert_eq!(
            controller.evaluate(Duration::from_secs(41), sample(1, 7)),
            EgressDecision::Open
        );
        assert_eq!(
            controller.evaluate(Duration::from_secs(42), sample(1, 8)),
            EgressDecision::Disconnect
        );
    }
}

#[test]
fn progress_before_deadline_resets_it_and_eta_uses_fixed_interval() {
    let mut controller = EgressController::new(Duration::ZERO, 0);
    assert_eq!(
        controller.evaluate(Duration::from_secs(11), sample(200_000, 50_000)),
        EgressDecision::Open
    );
    // Variable-interval rate would have incorrectly gated the previous sample.
    assert_eq!(
        controller.evaluate(Duration::from_secs(22), sample(1, 50_000)),
        EgressDecision::Open
    );
    assert_eq!(
        controller.evaluate(Duration::from_secs(23), sample(1, 50_001)),
        EgressDecision::Disconnect
    );
}

#[test]
fn admission_includes_whole_encoded_frame_and_rejects_overflow() {
    let mut controller = EgressController::new(Duration::ZERO, 0);
    assert!(controller.permits_frame(HIGH_WATERMARK - 2, 2, HIGH_WATERMARK));
    assert!(!controller.permits_frame(HIGH_WATERMARK - 2, 3, HIGH_WATERMARK));
    assert!(!controller.permits_frame(u64::MAX, 1, u64::MAX));
    controller.stalled = true;
    assert!(!controller.permits_frame(0, 2, HIGH_WATERMARK));
}
