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
