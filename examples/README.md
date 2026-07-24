# Reticulum examples in Rust

This crate contains one executable counterpart for every Python program in
`Reticulum/Examples`. Run one with:

```text
cargo run -p rns-examples --bin minimal
cargo run -p rns-examples --bin channel
cargo run -p rns-examples --bin resource
```

The binaries are intentionally small compile-time API probes. They cover the
public primitives that are currently usable by an application:

| Python example | Rust binary | Public API exercised |
| --- | --- | --- |
| Announce.py | `announce` | Identity, Destination, announce construction |
| Broadcast.py | `broadcast` | Plain Destination |
| Buffer.py | `buffer` | ChannelBuffer |
| Channel.py | `channel` | MessageBase, Channel |
| Echo.py | `echo` | Destination and proof strategy |
| ExampleInterface.py | `example_interface` | Interface API boundary |
| Filetransfer.py | `filetransfer` | Resource segmentation/reassembly |
| Identify.py | `identify` | Link creation |
| Link.py | `link` | Link creation |
| Minimal.py | `minimal` | Identity, Destination, announce construction |
| Ratchets.py | `ratchets` | Destination ratchet policy |
| Request.py | `request` | Link creation/request foundation |
| Resource.py | `resource` | Resource segmentation/reassembly |
| Speedtest.py | `speedtest` | Resource data path |

## API gaps found by the port

The protocol implementation is substantially more complete than its public
application API. The following Python-level operations do not currently have
a general Rust counterpart:

- registering an application Destination with a running Reticulum instance;
- `Destination.announce()` (construction exists, dispatch is manual);
- `Packet.send()` and receipt lifecycle orchestration;
- registering filtered announce handlers;
- recalling an announced remote Identity through a stable public API;
- creating and driving a general outbound Link session;
- attaching Channel, Buffer, Request and Resource callbacks to such a session;
- registering a custom interface from application code.

Some internal runtime components implement these flows for individual tools
(`rnsh-rs`, `rncp-rs`, `rnprobe-rs`), but that orchestration is not exposed as
a reusable application API. Consequently these examples validate and
demonstrate the available layers without claiming end-to-end network behavior.
