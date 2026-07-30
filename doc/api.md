# Публичный API rsReticulum

## Назначение документа

Этот документ фиксирует публичную поверхность `rsReticulum` перед миграцией
долговременного состояния на SQLite. Его основная задача — отделить API,
используемый приложениями, от деталей текущего представления данных в памяти и
на диске.

Аудит выполнен по исходникам:

- `rsReticulum`, включая workspace crates, tools и examples;
- `rsLXMF`;
- `rsNodePage`;
- `rsNomadNet`;
- `rsRRC`;
- `rsRRC-client`;
- `rsRRCD`.

Проверена default/serial Linux-поверхность. Условные BLE, Android, Apple,
Windows, `rnode-tcp`, REST API и hardware PIV модули учтены по исходным
`cfg`/feature declarations. Каноническая локальная документация основного
workspace строится командой:

```bash
cargo doc --workspace --no-deps --offline
```

Документ описывает:

1. весь публичный API на уровне crates и модулей;
2. публичные transport/runtime контракты, которые затрагивает SQLite;
3. фактическое использование API загруженными приложениями;
4. обязательства совместимости;
5. API, который является утечкой внутреннего представления и требует
   compatibility adapter.

## Итог аудита

Ни одно загруженное внешнее приложение не обращается напрямую к:

- `rns_transport::path_table::PathTable`;
- `rns_transport::hashlist::PacketHashlist`;
- `rns_transport::persistence`;
- `rns_transport::actor::TransportActor`;
- `rns_transport::tunnel::TunnelTable`;
- `rns_transport::blackhole::BlackholeTable`;
- `rns_transport::rate_limit::RateTable`;
- внутреннему `recent_announces`.

Это позволяет заменить их хранение на SQLite без изменений приложений.

Обязательная compatibility surface сосредоточена в:

- `rns_runtime::reticulum`;
- `rns_runtime::application`;
- `rns_runtime::link_client`;
- `rns_runtime::link_manager`;
- `rns_runtime::link_session`;
- `rns_runtime::lifecycle`;
- `rns_runtime::config`;
- `rns_transport::messages`;
- `rns_transport::link_messages`;
- `rns_identity`;
- `rns_wire`;
- криптографических функциях `rns_crypto`.

## Политика совместимости

В ходе SQLite-миграции без отдельного решения нельзя:

- удалять или переименовывать перечисленные используемые модули;
- менять значения и поля публичных enum variants;
- менять типы полей публичных DTO;
- менять сигнатуры high-level async API;
- менять семантику `ReticulumHandle.transport_tx`;
- менять формат событий destination/link/announce;
- менять wire-visible constants и packet encoding;
- менять поведение identity, signing, hashing и destination derivation;
- менять feature names, необходимые приложениям.

Допускается:

- заменить внутренний `HashMap` на SQLite;
- заменить полные persistence snapshots на инкрементальные транзакции;
- удалить Python-compatible файлы после удаления или закрытия legacy API;
- добавить owned и paginated API;
- оставить старые full-list API как compatibility wrappers;
- изменить `pub` storage API только после проверки downstream и периода
  deprecation.

## Workspace crates

### `rns-crypto`

Криптографические примитивы. Публичные модули:

- `aes_cbc` — AES-CBC encryption/decryption;
- `ed25519` — private/public keys, signing и verification;
- `hkdf` — HKDF-SHA256;
- `hmac` — HMAC-SHA256;
- `pkcs7` — padding;
- `random` — генерация случайных данных;
- `sha` — SHA-256, full и truncated hashes;
- `token` — Reticulum token encryption и `TOKEN_OVERHEAD`;
- `x25519` — X25519 private/public keys и ECDH.

Корневой API:

- `hex_encode(bytes: &[u8]) -> String`;
- `TOKEN_OVERHEAD`.

Используется приложениями:

- `sha::{sha256, full_hash, truncated_hash}`;
- `hkdf::hkdf_sha256`;
- `Ed25519PrivateKey`;
- `Ed25519PublicKey`;
- `hex_encode`.

