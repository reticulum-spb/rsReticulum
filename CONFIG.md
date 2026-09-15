# rsReticulum configuration reference

rsReticulum reads exactly one configuration file from its configuration
directory: `config.yaml`. The former ConfigObj/INI file named `config` is not
read, detected, converted, or used as a fallback.

The loading pipeline is:

```text
YAML parse -> Serde defaults/normalization -> semantic validation -> runtime
```

Unknown fields are errors everywhere except inside `plugin.config`. Field names
are case-sensitive and use `snake_case`. Ports are integers in `1..=65535`
unless a field is explicitly optional. Interface names must be non-empty and
unique, including disabled interfaces.

When rsReticulum or the Web UI writes the file, fields equal to their documented
defaults are omitted. Required identity and discriminator fields such as
interface `type` and `name` are always retained. Reading the compact form
restores all omitted values from the same defaults listed below.

Validate a file without starting Reticulum:

```bash
rnsd-rs --check /path/to/config.yaml
```

Print the maintained example:

```bash
rnsd-rs --exampleconfig
```

## Top-level document

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `reticulum` | mapping | `{}` | Core runtime settings. |
| `storage` | mapping | `{}` | SQLite transport storage settings; used with `reticulum.sqlite_storage`. |
| `logging` | mapping | `{}` | Logging settings. |
| `api` | mapping | `{}` | Optional Web API settings. |
| `interfaces` | sequence | `[]` when explicitly omitted during parsing; the generated default file contains one Auto interface | Built-in and future plugin interfaces. |

Minimal file:

```yaml
reticulum: {}
interfaces: []
```

## `reticulum`

