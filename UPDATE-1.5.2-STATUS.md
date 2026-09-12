# Обновление до Reticulum 1.5.2: матрица и журнал

## Текущий статус

Функциональные этапы 1–7 пройдены; сейчас выполняется итоговая сверка всей
матрицы и стыков компонентов. Это не утверждение о завершённом релизе 1.5.2:
финальная сверка продолжает выявлять пропуски, которые исправляются здесь.
Версия пакетов остаётся 1.0.1, объявленная совместимость — 1.3.8 до завершения
проверки. Исходная матрица и промежуточные записи ниже исторические; строки
«отсутствует»/«этап открыт» следует читать вместе с последующими результатами.

| Этап | Текущее состояние |
|---|---|
| 0 | Исходная матрица составлена; окончательная классификация всех её строк продолжается. |
| 1 | Discovery YAML/runtime/API/UI перенесены; при финальной сверке дополнены stamp caches и очистка historical blackholes. |
| 2 | Gravity, выбор маршрута, Link rebalance и internal policy реализованы; есть packet interop, полная многодемонная матрица отложена. |
| 3 | Backbone fast-flapping и диагностика блокировок реализованы. |
| 4 | Приоритетные очереди, inflight path requests, фильтры и счётчики реализованы. |
| 5 | Функциональная часть flow control/MTU/keepalive закрыта; длительный soak и before/after нагрузка отложены. |
| 6 | API/timeouts/Channel/Resource перенесены; Python file responses доступны явно через PythonFile, а не сменой старого API. |
| 7 | rnstatus, rnsh, rncp rates завершены; rnpath remote restrictions соответствуют отсутствующим Python endpoints. |
| Финал | Короткие проверки API/UI/config/features и codec interop выполняются; полный workspace test, живая межъязыковая матрица и проверки других ОС не объявлены пройденными. |

Новые test-only файлы не включаются в коммиты. Длительные тесты и аппаратные
проверки не запускаются согласно текущему указанию пользователя. Существующий
backlog rnodeconf/flashing и документированные границы API не выдаются за
полный функциональный паритет Python.

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
| 1.3.9 rnsh security | Сверено с исходниками: identity gate и authorized state сохранены; fatal errors терминальны, ошибочный peer не завершает listener. Короткие allowed/denied проверки пройдены | 7 |
| 1.3.9 rnsh config/identity paths | Перенесено: раздельные --config/--rnsconfig, identity[.SERVICE] и allowed_identities в выбранном rnsh каталоге; миграция описана в CONFIG.md | 7 |
| 1.3.9 Backbone fast flapping | Отсутствует: listener/config в `rns-interface/src/backbone.rs` не ведут историю блокировок IP | 3 |
| 1.3.9 LOG_PATHING / logging | YAML/runtime/UI принимают 7 Pathing и 8 Extreme; оба соответствуют Rust TRACE. Граница шкалы и LOG_NONE описана в CONFIG | 7 / финальная сверка |
| 1.3.9 internal discovery | Расхождение: `apply_discovery_mode_autocorrect` разрешает только gateway/AP | 1 |
| 1.3.9 location script | Отсутствует в YAML и scheduler; Python `Discovery.py:get_interface_announce_data` запускает executable | 1 |
| 1.3.9 RESOURCE_RCL, reliability | Реализована отмена в protocol/runtime, есть regression tests; межъязыковое поведение воспроизвести | 6 |
| 1.4.0 transport persistence / interface hash / known destinations background cleaning и отказ от recombination | Snapshot writes вынесены с actor, SQLite работает на worker; shared client не пишет сетевой cache, disk recombination при save нет. Исправлен timeout used entries по last_used во всех backend и dirty после очистки. Legacy sweep остаётся целиком на actor; latency большого каталога не измерена | 0, 7 / граница производительности |
| 1.4.0 invalid discovery stamp cache | Закрыто при итоговой сверке: task-local FIFO до 2048 invalid digest entries | 1 / финальная сверка |
| 1.4.0 valid discovery cache / sequential validation | Task-local FIFO до 2048 valid digest/value entries; source policy и обновление store остаются на каждом событии | 1 / финальная сверка |
| 1.4.0 Link stale teardown / watchdog race | Частично: `rns-link/src/keepalive.rs:is_stale` учитывает outbound; проверить вызовы в runtime и ошибки приёма | 6 |
| 1.4.0 Backbone None-check / exception logging | Python-specific None/exception детали; Rust Result/Option; эквивалентные disconnect ошибки проверить с fast flapping | 3 |
| 1.4.0 stamp default 16 | Реализовано в `discovery/constants.rs` и runtime | 1, сохранить |
| 1.4.0 blocked IP ifstats | Отсутствует вместе с механизмом блокировок | 3, 7 |
| 1.4.0 reduced log noise | Воспроизвести уровни на локальной нагрузке; Rust tracing не требует копирования Python сообщений | 7 |
| 1.4.1 dynamic rebalance / gravity | Реализованы gravity selection, authenticated transit/local pending rebalance и привязка активного Link. Python↔Rust packet interop прошёл для 96/99-byte proofs; многодемонная сеть остаётся финальной интеграцией | 2 |
| 1.4.1 set_max_request_size | Отсутствует в Destination и runtime request admission | 6 |
| 1.4.1 max_response_size | Реализован `link_client.rs:request_with_metadata_limit`, включая Resource advertisement; сегменты проверить | 6, сохранить |
| 1.4.1 autoconnect mode/gravity/to_internal, default_gravity, interface gravity/to_internal | YAML/normalized/factory/registration, autoconnect defaults, child metadata, RPC и API/UI реализованы; I2P live inheritance не подтверждено | 2 |
| 1.4.1 rnstatus gravity display/sort | Gravity есть в runtime/local RPC/remote schema; вывод и сортировка CLI ещё не перенесены | 7 |
| 1.4.1 boundary→boundary/gateway PR | Реализовано; mode-матрицы с recursive/internal flags проходят в transport tests | 2 |
| 1.4.1 I2P tasks garbage collection | Python GC причина неприменима к Tokio; прочие minor I2P fixes требуют локального воспроизведения | 5 |
| 1.4.1 ingress burst active deadlock | Перенесён порог снятия burst: IC_DEQUE_MIN_SAMPLE вместо IC_BURST_MIN_SAMPLES. Также перенесены sustained hold и PR cooldown из 1.5.2; 22 коротких ingress tests прошли. Maintenance выпускает held announces независимо от burst-флага | 4 / закрыто |
| 1.4.1 memory efficiency / LOG_EXTREME | Воспроизвести нагрузку и числовые уровни, не переносить Python allocation детали без измерений | 5, 7 |
| 1.4.1 historical discovery blackhole cleanup | Закрыто: list_with_blackholes удаляет записи по network_id/transport_id; runtime и autoconnect используют актуальный control snapshot, expired TTL исключены | 1 / финальная сверка |
| 1.4.2 zero-bitrate recursive PR | Перенесён upstream offline guard (4760103a): recursive PR не ставится в очередь и не резервирует announce cap до online. Короткий actor case проверяет offline/bitrate=0 → online; аппаратная проверка не заявляется | 2 / закрыто |
| 1.4.2 Android slow blackhole filtering | Общее фильтрование discovery перенесено через HashSet snapshot; Python runtime-specific slowdown/60s cache не копируется. Android hardware/performance не проверялись | 1 / граница платформенной проверки |
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
| 1.5.0 inbound/PR processing, limiting, jobs, pending link/announce state fixes | Actor/inflight перенесены; финальная сверка дополнительно исправила sustained ingress, offline recursive PR, relay proof timeout и preemptive PR egress (порог 2). Остальные семантические изменения проверяются по upstream commits | 4, 6 |
| 1.5.0 Backbone EPOLL starvation | EPOLL Python implementation неприменима; справедливость Tokio read/write проверить нагрузкой | 5 |
| 1.5.0 receipt callback deadlock | Python receipts_lock неприменим к штатному runtime API: RegisterReceipt не принимает callback, DeliveryProof передаётся через destination channel без ожидания приложения. Прямые PacketReceipt callbacks синхронные; произвольный блокирующий callback не объявляется безопасным | 6 / архитектурная граница |
| 1.5.0 Link watchdog exception reset | Python watchdog_lock неприменим: Rust receive возвращает Result и не удерживает persistent receive lock. Пропуск malformed DATA/Response/ResourceReq/HMU, authenticated ADV teardown и продолжение после ошибок проверены короткими runtime cases. Произвольные panic пользовательских callbacks не входят в гарантию | 6 / архитектурная граница |
| 1.5.0 Resource multisegment cancellation / part alignment/rebinding | Частично: сегменты/RCL реализованы; проверить индексы и повторное связывание Python↔Rust | 6 |
| 1.5.0 stale BLE device reference | Закрыто статической сверкой: connect_rnode заново вызывает resolve_ble_target; отсутствие кандидата возвращает Err, нет fallback на прежний conn. Android native bridge не хранит BLE device в Rust. Кеш платформенного BLE backend и аппаратное переподключение не проверялись | 6 / граница платформенной проверки |
| 1.5.0 retained ratchet cleanup | Реализовано ограниченное кольцо и retention в `rns-identity/src/ratchet.rs`; сохранить описанную границу 512 и повторить lifecycle | 6 |
| 1.5.0 invalid rnstatus stats / burst count | Частично: optional decode/defaults и burst flags есть; сравнить local/remote JSON | 7 |
| 1.5.0 miscellaneous packet/link/interface fixes | Packet.py/interface guards и RequestReceipt сверены. LinkClosed/Resource teardown исправлены. Keepalive admission централизован; initiator игнорирует request без изменения активности, ответы ограничены last_outbound. Оставшиеся Link receive/error paths ещё требуют сверки | 4–6 |
| 1.5.0 rngit Windows resources | Отсутствующая Rust утилита; общие Resource семантики остаются в этапе 6 | граница покрытия |
| 1.5.0 rnodeconf WiFi summary | Закрыта исправленная upstream ветка режима: `--info` выводит ровно одно состояние Station/AP/Disabled и канал; короткие EEPROM обрабатываются безопасно. Полный config-sector summary не заявляется | 7 |
| 1.5.0 speedtest stale link | Rust example не прерывает цикл на Stale. Исправлен runtime delivery-proof wait: валидный proof восстанавливает активность, закрытие Link завершает ожидание сразу. Rust использует окно подтверждений, не Python untracked flood | 6 |
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
| 1.5.1 announce signature cache | Реализовано в actor: `PreparedInbound` переносит `VerifiedAnnounce` от admission к dispatch без повторной криптографической проверки. Python кеширует флаг в одном Packet, не между пакетами; глобальный кеш не требуется | 5 |
| 1.5.1 optimized HDLC deframer | Rust deframer существует; побайтовая совместимость и производительность проверяются отдельно | 5 |
| 1.5.1 inbound defaults / announce queuing tuning | Новые очереди отсутствуют; использовать окончательные Python constants | 4 |
| 1.5.1 stream Resource > MAX_EFFICIENT_SIZE | Воспроизвести потоковые источники и граничные размеры Rust | 6 |
| 1.5.1 rngit prefix/page init/large downloads | Самостоятельная утилита вне этого репозитория; общая Resource регрессия остаётся в этапе 6 | граница покрытия |
| 1.5.1 RSSI/SNR reporting | Цепочка RNode/RNodeMulti → owned InboundPacket → record_packet_metrics → GetPacketRssi/Snr и RPC существует. Метрики копируются до очереди; Python исправление потери через mutable interface fields неприменимо к этой архитектуре. Аппаратная проверка не заявляется | 7 |
| 1.5.1 non-epoll keepalive | Проверить служебные кадры всех Backbone-совместимых драйверов | 5 |
| 1.5.1 blocked IP list includes unblocked | Новый механизм обязан отдавать только реально заблокированные IP | 3 |
| 1.5.1 shared instance inter-app totals | `rnstatus.rs` суммирует interface stats; фильтрацию local/shared проверить | 7 |
| 1.5.1 minor rnsh/rnir/identity fixes | rnsh сверено по diff 1.3.8..ea98db4f; пути, auth, повторная идентификация, копирование argv, timeout проверены. Python import/logging границы описаны; отдельная rnir отсутствует | 6, 7 |
| 1.5.1 AES exception description / Python2 umsgpack removal | Python-specific exception/dead-code изменения | неприменимо |
| 1.5.2 dataplane tuning | Требует этапа 5 с конечными параметрами 1.5.2 | 5 |
| 1.5.2 rngit block unidentified config example | Утилита вне покрытия; аналогичные rnsh authorization проверки остаются | граница покрытия |
| 1.5.2 Resource regression | Воспроизвести Python↔Rust, включая большой/многосегментный download | 6 |
| 1.5.2 I2P keepalive→transport | Закрыто: i2p_read_loop фильтрует empty HDLC frames и в initial_data, и при чтении; probes обновляют last_read, но не попадают в transport. Scoped watchdog/reader/writer уже перенесены; живой SAM/I2P не проверялся | 5 |

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
- [x] Этап 1: YAML/runtime publication, internal mode, location_cmd, API/UI.
- [x] Этап 2: gravity/internal, transit/local Link rebalance, interface binding; границы interop ниже.
- [x] Этап 3: Backbone fast-flapping, YAML/API/UI, RPC и локальный/удалённый статус.
- [x] Функциональные этапы 4–7 с границами, указанными в журнале.
- [ ] Финальная сверка матрицы и интеграция; текущие результаты — в конце журнала.

Уже выполненные команды:

- `cargo test -p rns-runtime yaml_config --lib`: сборка прошла, **0 тестов**;
  фильтр неверен (модуль называется `config`). Не считать проверкой YAML.
- `cargo test -p rns-transport discovery::announcer --lib`: 13/13 успешно.
- `cargo fmt --all`: выполнено после первоначальных изменений.

### Этап 1: реализация и результаты

Добавлены типизированные flat discovery-поля, bootstrap_only и
ignore_config_warnings; преобразования YAML↔normalized↔Web API сохраняют их.
Internal mode допустим; минуты интервала ограничены снизу пятью минутами.
Location/reachability executables обновляют metadata перед due announce;
ошибка изолирована на одном интерфейсе, выход ограничен 4096 байтами и 5 секундами.
Добавлены live scheduler refresh/deregister и сохранение API rollback пути.

Обнаружены два дополнительных блокирующих пробела: штатный runtime не
устанавливал stamper, а discovery destination не регистрировалась как локальная
(transport отбрасывал outbound announce). Исправлены оба. Native stamper
сопоставлен с реальным LXStamper; embedding override сохранён. PoW выполняется
в blocking worker с ограниченным числом попыток. Raw runtime receiver default
исправлен с 14 на 16, как уже было в typed YAML и документированном контракте.

Проверки:

- `cargo test -p rns-transport discovery --lib`: 84 passed.
- `cargo test -p rns-runtime --features api --lib`: 219 passed, 2 ignored
  до добавления API roundtrip и live scheduler regression; дополнительные тесты
  запускались отдельно.
- `cargo test -p rns-runtime --features api discovery_api_fields --lib`: 1 passed.
- `cargo test -p rns-runtime --features api,serial,rnode-tcp,sqlite-bundled discovery --lib -- --skip discovery_python_receiver`: 9 passed.
- `cargo check --workspace --all-targets`: успешно; существующие предупреждения
  о database_path и tracing_subscriber::prelude остаются.
- `cargo fmt --all -- --check`: успешно.
- `node --test crates/rns-runtime/web/app.test.js`: exit 0.
- `cargo test -p rns-runtime --features api discovery_python_receiver --lib -- --ignored --nocapture`:
  passed; два реальных Rust→TCP→Python receive сценария, открытый и зашифрованный.
  Python подтвердил signature/stamp, IFAC, operator LXMF и executable coordinates.
  Тест использует stamp target 8 для скорости; native HKDF vector отдельно совпал
  с Python (workblock 5120 bytes, SHA256 7c06f15571960ba62e26a1d51bc4f9c86ebc44810701d232af0fe9a6c10f9e61).

Первый live тест выявил отсутствие регистрации destination; после исправления
он прошёл. Сетевые tests внутри sandbox получали PermissionDenied, повторный
запуск с разрешёнными loopback сокетами прошёл. `python3` здесь версии 3.6.15,
не импортирует эталон; тесты используют установленный `python3.11`.

Границы этапа: I2P auto b32 publication не подтверждена, поддерживается явный
reachable_on. Радиометаданные покрыты преобразованиями, аппаратные тесты не
выполнены. Rust принимает строковую modulation (Python config/formatter
противоречат друг другу), ограничивает команды и исправляет longitude typo.
Оптимизации receiver caches и остальные открытые строки матрицы не закрыты
этим этапом. Полная нагрузочная проверка и этапы 2–7 остаются впереди.

Версия и заявленная совместимость пока не обновлены.

### Этап 2: проверенная конфигурационная и транзитная часть (этап не завершён)

- Добавлены signed `default_gravity` / `gravity`, autoconnect mode/gravity/
  announces_to_internal и интерфейсный tri-state `announces_to_internal`.
  Поля проходят YAML↔normalized↔API; интерфейсные значения доступны в форме UI.
- Configured interface наследует default_gravity, явный 0 его переопределяет.
  Autoconnect gravity по умолчанию 0 независимо от default_gravity, режим
  gateway при transport и full иначе. Python positive integer autoconnect
  announces_to_internal представлен boolean в YAML.
- Регистрация дочерних соединений наследует gravity через parent metadata;
  announces_to_internal остаётся None, как в Python TCP/Backbone/Auto/I2P.
  Проверено на runtime registration; SAM/I2P и радио live не запускались.
- Выбор announce-пути повторяет Transport.py: gravity разрешает замену при
  равном timestamp и не большем hops; строго выше gravity, включая signed
  значения и повтор того же random blob. Свежесть/expiry/unresponsive и
  подавление отказавшего интерфейса сохраняют свои существующие правила.
- Source-side announces_to_internal=True разрешает boundary→internal;
  False/None не запрещают прочие режимы. announces_from_internal=False
  остаётся отдельным egress-фильтром.
- Unknown PR с boundary отправляется только в boundary/gateway, если
  recursive_prs не переопределяет ограничение. Ingress/egress limits сохранены.
- Транзитный pending Link допускает изменение remaining_hops по LRPROOF
  только с правильного интерфейса и после проверки подписи. Уже validated
  Link, неверная подпись и неверный интерфейс не изменяют hops/path.
- Gravity и announces_to_internal добавлены в transport stats, shared RPC,
  remote-management schema и Web API. Старые ответы читаются с 0/None.

Проверки:

- `cargo test -p rns-transport --lib --quiet`: 401 passed. Один старый тест
  запрета boundary→gateway обновлён по Python 1.5.2; новые матрицы gravity,
  modes и transit proof authentication прошли.
- `cargo test -p rns-runtime --features api --lib --quiet`: 224 passed,
  2 ignored. Первый sandbox-прогон: 215 passed, 9 PermissionDenied на
  loopback-сокетах; повтор с разрешением прошёл полностью.
- `cargo check --workspace --all-targets`: успешно (старые warnings сохранены).
- `node --test crates/rns-runtime/web/app.test.js`: exit 0.

- `cargo test -p rns-runtime --features api,serial,rnode-tcp,sqlite-bundled gravity --lib --quiet`: 3 passed.
- `cargo fmt --all -- --check` и `git diff --check`: успешно.

Остаток этапа 2: перенос authenticated rebalance в локальный pending Link,
проверка привязки активных Links при смене announce-пути и Python↔Rust
многоузловые сценарии. I2P server использует placeholder parent ID 0 — нужно
проверить наследование при нескольких таких серверах отдельно; этот пробел
не выдаётся за закрытый. Sorting/display rnstatus остаются в этапе 7.
Текущие route-матрицы основаны на чтении Python 1.5.2, а не на live interop.

### Этап 2: локальные Links и завершение функциональной реализации

Предыдущий остаток закрыт на уровне runtime/transport и изолированного packet
interop. `Link::validate_proof_with_hops` проверяет подпись, длину и mode до
изменения hop estimate. Неверный mismatched proof оставляет Link Pending;
установленный Link повторно не перебалансируется. Runtime запрашивает
нормализацию hops у actor (включая shared-instance исключения), подтверждает
путь только после успешной аутентификации и привязывает Link к proof interface.
Привязка сохраняется при смене destination-маршрута и потере интерфейса;
второе подтверждение не меняет её. Чужой интерфейс отбрасывается до dedup.
Завершение, отказ и отмена освобождают временную регистрацию; cleanup при
заполненной очереди планируется в Tokio, а не теряется из-за try_send.

Дополнительно найдены и исправлены:

- 96-byte proof ранее разбирался, но проверялся как подписанный с signalling.
  Теперь длина wire payload определяет подписанные байты; усечение 99-byte
  proof без новой подписи отклоняется. Отсутствующий MTU signalling не меняет MTU.
- I2P servers/children использовали ID/parent_id 0. Сервер получает уникальный
  ID, а runtime передаёт заранее выделенный ID, сохраняя API teardown target.
  Старый spawn API сохранён; child metadata привязана к правильному серверу.

Interop: Python subprocess создаёт настоящий подписанный announce, Link ID,
ECDH и LRPROOF, проверяет encrypted RTT и application data. Rust actor сначала
учит путь в 4 hops, принимает proof в 2 hops, затем тот же announce с большей
gravity меняет destination interface. Действующий Link продолжает обмен через
старый интерфейс; Python→Rust encrypted reply успешно принят. Проверены
96- и 99-byte proofs и предварительно подложенный forged proof.

Ограничения: hop bytes/интерфейсы задаются тестовой топологией каналов, обмен с
Python идёт по stdin/stdout без сетевых сокетов. Это проверка реальных пакетов
и криптографии, не полный прогон нескольких Python/Rust демонов. Такой прогон
остаётся в финальной интеграции; SAM-сеть и физические интерфейсы не запускались.

Результаты проверок этой части:

- `cargo test -p rns-link --lib --quiet`: 97 passed.
- `cargo test -p rns-transport --lib --quiet`: 403 passed.
- `cargo test -p rns-interface --lib --quiet`: 175 passed; тест unique/reserved
  I2P IDs дополнительно перезапущен после расширения assertions, 1 passed.
- `cargo test -p rns-runtime --features api --lib --quiet`: 227 passed,
  4 ignored (сетевые тесты выполнены с разрешёнными loopback-сокетами).
- `cargo test -p rns-runtime --features api link_rebalance_and_active_route_binding --lib -- --ignored --nocapture`:
  оба Python packet interop теста passed (96/99-byte proofs).
- `cargo test -p rns-runtime --features api,serial,rnode-tcp,sqlite-bundled link_client --lib --quiet`:
  12 passed, 2 interop ignored.
- `cargo check -p rns-runtime --no-default-features --features client`: успешно.
- `cargo check --workspace --all-targets`, `cargo fmt --all -- --check`,
  `git diff --check`: успешно. Старые warnings database_path и
  tracing_subscriber::prelude не изменялись.
- Python reference HEAD остался ea98db4f, рабочее дерево чистое; используется
  `python3.11 -B`, процессы не записывают bytecode в эталон.

Следующий этап — Backbone fast-flapping (этап 3). Версия и полная заявленная
совместимость 1.5.2 пока не меняются.

## Этап 3 — Backbone fast-flapping

Перенесена семантика `BackboneInterface.py`: defaults true / 20 секунд /
grace 5 / 720 минут; короткое соединение определяется строгим `< threshold`,
блокировка — `count > grace`, истечение — строго после block time с последнего
короткого disconnect. Обычный долгий сеанс не сбрасывает историю; отвергнутое
подключение не продлевает блокировку. История IP общая для listeners процесса,
политика каждого listener своя. В Rust используется монотонное время; YAML
отклоняет отрицательные и неограниченные длительности, включая overflow при
переводе минут в секунды. IPv4 и IPv6 представлены типизированным IpAddr.

Настройки проходят YAML → normalized → factory → driver, round-trip API и
форму Web UI. Client сохраняет параметры, но защита применяется только к
listener. Accepted backend завершает read/write совместно; Drop guard учитывает
разрыв и отмену задачи. При отказе соединения дочерний интерфейс не создаётся.

Driver diagnostics предоставляют живой снимок после очистки истёкших записей;
transport/RPC получают count и list из одного снимка. `blocked_ips` и
`blocked_ip_list` доступны через локальный RPC, remote management, Web API/UI,
локальный и удалённый `rnstatus-rs`, включая JSON. Текстовый статус показывает
ненулевое число, `--blocked-ips` добавляет адреса. Старые ответы без полей
разбираются как 0 / пустой список. Другие драйверы не ведут такую диагностику.

Проверки:

- `cargo test -p rns-interface --lib --quiet`: 179 passed. Новый loopback-тест
  проверяет grace, реальный EOF при блокировке, отсутствие регистрации
  отвергнутого peer и выключенную защиту. Три теста с управляемым Instant
  проверяют точные границы threshold/expiry, долгие соединения, общую таблицу,
  выключенную защиту и отсутствие продления при rejected attempts.
- `cargo test -p rns-transport --lib --quiet`: 403 passed.
- `cargo test -p rns-runtime --features api --lib --quiet`: 230 passed,
  4 ignored; после расширения API assertions повторно выполнен фильтр
  `backbone`: 13 passed. Проверены YAML defaults/round-trip/валидация,
  factory minutes→seconds, API и RPC сериализация новых полей.
- `cargo test -p rns-tools --bin rnstatus-rs --quiet`: 9 passed; проверены
  новый CLI flag, remote snapshot и fallback для старых peers.
- `cargo check --workspace --all-targets` и client-only build: успешно.
- `cargo check -p rns-runtime --features api,serial,rnode-tcp,sqlite-bundled`:
  успешно.
- `cargo fmt --all -- --check`, `git diff --check`,
  `node --test crates/rns-runtime/web/app.test.js`: успешно.

Ограничения: Web-тесты проверяют существующие JS helpers, а не интерактивный
браузерный сценарий. Отдельный многопроцессный Python↔Rust тест блокировок
не запускался: policy сверена с локальными исходниками Python, сокетное
поведение проверено Rust loopback-тестом. Эталон ea98db4f остался чистым.

Следующий этап — приоритетные входящие очереди и path requests (этап 4).
Версия и полная заявленная совместимость 1.5.2 пока не меняются.

## Этап 4, часть 1 — контейнер очередей и сверка с Python

Этап 4 **не завершён**. Добавлен `rns-transport/src/inbound_queue.rs`:
четыре ограниченные FIFO с порядком Data → Announce → PathRequest →
IngressLimited, defaults 1024/128/128/8, независимым drop-tail и согласованным
снимком высот/потерь. Положительные размеры проверяются без предварительной
аллокации всей ёмкости; переполнение суммарного размера отклоняется. Удаление
пакетов интерфейса сохраняет порядок остальных и не считается overflow.

Это actor-owned контейнер, **пока не подключённый к рабочему actor**. YAML
параметры не добавлялись, чтобы не предоставлять неработающие настройки.
Контейнер не проверяет пакеты, не обеспечивает async wakeup и не включает
Backbone high-water throttling (последнее относится к этапу 5).
Строгий Python приоритет допускает starvation младших классов при непрерывном
data-потоке; round-robin не подставлялся вместо эталонной семантики.

Проверки:

- `cargo test -p rns-transport --lib --quiet`: 408 passed, включая пять новых
  тестов defaults/границ, FIFO/приоритета, независимого переполнения,
  вытеснения младших классов и удаления интерфейса; 40 000 попыток вставки
  в перегруженные очереди сохраняют заданный предел.
- `cargo test -p rns-transport --test inbound_queue_python -- --ignored`:
  1 passed. Сравнены результаты и snapshots после каждой из 4300 операций
  с настоящим классом `InboundQueues` из локального Python ea98db4f.
  Oracle извлекает AST только этого класса, не запускает Reticulum, сокеты
  или хранилище. `/usr/bin/python3.11 -B` не изменяет reference; throttling
  намеренно исключён высоким watermark. Тест по умолчанию ignored из-за
  зависимости от внешнего checkout; пути задаются RNS_PYTHON_ROOT и RNS_PYTHON_BIN.
- `cargo check --workspace --all-targets`: успешно; прежние warnings
  database_path и tracing_subscriber::prelude не изменялись.
- `cargo fmt --all -- --check`, `git diff --check`: успешно.

Подтверждённые точки следующей интеграции:

1. Оба цикла `actor/mod.rs::run` и `actor/sqlite.rs::run_sqlite` сейчас читают
   один канал. Управление/shutdown нужно отделить на входе, не просто поставить
   ещё четыре очереди за тем же забитым каналом. Сохранить SQLite shutdown.
2. До постановки в классовую очередь нужны IFAC/MTU/hop/filter checks,
   валидация announce, ingress admission и дедупликация PR tag. Существующие
   проверки в `actor/inbound.rs` нельзя повторно выполнять при drain: это
   удвоит учёт частот и может отфильтровать уже принятый пакет.
3. Rust `DiscoveryPathRequest` хранит один requesting_interface; Python 1.5.2
   хранит список requesting_interfaces и engaged. Нужен отдельный inflight
   gate до очереди, без подмены `path_requests` или pending_local requests.
4. Python gate timeout равен 45 секундам; Rust константа равна 120 и используется
   также для хранения duplicate tags. Нельзя менять её механически, не разделив
   эти назначения и не проверив очистку и повторные запросы.
5. Далее провести YAML → runtime → обе actor-петли → статистика/API/UI,
   тесты перегрузки/управления/shutdown, batching и ответов всем ожидающим.

## Этап 4, часть 2 — несколько ожидающих discovery path request

Рабочая обработка recursive discovery теперь хранит уникальный список
requesting_interfaces вместо одного получателя. Новый tag для уже ожидаемой
destination добавляет интерфейс, но не повторяет поиск и не продлевает timeout.
Повторный tag не добавляет получателя; ingress-limited интерфейс также не
присоединяется. Список пополняется только зарегистрированными интерфейсами;
повторы одного интерфейса не увеличивают память. Истёкшее ожидание удаляется
при новом запросе, даже если maintenance tick ещё не выполнялся.

Валидный announce доставляет PathResponse каждому ожидающему интерфейсу и
удаляет запись ожидания. Неверная подпись не потребляет ожидающих. При удалении
интерфейса из списка исключается только он; пустая запись удаляется. Это
исправляет прежний Rust-тест, ошибочно утверждавший, что Python сохраняет
discovery entry после ответа: в 1.5.2 `Transport.py:2436` делает pop, затем
отвечает всем requesting_interfaces.

Учёт входящей PR frequency перенесён после дедупликации tag, согласно ранней
обработке `Transport.py:1849–1858`. Прежний тест ожидал учёт повторных tagged
PR; теперь он проверяет отсутствие такого учёта.

Проверки:

- `cargo test -p rns-transport --lib --quiet`: 411 passed; добавлены тесты
  двух получателей, повторных и ingress-limited запросов, единственного
  поиска, неверной/верной подписи, удаления интерфейсов и истечения до tick.
- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  426 passed, 1 ignored.
- `cargo test -p rns-runtime --features api --lib --quiet`: 230 passed,
  4 ignored.
- `cargo check --workspace --all-targets`, `cargo fmt --all -- --check`,
  `git diff --check`: успешно.

Этап 4 всё ещё **не завершён**. Эта часть объединяет запросы уже начатого
recursive discovery. Отдельные inflight_path_requests, состояние engaged=false
до обработки очереди, разделение 45-секундного gate и хранения tags, ранняя
классификация, очередь управления, YAML и live-статистика ещё не подключены.
Имеющиеся path_requests и pending_local_path_requests не заменялись.
Многопроцессный Python↔Rust сценарий batching не запускался; ожидания сверены
с исходниками ea98db4f, тесты выполнялись внутри Rust actor. Эталон не изменён.

