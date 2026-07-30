# Миграция долговременного состояния Reticulum в SQLite

## Цель

Цель миграции — уменьшить и стабилизировать потребление оперативной памяти
`rsReticulum` на узлах с 256–512 МБ RAM. Долговременные таблицы и payload не
должны целиком загружаться в память при запуске или постоянно дублироваться
между RAM и отдельными файлами.

SQLite должна стать внутренним источником истины для долговременного состояния.
Данные загружаются по ключу или ограниченными страницами только тогда, когда они
нужны для маршрутизации, ответа на RPC или обслуживания кэша.

Приоритеты:

- предсказуемое потребление RAM;
- отсутствие больших периодических snapshot-копий;
- удаление лишних файлов и дублирующих форматов;
- атомарность метаданных и связанного packet payload;
- корректное восстановление после перезапуска;
- максимальное сохранение публичного API.

Совместимость формата хранения с Python Reticulum не требуется. После миграции
необязательно поддерживать Python-compatible msgpack tables, отдельные announce
cache files и старые Rust sidecar formats.

## Что хранится в памяти сейчас

`TransportActor` владеет всеми routing/runtime таблицами в одной задаче. Среди
долгоживущих структур наиболее важны:

- `PacketHashlist` — две `HashSet<[u8; 32]>`, до 2 000 000 packet hashes;
- `PathTable` — маршруты и отдельная таблица состояний;
- `recent_announces` — метаданные известных announces;
- raw announce cache — отдельные файлы на диске;
- tunnel table и восстановленные tunnel paths;
- blackhole table;
- rate table;
- persisted routing snapshots.

Кроме постоянного размера коллекций, текущий persistence создаёт временные пики:
path table клонируется, recent announces собираются в новый `Vec`, после чего
таблицы полностью сериализуются и записываются.

## Что следует перенести

### Packet hashlist

Это наиболее изолированный кандидат. Полный набор hashes хранится в SQLite, а в
RAM остаётся небольшой ограниченный кэш свежих значений. Логика двух поколений
может быть сохранена столбцом `generation`.

При невысокой нагрузке допустим point lookup в SQLite при промахе RAM-кэша.
Записи следует группировать в короткие транзакции.

### Announces и packet payload

Метаданные `recent_announces` и raw bytes announce должны храниться одной
атомарной записью. Отдельные файлы `cache/announces/<packet_hash>` после
миграции не нужны.

Обычные lookup и RPC-запросы метаданных не должны читать raw packet. Payload
извлекается только для CacheRequest replay или другого запроса конкретного
packet hash.

### Path table

Полная таблица маршрутов может храниться в SQLite, но это горячее состояние.
Рекомендуется гибрид:

- SQLite — источник истины;
- ограниченный LRU содержит активно используемые routes;
- часто изменяемое liveness state может временно оставаться в RAM;
- dirty route записывается в БД транзакционно;
- expiry и массовое удаление выполняются SQL-запросами.

Полный перевод следует делать после packet hashlist и announces, поскольку
`PathTable` имеет много обращений на packet hot path и методы, возвращающие
ссылки.

### Tunnel, blackhole и rate state

Эти таблицы можно перенести после измерения их фактического размера. Blackhole
и rate tables обычно малы, поэтому выигрыш может быть меньше стоимости
рефакторинга. Tunnel paths могут быть заметными на транспортном узле и должны
рассматриваться вместе с path storage.

## Что оставить в RAM

SQLite не должна использоваться для активного краткоживущего состояния:

- `LinkTable`;
- `ReverseTable`;
- announce/held announce queues;
- receipts и сопоставление receipts с LXMF messages;
- зарегистрированные interfaces и destination channels;
- path requests, waiters и discovery requests;
- suppressions и throttling timers;
- packet metrics;
- активные resource/link sessions;
- Tokio channels, callbacks и oneshot senders.

