use super::*;
use fs2::FileExt;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    fs::File,
    path::{Path, PathBuf},
    time::Duration,
};

const APPLICATION_ID: i64 = 0x524e5354; // RNST, never LXMF's database.
const SCHEMA_VERSION: i64 = 3;
const MIN_SCHEMA_VERSION: i64 = 2;
const COLUMNS: &str = "destination_hash,hops,app_data,timestamp,public_key,ratchet,packet_hash,is_path_response,retained,last_used,name_hash";

#[derive(Debug, Clone)]
pub struct SqliteOptions {
    pub page_cache_kib: u32,
    /// Automatic checkpoint threshold in WAL pages, not a memory allocation.
    pub wal_checkpoint_pages: u32,
    pub busy_timeout: Duration,
    /// Period between passive checkpoint / incremental vacuum passes.
    pub vacuum_interval: Duration,
    /// Maximum freelist pages reclaimed in one maintenance pass.
    pub vacuum_pages: u32,
    /// FULL by default: acknowledge committed pins even across power loss.
    /// false selects NORMAL, allowing loss of recent commits on power loss.
    pub durable_commits: bool,
}

impl Default for SqliteOptions {
    fn default() -> Self {
        Self {
            page_cache_kib: 1024,
            wal_checkpoint_pages: 128,
            busy_timeout: Duration::from_millis(50),
            vacuum_interval: Duration::from_secs(3600),
            vacuum_pages: 128,
            durable_commits: true,
        }
    }
}

pub struct SqliteTransportStorage {
    connection: Connection,
    path: PathBuf,
    active_sweep: Option<i64>,
    transactions: [metrics::Transactions; 6],
    // An advisory OS lock is held for the entire backend lifetime, including
    // idle periods. Never unlink the lock file (that would break ownership).
    _owner: File,
}

impl StorageHandle {
    pub async fn open_sqlite(
        path: PathBuf,
        role: StorageRole,
        options: SqliteOptions,
    ) -> Result<Self> {
        if role == StorageRole::SharedClient {
            return Err(StorageError::NotOwner);
        }
        Self::start(8, move || {
            SqliteTransportStorage::open(&path, role, options)
        })
        .await
    }
}

