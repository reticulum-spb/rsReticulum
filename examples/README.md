# Reticulum examples in Rust

This crate contains one executable counterpart for every Python program in
`Reticulum/Examples`. Run one with:

```text
cargo run -p rns-examples --bin minimal
cargo run -p rns-examples --bin channel
cargo run -p rns-examples --bin resource
```

By default, examples use the full Reticulum runtime. To build all binaries
as clients of an already-running shared Reticulum instance:

```text
cargo build -p rns-examples --bins --no-default-features --features=reticulum-client
cargo run -p rns-examples --no-default-features --features=reticulum-client --bin minimal
```

Cargo features are additive: omit `--no-default-features` to keep the full
runtime enabled. Client-only examples require a running shared instance.

The binaries are executable network examples that mirror the Python examples
as the public Rust application API reaches parity:

| Python example | Rust binary | Public API exercised |
| --- | --- | --- |
| Announce.py | `announce` | Identity, Destination, announce construction |
| Broadcast.py | `broadcast` | Plain Destination |
| Buffer.py | `buffer` | ChannelBuffer |
| Channel.py | `channel` | MessageBase, Channel |
| Echo.py | `echo` | Destination and proof strategy |
| ExampleInterface.py | `example_interface` | Interface API boundary |
| Filetransfer.py | `filetransfer` | Link packets and automatic Resource transfer |
| Identify.py | `identify` | Link creation |
| Link.py | `link` | Link creation |
| Minimal.py | `minimal` | Identity, Destination, announce construction |
| Ratchets.py | `ratchets` | Destination ratchet policy |
| Request.py | `request` | Link creation/request foundation |
| Resource.py | `resource` | Bidirectional, metadata-bearing, split Resource transfer |
| Speedtest.py | `speedtest` | Sustained Link packet throughput |
