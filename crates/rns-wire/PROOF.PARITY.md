# Packet proof compatibility

Reference: Python Reticulum 1.5.2, commit
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`, `RNS/Packet.py` and `RNS/Link.py`.

S6 compared different APIs and was not a Link proof incompatibility.
Python's general PacketReceipt.validate_proof and Rust's
rns_wire::receipt::PacketReceipt::validate_proof both accept a valid 64-byte
implicit signature or a 96-byte explicit hash-plus-signature proof.
Python's PacketReceipt.validate_link_proof and Rust's
rns_link::link::Link::validate_packet_proof both reject 64-byte proofs and
accept the explicit form after checking the expected hash and peer signature.
Both Link validators also accept a valid 96-byte prefix with extra trailing
bytes; the general validators require an exact supported length.

Actual Rust Link paths use Link::validate_packet_proof: LinkManager's
handle_link_packet_proof first rejects lengths below 96, and LinkClient's
proof reception paths call the Link validator. They do not use the general
receipt validator. Python Link.prove_packet and Rust Link's prove_packet
variants emit 96-byte hash-plus-signature bodies. No receiver or crypto
behaviour needs changing, and the S6 entry is removed from the gap registry.

Cross-implementation tests live in ../reticulum-e2e-tests:
- spec/rust/python/proof_reference.py builds actual Python receipts and Link
  objects, signs fixed hashes using real Identity APIs, and checks 11 cases.
- general_and_link_proof_apis_match_python in spec/rust/tests/reference_contract.rs
  validates those same proofs with Rust's general receipt and a real Link
  responder whose peer key comes from the Python request.
- Cases include explicit/implicit proofs, wrong hash, wrong signature, wrong
  peer key, short inputs and an extended explicit proof.
- Live Python/Rust Link packet tests verify delivery with proofs in both
  directions; the endpoint suite also covers Link identification and resources.
