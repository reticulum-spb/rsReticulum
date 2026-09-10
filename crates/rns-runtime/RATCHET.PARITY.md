# Destination ratchet compatibility

Reference: Python Reticulum 1.5.2, commit
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`, `RNS/Destination.py`,
`RNS/Identity.py` and `RNS/Packet.py`.

`RegisteredDestination::enable_ratchets(enforce)` loads a signed private-key
ring from `<config_dir>/storage/ratchets/<destination_hash>`, creating an empty
ring when the file does not exist. It now returns a `Result`; callers must
handle storage failures. Invalid existing files are errors, not a reason to
silently replace keys.

An announce rotates an empty or due ring (default interval 1800 seconds).
The candidate ring is atomically stored before its public key is advertised.
A repeated cached path-request tag reuses its announce without rotating.
As in Python, enabling from a saved ring resets its age: the first subsequent
announce rotates while retaining the previous keys. Received SINGLE packets
try the retained ratchets; identity-key fallback is allowed only when
`enforce` is false. Proofs are sent only after successful decryption.

The retention and interval setters reject zero. The existing Rust
`RatchetRing` bound of 512 retained keys remains; Python accepts larger positive
counts. Changing retention trims the live keys; the next rotation persists
the trimmed ring. The application sets its retention and interval again on
startup, as these settings are not part of the stored key list.

Both normal and SQLite runtime configurations use the same private ratchet
files. This contract covers key lifecycle and network behaviour, not exchange
of private-key storage files between implementations.

`tests/ratchet_reference.py` in the sibling `reticulum-e2e-tests` pins enablement, enforcement, same-interval
announces, rotation, old-key decryption and reload using actual Python calls.
`tests/test_rsreticulum_ratchets.py` mirrors those behaviours across the
Python/Rust Docker stack, including a real Rust container recreation with a
persistent storage volume. Python packs packets while each key is current and
sends those packets after rotation or restart; Rust must decrypt and prove
retained-key packets and refuse identity-key and evicted-key packets.

The control operations and persistent storage volume live directly in that
repository's Python/Rust drivers and Compose file. Its `script/rsreticulum-interop`
runs the lifecycle scenario before the remaining network tests, rebuilding the
stack with `down -v` / `up --build`. The positive lifecycle check replaces the
obsolete assertion that Rust announces contain no ratchet.
Repeat with the whole workspace `--no-default-features --features=sqlite-bundled`,
`reticulum.sqlite_storage: true`, and SQLite `PRAGMA integrity_check` after
shutdown. Local preparation scripts and artifacts are not committed.
