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
| `default_gravity` | signed integer | `0` | Default route preference for configured interfaces. Negative values are valid. |
| `qlen_in_data` | positive integer | `1024` | Admitted DATA/proof/link packet queue capacity, in packets. |
| `qlen_in_announce` | positive integer | `128` | Admitted announce queue capacity, in packets. |
| `qlen_in_pr` | positive integer | `128` | Admitted path-request queue capacity, in packets. |
| `qlen_in_il` | positive integer | `8` | Ingress-limited path-request and released held-announce queue capacity, in packets. |
| `autoconnect_interface_mode` | interface mode or null | `null` | Override discovered-connection mode; absent means `gateway` on transport nodes, otherwise `full`. |
| `autoconnect_interface_gravity` | signed integer or null | `null` | Discovered-connection gravity; absent means `0`, independently of `default_gravity`. |
| `autoconnect_announces_to_internal` | boolean | `false` | Set source-side `announces_to_internal: true` on discovered connections; false leaves the mode policy unchanged. Python's positive integer setting maps to YAML `true`. |
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
`usize`. Python 1.5.2 ignores non-positive overrides; strict Rust YAML rejects
them as configuration errors instead of silently falling back to defaults.

### Inbound queue diagnostics

`GET /api/v1/status` includes `inbound_queues` with `capacities`, `heights`,
`dropped` (four-element arrays in DATA/announce/PR/IL order) and `total`
(current queued packets). The dashboard displays occupancy, configured capacity
and overflow drops for each class. These are local actor metrics, excluding
raw-interface, control and SQLite storage queues. Drops accumulate for the actor
lifetime; rejected invalid packets and administrative queue cleanup are not
counted as overflow. A legacy single-channel actor returns `null`, displayed as
unavailable; an older API response without the field is also supported.

The shared-instance MessagePack `interface_stats` response includes the Python
1.5.2 top-level fields below; existing interface entries are unchanged.

| Class | Queued packets | Overflow drops | Pressure (fraction, not percent) |
|---|---|---|---|
| DATA | `rxqd` | `rxqdd` | `dqpressure` |
| Announce | `rxqa` | `rxqad` | `aqpressure` |
| PR | `rxqp` | `rxqpd` | `pqpressure` |
| IL | `rxqil` | `rxqild` | `ilqpressure` |
| Total | `rxqt` | `rxqtd` | `tqpressure` |

Queue counts and pressure come from one actor snapshot. Interface counters are
sampled separately. The total drop counter saturates at `u64::MAX`, the maximum
unsigned MessagePack integer. Existing Rust interface-only RPC decoders retain
their response shape and ignore these additional fields. `rnstatus-rs` display
and remote-management propagation of queue metrics remain pending.

### Announce and path-request traffic counters (partial 1.5.2 coverage)

Interface entries in shared-instance RPC and Web API include `arxb`, `atxb`,
`arxc`, `atxc` (announce RX/TX bytes and packet counts) and `prxb`, `ptxb`,
`prxc`, `ptxc` (path-request equivalents). The interface details panel displays
these totals. Sizes include the actual packet header, but exclude IFAC and
driver framing. Counters saturate at `u64::MAX` and reset on re-registration.
Inactive configured interfaces return `null`; older RPC responses default to
zero. The UI marks unavailable or unsafe JavaScript integer values with `—`.

RX announces count after signature/blackhole checks; PRs count after tag
deduplication, before the inflight gate. Both count before class-queue admission,
so queue overflow does not undo them. Dispatch does not recount a prepared
packet. Like Python, a held announce re-entering preprocessing is counted again;
these are processing counters, not unique on-wire packet counts.

TX counts after successful admission to the driver's channel, including queued
announces and forwarded path responses, using the post-mangling packet size.
Full/closed TX channels and outbound-disabled interfaces do not increment it.
This is not a physical-delivery acknowledgement: a driver can subsequently
drop the packet. Python invokes its counters at separate outbound/announce-queue
sites; Rust intentionally centralizes this accounting at channel admission.
These counters are per registered endpoint; parent-interface aggregation,
global external totals, traffic composition and PPS remain pending.
Existing frequency estimates and ingress/egress limits are unchanged.

