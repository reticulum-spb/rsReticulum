# Обновление до Reticulum 1.5.2: матрица и журнал

## Основание проверки

План: `updateTo-1.5.2.md`. Python: `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`.
Rust до начала работ: `15be6c0077e5398fed91e6dc4c17a26fe0b66219`.
Дата проверки: 11 сентября 2026. Применимых `AGENTS.md` при обходе каталогов
репозиториев и родителей Rust не обнаружено. Python рабочее дерево чистое.
Пользовательский файл плана первоначально untracked; отметки ведутся здесь.

После указанного в плане Rust `3b01eb7` уже исправлены transport ID заголовков,
проверка stream ID, отключённые signalling modes, Local IFAC default, проверка
tunnel synthesis и воспроизведение кешированных announces через transport.
Эти изменения сохраняются. Документы PKCS7/TOKEN/PROOF уточняют границы паритета.

Матрица ниже описывает исходное состояние, без незавершённых изменений текущего
рабочего дерева. «Реализовано» здесь означает наличие прочитанной реализации,
а не прохождение нового межъязыкового теста. «Воспроизвести» — обязательная
открытая проверка; она не является исключением из объёма обновления.

## Матрица changelog 1.3.9–1.5.2

Пути Rust ниже относительно `crates/`, Python — относительно `RNS/`.