| Field | Type | Default | Valid values and meaning |
| --- | --- | --- | --- |
| `sqlite_storage` | boolean | `false` | Store transport state in SQLite instead of the legacy backend; requires a build with `sqlite` or `sqlite-bundled`. Shared clients use their daemon's storage. |
| `share_instance` | boolean | `true` | Join or provide the local shared Reticulum instance. |
| `instance_name` | string | `default` | Shared-instance namespace. Must be suitable for the platform socket path. |
| `shared_instance_type` | enum | `platform_default` | `tcp`, `unix`, or `platform_default`. |
| `shared_instance_port` | integer | `37428` | Shared TCP data port, `1..=65535`. |
| `instance_control_port` | integer | `37429` | Shared RPC/control TCP port, `1..=65535`. |
| `enable_transport` | boolean | `false` | Enable transport-node behavior. |
| `static_transport_identity` | boolean | `false` | Reuse a stable wire-facing transport identity when transport is disabled. |
| `local_hops_delta` | boolean | `false` | Apply the runtime hop offset to locally originated packets. |
| `respond_to_probes` | boolean | `false` | Enable probe responses. |
| `use_implicit_proof` | boolean | `true` | Prefer implicit delivery proofs where the protocol permits them. |
| `panic_on_interface_error` | boolean | `false` | Fail startup when an enabled interface cannot be created. |
| `link_mtu_discovery` | boolean | `true` | Enable link MTU discovery. |
| `enable_remote_management` | boolean | `false` | Enable remote-management handlers. |
| `remote_management_allowed` | sequence of strings | `[]` | Allowed identity hashes. Each value is exactly 32 hexadecimal characters (16 bytes). |
| `rpc_key` | string or null | `null` | Hexadecimal RPC authentication key. When absent, runtime derives the normal key. |
| `force_shared_instance_bitrate` | integer or null | `null` | Positive simulated shared-instance bitrate in bits/s; affects Local TX pacing and automatic MTU. Zero is rejected. |
| `default_ar_target` | integer or null | `null` | Default minimum interval between announces for one destination, in seconds, on transport nodes; unset/`0` falls back to `3600`. |
| `default_ar_penalty` | integer or null | `null` | Extra blocking time in seconds after excessive announce-rate violations; transport default `0`. |
| `default_ar_grace` | integer or null | `null` | Tolerated announce-rate violations, `0..=4294967295`; transport default `5`. |
| `default_gravity` | signed integer | `0` | Default route preference for configured interfaces. Negative values are valid. |
| `qlen_in_data` | positive integer | `1024` | Admitted DATA/proof/link packet queue capacity, in packets. |
| `qlen_in_announce` | positive integer | `128` | Admitted announce queue capacity, in packets. |
| `qlen_in_pr` | positive integer | `128` | Admitted path-request queue capacity, in packets. |
| `qlen_in_il` | positive integer | `8` | Ingress-limited path-request and released held-announce queue capacity, in packets. |
| `autoconnect_interface_mode` | interface mode or null | `null` | Override discovered-connection mode; absent means `gateway` on transport nodes, otherwise `full`. |
| `autoconnect_interface_gravity` | signed integer or null | `null` | Discovered-connection gravity; absent means `0`, independently of `default_gravity`. |
| `autoconnect_announces_to_internal` | boolean | `false` | Set source-side `announces_to_internal: true` on discovered connections; false leaves the mode policy unchanged. |
| `ingress` | mapping | `{}` | Global ingress/egress-control overrides; see below. |
| `network_identity` | path or null | `null` | Optional network identity file. A leading `~/` is expanded using the user home directory. |
| `discover_interfaces` | boolean | `false` | Receive discovery announces; publication is independently enabled by per-interface `discoverable`. |
| `autoconnect_discovered_interfaces` | integer | `0` | Maximum discovered interfaces to connect automatically; `0` disables autoconnect. |
| `required_discovery_value` | integer | `16` | Required discovery stamp value, `0..=255`. |
| `interface_discovery_sources` | sequence of strings | `[]` | Accepted discovery publisher identity hashes; each is 32 hexadecimal characters. |
| `blackhole_sources` | sequence of strings | `[]` | Blackhole manifest publisher hashes; each is 32 hexadecimal characters. |
| `publish_blackhole` | boolean | `false` | Publish the local blackhole table. |
| `blackhole_update_interval_minutes` | number | `60.0` | Blackhole-source refresh interval in minutes. Runtime clamps it to at least two minutes. |
| `bootstrap_configs` | sequence of paths | `[]` | Additional bootstrap configuration paths consumed by discovery/bootstrap logic. |

The four `qlen_in_*` settings apply at startup to both the full runtime and
shared clients; changing them requires a runtime restart. Each queue is FIFO,
with strict priority DATA → announce → path request → ingress-limited. A full
queue drops its incoming packet without evicting another class. Capacities are
packet counts, not bytes, and do not resize the separate raw interface channel
(4096 messages) or control channel (256 messages). Buffers grow as needed rather
than reserving their maximum size at startup; large limits can still consume
substantial memory under load. All values and their sum must fit platform
`usize`. Zero and negative queue capacities are configuration errors.

### Ingress mappings

`reticulum.ingress` supplies global defaults; an interface's `ingress` overrides
individual fields. Missing or `null` fields inherit the global value, then the
built-in default shown below. Frequency is packets/second; durations are seconds.
These settings limit announce and path-request bursts, not ordinary DATA bitrate.

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `burst_freq_new` | number or null | `null` → `3` | Announce burst threshold while the interface is new. |
| `burst_freq` | number or null | `null` → `10` | Announce burst threshold after `new_time`. |
| `path_request_burst_freq_new` | number or null | `null` → `3` | Incoming path-request burst threshold while new. |
| `path_request_burst_freq` | number or null | `null` → `8` | Incoming path-request burst threshold after `new_time`. |
| `new_time` | number or null | `null` → `7200` | Interface age below which the new-interface thresholds apply. |
| `burst_hold` | number or null | `null` → `15` | Minimum burst hold and quiet recovery period before clearing the burst state. |
| `burst_penalty` | number or null | `null` → `15` | Additional delay before releasing held announces after burst activation. |
| `max_held_announces` | integer or null | `null` → `256` | Maximum distinct destinations whose announces may be held. |
| `held_release_interval` | number or null | `null` → `5` | Minimum interval between released held announces. |
| `egress_path_request_freq` | number or null | `null` → `5` | Outgoing path-request frequency threshold when egress control is enabled. |
| `egress_control` | boolean or null | `null` → `false` | Enable outgoing path-request frequency limiting; independent of `ingress_control`. |

