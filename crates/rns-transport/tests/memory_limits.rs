use rns_transport::{constants::*, hashlist::PacketHashlist, reverse_table::ReverseTable};
#[test]
fn reverse_admission_is_bounded_and_existing_proofs_remain_routable() {
    let mut table = ReverseTable::new();
    for n in 0..MAX_REVERSE_ENTRIES + 100 {
        table.insert((n as u128).to_be_bytes(), 1, 2);
    }
    assert_eq!(table.len(), MAX_REVERSE_ENTRIES);
    table.insert(0u128.to_be_bytes(), 7, 3);
    assert_eq!(table.get(&[0; 16]).unwrap().receiving_interface, 7);
}
#[test]
fn persisted_dedup_history_cannot_override_capacity() {
    let mut hashes = PacketHashlist::new_with_capacity(10);
    hashes.load_from(
        (0u64..100)
            .map(|n| {
                let mut h = [0; 32];
                h[..8].copy_from_slice(&n.to_be_bytes());
                h
            })
            .collect(),
    );
    assert!(hashes.len() <= 5);
    for n in 0u64..100 {
        let mut h = [0; 32];
        h[8..16].copy_from_slice(&n.to_be_bytes());
        hashes.insert(h);
    }
    assert!(hashes.len() <= 10);
}
