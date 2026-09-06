-- Version 1: announces and immutable-by-hash packet payload. Route metadata
-- remains legacy until a later migration; packet_refs preserves its blobs.
CREATE TABLE packet_blobs (
    packet_hash BLOB PRIMARY KEY CHECK(length(packet_hash)=32),
    destination_hash BLOB NOT NULL CHECK(length(destination_hash)=16),
    raw_packet BLOB NOT NULL CHECK(length(raw_packet)<=65536)
) WITHOUT ROWID;

CREATE TABLE announces (
    destination_hash BLOB PRIMARY KEY CHECK(length(destination_hash)=16),
    hops INTEGER NOT NULL CHECK(hops BETWEEN 0 AND 255),
    app_data BLOB CHECK(app_data IS NULL OR length(app_data)<=65536),
    timestamp REAL NOT NULL,
    public_key BLOB CHECK(public_key IS NULL OR length(public_key)=64),
    identity_hash BLOB CHECK(identity_hash IS NULL OR length(identity_hash)=16),
    ratchet BLOB CHECK(ratchet IS NULL OR length(ratchet)=32),
    packet_hash BLOB REFERENCES packet_blobs(packet_hash),
    is_path_response INTEGER NOT NULL CHECK(is_path_response IN (0,1)),
    retained INTEGER NOT NULL CHECK(retained IN (0,1)),
    last_used REAL,
    name_hash BLOB NOT NULL CHECK(length(name_hash)=10)
) WITHOUT ROWID;

CREATE INDEX announces_packet_hash ON announces(packet_hash);
CREATE INDEX announces_name_hash ON announces(name_hash,destination_hash);
CREATE INDEX announces_identity_hash ON announces(identity_hash,destination_hash);
CREATE INDEX announces_expiry ON announces(retained,timestamp);

CREATE TABLE packet_refs (
    kind INTEGER NOT NULL CHECK(kind IN (0,1,2)),
    owner_key BLOB NOT NULL,
    destination_hash BLOB,
    packet_hash BLOB NOT NULL REFERENCES packet_blobs(packet_hash),
    PRIMARY KEY(kind,owner_key),
    CHECK((kind=0 AND length(owner_key)=16 AND length(destination_hash)=16 AND destination_hash IS NOT NULL)
       OR (kind=1 AND length(owner_key)=48 AND length(destination_hash)=16 AND destination_hash IS NOT NULL)
       OR (kind=2 AND length(owner_key)=32 AND destination_hash IS NULL))
) WITHOUT ROWID;

CREATE INDEX packet_refs_hash ON packet_refs(packet_hash);
CREATE INDEX packet_refs_destination ON packet_refs(destination_hash);

CREATE TABLE packet_keep (packet_hash BLOB PRIMARY KEY REFERENCES packet_blobs(packet_hash), generation INTEGER NOT NULL) WITHOUT ROWID;
CREATE TABLE sweep_state (id INTEGER PRIMARY KEY CHECK(id=1), generation INTEGER NOT NULL);
INSERT INTO sweep_state VALUES(1,0);
