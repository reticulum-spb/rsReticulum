# Профилирование Rust-структур через allocative

Диагностическая Cargo feature `memory-profile` добавляет снимки дерева объектов
`TransportActor`, включая его SQLite-состояние. Она не заменяет аллокатор.
Без feature код профилирования и зависимость allocative не включаются.

## Сборка и запуск

```sh
cargo build --release -p rns-tools --bin rnsd-rs \
  --features sqlite-bundled,memory-profile

RNS_ALLOCATIVE_DIR=/path/to/profiles \
RNS_ALLOCATIVE_INTERVAL=300 \
  ./target/release/rnsd-rs --config /path/to/config-directory
```

Сохраняйте остальные features вашей обычной сборки: например, при системной
SQLite используйте `sqlite,memory-profile` вместо `sqlite-bundled,memory-profile`.
Для cross-сборки добавьте обычные `--target` и настройки toolchain.
Отладочные символы для дерева объектов не требуются.

- `RNS_ALLOCATIVE_DIR`: каталог вывода; без переменной снимки отключены даже
  в диагностической сборке. Каталог создаётся при записи.
- `RNS_ALLOCATIVE_INTERVAL`: интервал в секундах, по умолчанию 300;
  допустимо 1–86400. Некорректное значение отключает запись с WARN.
- Первый снимок — на первом тике actor, последующие — через указанный интервал.
  Это не гарантия точного расписания под нагрузкой. Настройки читаются при создании actor.

У процесса сервиса переменные должны быть заданы в его окружении, затем сервис
нужно перезапустить. Отдельный второй демон с рабочим конфигом запускать не нужно.
Выбирайте каталог на диске с достаточным местом: автоматической ротации нет.

## Чтение и сравнение

Для каждого снимка записываются `transport-PID-ACTOR-TIME-SEQUENCE.folded` и
одноимённый `.txt`. В folded — пути типов/полей и размеры в байтах, без содержимого
пакетов, ключей таблиц или строк приложения. В txt — время Unix в наносекундах,
оценка общего размера, время обхода, ограничения покрытия и предупреждения allocative.
Успешная запись и длительность обхода также выводятся на INFO.

Сравните ранний и поздний снимки одного actor из одного запуска:

```sh
python3 scripts/allocative_diff.py early.folded late.folded
python3 scripts/allocative_diff.py early.folded late.folded --depth 0 --top 50
```

По умолчанию группировка идёт по двум первым компонентам: тип actor и поле.
`--depth 0` показывает полные пути; другие значения позволяют раскрывать дерево
постепенно. Строки отсортированы по приросту байтов. Пример пути:
`TransportActor;recent_announces;…;RecentAnnounce;app_data;…`.
`unused_capacity` показывает сохранённый резерв после удаления элементов.
Folded-файл также можно визуализировать любым совместимым flamegraph viewer;
ширина здесь означает байты, а не время CPU.

## Что измеряется

Обход включает пути и random blobs, link/reverse/tunnel/announce/rate/blackhole
таблицы, оба поколения packet hashes, recent announces с app_data, discovery и
path-request state, metadata пакетов, receipts, интерфейсные ingress/announce
очереди, подготовленные inbound-очереди, persisted entries, SQLite raw cache,
staged writes и буфер admission. Для destination channels и path waiters
учитываются контейнеры и inline handles, но не внутренние allocations каналов.

Ограничения записываются также в каждый `.txt`:

- Это оценка охваченного дерева, не RSS и не полный heap процесса.
- `HashMap`/`HashSet`: резерв оценивается как `capacity * size_of(entry)`;
  внутренние control bytes, дополнительные buckets и allocator overhead не учтены.
- `Bytes`: `visible_payload_len` — доступная длина, не размер backing allocation.
  Общие slices могут учитываться повторно; static Bytes вообще не требуют heap.
- Не обходятся внутренности Tokio-каналов/задач, сообщения SQLite admission,
  выполняющиеся storage jobs, storage worker и C-аллокатор SQLite, driver TX queues
  и diagnostics, замыкания receipt callbacks, crypto identity подготовленного
  announce, resource proof cache, shared blackhole snapshot и состояние других
  runtime/link/protocol/plugin компонентов.
- Объекты, потерявшие связь с корнем обхода, не обнаруживаются этим методом.

Обход выполняется синхронно внутри actor, без копирования его таблиц и payload.
На больших таблицах он задерживает обработку сообщений; смотрите `capture_us`.
Файлы записываются в blocking task. На actor допускается только одна незавершённая
запись: при медленном диске новые снимки откладываются, очередь отчётов не растёт.
При аварийном завершении последний файл может быть неполным.

Для программного использования с той же feature доступен
`TransportActor::memory_snapshot()`, возвращающий folded-текст, оценку байтов
и предупреждения. Рост поля между снимками локализует удержание памяти;
сам по себе он ещё не доказывает ошибку времени жизни.