## Этап 4, часть 3 — отдельный inflight gate

Добавлен отдельный `inflight_path_requests`, не заменяющий локальные
`path_requests`, discovery waiters и pending_local requests. Регистрация
происходит после проверки tag и до обработки назначения. Повторный запрос
не продлевает gate: неограниченный ingress добавляет интерфейс к ожидающим,
при необходимости создавая discovery entry с `engaged=false`; ограниченный
ingress не добавляет получателя. Recursive search устанавливает engaged=true.

Gate теперь 45 секунд, как в Python 1.5.2. Точная граница сохраняется:
на 45 секундах запрос ещё in-flight, строго позже допускается новый.
15-секундное ожидание discovery независимо: его истечение не снимает gate.
Старый тест, разрешавший поиск только после истечения discovery, исправлен.
Время хранения tags выделено в `DISCOVERY_PR_TAG_RETENTION=120`, поэтому
изменение gate не сокращает существующую историю дубликатов. Это пока старый
Rust time-based механизм tags, не Python ротация двух наборов; полная ранняя
дедупликация остаётся пунктом дальнейшей интеграции.

Gate снимается при валидном принятом announce, локальном ответе, постановке
кешированного ответа на отправку, maintenance timeout, DropPathTable и сбросе
shared connection. При удалении исходного интерфейса запись сохраняется за
оставшимся получателем либо удаляется вместе с последним ожидающим. Состояние
не сохраняется на диск. Введён Rust safety cap 32 000 незавершённых destinations:
при заполнении новые отклоняются, без обхода всей таблицы на каждый пакет;
maintenance освобождает истёкшие записи. Это ограничение отдельной таблицы,
не заявление об ограниченности всего inbound pipeline.

Проверки:

- `cargo test -p rns-transport --lib --quiet`: 414 passed. Новые проверки:
  точная граница 45 секунд с управляемым временем, отсутствие продления,
  engaged=false, ограниченный ingress, cap, удаление интерфейсов, сброс,
  независимое истечение gate и tag history. Расширены проверки локального,
  кешированного и полученного по сети ответа.
- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  429 passed, 1 ignored (до дополнительных assertions local/cache/reset).
- `cargo test -p rns-runtime --features api --lib --quiet`: 230 passed,
  4 ignored.
- `cargo check --workspace --all-targets`: успешно.
- `cargo fmt --all -- --check`, `git diff --check`: успешно.

Этап 4 **не завершён**: приоритетные очереди ещё не включены в actor, нет
отдельного управляющего входа, YAML размеров, ранних protocol counters и
live-статистики очередей. Admission helper сейчас вызывается синхронным
обработчиком PR; его перенос перед очередью должен сохранить однократный
учёт tag/частот и обработку engaged. Межъязыковой тест lifecycle gate не
запускался; семантика сверена с локальным `Transport.py` ea98db4f, reference
остался чистым. Adaptive medium timeout относится к последующим этапам.

## Этап 4, часть 4 — отдельный канал управления в runtime

Добавлен `TransportActor::new_with_control_channel`: независимые ограниченные
входы для driver traffic (4096 сообщений) и runtime/API/RPC команд (256).
Обычный `new`/`new_with_capacity` сохраняет прежний single-channel API для
существующих пользователей и тестов. Full runtime и client-only runtime
переведены на новый конструктор; встроенные, динамические и discovery-драйверы
получают пакетный sender, а публичный ReticulumHandle.transport_tx и shutdown
работают через управляющий вход. Ввод/вывод сообщений по-прежнему типизирован
TransportMessage; это разделение runtime-маршрутов, не новый wire-протокол.

Обе actor-петли (memory и SQLite) обслуживают каналы независимо через fair
Tokio select. Закрытие одного sender не останавливает оставшийся канал;
закрытие обоих завершает actor. Явный Shutdown с любого входа завершает работу;
SQLite сохраняет существующее дренирование принятой storage-очереди и shutdown
worker. Счётчик queued_messages учитывает оба входа.

Проверки:

- Новые тесты полностью заполняют пакетный вход 4096 сообщениями до запуска
  actor, проверяют независимый приём RPC, затем поддерживают поток пакетов.
  Ответ RPC и shutdown завершаются в пределах двухсекундных test deadlines.
  Один и тот же сценарий проходит для memory и SQLite actor; отдельно проверено
  независимое закрытие каждого входа и остановка после закрытия обоих.
- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  432 passed, 1 ignored; целевой фильтр `separate_`: 3 passed.
- `cargo test -p rns-transport --lib --quiet`: 416 passed.
- `cargo test -p rns-runtime --features api --lib --quiet`: 230 passed,
  4 ignored.
- `cargo check --workspace --all-targets`, client-only build и
  `cargo check -p rns-runtime --features api,serial,rnode-tcp,sqlite-bundled`:
  успешно. Прежние warnings не изменялись.
- `cargo fmt --all -- --check`, `git diff --check`: успешно; Python reference
  остаётся чистым.

Граница этой части: тестовый flood — поток malformed packets для проверки
изоляции каналов, не benchmark throughput/криптографической нагрузки. Команды
driver lifecycle пока идут вместе с driver traffic; команды runtime, включая
явное отключение/статус/shutdown, используют независимый вход. Массовый outbound
от приложений ещё разделяет вход с runtime-командами. Приоритеты внутри control
channel не добавлялись, глобальный порядок между двумя каналами не гарантируется.

Этап 4 **не завершён**: классовый контейнер InboundQueues пока не включён,
нужны ранняя валидация/классификация до постановки, YAML размеров и live queue
статистика. Следующий шаг — связать обработку с четырьмя очередями без повторного
учёта IFAC, dedup, ingress frequency и inflight gate при drain.

## Этап 4, часть 5 — рабочие классовые очереди и ранний допуск

В actor, создаваемом `new_with_control_channel` (full/client runtime), включены
четыре FIFO: Data → Announce → PathRequest → IngressLimited. Используются
defaults 1024/128/128/8, независимый drop-tail и счётчики контейнера. Переполнение
также увеличивает общий channel_drops; queued_messages включает эти очереди.
Обычный single-channel конструктор сохраняет немедленную обработку для
совместимости существующих встраиваний. YAML размеров пока не добавлен.

Обработка разделена на admission и dispatch. PreparedInbound имеет закрытые
поля и не реализует Clone. До очереди выполняются IFAC, разбор заголовка,
hop checks, проверка известных packet hashes, подпись/blackhole и ingress
announce, PR tag/частоты/inflight. Класс PR определяется после ingress-проверки.
При dispatch используются разобранный заголовок, проверенные данные announce
и подготовленный PR — IFAC, подпись, счётчики и gate не выполняются повторно.
SQLite получает тот же admission token, загружает необходимые записи и не
проверяет повторно IFAC над уже очищенными байтами. Cache-dependent фильтр
Requested-client проверяется после загрузки SQLite записи.

Хеш принятого пакета записывается при dispatch, как `Transport._inbound` Python,
а не при попытке постановки: отброшенная при переполнении передача не мешает
приёму её ретрансляции. Существующие исключения dedup сохранены. Подпись announce
проверяется до очереди, binding destination к identity остаётся на dispatch
после загрузки кеша; это соответствует разделению ранней signature validation
и полной обработки. IFAC-flag на интерфейсе без IFAC теперь отклоняется явно.

Освобождаемые held-announces проходят отдельный доверенный вход с уже снятым
IFAC и классом IngressLimited, как Python Interface.py:295. Это исправляет
прежнюю повторную проверку IFAC при release. Локальный replay из announce cache
также использует классовую очередь в новом режиме.

Очередь очищается от пакетов удаляемого интерфейса и при shared-state reset.
На dispatch повторно проверяется актуальность interface TX endpoint, Link
binding, blackhole и локальной destination: команды могут изменить их во время
ожидания, в том числе в storage worker. Закрытие обоих каналов дочитывает уже
допущенные пакеты; явный Shutdown не обязан обрабатывать всю сетевую очередь.
SQLite сохраняет завершение уже принятых storage jobs.

Проверки:

- Целевые тесты проверяют все четыре класса, строгий порядок выдачи,
  независимое переполнение, malformed/signature rejection до очереди,
  PR dedup, повтор передачи после overflow, IFAC без повторной проверки,
  held-release в IL и отказ для заменённого интерфейса.
- PR повторно проверяет актуальную ingress policy при dispatch без нового
  frequency sample; тест покрывает burst, начавшийся во время ожидания.
- Memory и SQLite async loops проверены с queued IFAC announce и закрытыми
  входами: callback вызывается ровно один раз. Тест независимого управления
  теперь использует уникальные корректные DATA-заголовки вместо пустых кадров.
- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  440 passed, 1 ignored.
- `cargo test -p rns-transport --lib --quiet`: 423 passed.
- `cargo test -p rns-runtime --features api --lib --quiet`: 230 passed,
  4 ignored. В одном запуске 9 socket-тестов получили sandbox PermissionDenied;
  повтор с разрешением на loopback прошёл полностью.
- `cargo check --workspace --all-targets`, комбинированная сборка
  `api,serial,rnode-tcp,sqlite-bundled`, client-only build,
  `cargo fmt --all -- --check`, `git diff --check`: успешно. Новых warnings нет.
- `cargo test -p rns-runtime --features api link_rebalance_and_active_route_binding --lib -- --ignored --nocapture`:
  2 Python packet interop passed (до последних изменений held-release/hash
  recording). Это регрессия Link proof, не interop нагрузки очередей.

Этап 4 **не завершён**. Следующие пункты: YAML qlen_in_*, live queue counters
в RPC/API/UI, ранние protocol/IFAC violation counters и проверка MTU/filter
семантики; завершение сверки ротации PR tags. Перед admission остаётся bounded
driver channel; SQLite имеет собственную bounded storage queue. Приоритет
относится к выбору из четырёх классов и не отменяет уже выполняющийся storage
job. Throughput, задержки и dataplane throttling относятся к этапу 5; их
эффективность этим изменением не заявляется.

## Этап 4 — YAML-размеры входящих очередей

Добавлены `reticulum.qlen_in_data`, `qlen_in_announce`, `qlen_in_pr`,
`qlen_in_il`: YAML → normalized config → runtime config → создание actor.
Цепочка подключена как в полном runtime (включая SQLite), так и в client-only
сборке. Defaults берутся из общего `InboundQueueLimits`: 1024/128/128/8 пакетов.
Существующий конструктор actor сохраняет defaults; новый принимает проверенные
лимиты при создании, не меняя raw/control каналы 4096/256 сообщений.

Проверяются положительные целые значения и отсутствие переполнения суммы
`usize`, в том числе при прямой передаче normalized config. Python
`Reticulum.py:705–720` применяет только положительные overrides и игнорирует
неположительные; Rust отклоняет их явно, в соответствии со строгой YAML-схемой.
Различие документировано в CONFIG.md. Предварительного выделения максимальных
буферов нет; тест создаёт actor с предельной допустимой суммой размеров.

Изменения применяются после перезапуска; Web API `restart_required` учитывает
отличие сохранённых лимитов от работающего runtime. Отдельные поля формы
редактирования и live-статистика очередей этим изменением не добавлены.
Обновлены пример YAML и справочник CONFIG.md.

Проверки:

- `cargo test -p rns-runtime --test inbound_queue_config --quiet`: 3 passed.
- `cargo test -p rns-runtime --no-default-features --features client --test inbound_queue_config --quiet`:
  3 passed. Проверены round-trip, defaults, частичный override, нулевые,
  отрицательные, нецелые/нечисловые значения, overflow и normalized bypass.
- Actor regression с лимитами 1/2/3/4 проверяет фактическую ёмкость каждого
  класса, независимые drops, порядок dispatch и неизменные raw/control каналы.
- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  440 passed, 1 ignored.
- `cargo test -p rns-runtime --features api --lib --quiet`: 231 passed,
  4 ignored. Повтор выполнен с разрешением на loopback после 9 sandbox
  PermissionDenied; тест restart_required покрывает изменение каждого лимита.
- `cargo check --workspace --all-targets`, сборка runtime с
  `api,serial,rnode-tcp,sqlite-bundled`, `cargo fmt --all -- --check`,
  `git diff --check`: успешно. Имеющиеся warnings не затрагивались.

Этап 4 остаётся частичным: далее live queue counters в RPC/API/UI, ранние
protocol/IFAC violation counters, MTU/filter semantics и ротация PR tags.
Этапы 5–7 и итоговая интеграция ещё не завершены; версия не изменена.

## Этап 4 — live queue counters в RPC/API/Web UI

Добавлен `GetInboundQueueStats`: actor возвращает единый snapshot текущих
высот, drops и настроенных capacities четырёх классов. Legacy single-channel
actor возвращает None, а не фиктивные нулевые показатели активных очередей.
Очереди и счётчики принадлежат локальному actor; raw/control/storage каналы
не входят в эти показатели. Очистка после dispatch не сбрасывает drops.

Shared-instance RPC `interface_stats` дополнен top-level полями Python 1.5.2
`rxqt/rxqd/rxqa/rxqp/rxqil`, `rxqtd/rxqdd/rxqad/rxqpd/rxqild` и пятью
`*qpressure` по `Reticulum.py:1596–1610`. Pressure — доля ёмкости, не проценты.
Общий drop counter насыщается на u64::MAX для MessagePack, индивидуальные
счётчики уже насыщаемые. Существующий Rust decoder по-прежнему возвращает
InterfaceStats(Vec), игнорируя дополнительные поля: пользователи прежнего
API не получают другую форму ответа. RPC server получает queue snapshot
отдельно от interface counters; общая атомарность всех метрик не заявляется.

`/api/v1/status.inbound_queues` содержит capacities/heights/dropped и total.
Dashboard показывает четыре класса, ёмкость, заполнение в процентах и drops.
При отсутствии поля, null или сбое загрузки состояние показывается как
Unavailable, без сохранения устаревших нулей/показателей. Некорректные значения
и числа за пределами точности JS не выдаются за точные счётчики.

Проверки:

- Actor regression проверяет все четыре заполненные очереди, drops, capacities,
  total до/после dispatch и None в legacy mode.
- RPC тесты проверяют точные Python-имена, pressure, насыщение суммы drops,
  чтение расширенного ответа старым decoder и реальные лимиты actor через
  обработчик `interface_stats`.
- JSON-тест фиксирует форму API и null; JS-тест — четыре строки и fallback.
- `cargo test -p rns-runtime --test inbound_queue_rpc_python -- --ignored`:
  1 passed. Переполненные Rust очереди → RPC MessagePack → эталонный
  `RNS/vendor/umsgpack.py` Python 3.11. Запуск с `-B`, без RNS runtime/сокетов,
  эталонный checkout не изменён. Это codec interop, не нагрузочный сетевой тест.
- `cargo test -p rns-runtime --features api --lib --quiet`: 234 passed,
  4 ignored; повтор с loopback-разрешением после 9 sandbox PermissionDenied.
- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  441 passed, 1 ignored.
- `node --test crates/rns-runtime/web/app.test.js`: passed.

Сборки `cargo check --workspace --all-targets`, runtime client-only и
runtime `api,serial,rnode-tcp,sqlite-bundled` прошли; форматирование и
`git diff --check` — без ошибок. Новых warnings нет.

Этап 4 остаётся частичным: ранние protocol/IFAC violation counters, MTU/filter
семантика, ротация PR tags и оставшаяся traffic-flow статистика. Вывод очередей
в `rnstatus-rs` и remote-management — ещё не выполненная часть этапа 7.

## Этап 4 — ранние receive violation counters, частичное покрытие

InterfaceEntry получил actor-owned InboundDiagnostics с тремя независимыми
насыщаемыми u64: protocol_violations, ifac_violations, packet_filter_hits.
Они передаются в transport query, shared-instance RPC, native JSON и Web API;
UI показывает их в деталях интерфейса. Старые wire/JSON ответы без полей
декодируются с нулями; у неактивных configured-only интерфейсов API выдаёт null.
Удаление и новая регистрация интерфейса создают новые счётчики.

Подключён учёт к существующим ранним отказам: IFAC, header unpack, wire hops,
announce payload/signature и packet-hash dedup. До IFAC добавлена проверка
длины <=2: это protocol violation, а не IFAC violation, как Python
Transport.py:1752–1809. PR без tag считается нарушением; превышение 16-byte tag
также считается, но tag по-прежнему усекается и обрабатывается. Payload <16
и повторный PR tag не увеличивают эти счётчики (Transport.py:1830–1845).
Blackhole, ingress hold, queue overflow и административная очистка не выдаются
за protocol/IFAC/packet-filter нарушения. Held-release не повторяет IFAC учёт.

Проверки:

- Actor tests: разные типы отказов, независимость от queue drops, blackhole
  без ложного protocol violation, сохранение counters при query, насыщение,
  reset при новой регистрации, bad IFAC и held-release, tagless/overlong PR.
- RPC round-trip сохраняет ненулевые counters; legacy wire/JSON без новых
  полей принимаются. JSON сериализуется с плоскими Python-именами.
- Async API regression: malformed frame → actor → query → interface JSON.
- `cargo test -p rns-runtime --features api --lib --quiet`: 236 passed,
  4 ignored; с разрешением на loopback после 9 sandbox PermissionDenied.
- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  443 passed, 1 ignored.
- `node --test crates/rns-runtime/web/app.test.js`: passed.
- Workspace all-targets, runtime client-only и runtime
  `api,serial,rnode-tcp,sqlite-bundled` успешно собираются. Существующие warnings
  не затронуты. Python checkout остаётся чистым.
- `cargo fmt --all -- --check` и `git diff --check`: успешно.

Это **не полная packet_filter parity**. Следующие пункты этапа 4: MTU boundary
checks, transport-address/PLAIN/GROUP/shared-client filter semantics,
дополнительные dispatch-time violation sites, ротация PR tags и traffic flow.
Диагностика CLI/remote-management остаётся этапом 7; версия не изменена.

## Этап 4 — MTU и PLAIN/GROUP early filtering

До разбора заголовка проверяется размер IFAC-stripped кадра относительно
`InterfaceEntry.mtu + ifac_size`; арифметика без переполнения. Ровно предел
допустим. Это намеренно сохраняет порядок Python Transport.py:1788–1804,
включая IFAC-добавку после снятия тега, а не вводит другой wire-MTU контракт.
Announce имеет дополнительный предел 500 байт до подписи/постановки в очередь,
независимый от большого MTU интерфейса. Превышение даёт protocol_violation,
не overflow drop. YAML ifac_size по-прежнему в байтах.

Ранний фильтр PLAIN/GROUP использует wire hops (до adjusted_inbound_hops):
не допускает hops >1 и announce type. Context exemptions сохранены; PLAIN/GROUP
data не отбрасывается как повтор по packet hash. Shared clients обходят эти
части packet_filter, включая early hash dedup, как Python, но не IFAC, MTU,
announce signature и отдельную Rust Requested-client policy. Проверки активных
Link и их hash exceptions не менялись.

Нюанс эталона: Packet.receiving_interface изначально None (Packet.py:166),
а preprocess присваивает его лишь после packet_filter. Поэтому PLAIN/GROUP
отказ на обычном входе увеличивает только packet_filter_hits, несмотря на
условный protocol_violation внутри самого фильтра. Сохранено фактическое
поведение 1.5.2, подтверждённое выполнением эталонного кода.

Проверки:

- `cargo test -p rns-transport --lib admission_boundaries_match_python_preprocess -- --ignored --nocapture`:
  passed, 120 комбинаций destination type/hops/context/client mode/MTU.
  Oracle извлекает AST оригинальных packet_filter и admission prefix,
  использует настоящий RNS.Packet, сравнивает admission и оба counters.
  Runtime Python не запускается, только импорт; `-B`, read-only checkout.
  Подпись, IFAC crypto и дальнейший dispatch этим oracle не проверяются.
- Rust boundary tests: DATA 499/500/501/503/504/505 с IFAC 0/4;
  подписанные IFAC announces 499/500/501 на интерфейсе MTU 65535;
  PLAIN/GROUP hops 0/1/2/127, context/client exemptions и повторы;
  PLAIN/GROUP announces; shared-client SINGLE duplicate bypass.
- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  448 passed, 2 ignored.
- `cargo test -p rns-runtime --features api --lib --quiet`: 236 passed,
  4 ignored. После sandbox PermissionDenied повтор с loopback-разрешением.
- Workspace all-targets, runtime client-only и runtime
  `api,serial,rnode-tcp,sqlite-bundled` собираются; новых warnings нет.

Этап 4 остаётся частичным: ранняя адресация transport, сверка hash exceptions
для активных Link, дополнительные violation sites, ротация PR tags и
traffic-flow статистика. Этапы 5–7 и финальная интеграция ещё не завершены.

## Этап 4 — transport ID и отложенная запись Link hashes

До context/hash exemptions отклоняются non-announce Header2 пакеты с чужим
transport ID, включая кадры для локальной destination. Отказ увеличивает
packet_filter_hits. Shared clients обходят проверку; announces сохраняют
transport ID источника и также освобождены от неё. Незаданный identity actor
не разрешает произвольный Header2: runtime инициализирует identity при старте,
а custom embeddings должны явно сделать это до приёма адресованного трафика.

Исправлено разделение проверки и записи packet hash по Transport.py:1624–1680,
1938–1961. Наличие LinkEntry или LRPROOF context больше не отключает ранний
поиск уже известного хеша. Откладывается именно запись: для пакета из актуальной
Link table либо PROOF с LRPROOF context она остаётся в routing/validation после
подтверждения направления. Решение принимается при dispatch, а не сохраняется
при admission: Link может появиться/исчезнуть за время ожидания. Остальные
context exemptions и Rust bounded hash cache сохранены.

Два прежних unit fixtures уточнены: shared-peer тест теперь включает
shared_instance_client_mode, как настоящий runtime; тест локального Header2
явно задаёт соответствующий transport identity. Ослабления фильтра для этих
fixtures нет. Неправильный ID для локальной destination проверяется отдельно.

Проверки:

- `admission_boundaries_match_python_preprocess -- --ignored`: 360 сценариев
  против исполняемого Python admission prefix. Добавлены Header1, свой/чужой
  Header2 transport ID, в том числе keepalive и shared-client exemptions.
- Новые unit tests проверяют local destination/LINKREQUEST rejection,
  announce exemption, ранний hash lookup для Link и LRPROOF, изменение Link
  table между admission и dispatch, различие DATA/LRPROOF и PROOF/LRPROOF.
- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  450 passed, 2 ignored.
- `cargo test -p rns-runtime --features api --lib --quiet`: 236 passed,
  4 ignored; повтор с разрешением на loopback после sandbox PermissionDenied.
- `cargo test -p rns-runtime --features api link_rebalance_and_active_route_binding --lib -- --ignored --nocapture`:
  2 Python packet interop passed (normal/legacy Link rebalance proofs).
- Workspace all-targets, runtime client-only и runtime
  `api,serial,rnode-tcp,sqlite-bundled` собираются. Форматирование и diff checks
  проходят; новых warnings нет; эталонный checkout не изменён.

Этап 4 ещё частичный: ротация PR tags, дополнительные dispatch-time violation
sites и оставшаяся traffic-flow статистика. Этапы 5–7 и итоговая интеграция
не завершены; версия не менялась.

## Этап 4 — два поколения discovery PR tags

Временная HashMap с 120-second expiry и сортировкой/обрезкой до 32000
заменена на два HashSet: discovery_pr_tags и discovery_pr_tags_prev.
Проверка повтора учитывает оба множества до ingress sample/inflight gate;
повтор не обновляет историю и не переносится в текущее поколение.

На maintenance при current.len() >16000 предыдущее поколение заменяется
текущим целиком, current становится пустым (`mem::take`). При ровно 16000
и при idle ticks ротации нет. Это Python Transport.py:193–195,850–853,
1849–1852, без TTL и без oldest-first eviction. Размер между обслуживающими
проходами может превышать порог; 16000 не выдаётся за жёсткий per-insertion
cap. Старое поколение освобождается при замене. Сортировка истории удалена.

Shared-state reset и DropPathTable сохраняют оба поколения, как прежняя Rust
история и фактический Python lifecycle. Inflight timeout 45 секунд, discovery
waiters и обработка tagless/overlong PR не менялись. Старый constant
DISCOVERY_PR_TAG_RETENTION удалён; MAX_DISCOVERY_PR_TAGS теперь означает
порог 16000, а не ошибочно описанный hard cap.

Проверки:

- Unit tests покрывают 16000/16001, сохранение поколения на idle tick,
  замену старого поколения при второй ротации, отсутствие promotion при
  повторе, lifecycle reset и повторный допуск забытого tag.
- `cargo test -p rns-transport --lib path_request_tag_generations_match_python -- --ignored --nocapture`:
  passed. 32012 операций admission/maintenance сверены с AST оригинальных
  Python tag admission и rotation, включая превышение порога до tick и
  повтор из previous. Inflight gate в тесте сбрасывается отдельно для
  изоляции tag semantics. RNS runtime не импортируется/не запускается;
  Python 3.11 с `-B`, эталонный checkout не изменён.
- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  451 passed, 3 ignored.
- `cargo test -p rns-runtime --features api --lib --quiet`: 236 passed,
  4 ignored; повтор с loopback-разрешением после sandbox PermissionDenied.
- `cargo fmt --all -- --check` и `git diff --check`: успешно.
- Workspace all-targets, runtime client-only и runtime
  `api,serial,rnode-tcp,sqlite-bundled` собираются; новых warnings нет.

Этап 4 остаётся частичным: дополнительные dispatch-time violation sites,
оставшаяся traffic-flow статистика и итоговая проверка смешанной нагрузки.
Этапы 5–7 и финальная интеграция ещё не завершены; версия не менялась.

## Этап 4 — dispatch violations и запрет transit traffic до валидации Link

В route_link_packet_via_link_table добавлен отсутствовавший validated gate:
DATA и обычные proofs через transit Link не пересылаются до его подтверждения,
включая keepalive/context-exempt traffic. Отказ увеличивает protocol_violations
на входящем интерфейсе, не записывает hash и не обновляет timestamp Link.
Проверяется актуальная запись при dispatch; LRPROOF context обрабатывается
своей веткой, а не этой маршрутизацией. Локальные Link endpoints по-прежнему
обрабатываются собственным runtime. Основание: Transport.py:2122–2128.

Full destination/key binding failure announce теперь увеличивает счётчик при
dispatch, после ранней signature validation (Transport.py:2173). Это важно
при изменении first-seen cache во время ожидания, включая SQLite preparation.

Результат validate_transit_lrproof разделён на Valid, InvalidSignature и
Unavailable. Bad signature считается protocol violation только при совпадении
hops и входящего интерфейса с маршрутом; отсутствие identity, неверная длина,
чужой интерфейс и неудачная rebalance-попытка не получают этот счётчик. Это
разные ветки Python Transport.py:2608–2673, а не один bool failure category.

Остальные просмотренные sites не перенесены механически:

- Tunnel handler считает исключения, но не обычный False от signature
  validation; Rust bad-signature rejection оставлен без counter и закреплён
  тестом (Python 2790–2810).
- Missing random blob не является отдельным состоянием Rust AnnounceData:
  поле фиксированного размера, укороченный payload отклоняется раньше.
- Generic processing exception и MTU-signalling exceptions требуют отдельной
  сверки с Result/Option и path-MTU реализацией. Отсутствие исключения в Rust
  не выдано за эквивалентный protocol violation; полный паритет не заявляется.

Проверки:

- Новый тест DATA/PROOF × normal/keepalive: изменение validated после admission,
  отсутствие пересылки/hash/timestamp до валидации, затем настоящий корректный
  LRPROOF и успешный повтор того же пакета.
- Новый тест signature admission → conflicting first-seen cache → binding
  rejection с ровно одним counter, без замены кеша/создания пути.
- Transit proof tests дополнены checks счётчиков для invalid signature,
  bad rebalance, wrong interface, missing identity и malformed length.
- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  453 passed, 3 ignored.
- `cargo test -p rns-runtime --features api --lib --quiet`: 236 passed,
  4 ignored; повтор с loopback-разрешением после sandbox PermissionDenied.
- `cargo fmt --all -- --check` и `git diff --check`: успешно.
- `cargo test -p rns-runtime --features api link_rebalance_and_active_route_binding --lib -- --ignored`:
  2 Python packet interop passed.
- Workspace all-targets, runtime client-only и runtime
  `api,serial,rnode-tcp,sqlite-bundled` собираются; новых warnings нет.

Этап 4 остаётся частичным: traffic-flow статистика и итоговая проверка
смешанной нагрузки; exception/MTU signalling coverage отмечено выше.
Этапы 5–7 и финальная интеграция не завершены, версия не менялась.

## Этап 4 — счётчики пакетов и байтов announce/PR

Добавлен фиксированный `ControlTraffic` на регистрацию интерфейса: `arxb`,
`atxb`, `arxc`, `atxc`, `prxb`, `ptxb`, `prxc`, `ptxc`. Все восемь счётчиков
насыщаются на `u64::MAX`; отдельной таблицы по destination/packet hash нет.
Удаление/замена регистрации не оставляет накопленные счётчики новому endpoint.

RX подключён к существующим точкам preprocess: announce после проверки
подписи/blackhole, PR после tag dedup до inflight gate. Размер — весь stripped
пакет, включая Header1/Header2, без IFAC и framing. Учёт предшествует
переполнению классовой очереди; queue drop не отменяет его. Dispatch уже
подготовленного пакета не повторяет учёт. Повторный preprocess released held
announce, как в Python, учитывается вновь; это не unique wire RX totals.

TX централизован в успешной постановке `send_to_interface` в канал драйвера:
включает local/forwarded/queued announces и PR, использует размер после
hop/header mangling, до IFAC. Full/closed channel и запрет outbound не считаются
передачей. Это сознательная архитектурная граница: Python вызывает счётчики
в отдельных outbound/announce-queue местах, Rust считает channel admission,
а не физическую доставку (драйвер может отбросить пакет позже). Existing
frequency samples/ingress/egress thresholds не изменены. Константный PR
destination hash кешируется через OnceLock, чтобы новая TX-классификация
не вычисляла хеш имени для каждого DATA-пакета.

Цепочка подключена: actor GetInterfaceStats → full/client runtime bridge →
shared-instance MessagePack RPC → Web API → четыре поля карточки интерфейса.
Wire/JSON ключи совпадают с Python Interface stats. Старые ответы без полей
читаются с нулями; configured-only API возвращает null; UI не выдаёт null или
небезопасные JS integers за точные значения. `u64::MAX` сохраняется в RPC/JSON.

Проверки:

- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  456 passed, 3 ignored. Новые tests: IFAC, Header1/Header2, уникальные теги
  при уже занятом inflight gate, invalid signature, отсутствие recount на
  dispatch, queue overflow всех классов, full TX/outbound disabled, query,
  сброс регистрации, saturation.
- `cargo test -p rns-runtime --features api --lib --quiet`:
  236 passed, 5 ignored; loopback-тесты повторены с разрешением после EPERM.
  Добавлены проверки API mapping, RPC round-trip и defaults старого JSON/RPC.
- `cargo test -p rns-runtime --features api --lib control_traffic_rpc_matches_python_interface_counters -- --ignored`:
  1 passed. Из AST эталонного Interface.py извлечены реальные четыре метода;
  результаты сравниваются с Rust RPC, декодированным vendored umsgpack.
  Проверяются суммы/число вызовов и wire keys; это не end-to-end нагрузочная
  проверка транспортов и не проверка parent-interface aggregation.
- `node --test crates/rns-runtime/web/app.test.js`: успешно, включая
  нулевые/отсутствующие/слишком большие counter values.
- Workspace all-targets, runtime client-only и runtime
  `api,serial,rnode-tcp,sqlite-bundled` собираются; новых warnings нет.