Эти структуры содержат runtime-объекты, часто меняются и очищаются за секунды
или минуты. Их перенос увеличит задержки и сложность, почти не уменьшив
долговременный RSS.

## Предлагаемая архитектура

```text
TransportActor
      |
      v
TransportStorage trait
      |
      +-- SqliteTransportStorage
      +-- MemoryTransportStorage (unit tests)
```

Владельцем SQLite-соединения должен быть `TransportActor` либо отдельный
storage actor. Интерфейсы TCP, LoRa, KISS и другие компоненты не должны
обращаться к базе напрямую.

Основные требования к storage API:

- point lookup без создания полного snapshot;
- выборка ограниченными страницами;
- разделение чтения metadata и payload;
- транзакционные batch inserts/deletes;
- отсутствие неограниченного внутреннего кэша;
- возможность реализации memory backend для тестов.

## Предварительная схема

```sql
CREATE TABLE schema_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE packet_hashes (
    hash       BLOB PRIMARY KEY CHECK(length(hash) = 32),
    generation INTEGER NOT NULL,
    inserted_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX packet_hashes_generation
    ON packet_hashes(generation);

CREATE TABLE announces (
    destination_hash BLOB PRIMARY KEY CHECK(length(destination_hash) = 16),
    packet_hash      BLOB UNIQUE CHECK(packet_hash IS NULL OR length(packet_hash) = 32),
    hops             INTEGER NOT NULL,
    app_data         BLOB,
    timestamp        INTEGER NOT NULL,
    public_key       BLOB,
    ratchet          BLOB,
    is_path_response INTEGER NOT NULL,
    retained         INTEGER NOT NULL,
    last_used        INTEGER,
    name_hash        BLOB NOT NULL CHECK(length(name_hash) = 10),
    raw_packet       BLOB
) WITHOUT ROWID;

CREATE INDEX announces_packet_hash
    ON announces(packet_hash);

CREATE INDEX announces_last_used
    ON announces(last_used);

CREATE INDEX announces_name_hash
    ON announces(name_hash);

CREATE TABLE paths (
    destination_hash BLOB PRIMARY KEY CHECK(length(destination_hash) = 16),
    timestamp        INTEGER NOT NULL,
    next_hop         BLOB,
    hops             INTEGER NOT NULL,
    expires          INTEGER NOT NULL,
    interface_key    TEXT NOT NULL,
    packet_hash      BLOB,
    state            INTEGER NOT NULL DEFAULT 0,
    random_blobs     BLOB
) WITHOUT ROWID;

CREATE INDEX paths_expires ON paths(expires);
CREATE INDEX paths_interface ON paths(interface_key);
CREATE INDEX paths_next_hop ON paths(next_hop);
```

Схема может быть нормализована позднее. `random_blobs` можно хранить одним
кодированным BLOB, поскольку они всегда читаются вместе с route. Если нужна
частичная проверка на стороне SQL, их можно вынести в дочернюю таблицу.

Для привязки пути к интерфейсу нельзя полагаться только на runtime
`InterfaceId`, который может измениться после перезапуска. Нужно использовать
стабильный interface key, построенный из имени и конфигурации, а при регистрации
интерфейса связывать его с текущим runtime ID.

## Packet payload в SQLite

Raw announce должен храниться в `announces.raw_packet`, связанный с
`packet_hash`. Это устраняет:

- отдельный каталог announce cache;
- orphan-файлы;
- сканирование каталога при запуске;
- отдельный sweep файлов;
- рассинхронизацию metadata и payload.

Metadata-запросы должны явно перечислять столбцы и не выбирать `raw_packet`.
Payload загружается только для replay конкретного announce.

Если в будущем в Reticulum появятся другие крупные persisted packet payload,
их следует хранить в отдельной универсальной таблице `packet_blobs`, а
`announces` будет ссылаться на неё по packet hash. Для текущего единственного
владельца проще и атомарнее хранить payload вместе с announce.

## Сохранение публичного API

Нужно сохранить внешние:

