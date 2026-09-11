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
| 1.4.1 dynamic rebalance / gravity | Реализованы gravity selection, authenticated transit/local pending rebalance и привязка активного Link. Python↔Rust packet interop прошёл для 96/99-byte proofs; многодемонная сеть остаётся финальной интеграцией | 2 |
| 1.4.1 set_max_request_size | Отсутствует в Destination и runtime request admission | 6 |
| 1.4.1 max_response_size | Реализован `link_client.rs:request_with_metadata_limit`, включая Resource advertisement; сегменты проверить | 6, сохранить |
| 1.4.1 autoconnect mode/gravity/to_internal, default_gravity, interface gravity/to_internal | YAML/normalized/factory/registration, autoconnect defaults, child metadata, RPC и API/UI реализованы; I2P live inheritance не подтверждено | 2 |
| 1.4.1 rnstatus gravity display/sort | Gravity есть в runtime/local RPC/remote schema; вывод и сортировка CLI ещё не перенесены | 7 |
| 1.4.1 boundary→boundary/gateway PR | Реализовано; mode-матрицы с recursive/internal flags проходят в transport tests | 2 |
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
- [x] Этап 1: YAML/runtime publication, internal mode, location_cmd, API/UI.
- [x] Этап 2: gravity/internal, transit/local Link rebalance, interface binding; границы interop ниже.
- [x] Этап 3: Backbone fast-flapping, YAML/API/UI, RPC и локальный/удалённый статус.
- [ ] Этапы 4–7 и финальная интеграция.

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