| Версия / изменение | Исходное состояние и доказательство | Дальнейший этап |
|---|---|---|
| 1.3.9 rnsh security | Реализована защита в `rns-runtime/src/rnsh.rs`: identity gate, authorized/session state, регрессии запрещённого Execute. Повторить сценарии | 7 |
| 1.3.9 rnsh config/identity paths | Частично: `rns-tools/src/commands/rnsh.rs` использует `--config` для RNS; сверить новые Python defaults | 7 |
| 1.3.9 Backbone fast flapping | Отсутствует: listener/config в `rns-interface/src/backbone.rs` не ведут историю блокировок IP | 3 |
| 1.3.9 LOG_PATHING / logging | Частично: tracing и числовые уровни в runtime; прямое соответствие новых уровней проверить | 7 |
| 1.3.9 internal discovery | Расхождение: `apply_discovery_mode_autocorrect` разрешает только gateway/AP | 1 |
| 1.3.9 location script | Отсутствует в YAML и scheduler; Python `Discovery.py:get_interface_announce_data` запускает executable | 1 |
| 1.3.9 RESOURCE_RCL, reliability | Реализована отмена в protocol/runtime, есть regression tests; межъязыковое поведение воспроизвести | 6 |
| 1.4.0 transport persistence / interface hash / known destinations background cleaning и отказ от recombination | Частично: actor maintenance, storage worker и сохранённые interface hashes; профилирование и семантику очистки сверить. Python lock/thread оптимизации буквально неприменимы | 0, 7 |
| 1.4.0 invalid discovery stamp cache | Отсутствует в `discovery/receiver.rs:process_event`: каждый stamp проверяется заново | 1 / контроль нагрузки |
| 1.4.0 valid discovery cache / sequential validation | Частично: отдельная последовательная receiver task; повторное событие снова проходит decode/stamp/store | 1 / контроль нагрузки |
| 1.4.0 Link stale teardown / watchdog race | Частично: `rns-link/src/keepalive.rs:is_stale` учитывает outbound; проверить вызовы в runtime и ошибки приёма | 6 |
| 1.4.0 Backbone None-check / exception logging | Python-specific None/exception детали; Rust Result/Option; эквивалентные disconnect ошибки проверить с fast flapping | 3 |
| 1.4.0 stamp default 16 | Реализовано в `discovery/constants.rs` и runtime | 1, сохранить |
| 1.4.0 blocked IP ifstats | Отсутствует вместе с механизмом блокировок | 3, 7 |
| 1.4.0 reduced log noise | Воспроизвести уровни на локальной нагрузке; Rust tracing не требует копирования Python сообщений | 7 |
| 1.4.1 dynamic rebalance / gravity | Отсутствует: `PathEntry` и выбор announce-пути в `actor/inbound.rs` не содержат gravity/rebalance | 2 |
| 1.4.1 set_max_request_size | Отсутствует в Destination и runtime request admission | 6 |
| 1.4.1 max_response_size | Реализован `link_client.rs:request_with_metadata_limit`, включая Resource advertisement; сегменты проверить | 6, сохранить |
| 1.4.1 autoconnect mode/gravity/to_internal, default_gravity, interface gravity/to_internal | Отсутствуют в `yaml_config.rs`, runtime autoconnect и transport registration | 2 |
| 1.4.1 rnstatus gravity display/sort | Отсутствует в CLI и статистике | 7 |
| 1.4.1 boundary→boundary/gateway PR | Воспроизвести таблицу переходов `actor/outbound.rs` с recursive/internal flags | 2 |
| 1.4.1 I2P tasks garbage collection | Python GC причина неприменима к Tokio; прочие minor I2P fixes требуют локального воспроизведения | 5 |
| 1.4.1 ingress burst active deadlock | Воспроизвести тайминги `ingress.rs` и maintenance без новых announces | 4 |
| 1.4.1 memory efficiency / LOG_EXTREME | Воспроизвести нагрузку и числовые уровни, не переносить Python allocation детали без измерений | 5, 7 |
| 1.4.1 historical discovery blackhole cleanup | Воспроизвести: `DiscoveryStore` хранит историю; проверить runtime blackhole фильтр и очистку | 1 |
| 1.4.2 zero-bitrate recursive PR | Воспроизвести незапущенный RNode и bitrate=0 без аппаратуры | 2 |
| 1.4.2 Android slow blackhole filtering | Python Android причина не доказана в Rust; проверить общую фильтрацию discoveries | 1 |
| 1.5.0 discovery operator LXMF | Wire/runtime реализованы; YAML отсутствует | 1 |
| 1.5.0 prioritized inbound / configurable four queue lengths | Отсутствуют: `TransportActor::new` создаёт один mpsc для control и inbound | 4 |
| 1.5.0 early filtering / excessive hops | Частично: проверки в `actor/inbound.rs`, нет новой классификации до общей очереди | 4 |
| 1.5.0 protocol violation tracking | Отсутствуют отдельные interface counters; BlackholeReason не заменяет статистику | 4, 7 |
| 1.5.0 inflight PR tracking / batching | Частично: `path_requests`, `discovery_path_requests`, `pending_local_path_requests` имеют разные назначения; нового inflight множества ожидающих нет | 4 |
| 1.5.0 blackholed announce validation API | Воспроизвести `rns-identity/src/announce.rs` и runtime API; blackhole transport filtering не равен возвращаемому API статусу | 6 |
| 1.5.0 Channel/Buffer full MDU | Частично: `Channel::channel_mdu` есть; вызовов за пределами собственных тестов поиском не найдено, проверить send/split реализацию | 6 |
| 1.5.0 queue pressure/drop statistics | Отсутствует с новыми очередями | 4, 7 |
| 1.5.0 detailed announce/PR flow, totals/frequencies/composition | Частично: `traffic.rs`, `ingress.rs`, RPC/CLI частоты есть; полная композиция отсутствует | 7 |
| 1.5.0 active links / blocked IP listings | Частично: LinkCount есть; отдельную статистику active Links проверить; blocked IP отсутствуют | 7 |
| 1.5.0 medium bitrate helpers/RPC, slow-medium discovery PR timeout | Отсутствуют: RPC имеет first_hop_timeout, что не является medium_path_timeout | 6 |
| 1.5.0 adaptive rncp/rnpath/rnprobe timeouts | Отсутствует связь с medium helper | 6 |
| 1.5.0 adaptive rnx/rngit timeouts | В этом репозитории соответствующие CLI не обнаружены; не добавлять полные новые утилиты в обновление ядра | граница покрытия |
| 1.5.0 inbound/PR processing, limiting, jobs, pending link/announce state fixes | Частично: actor и ingress существуют; каждую семантическую регрессию сверить с Python тестами/изменениями | 4, 6 |
| 1.5.0 Backbone EPOLL starvation | EPOLL Python implementation неприменима; справедливость Tokio read/write проверить нагрузкой | 5 |
| 1.5.0 receipt callback deadlock | Python receipt lock не переносится; проверить повторную отправку из callback в actor архитектуре | 6 |
| 1.5.0 Link watchdog exception reset | Воспроизвести runtime malformed receive/error path | 6 |
| 1.5.0 Resource multisegment cancellation / part alignment/rebinding | Частично: сегменты/RCL реализованы; проверить индексы и повторное связывание Python↔Rust | 6 |
| 1.5.0 stale BLE device reference | Воспроизвести программную lifecycle модель `rns-interface/src/ble_*`; аппаратный тест отдельно | 6 |
| 1.5.0 retained ratchet cleanup | Реализовано ограниченное кольцо и retention в `rns-identity/src/ratchet.rs`; сохранить описанную границу 512 и повторить lifecycle | 6 |
| 1.5.0 invalid rnstatus stats / burst count | Частично: optional decode/defaults и burst flags есть; сравнить local/remote JSON | 7 |
| 1.5.0 miscellaneous packet/link/interface fixes | Не конкретизированы changelog: требуется сопоставление Python diff и регрессионных тестов; не считать выполненными | 4–6 |
| 1.5.0 rngit Windows resources | Отсутствующая Rust утилита; общие Resource семантики остаются в этапе 6 | граница покрытия |
| 1.5.0 rnodeconf WiFi summary | Воспроизвести существующий summary Rust, отдельно от flashing backlog | 7 |
| 1.5.0 speedtest stale link | Проверить эквивалентные примеры и transfer status, не объявлять все примеры совместимыми заранее | 6 |
| 1.5.0 documentation queue/discovery | Отсутствуют новые YAML настройки в CONFIG/Web; обновить с реализацией | 1, 4 |
| 1.5.1 adaptive dataplane ingress/egress | Отсутствует: `backbone_read_loop` ожидает общий mpsc; write path пишет отдельный HDLC frame | 5 |
| 1.5.1 coalescing TX buffers | Отсутствует в Backbone backend | 5 |
| 1.5.1 early invalid frames | Частично: deframer cap и parser checks; отсутствуют новые counters/early admission | 4, 5 |
| 1.5.1 discovery implementation/version | Реализована wire metadata; требования при приёме перепроверить | 1 |
| 1.5.1 Profiler/decorator/reentrant bounded capture/live output | Python decorator неприменим; live Rust profiling отсутствует, определить границу и измеримые метрики | 7 |
| 1.5.1 PPS/MTU/TX drops/TX buffer rnstatus | MTU/TX drops локально реализованы; PPS/TX buffer и remote parity требуют реализации | 7 |
| 1.5.1 throughput benchmarker | Новый сопоставимый локальный baseline ещё не выполнен | 5 |
| 1.5.1 compiled Python modules/build reporting | Python-specific; Rust уже компилируется нативно | неприменимо |
| 1.5.1 HDLC/IFAC/HKDF parity tests | Rust crypto/wire тесты существуют; сравнить Python fixtures при изменениях | 5 |
| 1.5.1 shared medium hints / auto MTU | Частично: `traits.rs:optimise_mtu` есть; финальные hints/пороги Python сверить | 5 |
| 1.5.1 memory/CPU, traffic classes, HKDF/IFAC, locks, hashmap Links, hash reuse | Архитектурно частично: Rust HashMaps и crypto primitives; новые классы отсутствуют, оптимизации обосновывать benchmark | 4, 5 |
| 1.5.1 announce signature cache | В `actor/inbound.rs` validate вызывается для принятого announce; кеширование результатов требует анализа/измерения | 5 |
| 1.5.1 optimized HDLC deframer | Rust deframer существует; побайтовая совместимость и производительность проверяются отдельно | 5 |
| 1.5.1 inbound defaults / announce queuing tuning | Новые очереди отсутствуют; использовать окончательные Python constants | 4 |
| 1.5.1 stream Resource > MAX_EFFICIENT_SIZE | Воспроизвести потоковые источники и граничные размеры Rust | 6 |
| 1.5.1 rngit prefix/page init/large downloads | Самостоятельная утилита вне этого репозитория; общая Resource регрессия остаётся в этапе 6 | граница покрытия |
| 1.5.1 RSSI/SNR reporting | Воспроизвести interface metadata→transport→RPC | 7 |
| 1.5.1 non-epoll keepalive | Проверить служебные кадры всех Backbone-совместимых драйверов | 5 |
| 1.5.1 blocked IP list includes unblocked | Новый механизм обязан отдавать только реально заблокированные IP | 3 |
| 1.5.1 shared instance inter-app totals | `rnstatus.rs` суммирует interface stats; фильтрацию local/shared проверить | 7 |
| 1.5.1 minor rnsh/rnir/identity fixes | rnsh/identity воспроизвести; отдельная rnir утилита не обнаружена | 6, 7 |
| 1.5.1 AES exception description / Python2 umsgpack removal | Python-specific exception/dead-code изменения | неприменимо |
| 1.5.2 dataplane tuning | Требует этапа 5 с конечными параметрами 1.5.2 | 5 |
| 1.5.2 rngit block unidentified config example | Утилита вне покрытия; аналогичные rnsh authorization проверки остаются | граница покрытия |
| 1.5.2 Resource regression | Воспроизвести Python↔Rust, включая большой/многосегментный download | 6 |
| 1.5.2 I2P keepalive→transport | Воспроизвести текущий I2P/Backbone read path | 5 |