SQLite-миграция не должна менять этот crate.

### `rns-wire`

Публичные модули:

- `constants`;
- `context`;
- `flags`;
- `hash`;
- `header`;
- `packet`;
- `proof`;
- `receipt` при feature `std`;
- `types`.

Основной API:

- `PacketContext`;
- `HeaderType`, `ContextFlag`, `TransportType`, `DestinationType`,
  `PacketType`, `PacketFlags`;
- `PacketHeader`, pack/unpack;
- packet hash и truncated packet hash;
- packet encode/decode types;
- proof validation/building;
- `PacketReceipt`, receipt status;
- typed hashes из `types`;
- wire-size constants, включая `ENCRYPTED_MDU`.

Используется напрямую `rsLXMF`, `rsNodePage` и `rsNomadNet`. Структуры,
discriminants и encoding являются стабильным wire contract.

### `rns-identity`

Публичные модули:

- `announce`;
- `announce_state`;
- `destination`;
- `identity`;
- `ifac`;
- `known_destinations`;
- `name_hash`;
- `persistence`;
- `ratchet`.

Основные группы API:

- `AnnounceData`: создание, pack/unpack, проверка подписи, извлечение public
  key/app data/ratchet;
- `RatchetControlState`;
- `Destination`, `DestinationError`, `Direction`, `DestType`,
  `ProofStrategy`, `AnnounceTime`;
- `Identity`: генерация, импорт/экспорт, public/private keys, hash, sign,
  verify, encrypt/decrypt и ECDH;
- IFAC derivation и transformation;
- `KnownDestination`, `KnownDestinations`;
- `name_hash`;
- atomic/read persistence helpers;
- `RatchetRing`, `RatchetRingFormat`, ratchet public key helpers.

Используется всеми сетевыми приложениями. Особо стабильны:

- `Identity::new`;
- `Identity::from_public_key`;
- чтение/сохранение identity;
- `Identity.hash` и public key representation;
- `Destination` constructors/hash derivation;
- `AnnounceData::unpack`;
- `name_hash`;
- ratchet types, используемые `rsLXMF`.

`KnownDestinations` является кандидатом на внутренний SQLite backend, но его
публичные методы должны сохраняться либо получить совместимый wrapper.

### `rns-transport`

Публичные модули:

- `actor`;
- `announce`;
- `await_path`;
- `blackhole`;
- `constants`;
- `discovery`;
- `hashlist`;
- `ifac`;
- `ingress`;
- `link_messages`;
- `link_table`;
- `messages`;
- `path_table`;
- `persistence`;
- `rate_limit`;
- `reverse_table`;
- `traffic`;
- `tunnel`.

Корневая функция:

- `now_f64() -> f64`.

Подробный storage-sensitive API описан ниже.

### `rns-link`

Публичные модули:

- `constants`;
- `encryption`;
- `handshake`;
- `keepalive`;
- `key_derivation`;
- `link`;
- `mtu_discovery`;
- `request`.

Основные группы API:

- link encryption/session key derivation;
- initiator/responder handshake;
- proof validation;
- keepalive state;
- `Link` и link state;
- MTU discovery;
- request/response encoding и tracking.

Приложения в основном используют этот слой через `rns-runtime`; прямое
изменение типов и link semantics при SQLite-миграции не требуется.

### `rns-protocol`

Публичные модули:

- `buffer`;
- `channel`;
- `channel_message`;
- `compression`;
- `resource`;
- `resource_adv`;
- `rnsh`;
- `stream_data`.

Основные группы API:

- link-backed buffers;
- `Channel`, `LinkChannel`, channel window и envelope;
- `MessageBase`, channel message encoding;
- compression;
- resource advertisement, segmentation, proof и assembly;
- RNSH messages/codecs;
- stream-data messages.

