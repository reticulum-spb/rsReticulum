//! Receive attacker-controlled bytes through a real socket before invoking the
//! endpoint decoders. Transport encryption is orthogonal to these size checks.
use rns_protocol::resource::*;
#[test]
fn network_part_cannot_exceed_advertised_resource_size() {
    let receiver = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let data = vec![0x55; 128];
    let random = [1; RANDOM_HASH_SIZE];
    let mut resource = InboundResource::new(
        1,
        32,
        32,
        random,
        [2; 32],
        ResourceFlags::default(),
        vec![get_map_hash(&data, &random)],
    )
    .unwrap();
    sender
        .send_to(&data, receiver.local_addr().unwrap())
        .unwrap();
    let mut buf = [0; 256];
    let (n, _) = receiver.recv_from(&mut buf).unwrap();
    assert!(
        !resource.receive_part(buf[..n].to_vec()),
        "accepted more bytes than advertised"
    );
}

#[test]
fn unread_stream_has_a_byte_limit() {
    use rns_protocol::{buffer::StreamReader, stream_data::StreamDataMessage};
    let mut stream = StreamReader::new(1);
    let message = StreamDataMessage::new(1, vec![1; 16384], false).unwrap();
    for _ in 0..128 {
        stream.feed(&message);
    }
    assert!(stream.available() <= 1024 * 1024, "unbounded unread stream");
}