impl SqliteTransportStorage {
    /// Blocking: call on the storage worker. Does not create parent directories,
    /// touch legacy files, import data or silently recreate a broken database.
    pub fn open(path: &Path, role: StorageRole, options: SqliteOptions) -> Result<Self> {
        if role == StorageRole::SharedClient {
            return Err(StorageError::NotOwner);
        }
        if !(16..=16384).contains(&options.page_cache_kib)
            || !(64..=4096).contains(&options.wal_checkpoint_pages)
            || options.busy_timeout > Duration::from_secs(1)
        {
            return Err(StorageError::Invalid("SQLite cache/timeout bounds"));
        }
        // Resolve symlinks so two configured names for one database cannot
        // obtain different owner locks. Parent storage must already exist.
        let canonical = if path.exists() {
            std::fs::canonicalize(path)?
        } else {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            std::fs::canonicalize(parent)?.join(
                path.file_name()
                    .ok_or(StorageError::Invalid("database filename"))?,
            )
        };
        let path = canonical.as_path();
        let mut lock_path = path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let owner = private_file(Path::new(&lock_path))?;
        owner.try_lock_exclusive().map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                StorageError::AlreadyOwned
            } else {
                StorageError::Io(e)
            }
        })?;
        // Create with 0600 on Unix. SQLite's WAL/SHM inherit the DB permissions.
        // Preserve permissions on an existing DB and never truncate its bytes.
        drop(private_file(path)?);
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        // FAT and several removable-media filesystems derive Unix mode bits
        // from mount options and can reject chmod. Permission hardening is
        // best-effort; SQLite open/write and ownership failures remain fatal.
        secure_database_files(path, Path::new(&lock_path));
        connection.busy_timeout(options.busy_timeout)?;
        let application: i64 =
            connection.pragma_query_value(None, "application_id", |r| r.get(0))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let tables: i64 = connection.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        let empty = application == 0 && version == 0 && tables == 0;
        // Check before changing journal mode or executing any migration SQL.
        if !empty {
            if application != APPLICATION_ID {
                return Err(StorageError::ForeignDatabase);
            }
            if !(MIN_SCHEMA_VERSION..=SCHEMA_VERSION).contains(&version) {
                return Err(StorageError::Schema(version));
            }
        }
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "cache_size", -(options.page_cache_kib as i64))?;
        connection.pragma_update(None, "mmap_size", 0)?;
        connection.pragma_update(None, "temp_store", "FILE")?;
        if empty {
            // auto_vacuum must be selected before creating the first table.
            connection.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
            let tx =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(include_str!("schema.sql"))?;
            tx.pragma_update(None, "application_id", APPLICATION_ID)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        } else if version == 2 {
            // Version 2's CleanKnown query had to scan packet_blobs once for
            // every candidate announce. On a live daemon this eventually kept
            // the storage worker at one full core. Adding the lookup index is
            // atomic and preserves the newly-created SQLite store in place.
            let tx =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute(
                "CREATE INDEX IF NOT EXISTS packet_blobs_destination ON packet_blobs(destination_hash)",
                [],
            )?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        let mode: String =
            connection.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(StorageError::Invalid("WAL unavailable"));
        }
        connection.pragma_update(
            None,
            "synchronous",
            if options.durable_commits {
                "FULL"
            } else {
                "NORMAL"
            },
        )?;
        connection.pragma_update(None, "wal_autocheckpoint", options.wal_checkpoint_pages)?;
        connection.set_prepared_statement_cache_capacity(16);
        // The observed keep-set is scratch state, not a second durable copy.
        // FILE storage and an explicit page-cache budget bound its RAM use.
        connection.execute_batch(
            "PRAGMA temp.cache_size=-256;
             CREATE TEMP TABLE sweep_seen(packet_hash BLOB PRIMARY KEY) WITHOUT ROWID;",
        )?;
        // Detect an incomplete schema at open, not as a later lookup miss.
        connection.prepare(&format!("SELECT {COLUMNS} FROM announces LIMIT 0"))?;
        connection
            .prepare("SELECT packet_hash,destination_hash,raw_packet FROM packet_blobs LIMIT 0")?;
        connection.prepare(
            "SELECT kind,owner_key,destination_hash,packet_hash FROM packet_refs LIMIT 0",
        )?;
        // journal_mode may have created WAL/SHM after the first pass.
        secure_database_files(path, Path::new(&lock_path));
        Ok(Self {
            connection,
            path: path.to_path_buf(),
            active_sweep: None,
            transactions: [
                "apply",
                "begin_sweep",
                "keep_packets",
                "finish_sweep",
                "clean_known_page",
                "collect_packets_page",
            ]
            .map(metrics::Transactions::new),
            _owner: owner,
        })
    }

    fn apply(&mut self, mutations: Vec<Mutation>) -> Result<()> {
        let started = std::time::Instant::now();
        let items = mutations.len();
        let bytes = mutation_batch_bytes(&mutations);
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for m in mutations {
            match m {
                Mutation::PutAnnounce { announce: a, raw } => {
                    if let Some(hash) = a.packet_hash {
                        if let Some(raw) = raw {
                            put_packet(&tx, &hash, &raw)?;
                        }
                        require_packet(&tx, &hash, Some(a.dest_hash))?;
                    }
                    tx.prepare_cached("INSERT INTO announces
                        (destination_hash,hops,app_data,timestamp,public_key,ratchet,packet_hash,is_path_response,retained,last_used,name_hash,identity_hash)
                        VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                        ON CONFLICT(destination_hash) DO UPDATE SET
                        hops=excluded.hops,app_data=excluded.app_data,timestamp=excluded.timestamp,
                        public_key=excluded.public_key,identity_hash=excluded.identity_hash,ratchet=excluded.ratchet,packet_hash=excluded.packet_hash,
                        is_path_response=excluded.is_path_response,name_hash=excluded.name_hash,
                        retained=announces.retained OR excluded.retained,
                        last_used=CASE WHEN announces.last_used IS NULL THEN excluded.last_used
                            WHEN excluded.last_used IS NULL THEN announces.last_used
                            ELSE max(announces.last_used,excluded.last_used) END")?.execute(params![a.dest_hash.as_slice(), a.hops, a.app_data, a.timestamp,
                            a.public_key.as_ref().map(|k| k.as_slice()), a.ratchet.as_ref().map(|k| k.as_slice()),
                            a.packet_hash.as_ref().map(|k| k.as_slice()), a.is_path_response, a.retained, a.last_used, a.name_hash.as_slice(),
                            a.public_key.map(|pk| rns_crypto::sha::truncated_hash(&pk)).as_ref().map(|h| h.as_slice())])?;
                }
                Mutation::PutPacket { hash, raw } => put_packet(&tx, &hash, &raw)?,
                Mutation::SetReference { owner, hash } => {
                    let destination = owner.destination();
                    require_packet(&tx, &hash, destination)?;
                    let (kind, key) = owner_key(&owner);
                    tx.prepare_cached("INSERT INTO packet_refs(kind,owner_key,destination_hash,packet_hash) VALUES (?1,?2,?3,?4)
                        ON CONFLICT(kind,owner_key) DO UPDATE SET packet_hash=excluded.packet_hash")?.execute(params![kind,key,destination.as_ref().map(|d| d.as_slice()),hash.as_slice()])?;
                }
                Mutation::RemoveReference(owner) => {
                    let (kind, key) = owner_key(&owner);
                    tx.prepare_cached("DELETE FROM packet_refs WHERE kind=?1 AND owner_key=?2")?
                        .execute(params![kind, key])?;
                }
                Mutation::RemoveAnnounce(dest) => {
                    tx.prepare_cached("DELETE FROM announces WHERE destination_hash=?1")?
                        .execute([dest.as_slice()])?;
                }
                Mutation::SetRetained {
                    destination,
                    retained,
                } => {
                    tx.prepare_cached(
                        "UPDATE announces SET retained=?2 WHERE destination_hash=?1",
                    )?
                    .execute(params![destination.as_slice(), retained])?;
                }
                Mutation::Touch { destination, at } => {
                    tx.prepare_cached("UPDATE announces SET last_used=CASE WHEN last_used IS NULL THEN ?2 ELSE max(last_used,?2) END WHERE destination_hash=?1")?.execute(params![destination.as_slice(),at])?;
                }
            }
        }
        self.transactions[0].commit(tx, started, items, bytes)?;
        Ok(())
    }
}

