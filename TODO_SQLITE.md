# План миграции rsReticulum на SQLite

## 0. Baseline и диагностика

- [ ] Снять `VmRSS`, `VmHWM`, `RssAnon`, `RssFile` и `MemAvailable`.
- [ ] Повторить измерения после 1, 6, 24 и 72 часов работы.
- [ ] Добавить счётчики размеров:
  - [ ] packet hashlist по поколениям;
  - [ ] path table и path states;
  - [ ] recent announces и суммарный `app_data`;
  - [ ] raw announce cache;
  - [ ] link table;
  - [ ] reverse table;
  - [ ] tunnel table и tunnel paths;
  - [ ] receipt table;
  - [ ] blackhole table;
  - [ ] rate table;
  - [ ] packet metrics;
  - [ ] path requests/waiters/suppressions;
  - [ ] held announces и actor queues.
- [ ] Измерить пиковую память во время `save_state`.
- [ ] Измерить объём временных clone/serialization buffers.
- [ ] Установить целевой нормальный RSS для 256 и 512 МБ RAM.
- [ ] Установить допустимый пик RSS при полном RPC dump.

## 1. Аудит публичного API

- [ ] Составить список публичных типов `rns-transport`.
- [ ] Составить список публичных типов `rns-runtime`.
- [ ] Зафиксировать `TransportMessage`, `TransportQuery` и responses.
- [ ] Найти публичные поля routing tables.
- [ ] Найти методы, возвращающие `&T`, `&mut T` и reference iterators.
- [ ] Проверить использование API в `rns-runtime`, `rns-tools` и examples.
- [ ] Проверить API, используемый внешними приложениями.
- [ ] Определить обязательную compatibility surface.
- [ ] Добавить API regression/compile tests.
- [ ] Спроектировать совместимый wrapper для `PathTable`.
- [ ] Добавить новый постраничный API, не удаляя старый full-list API.

## 2. Storage abstraction

- [ ] Выбрать SQLite library (`rusqlite` как основной кандидат).
- [ ] Решить вопрос system SQLite против bundled SQLite.
- [ ] Оценить итоговый размер binaries.
- [ ] Ввести `TransportStorage` interface.
- [ ] Ввести единый `StorageError`.
- [ ] Реализовать `MemoryTransportStorage` для unit-тестов.
- [ ] Реализовать `SqliteTransportStorage`.
- [ ] Определить владельца SQLite connection.
- [ ] Не предоставлять интерфейсам прямой доступ к базе.
- [ ] Не удерживать общий async `Mutex` во время длительного SQL.
- [ ] Вынести тяжёлые maintenance operations в blocking worker.
- [ ] Добавить schema version.
- [ ] Реализовать последовательные migrations.
- [ ] Запретить открытие неизвестной более новой схемы.

## 3. Конфигурация и жизненный цикл базы

- [ ] Добавить путь к database в runtime storage paths.
- [ ] Добавить конфиг SQLite page cache.
- [ ] Добавить конфиг RAM cache/LRU.
- [ ] Применить и проверить:
  - [ ] WAL;
  - [ ] `synchronous=NORMAL`;
  - [ ] `temp_store=FILE`;
  - [ ] ограниченный `cache_size`;
  - [ ] `mmap_size=0`;
  - [ ] `wal_autocheckpoint`;
  - [ ] `busy_timeout`;
  - [ ] incremental auto-vacuum.
- [ ] Защитить database, WAL и SHM filesystem permissions.
- [ ] Определить поведение при read-only storage.
- [ ] Определить поведение при заполненном диске.
- [ ] Определить shutdown checkpoint.
- [ ] Не выполнять полный автоматический `VACUUM`.

## 4. Packet hashlist