## Цепочка конфигурации

`config::Config` (`yaml_config.rs`) → `to_runtime_config` → `NormalizedConfig`
→ `interface_factory` / `get_post_init_for_config` → регистрация в transport.
Discovery publication отдельно строится в `discovery_config_for_interface` и
передаётся scheduler. Web API дополнительно использует обратное преобразование
`interface_from_normalized_section` / `common_from_normalized`; новые поля обязаны
сохраняться в обе стороны. UI и runtime mutation требуют отдельных проверок.

На исходном HEAD publication/bootstrap параметры недоступны через строгую YAML
схему. Gravity/queue/flapping/medium timeout отсутствуют далее по цепочке, их
нельзя закрыть только добавлением полей YAML. Статистика new queues/gravity/IP
должна доходить до local RPC, remote API и обеих веток rnstatus.

## Ход работ и проверки

- [x] Проверены HEAD/рабочие деревья и инструкции, прочитан changelog 1.3.9–1.5.2.
- [x] Составлена матрица с привязкой пробелов к реализации и этапам.
- [ ] Уточнить все строки «воспроизвести» целевыми проверками соответствующих этапов.
- [ ] Этап 1: незавершённые изменения YAML/runtime discovery в рабочем дереве.
- [ ] Этапы 2–7 и финальная интеграция.

Уже выполненные команды:

- `cargo test -p rns-runtime yaml_config --lib`: сборка прошла, **0 тестов**;
  фильтр неверен (модуль называется `config`). Не считать проверкой YAML.
- `cargo test -p rns-transport discovery::announcer --lib`: 13/13 успешно.
- `cargo fmt --all`: выполнено после первоначальных изменений.

Ни версия, ни заявленная совместимость пока не обновлены. Аппаратные,
межъязыковые и нагрузочные проверки текущего обновления пока не выполнены.
