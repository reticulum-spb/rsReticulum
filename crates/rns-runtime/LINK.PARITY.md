# Link request compatibility

Reference: Python Reticulum 1.5.2, commit
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`, `RNS/Link.py` and `RNS/Resource.py`.

`LinkSession::request` chooses the transport from the size of the packed
request, including its timestamp, path hash and MessagePack framing. Requests
at or below the negotiated Link MDU use REQUEST packets and the transmitted
packet hash as their request ID. Larger requests use Resource transfers with
the packed plaintext hash as the request ID. The transfer and response share
one caller deadline.

`LinkManager` dispatches completed request resources through the same request
handlers as REQUEST packets. Ordinary resource callbacks do not receive request
bodies. Each segment is proved before dispatch; a split request is dispatched
only after complete reassembly. Responses retain the existing packet/Resource
selection and request ID correlation.

Cross-implementation coverage lives in `tests/test_rsreticulum_resource_requests.py` in the sibling `reticulum-e2e-tests`:
Python and Rust each send a request at the Python-derived packet boundary, one
byte above it, and at 100,000 bytes. The peer's echo response must match the full
length and SHA-256 digest. The surrounding endpoint suite also checks small
requests, large responses and ordinary resources through 1,500,000 bytes.
The new tests require the Python/Rust Docker stack from the sibling
`reticulum-e2e-tests` repository; they call its client APIs and do not implement
Resource transport in the drivers.

Example against an already prepared and running stack (from this repository):

```bash
PYTHONPATH=../Reticulum:../reticulum-e2e-tests/tests python -m pytest \
  ../reticulum-e2e-tests/tests/test_rsreticulum_resource_requests.py -v
```

Use a fresh source snapshot and rebuild the stack for each run. Repeat with
workspace `--no-default-features --features=sqlite-bundled` and
`reticulum.sqlite_storage: true`. Local preparation scripts and run artifacts
are intentionally not committed.