- [ ] Создать таблицу `packet_hashes`.
- [ ] Сохранить семантику двух поколений.
- [ ] Реализовать `contains` через RAM cache + SQLite fallback.
- [ ] Реализовать `insert` с дедупликацией.
- [ ] Группировать inserts в ограниченные транзакции.
- [ ] Реализовать атомарную смену поколений.
- [ ] Удалять старое поколение без загрузки hashes.
- [ ] Ввести ограниченный RAM cache свежих hashes.
- [ ] Сделать размер cache конфигурируемым.
- [ ] Не загружать persisted hashlist целиком при старте.
- [ ] Удалить старый full snapshot hashlist.
- [ ] Добавить тесты duplicate detection.
- [ ] Добавить тесты generation rotation.
- [ ] Добавить тест restart между rotation phases.
- [ ] Добавить тест crash/rollback.
- [ ] Измерить lookup latency на OrangePi.
- [ ] Измерить write amplification на SD-карте.
- [ ] Сравнить RSS до и после миграции.

## 5. Recent announces и raw packet

- [ ] Создать таблицу `announces`.
- [ ] Хранить metadata и raw packet атомарно.
- [ ] Не выбирать raw packet для metadata queries.
- [ ] Реализовать lookup по destination hash.
- [ ] Реализовать lookup raw payload по packet hash.
- [ ] Реализовать upsert announce.
- [ ] Реализовать retained/unretained state.
- [ ] Реализовать `last_used`.
- [ ] Реализовать фильтр по name hash/aspect.
- [ ] Реализовать recent announces query с limit.
- [ ] Реализовать cleanup по age/path/retained rules.
- [ ] Реализовать удаление без orphan payload.
- [ ] Перевести CacheRequest replay на SQLite.
- [ ] Перевести known destination queries.
- [ ] Сохранить текущие RPC response DTO.
- [ ] Добавить постраничный RPC/query API.
- [ ] Добавить тесты announce/path response replacement.
- [ ] Добавить тесты retained entries.
- [ ] Добавить тест raw packet replay.
- [ ] Добавить тест large `app_data`.

## 6. Удаление announce cache files

- [ ] Прекратить создание `cache/announces/<packet_hash>`.
- [ ] Удалить directory index scan.
- [ ] Удалить per-file atomic write для announce payload.
- [ ] Удалить filesystem announce sweep.
- [ ] Удалить orphan cache cleanup.
- [ ] Удалить `announce_cache.msgpack`.
- [ ] Решить вопрос старых данных:
  - [ ] одноразовый импорт;
  - [ ] либо документированный чистый старт.
- [ ] После переходного периода удалить legacy loaders.
- [ ] Удалить Python-compatible announce persistence.
- [ ] Обновить storage layout documentation.

## 7. Отказ от полных snapshots

- [ ] Удалить clone path table при `save_state`.
- [ ] Удалить сбор recent announces в полный `Vec`.
- [ ] Не сериализовать SQLite-backed tables целиком.
- [ ] Записывать mutations инкрементально.
- [ ] Оставить explicit checkpoint вместо полного save.
- [ ] Сохранить shutdown semantics.
- [ ] Обеспечить восстановление после внезапного power loss.
- [ ] Измерить пиковый RSS до и после.

## 8. Path storage

- [ ] Создать таблицу `paths`.
- [ ] Определить стабильный interface key.
- [ ] Реализовать привязку stable key к runtime `InterfaceId`.
- [ ] Реализовать point lookup route.
- [ ] Реализовать `has_path` и `hops_to`.
- [ ] Реализовать insert/upsert.
- [ ] Реализовать explicit `touch`.
- [ ] Реализовать обновление liveness state.
- [ ] Реализовать remove/expire.
- [ ] Реализовать `drop_all_via`.
- [ ] Реализовать `drop_all_via_next_hop`.
- [ ] Реализовать batch expiry culling.
- [ ] Реализовать cull dead interfaces.
- [ ] Хранить random blobs без отдельной загрузки payload.
- [ ] Ввести ограниченный LRU активных routes.
- [ ] Не позволять LRU расти без ограничения.
- [ ] Определить write-through или write-back policy.
- [ ] При write-back обеспечить flush на shutdown.
- [ ] Сохранить прежний high-level routing API.
- [ ] Адаптировать методы, возвращающие `&PathEntry`.
- [ ] Адаптировать методы, возвращающие `&mut PathEntry`.
- [ ] Не выполнять SQL lookup несколько раз для одного packet path.
- [ ] Добавить тесты всех interface modes.
- [ ] Добавить тесты expiry и touch.
- [ ] Добавить тесты interface reconnect/rebind.
- [ ] Добавить тесты path response.
- [ ] Добавить тесты tunnel path restore.
- [ ] Измерить packet processing latency.

