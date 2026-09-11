# Link signalling compatibility

Reference: Python Reticulum 1.5.2, commit
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`, `RNS/Link.py`.

`SignallingData::new` returns `Result<Self, SignallingModeError>` and checks
`ENABLED_MODES` before applying the MTU wire mask. Only AES256-CBC (mode 1)
is enabled. Known disabled modes and unknown values, including values above
three bits, are rejected without folding them into another mode. Python's
`Link.signalling_bytes` raises `TypeError` for known disabled modes and
`KeyError` for unknown modes when formatting its error; Rust reports the
original mode value in one explicit error type.

The responder validates the request's parsed mode through the same constructor
before generating ephemeral keys or deriving session keys. A disabled mode
returns `HandshakeError::UnsupportedMode`; no proof is emitted. Python's
`Link.prove` likewise refuses to serialize signalling for disabled modes.
Legacy requests without signalling still select the enabled default mode.
AES128 key derivation and crypto primitives remain available independently.

The low-level wire parser and `pack` codec retain all three mode bits. Public
fields can still represent received or deliberately constructed disabled modes;
constructor validation is not a guarantee for arbitrary struct literals or
field mutation. The responder checks such wire input before using it.

Cross-implementation fixtures and tests live in `../reticulum-e2e-tests`:
`spec/rust/python/signalling_reference.py` invokes the real Python encoder for
mode and MTU boundaries; `spec/rust/tests/reference_contract.rs` mirrors the
acceptance decisions and exact bytes. The Python responder test calls real
`Link.prove` on disabled modes. Rust responder unit tests reject all disabled
three-bit modes, and existing property tests preserve raw codec coverage.