- `cargo fmt --all -- --check`, `git diff --check`: успешно.

Этап 4 остаётся частичным: скорости/композиция traffic flow и итоговая
смешанная нагрузка, ранее обозначенные exception/MTU signalling границы.
Parent aggregation, глобальные внешние totals, PPS, CLI/remote presentation
остаются для продолжения диагностики (этап 7); они не подменены простым
суммированием новых endpoint counters. Этапы 5–7 и финальная интеграция не
завершены. Версия и пользовательский план не изменены; push не выполнялся.

## Этап 4 — измерение скоростей announce/PR

Добавлены `arxs`, `atxs`, `prxs`, `ptxs` в endpoint ControlTraffic, RPC/API
и Web UI. Единицы — **бит/с**, согласно выражениям `count_traffic_loop` Python:
`delta_bytes * 8 / elapsed_seconds`. Это per-interface расширение диагностики
Rust с именами Python global speed fields, не готовые глобальные totals.

Каждая регистрация хранит один baseline (Instant + четыре byte counters).
Первый maintenance sample только устанавливает baseline; следующие используют
фактическую монотонную длительность, а не предполагаемую одну секунду. Idle
interval обнуляет скорости; query не изменяет окно. Нулевая/обратная отметка
не сдвигает baseline; защитный saturating delta не превращает сброс counters
в огромную скорость. Sampling вызывается в существующем примерно 1 Hz
maintenance gate; сам gate по-прежнему использует wall-clock actor scheduling.

Полная цепочка наследует прежние full/client bridges; MessagePack добавляет
float поля, старые ответы дают 0, malformed negative/non-finite RPC rates
также нормализуются в 0. API inactive interfaces возвращает null. UI явно
пишет bit/s и не выдаёт отсутствующую скорость за нулевую. Существующие общие
`rx_rate`/`tx_rate`, frequency estimates и общий legacy TrafficCounter не менялись.

Проверки:

- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  458 passed (четыре ignored после добавления нового Python oracle).
  Управляемые интервалы, delayed/idle sample, reset, invalid time;
  реальный maintenance → query, новая регистрация без старого baseline.
- `cargo test -p rns-runtime --features api --lib --quiet`:
  236 passed, 5 ignored; API float mapping, RPC round-trip, старый JSON/RPC.
- `cargo test -p rns-transport --features sqlite-bundled --lib control_rates_match_python_transport_expressions -- --ignored`:
  1 passed. AST oracle выполняет реальные четыре выражения скоростей Python
  на интервалах 2.5, 1.5, 0.125 и 10 секунд; runtime RNS не запускается.
  Это проверка формул, не Python scheduler/parent aggregation.
- `cargo test -p rns-runtime --features api --lib control_traffic_rpc_matches_python_interface_counters -- --ignored`:
  прежняя Python/RPC проверка counters сохранена.
- Node Web UI tests, fmt и diff check успешны; workspace all-targets,
  runtime client-only и `api,serial,rnode-tcp,sqlite-bundled` собираются.

Этап 4 ещё частичный: композиция traffic flow и смешанная нагрузка, прежние
exception/MTU signalling границы. Для композиции требуется согласованный
общий учёт: processed RX может включать повторный preprocess held announce,
TX admission не равен физической передаче. Parent aggregation, внешние totals,
PPS и CLI/remote presentation по-прежнему не объявлены готовыми. Этапы 5–7
и финальная интеграция не завершены. Версия не менялась, push не выполнялся.

## Этап 4 — смешанная нагрузка и завершение actor

Добавлены два общих сценария, каждый запускается на memory и SQLite backend
через настоящий async actor loop (четыре новых теста):

- Непрерывная смесь DATA, подписанных ANNOUNCE, PR и ingress-limited PR.
  До запуска детерминированно заполнены все четыре class queues (по 4 пакета)
  и проверено по одному overflow drop. Затем заполнен raw channel на 4096
  сообщений, а отдельная задача продолжает подавать смесь до shutdown.
  Проверяется не только ответ на предварительно поставленный запрос: RX
  counters всех control-классов должны вырасти после начала loop, DATA и
  announce должны дойти до зарегистрированных обработчиков. Восемь повторных
  queue queries проверяют согласованность total/heights, пределы ёмкости и
  сохранение drops. Через control channel удаляется третий, не участвующий
  в генерации интерфейс; последующий query подтверждает удаление. Shutdown
  выполняется при живом producer, actor завершается, заблокированный sender
  освобождается. SQLite должен закончить уже принятые storage jobs.
- Мягкое завершение после закрытия обоих входов: классы ставятся в обратном
  порядке приоритета, snapshot подтверждает `[1,1,1,1]`. После actor exit
  проверены одна доставка DATA, один callback announce и два локальных
  AnnounceRequested для обычного и ingress-limited PR, с правильными tags и
  attached interfaces. Это проверка обработки очередей, а не только их очистки.

Проверки:

- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  462 passed, 4 ignored.
- `cargo test -p rns-transport --lib --quiet`: 443 passed, 3 ignored.
- Целевые `all_classes_drain` и `mixed_class_load`: по 2 passed; смешанный
  сценарий дополнительно повторён три раза на обоих backend.
- `cargo check --workspace --all-targets`, `cargo fmt --all -- --check`,
  `git diff --check`: успешно; новых production изменений и warnings нет.

Границы проверки: это локальная actor/storage интеграция без socket drivers,
радио и Python-процессов. Проверены пределы очередей, не RSS всего процесса;
timeouts (query 10 s, shutdown 15 s, общий сценарий 30 s) обнаруживают
зависания, но не являются latency SLA/benchmark. Непрерывный DATA по правилам
strict priority может задерживать низкие классы: fairness не заявляется.
Lifecycle test удаляет отдельный idle endpoint, а не имитирует физический
обрыв активного peer. Существующие tests stale registration/очистки waiters
и таймаутов остаются отдельными проверками.

Actor-level смешанная нагрузка этапа 4 теперь проверена на обоих backend.
Композиция в Python rnstatus использует глобальные rxs/txs и относится к
диагностике этапа 7; глобальный учёт/parent aggregation/PPS ещё не перенесены.
Прежние exception/MTU signalling границы и межпроцессная интеграция остаются
открытыми, полный этап 4 и обновление до 1.5.2 не объявлены завершёнными.
Версия и пользовательский план не менялись, push не выполнялся.

## MTU signalling review — согласованность локального responder

При проверке оставшихся MTU violation sites этапа 4 найден базовый дефект
Link, который нужно устранить до расширенного MTU этапа 5. У responder
локальный `mtu = min(request_mtu, 500)`, но подписанный LRPROOF всегда
advertised 500. Нулевая offer сохранялась как 0 вместо Python fallback 500;
MDU до получения RTT оставался стандартным. На диапазоне MTU 69–83 прежний
`update_mdu` мог вычитать 1 из нуля с panic в debug/underflow в release.

Исправлено в `rns-link`:

- Один effective MTU для state и подписанного proof. Legacy/zero offer → 500,
  остальные → min(offer, 500); существующий локальный cap пока сохранён.
- MDU рассчитывается сразу при создании responder. При слишком маленьком
  MTU unsigned MDU равен 0; это явная безопасная граница вместо отрицательного
  значения, которое может получиться в Python, или stale/default MDU в Rust.
- Link ID по-прежнему не зависит от signalling. Подпись покрывает фактические
  signalling bytes; формат пакета и проверка допустимых mode не менялись.

Проверки:

- Новый regression test сначала воспроизвёл несовпадение zero-offer (0 вместо
  500), после исправления проходит. Матрица 0/1/67/68/69/83/84/128/300/499/500/
  1064/2097151 проверяет proof, state, MDU до и после RTT, полный Rust handshake
  и совпадение Link ID. Для положительного MDU передаётся payload ровно MDU в
  обоих направлениях с проверкой размера зашифрованного кадра.
- `cargo test -p rns-link --quiet`: 98 passed, 1 ignored.
- `cargo test -p rns-link --lib responder_mtu_proofs_match_python -- --ignored`:
  1 passed, 13 variants, включая legacy 64-byte request. Python RNS разбирает
  пакеты, вычисляет Link ID/signalling/MDU и проверяет Ed25519-подпись Rust
  LRPROOF. Runtime RNS/сокеты не запускаются. Cap 500 явно указан в oracle:
  это не проверка autoconfigure/fixed-MTU interfaces.
- `cargo test -p rns-protocol --quiet`: 173 passed.
- `cargo test -p rns-runtime --features api --lib --quiet`: 236 passed,
  5 ignored. Workspace all-targets, client-only и runtime
  `api,serial,rnode-tcp,sqlite-bundled` собираются; новых warnings нет.
- `cargo fmt --all -- --check`, `git diff --check`: успешно.

MTU counters на Transport.py:2086/2563 связаны с ветками clamp и возможностью
signalling_bytes отказать для недопустимого mode. В Rust нет полноценной
передачи AUTOCONFIGURE_MTU/FIXED_MTU capabilities в actor, а локальный Link
ограничен 500. Эти сайты не имитируются без соответствующей MTU-семантики.
Это локальная prerequisite-починка, не завершение этапов 4/5/6: transit clamp,
увеличенный local MTU, dataplane control и прежние ограничения остаются.
Обновлены границы в CONFIG.md; версия и пользовательский план не изменены.

## Этап 4 — точная длина tunnel synthesis и exception-counter границы

При сверке Python `tunnel_synthesize_handler` найдено реальное расхождение:
Python обрабатывает только ровно 176 байт, Rust `TunnelSynthesisData::unpack`
принимал любую длину >=176, игнорируя хвост. Подписанный prefix с лишним байтом
мог создать/обновить туннель. Regression test сначала воспроизвёл создание
туннеля при длине 177, затем проверка исправлена на строгое равенство.

Дополнены parser/actor tests: длины 0/1/64/175/176/177/200/300; отказ не
меняет binding и expires существующего туннеля. В пределах допустимого frame
MTU эти отказы не увеличивают protocol_violations: Python просто не входит
в handler body при неверной длине. Bad signature тоже остаётся обычным False.

Добавлен AST oracle для реального Python tunnel handler: 11 вариантов длины,
подписи и signing key, сравниваются installation count и protocol violations
с Rust actor. Используются реальные RNS.Identity/crypto, установка туннеля
подменена локальной записью вызова; RNS runtime/сокеты не запускаются.
Эталонный checkout остался неизменным на ea98db4f.

Уточнены границы оставшихся exception sites:

- Missing random blob: не отдельное состояние Rust AnnounceData, поле `[u8;10]`;
  короткий announce отвергается при parse с уже существующим counter.
- Tunnel exception branch: неверная длина/подпись/рассмотренные некорректные
  ключи не требуют нового exception counter; это подтверждено oracle.
- Generic Python `inbound_job` catch: не переносится как catch_unwind вокруг
  actor. Result/Option failures уже имеют явные rejection paths; нарушение
  внутренних инвариантов не объявляется ошибкой протокола peer. Это граница
  архитектурного соответствия, а не доказательство отсутствия любых panic.
- MTU signalling exceptions: по-прежнему зависят от реального transit/local
  clamp и передачи MTU capabilities (этап 5), фиктивный счётчик не добавлен.

Проверки:

- `cargo test -p rns-transport --features sqlite-bundled --lib --quiet`:
  464 passed, 5 ignored.
- `cargo test -p rns-transport --features sqlite-bundled --lib tunnel_rejection_and_counters_match_python -- --ignored`:
  1 passed (11 cases).
- `cargo test -p rns-runtime --features api --lib --quiet`: 236 passed,
  5 ignored; workspace all-targets собирается, новых warnings нет.
- `cargo fmt --all -- --check`, `git diff --check`: успешно.

Для этапа 4 остаются межпроцессная проверка смешанной нагрузки/активного
disconnect и итоговая фиксация границ. MTU-dependent diagnostics остаются
явной зависимостью от этапа 5; глобальные flow totals/composition/PPS — этап 7.
Полное обновление не завершено, версия и пользовательский план не менялись.

## Этап 4 — межпроцессный TCP-тест и фиксация итогов

Добавлен `rns-runtime/tests/mixed_tcp_python.rs` с Python peer
`mixed_tcp_peer.py`: отдельный процесс слушает свободный loopback TCP-порт,
два настоящих Rust TcpClient driver подключаются к нему. Пакеты формируются
Python RNS, announce подписан реальной Identity, framing использует Python
HDLC.escape. Полный Reticulum daemon Python не запускается, внешние адреса
и действующие порты не используются. Actor запускается с class limits `[4;4]`,
на memory и SQLite backend; интерфейсы регистрируются тестовым harness,
это не повторная проверка YAML/runtime factory.

Проверяется:

- DATA/announce/PR приходят по первому соединению, ingress-limited PR — по
  второму. RX bytes и counters растут, pr_burst_active подтверждён.
- DATA с байтами `~}` проходит реальный HDLC escaping/deframing и доставку
  destination; announce проходит проверку и создаёт маршрут через первый peer.
- Восемь запросов статистики под нагрузкой отвечают; class heights не
  превышают limits, total совпадает с суммой heights.
- Python закрывает оба активных TCP-соединения; реальные read tasks завершаются
  и отправляют deregistration. Actor удаляет интерфейсы, class backlog и
  маршруты через них. После этого shutdown успешно завершает actor/SQLite.

Генератор ограничен 256 batches на соединение с паузой до 1 ms между batches:
это воспроизводимый lifecycle/interop test, а не максимальный throughput или
гарантия drops. Первоначальный неограниченный вариант на SQLite не успевал
прочитать до EOF накопленные TCP-байты за 15 s; это зафиксированная граница
тестовой нагрузки, не доказательство зависания actor. Предельный backpressure
остаётся предметом этапа 5.
Неограниченная actor-level нагрузка и детерминированные overflows всех классов
проверяются отдельными тестами предыдущего коммита. Обратная передача PR
ответов и peer re-connect не заявляются покрытыми этим новым сценарием.

Проверки:

- `cargo test -p rns-runtime --features api,sqlite-bundled --test mixed_tcp_python -- --ignored`:
  2 passed, повторный запуск 2 passed. Требуется loopback-разрешение; после
  sandbox EPERM выполнен разрешённый повтор. Успешные SQLite runs удаляют
  собственные временные DB directories; Python завершает соединения и threads.
- `cargo test -p rns-transport --features sqlite-bundled --quiet -- --include-ignored --skip sqlite_crash_fixture`:
  468 unit tests и 1 integration oracle passed, включая Python filter/tags/
  traffic/tunnel/queue проверки. Внутренний sqlite_crash_fixture не запускается
  отдельно: он требует directory env и вызывается своим parent crash test.
  Первоначальный include-ignored без skip закономерно упал именно на fixture.
- `cargo test -p rns-runtime --features api --lib --quiet`: 236 passed,
  5 ignored; повтор с loopback-разрешением после EPERM.
- `cargo check --workspace --all-targets`: успешно.
- Расширенная `cargo check -p rns-runtime --no-default-features --features client --tests`
  обнаружила прежний gravity test, обращавшийся к full-only runtime полям.
  Его full-only assertions теперь gated; YAML/normalization проверяются и
  в client-only. Check и целевой gravity test в client-only теперь проходят.
- `cargo fmt --all -- --check`, `git diff --check`: успешно.

Итог этапа 4: его основная область реализована и проверена — четыре класса и
приоритеты, YAML limits, отдельное управление, ранняя фильтрация и явные
violation counters, inflight PR/объединение/очистка, queue и per-interface
traffic counters/rates, actor/storage и ограниченная межпроцессная нагрузка.
Фиксируем этап с двумя межэтапными оговорками, не выдавая их за готовность:

- MTU-dependent clamp/diagnostics требуют capabilities и MTU-семантики этапа 5.
- Глобальные totals/parent aggregation/composition/PPS и CLI/remote вывод —
  диагностика этапа 7.

Это позволяет продолжить этап 5 без дальнейшего расширения входящих очередей
задачами глобальной диагностики. Полный паритет 1.5.2 и финальная интеграция
не заявляются; версия и пользовательский план остаются неизменными.

### Этап 5 — ограниченное объединение TX-записей Backbone

Первый участок этапа 5: общий writer клиента и принятых соединений Backbone
объединяет уже готовые кадры в encoded buffer до 65536 bytes (Python
TransmitBuffer.COALESCE_TARGET), до 64 кадров за batch. Ожидания заполнения
нет; после отправки batch writer уступает выполнение другим задачам.
Крупные кадры кодируются частями с сохранением cursor и HDLC delimiters;
обычные последовательности байтов копируются целиком. Отдельная allocation
полного escaped frame больше не нужна. Существующие входные mpsc queues
ограничены числом пакетов, не суммарным объёмом: лимит 64 KiB относится
только к encoded batch и не означает общего ограничения памяти интерфейса.

TX counter обновляется по каждому успешному write, включая частичный.
Interrupted повторяется, zero write/error завершает writer; незаписанный
остаток больше не засчитывается. Это принятые socket bytes с framing, не
подтверждение доставки peer и не изменение actor control-traffic counters.

Проверки:

- `cargo test -p rns-interface --lib --quiet`: 184 passed, 1 ignored,
  с разрешением локальных сокетов. Целевые Backbone tests: 15 passed,
  1 ignored; sandbox EPERM в loopback-тесте устранён разрешённым повтором.
- Пять новых обычных тестов: bounded chunks и точное совпадение с прежним
  HDLC encoder до 1 MiB escaped input; batching готовых кадров; порядок
  больших/малых кадров при fragmented writes; Interrupted/error/zero write
  и точные counters; остановка/возобновление duplex reader и sparse traffic.
- `cargo test -p rns-interface --release compare_coalesced_and_legacy_writes -- --ignored --nocapture`:
  1 passed. На 8192 кадрах по 500 bytes число вызовов записи 8192 → 128,
  wire bytes совпадают. Один in-memory run: legacy 5.80 ms, новый 6.07 ms.
  Это не сетевой throughput benchmark и не доказательство ускорения CPU.
- `cargo fmt --all -- --check`, `git diff --check`: успешно.

Осталось по этапу 5: адаптивный backpressure/пороги и stalled-peer teardown,
ingress throttling, capabilities/MTU-семантика и связанные diagnostics,
сетевая многопользовательская нагрузка с медленными peers и измерением памяти.
Полный Python TransmitBuffer/controller parity этим изменением не заявляется.

### Этап 5 — прекращение зависшего TX Backbone

Общий writer Backbone теперь завершает соединение после 12 секунд без
успешной записи при наличии pending encoded output. Значение взято из
Python 1.5.2 `DP_EC_DEAD_TIME`. Пустое соединение, ожидающее `rx.recv()`,
таймером не ограничивается. Каждая частичная запись продлевает deadline;
Interrupted не продлевает его и уступает выполнение другим задачам.
Timeout сохраняет точный TX counter и выставляет offline; существующий
connection select отменяет reader и запускает штатный disconnect/reconnect
путь. Обратный RX-трафик не считается прогрессом TX.

Явное отличие от Python: здесь monotonic deadline от фактического write,
а не проверка счётчиков раз в секунду. Порядок Python evaluate (dead check
до обработки свежего drained sample) не воспроизводится. Это отдельная
защита pending writer, не завершённый адаптивный egress controller.
Queue-byte accounting, ETA gating/hysteresis и ingress control ещё впереди.

Добавлены три теста с виртуальным временем Tokio (test-util только для tests):
idle дольше 12 секунд и timeout ровно на границе с закрытием очереди;
медленный reader с прогрессом каждые 11 секунд успешно получает целый кадр;
непрерывный Interrupted не вызывает starvation и завершается по deadline.
Целевые TX tests: 8 passed, 1 ignored. Проверка сетевого teardown нескольких
конкурирующих peers под давлением остаётся задачей нагрузочных тестов этапа 5.

Итоговые проверки: `cargo test -p rns-interface --lib --quiet` — 187 passed,
1 ignored (с разрешением локальных сокетов); `cargo check --workspace --all-targets`,
`cargo fmt --all -- --check` и `git diff --check` — успешно. Check сохраняет
прежние предупреждения database_path и tracing_subscriber::prelude.

### Этап 5 — исправление lifecycle клиента при завершении TX

При проверке точки подключения учёта TX backlog обнаружен пробел предыдущего
изменения: connection select существовал у принятых сервером peers, но клиент
запускал writer отдельно и ожидал только reader. Поэтому завершение writer,
включая новый timeout, само по себе не переводило клиент в reconnect/deregister.
Утверждение предыдущего раздела о connection select для общего клиентского
пути было преждевременным; настоящим изменением этот путь исправлен.

Клиент теперь ожидает reader и writer через один select. Завершение любого
из них отменяет другой и освобождает обе socket halves; forwarding task
останавливается и ожидается, online сбрасывается, затем выполняется прежняя
политика reconnect/max_reconnect_tries. Отдельного detached writer нет.

Добавлены два loopback regression tests:

- Обычный: закрытие TX sender дописывает последний escaped frame и завершает
  соединение с молчащим peer, не закрывающим свою отправляющую половину;
  проверяются EOF, offline, TX counter и deregistration при max tries = 1.
- Ignored длительный: 32 ссылки Bytes на payload 1 MiB, peer не читает,
  receive buffer уменьшен до 4096. При живом sender TX no-drain timeout
  вызывает offline/deregistration и закрытие очереди. После возобновления
  чтения kernel-accepted prefix побайтово совпадает с исходным HDLC stream,
  а его длина — с TX counter. Один run занял 23.45 s вместе с draining.
  Это проверка lifecycle одного peer, не throughput/RSS benchmark.

Проверки: `cargo test -p rns-interface --lib --quiet` — 188 passed, 2 ignored;
`cargo test -p rns-interface client_stalled_socket -- --ignored --nocapture` —
1 passed. Сетевые тесты выполнены с loopback-разрешением.
`cargo check --workspace --all-targets` — успешно с прежними warnings;
`cargo fmt --all -- --check`, `git diff --check` — успешно.

Учёт байтов очередей и адаптивный egress controller в этом изменении
не добавлены: сначала устранён найденный lifecycle-пробел. Они остаются
следующим участком этапа 5.

### Этап 5 — проверенная политика адаптивного egress (ещё не подключена)

Добавлен `backbone_flow::EgressController`: независимый от socket/queue
владелец состояния Python 1.5.2 `_dp_ec_evaluate`. Он принимает полный
encoded backlog, sendable bytes и cumulative successful writes, возвращает
Open/Gated/Disconnect. Это подготовительный компонент, не включённый
адаптивный backpressure: существующий Sender<Bytes> ещё не даёт полного
учёта очереди. Работающий writer пока использует прежний no-progress timeout,
теперь с общей константой DEAD_TIME из нового модуля.

Семантика политики:

- Интервал 1 s, mid watermark 128 KiB, high watermark 4 MiB, 3 zero-drain
  ticks, max ETA 10 s, release ETA 5 s и dead time 12 s.
- При backlog >128 KiB ETA >10 включает gate, ETA <5 снимает его;
  равенство 5/10 сохраняет состояние. Backlog <=128 KiB снимает gate.
- Пустой buffered или sendable сбрасывает gate/zero ticks и last_drain
  до проверки dead time. Dead check выполняется до учёта свежего drained.
- Drain rate использует фиксированный интервал 1 s, как Python, а не
  реальное время между задержанными вызовами evaluate.
- Predicate допуска целого encoded frame принимает ровно high watermark,
  отклоняет превышение, stalled и u64 overflow. Он не резервирует bytes:
  проверку и enqueue будущий владелец очереди должен делать атомарно.
- Каждому connection нужен отдельный controller; Disconnect терминален.
  Требуются монотонные timestamps/counters; saturating arithmetic — защита
  Rust от underflow, не заявленный паритет неверных snapshots.

Проверки: пять обычных boundary/state tests. Отдельный ignored Python oracle
AST-извлекает исходный метод и DP_EC constants из локального reference без
запуска Reticulum daemon; заменены только логирование/socket side effects.
400 детерминированных последовательностей, 2609 samples: 1842 Open,
367 Gated, 400 Disconnect. Совпали решения и внутренние stalled/zero ticks/
last_drain/previous_sent. Oracle проверяет policy, не реальный socket teardown.

`cargo test -p rns-interface --lib --quiet`: 193 passed, 3 ignored;
`cargo test -p rns-interface sampled_policy_matches_python_152 -- --ignored --nocapture`:
1 passed. `cargo check --workspace --all-targets`, fmt check и diff check
успешны; прежние workspace warnings сохраняются.

Далее требуется byte-accounted admission на границе actor→driver, корректное
освобождение backlog при partial writes/drop/reconnect, подключение gate и
drop counters. Ограничивать только encoded batch writer было бы неполным
учётом: клиент сейчас имеет также внешнюю и forwarding mpsc queues.

### Этап 5 — byte-accounted TX и подключение адаптивного gate

`InterfaceHandle.tx` и `InterfaceEntry.tx` теперь используют `InterfaceTx`:
обычные драйверы оборачивают прежний Sender<Bytes> через `.into()` и сохраняют
его поведение; Backbone использует managed byte channel. Конструктор
InterfaceEntry::new принимает оба варианта. Это изменение Rust API для
потребителей, создающих struct literals: полю tx нужен `.into()`.
Проверка same_channel для prepared inbound сохраняет идентичность регистрации.

Для клиента и accepted Backbone peers включён общий лимит 4 MiB outstanding
encoded bytes. Полный HDLC-размер (включая IFAC, если actor его добавил,
escaping и delimiters) резервируется атомарно до enqueue. Gate, byte quota
или packet slots отклоняют кадр целиком. Managed send отклоняет давление
немедленно, как Python process_outgoing, а не ждёт; Plain send по-прежнему
ожидает свободный slot. Sender clones разделяют один budget.

RAII lease сопровождает кадр через внешнюю/forwarding очереди и writer.
Batch хранит leases своих сегментов: каждый успешный partial write уменьшает
reserved remainder и увеличивает sent. Drop/ошибка/отмена освобождает только
незаписанный остаток, без двойного вычитания. Предел ограничивает outstanding
wire bytes, не RSS: удерживаемые raw payloads с уже записанным префиксом,
batch до 64 KiB, metadata, внешние Bytes clones и kernel buffers учитываются
отдельно. Полный memory/throughput benchmark остаётся впереди.

Подключён проверенный EgressController с интервалом 1 s: он видит полный
backlog обоих каналов и batch; все принятые Rust-кадры считаются sendable,
отдельного скрытого Python coalescing tail нет. Gate запрещает новые кадры,
но не останавливает draining. Controller создаётся заново на connection,
беря baseline накопленного sent. Его sampled Disconnect работает вместе с
предыдущим actual-write deadline; это всё ещё оговоренное отличие от одного
Python sampled timer. При disconnect gate освобождается; остаток внешней
клиентской очереди сохраняется для reconnect, как раньше, вместе с резервами.

Forwarding клиента теперь future внутри connection task, а не отдельная
spawned task: abort не оставляет orphan receiver с удержанным budget. Конец
forwarding сам по себе не обрывает writer — принятые кадры дописываются.
Порядок read/write disconnect и reconnect остаётся прежним.

Actor учитывает отказы managed admission в существующем tx_drops и не
увеличивает control TX traffic. Managed accounting дополнительно хранит
число отклонённых кадров и их полный encoded size; эти snapshots доступны
через TX handle, но RPC/UI для byte-drop diagnostics ещё не добавлен.
Drops при teardown не выдаются за admission rejections.

Новые проверки: конкурирующие producers не превышают quota; packet-slot/gate
отказы не оставляют резерв; partial progress/drop освобождают правильный
остаток; duplex gate/recovery с escaped payload сохраняет wire bytes;
write error и abort освобождают queues/batch; loopback reconnect сохраняет
accounting и принимает новые кадры; actor rejection не считается control TX.
Длительный stopped-reader TCP test адаптирован к quota: избыток отклоняется,
принятый prefix сохраняет HDLC и точные TX bytes.

Проверки: interface lib — 197 passed, 3 ignored; целевые queue tests — 3 passed,
actor admission test — 1 passed. Финальный Transport SQLite/include-ignored
suite: 472 unit + 1 Python integration passed (sqlite_crash_fixture исключён
из прямого запуска и проверяется через parent test).
Финальный stopped-reader TCP run — 1 passed, 23.66 s с draining после timeout.
Python↔Rust mixed TCP tests с memory/SQLite — 2 passed. Workspace all-targets
и runtime client-only tests checks успешны, прежние warnings сохраняются;
fmt/diff check успешны. Сетевые проверки выполнены с loopback-разрешением.

Этап 5 не закрыт: ingress dataplane control, MTU/capabilities/служебные кадры,
расширенные drop diagnostics и multi-peer throughput/latency/RSS сравнение
на одинаковой нагрузке ещё требуют работы.

### Этап 5 — модель dataplane ingress и проверка арифметики Python

Добавлен самостоятельный `rns_transport::backbone_ingress::IngressPolicy`.
Он пока НЕ подключён к actor/Backbone reader: существующий ingress.rs для
announces/path requests не заменён, фактическая приостановка socket reads
этим коммитом не включается.

Модель принимает depth только DATA queue и упорядоченные snapshots применимых
Backbone peers. Frame counters учитывают все классы доставленных driver frames,
byte counters — прочитанные socket bytes с framing. Порядок входного списка
должен соответствовать регистрации, а не случайному HashMap iteration.

- Пороги из Reticulum.py: high=max(4,int(90%*capacity)), mid=max(2,int(68%)),
  low=int(10%). Immediate trigger InboundQueues имеет отдельный минимум 128,
  проверяется по depth ДО append, в том числе перед rejected append.
- Период 250 ms. При depth >mid выбирается первый peer с максимальным
  ненулевым packet count. Уже gated лидер не заменяется следующим producer.
  При depth <low освобождается только первый gated peer с истёкшим hold.
  Равенство mid/low ничего не делает. Periodic сбрасывает counters, immediate — нет.
- Hold использует headroom max(peer_count,32) и penalty 1.5. Periodic делит
  доступный byte rate на max(allocated_rate,1), immediate — без этого clamp.
  Поэтому при низком byte rate нельзя просто заменить обе формулы на span*48.
- Модель возвращает действие; caller должен менять gate/hold только после
  успешного применения к reader. Snapshot time — monotonic Duration.
  Zero span/bytes immediate безопасно откладывается вместо Python division
  error; непредставимый Duration также не вызывает panic.

Обнаруженная граница, которую необходимо решить до включения: при DATA
capacity <10 lower watermark равен 0, поэтому буквальное `depth <low` никогда
не снимет gate. В модели сохранено и протестировано поведение reference;
рабочие сокеты пока не подвергаются этому риску. Существующие малые YAML queue
limits не изменены и не запрещены.

Пять unit tests покрывают выбор/тай-брейк, strict boundaries, hold expiry,
одиночный release, sample reset, gated leader, immediate threshold и малые
очереди. Новый ignored oracle AST-извлекает именно исходные выражения порогов,
allocation и hold из локального Python 1.5.2: совпали 2048 наборов watermark
и 120 hold calculations (допуск 1e-7 s для Duration rounding).
Это проверка арифметики, НЕ запуск полного Python ingress job или interop
socket throttle. Eligible-peer filtering задаётся контрактом входного списка;
не заявляется буквальное воспроизведение Python comprehension, где periodic
job использует имя `interface` в LocalClient check вместо `iface`.

Следующее подключение должно добавить shared driver counters/gate, wakeup
reader, 250 ms evaluation для memory/SQLite actor, immediate DATA pressure
hook и deregistration cleanup, с отдельными тестами маленьких очередей и
конкурирующих peers. SO_RCVBUF=32768 зафиксирован константой, но socket tuning
в этом изменении не менялся.