fn private_file(path: &Path) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(unix)]
fn secure_database_files(database_path: &Path, lock_path: &Path) {
    secure_database_files_with(database_path, lock_path, |path, permissions| {
        std::fs::set_permissions(path, permissions)
    });
}

#[cfg(unix)]
fn secure_database_files_with<F>(database_path: &Path, lock_path: &Path, mut set_permissions: F)
where
    F: FnMut(&Path, std::fs::Permissions) -> std::io::Result<()>,
{
    use std::os::unix::fs::PermissionsExt;

    for path in [
        database_path.to_path_buf(),
        sqlite_sidecar(database_path, "-wal"),
        sqlite_sidecar(database_path, "-shm"),
        lock_path.to_path_buf(),
    ] {
        match set_permissions(&path, std::fs::Permissions::from_mode(0o600)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                path = %path.display(),
                %error,
                "could not restrict SQLite file permissions; continuing"
            ),
        }
    }
}

#[cfg(not(unix))]
fn secure_database_files(_database_path: &Path, _lock_path: &Path) {}

fn check_generation(conn: &Connection, generation: i64) -> Result<()> {
    let current: i64 =
        conn.query_row("SELECT generation FROM sweep_state WHERE id=1", [], |r| {
            r.get(0)
        })?;
    if current != generation {
        return Err(StorageError::Invalid("stale sweep"));
    }
    Ok(())
}

fn owner_key(owner: &PacketOwner) -> (i64, Vec<u8>) {
    match owner {
        PacketOwner::Path(d) => (0, d.to_vec()),
        PacketOwner::Tunnel {
            tunnel,
            destination,
        } => {
            let mut key = tunnel.to_vec();
            key.extend_from_slice(destination);
            (1, key)
        }
        PacketOwner::Cache(token) => (2, token.to_vec()),
    }
}

fn put_packet(conn: &Connection, hash: &PacketHash, raw: &[u8]) -> Result<()> {
    let destination = validate_packet(hash, raw)?;
    // Hash excludes mutable forwarding headers. Preserve the first wire
    // representation, just like the existing write-if-absent file cache.
    conn.prepare_cached(
        "INSERT INTO packet_blobs(packet_hash,destination_hash,raw_packet) VALUES (?1,?2,?3)
        ON CONFLICT(packet_hash) DO NOTHING",
    )?
    .execute(params![hash.as_slice(), destination.as_slice(), raw])?;
    Ok(())
}

fn require_packet(
    conn: &Connection,
    hash: &PacketHash,
    destination: Option<DestinationHash>,
) -> Result<()> {
    let found = conn
        .prepare_cached("SELECT destination_hash FROM packet_blobs WHERE packet_hash=?1")?
        .query_row([hash.as_slice()], |r| fixed::<16>(r, 0))
        .optional()?;
    let found = found.ok_or(StorageError::MissingPacket)?;
    if destination.is_some_and(|d| d != found) {
        return Err(StorageError::Invalid("reference destination mismatch"));
    }
    Ok(())
}

fn fixed<const N: usize>(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<[u8; N]> {
    let bytes: Vec<u8> = row.get(index)?;
    bytes.try_into().map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Blob,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid hash/key length",
            )),
        )
    })
}

