//! Visit durable snapshot hashes without materializing the whole file/table.
//! A callback may stage pins, but the caller must not finish a sweep until the
//! entire snapshot (including its version field) has been validated.
use super::*;
use crate::persistence::{
    PersistedPathEntry, PersistedPathTable, PersistedTunnelPath, PersistedTunnelTable,
};
use serde::Deserializer;
use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::{fmt, io::BufReader, path::Path};

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Paths,
    Tunnels,
}

pub(crate) fn visit<F>(path: &Path, kind: Kind, emit: &mut F) -> Result<()>
where
    F: FnMut(PacketHash) -> Result<()>,
{
    let file = std::fs::File::open(path)?;
    let mut callback_error = None;
    let result = Table {
        kind,
        emit: &mut |hash| {
            emit(hash).map_err(|error| {
                callback_error = Some(error);
                StorageError::Invalid("snapshot pin staging failed")
            })
        },
    }
    .deserialize(&mut rmp_serde::Deserializer::new(BufReader::new(file)))
    .map_err(|_| StorageError::Invalid("routing snapshot unreadable; refusing GC"));
    if let Some(error) = callback_error {
        Err(error)
    } else {
        result
    }
}

struct Table<'a, F> {
    kind: Kind,
    emit: &'a mut F,
}
impl<'de, F: FnMut(PacketHash) -> Result<()>> DeserializeSeed<'de> for Table<'_, F> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_struct("RoutingSnapshot", &["entries", "version"], self)
    }
}
impl<F: FnMut(PacketHash) -> Result<()>> Table<'_, F> {
    fn version<E: de::Error>(&self, version: u32) -> std::result::Result<(), E> {
        let supported = match self.kind {
            Kind::Paths => PersistedPathTable::SUPPORTED_VERSIONS,
            Kind::Tunnels => PersistedTunnelTable::SUPPORTED_VERSIONS,
        };
        if supported.contains(&version) {
            Ok(())
        } else {
            Err(E::custom("unsupported snapshot version"))
        }
    }
}
impl<'de, F: FnMut(PacketHash) -> Result<()>> Visitor<'de> for Table<'_, F> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("routing snapshot")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        seq.next_element_seed(Entries {
            kind: self.kind,
            emit: &mut *self.emit,
        })?
        .ok_or_else(|| de::Error::missing_field("entries"))?;
        let version = seq
            .next_element()?
            .ok_or_else(|| de::Error::missing_field("version"))?;
        self.version(version)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        let mut entries = false;
        let mut version = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "entries" if !entries => {
                    map.next_value_seed(Entries {
                        kind: self.kind,
                        emit: &mut *self.emit,
                    })?;
                    entries = true;
                }
                "version" if version.is_none() => version = Some(map.next_value()?),
                "entries" | "version" => return Err(de::Error::custom("duplicate snapshot field")),
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        if !entries {
            return Err(de::Error::missing_field("entries"));
        }
        self.version(version.ok_or_else(|| de::Error::missing_field("version"))?)
    }
}

struct Entries<'a, F> {
    kind: Kind,
    emit: &'a mut F,
}
impl<'de, F: FnMut(PacketHash) -> Result<()>> DeserializeSeed<'de> for Entries<'_, F> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de, F: FnMut(PacketHash) -> Result<()>> Visitor<'de> for Entries<'_, F> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("routing entries")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        match self.kind {
            Kind::Paths => {
                while let Some(entry) = seq.next_element::<PersistedPathEntry>()? {
                    emit_hash(entry.packet_hash, self.emit)?;
                }
            }
            Kind::Tunnels => {
                while seq
                    .next_element_seed(Tunnel {
                        emit: &mut *self.emit,
                    })?
                    .is_some()
                {}
            }
        }
        Ok(())
    }
}

fn emit_hash<E: de::Error, F: FnMut(PacketHash) -> Result<()>>(
    hash: Option<Vec<u8>>,
    emit: &mut F,
) -> std::result::Result<(), E> {
    if let Some(hash) = hash {
        // Invalid hashes must not silently remove a durable packet's protection.
        let hash = hash
            .try_into()
            .map_err(|_| E::custom("invalid snapshot packet hash"))?;
        emit(hash).map_err(E::custom)?;
    }
    Ok(())
}

struct Tunnel<'a, F> {
    emit: &'a mut F,
}
impl<'de, F: FnMut(PacketHash) -> Result<()>> DeserializeSeed<'de> for Tunnel<'_, F> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_struct(
            "PersistedTunnelEntry",
            &[
                "tunnel_id",
                "interface_id",
                "expires",
                "paths",
                "interface_name",
                "interface_hash",
            ],
            self,
        )
    }
}
impl<'de, F: FnMut(PacketHash) -> Result<()>> Visitor<'de> for Tunnel<'_, F> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("tunnel entry")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        for _ in 0..3 {
            seq.next_element::<IgnoredAny>()?
                .ok_or_else(|| de::Error::custom("incomplete tunnel"))?;
        }
        seq.next_element_seed(TunnelPaths { emit: self.emit })?
            .ok_or_else(|| de::Error::missing_field("paths"))?;
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        let mut paths = false;
        while let Some(key) = map.next_key::<String>()? {
            if key == "paths" {
                if paths {
                    return Err(de::Error::duplicate_field("paths"));
                }
                map.next_value_seed(TunnelPaths {
                    emit: &mut *self.emit,
                })?;
                paths = true;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        if paths {
            Ok(())
        } else {
            Err(de::Error::missing_field("paths"))
        }
    }
}

struct TunnelPaths<'a, F> {
    emit: &'a mut F,
}
impl<'de, F: FnMut(PacketHash) -> Result<()>> DeserializeSeed<'de> for TunnelPaths<'_, F> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de, F: FnMut(PacketHash) -> Result<()>> Visitor<'de> for TunnelPaths<'_, F> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("tunnel paths")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        while let Some(entry) = seq.next_element::<PersistedTunnelPath>()? {
            emit_hash(entry.packet_hash, self.emit)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::PersistedTunnelEntry;

    #[test]
    fn streams_array_and_named_snapshots_and_rejects_incomplete_input() {
        let table = PersistedTunnelTable {
            version: PersistedTunnelTable::CURRENT_VERSION,
            entries: vec![PersistedTunnelEntry {
                tunnel_id: vec![1; 32],
                interface_id: 1,
                expires: 0.0,
                interface_name: None,
                interface_hash: None,
                paths: (0..1024)
                    .map(|n| PersistedTunnelPath {
                        destination_hash: vec![0; 16],
                        next_hop: None,
                        hops: 1,
                        expires: 0.0,
                        timestamp: 0.0,
                        random_blobs: vec![],
                        packet_hash: Some([n as u8; 32].to_vec()),
                    })
                    .collect(),
            }],
        };
        for bytes in [
            rmp_serde::to_vec(&table).unwrap(),
            rmp_serde::to_vec_named(&table).unwrap(),
        ] {
            let mut count = 0;
            Table {
                kind: Kind::Tunnels,
                emit: &mut |_| {
                    count += 1;
                    Ok(())
                },
            }
            .deserialize(&mut rmp_serde::Deserializer::new(bytes.as_slice()))
            .unwrap();
            assert_eq!(count, 1024);
            assert!(
                Table {
                    kind: Kind::Tunnels,
                    emit: &mut |_| Ok(())
                }
                .deserialize(&mut rmp_serde::Deserializer::new(&bytes[..bytes.len() - 1]))
                .is_err()
            );
        }
    }
}