Итоговые проверки: transport с SQLite и include-ignored (кроме прямого запуска
sqlite_crash_fixture) — 477 unit + 2 Python integration tests passed;
workspace all-targets check и fmt/diff checks успешны. Прежние warnings
database_path и tracing_subscriber::prelude сохраняются.

### Этап 5 — подключение dataplane ingress к actor и reader

Backbone client/accepted peers получили shared IngressControl с packet/byte
snapshots, gate/hold и Notify для единственного reader. Listener fast-flap
diagnostics не заменены; optional control доступен через InterfaceDiagnostics.
Reader считает socket bytes и передаваемые transport frames, проверяет gate
перед чтением и между кадрами. Frame counters включают все классы, pressure —
только DATA. Запрос SO_RCVBUF теперь 32768 bytes, итог определяется ОС.
TX loop независим от reader gate.

Actor memory и SQLite имеют отдельный 250 ms timer с MissedTickBehavior::Skip,
не зависящий от mobile maintenance interval. Periodic snapshots атомарно
снимают/сбрасывают driver counters; immediate evaluation вызывается перед
DATA append при достижении порога, не сбрасывает counters и не заменяет
drop-tail admission. Выбор упорядочен по созданию shared controls, не HashMap;
при reconnect клиент сохраняет control/order, что отличается от новой
позиции socket в Python registry. LocalClient и интерфейсы без control исключены.

Защитное отличие для малых queue limits: effective release watermark в actor
равен max(Python low,1). Для capacity <10 пустая DATA queue после hold может
освободить peer, вместо вечного gate при literal depth<0. Статическая модель
и Python oracle по-прежнему проверяют исходные пороги; адаптация явная и
локализована в рабочем actor. YAML limits остаются прежними.

Release/reset будит reader без потерянного уведомления; reconnect, окончание
reader, deregistration и shutdown очищают gate/counters. Уже запущенный read
или transport send может завершиться после включения gate; последующие reads
и deliveries ожидают release. Имеющийся deframer/frame buffer сохраняется.

Оставшаяся lifecycle-граница: paused reader не наблюдает EOF немедленно, пока
не получит release (в отличие от отдельного Python EPOLLHUP handling).
При длительном давлении от других producers это может задерживать cleanup;
нужны дополнительные проверки/обработка HUP, прежде чем заявлять полный
multi-peer teardown parity. RX metrics/RPC/UI и нагрузочное сравнение также
ещё не завершены. Полный этап 5 не закрывается.

Новые проверки: shared sample/reset и release wakeup; actor gate при tiny
DATA queue, удержание до deadline, recovery при пустой очереди; immediate
перед 922-м append при исходной глубине 921; deregistration/shutdown cleanup;
реальный 250 ms release в memory и SQLite actor. Loopback reader test проверяет
две остановки/возобновления с escaped frame, отсутствие delivery до release,
сохранение bytes и reset после EOF. Это не multi-peer stress benchmark.

Interface suite: 198 passed, 3 ignored; отдельный TCP ingress test — 1 passed.
Финальный transport SQLite/include-ignored run — 481 unit + 2 Python
integration passed; timer test прошёл в обоих backend. Отдельные межпроцессные
Python↔Rust mixed TCP memory/SQLite tests — 2 passed.
Workspace all-targets и runtime client-only tests checks успешны, fmt/diff
checks успешны, прежние warnings сохраняются. Сетевые проверки выполняются
с loopback-разрешением.

### Этап 5 — закрытие соединения во время ingress gate

Backbone reader теперь ожидает либо release, либо close/error readiness
сокета. При READ_CLOSED/error общий read loop завершается, очищает ingress
state и online; connection select запускает штатный disconnect/reconnect
и deregistration. Непрочитанные bytes не извлекаются ради обнаружения FIN/RST.

Готовность к чтению может оставаться активной из-за unread data. Поэтому
после такого события повторная проверка ограничена интервалом 50 ms, с
немедленным пробуждением по release. Это не busy loop и не гарантированный
wall-clock deadline teardown: задержка зависит также от runtime/OS readiness.
Если gate открыт, приоритет имеет обычное чтение — buffered complete frames
перед FIN доставляются как прежде. Если peer закрывается при gate, pending
inbound frames отбрасываются без доставки в перегруженный transport.

Проверки Linux loopback:

- FIN и RST клиента при gate на час, с pending frame и без него: peer
  закрывается/deregisters без actor release, gate очищается, delivery нет.
- Gate до запуска reader + kernel-buffered payload + FIN: завершение без
  чтения payload, rxb=0, transport channel пуст.
- Два peers: gate первого не мешает его TX и RX второго; после FIN первого
  второй продолжает доставлять данные. Это небольшой isolation test,
  не throughput/latency/RSS benchmark с автоматическим actor pressure.
- Обычный FIN без gate сохраняет доставку полного buffered escaped frame.

В первом новом тесте найдена и исправлена гонка тестовой установки gate:
accept ещё не означает, что клиент завершил свой startup ingress reset.
Теперь тест явно ждёт online перед gate. Production startup не менялся.

Снята ранее отмеченная задержка EOF именно в wait-on-gate. Уже ожидающий
transport_tx.send при заполненном raw channel не заменён этим механизмом;
его cancellation/backpressure остаётся отдельным сценарием перегрузки.
Проверены Tokio close/error flags на Linux, не все поддерживаемые ОС и не
буквальная эквивалентность Python EPOLLHUP при half-close.

Итог: interface lib — 202 passed, 3 ignored; workspace all-targets и runtime
client-only tests checks успешны; fmt/diff checks успешны. Прежние workspace
warnings сохраняются. Сетевые тесты выполнены с loopback-разрешением.

### Этап 5 — разрыв при заполненном transport-канале

Ожидание `transport_tx.send` в Backbone reader теперь конкурирует с
наблюдением close/error readiness. Готовая отправка имеет приоритет: если
канал принимает кадр, buffered complete frames перед обычным FIN сохраняют
доставку. Если канал полон и peer закрывается, незавершённая отправка
отменяется, unsent frame/оставшийся read batch освобождаются, reader выходит
через общий offline/reset/disconnect путь. При живом peer кадр продолжает
ждать capacity; освобождение канала не приводит к потере или перестановке.

Gate wait и channel wait используют общий wait_socket_closed. Unread bytes
не извлекаются ради диагностики, повторная проверка постоянно readable socket
ограничена 50 ms. Это по-прежнему readiness-based Linux-проверенная семантика,
не универсальная гарантия teardown за 50 ms на любой платформе.

Отдельная граница подтверждена тестом: DeregisterInterface отправляется через
тот же transport channel. Пока он полон, уведомление и завершение connection
task могут ждать, но сокет уже закрыт и online=false. После освобождения
slot доставляется deregistration, task завершается. Не заявляется мгновенное
завершение всей задачи при навсегда остановленном actor.

Три новых loopback tests (четыре сценария): full channel + FIN/RST отменяют
unsent frame; живой peer после освобождения capacity доставляет два кадра
в исходном порядке; реальный Backbone client закрывает сокет до возможности
enqueue deregistration. Channel filler в тестах не исполняется actor.

Проверки: целевые тесты — 3 passed; interface lib — 205 passed, 3 ignored;
workspace all-targets, runtime client-only tests check, fmt/diff checks —
успешны. Прежние warnings сохраняются, сетевые тесты выполнены с разрешением
loopback. Этап 5 остаётся открытым: multi-peer нагрузочные сравнения,
MTU/capabilities, служебные кадры и оставшаяся диагностика не завершены.

### Этап 5 — включительные пороги автоматического MTU

Исправлен общий `traits::optimise_mtu`: девять сравнений `>` заменены на
`>=`, как в Python 1.5.2 Interface.optimise_mtu. Верхняя ступень 1 Gbit/s уже
была включительной. Изменение влияет только на значения ровно на порогах:
62500 bit/s теперь даёт 1024, 1/2/5/10/100/200/400/750 Mbit/s — соответствующую
старшую ступень. Между порогами и при >=1 Gbit/s результаты не изменяются.

Backbone peer estimate 100 Mbit/s теперь выбирает 32768 bytes вместо 16384.
Тест child_mtu усилен до точного равенства; прежний неверный комментарий
про 64 KiB исправлен. Все общие TCP/Backbone callers используют эту же
таблицу. Fallback ниже минимального bitrate и fixed_mtu callers не изменены.

Boundary test проверяет каждую из десяти ступеней на -1/0/+1 bit/s, а также
0 и u64::MAX. Ignored oracle AST-извлекает настоящий optimise_mtu из локального
Python Interface.py с AUTOCONFIGURE_MTU=True; 32 граничных/крайних и 1024
детерминированных дополнительных значения (всего 1056) совпали.

Это не полная MTU-миграция. Python Backbone listener имеет другую исходную
bitrate guess и в конце spawn копирует HW_MTU родителя поверх вычисленного
child MTU. Наследование/overrides, shared_medium hints, driver receive limits,
IFAC allowance и transport capability/Link clamp ещё требуют отдельных
изменений. Local Link cap 500 и версия проекта не изменены.

Проверки: `cargo test -p rns-interface mtu_ -- --include-ignored --nocapture` —
3 passed, включая Python oracle; полный interface lib — 206 passed, 4 ignored.
Workspace all-targets, runtime client-only tests check и fmt/diff checks
успешны; прежние warnings сохраняются. Полный interface suite выполнен с
loopback-разрешением.

### Этап 5 — bitrate/MTU listener и дочерних Backbone peers

Listener default bitrate приведён к Python 1.5.2: 100 Mbit/s вместо 1 Gbit/s.
В driver configs listener/client добавлен bitrate; MTU вычисляется перед
spawn из исправленной общей таблицы. Parent handle теперь сообщает этот MTU
(по умолчанию 32768 вместо прежних 500). Accepted child копирует bitrate и
вычисленный MTU listener, вместо независимой постоянной оценки. Клиент тоже
вычисляет handle MTU по настроенному bitrate до подключения.

Общий YAML bitrate сохраняется в BackboneInterfaceConfig фабрики и передаётся
driver config до spawn. Прежде post_init менял только bitrate actor entry,
не влияя на driver MTU и детей listener. Обратное typed YAML-преобразование
сохраняет bitrate; новых YAML keys не добавлено. Rust struct literals новых
driver configs должны задавать bitrate; constructors сохраняют defaults.

Loopback test проверяет parent/child пары на 62500, 100M, 200M и 1G bit/s,
включая точные равенства MTU. Отдельный client test проверяет 200 Mbit/s →
65536 bytes в handle до завершения connect. Factory test сверяет driver
bitrate с post_init в client/listener режимах и typed YAML преобразование.

Границы изменения: ниже 62500 bit/s сохранён Rust fallback 500, поскольку
handle.mtu не nullable; явный bitrate=0 также остаётся Rust explicit value,
а не Python truthy-config fallback. Административные runtime helpers без
bitrate-параметра используют default. HW_MTU constant — ceiling, не фактический
MTU каждого handle. Deframer receive limits/IFAC allowance не менялись,
shared_medium/capabilities и Link clamp ещё не завершены. Версия прежняя.

Проверки: interface lib — 208 passed, 4 ignored; runtime API lib — 237 passed,
5 ignored. Workspace all-targets и runtime client-only tests checks успешны;
fmt/diff checks успешны после форматирования. В новом YAML assertion исправлен
путь модуля: файл yaml_config.rs экспортируется как crate::config.
Прежние warnings сохраняются; сетевые tests выполнены с loopback-разрешением.

### Этап 5 — границы Backbone RX с учётом HDLC-экранирования

Backbone reader теперь получает вычисленный MTU и ограничивает decoded frame
размером MTU + 64 bytes. Драйвер пока не знает фактический IFAC size, поэтому
64 — консервативный максимальный допуск; точный размер проверяется actor.
Encoded accumulator ограничен удвоенным decoded limit без FLAG delimiters.
Прежний общий encoded cap 524288 мог отбрасывать допустимые полностью
экранированные крупные кадры. Другие драйверы сохраняют legacy limit через
HdlcDeframer::new(); новая граница включена только для Backbone.

Decoded overflow проверяется на завершённом кадре, encoded overflow — при
накоплении. Переполнение отбрасывает кадр один раз, чтение восстанавливается
на следующем FLAG. Это ограничение накопителя, не точный предел RSS: scratch,
ёмкость Vec, очередь готовых кадров и kernel buffers учитываются отдельно.
Decoder считает oversized frames; Backbone пишет debug log. Отброшенные здесь
кадры не доходят до actor protocol-violation counters; RPC/UI ещё не дополнены.

Тесты проверяют полностью экранированный кадр 512 KiB + 64 bytes при трёх
размерах chunks, decoded/encoded overflow с восстановлением и reset, а также
реальный loopback Backbone client на границе MTU + 64 и после превышения.
Payload fixtures проверяют framing, а не криптографическую корректность IFAC.
Отдельный ignored oracle использует локальный Python 1.5.2 HDLC.ReceiveBuffer:
36 готовых кадров (четыре лимита, -1/0/+1 byte, plain/FLAG/ESC) совпали.

Это не буквальная parity неполных Python frames: Python ограничивает незакрытый
буфер через mtu * 2 без IFAC allowance. Rust намеренно допускает 2*(MTU+64)
encoded bytes без delimiters, чтобы результат не зависел от дробления чтения.
Minimum/service frames, точный IFAC в драйвере, capabilities/Link clamp,
shared_medium, диагностика и multi-peer нагрузочные сравнения остаются открыты.
Версия проекта и пользовательский план не изменены.

Проверки: interface lib — 211 passed, 5 ignored; отдельный Python oracle —
1 passed. Workspace all-targets и runtime client-only tests checks успешны;
прежние warnings сохраняются. Сетевые тесты выполнены с loopback-разрешением.

### Этап 5 — точный IFAC allowance для Backbone RX

Backbone driver configs получили receive_ifac_size: Some(0..=64) задаёт
точный допуск, None сохраняет прежний консервативный допуск 64 bytes для
прямых callers, ещё не передающих настройку. Некорректные значения отвергаются
до bind/connect и создания задач. Это параметр границы RX, не включение IFAC.

Runtime передаёт размер до spawn во всех Backbone путях: старт из YAML,
добавление из конфигурации, административные client/server helpers и discovery
autoconnect. Размер определяется тем же выводом IFAC key, что при регистрации:
нет ключа — нулевой допуск; есть ключ — explicit size или class default 16.
Один только ifac_size без credentials не расширяет лимит. Accepted children
наследуют receive limit listener, reconnect использует исходную настройку.
Actor сохраняет независимую проверку и аутентификацию IFAC.

Loopback client test расширен до None/0/1/16/64; проверяет точную верхнюю
границу, overflow на один байт и восстановление. Listener/child test теперь
проверяет наследование допуска 16 bytes на четырёх bitrate/MTU ступенях.
Добавлены rejection test для invalid driver sizes, runtime test соответствия
allowance регистрации и YAML matrix для listener/client с отсутствующими,
default и explicit IFAC settings. Как и раньше, driver fixtures проверяют
длину/framing, а не криптографию. Python source HEAD ea98db4f не изменён;
max_frame_len и наследование ifac_size сверены с BackboneInterface.py.

Проверки: interface lib — 212 passed, 5 ignored; runtime API lib — 239 passed,
5 ignored, включая YAML matrix (она также отдельно прошла целевой запуск).
Workspace all-targets, client-only tests и fmt/diff checks успешны;
прежние warnings сохраняются.
Этап 5 не завершён: minimum/service frames, capabilities/Link clamp,
shared_medium, диагностика и multi-peer нагрузочные сравнения ещё открыты.
Пользовательский план и версия проекта не изменены.

### Этап 5 — минимальная длина Backbone frames

Backbone reader теперь отбрасывает decoded frames длиной 1..=HEADER_MINSIZE
(19 bytes) до dataplane packet counter и transport channel. Это строгое
сравнение Python 1.5.2 ReceiveBuffer: допустима длина >19, не >=19. Проверка
идёт до снятия IFAC; допуск IFAC к минимуму не прибавляется. Пустые frames,
включая последовательности FLAG, по-прежнему игнорирует общий deframer.
Проверки header и IFAC в actor не заменены; другие драйверы не изменены.

Physical rxb и ingress byte samples продолжают учитывать принятые socket
bytes, включая rejected/empty frames; packet samples учитывают только кадры,
прошедшие driver limits. Short drops debug-логируются и не доходят до actor
protocol-violation counters. Полная унификация диагностик остаётся этапом 7.

Новый loopback test подаёт escaped frames всех размеров 0..=19, затем 20 bytes
частями по 7 encoded bytes в reader с transport capacity=1. Проверены единственная
доставка (20 bytes), ingress packets=1, physical byte total и нормальное EOF.
Старые reader fixtures удлинены выше минимума, сохраняя проверки backpressure,
FIN/RST, gating, порядка и resynchronisation. TX-only framing не ограничен
новой минимальной длиной.

Проверки: interface lib — 213 passed, 5 ignored; workspace all-targets и
runtime client-only tests checks успешны. Прежние warnings сохраняются.
Python Backbone/HDLC source сверены статически. I2P использует отдельный
read_watchdog с FLAG FLAG probes и process_incoming guard для empty payload;
его watchdog/liveness parity этим изменением не заявляется. Этап 5 открыт.

### Этап 5 — I2P empty probes и read watchdog

Подключены ранее неиспользуемые I2P liveness timings: tick 1 s, probe после
строго >10 s без завершённой data-записи, stale после >20 s без чтения,
закрытие после >110 s. Любые прочитанные байты, включая пустые HDLC probes,
обновляют receive timestamp. Как Python, probes FLAG FLAG не обновляют
last_write и data TX totals: после idle threshold повторяются каждый tick
до следующей data-записи. Пустые frames по-прежнему не доходят до transport.
Stale/active пока только debug log, не поле diagnostics/RPC/UI.

Общий i2p_connection используется инициатором и accepted peers. Reader,
serialized writer и watchdog — scoped futures: EOF, writer error, timeout
или abort закрывают остальные операции и сбрасывают online. Это исправляет
прежние независимые writer tasks: ошибка записи могла оставить reader ждать,
а server writer мог пережить reader. Client forwarding тоже scoped и отменяется
вместе с connection; после завершения действует прежний reconnect pacer.
Server accepted peer завершает task; существующая схема регистрации не менялась.

Rust adaptations: monotonic Tokio time и skip missed ticks; probe ждёт
завершения текущего кадра вместо конкурентной записи в сокет. Watchdog работает
независимо от блокировки transport admission/socket write. При длительно полной
очереди чтение не продвигается и timeout может закрыть соединение с unread
kernel bytes — это не признак подтверждённой смерти удалённого узла.
Нет отдельного write deadline при продолжающемся RX. RX totals сохраняют
physical bytes; TX теперь учитывает завершённые framed data writes, не попытки,
но partial bytes до ошибки не учитываются. Полная унификация counters впереди.

Шесть новых tests на in-memory duplex и виртуальном времени: строгие 110/111 s
и renewal, период probes и reset после data TX, empty prefetched/fragmented
probes без transport delivery, timeout при одновременно blocked admission
и writer, abort teardown, writer failure при idle reader. Это не проверка
реальной I2P сети, SAM handshake/reconnect integration или полного Python oracle;
Python read_watchdog/process_incoming сверены статически с локальным эталоном.

Проверки: interface lib — 219 passed, 5 ignored. Workspace all-targets и
runtime client-only tests checks успешны; прежние warnings сохраняются.
Этап 5 остаётся открытым: capabilities/Link clamp, shared_medium, полная
диагностика и multi-peer нагрузочные сравнения не завершены. Версия прежняя.

### Этап 5 — проверка границ TCP fixed_mtu и shared-medium hints

В фабрике normalized config устранено unchecked u64→u32 преобразование
fixed_mtu: значения вне 500..=u32::MAX теперь отвергаются до cast. Ранее
u32::MAX+1 превращался в нулевой MTU, несмотря на проверку нижней границы.
Строгая YAML-схема уже использует Option<u32> и отвергает переполнение;
исправлен именно normalized factory path, а не обход строгого YAML parser.
Прямой spawn_tcp_client теперь также отвергает fixed_mtu <500 до создания
задач/соединения; раньше проверка существовала только в конфигурации.

Границы покрыты тестами: factory rejects 0/400/499/u32::MAX+1/u64::MAX,
driver rejects 0/1/499, оба сохраняют exact 500/1064/u32::MAX. Driver metadata
проверяется без открытия сокета. Верхняя граница — представимость поля,
не обещание приёма 4 GiB кадра: TCP deframer limits и Link capability/clamp
требуют отдельной работы. Явный zero остаётся ошибкой Rust, а не Python
truthy-config fallback; null/отсутствие настройки сохраняют автоматический MTU.

Сверка shared_medium по всему локальному Python RNS (HEAD ea98db4f, clean)
обнаружила только объявления: default false; true у UDP, Serial, Pipe,
KISS/AX25KISS, RNode и соответствующих вариантов. Чтений этого поля в Python
ядре нет, поэтому не добавлено неиспользуемое Rust поле и не заявлена новая
transport семантика. Отдельные AUTOCONFIGURE_MTU/FIXED_MTU реально участвуют
в Link request clamping и next-hop MTU — этот перенос остаётся открытым.

Проверки: целевой interface test — 1 passed; целевой runtime API factory test —
1 passed. Workspace all-targets, runtime client-only tests и fmt/diff checks
успешны; прежние warnings сохраняются. Полные suites в этом блоке повторно
не запускались. Этап 5, версия и пользовательский план остаются без изменений.

### Этап 5 — TCP/Backbone capabilities и transit Link MTU clamp

Добавлена driver capability link_mtu()->Option<u32> через существующий
InterfaceDiagnostics metadata channel. LinkMtuDiagnostics оборачивает прежний
объект и делегирует ingress-control/blocked-IP методы; runtime уже переносит
этот Arc в actor registration. Отдельное поле raw mtu не переосмыслено.
TCP client сообщает fixed_mtu либо automatic MTU; TCP server/children —
automatic. Backbone parent/children/client — automatic, None ниже 62500 bit/s.
Другие драйверы пока не сообщают capability и считаются без MTU upgrade.

При transit LinkRequest с exact payload 64+3 bytes и nonzero offer транспорт
ограничивает MTU минимумом offer, incoming raw MTU и outgoing negotiable MTU.
Без outgoing capability удаляются три signalling bytes. Ключи и Link ID
сохраняются; relay table использует прежнюю identity. Zero offer, обычные
64-byte requests и другие payload lengths не меняются. Local destination
ветка не изменена: существующий local Link cap 500 остаётся.

Это частичная capability migration: нет полного nullable HW_MTU, Local/Auto
и прочих driver capabilities, next-hop MTU RPC и новых mode-validation
protocol violations. Mode bits сохраняются без новой валидации. Rust явно
не увеличивает offer и при неизвестном incoming interface использует offer
как предел; Python min(nh_mtu, ph_mtu) в этом месте может встретить None.
RX limits драйверов этим изменением не расширены.

Новый actor matrix проверяет обе стороны clamp, отсутствие увеличения,
удаление unsupported signalling и zero offer, неизменность ключей/Link ID
и relay entry. Старый legacy forwarding test сохранён. Driver tests проверяют
fixed TCP capability и Backbone parent/child capability вместе с сохранением
ingress/fast-flap diagnostics. Shared runtime registration использует тот же
metadata Arc; отдельная live RPC проверка не выполнялась.

Проверки: целевые forwarding tests — 2 passed; interface lib — 220 passed,
5 ignored; transport lib (default features) — 459 passed, 4 ignored.
Workspace all-targets и runtime client-only tests checks успешны; прежние
warnings сохраняются. Этап 5 остаётся открытым; версия проекта не менялась.

### Этап 5 — Local и Auto Link MTU capabilities

Local server/client/accepted handles теперь сообщают capability 262144 bytes.
Исправлена прежняя автоматическая оценка 524288: Python Local имеет
AUTOCONFIGURE_MTU=True, но при обычном старте HW_MTU остаётся 262144;
__start_local_interface вызывает optimise_mtu только для специального
_force_shared_instance_bitrate. Rust этот override не реализует. Проверены
LocalInterface.py и точки инициализации в Reticulum.py, а не только флаг класса.

Local reader использует decoded limit 262144 без IFAC allowance и encoded
limit вдвое больше. Допустимый полностью escaped boundary frame принимается;
превышение plain/escaped размера отбрасывается с resynchronisation. Минимальная
длина кадров и Local diagnostics в этом блоке не менялись. Не смешивать
interface MTU с прежним local Link cap 500 — последний пока сохранён.

Auto handle сообщает fixed capability 1196 bytes независимо от bitrate.
Python Auto/AutoPeer имеют FIXED_MTU=True и наследуют этот HW_MTU; Rust пока
представляет Auto peers общим handle, его архитектура не менялась. Остальные
существующие Python драйверы не включают AUTOCONFIGURE_MTU/FIXED_MTU;
Weave — отдельный отсутствующий драйвер, его реализация не добавлялась.
Для остальных Rust drivers, включая plugins, upgrade остаётся отключённым,
пока capability не объявлена явно.

Проверки: Local IPC roundtrip сверяет server/client/accepted metadata; новый
duplex test — escaped boundary, plain/escaped overflow и следующий frame;
Auto spawn test — configured 1 Gbit/s при fixed 1196, ephemeral UDP ports и
пустой whitelist NIC (без реального multicast discovery). Transit actor
matrix дополнена Local 262144 и Auto 1196. Interface lib — 222 passed,
5 ignored; целевые forwarding tests — 2 passed. Workspace all-targets,
runtime client-only tests и fmt/diff checks успешны; прежние warnings остаются.
Этап 5 остаётся открытым; версия и пользовательский план не изменены.

### Этап 5 — TCP HDLC receive limit из driver MTU

Перед снятием local Link cap проверена receive цепочка. TCP HDLC всё ещё
использовал legacy encoded limit 524288 независимо от сообщаемого MTU.
Теперь клиент вычисляет MTU до spawn и передаёт его reader; accepted peer
использует ту же automatic оценку, что его handle. Decoded limit — MTU+64,
encoded accumulator — удвоенный decoded limit без delimiters. IFAC allowance
пока консервативный: точная настройка в TCP driver ещё не передаётся.
Сложение saturating для представимости usize на 32-bit targets.

Overflow debug-логируется, не доходит до actor и не увеличивает его violation
counters; чтение восстанавливается на следующем FLAG. Это framing/length
проверка, не IFAC authentication. KISS branch и минимальная длина TCP frames
не менялись; local Link cap 500 остаётся. Крупные fixed_mtu — административная
настройка границы, не обещание ограниченного RSS независимо от её значения.

Новый loopback test проверяет default automatic MTU, fixed 500 и fixed 524288:
полностью escaped boundary MTU+64, plain/escaped overflow на один байт,
resynchronisation и deregistration после EOF. Используются structural payloads,
не криптографические IFAC packets. Interface lib — 223 passed, 5 ignored;
workspace all-targets, runtime client-only tests и fmt/diff checks успешны.
Прежние warnings сохраняются; этап 5 открыт, версия не менялась.

### Этап 5 — TCP KISS control frames не являются transport data

TCP KISS reader раньше игнорировал возвращённую команду (_cmd) и пересылал
любое непустое содержимое actor. Теперь допускается только CMD_DATA после
удаления port nibble общим KissDeframer. Non-DATA и empty frames отбрасываются
до record_rx и channel send; значения RX packet/byte totals их не включают.
Это соответствует command gate в Python TCPInterface.read_loop. Raw/extended
KISS для RNode и обработка команд остальных драйверов не изменены.

Новый loopback test использует transport capacity=1 и feed chunks по 3 bytes:
15 non-DATA команд для каждого из портов 0/1/7 (45 control frames), пустые
delimiters/пустые DATA и три escaped DATA. Проверены только три доставки,
правильные payload/interface_id, rx_packets=3, rx_bytes=12 и offline после EOF.
Interface lib — 224 passed, 5 ignored; workspace all-targets и runtime
client-only tests checks успешны; fmt/diff checks успешны. Прежние warnings
сохраняются. Python command gate сверено статически, отдельный oracle не
запускался. KISS decoded-MTU limits и точный TCP IFAC allowance ещё открыты;
local Link cap 500 и версия проекта не менялись. Этап 5 не завершён.

### Этап 5 — TCP KISS decoded-MTU limits

KissDeframer получил opt-in with_max_decoded_size и oversized counter.
Encoded accumulator ограничен удвоенным decoded limit; command byte и FEND
не входят в payload. Decoded overflow проверяется на завершённом кадре,
encoded overflow — при накоплении. Кадр отбрасывается целиком, чтение
восстанавливается на FEND; reset очищает assembly, сохраняя counter.
Обычные new() и RawKissDeframer callers сохраняют legacy limit, поэтому
serial/RNode admin-command пути не переводились на новые ограничения.

TCP KISS использует MTU+64, как HDLC: пока это conservative IFAC allowance,
точная настройка ещё не передаётся драйверу. Oversized frames debug-логируются
до transport admission; существующая фильтрация CMD_DATA сохранена.
Это намеренная адаптация, не буквальное копирование Python TCP KISS parser:
Python прекращает накапливать payload при HW_MTU и может передать truncated
prefix, Rust отбрасывает overflow целиком. Допуск IFAC также отличается от
plain HW_MTU bound этого Python loop. Не заявлена полная parser parity.

Новый deframer test: limits 0/1/500/524352, plain/FEND/FESC payloads,
fragmented command/data, exact boundary, overflow, reset и recovery. TCP
loopback matrix расширена на HDLC/KISS × default/fixed500/fixed524288;
проверяет boundary+IFAC allowance, два overflow варианта, recovery и EOF.
Interface lib — 225 passed, 5 ignored; workspace all-targets, runtime
client-only tests и fmt/diff checks успешны. Прежние warnings сохраняются.

Этап 5 открыт; local Link cap 500, версия и пользовательский план не менялись.

### Этап 5 — точный TCP IFAC allowance

TcpClientConfig/TcpServerConfig получили receive_ifac_size: Some(0..=64)
задаёт wire allowance, None сохраняет 64 для прямых callers. Размер >64
отвергается до bind/connect. Это настройка bounds, а не включение IFAC;
существующие constructors совместимы, struct literals дополнены новым полем.

Runtime передаёт active IFAC size до spawn из YAML/normalized factory и
административных helpers, используя общий с Backbone wire_ifac_size. Нет
активного ключа — zero, иначе explicit size или class default 16. Shared TCP
fallback явно использует zero. Listener передаёт размер accepted children;
client сохраняет его при reconnect. Оба формата HDLC/KISS применяют одинаковый
decoded limit MTU+IFAC. Actor продолжает независимую аутентификацию.

TCP loopback matrix: None/0/1/16/64 × HDLC/KISS × default/fixed500/fixed524288,
boundary/overflow/recovery/EOF. Отдельно invalid sizes 65/usize::MAX отклонены
до spawn. Server roundtrip с allowance=0 проверяет, что child отбрасывает
MTU+1 (раньше он попадал в +64 допуск). YAML matrix расширена с Backbone на
TCP client/server: absent credentials, size-only, default и explicit size.
Это structural framing tests, не новые криптографические interop tests.

Interface lib — 226 passed, 5 ignored; runtime API lib — 239 passed, 5 ignored.
Workspace all-targets, runtime client-only tests и fmt/diff checks успешны;
прежние warnings сохраняются.
Этап 5 не завершён; local Link cap 500 и версия проекта сохранены.

### Этап 5 — ошибки режима при transit MTU clamping

При уменьшении nonzero MTU offer транспорт теперь проверяет signalling mode:
недопустимый режим отбрасывает request до отправки и создания relay entry,
увеличивая protocol_violations только incoming interface. Python вызывает
Link.signalling_bytes именно в ветке уменьшения; при unchanged offer,
zero offer или удалении signalling новая проверка не применяется.