`rsLXMF` использует `channel_message::{MessageBase, ChannelMessageError}` и
runtime link/resource API. SQLite не должна менять protocol behavior.

### `rns-interface`

Публичные модули основной сборки:

- `auto`;
- `ax25kiss`;
- `backbone`;
- `hdlc`;
- `i2p`;
- `kiss`;
- `local`;
- `pipe`;
- `rnode`;
- `rnode_admin`;
- `serial_tcp_stream` при `serial` или `rnode-tcp`;
- `socket_tuning`;
- `tcp`;
- `traits`;
- `udp`;
- `weave`.

Feature/platform modules:

- `android_usb` на Android;
- `kiss_iface`, `rnode_multi`, `serial` с feature `serial`;
- `ble_central_apple`, `ble_central_apple_connect` на Apple с BLE;
- `ble_central_lifecycle`, `ble_peer`, `ble_peer_lifecycle`, `ble_rnode`
  с BLE.

Каждый concrete interface предоставляет config/event/handle types и spawn
functions. Общий контракт находится в `traits`:

- `InterfaceHandle`;
- inbound/outbound channels;
- interface mode/capability/state;
- counters и lifecycle.

SQLite backend не должен попадать в interface API. Все обращения к storage
остаются сериализованными через `TransportMessage`.

### `rns-runtime`

Публичные модули:

- `application`;
- `config`;
- `constants`;
- `interface_factory`;
- `jobs`;
- `lifecycle`;
- `link_client`;
- `link_manager`;
- `link_session`;
- `platform`;
- `probe`;
- `remote_management`;
- `remote_management_schema`;
- `reticulum`;
- `rncp`;
- `rnsh`;
- `rpc`;
- `rpc_server`;
- `api_server` с feature `api`.

Это основной прикладной API. Его детальный compatibility contract приведён
ниже.

### `rns-ratkey`

Публичные модули:

- `apdu`;
- `attestation`;
- `detect`;
- `error`;
- `hardware`;
- `hwid`;
- `mgmt`;
- `mock`;
- `pin`;
- `provision`;
- `seed`;
- `session`;
- `transport`;
- `backend` с feature `hardware`.

Корневые реэкспорты:

- `RatkeyError`;
- `HardwareIdentity`, `IdentityBackend`;
- `HwidConfig`;
- `MockPivSession`;
- `PinCache`;
- `ProvisionConfig`, `ProvisionResult`;
- `DeviceMeta`, `PivSession`;
- `PivTransport`;
- `HardwareBackend`, `load_hardware_identity`, `PcscPivSession`,
  `PcscTransport` с feature `hardware`.

SQLite-миграция transport state этот crate не затрагивает.

### `rns-tools`

Library API:

- модули `format` и `hash`;
- `RS_RETICULUM_VERSION`;
- `RETICULUM_COMPAT_VERSION`;
- `config_log_timestamps`;
- `init_tracing`.

CLI command modules также содержат публичные helper types/functions, но они не
являются storage contract приложений. Существующие binaries и их CLI/RPC
поведение должны продолжить работать.

## Storage-sensitive API `rns-transport`

### `PacketHashlist`

Публичные методы:

- `new`;
- `new_with_capacity`;
- `contains`;
- `insert`;
- `force_rotate`;
- `len`;
- `is_empty`;
- `clear`;
- `all_hashes`;
- `load_from`.

`all_hashes` и `load_from(Vec<_>)` предполагают полную материализацию. Они не
используются внешними загруженными приложениями, но являются публичными.
SQLite-вариант должен:

- временно сохранить их как compatibility methods;
- пометить как memory-expensive;
- добавить page/stream API;
- не использовать их внутри actor.

`contains`, `insert`, `len`, `is_empty`, `clear` и rotation semantics должны
сохраниться.

### `PathEntry` и `PathTable`

`PathEntry` публично содержит route metadata: timestamp, next hop, hops,
expiry, random blobs, interface ID и packet hash.

Методы `PathEntry`:

