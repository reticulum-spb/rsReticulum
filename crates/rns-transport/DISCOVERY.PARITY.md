# Discovery compatibility

Reference: Python Reticulum 1.5.2, commit
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`, `RNS/Discovery.py`.

## Optional discovery fields (RUST-S12)

The decoder accepts the reference's unconfigured announce: a nil name becomes
`Discovered <interface type>`, and nil latitude, longitude and height remain
absent. These keys must be present even when their values are nil. Python
accepts floating-point coordinates and rejects integers and booleans. Its name
sanitizer applies the fallback to false values before attempting string
operations; the Rust decoder preserves this rule.

`DiscoveryInfo` and `DiscoveryInterfaceConfig` represent coordinates as `Option<f64>`.
Callers migrating from the earlier API should wrap configured coordinates in
`Some` and use `None` for an unset location. Storage preserves nil values;
`rnstatus` reports an absent location as N/A and JSON null, while zero remains
a real coordinate. Existing files containing floating-point coordinates remain
readable.

`interop_tests/discovery_reference.py` generates stable maps with the real
Python announcer and records acceptance and field values from the real Python
receiver. The malformed-map checks retain real stamp validation with an explicit
zero-cost requirement to isolate decoding. The committed fixture excludes
random stamps and receive timestamps. `tests/discovery_parity.rs` feeds every
map to the public Rust decoder and checks the Python result, including populated
and operator announces.

From the repository root:

```bash
PYTHONPATH=../Reticulum python interop_tests/discovery_reference.py --check
cargo test --workspace discovery
cargo test --workspace --no-default-features --features=sqlite-bundled discovery
```

The fixture generator requires the sibling `reticulum-e2e-tests` checkout for
its existing Python announcer setup (override with `--suite`).

## Stamp defaults (RUST-S9)

The producer default and runtime receiver minimum are both 16. YAML continues
to accept an explicit minimum; the runtime default refers to the shared
transport constant to prevent the two defaults diverging again.

`crates/rns-runtime/tests/discovery_python.rs` passes the actual public Rust
settings to `interop_tests/discovery_stamp_reference.py`. The Python test
first establishes acceptance with the real receiver and stamps of exactly
15 and 16 leading zero bits, then checks the Rust settings against that
boundary and an explicit minimum of 15. It does not assume that mining with
a target of 14 always produces a stamp below 16.

This opt-in cross-process test requires the reference Python environment:

```bash
PARITY_PYTHON=/path/to/reference/python PYTHONPATH=../Reticulum \
  cargo test --workspace discovery_stamp_defaults_match_python_receiver -- --ignored --nocapture
PARITY_PYTHON=/path/to/reference/python PYTHONPATH=../Reticulum \
  cargo test --workspace --no-default-features --features=sqlite-bundled \
  discovery_stamp_defaults_match_python_receiver -- --ignored --nocapture
```

The Rust PoW implementation is supplied by the embedding application through
`DiscoveryStamper`. These checks establish the configured costs and the Python
acceptance boundary; they do not exercise a live Rust discovery publisher.
Without an installed stamper, runtime discovery remains inactive.

## Operator address and implementation metadata (RUST-S10)

The codec publishes `TRANSPORT_IMPL=0xFD` as `rsReticulum` and
`TRANSPORT_VERS=0xFC` as the crate version. These informational fields are
emitted independently of received metadata; Python itself does not surface
them from its receiver. Map ordering is not part of the contract.

`OP_ADDR=0xF0` is represented by `operator_address: Option<[u8; 16]>` in the
codec and interface announce configuration. Incoming nil and byte strings of
any other length produce no operator address; non-byte, non-nil values reject
the announce, matching Python. The operator address survives discovery storage
and appears in `rnstatus --json` as `operator_lxmf_address`. The runtime's
Reticulum-style interface config reads `discovery_lxmf_address` as hex.

The Python generator covers 30 decoder cases including operator lengths and
invalid types. `discovery_operator_address_survives_python_to_rust` checks the
Python address through the Rust codec; `discovery_metadata_reaches_python`
passes Rust output to the real Python receiver and verifies the address and
published implementation/version fields. Run both directions with the same
reference environment as S9:

```bash
PARITY_PYTHON=/path/to/reference/python PYTHONPATH=../Reticulum \
  cargo test --workspace discovery -- --include-ignored
PARITY_PYTHON=/path/to/reference/python PYTHONPATH=../Reticulum \
  cargo test --workspace --no-default-features --features=sqlite-bundled \
  discovery -- --include-ignored
```