Список разрешённых режимов вынесен в rns-wire::constants::LINK_ENABLED_MODES;
rns-link::constants::ENABLED_MODES сохраняет публичное имя и ссылается на него.
Transport и handshake generation теперь используют один список (AES-256-CBC).
Mode bits валидного кадра и Link ID не изменяются. Local responder validation
остаётся отдельной существующей проверкой; local Link cap по-прежнему 500.

Actor matrix расширена до 8 modes × 7 MTU cases, включая уменьшение по входу
и выходу, unchanged/zero и unsupported interface. Для отказа проверены пустой
TX, отсутствие relay entry и точный счётчик нарушения; для разрешённого
forwarding — прежние ключи/identity/signalling. Целевые forwarding tests —
2 passed; rns-link lib — 98 passed, 1 ignored. Workspace all-targets и
client-only tests checks успешны; прежние warnings сохраняются.

Дополнительно: полный transport lib — 459 passed, 4 ignored; rns-wire lib —
46 passed. Fmt/diff checks успешны. Этап 5 остаётся открытым; версия прежняя.

### Этап 5 — минимальная длина TCP HDLC

TCP HDLC reader теперь отбрасывает decoded frames 1..=19 bytes до transport
channel и record_rx, как Python TCPInterface.check_frame_len. Пустые frames
по-прежнему игнорирует deframer. Это strict >HEADER_MINSIZE до IFAC removal,
без добавления IFAC size к минимуму. KISS branch не изменена: Python применяет
этот check только в HDLC ветке. Short drops debug-логируются, но не доходят
до actor protocol-violation counters; authentication остаётся в actor.

Новый loopback test подаёт escaped frames размеров 0..=20 chunks по 3 bytes
в очередь capacity=1 при IFAC allowance=16. Только 20-byte frame доставлен;
rx_packets=1 и rx_bytes=20. Старые TCP roundtrip fixtures удлинены выше минимума,
общие HDLC codec tests коротких payloads оставлены без изменений.
Interface lib — 227 passed, 5 ignored. Этап 5 остаётся открытым; local Link
cap 500, версия проекта и пользовательский план не изменены.

Runtime API lib — 239 passed, 5 ignored; workspace all-targets, runtime
client-only tests и fmt/diff checks успешны. Прежние warnings сохраняются.

### Этап 5 — минимальная длина Local IPC

Local reader теперь пропускает только decoded frames >HEADER_MINSIZE (19),
как Python LocalInterface.ReceiveBuffer. Empty/short frames тихо отбрасываются
до transport admission; physical rxb продолжает учитывать весь прочитанный
поток. Actor violation counters не увеличиваются для этих driver drops.
Upper bound 262144, IFAC поведение и local Link cap 500 не менялись.

Новый duplex test подаёт escaped frames размеров 0..=20 chunks по 3 bytes
в очередь capacity=1. Проверены единственный 20-byte inbound, physical total
всего wire, EOF и offline. Local reconnect/roundtrip fixtures удлинены выше
минимума; TX-only short reply fixture оставлен, поскольку новый фильтр — RX.
Interface lib — 228 passed, 5 ignored. Этап 5 остаётся открытым; версия проекта
и пользовательский план сохранены.

Runtime API lib — 239 passed, 5 ignored; workspace all-targets, runtime
client-only tests и fmt/diff checks успешны; прежние warnings сохраняются.

### Этап 5 — in-process next-hop MTU query

Добавлен TransportQuery::GetNextHopMtu: actor выбирает live path interface,
а при отсутствии live path для local destination — SharedServer fallback,
как соседние next-hop queries. Возвращается именно link_mtu capability,
не raw receive MTU. Без интерфейса/capability ответ IntResult(-1).
ReticulumHandle::next_hop_mtu(destination) предоставляет Option<u32> API,
используя query_transport текущего процесса, включая shared-client режим.
Внешний control RPC/remote-daemon lookup этим изменением не добавлены.

Это подготовка к выбору предложения инициатором, не включение больших Links:
new_initiator всё ещё предлагает 500, responder сохраняет 500-byte cap.
Проверен Python Transport.next_hop_interface_hw_mtu, который также требует
AUTOCONFIGURE_MTU/FIXED_MTU capability, а не просто большого HW_MTU.

Actor test проверяет unknown path, большой raw MTU без capability, local
SharedServer fallback, приоритет live path, отсутствующий path interface,
expired path с fallback и без него. Путь не переходит на SharedServer только
потому, что его зарегистрированный интерфейс пропал: fallback применяется
при отсутствии live path. Набор transport tests и проверки сборки выполнены.

Проверки: целевой test — 1 passed; transport lib — 460 passed, 4 ignored.
Workspace all-targets, runtime client-only tests и fmt/diff checks успешны;
прежние warnings сохраняются. Этап 5 открыт; версия проекта не менялась.

### Этап 5 — explicit MTU handshake API библиотеки Link

Добавлены Link::new_initiator_with_mtu, new_responder_with_mtu и вариант
new_responder_with_signer_and_mtu для внешнего подписанта. Инициатор использует
явный MTU в signalling, pending state и MDU. Responder ограничивает nonzero
offer заданным максимумом, подписывает именно эффективный MTU и хранит тот же
MTU/MDU. Missing/zero offer сохраняет базовые 500. Явные параметры bounds
нормализуются в 500..=MTU_BYTEMASK (2097151), без wrap через битовую маску.
Малый nonzero wire offer по-прежнему может уменьшить effective MTU ниже 500.

Старые constructors делегируют новым с лимитом 500. Runtime пока использует
старые constructors: ни next-hop query в инициатор, ни incoming capability в
responder в этом блоке не подключены. Это рабочий opt-in library API, не
объявление полной поддержки больших Links через runtime/сеть.

Новый handshake test: шесть пар offer/cap × обычный/внешний signer. Проверены
wire offer, signed proof, Link ID, совпадение MTU/MDU после proof+RTT,
двустороннее encryption/decryption payload до 64 KiB. Включены bounds 0 и
u32::MAX, уменьшение по каждой стороне и fallback500. Верхний MTU проверяется
как signalling/state, не передачей 2 MiB по реальному интерфейсу.

Link lib — 99 passed, 1 ignored; workspace all-targets и runtime client-only
tests checks успешны. Прежние warnings сохраняются; этап 5 открыт, версия
и пользовательский план не менялись.

### Этап 5 — MTU discovery в runtime инициаторе

Async LinkSession::open/open_with_public_key и LinkClient::query запрашивают
GetNextHopMtu у локального actor до создания Link и используют explicit MTU
constructor. Unknown, malformed, unsupported и <500 ответы дают fallback500.
Query ограничен min(1s, remaining budget), включая send в заполненную очередь;
для open время запроса вычитается из handshake deadline, у LinkClient query
сохраняется общий absolute deadline. Synchronous prepare остаётся I/O-free
и предлагает 500. Внешний daemon RPC не используется.

Исправлена совместимость с legacy/zero-MTU proof: после большого offer
подтверждение без positive MTU уменьшает инициатор до min(offer,500), а не
оставляет неподтверждённый большой MTU. Подпись проверяется до изменения state.
Legacy proof test теперь начинает с offer32768 и проверяет downgrade500;
подрезанный без переподписания modern proof по-прежнему отвергается.

Новые runtime tests проверяют capability→prepared wire offer и fallback
1196/262144/-1/0/499, а также ограничение ожидания при полном transport channel.
Runtime API lib — 241 passed, 5 ignored; Link lib — 99 passed, 1 ignored;
workspace all-targets и runtime client-only tests checks успешны. Прежние
warnings сохраняются. Runtime responder ещё ограничен 500; полномасштабный
large-packet transport interop не заявляется. Этап 5 открыт, версия прежняя.

### Продолжение этапа 5: MTU входного интерфейса в runtime responder

Actor передаёт capability входного интерфейса через новое поле
`DestinationEvent::LinkRequest.max_mtu`. Отсутствующие/нулевые/меньшие 500
capabilities дают 500; большой raw receive MTU сам по себе не разрешает upgrade.
LinkManager использует explicit-MTU constructors для software key и external
signer: эффективный MTU ограничен offer и интерфейсом до подписи LRPROOF,
тот же MTU/MDU сохраняется в active Link. Raw request не переписывается.
Приложения, создающие LinkRequest event напрямую, должны добавить max_mtu;
значение 500 сохраняет прежний cap. Формат пакета и YAML не меняются.

Actor test проверяет fallback None/0/499 и capabilities1196/262144 при доставке
Header2 через shared instance. Runtime event-path matrix: два способа подписи
× пять пар offer/cap, проверка подписи, RTT, interface_id и одинаковых MTU/MDU;
шифрованный двусторонний обмен в памяти до полного MDU, включая MTU262144.
Это не сетевой large-packet interop или нагрузочный benchmark.

Python Transport.py local Link Request branch переписывает signalling до
destination.receive; Rust ограничивает его при создании responder. Полная
семантика nullable HW_MTU/удаления mode signalling и protocol_violation при
недопустимом mode в локальной ветке пока не заявляется.

Проверки: runtime API lib — 242 passed, 5 ignored; transport lib — 460 passed,
4 ignored; workspace all-targets и runtime client-only tests checks успешны;
fmt и diff check чистые. Прежние warnings без изменений. Далее — сетевые
large-MTU проверки и измерения этапа 5. Этап открыт, версия остаётся 1.0.1.

### Продолжение этапа 5: реальный TCP Link MTU round-trip

Добавлен `crates/rns-runtime/tests/link_mtu_tcp.rs`: локальный TCP listener-peer
с библиотечным Rust Link подключён к настоящему TcpClient driver, transport
actor и runtime LinkManager responder. Матрица offer/cap/expected:
32768/500/500, 32768/1196/1196, 524288/262144/262144,
1196/262144/1196. IFAC выключен явно, порты выбираются ОС, общий deadline30s;
задачи actor/driver отменяются guard-ом, включая panic/timeout.

Проверяется LRPROOF signature, RTT handshake и совпадение MTU/MDU, затем
payload1 и полный MDU в обе стороны через TCP с проверкой содержимого и
wire packet length <= negotiated MTU. HDLC записывается частями997 bytes.
Перед корректными данными отправляются полностью escaped frame cap+1 и
валидно зашифрованный Link DATA packet выше receive cap; следующий допустимый
пакет доставляется приложению, oversized payload не доставляется. При чтении
ответа peer отделяет Link proofs и посторонний служебный трафик actor.

Проверки: новый TCP test — 1 passed в default и 1 passed в client-only сборке
(каждый включает четыре случая); существующий Python mixed TCP load/disconnect
test — 1 passed с эталоном ea98db4f; workspace all-targets check успешен;
fmt/diff check чистые, прежние warnings без изменений.

Это responder-side сетевой regression, не полный runtime-to-runtime тест:
инициатор задаёт offer библиотечным API, не использует async MTU discovery.
Python large-MTU Link interop, transit/multi-peer сценарии и сопоставимые
throughput/latency/drop/RSS измерения остаются. Этап 5 открыт, версия 1.0.1.

### Продолжение этапа 5: Python initiator → Rust responder, большой MTU по TCP

В `link_mtu_tcp` добавлен ignored interop test с `link_mtu_peer.py`.
Python Link создаёт настоящий LINKREQUEST, проверяет подпись Rust LRPROOF,
выполняет ECDH и отправляет LRRTT; Python Packet шифрует/упаковывает данные.
Четыре пары offer/cap прежней сетевой матрицы проверяют MTU500/1196/262144,
равенство MDU у Python и Rust, payload1 и полный MDU в обе стороны.
Приём/отправка идут через loopback TCP, Rust driver, actor и LinkManager.

Python работает без daemon: routing/hops/first-hop timeout/MTU capability,
регистрация Link, outbound dispatch и watchdog scheduling подменены тестовыми
службами. Криптография, формат пакетов, согласование Link и расчёт MDU взяты
из неизменённого локального эталона ea98db4f. HDLC escape — эталонный helper,
unescape — те же две замены, что в TCPInterface.read_loop; тестовый socket
decoder не является полным Python TCP driver. Процесс kill_on_drop, socket
timeout10s, общий deadline45s; никаких внешних узлов/изменений Python repo.

Запуск: `cargo test -p rns-runtime --test link_mtu_tcp -- --include-ignored`.
Обычная и client-only сборки: по 2 passed (Rust peer + Python peer, каждый
с четырьмя MTU случаями). Workspace all-targets check успешен, fmt/diff check
чистые; прежние warnings сохраняются. CONFIG описывает окружение и границы.

Следующие проверки: Rust initiator → Python responder, async MTU discovery
по настоящему сетевому пути, transit/multi-peer и сопоставимые нагрузочные
измерения throughput/latency/drops/RSS. Этап 5 открыт, версия остаётся 1.0.1.

### Продолжение этапа 5: Rust initiator → Python responder по TCP

Python peer получил responder-режим: создаёт локальную identity/destination,
обрабатывает Rust LINKREQUEST через эталонный `Link.validate_request`, подписывает
LRPROOF и принимает LRRTT через `Link.rtt_packet`. Тестовый адаптер применяет
fixed-interface clamp из локальной ветки Transport.py до validate_request;
полный Python daemon/driver и его nullable HW_MTU policy не запускаются.

Новый ignored Rust test использует настоящий TcpClient driver и transport actor.
Прямой маршрут предварительно внесён в path table; offer берётся из ответа
GetNextHopMtu для этого маршрута. После проверки Python подписи Rust подтверждает
привязку proof к интерфейсу через штатный ConfirmLocalLinkProof. Шифрование и
handshake выполняет библиотечный Link, не LinkSession::open.

Матрица offer/cap/expected: 32768/500/500, 32768/1196/1196,
524288/262144/262144, 1196/262144/1196. Проверяются одинаковые link_id/MTU/MDU,
payload1 и полный MDU в обе стороны, размер wire packet <= negotiated MTU.
Общий deadline45s, socket timeout10s, ОС выбирает loopback ports; child
kill_on_drop и task guard ограничивают время жизни ресурсов.

`cargo test -p rns-runtime --test link_mtu_tcp -- --include-ignored`:
3 passed в default и 3 passed в client-only сборке (по четыре MTU случая на
каждый тест). Workspace all-targets check успешен; fmt/diff check чистые.
Прежние warnings без изменений. Эталон ea98db4f не изменён.

Далее: полный async LinkSession open с MTU discovery, transit/multi-peer и
сопоставимые throughput/latency/drop/RSS измерения. Этап 5 открыт, версия 1.0.1.

### Продолжение этапа 5: полный runtime LinkSession::open по Python announce

Добавлен full-only ignored integration test
`runtime_session_open_discovers_python_route_and_large_mtu`. Runtime запускается
штатным init из отдельного временного YAML: share_instance=false, transport=false,
один loopback TCPClient с fixed_mtu. Python responder отправляет настоящий
подписанный announce; ключ destination и прямой маршрут узнаются через драйвер
и actor, без ручной записи в cache/path table и без передачи public key в open.

Тест вызывает публичные LinkSession::open/send/recv. Проверяется capability
маршрута, wire offer на стороне Python, подписанный handshake и одинаковые
link_id/MDU. Матрица offer/cap/expected: 32768/500/500, 32768/1196/1196,
524288/262144/262144, 1196/262144/1196. Payload1 и полный MDU передаются в обе
стороны. Python Link/Packet остаются эталонными; daemon services и fixed-MTU
clamp — тестовый адаптер, как в предыдущем interop.

Каждый случай имеет deadline30s и собственный Tokio runtime. Shutdown guard
срабатывает при выходе/ошибке, child kill_on_drop; задачи останавливаются до
удаления только эксклюзивно созданного временного каталога конфигурации/storage.
Эталон Python ea98db4f не изменён.

Полный TCP target с include-ignored: default/full — 4 passed; client-only —
3 passed (runtime-init тест исключён cfg(full)). Workspace all-targets check
успешен, fmt/diff check чистые. Прежние warnings сохраняются.

Остаются shared-client session opening, active path-request discovery,
transit/multi-peer проверки и сопоставимые throughput/latency/drops/RSS
измерения этапа 5. Этап открыт, версия остаётся 1.0.1.

### Продолжение этапа 5: Link MTU через TCP transit relay

Добавлен неигнорируемый `transit_link_mtu_tcp_roundtrip`: два отдельных loopback
TCP-соединения через настоящие Rust TcpClient drivers и один transport actor.
На концах — библиотечные Rust Links; маршрут к responder и ключ проверки его
подписи заранее внесены в actor. Этот тест не запускает announce discovery.

Пять случаев offer/incoming/outgoing/expected:
32768/32768/500/500, 32768/32768/1196/1196,
524288/524288/262144/262144, 32768/1196/32768/1196,
1196/32768/32768/1196. Проверяется MTU прямо в ретранслированном LINKREQUEST
до создания responder, преобразование Header2→Header1, hops, неизменность
ключевых байтов и Link ID. LRPROOF проходит проверку подписи relay и возвращается
инициатору; LRRTT и payload1/полный MDU идут по Link relay в обе стороны.
Wire packets не превышают согласованный MTU. Общий deadline30s, порты ОС,
task guard отменяет actor/driver при выходе/ошибке.

Полный TCP target с include-ignored: default/full — 5 passed; client-only —
4 passed. Workspace all-targets check, fmt/diff check успешны; прежние warnings
сохраняются. Python-эталон ea98db4f не изменён.

Одиночный relay проверен, но concurrent multi-peer и multi-relay сценарии,
shared-client session opening, active path requests и сопоставимые измерения
throughput/latency/drops/RSS остаются. Следующий шаг — конкурирующие peers и
нагрузочные измерения. Этап 5 открыт, версия 1.0.1.

### Продолжение этапа 5: два Backbone peers, остановка чтения и замеры TX

Добавлен ignored `backbone::tx_tests::measure_two_peer_tcp_isolation_and_recovery`.
Два настоящих BackboneClient TCP-соединения в одном Tokio runtime, два worker
threads. Первый получатель не читает до срабатывания egress gate; второй затем
принимает 256×4096 байт, пока первый остаётся gated. Для первого — burst4096
попыток по16384 байт; отклонённые admission считаются drops, не повторяются.
На каждой попытке проверяется buffered<=4194304; все принятые кадры после
возобновления чтения должны прийти в исходном порядке без повреждений. После
drain gate освобождается, новый кадр успешно принимается и доставляется.

Запуск отдельно:
`cargo test -p rns-interface backbone::tx_tests::measure_two_peer_tcp_isolation_and_recovery -- --ignored --exact --nocapture`.
Общий deadline20s; ожидание gate6s, fast delivery3s, release3s. TCP receive
buffer медленного peer сначала4096, при восстановлении запрашивается4MiB.
SO_* значения ОС может корректировать. Transport actor/ingress под нагрузкой
в этот сценарий не входят: измеряется драйверный TX и независимость peers.

Два последовательных прогона окончательного теста (debug/test profile,
rustc1.97.1, Linux6.4.0-150600.23.84-default):
- fast payload throughput42.684/43.214 MiB/s, p50 enqueue-to-receive
  10.293/10.056ms, p99 14.029/13.780ms, fast drops0;
- slow accepted282 из4096, rejected3814; наблюдаемый max buffered
  4178844/4178845 байт при лимите4194304;
- slow drain после возобновления чтения2694.257/2786.340ms;
- process RSS start10632/10500 KiB, gated14856/14724, end15496/15364.
RSS — отдельные контрольные точки всего тестового процесса, не peak и не
доказательство ограничения памяти. Это текущая точка отсчёта, не before/after
ускорение и не оценка production capacity. Задержка включает enqueue и очередь.

Проверки: новый тест повторно успешен; interface lib — 228 passed, 6 ignored;
workspace all-targets, fmt/diff checks успешны. Прежние warnings сохраняются;
Python-эталон ea98db4f не изменён.

Остаются сопоставимый baseline старой реализации, расширенная конкуренция
peers через actor/ingress, peak/RSS измерения и прочие незавершённые сценарии
этапа 5. Этап открыт, версия остаётся 1.0.1.

### Продолжение этапа 5: два Backbone peers через входные очереди actor

Добавлен full-only integration test `backbone_ingress_load` с двумя настоящими
BackboneClient TCP drivers. Два peers отправляют по4096 уникальных DATA-пакетов
по256 bytes; actor запускается после заполнения raw ingress channel, то есть
начальное давление возникает от драйверов, не от синтетических actor messages.
Admitted queues имеют по4 слота, application delivery channel вмещает весь
ограниченный burst и не добавляет свои drops.

Тест сверяет доставку+DATA queue drops=8192, отсутствие drops других классов,
целостность содержимого и возрастающий sequence отдельно для каждого peer.
Оба peers должны продвинуться. Параллельные GetInboundQueueStats ограничены
deadline1s, включая admission в control channel; наблюдаемые heights<=capacities.
После разгрузки два последовательных marker-пакета проходят по прежним TCP
соединениям без новых drops. Shutdown actor завершает тест; общий deadline45s
оставляет время для policy hold/release, task guard отменяет фоновые задачи.
Конкретное срабатывание adaptive gate не является обязательным assertion.

Запуск: `cargo test -p rns-runtime --test backbone_ingress_load -- --nocapture`.
Повторный default/full прогон: delivered3811+3843, DATA drops538, сумма8192;
170 control queries, max1.560ms, тест12.30s. Первый прогон: delivered3797+3818,
drops577, max control3.038ms. Разброс drops зависит от Tokio scheduling;
это проверка учёта/восстановления, не throughput или fairness benchmark.

Backbone отсутствует в client-only, поэтому файл ограничен cfg(full).
Client-only tests check успешен, workspace all-targets check успешен;
transport lib — 460 passed, 4 ignored; fmt/diff checks чистые. Прежние warnings
без изменений. Эталон Python ea98db4f не изменён.

Расширена проверка actor/ingress под конкурентной нагрузкой. Сопоставимый
before/after baseline, sustained/many-peer fairness и peak/RSS измерения
по-прежнему остаются; этап 5 открыт, версия 1.0.1.

### Продолжение этапа 5: sampled RSS peak и process VmHWM под Backbone нагрузкой

Двух-peer TX benchmark теперь наблюдает RSS во время admission, gate и recovery:
Tokio sampler запрашивает /proc/self/status каждые5ms, пропускает пропущенные
ticks и сообщает max фактического интервала. Отдельно читаются VmRSS и VmHWM
до/после нагрузки. Sampler отменяется и ожидается перед финальным отчётом;
при panic/timeout его также отменяет существующий task guard.

Вывод дополнен sampled_peak_kib, valid/unavailable sample counts,
requested_period_ms, observed_max_gap_ms и lifetime_hwm_start/end_kib.
Отсутствие procfs/поля/корректной единицы даёт None, не нулевую память.
Parser unit test проверяет независимые RSS/HWM, пропуски, неверные значения
и единицы; накопитель сохраняет максимум и считает недоступные наблюдения.

Два отдельных debug-прогона штатной команды benchmark:
- sampled peak15744/15488 KiB, valid samples1406/1407, unavailable0;
- actual max sampling gap6.010/6.068ms при запросе5ms;
- lifetime VmHWM start10624/10368 KiB, end15744/15488 KiB;
- fast payload throughput42.754/43.596 MiB/s, p99 13.771/13.550ms,
  fast drops0; slow accepted282, rejected3814 в обоих прогонах.

Sampled peak может пропустить короткие всплески. VmHWM — сообщаемый ядром
максимум за жизнь всего процесса, включая время до нагрузки. Оба значения
включают harness/peers/sampler и не доказывают bound драйвера; sampling добавляет
overhead. Лимит encoded TX bytes4MiB проверяется независимо. CONFIG уточнён.

Проверки: interface lib — 229 passed, 6 ignored; benchmark дважды успешен;
workspace all-targets, fmt/diff checks успешны. Прежние warnings сохраняются.
Эталон Python ea98db4f не изменён.

Память текущего ограниченного двух-peer сценария измерена. Сопоставимый
before/after baseline, sustained/many-peer fairness и масштабирование памяти
при росте нагрузки ещё остаются. Этап 5 открыт, версия 1.0.1.

### Продолжение этапа 5: TCP сравнение старого и текущего TX writer

Добавлен ignored `compare_legacy_and_coalesced_tcp_writers`. Старый цикл взят
из `git show 7a5747c^:crates/rns-interface/src/backbone.rs`: frame для одного
сообщения, предучёт txb, write_all. Он воспроизводится только в тесте с теми же
нынешними HDLC helpers, что и текущий coalescing writer; рабочий driver не менялся.

Одинаковые prefilled bounded queues4096×500 bytes, sequence и HDLC FLAG/ESC
в payload, wire total4071456 bytes. TCP loopback, NODELAY, requested SO_SNDBUF/
SO_RCVBUF65536, фактические значения131072/131072 на данном Linux. Два раунда
AB/BA для каждого режима чтения: continuous и pause100ms + sleep1ms после
каждого read до8192 bytes. Проверяется весь payload, порядок, число кадров,
полная длина wire и txb. Join не оставляет detached writer/reader tasks;
общий deadline30s. Успешные write polls считаются отдельно от OS syscalls.

Debug-результаты двух раундов (один полный запуск8 случаев):

| Получатель | Старый цикл MiB/s | Текущий MiB/s | Write polls старый / текущий |
| --- | --- | --- | --- |
| Continuous | 1.172 / 1.182 | 7.330 / 7.325 | 4105 / 64 |
| Paused/throttled | 1.070 / 1.092 | 1.047 / 1.057 | 4144–4143 / 92 |

Continuous p99 completion: старый1651.740–1665.102ms, текущий265.441–265.599ms.
Paused/throttled p99: старый1772.365–1807.892ms, текущий1829.687–1848.308ms.
При медленном чтении throughput не улучшился, текущий вариант немного медленнее.
Completion отсчитывается от запуска уже заполненной очереди, не от времени
индивидуального producer enqueue. Результат относится к этому debug/socket
стенду; managed admission, egress controller, полная старая версия transport и
сравнение RSS между версиями сюда не входят. Универсальное ускорение не заявляется.

Команда и ограничения добавлены в CONFIG. Проверки: новый benchmark — 1 passed
(8 случаев); interface lib — 229 passed, 7 ignored; workspace all-targets и
fmt/diff checks успешны. Прежние warnings сохраняются, Python ea98db4f не изменён.

Изолированное before/after TX writer сравнение выполнено. Whole-pipeline
сравнение, sustained/many-peer fairness и масштабирование памяти остаются.
Этап 5 открыт, версия остаётся 1.0.1.

### Продолжение этапа 5: четыре Backbone peers, 100 раундов давления

Добавлен full-only ignored
`four_backbone_peers_repeated_pressure_and_progress` в `backbone_ingress_load`.
Четыре неизменных TCP-соединения, 100 раундов по128 уникальных DATA-пакетов
на peer; producers стартуют через barrier и работают отдельными задачами.
JoinSet ограничен четырьмя задачами и отменяет их при выходе/ошибке. Между
раундами — полная сверка доставки/drops и пауза250ms, без переподключений.

В каждом раунде проверяются прогресс всех четырёх peers, возрастающая
последовательность каждого peer, содержимое пакетов, отсутствие drops других
классов и delivery+DATA drops=offered. Control queries имеют deadline1s;
DATA/class capacities по4, application queue ограничена одним раундом+markers.
Финальные marker-пакеты проходят через все прежние соединения без новых drops.
Общий deadline180s допускает policy holds; actor/drivers имеют task guard.

Команда:
`cargo test -p rns-runtime --test backbone_ingress_load four_backbone -- --ignored --nocapture`.
Полный прогон100 раундов занял75.516s: offered51200, delivered
[11905,11939,11827,11816], DATA drops3713. Минимальная доставка за один раунд
по peers:[103,108,105,106] из128; control queries18816, max10.957ms.
Все assertions и конечные markers успешны. Ранний20-round проверочный прогон
тоже прошёл. Эти результаты не являются общим доказательством fairness:
раунды ждут reconciliation, канал не насыщается непрерывно, часы работы,
неравные скорости producers и churn здесь не моделируются.
Неигнорируемый ingress target после изменений: 1 passed, 1 ignored.

Workspace all-targets и client-only tests checks успешны; fmt/diff checks
чистые. Прежние warnings сохраняются, Python ea98db4f не изменён.
CONFIG описывает запуск и границы сценария.

Проверка повторного давления четырёх peers выполнена. Whole-pipeline
before/after сравнение, непрерывная/неравномерная нагрузка и масштабирование
памяти при росте числа peers остаются. Этап 5 открыт, версия 1.0.1.

### Продолжение этапа 5: независимые Backbone потоки с темпами64:16:4:1

Добавлен full-only ignored `asymmetric_backbone_streams_accounting_and_recovery`.
Четыре producers имеют только начальный barrier, затем независимо отправляют
по500 batches с периодом20ms и размерами64/16/4/1. Между batches нет ожидания
других peers или сверки доставки; при socket backpressure producer ждёт write,
а timer пропускает пропущенные ticks. Всего42500 DATA-пакетов по256 bytes,
существующие raw/control каналы, class queues по4, application queue ограничена
полным конечным workload+markers, чтобы не добавлять app-channel drops.

Проверяются content/order каждого peer, delivery+DATA drops=42500,
отсутствие drops других классов, control queries с deadline1s и markers на
всех соединениях после drain. Работа по доставке ограничена512 событиями
между control probes. JoinSet и task guard отменяют producers/actor/drivers
при выходе или deadline120s. Нагрузка paced, не гарантированное насыщение
line rate; равные доли полосы для неравных источников не утверждаются.

Команда:
`cargo test -p rns-runtime --test backbone_ingress_load asymmetric_backbone -- --ignored --nocapture`.
Прогон37.816s: offered[32000,8000,2000,500], delivered
[29617,7526,1962,492], DATA drops2903, полная сверка и markers успешны.
Producer completion times:[28.022,9.982,9.982,9.982]s;
наблюдаемые max gaps доставки:[12.113,0.051,0.054,0.045]s.
Gaps включают ожидание первого пакета, но не тишину после последнего;
это наблюдение, не доказательство отсутствия starvation во всех условиях.
Control queries5503, max9.870ms. Задержка быстрого потока не скрывается:
он завершил передачу позже остальных, затем все принятые пакеты учтены.

Workspace all-targets и client-only tests checks, fmt/diff checks успешны.
Прежние warnings сохраняются. Python-эталон ea98db4f не изменён.
CONFIG описывает команду и ограничения.

Неигнорируемый ingress target: 1 passed, 2 ignored.
Независимая неравномерная нагрузка проверена. Whole-pipeline before/after,
масштабирование RSS при росте числа peers, churn/длительный soak остаются.
Этап 5 открыт, версия 1.0.1.

### Продолжение этапа 5: память при 1/2/4/8 остановленных Backbone peers

Добавлен ignored `measure_tcp_memory_scaling`: родитель запускает каждый
случай в отдельном процессе через exact internal fixture. Так allocator и
VmHWM предыдущего случая не влияют на следующий. Deadline дочернего сценария
30s, внешнего ожидания40s; kill-on-drop и guards отменяют дочерний процесс,
драйверы и параллельные readers при ошибке/таймауте.

По512 попыток отправки16384 bytes на соединение, round-robin admission.
Настоящие BackboneClient и raw TCP peers используют loopback/свободные порты.
Чтение остановлено до gating всех соединений, затем все peers читают
одновременно. Проверяются encoded TX quota4MiB на peer, drops, полный
content/order принятых кадров, нулевой остаток TX и release gates при живых
соединениях. Transport actor в этом сценарии не участвует.

Команда:
`cargo test -p rns-interface backbone::tx_tests::measure_tcp_memory_scaling -- --ignored --exact --nocapture`.

