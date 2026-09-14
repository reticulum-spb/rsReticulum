# Resource advertisement compatibility

Reference: Python Reticulum 1.5.2, commit
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`, `RNS/Resource.py`.

`ResourceAdvertisement::unpack` accepts a transfer size up to and including
`3 * MAX_EFFICIENT_SIZE` (3,145,725 bytes) and rejects larger values, matching
Python's advertisement parser. Integer fields use checked conversion from
MessagePack u64 to usize so a large value cannot wrap on a 32-bit target.

The runtime receive paths in LinkManager, LinkSession and rncp call this
parser before constructing an inbound transfer. The existing, stricter
`InboundResource::with_sdu` limits on encrypted segment size and part count
remain in effect before the parts array is allocated. Accepting a size in
an advertisement therefore does not promise that an inconsistent transfer
will be accepted. LinkManager follows its existing policy of closing the
link when advertisement parsing fails; this is not a claim that Python's
runtime closes the link in every resource-rejection case.

Cross-implementation checks live in `../reticulum-e2e-tests`:

- `spec/rust/python/resource_adv_reference.py` packs fixed real Python
  advertisements at the boundary and above it, including u64::MAX.
- `spec/rust/tests/reference_contract.rs` passes those bytes to the Rust parser.
- `tests/test_rsreticulum_interop.py` sends actual encrypted Python Resource
  advertisements over a live link and observes Rust's rejection, alongside
  normal 100 KB and segmented 1.5 MB transfers in both directions.

## Negotiated Resource part size

`Link::resource_sdu` matches Python's `Resource.sdu`: negotiated MTU minus
`HEADER_MAXSIZE` and one reserved IFAC byte. Link encryption covers the entire
Resource blob before splitting, so per-packet token overhead is not subtracted
from each part. LinkSession and LinkManager propagate this SDU to ordinary,
request/response, file and multi-segment sends. Inbound timing and throughput
accounting use the same SDU. Existing constructors retain their base-MTU defaults.

Resource advertisement/HMU hashmap segment size remains fixed at 74 hashes,
as in Reticulum 1.5.2, even when the negotiated Link MDU is larger.
`spec/rust/python/resource_mtu_reference.py` and
`spec/rust/tests/resource_mtu_python.rs` in the sibling test repository pin
these distinct sizes using real Python Link/Resource objects. Live tests force
Resource mode for small payloads and verify complete 100 KB/1.5 MB deliveries
in both directions; request boundaries use the established Link MDU.