- `new`;
- `add_random_blob`;
- `has_random_blob`;
- `is_expired`;
- `touch`;
- `blobs_for_persist`.

Методы `PathTable`:

- `new`;
- `insert`;
- `get`, `get_mut`;
- `get_live`, `get_live_mut`;
- `has_path`, `hops_to`;
- `remove`;
- `drop_all_via`, `drop_all_via_next_hop`;
- `expire`;
- `set_state`, `get_state`;
- `cull_expired`, `cull_expired_batch`, `cull_dead_interfaces`;
- `len`, `is_empty`;
- `iter`, `iter_live`.

Проблемные для SQLite методы:

- `get -> Option<&PathEntry>`;
- `get_mut -> Option<&mut PathEntry>`;
- `get_live/get_live_mut`;
- `iter/iter_live`.

Они требуют resident cache или owned replacement. Внешние загруженные
приложения их не вызывают. Внутри actor можно перейти на owned/update
operations, сохранив старый `PathTable` как compatibility facade.

### Transport messages

`rns_transport::messages` является обязательным контрактом.

Публичные типы:

- `InterfaceId`;
- `InterfaceRole`;
- `InboundPacket`;
- `OutboundRequest`;
- `TimerTick`;
- `InterfaceEntry`;
- `QueuedAnnounce`;
- `AnnounceHandlerEvent`;
- `TransportMessage`;
- `TransportQuery`;
- `TransportQueryResponse`;
- `PathTableRpcEntry`;
- `InterfaceStatRpcEntry`;
- `RateTableRpcEntry`;
- `AnnounceRpcEntry`;
- `BlackholeRpcEntry`.

Все существующие variants `TransportMessage`, `TransportQuery` и
`TransportQueryResponse`, их payload types и semantics сохраняются.
SQLite backend реализуется за actor boundary.

Особенно используются приложениями:

- `TransportMessage::Outbound`;
- `TransportMessage::RegisterDestination`;
- `TransportMessage::RegisterAnnounceHandler`;
- `TransportMessage::Rpc`;
- `OutboundRequest`;
- `TransportQuery::GetPathTable`;
- `TransportQuery::GetInterfaceStats`;
- `TransportQuery::GetRecentAnnounces`;
- соответствующие response variants;
- `AnnounceHandlerEvent`.

Старые full-table query могут материализовать `Vec`, чтобы сохранить API.
Следует добавить новые paginated variants, не заменяя существующие.

### Announces

`AnnounceEntry` и `AnnounceTable` публичны. `AnnounceTable` предоставляет:

- `new`;
- `insert`;
- `get/get_mut`;
- `remove`;
- `len/is_empty`;
- `due_for_retransmit`;
- `iter`;
- `cull_exhausted`.

Это активная очередь retransmit, а не persisted `recent_announces`; она
остаётся в RAM.

Persisted announce functions/types из `persistence` публичны, но не
используются внешними приложениями. Их следует deprecated до удаления:

- `PersistedAnnounceEntry`;
- `LegacyPersistedAnnounceEntryV5`;
- `PersistedAnnounceCache`;
- `save_announce_cache`;
- `load_announce_cache`;
- `load_announce_cache_legacy_v5`;
- `migrate_legacy_announce_entries`;
- Python cached announce read/write;
- `sweep_announce_cache`.

### Persistence

Модуль экспортирует persisted path, hashlist, blackhole, announce и tunnel
types и функции сохранения/загрузки, включая Python formats.

После SQLite-миграции это legacy API. Порядок изменения:

1. перестать использовать его внутри actor;
2. пометить старые функции deprecated;
3. при необходимости оставить одноразовый importer;
4. удалить только в следующем breaking release.

Так максимальная совместимость сохраняется без продолжения записи лишних
файлов.

### Tunnel, blackhole и rate

Публичные table APIs используют обычные `new`, insert/update, lookup,
remove/cull, `len/is_empty` и iterator methods.