The interface entries also expose `arxs`, `atxs`, `prxs`, `ptxs`: sampled
announce/PR RX/TX speeds in **bits per second**. These per-interface extensions
use the names of Python's global speed fields; they are not global totals.
The details panel labels them explicitly as `bit/s`. Do not interpret them as
the existing API `rx_rate`/`tx_rate` byte-per-second fields.
Maintenance samples approximately once per second, dividing byte deltas by
the actual monotonic elapsed time and multiplying by eight. The first sample
establishes a baseline and reports zero; an idle interval returns the rates
to zero. Queries read the last sample without changing the measurement window.
Re-registration resets both rates and baseline. Old RPC responses default to
zero; inactive configured interfaces expose `null`. Parent/global aggregation
and the legacy general-traffic sampler are not changed by this calculation.

### Receive violation diagnostics (partial 1.5.2 coverage)

Runtime interface entries in shared-instance RPC and Web API expose
`protocol_violations`, `ifac_violations` and `packet_filter_hits`; the interface
details panel displays them. Counters are per registration, saturate at
`u64::MAX`, and reset when an interface is removed and created again.
Configured but inactive interfaces display unavailable values. Older RPC
responses without these fields are accepted with zero defaults.

Currently counted protocol violations: frames too short for IFAC processing,
malformed headers, invalid wire hop counts, missing/malformed announce payloads,
invalid announce signatures, tagless path requests and overlong PR tags. As in
Python, overlong tags are counted but still truncated and processed; PR payloads
shorter than 16 bytes are ignored without a violation. IFAC violations cover
authentication failure, a missing required IFAC flag/tag and an unexpected IFAC
flag on an unprotected interface. Packet-filter hits currently count early
packet-hash duplicate rejection and early PLAIN/GROUP rejection. Blackhole rejection, ingress policy, PR-tag
deduplication and queue overflow are not counted as these filter hits.

Frames exceeding the registered interface MTU are rejected before header
processing. Matching Python 1.5.2, the check compares the **IFAC-stripped** size
to `interface MTU + ifac_size` (bytes); equality is accepted. Announces also
have an independent 500-byte stripped-frame ceiling even on large-MTU links.
Both size failures increment `protocol_violations`, not queue drops.

