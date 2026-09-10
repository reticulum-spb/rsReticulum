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
its existing Python announcer setup (override with `--suite`). Operator/implementation metadata (RUST-S10) remains a separate gap.

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
