use rns_runtime::rpc::{RpcResponse, encode_response};
use rns_transport::inbound_queue::{InboundQueueLimits, InboundQueues, TrafficClass};

#[test]
#[ignore = "requires Python reference checkout; configure RNS_PYTHON_ROOT and RNS_PYTHON_BIN"]
fn python_decodes_queue_stats_from_rust_rpc() {
    let mut queues = InboundQueues::new(InboundQueueLimits::new([2, 4, 8, 16]).unwrap());
    for (index, class) in TrafficClass::ALL.into_iter().enumerate() {
        for _ in 0..[3, 6, 11, 20][index] {
            let _ = queues.try_push(class, ());
        }
    }
    let encoded = encode_response(&RpcResponse::InterfaceStatsWithQueues(
        vec![],
        queues.stats(),
    ))
    .unwrap();
    let output = std::process::Command::new(
        std::env::var("RNS_PYTHON_BIN").unwrap_or_else(|_| "/usr/bin/python3.11".into()),
    )
    .args(["-B", "-c", include_str!("inbound_queue_rpc_receiver.py")])
    .arg(std::env::var("RNS_PYTHON_ROOT").unwrap_or_else(|_| "/home/room/src/Reticulum".into()))
    .arg(hex::encode(encoded))
    .output()
    .expect("start Python");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
