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
| `force_shared_instance_bitrate` | integer or null | `null` | Optional shared-instance bitrate cap in bits/s. |
| `default_ar_target` | integer or null | `null` | Default announce-rate target. `0` disables the target after normalization. |
| `default_ar_penalty` | integer or null | `null` | Default announce-rate penalty. |
| `default_ar_grace` | integer or null | `null` | Default announce-rate grace, `0..=4294967295`. |
| `ingress` | mapping | `{}` | Global ingress/egress-control overrides; see below. |
| `network_identity` | path or null | `null` | Optional network identity file. A leading `~/` is expanded using the user home directory. |
| `discover_interfaces` | boolean | `false` | Publish and process interface-discovery announces. |
| `autoconnect_discovered_interfaces` | integer | `0` | Maximum discovered interfaces to connect automatically; `0` disables autoconnect. |
| `required_discovery_value` | integer | `14` | Required discovery stamp value, `0..=255`. |
| `interface_discovery_sources` | sequence of strings | `[]` | Accepted discovery publisher identity hashes; each is 32 hexadecimal characters. |
| `blackhole_sources` | sequence of strings | `[]` | Blackhole manifest publisher hashes; each is 32 hexadecimal characters. |
| `publish_blackhole` | boolean | `false` | Publish the local blackhole table. |
| `blackhole_update_interval_minutes` | number | `60.0` | Blackhole-source refresh interval in minutes. Runtime clamps it to at least two minutes. |
| `bootstrap_configs` | sequence of paths | `[]` | Additional bootstrap configuration paths consumed by discovery/bootstrap logic. |

### Ingress mappings

The `reticulum.ingress` mapping and every interface's `ingress` mapping accept
the same optional override fields. `null` means “use the runtime/default or the
next outer level”.

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `burst_freq_new` | number or null | `null` | New-peer burst frequency. |
| `burst_freq` | number or null | `null` | Established-peer burst frequency. |
| `path_request_burst_freq_new` | number or null | `null` | New-peer path-request burst frequency. |
| `path_request_burst_freq` | number or null | `null` | Established-peer path-request burst frequency. |
| `new_time` | number or null | `null` | Duration for treating a peer as new. |
| `burst_hold` | number or null | `null` | Burst hold duration. |
| `burst_penalty` | number or null | `null` | Burst penalty duration. |
| `max_held_announces` | integer or null | `null` | Maximum held announces. |
| `held_release_interval` | number or null | `null` | Held-announce release interval. |
| `egress_path_request_freq` | number or null | `null` | Egress path-request frequency. |
| `egress_control` | boolean or null | `null` | Explicit egress-control override. |

## `logging`

| Field | Type | Default | Valid values and meaning |
| --- | --- | --- | --- |
| `level` | integer | `4` | Log level `0..=7`. |
| `timestamps` | boolean | `true` | Include timestamps in formatted logs. |

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
| `announce_rate_target` | integer or null | `null` | Per-interface announce-rate target. |
| `announce_rate_grace` | integer or null | `null` | Grace count. When a target exists and grace is absent, runtime uses `0`. |
| `announce_rate_penalty` | integer or null | `null` | Penalty duration. When a target exists and penalty is absent, runtime uses `0`. |
| `ifac_network_name` | string or null | `null` | IFAC network name. |
| `ifac_passphrase` | string or null | `null` | IFAC passphrase. |
| `ifac_size` | integer or null | `null` | IFAC size in bytes, `1..=64`; interface-class default is used when absent. |
| `ingress_control` | boolean | `true` | Enable ingress control for this interface. |
| `ingress` | mapping | `{}` | Per-interface overrides listed in “Ingress mappings”. |
| `recursive_path_requests` | boolean | `false` | Force recursive path requests. |
| `announces_from_internal` | boolean | `true` | Permit rebroadcast of announces learned from internal interfaces. |

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
| `fixed_mtu` | integer or null | `null` | Fixed MTU, at least `500`. |

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

## `type: plugin` — reserved configuration shape

Plugin loading and ABI support are not implemented yet. The shape is reserved
so adding them later does not require another configuration migration.

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `name` and common fields | — | — | Same common fields as built-ins. |
| `plugin` | string | none | Required, non-empty plugin identifier. |
| `config` | any YAML value with string mapping keys | `null` | Opaque to core; unknown fields are allowed recursively here. |

An enabled plugin entry is rejected until the plugin ABI exists. A disabled
entry can be stored and round-tripped safely:

```yaml
interfaces:
  - type: plugin
    name: Future LoRa
    enabled: false
    plugin: sx1262
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

interfaces:
  - type: auto
    name: LAN discovery

  - type: tcp_client
    name: Backbone peer
    enabled: false
    target_host: example.org
    target_port: 4242
```