- `TransportMessage` и `TransportQuery`;
- варианты RPC request/response;
- DTO path table, announces, blackhole и rate entries;
- runtime handle и high-level methods;
- callbacks и announce handler API;
- конфигурационный API, кроме добавления необязательных SQLite-настроек.

Внутренний storage backend не должен быть виден приложениям.

Главное ограничение — методы `PathTable`, возвращающие `&PathEntry` и
`&mut PathEntry`. Нельзя вернуть обычную ссылку на строку, загруженную из
SQLite, без удержания записи в памяти. Возможные решения:

- оставить `PathTable` как ограниченный LRU и добавить storage fallback;
- для внутреннего кода перейти на owned `PathEntry`;
- заменить mutable reference на явную операцию `touch/update`;
- сохранить старый API через compatibility layer только там, где он публично
используется.

До изменения сигнатур нужен аудит внешних приложений. Максимальное сохранение
API не означает обязательное сохранение всех внутренних representation leaks,
если они не являются реально используемой публичной поверхностью.

## Отказ от snapshot persistence

После перевода таблицы на SQLite её не следует параллельно:

- клонировать;
- сериализовать целиком;
- сохранять в msgpack;
- экспортировать в Python-compatible format;
- восстанавливать из нескольких fallback formats.

Изменения записываются инкрементально. Для команд полного списка используются
ограниченные SQL-выборки, после чего существующий RPC response может быть
сформирован в прежнем формате.

Если публичный RPC требует вернуть всю таблицу одним `Vec`, API сохраняется, но
пик памяти останется на время такого запроса. Позднее можно добавить новый
постраничный API, не удаляя старый.

## Настройки для малой памяти

Предварительная конфигурация:

```sql
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA temp_store=FILE;
PRAGMA cache_size=-1024;
PRAGMA mmap_size=0;
PRAGMA wal_autocheckpoint=128;
PRAGMA busy_timeout=5000;
PRAGMA auto_vacuum=INCREMENTAL;
```

Page cache должен быть ограничен явно. `mmap_size=0` предотвращает скрытое
увеличение mapped/resident memory. Для узла с 256 МБ RAM разумная начальная
цель — около 1 МиБ SQLite page cache на соединение.

Записи packet hashes и часто обновляемых metadata необходимо группировать.
Отдельный durable commit на каждый принятый пакет недопустим из-за задержек и
износа SD-карты.

Полный автоматический `VACUUM` использовать не следует. Возврат места
выполняется редким `incremental_vacuum`, когда узел не занят передачей.

## Наблюдаемость

Перед миграцией и после каждого этапа нужны метрики:

- RSS, high-water mark, anonymous/file RSS;
- размер packet hashlist и каждого поколения;
- количество routes и path states;
- количество recent announces и суммарный размер app data;
- размер raw announce payload;
- размеры link/reverse/tunnel/receipt/rate/blackhole tables;
- размеры actor channels и очередей;
- размер database, WAL и SHM;
- число SQLite lookup/insert/delete;
- cache hit/miss;
- длительность transaction, checkpoint, cull;
- количество dropped actor messages.

Это позволит проверить, что SQLite уменьшает именно resident memory, а не
только меняет формат файлов.

## Рекомендуемый порядок

1. Добавить измерения всех таблиц и временных snapshot allocations.
2. Ввести storage abstraction и версию схемы.
3. Перенести packet hashlist.
4. Объединить recent announce metadata и raw payload в SQLite.
5. Удалить announce cache directory и полный announce snapshot.
6. Устранить полное клонирование routing state.
7. Реализовать SQLite-backed path storage с ограниченным LRU.
8. По результатам измерений перенести tunnel state.
9. Перенести blackhole/rate tables только при реальной пользе.
10. Удалить старые форматы и compatibility loaders.

Каждый этап должен отдельно измеряться на узле с TCP, LoRa и KISS. Ошибка или
задержка SQLite не должна блокировать обработку интерфейсов на неопределённое
время.

