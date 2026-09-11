# Stream ID compatibility

Reference: Python Reticulum 1.5.2, commit
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`, `RNS/Buffer.py`.

`StreamDataMessage::new` rejects IDs above `0x3fff` with `StreamIdError`,
matching Python's constructor. It returns `Result<Self, StreamIdError>`.
`StreamWriter::new` and `ChannelBuffer::new` return the same error before
any data or EOF frame can be emitted. Python's writer retains the supplied
ID and rejects it later when it constructs a message on write/close;
Rust validates earlier instead of silently selecting a different stream.
The writer's private ID remains validated for its lifetime.

`StreamReader::new` retains the supplied ID, as Python does. An out-of-range
reader cannot receive a valid wire stream by aliasing its low 14 bits.

Parsing still masks off EOF/compressed flag bits. Message fields remain
public and packing still masks the ID, as Python does after direct attribute
mutation; this change validates constructor input, not arbitrary later mutation.
Negative IDs are outside Rust's unsigned API.

Cross-implementation fixtures and tests live in `../reticulum-e2e-tests`:
`spec/rust/python/stream_id_reference.py` covers IDs 0, 16383, 16384 and 65535
with all EOF/compressed combinations through real Python pack/unpack calls.
`spec/rust/tests/reference_contract.rs` checks Rust constructor acceptance,
Python wire bytes, flag parsing and decompression, plus reader non-aliasing.
Existing Python-generated stream and compression vectors remain applicable.
EOF ordering and compression chunk selection are unchanged.
