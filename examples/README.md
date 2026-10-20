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

The `echo` binary runs a server when `--destination` is omitted. It creates
`~/.rsEcho` and stores its persistent private identity in `identity`, keeping
the server destination address stable across restarts. Requests are appended
to `requests.log` using the same `tracing` formatter as `rnsd-rs --service`,
with readable UTC timestamps and no ANSI colors. Entries include the packet
hash, interface ID, payload size, and proof-sent status. Runtime messages at
INFO level and above are also written to this file.
Use `--data-dir PATH` to choose another storage directory; `--config PATH`
selects the Reticulum configuration directory independently.

```text
cargo run -p rns-examples --bin echo -- --help
cargo run -p rns-examples --bin echo -- --data-dir /tmp/rsEcho
cargo run -p rns-examples --bin echo -- --destination <destination-hash>
```

The server creates `config.yaml` in its data directory if it is missing:

```yaml
announce_interval: 300
app_name: example_utilities.echo.request
```

`announce_interval` is a positive integer in seconds. The server announces
immediately on startup, then periodically without requiring keyboard input.
Restart echo after changing the configuration. Missing fields use the defaults
shown above. `app_name` applies to the server, client and `--identity`; changing
it changes the destination address even with the same saved identity. Configure
the same `app_name` on the client and server.

Client mode sends one
request and prints the round-trip time without creating server storage.
Use `echo --identity` (optionally with `--data-dir PATH`) to print only the saved
server destination address and exit without starting Reticulum or writing files.
The identity must already exist; this option cannot be combined with `--destination`.
When reception metrics are available, the server records `rssi_dbm` and `snr_db`
in its log and prints RSSI/SNR to the console. The client displays RSSI/SNR
for the received proof reply alongside the round-trip time. These are local
reception measurements, not measurements sent back by the remote server.
Shared-instance clients obtain them from the daemon through control RPC.
Unavailable metrics are omitted.

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