Первый полный успешный debug-прогон20.08s (примерно5s на случай):

| Peers | RSS до соединений, KiB | RSS после соединений | RSS gated | RSS drained | VmHWM в конце |
|---|---|---|---|---|---|
| 1 | 10184 | 12488 | 16840 | 17224 | 17224 |
| 2 | 8336 | 10640 | 19088 | 19728 | 19728 |
| 4 | 8080 | 10768 | 27536 | 28560 | 28560 |
| 8 | 8080 | 10768 | 43920 | 25388 | 45456 |

В каждом случае на каждом peer accepted282/rejected230, наблюдаемый максимум
учтённого TX4184744 bytes при лимите4194304. Все принятые кадры доставлены,
gates освобождены, соединения online. RSS включает оба TCP endpoints и harness,
но не память kernel socket buffers; checkpoints могут пропускать пики,
VmHWM относится ко всей жизни дочернего процесса. Отдельного sampler здесь
нет. Retention allocator и разные базовые RSS не позволяют объявлять точную
линейную формулу памяти/production capacity или отсутствие утечек при churn.
Без procfs значения остаются unavailable, а не нулём.

Контрольный повтор всех четырёх случаев успешен за20.07s. Gated RSS:
14992/19088/27412/45768KiB, конечный VmHWM15504/19856/28308/47048KiB.
В случае1 peer приняты283, отклонены229; остальные снова282/230 на peer.
Тест проверяет сверку фактических значений, а не фиксирует scheduling-dependent
число принятых кадров или абсолютный RSS как обязательный порог.

Ранний вариант с уменьшением SO_RCVBUF до4096 и последующим увеличением
получил два таймаута30s (в случаях1 и2 peers, во втором подтверждена стадия
drain). Финальный сценарий не меняет receive buffer активного сокета,
использует штатные OS-буферы и сохраняет реальные stop/read/gating проверки.
Таймауты раннего варианта не считаются успешными проверками; точная причина
взаимодействия изменяемого TCP окна отдельно не установлена.

Регрессии интерфейсов:229 passed,9 ignored. Workspace all-targets,
fmt/diff checks успешны, прежние warnings сохраняются. Python ea98db4f
не изменён. CONFIG описывает команду и границы измерений.

Масштабирование памяти на конечной нагрузке до8 peers проверено.
Whole-pipeline before/after, churn/длительный soak остаются.
Этап 5 открыт, версия 1.0.1.

### Продолжение этапа 5: повторное создание и освобождение нагруженных клиентов

Добавлен ignored `repeated_pressured_client_lifecycles_release_reservations`:
12 раундов по4 новых BackboneClient/raw TCP peers, каждый на свободном
loopback-порту. На каждом соединении сначала проверяется доставка уникального
HDLC marker, затем512 попыток отправить отдельно выделенные16384 bytes.
При остановленном чтении все четыре TX должны перейти в gated за6s;
учтённый encoded backlog проверяется против4MiB на каждом шаге admission.

После gating два peers отправляют FIN с сохранением read half, два — RST;
роли чередуются по раундам. За3s требуется ровно одна deregistration на id
и завершение всех драйверов. Проверяются offline, закрытый TX sender,
buffered=0, gated=false, соответствие dropped_frames отказам admission.
После удаления handles слабые ссылки на TX accounting должны перестать
upgrade: forwarding/writer не удерживают учёт очереди. Guard отменяет
драйверы при ошибке/таймауте; внешний deadline120s.

Это полный dispose с max_reconnect_tries=1 и последующим созданием новых
handles, не автоматический reconnect того же handle, не listener flap policy
и не transport-actor churn. Доставка нагрузки после преднамеренного разрыва
не ожидается: accepted data может быть потеряна, drops здесь только admission.
Тест не меняет runtime-политику и не добавляет гарантии доставки при разрыве.

Команда (запускать отдельно от других тестов того же процесса):
`cargo test -p rns-interface backbone::tx_tests::repeated_pressured_client_lifecycles_release_reservations -- --ignored --exact --nocapture`.

Debug-прогон успешен за48.26s:48 жизненных циклов,24 FIN/24 RST.
На каждом peer приняты285 и отклонены227 из512 попыток (marker отдельно).
Всего24576 попыток,13680 accepted,10896 rejected; тест сверяет фактические
счётчики и не требует именно такого scheduling-dependent распределения.
После каждого раунда все TX reservations/accounting освобождены;
число process descriptors10 до нагрузки и после каждого из12 раундов.
RSS baseline8204KiB; после раундов:
27532,27788,27788,27788,27916,27916,27916,27916,28044,28044,28044,28044KiB.
Конечный VmHWM28044KiB. Метрики process-wide, checkpoints не ловят все пики,
allocator retention не равен утечке; строгих RSS/FD порогов в тесте нет.
Отсутствующий procfs даёт unavailable. Короткий прогон не доказывает отсутствие
утечек на часах работы или во всех сценариях reconnect.

Регрессии интерфейсов:229 passed,10 ignored. Workspace all-targets,
fmt/diff checks успешны; прежние warnings database_path/tracing prelude
сохраняются. Python ea98db4f не изменён. CONFIG дополнен командой и границами.

Повторный pressured client lifecycle проверен. Whole-pipeline before/after,
длительный soak и churn на уровне transport остаются.
Этап 5 открыт, версия 1.0.1.

### Продолжение этапа 5: сравнение connected TX pipeline до/после

Добавлен ignored `compare_live_tcp_transmit_pipelines`. Текущая ветка
использует настоящий BackboneClient; старая воспроизводит connected TX path
из7a5747c^: два mpsc по1024 frames, отдельные forward/writer tasks,
покадровый HDLC/write_all без byte quota и egress controller. Сохраняются
текущие HDLC helpers и socket tuning в обеих ветках. Исторические DNS,
reconnect, RX и весь transport actor не реконструируются: это сравнение
TX datapath от admission до декодирования TCP, не целых версий Reticulum.

После готовности соединения producer делает4096 try_send попыток с payload
2048 bytes (FLAG fill, ESC marker, sequence, timestamp), yield каждые32.
Очереди изначально пусты; reader работает одновременно с admission. Два
режима: постоянное чтение и pause100ms + sleep1ms после каждого read≤8192.
Каждый режим проверяется AB/BA; deadline20s на случай, guards отменяют tasks.
После закрытия producer требуется EOF, точная сверка всех принятых sequence,
content и wire bytes, TX counter, rejected+accepted=4096; для managed TX
также quota≤4MiB, освобождение buffered/gated и drops=отказам admission.

Команды:
`cargo test -p rns-interface backbone::tx_tests::compare_live_tcp_transmit_pipelines -- --ignored --exact --nocapture`
и та же команда с `--release` после `cargo test`.

Это одинаковая политика конечного burst, но не одинаковый offered rate по
настенным часам: scheduling/admission cost меняют время producer. При отказах
доставленные объёмы различаются; throughput считается по accepted payload,
latency только по принятым кадрам, от timestamp перед admission до декодирования.
Нет строгих performance thresholds или вывода «меньшая latency = ускорение».

Финальные последовательные прогоны: debug13.75s, release8.68s, по8 случаев.
Диапазоны двух AB/BA наблюдений:

| Profile / receiver | TX path | Accepted | Rejected | Delivered MiB/s | p99, ms |
|---|---|---|---|---|---|
| debug / draining | legacy | 4096 | 0 | 13.753–14.036 | 7.745–7.839 |
| debug / draining | current | 3081–3105 | 991–1015 | 10.289–10.387 | 198.830–199.687 |
| debug / throttled | legacy | 2168 | 1928 | 1.093–1.095 | 3782.017–3787.325 |
| debug / throttled | current | 1152–1164 | 2932–2944 | 1.231–1.237 | 1759.664–1770.285 |
| release / draining | legacy | 4096 | 0 | 138.159–148.340 | 0.968–1.194 |
| release / draining | current | 4096 | 0 | 142.086–147.806 | 1.051–1.455 |
| release / throttled | legacy | 2168 | 1928 | 1.545–1.561 | 2678.047–2702.060 |
| release / throttled | current | 1146–1153 | 2943–2950 | 1.483–1.513 | 1464.217–1483.320 |

Отрицательный debug-результат не скрывается: текущий driver на этом burst
имеет ниже throughput, больше отказов и выше latency при быстром receiver.
В release обе ветки доставили всю нагрузку быстрому receiver без drops;
два наблюдения не доказывают устойчивое ускорение. При медленном receiver
current допускает меньшую очередь/объём и чаще отказывает; уменьшение p99
сопровождается потерями admission, а release throughput чуть ниже legacy.
Причины debug-разницы не профилировались; результаты не переносятся на
равный paced offered rate или production capacity без дополнительных замеров.
Предварительные debug/release прогоны также успешны. В финальном варианте
выравнен старт после online и убран лишний счётчик write polls только у
исторической ветки; таблица относится к этому варианту. Общий helper
legacy_socket_writer стал generic по AsyncWrite: прежний изолированный writer
benchmark сохраняет одинаковые CountedSocket wrappers в обеих ветках,
а новый pipeline benchmark не добавляет их ни в одну ветку.

Регрессии интерфейсов:229 passed,11 ignored; workspace all-targets,
fmt/diff checks успешны. Прежние warnings сохраняются, Python ea98db4f
не изменён. CONFIG содержит команды и ограничения сравнения.

Сравнение старой/текущей connected TX-цепочки выполнено. Сравнение всей
цепочки с transport actor, длительный soak и transport-level churn остаются.
Этап 5 открыт, версия 1.0.1.

### Продолжение этапа 5: смена Backbone peers при работающем transport actor

Добавлен full-only ignored `backbone_actor_churn_preserves_progress_and_control`
в `backbone_ingress_load`. Один actor работает на протяжении64 раундов;
в каждом регистрируются4 новых настоящих BackboneClient на loopback/свободных
портах. Используются повторно ID1..4 через RegisterInterface в control channel,
а не прямое изменение таблицы запущенного actor. Все DATA/class queues по4,
application channel ограничен516 событиями и не растёт с числом раундов.

В первой фазе четыре независимых producer со стартовым barrier отправляют
по128 DATA-пакетов256 bytes. После полной сверки доставки/drop два peers
отправляют FIN с сохранением read halves; другие два отправляют ещё по128,
пока actor может обрабатывать DeregisterInterface от отключившихся драйверов.
JoinSet producers опрашивается вместе с bounded delivery batches≤512 и RPC
queue stats. Проверяются порядок/content/ID, положительная доставка у каждого
активного peer в каждой фазе, monotonic DATA drops и отсутствие drops других
классов. Весь offered workload равен delivery+DATA drops; потерю ещё не
прочитанных байтов при разрыве этот сценарий намеренно не моделирует.

GetInterfaceStats должен показать только survivors3/4; serial markers на них
проверяют восстановление доставки без дополнительных drops. После их FIN
требуются завершение всех read_task и пустая таблица интерфейсов до нового
раунда/повторного использования ID. Control queries имеют deadline1s,
ожидания cleanup и финального shutdown actor —3s. Guards отменяют actor,
драйверы/producers при ошибке или внешнем таймауте.

Команда:
`cargo test -p rns-runtime --test backbone_ingress_load backbone_actor_churn -- --ignored --nocapture`.
Для более продолжительного прогона RNS_BACKBONE_CHURN_ROUNDS принимает1..10000,
default64; общий deadline rounds*20+30s, между раундами pause1s. Это смена
handles/регистраций, не автоматический reconnect того же handle и не
listener-side flap protection. Не гарантируется конкретный момент gating,
непрерывное насыщение или отсутствие starvation во всех условиях.

Default debug-прогон64 раундов успешен за90.66s:256 подключений и их
удалений,49152 DATA workload, per-peer delivered[7516,7610,15218,15219],
сумма45563, DATA drops3589. Дополнительно все128 survivor markers доставлены
без увеличения drops. RPC queue probes2344, max11.997ms; очереди≤4,
GetInterfaceStats подтверждает survivors и пустую таблицу после каждого
раунда. Все drivers завершились; финальный actor shutdown успешен.
Время включает63 паузы по1s и адаптивные задержки. Это90-секундная проверка
с повторным churn, не выполненный многочасовой soak и не доказательство
отсутствия утечек памяти при произвольной длительности.

Проверен и override `RNS_BACKBONE_CHURN_ROUNDS=1`:1 раунд успешен за0.04s,
768 offered,727 delivered,41 DATA drops,8 queue probes/max0.803ms;
survivor markers, удаление интерфейсов и shutdown также успешны.

Неигнорируемый ingress target:1 passed,3 ignored. Workspace all-targets,
client-only tests check, fmt/diff checks успешны. Прежние warnings сохраняются;
Python ea98db4f не изменён. CONFIG содержит команду, настройку длительности
и границы сценария.

Transport-level churn с восстановлением проверен на64 раундах. Сравнение
старой/текущей цепочки с transport actor и многочасовой soak остаются.
Этап 5 открыт, версия 1.0.1.

### Завершение функциональных пунктов этапа 5: Local MTU и forced bitrate

По указанию пользователя работа возвращена к функционалу; длительные тесты
и дальнейшее расширение нагрузочных сценариев отложены.

- InterfaceDiagnostics теперь различает отсутствие поддержки MTU upgrade и
  явно неизвестный hardware MTU. Существующие реализации trait совместимы
  через default-метод; LinkMtuDiagnostics(None) сообщает unknown.
- Локальный LinkRequest до доставки приложению удаляет ненулевое signalling
  при unknown MTU либо ограничивает offer известным cap (без upgrade —500).
  Недопустимый mode при фактическом rewrite учитывается как protocol violation
  входного интерфейса, пакет не доставляется. Zero/unchanged offers по-прежнему
  проверяет handshake. Ключи и Link ID сохраняются. Transit больше не принимает
  резервный raw receive limit за известный MTU предыдущего hop.
- force_shared_instance_bitrate подключён от YAML/normalized config к
  автоматически создаваемым shared endpoints в full и client-only runtime.
  Положительное значение определяет bitrate, automatic MTU и сериализованную
  задержку raw_bytes*8/bitrate до HDLC write через отменяемый Tokio timer.
  Ноль отвергается схемой, runtime parser и новым driver API до подключения.
- Shared-instance TCP использует Local HDLC driver через tcp:// endpoint,
  как и Unix IPC, вместо обычного сетевого TCP driver. Без override default
  bitrate1G/MTU262144 соответствует Python Local; connecting clients сохраняют
  настройки и стабильный канал при reconnect. Listener/connecting-client MTU
  с override берётся из общей автоматической кривой. Accepted Local clients
  наследуют bitrate/pacing, но сохраняют constructor MTU262144 — именно так
  работает Python LocalServer, который не вызывает optimise_mtu на child.
- Старые LocalConfig structs и spawn API сохранены; добавлены варианты
  spawn_local_server_with_bitrate/spawn_reconnecting_local_client_with_bitrate.
  Client-only ReticulumConfig дополнен force_shared_instance_bitrate.

Обоснованные границы: Rust сохраняет bounded integer RX limits вместо
nullable/unbounded буфера (500 при неизвестном Local MTU), не воспроизводит
ошибку Python min(nh_mtu, None), не передаёт усечённые oversized KISS frames.
Forced Local pacing действует и в async backend Rust; Python epoll TX путь
пропускает artificial delay, обычный Local backend задерживает. Эти отличия
явно описаны в CONFIG. shared_medium не переносится как неиспользуемое ядром
Python поле. Weave не добавляется; внешняя next-hop MTU RPC/расширенная
диагностика не объявляются реализованными.

Короткие целевые проверки: Local MTU/pacing с виртуальным временем,
матрица local LinkRequest strip/clamp/mode, существующий runtime shared TCP
server+2 clients с forced1M, RPC metadata и передачей данных — успешны.
В последнем проверены server MTU2048, connecting client2048 и children262144,
bitrate1M на всех. Workspace all-targets и client-only checks успешны;
старые warnings database_path/tracing prelude сохраняются. Python ea98db4f чист.

Финальные короткие targets: Local9 passed (1.00s), LinkRequest10 passed
(0.01s), client-only reticulum3 passed (0.77s). Shared TCP runtime сценарий
1 passed (0.52s); fmt/diff checks чистые. Новых нагрузочных тестов нет.

Функциональная часть этапа 5 закрыта с перечисленными границами применимости.
Дополнительное before/after через весь actor и многочасовой soak остаются
отложенной валидацией, а не блокируют переход к функционалу этапа 6.
Это не завершение всего обновления: этапы6/7 и итоговая интеграционная
проверка впереди; версия остаётся1.0.1.

### Этап 6: medium_path_timeout и автоматические таймауты CLI

Перенесён расчёт Python Transport.medium_path_timeout из ea98db4f:
2*(MTU500*8/max(lowest_online_nonzero_bitrate,5))+DEFAULT_PER_HOP_TIMEOUT6.
Нет подходящей среды —0. Actor вычисляет минимум по актуальным интерфейсам
на запросе, учитывает online/изменение bitrate/удаление; отсутствующий legacy
online flag означает online. Python cached minimum/stale-state не копируется.
MINIMUM_BITRATE вынесен в общий wire constant, runtime сохраняет прежний alias.

Добавлены TransportQuery::MediumPathTimeout, RpcRequest::GetMediumPathTimeout,
dispatch и MessagePack get=medium_path_timeout с FloatResult(seconds/None).
ReticulumHandle::medium_path_timeout доступен в full/client-only builds.
Client control plane проксирует запрос daemon; без доступного авторизованного
RPC действует существующий local fallback с его ограниченным обзором сети.
Первый hop timeout не заменён и по-прежнему зависит от destination.

rncp различает отсутствие -w и explicit value: default operation/path budgets
max(15s,medium). rnpath адаптирует default discovery и remote management waits.
rnprobe default path/proof waits=max(12s+first_hop,medium), CLI получает shared
medium floor, local estimate обновляется для каждого automatic wait. Старый
probe_once API сохранён, новый вариант принимает дополнительный floor.
Explicit -w/remote-timeout не увеличиваются — приоритет пользовательского
значения из плана; это документированное отличие от Python rncp/rnpath max.
Старые daemon/недоступный query оставляют defaults/fallback. Полный аудит
Resource timers, включая отдельный transfer proof deadline, ещё впереди.

Доказательство переноса без расширения тестовой инфраструктуры: один inline
actor test для0/online/offline/смены bitrate/минимума5/removal и один inline
RPC test request/response shape и round-trip, оба успешны. Существующий
перечень codec variants дополнен новым запросом. Новых test-only файлов нет.
Команды: `cargo test -p rns-transport --lib medium_path_timeout --quiet` и
`cargo test -p rns-runtime --lib medium_path_timeout --quiet` — по1 passed.
Workspace all-targets и client-only tests checks успешны; прежние warnings
database_path/tracing prelude сохраняются. Python reference чист, ea98db4f.

Первые два функциональных пункта этапа6 выполнены. Далее — Destination
max_request_size и отказ от слишком большого Resource до его приёма,
затем аудит Channel/Buffer/Resource. Этап6 открыт, версия1.0.1.

### Этап 6: Destination.max_request_size

Добавлены Destination::set_max_request_size / max_request_size /
clear_max_request_size: None по умолчанию, usize в байтах, 0 допустим.
LinkManager предоставляет те же методы и применяет лимит ко всем своим
существующим и будущим links, синхронизируя принадлежащий ему Destination.
Отдельно созданный Destination не конфигурирует чужой LinkManager.

Обычный REQUEST проверяется после decrypt, до unpack: учитывается весь
MessagePack envelope, а не только пользовательские данные. Превышение
игнорируется без закрытия link. Для request Resource проверяется поле d
объявления (Python ResourceAdvertisement.read_size), не transfer_size/t.
Превышение вызывает encrypted RESOURCE_RCL с resource hash до создания
transfer/split state. Без request handler сохранено прежнее игнорирование.
Перед dispatch завершённого Resource дополнительно проверяется реальная
длина packed request. Это не общий memory cap для недостоверных объявлений
или распаковки. Ответы и обычные Resource лимитом не затрагиваются.

Один inline test покрывает packet ниже/на/выше лимита, 0, clear, early
RESOURCE_RCL с проверкой payload и отсутствия transfer/split state.
`cargo test -p rns-runtime --lib max_request_size --quiet`: 1 passed, 0.01s.
`cargo check --workspace --all-targets --quiet`: успешно, прежние warnings
database_path/tracing prelude. Новых test-only файлов нет, длительные тесты
не запускались. Сопоставлено с чистым Python reference ea98db4f.

Третий функциональный пункт этапа6 выполнен. Далее — существующий
request_with_metadata_limit/max_response_size, затем Channel/Buffer и
Resource-исправления. Этап6 ещё открыт, версия1.0.1.

### Этап 6: аудит существующего ограничения ответа

request_with_metadata_limit не реализован заново, сигнатура сохранена.
Исправлена ошибка суммирования d: каждый сегмент содержит полный размер
ответа, включая envelope и metadata, а не размер отдельного сегмента.
Превышение теперь отправляет encrypted RESOURCE_RCL до приёма вместо одного
локального error. Повторы ADV не сбрасывают transfer и не учитываются дважды.
Проверяются согласованность original_hash/числа сегментов/d, диапазон индексов
и MAX_SEGMENTS до создания coordinator; конфликтующие hash/index отклоняются.
Реальный собранный размер с metadata prefixes проверяется перед proof/delivery,
до объединения сегментов. Это не ограничение памяти самой декомпрессии.

Семантика packet response сопоставлена: Python использует
len(packb(response_data))-2, Rust ограничивает возвращаемые байты. Для binary
с двухбайтовым префиксом совпадает, для других типов/длин может отличаться.
Существующий Rust-контракт оставлен для совместимости приложений; различие
описано в CONFIG.md. Raw file response Python, не содержащий packed envelope,
не добавлен этим исправлением; текущий decoder ожидает [request_id, data].

Точечный inline test проверяет d=32 для двух сегментов и повторного ADV при
лимитах31/32/33, ResourceReq для допуска и decrypt RESOURCE_RCL при отказе.
`cargo test -p rns-runtime --lib response_size_limit_uses_total --quiet`:
1 passed, 0.03s. Это проверка admission, не полный многосегментный interop.
`cargo check --workspace --all-targets --quiet` и `git diff --check` успешны;
сохраняются прежние warnings database_path/tracing prelude.
Новых test-only файлов нет, длительные тесты не запускались. Python reference
ea98db4f чист. Пункт аудита ограничения ответа выполнен с описанными границами;
далее — полный Link MDU в Channel/Buffer и оставшийся Resource-аудит.

### Этап 6: Link MDU в Channel/Buffer

Расчёт Channel::channel_mdu уже существовал, но send не применял его.
Channel получил настраиваемый Link MDU и getter payload MDU; send проверяет
envelope до расходования sequence/window. Превышение согласованного размера
или u16 length возвращает MessageTooBig. Runtime LinkSession/LinkManager и
rnsh client связывают канал с текущим согласованным Link.mdu.
Standalone-конструкторы сохранены: до set_link_mdu применяется wire ceiling,
а не предположение о конкретном размере транспорта.

LinkSession::channel_buffer и LinkChannel::buffer создают существующий Buffer
с бюджетом min(link_mdu-6,65535)-2. Старый явный max_data_len API сохранён,
но ограничен wire ceiling65533; нулевая вместимость теперь возвращает ошибку
для непустой записи вместо бесконечного цикла. Запись/ACK/EOF остаются под
управлением вызывающего кода. Как Python, writer сохраняет MAX_CHUNK_LEN16KiB
и прежние правила compression; это не ошибочный legacy MDU cap. Бюджет Buffer
фиксируется при создании. Собственный rnsh stream chunking не менялся.

Один inline test проводит send и Buffer round-trip при Link MDU415/1100/70000,
проверяет точную границу/превышение, отсутствие расхода sequence при отказе,
увеличенные stream frames, EOF и zero capacity. Команда
`cargo test -p rns-protocol --lib negotiated_mdu_bounds --quiet`:
1 passed, 1.62s. Это локальная проверка формата, не Python↔Rust live interop.
Workspace all-targets check, fmt check и diff check успешны; прежние warnings
database_path/tracing prelude сохраняются.
Новых test-only файлов нет, длительные тесты не запускались. Python reference
ea98db4f чист. Пункт Channel/Buffer перенесён; далее — оставшиеся Resource
исправления и Link/watchdog/ratchet/blackholed API. Этап6 открыт, версия1.0.1.

### Этап 6: отмена Resource и освобождение split-state

Исправлены конкретные расхождения с Link.py/Resource.py1.5.2:
LinkSession при отправке больше не принимает любой пакет контекста RCL за
отмену: требуется успешный decrypt и hash текущего сегмента. Чужой hash,
короткий payload и невалидный ciphertext игнорируются. Последовательная
отправка split Resource останавливается через существующий Result/error path.

При ожидании ответа matching RESOURCE_ICL теперь завершает запрос с ошибкой
и отправляет encrypted RESOURCE_RCL; локальные transfers/coordinator целиком
освобождаются при выходе. Link остаётся пригодным к использованию.
LinkManager при RCL освобождает не только активный outbound segment, но и
очередь оставшихся сегментов по original_hash. ICL использует единый inbound
cleanup, включая tracking отменённого сегмента (ранее очищались лишь siblings),
и отвечает RCL для активной передачи, как Python Resource.cancel.

Один новый inline test проверяет удаление queued tail/tracking и отсутствие
изменений при чужом hash. Существующий response-admission test дополнен
ICL текущего/чужого сегмента, проверкой encrypted RCL и invalid/short cancel.
`cargo test -p rns-runtime --lib resource_cancel --quiet`: 1 passed, 0.01s;
`cargo test -p rns-runtime --lib response_size_limit_uses_total --quiet`:
1 passed, 0.03s. Новых test-only файлов нет; длительные тесты не запускались.
Python reference ea98db4f чист. Resource-аудит остаётся открытым: индексы и
перепривязка частей, потоковые источники, регрессия1.5.2 и оставшиеся таймеры.
Workspace all-targets check и diff check успешны; прежние warnings
database_path/tracing prelude сохраняются.

### Этап 6: индексы частей и протокольная отмена

Сопоставлены точные Python fixes65222e0d (receive_part/request_next index),
0c410277 (не перепривязывать data при создании частей), c1d7c12b/6bc0481c
(stream proxy, flush/seek, регрессия1.5.2). Rust consecutive_completed хранит
число завершённых подряд частей, а не индекс последней: receive_part и
request_next начинают с одного первого незавершённого слота. Разбиение blob
на parts через chunks не перепривязывает исходные данные. Эти fixes уже
покрыты текущей архитектурой, повторная реализация не нужна.

Stream proxy в Rust отсутствует: Resource API принимает Vec<u8>, rncp читает
файл целиком. Ошибка Python temporary-file flush/seek здесь неприменима,
но полноценный bounded-memory reader/file source остаётся отсутствующей
возможностью, а не выполненным потоковым переносом.

Найден и закрыт runtime gap: SendCancel из протокольного handler больше не
игнорируется LinkSession. Некорректный exhausted request вызывает encrypted
ICL и немедленный Result/error вместо ожидания proof timeout. Cancelling HMU
в ожидании ответа вызывает RCL и завершает операцию, освобождая coordinator.
LinkManager в обоих случаях освобождает transfer/split-state. Проверка hash
в OutboundTransfer::handle_request не позволяет чужому запросу менять state.

Существующий inline test non-boundary exhausted request дополнен чужим hash:
`cargo test -p rns-protocol --lib test_exhausted_request_non_boundary_cancels --quiet`
— 1 passed, 0.01s. Добавлен один inline runtime scenario с handshake и
проверкой encrypted ICL; отдельных test-only файлов нет. Длительные тесты
не запускались. Python reference ea98db4f чист. Resource-таймеры и общий
оставшийся аудит этапа6 ещё открыты; версия1.0.1.
`cargo test -p rns-runtime --lib malformed_resource_request_sends_cancel --quiet`:
1 passed, 0.01s. Workspace all-targets check и diff check успешны, прежние
warnings database_path/tracing prelude сохраняются.

### Этап 6: watchdog приёма Resource-ответа

Обнаружен пропущенный вызов существующего InboundTransfer::check_timeout:
LinkSession::wait_for_response раньше только ожидал события до общего deadline.
Добавлен select с периодическим tick1s (Python WATCHDOG_MAX_SLEEP1), missed
ticks пропускаются. При истечении адаптивного окна отправляется encrypted
RESOURCE_REQ. Формула EIFR/RTT/HMU wait/retry backoff не продублирована и не
заменена фиксированным timeout. Исчерпание retries отменяет все активные
сегменты ответа через RCL и завершает операцию с освобождением coordinator.
LinkManager теперь также отправляет RCL перед удалением inbound по timeout.
Общий deadline пользователя остаётся прежним и не продлевается повтором.

Один новый inline test проверяет retry/exhaustion с расшифровкой REQ/RCL,
существующий manager timeout test дополнен проверкой RCL. Без ожидания реальных
таймаутов: last_activity заранее сдвинут назад. Команды
`cargo test -p rns-runtime --lib response_resource_timer --quiet` и
`cargo test -p rns-runtime --lib tick_removes_timed_out_inbound_resource --quiet`:
по1 passed, 0.01s. Новых test-only файлов нет, длительные тесты не запускались.

Аудит таймеров ещё открыт: sender advertisement retries, восстановление
потерянного final proof, cleanup/signalling при общем deadline, rncp proof
wait cap120s. Наличие констант MAX_ADV_RETRIES/PROOF_TIMEOUT_FACTOR не считается
реализованным runtime-поведением. Этап6 открыт, версия1.0.1.
Workspace all-targets check, fmt check и diff check успешны; прежние warnings
database_path/tracing prelude сохраняются.

### Этап 6: повтор Resource advertisement

Добавлен отдельный OutboundTransfer::check_advertisement_timeout, не tick
старого push-window режима: он не отправляет незапрошенные части. Состояние
Advertised получает timestamp первого ADV и отдельный счётчик повторов.
Окно RTT*TRAFFIC_TIMEOUT_FACTOR6+PROCESSING_GRACE1 соответствует текущей Rust
модели Link и Python default. После первоначального ADV допускаются четыре
повтора; затем SendCancel(ICL). После valid RESOURCE_REQ повтор ADV прекращается.

LinkSession вызывает проверку раз в секунду, не продлевая общий deadline;
LinkManager — в существующем tick. Оба шлют encrypted ADV/ICL; при exhaustion
manager освобождает очередь split tail, session завершает Result/error и
не отправляет следующие сегменты. Существующие публичные конструкторы сохранены.

Один новый inline test проверяет идентичность ADV, четыре повтора, отсутствие
push-parts, сброс времени ожидания, exhaustion и прекращение после запроса.
`cargo test -p rns-protocol --lib advertisement_watchdog --quiet`: 1 passed,
0.00s. Существующий `cargo test -p rns-runtime --lib malformed_resource_request_sends_cancel --quiet`:
1 passed, 0.02s. Новых test-only файлов нет, длительные тесты не запускались.
Финальный proof/cache recovery, sender inactivity, общий deadline cleanup и
rncp proof cap120s ещё требуют переноса/аудита. Этап6 открыт, версия1.0.1.
Workspace all-targets check, fmt check и diff check успешны; Python reference
ea98db4f чист. Прежние warnings database_path/tracing prelude сохраняются.

### Этап 6: восстановление финального Resource proof

OutboundTransfer::check_sender_timeout теперь включает ожидание proof:
RTT*PROOF_TIMEOUT_FACTOR3+SENDER_GRACE_TIME10; до16 запросов, затем ICL.
QueryProof содержит hash ожидаемого Header1 Link Proof/RESOURCE_PRF с payload
resource_hash||expected_proof. LinkSession и LinkManager вызывают общий
watchdog и отправляют plaintext CACHE_REQUEST, как Packet.py Python.
Только валидный proof завершает Resource; cache hit не подменяет проверку.

