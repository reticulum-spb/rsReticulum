# IFAC compatibility on large-MTU interfaces

Reference: Python Reticulum 1.5.2, commit
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`, `Transport.handle_outgoing_ifac`,
`Transport.handle_ifac`, and `Cryptography.HKDF.hkdf`.

IFAC masks cover the complete frame including the tag. Negotiated TCP MTUs
can produce masks longer than 8160 bytes. Python wraps the one-byte HKDF block
counter after block 255 while retaining the previous digest in every block.
The IFAC implementation reproduces this wire expansion for both signing and
verification. It neither rejects valid large Python frames nor panics when
sending them. The general `rns_crypto::hkdf_sha256` key-derivation API retains
its RFC 5869 output limit; this extension is private to the IFAC wire codec.

Cross-implementation checks live in `../reticulum-e2e-tests`:
`spec/rust/python/ifac_large_reference.py` uses the real Python transport IFAC
codec with fixed interface key bytes and deterministic packet bytes.
`spec/rust/tests/ifac_large_python.rs` verifies those Python frames and compares
Rust-produced frames byte for byte. Cases include IFAC sizes 1, 16 and 64,
mask lengths 8159/8160/8161 and 16384, plus full-size 262144-byte TCP frames.
The live endpoint suite crosses an IFAC segment for packet-boundary requests,
large responses and Resource transfers in both directions.