Загруженные приложения работают с ними через `TransportQuery`, а не напрямую.
При переводе в SQLite high-level message/RPC behavior сохраняется. Reference
iterators могут остаться в compatibility in-memory facade либо быть
deprecated в пользу owned/page API.

## Основной `rns-runtime` contract

### `ReticulumHandle`

Публично используются:

- `transport_tx`;
- `config_dir`;
- `instance_mode`;
- `interface_configs`;
- `id_gen`;
- `handle_tx`;
- `socket_base`;
- `config`;
- `is_foreground`;
- `shutdown`;
- `transport_identity`;
- `network_identity`;
- `discovery`;
- `transport_enabled`;
- `should_use_implicit_proof`;
- `remote_management_enabled`;
- `link_mtu_discovery`;
- `await_path`;
- `query_transport`;
- `query_control`;
- discovery enable/query methods;
- blackhole configuration accessors.

Поля и методы, уже доступные приложениям, сохраняются. SQLite connection не
должно стать публичным полем handle. Закрытый `interface_controls` остаётся
деталью runtime.

### Инициализация и runtime configuration

Стабильный API:

- `reticulum::init`;
- `get_instance`;
- `InstanceMode`;
- `SharedInstanceType`;
- `SharedInstanceRpcEndpoint`;
- `ReticulumConfig`;
- shared instance endpoint helpers;
- runtime interface spawn/teardown functions;
- `spawn_interface_from_config`.

Feature-gated BLE/Android/RNode spawn functions сохраняют сигнатуры.

### Application API

Стабильные типы и функции:

- `ApplicationError`;
- application `InboundPacket`;
- application `PacketReceipt`;
- `PacketSubmission`;
- `RegisteredDestination`;
- `build_announce_packet`;
- `await_path`;
- `recall_identity`;
- `announce_stream`;
- `send_packet`;
- `send_pre_encrypted_packet`;
- `send_pre_encrypted_packet_with_receipt`;
- `try_send_pre_encrypted_packet`;
- `try_send_pre_encrypted_packet_on_transport`.

### Link client

Стабильные типы:

- `LinkClientError`;
- `LinkClient`;
- `PreparedLinkSession`;
- `LinkSession`;
- `LinkSessionHandle`;
- `ReceivedResource`;
- `LinkResponse`;
- `LinkPayloadSendReceipt`.

Стабильные операции:

- prepare/open link с recalled или заданным public key;
- establish/spawn;
- `id`, `rtt`, `mdu`, `remote_identity`;
- identify;
- send/receive packet;
- send/receive payload и resource;
- request/response;
- channel send/receive;
- delivery proof;
- close.

### Link manager

Стабильные публичные типы:

- `LinkResponse`;
- `LinkChannelMessage`;
- `ChannelSendReceipt`;
- `LinkPacketSendReceipt`;
- `LinkResourceSendReceipt`;
- `LinkPacketProof`;
- `LinkResourceProof`;
- `LinkPayloadSendReceipt`;
- `ChannelSendError`;
- `LinkSendError`;
- `LinkManagerCommand`;
- `ResourceCompletion`;
- `RequestOutcome`;
- `LinkManager`.

Сохраняются constructors, run/step/tick, handler/channel setters, active link
access, channel/packet/resource send methods и `register_destination`.

`rsLXMF` особенно зависит от `LinkManagerCommand`, proof/receipt types и
setter channels. Их изменение потребует согласованной миграции и поэтому не
допускается в SQLite-этапе.

### Link listener

`rns_runtime::link_session` экспортирует listener types/events, send/channel/
close API и `DEFAULT_LINK_TIMEOUT`. Это основной server-side API `rsRRCD`.

### Config и lifecycle

Стабильны:

- `Config`, `ConfigSection`, `ConfigValue`, `ConfigError`;
- parse/from_file/default/write/save и section/subsection operations;
- `ShutdownSignal`;
- `ExitHandler`;
- `install_signal_handlers`;
- platform path resolution.