## 9. Tunnel state

- [ ] Измерить размер tunnel table на реальном узле.
- [ ] Спроектировать таблицы tunnels и tunnel paths.
- [ ] Использовать тот же route representation, где возможно.
- [ ] Перенести persisted tunnel metadata.
- [ ] Реализовать expiry без полного table scan.
- [ ] Сохранить активное tunnel runtime state в RAM при необходимости.
- [ ] Удалить старый tunnel table snapshot.
- [ ] Добавить restart и expiry tests.

## 10. Blackhole и rate tables

- [ ] Измерить их фактический вклад в RSS.
- [ ] Не переносить их только ради унификации.
- [ ] При подтверждённой пользе создать SQLite tables.
- [ ] Сохранить быстрый RAM snapshot blackholed identities.
- [ ] Атомарно обновлять snapshot после DB mutation.
- [ ] Реализовать expiry SQL queries.
- [ ] Сохранить прежние RPC responses.

## 11. Горячее состояние, которое не переносится

- [ ] Оставить `LinkTable` в RAM.
- [ ] Оставить `ReverseTable` в RAM.
- [ ] Оставить receipts в RAM.
- [ ] Оставить interfaces и destination channels в RAM.
- [ ] Оставить path requests/waiters в RAM.
- [ ] Оставить suppressions/timers в RAM.
- [ ] Оставить active resource/link sessions в RAM.
- [ ] Проверить и ограничить packet metrics.
- [ ] Проверить и ограничить held announces.
- [ ] Проверить ёмкости actor channels.
- [ ] Добавить явные верхние пределы для всех оставшихся коллекций.

## 12. Дисковое пространство и обслуживание

- [ ] Контролировать размер database, WAL и SHM.
- [ ] Резервировать свободное место для checkpoint.
- [ ] Определить реакцию на low disk space.
- [ ] Не блокировать packet actor длительным checkpoint.
- [ ] Выполнять incremental vacuum только в idle period.
- [ ] Добавить метрики free pages и WAL size.
- [ ] Добавить метрики transaction/checkpoint latency.
- [ ] Оценить износ SD-карты.
- [ ] Документировать рекомендуемый тип накопителя.

## 13. Тестирование

- [ ] Unit-тесты SQLite CRUD.
- [ ] Тесты schema migrations.
- [ ] Тесты повреждённой базы.
- [ ] Тесты заполненного диска.
- [ ] Тесты read-only filesystem.
- [ ] Тесты внезапного завершения процесса.
- [ ] Тесты восстановления WAL.
- [ ] Тесты одновременного RPC dump и packet traffic.
- [ ] Тесты TCP interface.
- [ ] Тесты LoRa/RNode interface.
- [ ] Тесты KISS interface.
- [ ] Тест нескольких физических интерфейсов одновременно.
- [ ] Soak test не менее 72 часов.
- [ ] Проверить отсутствие монотонного роста RSS.
- [ ] Проверить отсутствие unbounded growth RAM caches.
- [ ] Сравнить latency и packet loss с baseline.

## 14. Завершение

- [ ] Описать новый storage layout.
- [ ] Описать backup/restore базы.
- [ ] Описать safe shutdown/checkpoint.
- [ ] Описать memory/cache configuration.
- [ ] Обновить основной README.
- [ ] Удалить legacy msgpack persistence.
- [ ] Удалить Python-compatible persistence.
- [ ] Удалить неиспользуемые filesystem cache helpers.
- [ ] Удалить переходные feature flags после стабилизации.
- [ ] Провести финальный аудит публичного API.
- [ ] Зафиксировать измерения на 256 и 512 МБ RAM.
