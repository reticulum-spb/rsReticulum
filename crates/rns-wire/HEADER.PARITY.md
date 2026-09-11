# Packet header compatibility

Reference: Python Reticulum 1.5.2, commit
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`, `RNS/Packet.py`.

`PacketHeader::pack` returns `Result<Vec<u8>, HeaderError>` and rejects
HEADER_2 without a transport ID with `MissingTransportId`. `Packet::new`
propagates this through `PacketError::Header` before hashing or constructing
the packet. HEADER_1 ignores any supplied transport ID, matching Python;
`size()` follows the selected header type (19 or 35 bytes).

Callers that accept arbitrary headers must handle the error. Internal packet
builders assert their locally constructed invariants (HEADER_1, or HEADER_2
with an explicitly supplied ID); fallible packet and forwarding paths
propagate failure. No malformed header bytes are returned on error.

Cross-implementation tests are in `../reticulum-e2e-tests`:
`spec/rust/python/header_reference.py` packs all four combinations through
real Python APIs, and `spec/rust/tests/reference_contract.rs` mirrors them.
Existing packet vectors and the two-transport Docker topology cover valid
headers, forwarding and hash preservation.