## `storage`

Used when `reticulum.sqlite_storage: true`. Paths are relative to the
configuration directory unless absolute; a leading `~/` is expanded.

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `database_path` | path or null | `null` | SQLite database file; default `storage/sqlite/transport.sqlite` under the config directory. |
| `page_cache_size` | integer | `1024` | Page-cache budget in KiB; runtime clamps to `16..=16384`. |
| `wal_checkpoint_pages` | integer | `128` | Automatic WAL checkpoint threshold in pages; runtime clamps to `64..=4096`. Does not change the page-cache budget. |
| `announce_batch_delay_ms` | integer | `10` | Ordinary announce accumulation window in milliseconds (`1..=1000`). Starts with the first staged write and does not slide as more arrive. Barriers may flush earlier; commit and scheduling can add delay. Limits remain 32 mutations / 64 KiB, with the existing exception for a single large mutation. Restart required. |
| `vacuum_interval` | integer | `3600` | Maintenance interval in seconds; runtime enforces at least `60`. |
| `vacuum_pages` | integer | `128` | Maximum free pages reclaimed per incremental vacuum; `0` disables reclamation, not WAL checkpointing. |

## `logging`

| Field | Type | Default | Valid values and meaning |
| --- | --- | --- | --- |
| `level` | integer | `4` | Log level `0..=8`; `7` = Pathing, `8` = Extreme. |
| `timestamps` | boolean | `true` | Include timestamps in formatted logs. |
| `filter` | string | unset | Standard target filter, e.g. `info,rns_interface::plugin=debug`. `RUST_LOG` overrides this value. See [Logging](README.md#logging) for syntax and precedence. |
| `rss_interval` | unsigned integer | `300` | Seconds between process RSS logs; `0` disables sampling. Linux/Android, owning runtime only (not attached clients). Requires INFO to be enabled for `rns_runtime::process_memory` by the active filter or verbosity level. |

RSS entries (`process memory`) contain `rss_kib` and, when available, `peak_rss_kib` for the entire process. The first sample is taken after one interval. Values come from `/proc/self/status` and are approximate; the peak covers the process lifetime. Sampling uses a fixed 16 KiB buffer and retains no history. Configuration changes take effect after restart.

`rnsd-rs` maps levels 0–1 to ERROR, 2 to WARN, 3–4 to INFO, 5–6 to DEBUG,
and 7–8 to TRACE. Pathing and Extreme are distinct stored configuration values
but produce the same tracing level. Negative levels are not supported.

## `api`

| Field | Type | Default | Valid values and meaning |
| --- | --- | --- | --- |
| `port` | integer or null | `null` | Web API listen port. `null` disables the API. |
| `user` | string or null | `null` | Required and non-empty when `port` is set. |
| `password` | string or null | `null` | Required and non-empty when `port` is set. |

The API is compiled only with the `api` Cargo feature. Web UI writes are
atomic. Before a mutation, the previous file is saved as
`config.yaml.web-ui.bak`.

## Interface list and common fields

Every item in `interfaces` is selected by its required `type` discriminator.
Every built-in item accepts the following common fields at the same mapping
level as `type`:

| Field | Type | Default | Valid values and meaning |
| --- | --- | --- | --- |
| `name` | string | none | Required, non-empty, unique interface name. |
| `enabled` | boolean | `true` | Whether runtime creates the interface. Disabled entries are still parsed and validated. |
| `mode` | enum | `full` | `full`, `point_to_point`, `access_point`, `roaming`, `boundary`, `gateway`, or `internal`. |
| `outgoing` | boolean | `true` | Permit normal outbound traffic. |
| `bitrate` | integer or null | `null` | Configured bitrate in bits/s. |
| `announce_cap` | number or null | `null` | Announce cap as a percentage in `(0, 100]`; normalized to a fraction for runtime. |
| `announce_rate_target` | integer or null | `null` | Minimum interval in seconds between announces for one destination; overrides the transport default. |
| `announce_rate_grace` | integer or null | `null` | Grace count. When a target exists and grace is absent, runtime uses `0`. |
| `announce_rate_penalty` | integer or null | `null` | Extra blocking duration in seconds. When a target exists and penalty is absent, runtime uses `0`. |
| `ifac_network_name` | string or null | `null` | IFAC network name. |
| `ifac_passphrase` | string or null | `null` | IFAC passphrase. |
| `ifac_size` | integer or null | `null` | IFAC size in bytes, `1..=64`; interface-class default is used when absent. |
| `ingress_control` | boolean | `true` | Enable ingress control for this interface. |
| `ingress` | mapping | `{}` | Per-interface overrides listed in “Ingress mappings”. |
| `recursive_path_requests` | boolean | `false` | Force recursive path requests. |
| `announces_from_internal` | boolean | `true` | Permit rebroadcast of announces learned from internal interfaces. |
| `gravity` | signed integer or null | `null` | Inherit `reticulum.default_gravity` when absent; explicit `0` overrides that default. Accepted TCP/Backbone and Auto children inherit their parent's gravity. |
| `announces_to_internal` | boolean or null | `null` | `true` permits this interface's boundary-origin announces on internal egress. False/null retain mode policy (not a blanket deny). Accepted children keep null. |

Higher gravity wins ties between equally dated announces when the candidate has no
more hops than the current route. It does not override announce freshness or
prefer longer routes of the same age. A boundary interface searches unknown
paths through boundary/gateway interfaces; `recursive_path_requests: true`
removes this mode restriction. Existing ingress and egress limits still apply.

## Discovery publication

These fields belong directly to each interface mapping, alongside `type` and
`name`. Receiving discovery uses `reticulum.discover_interfaces`; publication
uses `discoverable` independently. Autoconnect accepts only permitted sources
and currently non-blackholed, non-expired records.

| Field | Default / meaning |
| --- | --- |
| `discoverable` | `false`; publish a supported interface |
| `discovery_name` | Interface name |
| `reachable_on` | Advertised address or executable path; a non-wildcard listen address is a Rust fallback |
| `announce_interval` | `360` minutes; clamped to a minimum of `5` |
| `discovery_stamp_value` | `16`; `0` selects default; range `0..=255` |
| `publish_ifac` | `false`; include IFAC name and passphrase in discovery; this reveals the credentials to discovery recipients |
| `discovery_encrypt` | `false`; requires `reticulum.network_identity` |
| `discovery_lxmf_address` | Optional 32-character hexadecimal operator address |
| `latitude`, `longitude`, `height` | Optional finite decimal degrees / metres above sea level |
| `location_cmd` | Executable path returning `LAT,LON,HEIGHT` |
| `discovery_frequency`, `discovery_bandwidth` | Optional integer Hz; RNode falls back to its radio settings |
| `discovery_modulation` | Optional string for KISS metadata |
| `bootstrap_only` | `false`; mark a temporary bootstrap interface |
| `ignore_config_warnings` | `false`; preserve an otherwise automatically corrected mode |

Modes `gateway`, `access_point` and `internal` are preserved. Other discoverable
interfaces become `gateway` (RNode becomes `access_point`) unless
`ignore_config_warnings` is true. KISS-framed TCP clients advertise `KISSInterface`.
Auto/UDP/Local/Pipe are not publication types. Configure `reachable_on`
explicitly for I2P.

On Unix, commands execute directly without shell arguments and support `~/`.
They run when publication is due. Missing/non-executable location paths preserve
static coordinates. Invalid output or nonzero exit skips publication for that
interface; others continue. Commands are bounded to five seconds and 4096 stdout
bytes. Command output latitude must be in `[-90,90]`, longitude in `[-180,180]`,
and all three values must be finite. Windows uses static metadata.
`discovery_modulation` is a string, not a numeric code.

With `discovery_encrypt: true`, an unavailable network identity suppresses
publication; it never falls back to publishing the data in plaintext.

```yaml
interfaces:
  - type: backbone
    name: Public relay
    block_fast_flapping: true
    fast_flapping_threshold: 20       # seconds
    fast_flapping_grace: 5
    fast_flapping_block_time: 720     # minutes
    listen_on: 0.0.0.0
    port: 4242
    mode: internal
    discoverable: true
    reachable_on: relay.example.org
    announce_interval: 360
    discovery_lxmf_address: "0123456789abcdef0123456789abcdef"
```

## `type: auto`

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `group_id` | string | `reticulum` | Discovery group identifier. |
| `discovery_scope` | enum | `admin` | `link`, `admin`, `site`, `organisation`, or `global`. |
| `discovery_port` | integer | `29716` | `1..=65535`. |
| `data_port` | integer | `42671` | `1..=65535`. |
| `multicast_address_type` | enum | `temporary` | `permanent` or `temporary`. |
| `devices` | sequence of strings or null | `null` | Optional allow-list of network devices. |
| `ignored_devices` | sequence of strings | `[]` | Device deny-list. |
| `configured_bitrate` | integer or null | `null` | Optional advertised bitrate. |

## `type: tcp_client`

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `target_host` | string | none | Required and non-empty; hostname or IP address. |
| `target_port` | integer | none | Required, `1..=65535`. |
| `kiss_framing` | boolean | `false` | Use KISS instead of HDLC framing. |
| `connect_timeout` | integer | `5` | Initial connection timeout in seconds. |
| `max_reconnect_tries` | integer or null | `null` | Retry limit; `null` retries indefinitely. |
| `fixed_mtu` | integer or null | `null` | Fixed MTU metadata, `500..=4294967295`; does not imply receive-buffer or local Link support for the entire range. |

`fixed_mtu` sets the client's frame MTU; otherwise the driver selects it from
bitrate. The receive limit includes the active IFAC size. Oversized frames are
dropped, so configure an MTU compatible with the peer. This field does not
force Link negotiation to use a larger MTU than the next hop supports.

## `type: tcp_server`

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `listen_ip` | string | `0.0.0.0` | Bind address. |
| `listen_port` | integer | none | Required, `1..=65535`. |
| `kiss_framing` | boolean | `false` | Use KISS framing. |
| `prefer_ipv6` | boolean | `false` | Prefer an IPv6 address when resolving `device`. |
| `device` | string or null | `null` | Bind using an address from this network device. |

## `type: udp`

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `listen_ip` | string or null | `null` | Receive bind address. |
| `listen_port` | integer or null | `null` | Receive port, `1..=65535` when set. |
| `forward_ip` | string or null | `null` | Destination address. |
| `forward_port` | integer or null | `null` | Destination port, `1..=65535` when set. |
| `device` | string or null | `null` | Device used to derive missing IPv4 broadcast addresses. |

At least one of `listen_port` and `forward_port` is required.

## `type: local`

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `port` | integer | `37428` | Local shared-instance port, `1..=65535`. |

`reticulum.force_shared_instance_bitrate` applies to automatically created
shared-instance endpoints, over both Unix sockets and TCP. It sets advertised
bitrate, selects automatic MTU and paces outbound packets. It does not alter
explicit `type: local` entries. With no override there is no artificial delay.

## `type: i2p`

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `connectable` | boolean | `false` | Accept inbound I2P streams. |
| `peers` | sequence of strings | `[]` | I2P peer destinations. |
| `sam_host` | string | `127.0.0.1` | SAM API host. |
| `sam_port` | integer | `7656` | SAM API port, `1..=65535`. |

## `type: pipe`

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `command` | string | none | Required and non-empty. The child process transports frames over stdio. |
| `respawn_delay` | integer | `5` | Delay after EOF before respawn, in seconds. |

## `type: backbone`

`bitrate` affects automatic MTU selection; the default is 100 Mbit/s with an
MTU of 32768 bytes. Accepted peers inherit the listener's bitrate and MTU.
Below 62500 bit/s, the driver uses a 500-byte fallback. Adaptive byte-buffer
and DATA-pressure control is automatic, with no separate YAML threshold keys;
the common `ingress` mapping concerns announce/path-request frequency control.

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `listen_on` | string or null | `null` | Optional listen address. |
| `target_host` | string or null | `null` | Presence selects client mode; absence selects listener mode. |
| `port` | integer | none | Required, `1..=65535`. |
| `device` | string or null | `null` | Optional network device. |
| `prefer_ipv6` | boolean | `false` | Prefer IPv6. |
| `connect_timeout` | integer | `5` | Initial connection timeout in seconds. |
| `max_reconnect_tries` | integer or null | `null` | Retry limit; `null` retries indefinitely. |
| `i2p_tunneled` | boolean | `false` | Advisory I2P-tunnel marker. |
| `block_fast_flapping` | boolean | `true` | Enable listener-side protection against short-lived connections. |
| `fast_flapping_threshold` | number | `20` | Non-negative finite seconds; only shorter connections count. |
| `fast_flapping_grace` | integer | `5` | Non-negative number of short disconnects tolerated per IP. |
| `fast_flapping_block_time` | number | `720` | Non-negative finite **minutes**, converted to seconds in the driver. |

A block starts when the short-disconnect count is greater than grace
(the sixth by default).
History is shared across Backbone listeners in the process; each listener uses
its own policy. Long connections do not reset history. Entries expire strictly
after the block interval since the last short disconnect; rejected attempts do
not extend it. Rust uses monotonic elapsed time. Disabling protection bypasses
both recording and rejection for that listener. Client interfaces retain these
settings but do not apply the listener policy.

## Serial field set

`serial`, `kiss`, and `ax25_kiss` use these fields. They require the Cargo
`serial` feature when enabled.

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `port` | string | none | Required and non-empty; serial device path or supported transport URI. |
| `baud_rate` | integer | `9600` | Serial baud rate. `rnode_multi` defaults to `115200`. |
| `data_bits` | integer | `8` | Serial data bits. |
| `parity` | string | `N` | Parity value understood by the serial backend. |
| `stop_bits` | integer | `1` | Serial stop bits. |

## `type: serial`

Uses only the common fields and the Serial field set.

## `type: kiss`

In addition to the Serial field set:

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `preamble_ms` | integer | `350` | KISS preamble duration in milliseconds. |
| `tx_tail_ms` | integer | `20` | TX tail duration in milliseconds. |
| `persistence` | integer | `64` | CSMA persistence, `0..=255`. |
| `slot_time_ms` | integer | `20` | CSMA slot time in milliseconds. |
| `flow_control` | boolean | `false` | Enable hardware flow control. |
| `id_interval` | integer or null | `null` | Station-ID interval in seconds. |
| `id_callsign` | string or null | `null` | Station-ID callsign. |

## RNode radio field set

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `frequency` | integer | none | Required and non-zero, in Hz. |
| `bandwidth` | integer | none | Required, `7800..=1625000` Hz. |
| `spreading_factor` | integer | none | Required, `5..=12`. |
| `coding_rate` | integer | none | Required, `5..=8`. |
| `tx_power` | integer | none | Required, maximum `37` dBm; represented as signed 8-bit value. |
| `airtime_limit_short` | number or null | `null` | Short-term airtime percentage, `0..=100`. |
| `airtime_limit_long` | number or null | `null` | Long-term airtime percentage, `0..=100`. |

## `type: rnode`

Requires an enabled `serial`, `rnode-tcp`, or `ble` build feature appropriate
for the selected port.

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `port` | string | none | Required and non-empty. Serial device, supported TCP URI, or `ble://` URI. |
| all RNode radio fields | — | — | See “RNode radio field set”. |
| `flow_control` | boolean | `false` | Gate transmissions on RNode readiness. |
| `id_interval` | integer or null | `null` | Station-ID interval in seconds. |
| `id_callsign` | string or null | `null` | Station-ID callsign. |

## `type: rnode_multi`

Requires the Cargo `serial` feature.

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `port` | string | none | Required and non-empty. |
| `baud_rate` | integer | `115200` | Parent serial baud rate. |
| `flow_control` | boolean | `false` | Parent flow-control default. |
| `subinterfaces` | sequence | none | At least one entry; virtual ports must be unique. |
| `id_interval` | integer or null | `null` | Parent station-ID interval. |
| `id_callsign` | string or null | `null` | Parent station-ID callsign. |

Each `subinterfaces` entry accepts:

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `name` | string | none | Required subinterface name. |
| `vport` | integer | none | Required virtual port; unique within the parent and within the driver maximum. |
| `enabled` | boolean | `true` | Enable this virtual transceiver. |
| `outgoing` | boolean | `true` | Permit outbound traffic. |
| `flow_control` | boolean or null | `null` | `null` inherits the parent value. |
| `mode` | enum or null | `null` | `null` inherits the parent mode. |
| all RNode radio fields | — | — | See “RNode radio field set”. |

## `type: ax25_kiss`

Requires the Cargo `serial` feature. Uses the Serial field set and adds:

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `callsign` | string | none | Required and non-empty; AX.25 callsign validation is applied by the driver/API. |
| `ssid` | integer | `0` | `0..=15`. |
| `preamble_ms` | integer | `350` | Preamble duration. |
| `tx_tail_ms` | integer | `20` | TX tail duration. |
| `persistence` | integer | `64` | CSMA persistence, `0..=255`. |
| `slot_time_ms` | integer | `20` | CSMA slot time. |
| `flow_control` | boolean | `false` | Enable flow control. |

## `type: plugin`

Linux plugins are loaded in-process through the stable C ABI. `plugin: sx1262`
maps directly to `/usr/lib/reticulum-rs/sx1262.so`; no filename prefix is
added. Use `rnsd-rs --list-plugin` to inspect installed libraries.

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `name` and common fields | — | — | Same common fields as built-ins. |
| `plugin` | string | none | Required filename stem: 1–128 ASCII letters, digits, `_`, or `-`. |
| `mtu` | integer | `500` | Maximum packet size passed to plugin `send()`, greater than zero. |
| `config` | mapping or `null` | `null` | Serialized as opaque YAML and validated by the plugin. |

The plugin reports its physical bitrate and online state at runtime, so the
common `bitrate` setting does not override it. Failure to load or create one
plugin interface is logged but never stops other interfaces or daemon startup,
even when `panic_on_interface_error` is enabled.

```yaml
interfaces:
  - type: plugin
    name: SPI LoRa
    plugin: sx1262
    mtu: 500
    config:
      spi: /dev/spidev0.0
      reset_pin: 12
      frequency: 868000000
```

## Complete small example

```yaml
reticulum:
  share_instance: true
  enable_transport: false

logging:
  level: 4
  timestamps: true
  rss_interval: 300

interfaces:
  - type: auto
    name: LAN discovery

  - type: tcp_client
    name: Backbone peer
    enabled: false
    target_host: example.org
    target_port: 4242
```
