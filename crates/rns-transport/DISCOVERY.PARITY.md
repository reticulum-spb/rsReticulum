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

`DiscoveryInfo` and `AnnounceConfig` represent coordinates as `Option<f64>`.
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
its existing Python announcer setup (override with `--suite`). Stamp defaults
(RUST-S9) and operator/implementation metadata (RUST-S10) remain separate gaps;
this change does not close them.