SQLite-настройки добавляются как необязательные config keys с безопасными
default values.

## Фактические потребители

### `rsLXMF`

Самый широкий downstream consumer. Использует:

- crypto hashes, Ed25519 и HKDF;
- identity, destination, announces и ratchets;
- packet headers, flags, contexts и hashes;
- config/lifecycle/platform;
- runtime init и `ReticulumHandle`;
- application announce/pre-encrypted send;
- `LinkClient`, `LinkSession`;
- `LinkManager`, commands, proofs и receipts;
- transport outbound/RPC/announce messages;
- discovery stamper;
- protocol channel message traits.

Все эти элементы относятся к обязательному compatibility contract.

### `rsNodePage`

Использует:

- identity/destination/announce;
- packet header/flags/constants;
- runtime config/init/lifecycle/platform;
- `LinkManager` и `RequestOutcome`;
- `DestinationEvent`;
- `TransportMessage`.

Не зависит от storage tables напрямую.

### `rsNomadNet`

Использует:

- crypto hash и Ed25519 public key;
- identity/destination/name hash;
- runtime application, link client/manager и handle;
- announce handler и transport messages;
- packet header/flags/hash API;
- `rsRRC-client`;
- `rsLXMF`.

Изменения transport DTO могут транзитивно затронуть сразу три проекта.

### `rsRRC`

Не зависит от `rsReticulum`. Это общий wire protocol RRC.

### `rsRRC-client`

Использует:

- `Identity`;
- `ReticulumHandle`;
- `LinkSession` и `LinkSessionHandle`;
- `TransportMessage`, `TransportQuery`, `TransportQueryResponse`;
- `ShutdownSignal` в examples.

### `rsRRCD`

Использует:

- `Identity`;
- config и lifecycle;
- runtime init/handle;
- application `await_path`;
- link client;
- server-side `LinkListener`, `LinkListenerEvent`;
- link payload/resource receipts.

## Compatibility matrix для SQLite

| API | Используется приложениями | Решение |
|---|---:|---|
| `TransportMessage/Query/Response` | Да | Сохранить полностью |
| `ReticulumHandle` | Да | Сохранить полностью |
| Application/link APIs | Да | Сохранить полностью |
| Identity/wire/crypto | Да | Не менять |
| `PathTable` reference API | Нет | Facade/deprecation, внутренне owned |
| `PacketHashlist::all_hashes/load_from` | Нет | Compatibility wrapper, не использовать внутри |
| Persistence file functions | Нет | Deprecated, затем удалить |
| Full-table RPC | Да | Сохранить, добавить paging |
| Announce raw cache files | Нет | Удалить после перехода на SQLite |
| `TransportActor` fields | Нет | Свободно менять внутренне |
| Link/reverse/runtime tables | Нет напрямую | Оставить в RAM |

## Новые API, рекомендуемые до миграции

Добавления не ломают существующий код:

- paginated path query;
- paginated recent announce query;
- hashlist count/clear без материализации;
- owned route lookup;
- explicit route `touch/update`;
- storage statistics query;
- database health/checkpoint query для tools;
- конфигурация RAM LRU/page cache.

Старые full-list и reference APIs следует сохранять на первом этапе и
реализовывать как compatibility adapters. После перевода внутренних call sites
на новый API можно отдельно решить, какие методы объявить deprecated.

## Вывод

SQLite-миграцию можно выполнить без изменений существующих приложений, если
сохранить message-based boundary `rns-transport` и high-level `rns-runtime`.
Основные несовместимые с on-demand storage методы находятся в низкоуровневых
table types и не используются загруженными downstream projects.

Первый безопасный этап:

1. добавить новый внутренний storage backend;
2. не менять `TransportMessage`, `TransportQuery`, responses и runtime handle;
3. перевести hashlist и persisted announces;
4. сохранить legacy low-level API через adapters;
5. добавить owned/paginated API для дальнейшего перевода path table.