Before hop adjustment, PLAIN/GROUP packets with wire hops above 1, or with
announce type, are filtered. Keepalive/resource/cache-request/channel contexts
are exempt, and shared clients bypass these checks and early hash dedup.
Valid PLAIN/GROUP packets also bypass hash dedup. This does not bypass IFAC,
MTU, signature checks or the Rust client's opt-in announce policy. PLAIN/GROUP
rejection increments only `packet_filter_hits`, matching the actual Python
preprocess order (the packet's receiving interface is assigned after filtering).

Non-announce packets carrying a transport ID other than this actor's identity
are filtered before context exemptions, including keepalive/resource traffic
and packets for a local destination. Announces and shared clients bypass this
address check. Custom actor embeddings must initialise the transport identity
before receiving addressed Header2 traffic; the runtime does so at startup.

Link-table packets and LRPROOF proofs do not bypass early duplicate lookup.
Instead, recording their hash is deferred until routing/validation claims the
packet. The insertion decision uses the current Link table at dispatch, so
control changes while a packet is queued cannot prematurely record an overheard
packet. LRPROOF deferral applies to PROOF type, not arbitrary packets with that
context. Existing context exemptions remain unchanged.

At dispatch, transit Link traffic is blocked until the Link table entry is
validated, including ordinary proofs and context-exempt traffic such as
keepalive. This increments `protocol_violations` without recording the packet
hash or refreshing the Link timestamp, allowing a retry after a valid LRPROOF.
The current validation state is checked at dispatch, not assumed at admission.

Full announce destination/key-binding failure also counts as a protocol
violation. For transit LRPROOF, an invalid signature counts only on the claimed
interface with matching hops. A missing cached identity, unsupported proof
length, wrong interface or failed rebalance attempt is not counted as an invalid
signature, matching Python's distinction between these paths. Ordinary tunnel
signature rejection likewise does not count: the Python tunnel handler counts
exceptions, not a false signature-validation result.

Tunnel synthesis requires an exact 176-byte payload. Truncated or extended
payloads cannot install, refresh or rebind a tunnel, even if their first
176 bytes contain a valid signature. These length rejections do not increment
protocol violations, matching Python's handler rather than its exception branch.

Python's generic processing-exception counter is not implemented as a Rust
panic catcher: expected parse/validation failures use explicit rejection paths,
while actor invariant failures are programming errors, not automatically blamed
on the peer. This is not a claim that every processing path is panic-free.
Path-MTU signalling diagnostics remain incomplete.
CLI and remote-management display are pending.

### Local Link MTU negotiation (partial)

Runtime responders use the incoming interface's negotiable MTU capability,
passed by the actor in `DestinationEvent::LinkRequest.max_mtu`; unknown or invalid
capabilities fall back to 500. The responder signs
the same effective MTU in LRPROOF that it stores locally: a missing or zero
offer means 500, and a positive offer is capped at the interface limit. Its MDU is computed
before the proof is returned and agrees with the initiator after validation.
Offers too small to fit an encrypted payload produce an MDU of zero, not an
unsigned underflow or the default payload capacity. This is defensive arithmetic,
not a guarantee that handshake frames fit such a small offered MTU.
Library callers can explicitly opt into larger handshakes with
`Link::new_initiator_with_mtu`, `new_responder_with_mtu` or the external-signer
variant `new_responder_with_signer_and_mtu`. Explicit interface limits are
bounded to 500..=2097151; responder proof and MDU use the smaller offered limit.
The existing constructors still use 500. These APIs do not discover
or validate a network path's actual capacity on the caller's behalf.
Async `LinkSession::open*` and `LinkClient::query` now ask the local actor for
the next-hop capability before constructing the request. Unknown/invalid
capabilities fall back to 500. The query, including channel admission, is
bounded to one second and the caller's remaining budget. A legacy or zero-MTU
proof reduces the initiator to at most 500. Synchronous preparation remains
I/O-free and keeps its 500-byte offer. Runtime responder tests cover both signer
paths, authenticated MTU agreement and in-memory encrypted payloads up to MDU.
`cargo test -p rns-runtime --test link_mtu_tcp` additionally exercises a real
loopback TCP driver, actor and responder: MTU 500/1196/262144, full-MDU payloads
in both directions and receive recovery after oversized frames. The peer uses
the Rust Link library with an explicit offer. Running the same test target with
`-- --include-ignored` also checks Python 1.5.2 Link interop in both handshake
directions over TCP, including signed proof validation and full-MDU data both ways.
It requires the local reference (`RNS_PYTHON_ROOT`, default `/home/room/src/Reticulum`)
and Python (`RNS_PYTHON_BIN`, default `/usr/bin/python3.11`). The Python peer uses
stubbed daemon routing and watchdog services, but actual Link/Packet crypto and
packing. The Python responder adapter applies a fixed-interface MTU clamp before
the reference `Link.validate_request`. The Rust initiator test queries the actor's
next-hop MTU on a seeded direct route and confirms proof binding through the actor;
it uses the Link library, not `LinkSession::open*`. With the `full` feature, the
same target also starts a runtime from temporary YAML and tests public
`LinkSession::open/send/recv`: a real Python announce supplies the destination
key and direct route, and the session discovers the configured TCP MTU itself.
All four MTU cases exchange full-MDU data; runtime tasks stop before temporary
storage is removed. A non-ignored transit test also connects two TCP peers through
a Rust relay: incoming/outgoing/offer MTU limits, unchanged Link ID, authenticated
proof forwarding and full-MDU data in both directions. Its route and destination
verification key are seeded; endpoints use the Rust Link library. Shared-client
session opening, active path requests, concurrent multi-peer loads, multi-relay
paths and load/RSS benchmarks remain outside these tests.
Applications constructing
LinkRequest events directly must now supply `max_mtu` (500 preserves the old cap).
TCP, Backbone, Local and Auto expose negotiable MTU through driver metadata, separately
from the raw receive limit. Transit Link Requests with exactly 64 key bytes plus
3 signalling bytes retain at most the offered, incoming-interface and outgoing
negotiable MTU. If the outgoing driver does not expose this capability, nonzero
MTU signalling is removed. Zero offers and requests without exact signalling
remain unchanged. Link ID and key material are preserved.
TCP uses its automatic MTU or explicit `fixed_mtu`; Backbone uses its automatic
curve (no upgrade below its minimum bitrate). Local IPC uses the Python default
262144 bytes, not the automatic curve's 524288-byte result at 1 Gbit/s; Auto
advertises its fixed 1196-byte MTU regardless of configured bitrate. Remaining
drivers conservatively disable transit upgrades. Full nullable hardware MTU,
external next-hop MTU RPC and larger local Links remain
unfinished. Advertising a capability does not remove existing driver RX limits.
In-process callers can use `ReticulumHandle::next_hop_mtu(destination)` to
query the local actor's next-hop capability. It returns `None` for unknown
paths or unsupported interfaces, and uses the shared-server interface for a
local destination when no live path exists. It does not query a remote daemon
or change a Link's negotiated MTU.
When transit MTU clamping reduces an offer, an unsupported signalling mode
causes the request to be dropped and increments the incoming interface's
`protocol_violations`; no relay entry is created. Like Python, this mode check
does not apply when the offer stays unchanged or signalling is removed.

### Path-request tag history

Path requests are deduplicated by destination hash plus their truncated tag,
using current and previous generations as in Python 1.5.2. During maintenance,
if the current set has **more than 16,000** entries, it replaces the previous
set and a new empty current set is created. Equality does not rotate, and
duplicates from either set do not refresh or promote the tag. There is no
120-second tag expiry: at low traffic, history can persist until two rotations.

The threshold is not a hard per-packet memory cap: requests admitted between
maintenance passes can exceed it, and that oversized generation is retained
until replacement. Old generations are released as a whole; no timestamp sort
or oldest-first trimming is performed. Shared-state reset and path-table clear
do not erase tag history. The independent 45-second inflight request gate and
15-second discovery waiters keep their existing timeouts.

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
| `gravity` | signed integer or null | `null` | Inherit `reticulum.default_gravity` when absent; explicit `0` overrides that default. Accepted TCP/Backbone and Auto children inherit their parent's gravity. |
| `announces_to_internal` | boolean or null | `null` | `true` permits this interface's boundary-origin announces on internal egress. False/null retain mode policy (not a blanket deny). Accepted children keep null, as in Python. |

Gravity breaks ties between equally dated announces when the candidate has no
more hops than the current route. It does not override announce freshness or
prefer longer routes of the same age. A boundary interface searches unknown
paths through boundary/gateway interfaces; `recursive_path_requests: true`
removes this mode restriction. Existing ingress and egress limits still apply.

Link establishment can correct a stale hop estimate once, using an authenticated
LRPROOF with the requested encryption mode. This does not migrate active Links:
local Links remain bound to the proof's interface even if a newer announce or
higher gravity changes the destination route. Losing that interface does not
make the established Link broadcast its traffic on other interfaces.

## Discovery publication

These flat fields use the Python interface parameter names. The daemon installs
a native discovery stamper automatically; applications can override it through
`enable_on_network_discovery`. IFAC sizes elsewhere in YAML remain bytes.

| Field | Default / meaning |
| --- | --- |
| `discoverable` | `false`; publish a supported interface |
| `discovery_name` | Interface name |
| `reachable_on` | Advertised address or executable path; a non-wildcard listen address is a Rust fallback |
| `announce_interval` | `360` minutes; clamped to a minimum of `5` |
| `discovery_stamp_value` | `16`; `0` selects default; range `0..=255` |
| `publish_ifac` | `false`; include IFAC name and passphrase |
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
Auto/UDP/Local/Pipe are not publication types. Automatic discovery of the local
I2P b32 address remains unverified; configure `reachable_on` explicitly for I2P.

On Unix, commands execute directly without shell arguments and support `~/`.
They run when publication is due. Missing/non-executable location paths preserve
static coordinates, matching Python. Invalid output or nonzero exit skips that
interface; others continue. Rust bounds commands to five seconds and 4096 stdout
bytes. Longitude is correctly limited to `[-180,180]`, fixing the Python 1.5.2
parser's upper-bound typo. Windows uses static metadata. Rust modulation remains
a string: Python's numeric config parser conflicts with its KISS formatter.

The Web UI exposes these fields under **Advanced → Discovery publication**.
API changes use atomic save/rollback and refresh live publication; deletion or
disabling stops it. Missing encryption identity suppresses publication instead
of falling back to plaintext.

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

TCP HDLC and KISS readers use the handle MTU plus the active IFAC size
as the decoded frame limit, permitting twice that many encoded bytes (excluding
delimiters). Clients use `fixed_mtu` when configured; accepted peers use their
automatic MTU. Oversized frames are debug-logged and discarded before actor
admission, then framing resynchronises. Runtime supplies the size before spawn;
accepted peers inherit their listener's allowance. Without active credentials
the allowance is zero, even if `ifac_size` is set. Direct Rust callers may set
`receive_ifac_size: Some(0..=64)`; its default `None` retains the conservative
64-byte allowance. It is not a YAML key and does not enable authentication.
KISS command bytes are excluded from the payload limit. This is not an RSS limit
or permission to allocate arbitrarily large frames safely.

TCP HDLC also rejects decoded frames of 1..=19 bytes before transport admission
and RX accounting; empty frames are ignored by the deframer. The strict
minimum is checked before IFAC removal and does not grow with IFAC size.
Short-frame drops are debug-logged, not included in actor violation counters.
KISS retains its separate nonempty-DATA rule below.

In TCP KISS mode, only nonempty `CMD_DATA` frames reach transport and RX
packet/byte counters. The TNC port nibble is ignored; control commands and
empty frames are discarded. Serial/RNode deframer limits and command handling
are unchanged. Unlike Python's older TCP KISS loop, oversized payloads are
discarded whole instead of forwarding a truncated prefix. Including the IFAC
allowance also differs from that loop's plain hardware-MTU bound.

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

Local IPC handles and their Link MTU capability use 262144 bytes. The HDLC
reader bounds decoded frames to this size without an IFAC allowance; fully
escaped boundary frames are accepted and oversized frames are discarded.
Empty frames and decoded frames of 1..=19 bytes are silently discarded before
transport admission, matching Python Local's strict minimum. Physical RX byte
totals still include them; actor violation counters do not.
This interface MTU is separate from the current 500-byte local Link cap.
Python's forced shared-instance bitrate/MTU override is not implemented here.

## `type: i2p`

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `connectable` | boolean | `false` | Accept inbound I2P streams. |
| `peers` | sequence of strings | `[]` | I2P peer destinations. |
| `sam_host` | string | `127.0.0.1` | SAM API host. |
| `sam_port` | integer | `7656` | SAM API port, `1..=65535`. |

Connected I2P streams use a one-second liveness tick. After more than 10 seconds
without a completed data write, the serialized writer sends an empty HDLC
probe (`7E 7E`) each tick until another data write completes. Probes do not
advance the data-write timestamp or TX totals. Any received bytes, including
empty probes, refresh receive liveness; empty frames never reach transport.
After more than 20 seconds without received bytes, stale/active transitions
are debug-logged; after more than 110 seconds the watchdog closes the stream.
These states are not yet exposed through RPC/UI. The client uses its existing
reconnect policy; accepted peers terminate their connection task.

Reader, writer and watchdog share one connection lifetime: EOF, write failure,
read timeout or task cancellation drops the other operations and marks it
offline, including blocked transport admission. Probes cannot interleave with
an in-progress frame, so blocked writes can delay probes. A full transport
queue also stops socket reads; if it remains blocked for the receive timeout,
the connection closes even when unread bytes remain in the kernel.
I2P RX totals still count physical bytes; TX counts completed framed data writes
(not probes or a partial write that subsequently fails). No external I2P/SAM
interoperability or complete diagnostics parity is implied by these checks.

## `type: pipe`

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `command` | string | none | Required and non-empty. The child process transports frames over stdio. |
| `respawn_delay` | integer | `5` | Delay after EOF before respawn, in seconds. |

## `type: backbone`

The shared automatic MTU curve used by TCP/Backbone has inclusive bitrate
thresholds, matching Python 1.5.2. Backbone listeners and clients default to
100 Mbit/s and 32768 bytes (32 KiB). The common `bitrate` setting reaches the
driver before startup; accepted peers inherit their listener's bitrate and
computed MTU. Below 62500 bit/s the Rust driver retains a 500-byte fallback
instead of a nullable hardware MTU. This does not remove the separate local
Link cap or complete MTU capability propagation.

Backbone RX bounds decoded HDLC frames to the computed MTU plus the active
IFAC size. Runtime passes this size before startup for YAML, administrative
and discovery connections; accepted peers inherit the listener's limit.
Without an IFAC key the allowance is zero, even if `ifac_size` is configured.
Direct driver callers can set `receive_ifac_size` to `Some(0..=64)`; its default
`None` retains a conservative 64-byte allowance. This Rust-only field does not
enable IFAC or replace actor authentication. The encoded accumulator permits
twice the decoded limit, excluding delimiters,
so fully escaped valid frames also survive fragmented reads. Oversized frames
are discarded and framing resumes at the next delimiter. These driver drops
are debug-logged, but do not reach the actor's protocol-violation counters or
RPC/UI diagnostics. Other drivers retain their existing deframer limits.
Backbone ignores empty HDLC frames and rejects decoded frames of 1..=19 bytes
before transport admission and dataplane ingress packet accounting, matching
Python's strict `frame_len > HEADER_MINSIZE` check before IFAC removal. Physical
RX bytes still include these frames; short-frame drops are debug-logged and do
not reach actor violation counters. This is not header or IFAC authentication.
I2P uses its separate empty-frame keepalive and watchdog described above.

TX combines already queued HDLC frames into encoded batches of at most 64 KiB,
processing at most 64 frames per batch. It does not wait for more traffic to
fill a batch. Large frames span batches without changing wire framing. TX byte
counters include framing and count bytes accepted by the socket, including
partial writes, not confirmed remote delivery. A separate 4 MiB admission
limit covers outstanding encoded bytes across the input/forwarding queues and
writer. A frame that would exceed the limit is rejected whole. Partial writes
release byte reservations; dropped frames release their unwritten remainder.
This is not a 4 MiB process RSS limit: retained raw payloads, the encoded batch,
queue metadata and kernel socket buffers consume additional memory. Pending TX
with no successful socket write for 12 seconds closes the connection through
the normal disconnect/reconnect path. Idle connections are not timed out;
any partial write renews the deadline. Unlike Python's once-per-second drain
sampling, this deadline uses actual write progress and monotonic time.
Additionally, a once-per-second egress controller gates new frames when backlog
exceeds 128 KiB and estimated drain time exceeds 10 seconds, or after three
samples without progress above that watermark. It releases the gate below a
5-second estimate or at/below 128 KiB. Existing output continues draining while
gated. The sampled controller can also request disconnect after 12 seconds
without sampled progress; its boundary ordering follows Python 1.5.2 and is
separate from the actual-write deadline. Actor admission rejections increment
`tx_drops`; byte-drop snapshots are available on the driver TX handle but not
yet exposed through RPC/UI. These thresholds are fixed, not YAML settings.

Dataplane ingress control samples DATA queue pressure every 250 ms in both
memory and SQLite modes. Above the 68% watermark it pauses the most active
Backbone producer; before DATA enqueue at the high watermark (90%, minimum
128 packets) it can pause immediately. Release occurs one peer per sample,
after its hold expires and DATA depth is below the 10% watermark. For DATA
capacities below 10, the effective release watermark is 1 instead of Python's
0, so an empty queue can recover. Other Python threshold floors still apply.
Reader pauses do not stop TX. Counters include all delivered frame classes,
while the pressure signal is DATA only; announce/PR ingress settings are
independent. Backbone requests a 32768-byte socket receive buffer (the OS can
adjust it). An already pending read/send can finish before the gate takes
effect; subsequent reads and frame deliveries wait for release. While waiting
on the gate, reader observes socket close/error readiness without consuming
payload. With unread data, readiness is rechecked every 50 ms to avoid busy
polling; gate release wakes it immediately. Closure while gated discards
pending inbound frames and uses normal disconnect cleanup. When transport
capacity is available, ungated EOF still delivers complete buffered frames
first. While admission is blocked by a full transport channel, close/error
cancels the unsent frame and closes the connection; a live peer instead waits
and resumes delivery when capacity returns. Deregistration notification uses
the same transport channel and may wait for capacity after socket teardown
and the offline transition. This depends on platform socket
readiness support and has been verified on Linux loopback. Multi-peer overload
benchmarks and ingress diagnostics in RPC/UI are still under validation.

An opt-in two-peer Backbone TX measurement is available with
`cargo test -p rns-interface backbone::tx_tests::measure_two_peer_tcp_isolation_and_recovery -- --ignored --exact --nocapture`.
Run it alone: one real TCP receiver stops reading until egress gates, while
another receives 256 x 4096-byte frames without drops. The stopped peer receives
a burst of 4096 x 16384-byte admission attempts, with excess rejected at the
bounded TX queue. After reading resumes, all admitted frames must arrive in
order, the gate must release, and new admission/delivery must work.
Output reports payload throughput, enqueue-to-receive p50/p99 latency, rejected
admissions, observed buffered bytes and process-wide Linux RSS checkpoints.
A sampler requests a reading every 5 ms during admission, gating and recovery;
output includes its observed maximum RSS, successful/unavailable reads and
largest actual interval. Scheduling gaps can miss short-lived peaks. `VmHWM`
is reported before and after the workload as the kernel-reported process-lifetime
high-water mark, including pre-test allocations. Both metrics include the test
harness and both peers, not just driver queues. Missing procfs/fields produce
unavailable readings, not zero memory. Sampling adds overhead; neither metric
is a driver memory bound or a substitute for the encoded TX quota. This is a current
driver baseline, not a before/after speedup comparison, production capacity
estimate or transport-actor ingress/fairness benchmark; build profile, kernel
buffers and host scheduling affect the results.

For an isolated TCP writer comparison, run
`cargo test -p rns-interface backbone::tx_tests::compare_legacy_and_coalesced_tcp_writers -- --ignored --exact --nocapture`.
It recreates the per-frame `write_all` loop from `7a5747c^` alongside the current
coalescing writer, using the same current HDLC helpers and prefilled bounded
queues (4096 x 500-byte payloads with extensive escaping). Both use NODELAY and
the same requested 64 KiB socket buffers; actual OS buffer sizes are printed.
Two AB/BA rounds cover continuous reading and a 100 ms pause followed by 1 ms
pauses per read. Every decoded payload and wire-byte total must match.
Reported completion percentiles start when the prefilled queue is released,
not at an individual live producer's enqueue time. Positive write polls are not
an OS syscall count. This compares writer algorithms only: no managed admission,
adaptive controller or full historical runtime; do not generalise the resulting
speed ratios to production throughput or all of version 1.5.2.

`cargo test -p rns-runtime --test backbone_ingress_load -- --nocapture` covers
the actor-facing path separately in `full` builds (Backbone is not part of
client-only builds). Two real Backbone TCP peers send 4096 unique
256-byte packets each, filling the raw ingress channel before the actor starts.
With four slots per admitted traffic class, the test reconciles all 8192 DATA
packets as application deliveries or queue drops, checks per-peer order and
control-query responsiveness, then verifies fresh deliveries on both sockets.
Only DATA drops are expected; the application delivery channel is sized to avoid
adding unrelated application drops. This bounded burst is not a fairness or
throughput benchmark, and does not require a particular adaptive-gate timing.

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

Fast-flapping settings match Python names and units. A block starts when the
short-disconnect count is **greater than** grace (the sixth by default).
History is shared across Backbone listeners in the process; each listener uses
its own policy. Long connections do not reset history. Entries expire strictly
after the block interval since the last short disconnect; rejected attempts do
not extend it. Rust uses monotonic elapsed time. Disabling protection bypasses
both recording and rejection for that listener. Client interfaces retain these
settings but do not apply the listener policy.

RPC and local/remote `rnstatus-rs --json` report `blocked_ips` (count) and
`blocked_ip_list`; expired entries and IPs still within grace are excluded.
Text status shows nonzero counts; `--blocked-ips` also shows addresses. The Web
interface exposes the settings and live blocked IP diagnostics.

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

The web UI discovers installed plugins through `GET /api/v1/plugins`. A plugin
is available in the editor only when it publishes a valid ABI 1.1
`config_schema_json` object. The UI renders fields from that schema and the
server validates submitted values against it before saving the configuration.
There is intentionally no generic fallback form for plugins without a schema;
ABI 1.0 plugins can still be configured directly in YAML.

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

interfaces:
  - type: auto
    name: LAN discovery

  - type: tcp_client
    name: Backbone peer
    enabled: false
    target_host: example.org
    target_port: 4242
```