Исправлен переход AwaitingProof: используется sent_parts (уникальные части
всех запросов), а не число частей только последнего RESOURCE_REQ. Это важно
для передачи, которая не помещается в одно окно запросов.

Transport хранит до1024 локально отправленных proof в FIFO memory cache,
отдельно от announce cache. Ответ на Link CACHE_REQUEST проверяет packet hash
и link_id, отправляет исходный proof на входящий интерфейс и работает на leaf.
Запросы проходят по существующей link-table цепочке до получателя. Кэш
не персистентен, вытесняется при переполнении, transit proofs не сохраняются —
это явная граница с общим Python packet cache. Старый announce path сохранён.
rncp action dispatcher умеет отправлять QueryProof, но собственный sender
loop ещё не вызывает этот watchdog; его таймеры не объявляются перенесёнными.

Два inline tests без ожидания реальных timeout:
`cargo test -p rns-protocol --lib proof_watchdog --quiet` и
`cargo test -p rns-transport --lib resource_proof_cache_replays --quiet`:
по1 passed, 0.00s. Проверены multi-request transition, timer/query/valid proof,
replay после имитации потери и отказ для чужого Link. Это не полный live
interop. Новых test-only файлов нет. Workspace all-targets check успешен,
прежние warnings database_path/tracing prelude сохраняются. Python reference
ea98db4f чист. Sender inactivity, общий deadline cleanup и rncp loop ещё
открыты; этап6 не завершён, версия1.0.1.

### Этап 6: sender inactivity, deadline и цикл rncp

Общий Resource watchdog дополнен sender inactivity в состоянии Transferring:
RTT*6*MAX_RETRIES16 + SENDER_GRACE10 + sum(1..16)*PER_RETRY_DELAY0.5.
Активность отсчитывается от последнего обработанного valid RESOURCE_REQ.
Истечение возвращает ICL и использует уже подключённый cleanup runtime.

LinkSession при истечении ожидания отправки пытается послать ICL, при истечении
ожидания ответа — RCL для активных сегментов. Сигналы best-effort/try_send:
полная очередь не продлевает deadline; локальное состояние освобождается.
Это не гарантия отправки cancel при произвольном drop application future.

rncp drive_outbound перенесён с legacy push tick на один начальный ADV и
check_sender_timeout. Части отправляются только по RESOURCE_REQ, HMU больше
не интерпретируется как receiver ACK. Прогресс — число уникальных sent_parts;
окончание — валидный proof. RCL проверяется decrypt и текущим resource hash.
Отдельный proof cap120s удалён, все сегменты ограничены общим operation deadline.
Весь Resource loop дополнительно обёрнут timeout, включая ожидание admission;
при expiry выполняется неблокирующая попытка ICL.

Две короткие inline проверки: граница inactivity при RTT0.5 (125/127s без
реального ожидания), rncp handshake → ADV без push → REQ → части → proof.
Новых test-only файлов нет, длительные тесты не запускались. Полный streaming
API по-прежнему отсутствует; оставшиеся Link keepalive/watchdog, ratchet и
blackholed announce API ещё требуют аудита. Этап6 открыт, версия1.0.1.
`cargo test -p rns-protocol --lib sender_inactivity --quiet`: 1 passed, 0.00s;
`cargo test -p rns-runtime --lib resource_sender_waits_for_requests --quiet`:
1 passed, 0.01s. Workspace all-targets check успешен; прежние warnings
database_path/tracing prelude сохраняются. Python reference чист.

### Этап 6: Link liveness, ratchet retention и blackhole API

Сопоставлены e64d8150/fb7479a6/39e3854d Python Link fixes. Найдено и исправлено
условие initiator keepalive: тишина по inbound ИЛИ outbound, а не только inbound.
Непрерывный поток от responder больше не подавляет keepalive молчащего initiator.
Stale baseline теперь inbound/proof/activation, без local outbound как ложного
доказательства живого peer. Responder в LinkManager подавляет echo при свежем
исходящем application traffic. Rust jitter сохранён как локальная особенность.
Watchdog-lock/finally и отрицательный sleep из Python неприменимы к текущему
monotonic tick с эксклюзивным &mut Link; эти Python механизмы не копировались.

9b4947ef ratchet retention уже реализован: RatchetRing::clean обрезает до
retained_count, set_retained_ratchets вызывает clean, удаляемые ключи zeroize.
Повторной реализации не сделано.

Для API signal_blackholed добавлены AnnounceError::Blackholed(hash), методы
validate_with_blackhole и verify_signature_with_blackhole с caller predicate.
Transport использует второй метод до ingress admission и сохраняет повторную
проверку перед learning. Как Python, blocked identity отклоняется до signature
verification; это policy result, а не доказательство подлинности пакета.
Старые validate/validate_with_known_key и их сигнатуры сохранены.

Короткие проверки: blackhole_validation —1 passed0.02s; существующий
test_set_retained_ratchets —1 passed0.00s; inbound_only_traffic —1 passed0.00s;
`cargo test -p rns-link --lib keepalive --quiet` —10 passed0.07s.
Команды первых проверок: `cargo test -p rns-identity --lib blackhole_validation --quiet`,
`cargo test -p rns-identity --lib test_set_retained_ratchets --quiet`,
`cargo test -p rns-link --lib inbound_only_traffic --quiet`.
Новых test-only файлов нет, длительные тесты не запускались. Этап6 ещё не
объявлен закрытым: нужна итоговая сверка оставшихся функциональных границ,
в частности отсутствующего streaming Resource API. Версия1.0.1.
Workspace all-targets check, fmt check и diff check успешны; прежние warnings
database_path/tracing prelude сохраняются. Python reference чист.

### Этап 6: Resource из AsyncRead известной длины

Добавлен LinkSession::send_resource_reader: AsyncRead+Unpin, объявленная длина,
metadata/compression/deadline. Поток не требует seek и не загружается целиком:
готовится один сегмент, следующий читается после proof предыдущего. Рабочие
копии compression/encryption ограничены сегментом, а не всем файлом. До чтения
проверяются overflow, metadata budget, MAX_RESOURCE_SIZE/MAX_SEGMENTS.
read_exact не объявляет неполный сегмент при EOF; лишние байты не потребляются.
Общий timeout охватывает чтение и все отправки. Metadata идёт только в первом
сегменте и уменьшает его data budget; d повторяет общий размер во всех ADV.
original_hash равен hash первого сегмента, как Python Resource.py.

При ошибке/timeout после подготовки выполняется best-effort ICL. LinkManager
умеет очистить coordinator/siblings по original hash между сегментами, когда
первый segment transfer уже завершён. Старые Vec API не изменены.

Один inline source/admission test проверяет два сегмента, d/index/original hash,
точное потребление reader и ранний EOF. Fixture знает исходные bytes и выдаёт
валидные proofs без передачи частей — это не полный transfer/interop test.
`cargo test -p rns-runtime --lib reader_resource_preserves --quiet`:
1 passed, 0.14s. Workspace all-targets check успешен, прежние warnings
database_path/tracing prelude сохраняются. Новых test-only файлов нет.

Границы: неизвестная длина пока требует caller-side подготовки, автоматический
spool отсутствует; CLI rncp ещё читает весь файл и не переключён на reader API.
Этап6 остаётся открыт, версия1.0.1; длительные тесты не запускались.

### Этап 6: rncp send читает файл по сегментам

CLI заменил tokio::fs::read на File::open и metadata открытого handle.
Новый RncpSendReaderRequest/rncp_send_reader принимает AsyncRead+Unpin с длиной;
старый RncpSendRequest/rncp_send_file сохранён как wrapper с Cursor<Vec<u8>>.
Runtime не создаёт MultiSegmentOutbound со всеми частями в памяти: read_exact
одного сегмента → metadata/encrypt → существующий drive_outbound/proof →
следующий сегмент. До discovery проверяются общий размер/metadata/segment cap.
Объявленный d включает metadata, original_hash — hash первого сегмента.
Timeout чтения использует остаток общего deadline. Ошибка/EOF проходит обычный
rncp Link close/cleanup; хвост файла сверх первоначальной длины не читается.
Snapshot/locking файла не добавлен, изменение файла во время чтения возможно.

Прогресс взвешен по исходным byte spans, внутри сегмента — unique sent_parts;
не нужно заранее сжимать все сегменты для вычисления будущего числа частей.
Пустой файл сохраняет metadata/proof flow. Изменение касается CLI send;
fetch server source и receiver buffering этим коммитом не менялись.

Один новый inline test проверяет отказ от чрезмерного источника до connecting.
Повторно запускаются существующие короткие sender handshake/REQ/proof и
reader source/admission проверки. Полный CLI/Python interop не запускался.
Новых test-only файлов нет; неизвестная длина пока требует caller-side
подготовки, автоматический spool ещё отсутствует. Этап6 открыт, версия1.0.1.
Команды `cargo test -p rns-runtime --lib reader_size_is_validated --quiet`,
`cargo test -p rns-runtime --lib resource_sender_waits_for_requests --quiet`,
`cargo test -p rns-runtime --lib reader_resource_preserves --quiet`:
по1 passed, соответственно0.00/0.01/0.14s. Workspace all-targets check,
fmt check и diff check успешны. Прежние warnings database_path/tracing prelude
сохраняются, Python reference чист.

### Этап 6: rncp fetch отправляет файловый источник по сегментам

Fetch handler открывает File вместо std::fs::read всего файла. Новый
RequestOutcome::ReplyWithFile сохраняет ACK/Resource flow: LinkManager готовит
один сегмент в spawn_blocking, запускает его через обычный REQ/proof watchdog,
после валидного proof готовит следующий. d включает metadata, original hash
берётся из первого сегмента; metadata присутствует только в первом.
ACL/jail и admission по размеру выполняются до положительного ACK.
Отказ admission возвращает MessagePack false; поздняя ошибка чтения/подготовки
закрывает Link. Существующий ReplyWithResource с Vec остаётся доступен.

На manager ограничены 32 подготовки/непрочитанных хвоста; общая semaphore
допускает 32 blocking подготовки одновременно. Закрытие Link освобождает
источники и отменяет ожидающие задачи; уже начатое blocking чтение завершает
текущий сегмент. Отмена активного Resource освобождает хвост на ближайшем tick.
File handle читается с нуля; snapshot/locking не добавлены. Это перенос
отправки fetch, а не изменение приёмной буферизации или spool неизвестной длины.

Один inline test file_resource_prepares_only_one_segment проверяет непрочитанный
хвост после первого сегмента, d/index/original hash, metadata и итоговый offset.
`cargo test -p rns-runtime --lib file_resource_prepares_only_one_segment --quiet`:
1 passed, 0.20s. Workspace all-targets check успешен, прежние warnings
database_path/tracing prelude сохраняются. Python reference чист.
Новых test-only файлов нет; длительные тесты и Python interop не запускались.
Этап 6 открыт, версия 1.0.1 сохранена.

### Этап 6: поток неизвестной длины в API ядра Resource

Добавлен LinkSession::send_resource_stream(reader, max_size, metadata,
auto_compress, deadline). Это перенос stream proxy из RNS/Resource.py 1.5.2:
поток сохраняется во временный файл, выполняются flush/rewind, затем работает
существующий send_resource_reader с одним сегментом за раз. Буфера всего потока
в RAM нет. Используется anonymous tempfile с удалением ОС при закрытии
последнего handle; Tokio blocking I/O может завершиться уже после отмены future.
tempfile добавлен как production dependency, версия уже присутствовала в lock.

В отличие от неограниченного Python data.read(), Rust требует max_size,
дополнительно ограниченный protocol size/segment caps с учётом metadata.
До advertisement вход целиком проверяется на лимит; для определения превышения
читается максимум один лишний байт. Spool и отправка используют общий deadline.
Пустой поток допустим. Изменений утилит нет: rncp и этап 7 не затрагивались.

Две короткие inline проверки: non-seekable duplex → spool → точные bytes после
flush/rewind, пустой/избыточный поток; existing reader admission/proof fixture
дополнен отказом публичного stream API без advertisement при превышении лимита.
`cargo test -p rns-runtime --lib resource_stream_spool --offline --quiet`:
1 passed, 0.00s; `cargo test -p rns-runtime --lib reader_resource_preserves
--offline --quiet`: 1 passed, 0.14s. Workspace all-targets check успешен;
прежние warnings database_path/tracing prelude сохраняются. Новых test-only
файлов нет, длительные тесты не запускались. Python reference ea98db4f чист.
Этап 6 остаётся открыт: приёмная буферизация больших Resources ещё не перенесена
на файловый путь; итоговая сверка покрытия этапа не завершена. Версия 1.0.1.

### Этап 6: файловый приём Resource в LinkSession

Добавлен recv_resource_file(max_size, deadline) → ReceivedFileResource с
открытым anonymous tempfile, data_size, metadata и resource_hash. Проверенный
сегмент записывается/flush до proof; предыдущие сегменты в RAM не сохраняются.
Готовый файл перематывается на начало. max_size включает metadata, возвращаемый
data_size — только payload. Проверяются advertisement и фактический размер,
последовательность индексов, общий hash/count/d; последний размер должен точно
совпадать с объявленным. Старый recv_resource с Vec сохранён.

Общий receive loop теперь вызывает watchdog и отправляет best-effort RCL
активным передачам при ошибке/timeout; обработаны ICL и отмена через HMU.
Python оставляет has_metadata на последующих сегментах, но metadata prefix там
уже нет: флаг очищается перед декодированием этих сегментов, как в LinkManager.
Файл удаляется ОС после закрытия последнего handle, в том числе при ошибке или
отмене future; уже запущенное Tokio file I/O может закончиться позже.

Один короткий inline test resource_file_receives_segments_and_rejects_oversize
передаёт два маленьких сегмента с parts и проверенными proofs, повторённым
metadata flag, проверяет файл/metadata/hash и отказ RCL до REQ по лимиту.
1 passed, 0.01s. Это не большой transfer/interop test; длительные тесты не
запускались, новых test-only файлов нет. Workspace all-targets check успешен,
прежние warnings database_path/tracing prelude сохраняются.

Граница: файловый путь доступен напрямую через LinkSession; command handle,
LinkManager completion channel и request/response API пока возвращают bytes.
Их память этим изменением не перестроена. Утилиты не затрагивались.
Этап 6 остаётся открыт до итоговой сверки покрытия; версия 1.0.1 сохранена.

### Этап 6: файловый приём через управляемую Link-сессию

Добавлен LinkSessionHandle::recv_resource_file(max_size, deadline) и команда
ReceiveResourceFile. Worker передаёт приложению ReceivedFileResource, не читая
результат обратно в Vec. Выполняется существующий файловый receive loop.
Общий deadline учитывает установление Link, ожидание места в очереди и ранее
поставленные команды. Просроченные команды и команды с уже закрытым result
channel не начинают приём. Отмена уже начатого вызова не прерывает передачу:
worker завершает её или достигает исходного deadline, невостребованный файл
закрывается при неудачной доставке результата. Остальные API не изменены.

Существующая короткая проверка файлового приёма переведена на handle/worker:
два сегмента с parts/proofs, metadata, содержимое файла и отказ по лимиту.
Дополнительно проверена просроченная команда без сетевого вывода.
`cargo test -p rns-runtime --lib resource_file_handle --offline --quiet`:
1 passed, 0.02s. Новых test-only файлов нет; длительные тесты не запускались.
Workspace all-targets check успешен; прежние warnings database_path/tracing
prelude сохраняются. Этап 6 открыт: файловый completion в LinkManager и
итоговая сверка покрытия остаются. Утилиты не менялись, версия 1.0.1.

### Этап 6: файловый completion в LinkManager

Добавлен opt-in set_file_resource_completion_channel(sender, max_size) для
обычных inbound Resources. FileResourceCompletion передаёт открытый std File
с offset 0, link_id/original hash, payload size и metadata. Byte completion
каналы на этом пути не вызываются; без opt-in работают как раньше. Request и
response Resources сохраняют прежнюю обработку, а не перенаправляются в файл.

Сегмент после проверки hash передаётся фоновой записи; proof отправляется из
on_tick только после write/flush. Последний сегмент также требует успешной
доставки результата в completion channel: full/closed → RCL, без proof.
Coordinator остаётся пустым cancellation anchor, прежние сегменты в RAM не
сохраняются. Проверяются размер advertisement/decoded bytes, общий d/count и
порядок сегментов. Приёмов максимум 32 на manager, blocking записей максимум
32 на процесс. Ожидающие записи отменяются при освобождении состояния;
начатая OS запись завершается перед закрытием handle.

Закрытие Link удаляет файловое состояние сразу, отмена Resource — на следующем
tick. После proof ожидание следующего advertisement ограничено 120 секундами;
во время приёма сегмента действует прежний watchdog. Утилиты не затрагивались.

Существующая короткая split fixture используется для byte и file путей:
реальные parts/proofs, точный файл, отсутствие byte callbacks в file mode,
отсутствие преждевременного proof и очистка состояния после завершения.
`cargo test -p rns-runtime --lib test_split_resource_inbound --offline --quiet`:
2 passed, 0.02s. Workspace all-targets check успешен; прежние warnings
database_path/tracing prelude сохраняются. Новых test-only файлов нет,
длительные тесты не запускались. Этап 6 открыт до итоговой сверки покрытия;
request/response APIs по-прежнему возвращают bytes. Версия 1.0.1 сохранена.

### Этап 6: итоговая сверка и остаток файловых ответов

Сверены пункты плана с локальным Python ea98db4f (reference чист) и кодом:

| Пункт | Результат сверки |
|---|---|
| medium_path_timeout | Расчёт actor, runtime API, RPC и shared forwarding перенесены; first_hop_timeout сохранён. |
| Применение таймаутов CLI | Сделано ранее, explicit timeout сохраняет приоритет; в этой сверке утилиты не менялись. |
| Destination.max_request_size | API и проверки packet/Resource запросов до приёма присутствуют. |
| max_response_size | Проверяются повторённый total d и собранный размер с metadata; прежняя Rust-семантика packet bytes сохранена. |
| Channel/Buffer | Согласованный Link MDU применяется в runtime и разбиении Buffer, не только в getter. |
| Resource | RCL/ICL, индексы, watchdog/proof retry и stream/file API перенесены; raw file response остаётся функциональным пробелом ниже. |
| Link/ratchet/blackhole | Inbound-based stale, keepalive при одностороннем трафике, retained cleanup и blackhole validation API присутствуют. |

Во время сверки исправлены ещё три места в LinkSession:
- обычный recv_resource теперь проверяет total/index до выделения coordinator,
  как файловый путь; превышение MAX_SEGMENTS отклоняется через RCL;
- wait_for_response больше не разбирает повторённый Python has_metadata flag
  как новый metadata prefix после первого сегмента;
- ICL по original hash завершает обычный приём и ожидание ответа также между
  сегментами, когда активный transfer предыдущего сегмента уже удалён.

Одна новая inline проверка передаёт два сегмента packed response с metadata и
реальными proofs, затем проверяет ранний отказ обычного recv_resource по cap.
Повторно запущена существующая короткая проверка response size/cancellation.
Команды response_split_metadata_flag и response_size_limit_uses_total:
по 1 passed, 0.01/0.03s. Workspace all-targets check успешен; прежние warnings
database_path/tracing prelude сохраняются. Новых test-only файлов нет.

Остаток по существу: Python Link.response_resource_concluded трактует Resource
с metadata как raw file response и использует request_id из advertisement;
Rust wait_for_response всё ещё требует packed [request_id, data]. Нужно
перенести этот путь, явно разрешив совместимость с существующими Rust metadata
responses; простое переключение всех metadata ответов на raw изменит их контракт.
Полная Python↔Rust матрица/длительные проверки отложены по указанию пользователя,
а не объявлены пройденными. Этап 6 пока открыт, версия 1.0.1.

### Этап 6: приём raw file Resource-ответов Python

Добавлены ResourceResponseMode::{Packed, PythonFile} и публичный
LinkSession::request_with_response_mode(path, data, deadline, max_response_bytes,
mode). PythonFile повторяет правило Python Link.response_resource_concluded:
если Resource содержит metadata, payload возвращается как raw file bytes,
request_id берётся из проверенного encrypted advertisement. Для Resource без
metadata и обычных packet responses остаётся декодирование envelope.

Старые request/request_with_metadata/request_with_metadata_limit сохраняют
Packed по умолчанию, включая metadata-bearing packed responses. Эвристика
«попробовать MessagePack, иначе файл» не применяется: файл сам может содержать
корректную [request_id, data], и его нельзя незаметно преобразовывать.
Сохранены проверки q, размеров, hash/proofs и многосегментная сборка.

Существующая короткая response_split_metadata_flag fixture теперь проходит
оба режима: одни и те же bytes с metadata дают decoded data в Packed и точные
bytes файла в PythonFile, включая случай файла, похожего на envelope.
1 passed, 0.02s. Workspace all-targets check успешен; прежние warnings
database_path/tracing prelude сохраняются. Новых test-only файлов нет,
длительные тесты/полная Python interop матрица не запускались.

Пробел приёма raw file response закрыт opt-in API. Границы: LinkResponse.data
остаётся Vec под лимитом, не открытым файлом; генерация файловых request replies
на сервере этим изменением не добавлялась. Утилиты не менялись, версия 1.0.1.

### Этап 7: gravity в локальном и удалённом rnstatus

По согласованному плану начат этап 7, первый пункт диагностики. Gravity уже
передавался actor → runtime/RPC (включая signed i64 и fallback 0 для старого
peer), но отсутствовал в rnstatus. Добавлен в human и JSON local/remote output.
Поддержаны --sort gravity и alias g, по убыванию, --reverse по возрастанию,
как в Python 1.5.2. Сравнение gravity целочисленное, без потери точности f64.
Remote parser сохраняет отрицательные значения и использует 0 при отсутствии
поля. Существующие JSON поля и остальные ключи сортировки сохранены.

Короткий inline test проверяет aliases, отрицательное значение, соседние i64
у верхнего предела, reverse и старый remote response без gravity:
`cargo test -p rns-tools --bin rnstatus-rs gravity_sort --offline --quiet`:
1 passed, 0.00s. Проверка rnstatus и workspace all-targets успешна;
прежние warnings database_path/tracing prelude сохраняются. Reference ea98db4f
чист. Новых test-only файлов нет, длительные проверки не запускались.

Первый пункт этапа 7 ещё не закрыт: далее входящие очереди/потери, нарушения
протокола, PPS, TX-буферы и подробная статистика трафика. Существующую поддержку
blocked Backbone IP нужно сохранить и сверить, не реализовывать повторно.

### Этап 7: завершён блок rnstatus одним набором изменений

По указанию пользователя промежуточный gravity-коммит не выполнялся.
Весь блок rnstatus подготовлен вместе, включая runtime/RPC и remote management:

- Локальный RPC сохраняет raw authenticated snapshot для дополнительных полей,
  старые typed decoders сохраняют InterfaceStats. Аутентификация и лимиты frame
  не изменены. Unix/TCP пути поддержаны.
- Очереди: высоты/потери/pressure доходят до local и remote --queues/JSON.
  Remote management использует те же wire queue fields, что local RPC.
- Protocol/IFAC violations, packet filter hits и arx/atx/prx/ptx bytes/counts/
  rates передаются end-to-end. -A/-P показывают подробные счётчики.
- TX diagnostics: encoded buffered/dropped bytes и gate берутся из существующего
  ManagedTx accounting. Plain TX не притворяется побайтовым: эти поля null,
  число queued frames выводится отдельно. Сохранены tx_drops/blocked IP list.
- PPS: actor arrivals на Normal interfaces до validation (без повторного ingress
  release), TX successful queue admissions, не подтверждённые физические кадры.
  Скорость вычисляется по monotonic elapsed, без деления на условную секунду.
  RX/TX bitrate интерфейсов теперь вычисляется по driver byte-counter deltas.
- SharedServer/LocalClient/SharedInstancePeer исключены из внешних totals и
  суммарного control traffic; per-interface counters и фильтры остаются отдельно.
- link_count означает все записи link table, active_link_count — validated;
  добавлены отдельные actor/RPC запросы, исправлена прежняя подпись Active links.
- Дополнены сортировки Python (anns, counts, violations/filter, TX drops/buffer),
  gravity/g и reverse. Старые JSON поля/типы сохранены, новые поля additive.
- --profiling/-z запрашивает и показывает третий элемент remote Python status.
  Rust-граница — существующие tracing spans, не Python decorator profiler;
  отсутствие profiler данных обозначено явно, JSON profiling_supported=false.
  -q сохраняет Rust quiet, очереди доступны через --queues.

Проверки короткие: rnstatus binary unit tests — 11 passed; interface_stats
runtime tests — 4 passed (0.34s); remote_management tests — 13 passed (0.00s);
packet/byte-rate sampler и TX reservation diagnostics — по 1 passed (0.00s).
Добавлена snapshot-проверка PPS/TX wire полей, исключения shared totals,
сохранения legacy decoder и active_link_count request. Новых test-only файлов
нет. Workspace all-targets check/fmt/diff check успешны; прежние warnings
database_path/tracing prelude остаются. Длительные тесты и полная живая
Python↔Rust матрица не запускались; аппаратные измерения не заявляются.

Блок rnstatus закрыт с описанными границами измерений/profiling. Этап 7 целиком
не завершён: далее отдельная сверка rnsh и ограничений rnpath remote/rncp
--phy-rates по плану. Версия 1.0.1 сохранена.

### Этап 7: блок rnsh завершён целиком

Эталон `ea98db4f` остаётся чистым. Сверены актуальные args/rnsh/listener/
initiator/session/protocol и diff Python `1.3.8..ea98db4f`, включая 1.3.9
authorization fix и последующие исправления. Вывод не основан только на
формулировке changelog: Rust уже имел identity gate, authorized/Closed state,
копирование default argv и защиту от повторного изменения состояния identity.

Перенесено/исправлено одним блоком:

- CLI: `--config` для rnsh, `--rnsconfig` для RNS; выбор существующего
  `~/.config/rnsh` либо `~/.rnsh`; `identity`, `identity.default` и suffix
  сервиса; явный `-i` сохранён, Unicode service name очищается от разделителей.
  `-p` использует те же пути, не создаёт Reticulum storage/instance.
- Allow-list берётся только из выбранного rnsh каталога. Файл больше не
  копируется в постоянные `-a` grants, а LinkManager gate и listener используют
  общий загрузчик. Добавления/удаления учитываются при новой идентификации,
  недоступный файл не даёт разрешений; активные сессии не отзываются.
- Fatal protocol/command errors закрывают состояние до отправки ответа;
  последующие VersionInfo/Execute не могут его оживить. Malformed/unsupported
  сообщения и ошибки процесса изолированы в одной сессии, не останавливают
  весь listener. Порядок established → identified → channel сохранён между
  раздельными очередями; shutdown имеет приоритет.
- `--timeout` принимает конечные положительные дробные секунды без прежнего
  clamp до 1s; некорректные/непредставимые значения отклоняются без panic.
  Общий лимит жизни shell удалён: handshake/операции ограничены, а idle Link
  обслуживает watchdog/keepalive/stale detection. Клиент явно реагирует на
  shutdown. TRACE доступен через `-vvv`.
- README/CONFIG описывают несовместимую смену смысла `--config`, перенос старых
  identity вручную или через `-i`, неизменный raw-key формат и отсутствие
  автоматической замены повреждённого identity.

Короткие проверки: `cargo test -p rns-tools --bin rnsh-rs --offline --quiet`
— 16 passed (0.01s); `cargo test -p rns-runtime --lib rnsh::tests --offline
--quiet` — 9 passed (0.31s), включая denied/unidentified и разрешённое
выполнение локальной команды, fatal→Execute, обновление файла разрешений,
idle за пределами operation timeout, keepalive и PTY cleanup. Новые проверки
встроены в production-модули, новых test-only файлов нет.
`cargo check --workspace --all-targets --offline --quiet`, fmt check и
`git diff --check` успешны; прежние warnings database_path/tracing prelude
не менялись. Длительные тесты не запускались.

Границы: Rust tracing/stderr вместо Python logfile и числовой шкалы;
консервативные 240-byte stream chunks вместо адаптивного выбора compression
chunk (новое Python HEADER_LEN не требует изменения совместимого Rust wire
формата); Rust shell/exit-code defaults сохранены и описаны. Удаление Python
imports и загрузка compiled modules неприменимы. Полный паритет старого
интерактивного UX Python, длительные сессии и живая Python↔Rust матрица здесь
не заявляются. Блок обновления rnsh закрыт; далее по этапу 7 остаются
`rnpath` remote mode и `rncp --phy-rates`. Версия остаётся 1.0.1.

### Этап 7: rnpath remote/rncp --phy-rates; функциональный этап закрыт

Эталон Python `ea98db4f` проверен, рабочее дерево не изменено. Проверены
`rnpath.py` и `Transport.remote_path_handler`: `/path` поддерживает только
`table`/`rates`, фильтр destination и max_hops для таблицы. Rust это уже
реализует. Private blackhole table, mutations и active remote path request
не поддерживаются и в Python 1.5.2; новые wire endpoints не изобретались.
`-p` использует отдельный существующий published `/list` endpoint. CLI/README
теперь явно отличают эту границу протокола от недоделанного Rust endpoint.

`rncp -P/--phy-rates` реализован для send/fetch вместо прежнего отказа CLI:

- Эталон расчёта — `rncp.sender_progress`, а не счётчики интерфейсов:
  segment progress × Resource transfer size после metadata/compression/encryption.
  Это оценка закодированных Resource bytes, без packet/IFAC/HDLC overhead,
  повторных передач или airtime. В выводе явно указано `physical/resource estimate`.
- Runtime выдаёт optional latest-value `RncpTransferProgress` через новые
  `rncp_send_reader_with_stats`/`rncp_fetch_file_with_stats`. Существующие API
  и request structs сохранены; observer не блокирует передачу.
- Байты суммируются между сегментами, повторный прогресс не даёт двойного
  учёта и не сбрасывает счётчик. CLI использует окно до 32 отсчётов и перевод
  bytes/s → bits/s. При fetch доля текущего сегмента учитывает его индекс,
  вместо повторения локальных 0–100% для каждого сегмента.
- `--silent` сохраняет отсутствие progress output; listener не меняется.
  README/CONFIG описывают расчёт и границы, аппаратные PHY измерения не заявлены.

Короткие проверки: `cargo test -p rns-runtime --lib rncp::tests --offline
--quiet` — 8 passed (0.01s), включая существующий ADV→REQ→parts→proof сценарий
с проверкой итоговой статистики и compressed-size/multisegment/repeat учёт.
`cargo test -p rns-tools --bin rncp-rs --bin rnpath-rs --offline --quiet` —
1 + 2 passed (0.00s): rate units/window/CLI и локальный отказ неподдерживаемых
remote операций без запуска runtime. Новых test-only файлов нет; длительная
или аппаратная/внешняя проверка не запускалась.
`cargo check --workspace --all-targets --offline --quiet`, fmt check и
`git diff --check` успешны. Старые предупреждения database_path и tracing
prelude остались без изменений.

Этап 7 функционально закрыт с документированными границами rnstatus profiling,
rnsh и remote management. Дальше — итоговая интеграционная проверка и сверка
общей документации по плану, а не новые функции этапа 7. Версия 1.0.1
не менялась: полный переход/заявление совместимости 1.5.2 ещё не завершён.

### Итоговая сверка: Web API units, discovery stamp cache, feature builds

Сводка в начале журнала актуализирована отдельно от исторической матрицы.
README теперь явно отличает реализованные этапы от незавершённого релиза;
совместимость и package version не повышались.

Найдены и исправлены два функциональных пропуска:

