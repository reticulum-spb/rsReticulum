use rns_transport::inbound_queue::{InboundQueueLimits, InboundQueues, TrafficClass};
use std::io::Write;
use std::process::{Command, Stdio};

/// Requires the read-only Python 1.5.2 source checkout and Python >= 3.8.
#[test]
#[ignore = "requires local Python Reticulum reference; set RNS_PYTHON_ROOT and RNS_PYTHON_BIN if needed"]
fn inbound_queue_matches_python_152() {
    let mut queues = InboundQueues::new(InboundQueueLimits::new([7, 5, 3, 2]).unwrap());
    let mut operations = String::new();
    let mut expected = String::new();
    let mut seed = 0x152u64;
    // Initial bursts overflow every class. Mixed traffic exercises preemption,
    // admission after draining and empty reads, followed by a complete drain.
    for item in 0..4300 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let put = item < 100 || (item < 4200 && seed % 5 < 3);
        let result = if put {
            let index = (seed % 4) as usize;
            operations.push_str(&format!("put {index} {item}\n"));
            match queues.try_push(TrafficClass::ALL[index], item) {
                Ok(()) => "ok".to_owned(),
                Err(rejected) => format!("full:{rejected}"),
            }
        } else {
            operations.push_str("get\n");
            queues
                .pop()
                .map_or_else(|| "empty".to_owned(), |value| format!("item:{value}"))
        };
        let snapshot = queues.snapshot();
        expected.push_str(&format!(
            "{result}|{}|{:?}|{:?}\n",
            snapshot.total, snapshot.heights, snapshot.dropped
        ));
    }
    assert_eq!(queues.snapshot().total, 0);
    assert!(queues.snapshot().dropped.iter().all(|count| *count > 0));
    let python =
        std::env::var("RNS_PYTHON_BIN").unwrap_or_else(|_| "/usr/bin/python3.11".to_owned());
    let reference =
        std::env::var("RNS_PYTHON_ROOT").unwrap_or_else(|_| "/home/room/src/Reticulum".to_owned());
    let mut child = Command::new(python)
        .arg("-B")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/inbound_queue_reference.py"
        ))
        .arg(reference)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start Python queue oracle");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(operations.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}
