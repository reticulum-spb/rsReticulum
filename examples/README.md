# Reticulum examples in Rust

This crate contains one executable counterpart for every Python program in
`Reticulum/Examples`. Run one with:

```text
cargo run -p rns-examples --bin minimal
cargo run -p rns-examples --bin channel
cargo run -p rns-examples --bin resource
```

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