fn optional_fixed<const N: usize>(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<[u8; N]>> {
    if row.get_ref(index)?.data_type() == rusqlite::types::Type::Null {
        Ok(None)
    } else {
        fixed(row, index).map(Some)
    }
}

fn metadata(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecentAnnounce> {
    Ok(RecentAnnounce {
        dest_hash: fixed(row, 0)?,
        hops: row.get(1)?,
        app_data: row.get(2)?,
        timestamp: row.get(3)?,
        public_key: optional_fixed(row, 4)?,
        ratchet: optional_fixed(row, 5)?,
        packet_hash: optional_fixed(row, 6)?,
        is_path_response: row.get(7)?,
        retained: row.get(8)?,
        last_used: row.get(9)?,
        name_hash: fixed(row, 10)?,
    })
}

impl TransportStorage for SqliteTransportStorage {
    fn execute(&mut self, request: Request) -> Result<Reply> {
        request.validate()?;
        Ok(match request {
            Request::BeginSweep => {
                let started = std::time::Instant::now();
                let tx = self.connection.transaction()?;
                tx.execute("DELETE FROM temp.sweep_seen", [])?;
                tx.execute("UPDATE sweep_state SET generation=generation+1 WHERE id=1", [])?;
                let generation = tx.query_row("SELECT generation FROM sweep_state WHERE id=1", [], |r| r.get(0))?;
                self.transactions[1].commit(tx, started, 0, 0)?;
                self.active_sweep = Some(generation);
                Reply::Generation(generation)
            }
            Request::KeepPackets { generation, hashes } => {
                if self.active_sweep != Some(generation) {
                    return Err(StorageError::Invalid("inactive sweep"));
                }
                let started = std::time::Instant::now();
                let tx = self.connection.transaction()?;
                check_generation(&tx, generation)?;
                {
                    let mut seen = tx.prepare_cached(
                        "INSERT INTO temp.sweep_seen(packet_hash) SELECT packet_hash FROM packet_blobs
                         WHERE packet_hash=?1 ON CONFLICT(packet_hash) DO NOTHING",
                    )?;
                    let mut keep = tx.prepare_cached(
                        "INSERT INTO packet_keep(packet_hash,generation) SELECT packet_hash,?2 FROM packet_blobs
                         WHERE packet_hash=?1 AND NOT EXISTS(SELECT 1 FROM packet_keep WHERE packet_hash=?1)",
                    )?;
                    for hash in &hashes {
                        seen.execute([hash.as_slice()])?;
                        // Existing pins need no main-DB write or fsync. New pins
                        // are still committed durably before acknowledging this chunk.
                        keep.execute(params![hash.as_slice(), generation])?;
                    }
                }
                self.transactions[2].commit(tx, started, hashes.len(), hashes.capacity() * 32)?;
                Reply::Applied
            }
            Request::FinishSweep { generation } => {
                // After restart the scratch set is gone. Refuse to finish an
                // interrupted sweep: durable old AND new pins must survive.
                if self.active_sweep != Some(generation) {
                    return Err(StorageError::Invalid("inactive sweep"));
                }
                let started = std::time::Instant::now();
                let tx = self.connection.transaction()?;
                check_generation(&tx, generation)?;
                tx.execute(
                    "DELETE FROM packet_keep WHERE NOT EXISTS
                     (SELECT 1 FROM temp.sweep_seen s WHERE s.packet_hash=packet_keep.packet_hash)", [],
                )?;
                self.transactions[3].commit(tx, started, 0, 0)?;
                self.active_sweep = None;
                Reply::Applied
            }
            Request::ClearAnnounces => Reply::Removed(self.connection.execute("DELETE FROM announces",[])?),
            Request::CleanKnown { unused_before,used_before,limit } => Reply::Removed(self.connection.execute(
                "DELETE FROM announces WHERE destination_hash IN (SELECT a.destination_hash FROM announces a WHERE retained=0
                AND ((last_used IS NULL AND timestamp<?1) OR (last_used IS NOT NULL AND last_used<?2))
                AND NOT EXISTS(SELECT 1 FROM packet_refs r WHERE r.destination_hash=a.destination_hash)
                AND NOT EXISTS(SELECT 1 FROM packet_keep k JOIN packet_blobs p ON p.packet_hash=k.packet_hash WHERE p.destination_hash=a.destination_hash)
                ORDER BY destination_hash LIMIT ?3)",params![unused_before,used_before,limit as i64])?),
            Request::CleanKnownPage { unused_before, used_before, after, limit } => {
                let started = std::time::Instant::now();
                let tx = self.connection.transaction()?;
                let keys = gc_page_keys::<16>(&tx,
                    "SELECT destination_hash FROM announces WHERE destination_hash>?1 ORDER BY destination_hash LIMIT ?2",
                    after.as_ref().map(|key| key.as_slice()), limit)?;
                let mut destinations = Vec::new();
                {
                    let mut delete = tx.prepare_cached(
                        "DELETE FROM announces WHERE destination_hash=?1 AND retained=0
                         AND ((last_used IS NULL AND timestamp<?2) OR (last_used IS NOT NULL AND last_used<?3))
                         AND NOT EXISTS(SELECT 1 FROM packet_refs r WHERE r.destination_hash=announces.destination_hash)
                         AND NOT EXISTS(SELECT 1 FROM packet_blobs p JOIN packet_keep k ON k.packet_hash=p.packet_hash WHERE p.destination_hash=announces.destination_hash)",
                    )?;
                    for key in &keys {
                        if delete.execute(params![key.as_slice(), unused_before, used_before])? > 0 {
                            destinations.push(*key);
                        }
                    }
                }
                self.transactions[4].commit(tx, started, keys.len(), keys.len() * 16)?;
                Reply::CleanedPage { removed: destinations.len(), destinations, next: keys.last().copied() }
            }
            Request::CollectPacketsPage { after, limit } => {
                let started = std::time::Instant::now();
                let tx = self.connection.transaction()?;
                let keys = gc_page_keys::<32>(&tx,
                    "SELECT packet_hash FROM packet_blobs WHERE packet_hash>?1 ORDER BY packet_hash LIMIT ?2",
                    after.as_ref().map(|key| key.as_slice()), limit)?;
                let mut removed = 0;
                {
                    let mut delete = tx.prepare_cached(
                        "DELETE FROM packet_blobs WHERE packet_hash=?1
                         AND NOT EXISTS(SELECT 1 FROM announces a WHERE a.packet_hash=?1)
                         AND NOT EXISTS(SELECT 1 FROM packet_keep k WHERE k.packet_hash=?1)
                         AND NOT EXISTS(SELECT 1 FROM packet_refs r WHERE r.packet_hash=?1)",
                    )?;
                    for key in &keys {
                        removed += delete.execute([key.as_slice()])?;
                    }
                }
                self.transactions[5].commit(tx, started, keys.len(), keys.len() * 32)?;
                Reply::CollectedPage { removed, next: keys.last().copied() }
            }
            Request::Apply(mutations) => { self.apply(mutations)?; Reply::Applied }
            Request::Announce(dest) => Reply::Announce(self.connection.prepare_cached(
                &format!("SELECT {COLUMNS} FROM announces WHERE destination_hash=?1"))?
                .query_row([dest.as_slice()],metadata).optional()?),
            Request::Packet(hash) => Reply::Packet(self.connection.prepare_cached(
                "SELECT raw_packet FROM packet_blobs WHERE packet_hash=?1")?
                .query_row([hash.as_slice()],|r| r.get(0)).optional()?),
            Request::Page(q) => {
                // Avoid nullable-OR cursor predicates: they can turn each
                // successive page into a scan from the beginning of the table.
                let mut sql = format!("SELECT {COLUMNS} FROM announces WHERE destination_hash>?1");
                if q.name_hash.is_some() { sql.push_str(" AND name_hash=?2"); }
                if q.retained.is_some() { sql.push_str(" AND retained=?3"); }
                if q.identity_hash.is_some() { sql.push_str(" AND identity_hash=?5"); }
                sql.push_str(" ORDER BY destination_hash LIMIT ?4");
                let mut stmt = self.connection.prepare_cached(&sql)?;
                let mut bindings: Vec<rusqlite::types::Value> = vec![q.after.map_or(Vec::new(),|d|d.to_vec()).into(),
                    q.name_hash.map(|d|rusqlite::types::Value::Blob(d.to_vec())).unwrap_or(rusqlite::types::Value::Null),
                    q.retained.map(|r|rusqlite::types::Value::Integer(i64::from(r))).unwrap_or(rusqlite::types::Value::Null),
                    (q.limit() as i64).into()];
                if let Some(id)=q.identity_hash { bindings.push(id.to_vec().into()); }
                let mut rows = stmt.query(rusqlite::params_from_iter(bindings))?;
                let mut entries = Vec::new();
                let mut bytes = 0;
                while let Some(row) = rows.next()? {
                    let a = metadata(row)?;
                    let size = metadata_bytes(&a);
                    if bytes + size > MAX_PAGE_BYTES { break; }
                    bytes += size;
                    entries.push(a);
                }
                let next = entries.last().map(|a| a.dest_hash);
                Reply::Page(AnnouncePage { entries,next })
            }
            Request::ExpireAnnounces { before, limit } => Reply::Removed(self.connection.execute(
                "DELETE FROM announces WHERE destination_hash IN (SELECT a.destination_hash FROM announces a
                WHERE a.retained=0 AND a.timestamp<?1 AND (a.last_used IS NULL OR a.last_used<?1)
                    AND NOT EXISTS(SELECT 1 FROM packet_refs r WHERE r.destination_hash=a.destination_hash)
                ORDER BY a.destination_hash LIMIT ?2)",params![before,limit as i64])?),
            Request::CollectPackets { limit } => Reply::Removed(self.connection.execute(
                "DELETE FROM packet_blobs WHERE packet_hash IN (SELECT p.packet_hash FROM packet_blobs p
                WHERE NOT EXISTS(SELECT 1 FROM announces a WHERE a.packet_hash=p.packet_hash)
                    AND NOT EXISTS(SELECT 1 FROM packet_keep k WHERE k.packet_hash=p.packet_hash)
                    AND NOT EXISTS(SELECT 1 FROM packet_refs r WHERE r.packet_hash=p.packet_hash)
                ORDER BY p.packet_hash LIMIT ?1)",[limit as i64])?),
            Request::Stats => Reply::Stats(self.connection.query_row("SELECT
                (SELECT count(*) FROM announces), (SELECT count(*) FROM packet_blobs),
                (SELECT coalesce(sum(length(raw_packet)),0) FROM packet_blobs), (SELECT count(*) FROM packet_refs)", [], |r| Ok(StorageStats {
                    announces:r.get(0)?,packets:r.get(1)?,packet_bytes:r.get(2)?,references:r.get(3)?,
                }))?),
            Request::Maintain { vacuum_pages } => {
                let page_size = pragma_u64(&self.connection, "page_size")?;
                let page_count = pragma_u64(&self.connection, "page_count")?;
                let free_before = pragma_u64(&self.connection, "freelist_count")?;
                let (busy, wal_frames, checkpointed): (u64, u64, u64) = self.connection
                    .query_row("PRAGMA main.wal_checkpoint(PASSIVE)", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                let requested = u64::from(vacuum_pages).min(free_before);
                if requested > 0 {
                    self.connection.execute_batch(&format!("PRAGMA incremental_vacuum({requested})"))?;
                }
                let free_after = pragma_u64(&self.connection, "freelist_count")?;
                Reply::Maintenance(StorageMaintenance {
                    database_bytes: file_size(&self.path),
                    wal_bytes: file_size(&sqlite_sidecar(&self.path, "-wal")),
                    page_size,
                    page_count,
                    free_pages: free_after,
                    checkpointed_frames: checkpointed,
                    remaining_wal_frames: if busy == 0 { wal_frames.saturating_sub(checkpointed) } else { wal_frames },
                    vacuumed_pages: free_before.saturating_sub(free_after),
                    page_cache_kib: pragma_i64(&self.connection, "cache_size")?.unsigned_abs(),
                })
            }
            Request::Checkpoint => {
                let (_, frames, done): (i64,i64,i64) = self.connection.query_row("PRAGMA main.wal_checkpoint(PASSIVE)",[], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
                Reply::Checkpoint { remaining_frames: (frames-done).max(0) }
            }
        })
    }
}

fn gc_page_keys<const N: usize>(
    connection: &Connection,
    sql: &str,
    after: Option<&[u8]>,
    limit: usize,
) -> Result<Vec<[u8; N]>> {
    let mut statement = connection.prepare_cached(sql)?;
    Ok(statement
        .query_map(params![after.unwrap_or(&[]), limit as i64], |row| {
            fixed::<N>(row, 0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

fn pragma_u64(connection: &Connection, name: &str) -> Result<u64> {
    connection
        .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
        .map_err(StorageError::from)
}

fn pragma_i64(connection: &Connection, name: &str) -> Result<i64> {
    connection
        .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
        .map_err(StorageError::from)
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |metadata| metadata.len())
}

fn sqlite_sidecar(database_path: &Path, suffix: &str) -> PathBuf {
    let mut path = database_path.as_os_str().to_os_string();
    path.push(suffix);
    path.into()
}

#[cfg(test)]
pub(crate) mod failure_tests {
    use super::*;

    // A database the size of the observed RNS-Gate keep-set. Seed directly
    // because these tests measure storage work, not signature verification.
    pub(crate) fn populated_store(
        count: u32,
    ) -> (PathBuf, SqliteTransportStorage, Vec<PacketHash>) {
        let dir = std::env::temp_dir().join(format!(
            "rns-sweep-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let mut store = SqliteTransportStorage::open(
            &dir.join("transport.sqlite"),
            StorageRole::Standalone,
            Default::default(),
        )
        .unwrap();
        let mut hashes: Vec<_> = (0..count)
            .map(|n| rns_crypto::sha::full_hash(&n.to_be_bytes()))
            .collect();
        hashes.sort_unstable();
        let tx = store.connection.transaction().unwrap();
        {
            let mut packet = tx
                .prepare("INSERT INTO packet_blobs VALUES(?1,?2,zeroblob(200))")
                .unwrap();
            let mut keep = tx.prepare("INSERT INTO packet_keep VALUES(?1,0)").unwrap();
            for hash in &hashes {
                packet
                    .execute(params![hash.as_slice(), &hash[..16]])
                    .unwrap();
                keep.execute([hash.as_slice()]).unwrap();
            }
        }
        tx.commit().unwrap();
        store
            .connection
            .pragma_update(None, "wal_autocheckpoint", 0)
            .unwrap();
        store
            .connection
            .execute_batch("PRAGMA main.wal_checkpoint(TRUNCATE)")
            .unwrap();
        (dir, store, hashes)
    }

    pub(crate) fn obsolete_store(count: u32) -> (PathBuf, SqliteTransportStorage) {
        let (dir, store, _) = populated_store(count);
        store.connection.execute_batch(
            "DELETE FROM packet_keep;
             INSERT INTO announces(destination_hash,hops,timestamp,packet_hash,is_path_response,retained,name_hash)
             SELECT destination_hash,1,0,packet_hash,0,0,zeroblob(10) FROM packet_blobs;",
        ).unwrap();
        (dir, store)
    }

    #[test]
    fn checkpoint_threshold_is_configurable_without_growing_page_cache() {
        for pages in [128, 512, 1024] {
            let (dir, store, _) = populated_store(1);
            drop(store);
            let store = SqliteTransportStorage::open(
                &dir.join("transport.sqlite"),
                StorageRole::Standalone,
                SqliteOptions {
                    wal_checkpoint_pages: pages,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                pragma_u64(&store.connection, "wal_autocheckpoint").unwrap(),
                u64::from(pages)
            );
            assert_eq!(pragma_i64(&store.connection, "cache_size").unwrap(), -1024);
            drop(store);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn failed_keep_chunk_rolls_back_scratch_and_durable_changes() {
        let (dir, mut store, hashes) = populated_store(3);
        store
            .connection
            .execute(
                "DELETE FROM packet_keep WHERE packet_hash=?1",
                [hashes[2].as_slice()],
            )
            .unwrap();
        let Reply::Generation(generation) = store.execute(Request::BeginSweep).unwrap() else {
            panic!()
        };
        store
            .execute(Request::KeepPackets {
                generation,
                hashes: vec![hashes[0]],
            })
            .unwrap();
        store.connection.execute_batch("CREATE TEMP TRIGGER fail_new_pin BEFORE INSERT ON main.packet_keep BEGIN SELECT RAISE(ABORT,'injected pin failure'); END;").unwrap();
        assert!(
            store
                .execute(Request::KeepPackets {
                    generation,
                    hashes: vec![hashes[1], hashes[2]]
                })
                .is_err()
        );
        // Even scratch updates earlier in the failed transaction must roll back.
        let seen: i64 = store
            .connection
            .query_row("SELECT count(*) FROM temp.sweep_seen", [], |r| r.get(0))
            .unwrap();
        let pins: i64 = store
            .connection
            .query_row("SELECT count(*) FROM packet_keep", [], |r| r.get(0))
            .unwrap();
        assert_eq!(seen, 1);
        assert_eq!(pins, 2);
        store
            .connection
            .execute_batch("DROP TRIGGER temp.fail_new_pin")
            .unwrap();
        store
            .execute(Request::KeepPackets {
                generation,
                hashes: vec![hashes[1], hashes[2]],
            })
            .unwrap();
        store.execute(Request::FinishSweep { generation }).unwrap();
        assert!(matches!(
            store
                .execute(Request::CollectPacketsPage {
                    after: None,
                    limit: 128
                })
                .unwrap(),
            Reply::CollectedPage { removed: 0, .. }
        ));
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unchanged_keep_set_does_not_rewrite_wal() {
        let (dir, mut store, hashes) = populated_store(22_000);
        let Reply::Generation(generation) = store.execute(Request::BeginSweep).unwrap() else {
            panic!()
        };
        let wal = sqlite_sidecar(&store.path, "-wal");
        let before = file_size(&wal);
        let started = std::time::Instant::now();
        for chunk in hashes.chunks(MAX_BATCH_ITEMS) {
            store
                .execute(Request::KeepPackets {
                    generation,
                    hashes: chunk.to_vec(),
                })
                .unwrap();
        }
        assert_eq!(
            file_size(&wal),
            before,
            "unchanged pins must not write the main WAL"
        );
        store.execute(Request::FinishSweep { generation }).unwrap();
        assert_eq!(
            file_size(&wal),
            before,
            "unchanged finish must not rewrite pins"
        );
        eprintln!(
            "unchanged sweep: packets={} elapsed_ms={} wal_bytes={}",
            hashes.len(),
            started.elapsed().as_millis(),
            before
        );
        // Compare the old generation-rewrite algorithm with the same sorted
        // keys, connection, durability settings and page cache.
        store
            .connection
            .execute_batch("PRAGMA main.wal_checkpoint(TRUNCATE)")
            .unwrap();
        let started = std::time::Instant::now();
        for chunk in hashes.chunks(MAX_BATCH_ITEMS) {
            let tx = store.connection.transaction().unwrap();
            for hash in chunk {
                tx.execute("INSERT INTO packet_keep(packet_hash,generation) SELECT packet_hash,?2 FROM packet_blobs WHERE packet_hash=?1 ON CONFLICT(packet_hash) DO UPDATE SET generation=excluded.generation", params![hash.as_slice(), generation + 1]).unwrap();
            }
            tx.commit().unwrap();
        }
        let old_bytes = file_size(&wal);
        eprintln!(
            "old sorted sweep: elapsed_ms={} wal_bytes={}",
            started.elapsed().as_millis(),
            old_bytes
        );
        assert!(
            old_bytes > before * 100,
            "fixture must expose write amplification"
        );
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn permission_hardening_errors_are_nonfatal() {
        let paths = std::cell::RefCell::new(Vec::new());
        secure_database_files_with(
            Path::new("fat/transport.db"),
            Path::new("fat/transport.db.lock"),
            |path, _| {
                paths.borrow_mut().push(path.to_path_buf());
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "FAT mount controls mode bits",
                ))
            },
        );
        assert_eq!(
            paths.into_inner(),
            [
                PathBuf::from("fat/transport.db"),
                PathBuf::from("fat/transport.db-wal"),
                PathBuf::from("fat/transport.db-shm"),
                PathBuf::from("fat/transport.db.lock"),
            ]
        );
    }

    #[test]
    fn full_and_read_only_fail_explicitly_without_partial_metadata() {
        let dir = std::env::temp_dir().join(format!(
            "rns-sqlite-full-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let mut store = SqliteTransportStorage::open(
            &dir.join("transport.sqlite"),
            StorageRole::Standalone,
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            store
                .connection
                .pragma_query_value(None, "cache_size", |r| r.get::<_, i64>(0))
                .unwrap(),
            -1024
        );
        assert_eq!(
            store
                .connection
                .pragma_query_value(None, "mmap_size", |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .connection
                .pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .connection
                .pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        let a = RecentAnnounce {
            dest_hash: [1; 16],
            hops: 1,
            app_data: Some(vec![7; MAX_VALUE_BYTES]),
            timestamp: 100.0,
            public_key: None,
            ratchet: None,
            packet_hash: None,
            is_path_response: false,
            retained: false,
            last_used: None,
            name_hash: [2; 10],
        };
        store
            .connection
            .pragma_update(None, "query_only", true)
            .unwrap();
        let request = Request::Apply(vec![Mutation::PutAnnounce {
            announce: a,
            raw: None,
        }]);
        let error = store.execute(request.clone()).unwrap_err();
        assert!(
            matches!(error,StorageError::Sqlite(rusqlite::Error::SqliteFailure(ref e,_)) if e.code == rusqlite::ErrorCode::ReadOnly)
        );
        store
            .connection
            .pragma_update(None, "query_only", false)
            .unwrap();
        let pages: i64 = store
            .connection
            .pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap();
        store
            .connection
            .pragma_update(None, "max_page_count", pages)
            .unwrap();
        let error = store.execute(request).unwrap_err();
        assert!(
            matches!(error,StorageError::Sqlite(rusqlite::Error::SqliteFailure(ref e,_)) if e.code == rusqlite::ErrorCode::DiskFull)
        );
        assert!(store.connection.is_autocommit());
        assert!(matches!(
            store.execute(Request::Stats).unwrap(),
            Reply::Stats(StorageStats {
                announces: 0,
                packets: 0,
                references: 0,
                ..
            })
        ));
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