- После включения общего traffic sampler actor/RPC отдаёт `rxs`/`txs` в
  бит/с, тогда как Web API `rx_rate`/`tx_rate` и UI formatRate используют байт/с.
  `merge_iface_json` теперь делит на 8, сохраняя дробную часть (12 bit/s →
  1.5 B/s). В существующей actor→API проверке добавлены значения 8000 и 12.
  Устаревшие CONFIG утверждения о pending PPS/global totals уточнены.
- Исходные строки 1.4.0 про discovery caches не были закрыты реализацией
  publication в этапе 1. Теперь `receiver::spawn` владеет двумя FIFO caches
  по 2048 записей для valid/invalid stamps. Повторный body+stamp не запускает
  stamper вновь; source allow-list проверяется до дорогой работы, а decode,
  hops/last_heard/store/observer продолжают обновляться на cache hit.
  Кеш не содержит исходных payloads, только digest/value, и живёт в одной
  последовательной task. Config struct и прямой uncached process_event API
  сохранены. Отличие от Python: digest считается после расшифровки, поэтому
  decrypt не пропускается на encrypted hits; подпись announce и source policy
  не обходятся. Новые stamps по прежнему info проверяются отдельно.

Убраны два feature-зависимых предупреждения: database_path вычисляется только
при sqlite, tracing prelude импортируется только при api. Это не меняет
runtime-поведение включённых возможностей.

Короткие результаты на Linux, без длительной нагрузки:

- `cargo test -p rns-runtime --features api --lib config::tests --offline
  --quiet`: 20 passed, включая YAML defaults/example/roundtrip (0.01s).
- `cargo test -p rns-runtime --features api --lib api_server::tests --offline
  --quiet`: 22 passed после unit conversion (0.01s).
- `cargo test -p rns-transport --lib discovery::receiver::tests --offline
  --quiet`: 14 passed (0.02s), включая реальную spawn task, повторные valid/
  invalid stamps, source policy, обновление hops и FIFO eviction обоих caches.
- `cargo test -p rns-runtime --test inbound_queue_rpc_python --offline --quiet
  -- --ignored`: 1 passed (0.15s). Используется существующий read-only Python
  codec из локального эталона, без запуска демона, сетевых портов и новых файлов.
- `node --test crates/rns-runtime/web/app.test.js`: passed (около 0.13s).
- Проверки default workspace all-targets, runtime `api,serial,rnode-tcp,
  sqlite-bundled` all-targets и `--no-default-features --features client`
  прошли отдельно после исправлений, без прежних warnings; fmt/diff checks
  чистые. Это не утверждение о проверке BLE/всех ОС/all-features.

Дальше остаётся завершить построчную классификацию исходной матрицы (включая
историческую discovery/blackhole cleanup и применимость остальных performance
изменений), затем собрать окончательный перечень отложенных интеграционных
проверок. Полный workspace test, многодемонные Python↔Rust сценарии, длительный
soak и аппаратные проверки в этом проходе не запускались. Новых test-only
файлов нет; имеющиеся используются без изменения.

### Итоговая сверка: historical discovery blackholes и logging 1.5.2

Проверен `Discovery.list_discovered_interfaces` эталона: удаляются записи с
blackholed network_id **или** transport_id, не только новые announces на входе.
В Rust этого не было: исторический store проверял age/source-list, а startup
autoconnect читал его напрямую. Пропуск исправлен:

- Новый additive `DiscoveryStore::list_with_blackholes` удаляет соответствующие
  записи с диска; старый policy-free `list` сохранён для standalone callers.
- Runtime `discovered_interfaces` получает авторитетный GetBlackholedIdentities
  через query_control (в full client mode — RPC общего демона), фильтрует
  истёкшие TTL и передаёт HashSet в store. Весь запрос ограничен пятью секундами;
  при недоступности snapshot возвращается пустой список, файлы не удаляются.
- Startup autoconnect использует этот список; queued observer records проверяются
  заново перед подключением, чтобы учитывать изменение blackhole после announce.
  Не добавлено отключение уже работающих интерфейсов. Unblackhole не воскрешает
  удалённую историю, но новое допустимое объявление сохраняется обычным путём.
- Python 60s cache списка blackhole не перенесён: Rust получает свежий snapshot
  и проверяет membership через HashSet. Специфичная производительность Android
  не заявляется проверенной; client-only runtime, как прежде, не ведёт discovery.

Дополнительно закрыта строка LOG_PATHING/LOG_EXTREME: YAML validator и runtime
принимают уровень 8, Web UI различает 7 Pathing / 8 Extreme. Старое поведение
0–7 не ломается; daemon отображает оба подробных уровня в TRACE. Отдельная
Python-шкала сообщений и LOG_NONE=-1 не имитируются; граница описана в CONFIG.

Короткие проверки: storage module — 10 passed (0.00s), включая оба blocked IDs
и отсутствие восстановления удалённых записей при пустом следующем snapshot;
`discovery_history_uses` runtime — 1 passed (0.00s), с реальным actor, TTL,
повторной проверкой observer и сохранением файлов при недоступной политике;
`logging_pathing` — 1 passed (0.00s), YAML roundtrip и нормализация 7/8, отказ 9.
Web UI Node test passed (около 0.09s). Использованы inline проверки в существующих
production-модулях; новых test-only файлов и внешних соединений нет.
Default workspace all-targets, runtime api/serial/rnode-tcp/sqlite-bundled
all-targets и client-only checks прошли; fmt/diff checks чистые.

Матрица дополнена результатами, но общий релиз ещё не объявлен завершённым.
На момент этого блока оставались строки performance/signature cache, rnodeconf
WiFi summary и прочие не классифицированные minor fixes. Продолжение ниже;
длительная валидация по-прежнему отложена. Версия остаётся 1.0.1.

## Финальная сверка: announce signature reuse и rnodeconf WiFi summary

Сверено с неизменённым Python HEAD `ea98db4f` (1.5.2).

- `Identity.validate_announce` сохраняет `packet.announce_signature_validated`:
  оптимизация действует внутри одного Packet, а не между разными announces.
  Rust уже передаёт `VerifiedAnnounce` в owned `PreparedInbound` от admission
  к dispatch. Подпись там повторно не проверяется, но актуальная blackhole
  policy и destination binding проверяются. Глобальный кеш не добавлен;
  существующая граница описана комментарием в `actor/inbound.rs`.
- Upstream WiFi fix в `rnodeconf.py` заменяет второй `if` на `elif`, исключая
  одновременный вывод Station и Disabled. Rust EEPROM summary раньше вообще
  не показывал WiFi. Теперь `rnodeconf-rs --info` показывает ровно одно состояние
  Station/AP/Disabled, канал для включённого WiFi и состояние Bluetooth.
  Значение канала вне 1–14 нормализуется в 1, как в Python. Отсутствующие поля
  короткого EEPROM не индексируются: WiFi/канал обозначаются Unknown.
- Это перенос исправленной семантики режима в существующий Rust summary,
  не полная совместимость Python `--config`: SSID/PSK/IP из отдельного config
  sector в этот summary не добавлены. Flashing backlog не затрагивался.

Короткая проверка `cargo test -p rns-tools --bin rnodeconf-rs
eeprom_summary_reports_identity_and_radio_config --offline --quiet`:
1 passed (0.00s), включая четыре режима, границы канала и короткие образы.
Проверка расширена в существующем production-модуле; новых test-only файлов нет.
`cargo fmt --all` и `git diff --check` прошли.

Далее: оставшиеся неклассифицированные minor/performance fixes в финальной
матрице. Общая готовность релиза 1.5.2 пока не заявляется, версия не изменена.

## Финальная сверка: speedtest и delivery-proof lifecycle

Upstream `Examples/Speedtest.py` меняет условие Active на «не Closed».
Rust `examples/src/bin/speedtest.rs` уже не останавливает отправку на Stale,
но использует окно tracked packets вместо Python untracked flood. В общем
`LinkSession::recv_delivery_proof` обнаружены и исправлены два пробела:

- Событие закрытия своего Link и аутентифицированный remote teardown теперь
  немедленно возвращают ошибку закрытия, а не игнорируются до deadline.
  Чужие Link IDs и неподтверждённые teardown не завершают ожидание.
- Валидные proofs и успешно расшифрованные встречные application packets
  обновляют inbound activity и RX bytes. Proof восстанавливает Stale → Active;
  application packet также обновляет data activity и сохраняется для recv.
  Невалидный proof не обновляет эти счётчики. Общий deadline не продлевается.

RSSI/SNR сверены отдельно: Rust сохраняет значения прямо в owned InboundPacket
до enqueue, actor кеширует их по packet hash, RPC отдаёт GetPacketRssi/Snr.
Удаление Python сброса interface fields решает потерю при отложенном чтении
mutable interface; Rust повторно интерфейс для этих метрик не читает.
Сброс per-frame RSSI/SNR сохранён, аппаратная валидация не заявляется.

Короткий inline test `delivery_proof_recovers_stale_link_and_observes_close`:
1 passed (0.03s), с настоящими Link ключами и authenticated teardown.
Проверки сборки speedtest и client-only runtime, fmt/diff checks прошли.
Новых test-only файлов и длительных тестов нет. Receipt callbacks, остальные
minor/performance строки и итоговое объявление версии остаются отдельной работой.

## Финальная сверка: receipt notifications и очистка корреляций

Python `Transport.py` в 1.5.2 собирает candidate receipts под receipts_lock,
а proof validation с delivery callback выполняет после освобождения lock.
Штатный Rust `RegisterReceipt` принимает только hashes, msg_id и timeout;
actor создаёт receipt без callbacks, а `process_proof` отправляет DeliveryProof
через `try_send` в destination channels. Application/probe ждут уведомление
в собственной async-задаче. Это не требует переноса Python mutex или потоков.
Контракт пояснён у RegisterReceipt. Низкоуровневые callbacks PacketReceipt
остаются синхронными: вручную установленные через публичное состояние actor
блокирующие callbacks не входят в заявленную гарантию штатного runtime API.

При проверке обнаружена утечка `receipt_msg_ids`: maintenance удалял expired,
concluded и вытесненные по MAX_RECEIPTS receipts, но сохранял их correlation IDs.
Теперь после очистки и ограничения receipt table связанные msg_ids удаляются
в том же проходе обслуживания. IDs действующих receipts сохраняются; успешная
доставка, как прежде, удаляет обе записи в inbound path.

Короткий inline test `receipt_maintenance_removes_expired_and_evicted_correlations`:
1 passed (0.01s). Он регистрирует MAX_RECEIPTS+2 записей через настоящий handler,
проверяет timeout первой, вытеснение второй и сохранение корреляций остальных.
Новых test-only файлов нет, длительные тесты не запускались. Общая готовность
1.5.2 не объявляется; остальные minor/performance пункты ещё требуют сверки.

## Финальная сверка: BLE lifecycle и minor interface fixes

Эталон по-прежнему Python `ea98db4f`, рабочее дерево эталона чистое.
В этом блоке новых функциональных расхождений не найдено: исправления уже
покрыты существующим Rust-кодом. Матрица уточнена без повторной реализации.

- Android Python исправляет `self.ble_device == None` на присваивание при
  отсутствии найденных устройств. Rust `resolve_ble_target` возвращает Err,
  если список не содержит подходящего кандидата; `connect_rnode` каждый раз
  вызывает resolver. Reconnect loop получает новый локальный conn, а после
  разрыва отменяет и дожидается writer/forwarder перед следующей попыткой.
  Rust native Android branch использует новое TCP bridge connection;
  выбор BLE device принадлежит платформенному коду. Это не доказательство
  свежести кеша btleplug/ОС или работоспособности конкретного BLE оборудования.
- Python guards `if not data: return` уже имеют аналоги: UDP отбрасывает n=0;
  Serial/Pipe/KISS/RNodeMulti проверяют frame.is_empty; общий RNode response
  parser не выпускает пустой CMD_DATA. Эти ветки не требуют новых изменений.
- TCP HDLC reader уже применяет fixed MTU с IFAC allowance и минимальную длину;
  изменение Python check_frame_len покрыто существующим reader и его проверкой.
- I2P empty probes FLAG FLAG не передаются transport ни из начального SAM
  буфера, ни из последующих чтений. Они обновляют receive liveness. Reader,
  writer и watchdog живут в одном scoped connection; это покрывает отдельное
  исправление 1.5.2, уже реализованное на этапе 5.

Остальные packet/link minor fixes, persistence/known destinations cleanup и
окончательная классификация performance остаются открытыми. Общая версия и
заявленная совместимость не повышаются этим документальным уточнением.

Переиспользованы существующие короткие проверки: `i2p::liveness_tests` —
6 passed (0.00s, in-memory/virtual time),
`tcp_readers_use_fixed_mtu_with_escape_and_ifac_allowance` — 1 passed (0.86s,
локальный loopback). Новые test-only файлы не создавались и старые не менялись.

## Финальная сверка: persistence и last-use cleanup

`Identity.clean_known_destinations` Python 1.5.2 сохраняет retained и pathed
записи. Для never-used действует UNUSED_DESTINATION_LINGER от announce;
для used — DESTINATION_TIMEOUT × 1.25 от последнего использования. Rust ошибочно
использовал max(last_used, timestamp), позволяя свежему announce продлить срок
хранения уже давно не используемой pathless записи.

Исправлены три реализации: legacy actor maintenance, MemoryTransportStorage
и SQLite CleanKnown. Строгое сравнение порога сохранено: ровно на границе запись
остаётся. Pins и path/reference guards не изменены. Opt-in requested-client
пятиминутная политика — отдельное Rust поведение и не затрагивалась.
После удаления записей legacy actor теперь выставляет state_dirty, чтобы
очистка попала в следующий сохраняемый snapshot, даже если нет нового трафика.

Сохранение сверено отдельно: routing_snapshot собирается из текущего состояния,
save не подмешивает старые known destinations с диска. Periodic fsync/write
вынесены на blocking pool; повторный save не запускается поверх незавершённого.
Shared-instance client не сохраняет сетевой cache. SQLite выполняет запросы
на выделенном storage worker и ограничивает CleanKnown порцией 128 записей.
Legacy cleanup всё ещё проходит in-memory map на actor: эквивалент Python
background sleep не добавлялся, latency на большом каталоге не измерена.

Добавлены короткие inline проверки свежего announce при устаревшем last_used,
строгой границы, retained и dirty/no-op поведения; backend-проверка выполняется
для memory и, при sqlite feature, настоящего SQLite во временном каталоге.
Новых test-only файлов нет. Общая готовность обновления пока не объявляется.

`cargo test -p rns-transport --lib cleanup_tests --features sqlite-bundled
--offline --quiet`: 9 passed (0.04s). Fmt и diff checks прошли.

## Финальная сверка: Packet.py и границы admission/send

В diff Python `Packet.py` 1.3.8 → 1.5.2 найдены два ещё не перенесённых условия:

- `Packet.send` отказывается от hops >= PATHFINDER_M (128). Rust теперь делает
  это в обоих пользовательских outbound путях: обычном и OutboundAttached.
  Отказ происходит до TX accounting, dedup, receipts и отправки интерфейсу.
  Значение 127 разрешено; ingress hop adjustment и transit forwarding не менялись.
- `Packet.unpack` отказывается от нулевого payload. Rust проверяет это после
  разбора заголовка на inbound admission, до очереди/dedup и обработки типа.
  Отказ учитывается как protocol violation, в том числе для Header2. Низкоуровневый
  PacketHeader::unpack продолжает поддерживать отдельные заголовки для tooling.

Прочие изменения Packet.py классифицированы: fixed-size поля transport/destination
уже защищены bounds в header codec; traffic class и validated announce передаются
через PreparedInbound; hash/truncated hash helpers существуют; физические метрики
идут через packet metrics/RPC вместо Python singleton lookup. Python log formatting
и оптимизация доступа к атрибутам отдельной Rust реализации не требуют.

Короткая inline проверка `packet_boundaries_reject_empty_payload_and_exhausted_outbound_hops`
прошла (1 test, 0.00s): ordinary/attached sends, hops 127/128/255, Header1/Header2
и четыре packet types с пустым payload. Новых test-only файлов нет.
Оставшиеся Link/RequestReceipt minor fixes ещё требуют сверки; готовность всего
обновления и повышение версии этим блоком не заявляются.

Существующие `outbound` checks: 13 passed (0.01s). Fmt/diff checks чистые.

## Финальная сверка: RequestReceipt и завершение response wait

Python 1.5.2 добавляет response_rejected для уже доставленного запроса,
ответ на который превышает max_response_size. При сопоставлении обнаружены
и исправлены связанные расхождения Rust:

- `Link::handle_response_plaintext` удалял все pending entries кроме Sent,
  включая другие ACKed/Receiving requests. Теперь удаляется только receipt
  с совпадающим request_id; неизвестный/дублированный ответ не чистит остальные.
- `RequestReceipt::fail` допускает отказ после ACK-only Delivered, но не после
  Delivered с готовым ответом. Повторный отказ не вызывает callback повторно;
  ответ после Failed не меняет итог. Receiving может принять завершённый ответ.
- Добавлен additive `Link::decode_response_plaintext` без изменения receipts.
  Runtime проверяет ID и ограничения ответа до принятия, а не после вызова
  handle_response, уже завершившего receipt. Старые handle_response API сохранены.
- На всех обычных выходах из `wait_for_response` оставшийся receipt именно этого
  запроса удаляется и завершается успехом/отказом: timeout, link close, rejection,
  malformed response, cancellation Resource и PythonFile success. Остальные
  запросы не затрагиваются; ошибка вызывает failure callback при наличии.

Граница блока: отмена самой async future извне и ошибки отправки до входа в
wait_for_response не проверялись этим изменением. Остальные Link receive/error
paths также остаются в финальной сверке; полная готовность релиза не заявляется.

Короткие проверки: rns-link `request` — 19 passed; runtime `response_` —
13 passed (0.04s), включая новую проверку ACK-only failure при timeout/close
и сохранения чужого pending receipt. Inline проверки в production-модулях;
новых test-only файлов нет, длительные тесты не запускались.

## Финальная сверка: ранний отказ и отмена LinkSession request

`LinkSession::request_with_response_mode` теперь владеет PendingSessionRequest
на всём промежутке от prepare_request до выхода из response wait. Drop guard
удаляет оставшийся receipt своего запроса и вызывает fail при ранней ошибке
или отмене future вызывающим кодом. После штатного завершения response wait
receipt уже удалён, поэтому повторного завершения нет.

Guard обновляет свой ID одновременно с переходом на packet-hash request ID,
до await отправки. Для Resource request сохраняется исходный request ID.
Очистка не закрывает Link, не стирает остальные pending receipts и не запускает
фоновую задачу. Ошибки до создания receipt не требуют удаления.

Это локальная cancellation safety: уже поставленный transport packet невозможно
отозвать, а при внешней отмене Resource future не гарантируется отправка remote
RCL из синхронного Drop. Удалённая сторона использует штатные протокольные timeout;
гарантия данного блока — отсутствие локального зависшего receipt и возможность
следующего запроса на живом Link, а не отзыв уже доставленной удалённой операции.

Короткая inline проверка охватывает закрытый transport, отмену при заполненном
transport channel, отмену ожидания ответа, отмену Resource request и отказ для
неустановленного Link. После отмены packet send/response wait следующий настоящий
запрос на том же Link получает зашифрованный ответ; чужой receipt сохраняется.
Новых test-only файлов нет. Остальные Link receive/error paths остаются в сверке.

`session_request_cleans_receipt_on_send_error_and_cancellation`: 1 passed (0.08s);
существующие runtime `response_`: 13 passed (0.04s). Default runtime и client-only
cargo checks, fmt/diff checks прошли. Длительные тесты не запускались.

## Финальная сверка: закрытие Link во время receive/wait

Внутренний DestinationEvent::LinkClosed ранее часто только возвращал ошибку,
оставляя локальный Link Active с доступными session keys. Теперь собственное
событие закрытия вызывает mark_closed перед возвратом: packet recv, delivery
proof wait, handshake proof wait, response wait, Resource receive и Resource send.
Чужие Link IDs по-прежнему игнорируются. Ключи очищаются штатным Link::close;
последующая send через такую сессию завершается ошибкой.

При расширении короткой проверки на Resource обнаружен ещё один пропуск:
ветки Resource receive/send прерывались по одному контексту LinkClose, без
проверки payload и без перехода в Closed. Теперь они используют receive_teardown,
как остальные authenticated-close пути: поддельное закрытие игнорируется,
проверенное завершает Link. Это не меняет формат teardown и не добавляет
доверия к произвольным сетевым пакетам; внутреннее событие остаётся уведомлением
от runtime manager, а сетевой teardown должен пройти проверку ключом Link.

Проверка расширена в существующем production-модуле: четыре receive/send режима
× internal/remote close, поддельный teardown перед настоящим, Closed и отказ
последующей send. Первый запуск выявил Resource-пропуск, исправление внесено
в тот же блок. Новых test-only файлов нет. Keepalive admission и остальные
ещё не классифицированные Link error paths остаются в финальной сверке.

Повторный targeted test: 1 passed (0.11s); runtime `response_`: 13 passed
(0.04s). Fmt/diff checks прошли; длительные тесты не запускались.

## Финальная сверка: keepalive admission и last_outbound

Python Link.receive игнорирует keepalive request на initiator до обновления
activity; responder отвечает только на точный FF и после keepalive interval
с последней отправки. В Rust это исправлено через общий Link::receive_keepalive:
FF/FE должны быть ровно одним байтом, Link — Active/Stale; FF на initiator
не меняет state, last_inbound и RX totals. Принятый keepalive восстанавливает
Stale и учитывается в RX, но не обновляет application last_data. Closed не оживает.
Строгий отказ malformed keepalive до activity — Rust validation boundary:
Python также не отвечает на FF с хвостом, но общий receive может учитывать
неизвестное тело как inbound. Поддержка валидных wire frames сохранена.

Общая проверка подключена к LinkManager и LinkSession packet/proof/Channel/
Resource receive/send/response wait. Keepalive должен быть DATA packet.
Responder учитывает исходящий ответ только после успешного enqueue transport.

Короткая проверка выявила старое расхождение record_tx_keepalive: оно не
обновляло last_outbound, из-за чего повторные FF вызывали ответы без паузы.
Теперь last_outbound обновляется как Python had_outbound(is_keepalive=True),
без изменения inbound/last_data. Stale detection уже inbound-based и от
собственных исходящих probes не продлевается. Старое inline ожидание
«last_outbound не меняется» заменено проверкой правильных activity timestamps.

Новых test-only файлов нет; использованы короткие inline проверки точного
кадра, ролей, Closed/Stale, reply interval и существующие keepalive/response tests.
Остальные Link receive/error paths и итоговая классификация релиза остаются
отдельной работой; версия этим блоком не повышается.

Итог: rns-link `keepalive` — 11 passed (0.07s), runtime `keepalive` —
3 passed (0.01s), runtime `response_` — 13 passed (0.04s).
Default runtime check, fmt/diff checks прошли; длительных тестов нет.

## Финальная сверка: ошибки Resource advertisement в клиентских путях

Python Link.__receive и Rust LinkManager закрывают Link при успешно
расшифрованном, но неразбираемом Resource advertisement. Клиентские
recv_resource/response wait раньше только возвращали ошибку, сохраняя Link.
Общий receive_resource_advertisement теперь обеспечивает одинаковое поведение:

- Ошибка decrypt/HMAC не завершает операцию и не закрывает Link: кадр
  игнорируется, ожидание продолжается в пределах прежнего deadline.
- Ошибка ResourceAdvertisement::unpack после успешного decrypt переводит Link
  в Closed и очищает ключи; возвращается UnexpectedResponse. Предварительно
  строится authenticated teardown и передаётся transport через try_send.
  При полной/закрытой очереди отправка best-effort, локальная очистка не ждёт.
- Проверки размеров, request ID и согласованности сегментов остаются отдельными
  ветками. Валидный ADV, отвергнутый прикладным лимитом, не приравнивается
  к неразбираемому advertisement и не закрывает Link этим helper.

Inline test прогоняет оба клиентских пути: corrupted ciphertext, затем
authenticated invalid MessagePack; проверяет Closed, отказ следующей send,
один outbound teardown и его проверку ключами реального peer Link.
Результат: targeted — 1 passed (0.02s), runtime response_ — 13 passed (0.04s).
Новых test-only файлов и длительных тестов нет. Далее остаётся обработка
ошибок decrypt/parse обычных Link DATA/Response сообщений; общая готовность
релиза пока не заявляется.

## Финальная сверка: повреждённые Link DATA и packet Response

Python Link.receive не прекращает работу после ошибки приёма, а обычная DATA
доставляется приложению только при успешной расшифровке. В клиентской сессии
ошибка decrypt DATA раньше возвращалась наружу и обрывала текущую операцию.
Теперь recv, recv_delivery_proof и recv_resource игнорируют такую DATA и
продолжают ожидание. Повреждённому сообщению не отправляется delivery proof;
следующее корректное сообщение доставляется/сохраняется обычным путём.

Packet Response wait также продолжает ожидание после ошибки decrypt или
decode msgpack envelope. Неизвестный request ID остаётся игнорируемым.
Лимит размера применяется к корректному ответу своего запроса и по-прежнему
вызывает явный отказ; общий timeout оборачивает весь wait, а не каждый кадр,
поэтому malformed traffic не продлевает срок операции. Ошибки локальной
отправки proof и подтверждённое закрытие Link не скрываются этим изменением.

Короткая inline проверка охватывает DATA recv, proof wait, Resource receive,
ответ после corrupted ciphertext/invalid MessagePack/чужого request ID и
timeout при отсутствии корректного ответа. Используются настоящие session
keys и полный handshake обеих сторон. Новых test-only файлов нет.

Граница блока: это packet DATA/Response, не изменение обработки собранного
Resource response или служебных ResourceReq/HMU. Эти оставшиеся Resource
receive/error ветки требуют отдельной сверки; обновление ещё не объявлено готовым.

Итог: targeted test — 1 passed (0.06s), существующие runtime response_ —
13 passed (0.04s). Client-only check и fmt/diff checks прошли.
Длительные тесты не запускались.

## Финальная сверка: ResourceReq/HMU и собранные response envelopes

Клиентские Resource receive/send/response wait приведены к уже существующему
LinkManager: ciphertext, не прошедший decrypt, игнорируется. Неразбираемый
HMU также пропускается без отмены операции. После успешного разбора сохраняются
проверки resource hash и действия state machine: пустое/недопустимое обновление
hashmap, адресованное активной передаче, может вызвать штатную отмену. Ошибка
локального шифрования или отправки собственного ответа по-прежнему возвращается.

Для Packed Resource response успешное получение bytes ещё не означает успешный
ответ запроса: ошибка decode response envelope теперь пропускается, receipt
остаётся ожидающим до корректного ответа или исходного deadline. Валидный
последующий packet Response принимается. PythonFile остаётся raw bytes и не
проходит такой разбор. Resource proof подтверждает получение Resource, а не
валидность вложенного response envelope — этот порядок сохранён.

Существующие inline tests расширены: corrupted ResourceReq перед authenticated
invalid request (проверяется сохранение ICL), corrupted/invalid HMU перед
следующими DATA/Response и split Resource с invalid MessagePack envelope,
за которым приходит корректный packet Response. Короткие результаты:
`malformed_` — 3 passed (0.06s),
`response_split_metadata_flag_and_receive_segment_cap` — 1 passed (0.03s).
Новых test-only файлов нет.

Строка Python watchdog exception reset классифицирована: Rust не имеет
watchdog_lock, который мог остаться установленным после return Err; исправленные
receive paths продолжают обработку там, где Python ловит ошибку и продолжает.
Это не обещание изоляции произвольного panic в пользовательском callback.
Далее — итоговая сверка ещё открытых строк матрицы; версия пока не повышается.

## Финальная сверка: ingress burst recovery и sustained hold

В `ingress.rs` оставалось поведение до Python commit `48388756`: для снятия
announce burst требовалось шесть отсчётов, хотя decay оставляет минимум два.
Теперь используется `IC_DEQUE_MIN_SAMPLE`, как в 1.5.2. Снятие состояния при
повторной проверке больше не требует новых announces для пополнения deque.
Это не новый фоновый сброс флага: как и в Python, состояние меняется при вызове
limiter; maintenance независимо выпускает held announces по частоте и deadline.

В том же блоке перенесены недостающие `burst_sustained` для announces и PR:
выход возможен только после hold как от активации, так и от последнего
наблюдаемого превышения порога. Для PR добавлен cooldown из трёх успешных
проверок снижения частоты; следующая проверка снимает burst, ещё следующая
разрешает PR. Повторное превышение обновляет sustained time и сбрасывает
cooldown, а проверка до истечения hold также сбрасывает cooldown, как upstream.

Проверки без sleep: decay до двух отсчётов без новых announces, продолженный
burst после старой активации, PR cooldown и его сброс, выпуск held announce
при ещё активном burst. `cargo test -p rns-transport --lib ingress::tests
--offline --quiet`: 22 passed (0.00s). Изменены только production module с
inline tests и этот журнал; отдельных test-only файлов нет.

## Финальная сверка: offline recursive PR и relay proof timeout

Исправление RNode из Python 1.4.2 (`4760103a`) состоит в исключении offline
интерфейсов из recursive discovery. В Rust проверка добавлена непосредственно
в `send_path_request` перед egress limiter, резервированием announce cap,
отправкой и обновлением path_requests. Это также покрывает прямые внутренние
вызовы этой функции. Отсутствующий online flag сохраняет прежний смысл
«не отмечен offline»; нерекурсивные запросы не меняются. Существующий inline
case проверяет отсутствие отправки/резерва для offline интерфейса с bitrate=0
и нормальный recursive discovery после online с рабочим bitrate.

При сверке bitrate найдено связанное расхождение relay Link Request:
`extra_link_proof_timeout` в 1.5.2 (`bde5611a`) добавляет время передачи одного
базового MTU только при ненулевой скорости. Rust использовал длину LR и
`bitrate.max(1)`. Теперь добавка равна `MTU * 8 / bitrate`, а для нулевой
скорости — нулю. Базовый per-hop timeout и negotiated link MTU не изменены.
Существующий forwarding case проверяет расчёт и реальную пересылку при
скоростях 0, 1, 1200 и 115200 бит/с.

Короткие проверки: `recursive_path_request_` — 5 passed, Link Request
forwarding — 1 passed; обе группы 0.00s. Новых test-only файлов нет.

## Финальная сверка: preemptive PR egress limiting

Перенесён оставшийся расчёт из Python `6f6751d6`: egress limiter учитывает
один потенциальный исходящий запрос сверх записанных отсчётов и использует
`EC_BURST_MIN_SAMPLES = 2` вместо старого порога 6. Проверка минимального
количества записанных отсчётов остаётся после decay, как в upstream.
Потенциальная отправка не добавляется в deque и не влияет на status frequency.
Ingress и held release вызывают общий расчёт без добавочного отсчёта.
Старая публичная константа IC_BURST_MIN_SAMPLES сохранена для совместимости
исходного кода, но текущие limiters её больше не используют.

Поздняя проверка непосредственно в `send_path_request` уже существовала;
между ней и отправкой нет await, состояние обрабатывает один actor. Нового
lock или повторного учёта отправок не требуется. Egress control остаётся
выключенным по умолчанию.

Существующий inline case теперь проверяет два записанных PR с частотой ниже
порога, при которых третий был бы выше порога, отсутствие изменения истории,
минимум отсчётов и decay. Actor case ограниченного интерфейса теперь также
использует только два исходящих отсчёта. Результаты: ingress — 22 passed,
recursive PR — 5 passed (обе группы 0.00s). Отдельных test-only файлов нет.
