# Configuration parity

Reference: Python Reticulum 1.5.2, commit
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`, `RNS/Reticulum.py`
(`_synthesize_interface`).

Native YAML and Python's legacy configuration have different schemas.
There is no legacy config parser. YAML `ifac_size` remains a byte count in
1..=64; the explicit `InterfaceCommonConfig::import_python_ifac_size` helper
converts the Python field from bits, floors partial bytes, and leaves the
class default selected for absent values or values below 8. Values whose
converted size exceeds 64 bytes fail without changing the field. Python
accepts those larger sizes, so this bound remains an intentional difference.

Cross-implementation checks live in `../reticulum-e2e-tests`:

- `spec/rust/python/ifac_reference.py` constructs real Python UDP interfaces
  with fixed IFAC credentials and records the effective sizes and identity.
- `spec/rust/tests/ifac_config.rs` checks conversion against those observations.
- `spec/rust/tests/reference_contract.rs` pins the unchanged native YAML units
  and bounds. Network class defaults are checked separately from conversion.
- `tests/test_rsreticulum_interop.py::test_ifac_protects_the_python_to_rust_segment`
  verifies actual delivery through matching Python and Rust IFAC settings.
