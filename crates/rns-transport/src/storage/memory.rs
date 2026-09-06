use super::*;
use std::collections::BTreeMap;

/// Reference backend for contract tests and small ephemeral stores. Unlike
/// SQLite this intentionally holds all data in memory; never a fallback on
/// disk failure. Cloning for rollback here must not migrate into production SQL.
#[derive(Default, Clone)]
pub struct MemoryTransportStorage {
    generation: i64,
    keep: BTreeMap<PacketHash, i64>,
    announces: BTreeMap<DestinationHash, RecentAnnounce>,
    packets: BTreeMap<PacketHash, Vec<u8>>,
    references: BTreeMap<PacketOwner, PacketHash>,
}

impl MemoryTransportStorage {
    fn packet_destination(&self, hash: &PacketHash) -> Result<DestinationHash> {
        let raw = self.packets.get(hash).ok_or(StorageError::MissingPacket)?;
        validate_packet(hash, raw)
    }

    fn apply(&mut self, mutations: Vec<Mutation>) -> Result<()> {
        for m in mutations {
            match m {
                Mutation::PutAnnounce { mut announce, raw } => {
                    if let Some(hash) = announce.packet_hash {
                        if let Some(raw) = raw {
                            self.packets.entry(hash).or_insert(raw);
                        }
                        if self.packet_destination(&hash)? != announce.dest_hash {
                            return Err(StorageError::Invalid("packet destination mismatch"));
                        }
                    }
                    if let Some(old) = self.announces.get(&announce.dest_hash) {
                        announce.retained |= old.retained;
                        announce.last_used = match (old.last_used, announce.last_used) {
                            (Some(a), Some(b)) => Some(a.max(b)),
                            (a, b) => a.or(b),
                        };
                    }
                    self.announces.insert(announce.dest_hash, announce);
                }
                Mutation::PutPacket { hash, raw } => {
                    self.packets.entry(hash).or_insert(raw);
                }
                Mutation::SetReference { owner, hash } => {
                    let dest = self.packet_destination(&hash)?;
                    if owner.destination().is_some_and(|d| d != dest) {
                        return Err(StorageError::Invalid("reference destination mismatch"));
                    }
                    self.references.insert(owner, hash);
                }
                Mutation::RemoveReference(owner) => {
                    self.references.remove(&owner);
                }
                Mutation::RemoveAnnounce(dest) => {
                    self.announces.remove(&dest);
                }
                Mutation::SetRetained {
                    destination,
                    retained,
                } => {
                    if let Some(a) = self.announces.get_mut(&destination) {
                        a.retained = retained;
                    }
                }
                Mutation::Touch { destination, at } => {
                    if let Some(a) = self.announces.get_mut(&destination) {
                        a.last_used = Some(a.last_used.map_or(at, |old| old.max(at)));
                    }
                }
            }
        }
        Ok(())
    }
}

impl TransportStorage for MemoryTransportStorage {
    fn execute(&mut self, request: Request) -> Result<Reply> {
        request.validate()?;
        Ok(match request {
            Request::BeginSweep => {
                self.generation += 1;
                Reply::Generation(self.generation)
            }
            Request::KeepPackets { generation, hashes } => {
                if generation != self.generation {
                    return Err(StorageError::Invalid("stale sweep"));
                }
                for hash in hashes {
                    if self.packets.contains_key(&hash) {
                        self.keep.insert(hash, generation);
                    }
                }
                Reply::Applied
            }
            Request::FinishSweep { generation } => {
                if generation != self.generation {
                    return Err(StorageError::Invalid("stale sweep"));
                }
                self.keep.retain(|_, g| *g == generation);
                Reply::Applied
            }
            Request::ClearAnnounces => {
                let n = self.announces.len();
                self.announces.clear();
                Reply::Removed(n)
            }
            Request::CleanKnown {
                unused_before,
                used_before,
                limit,
            } => {
                let keys: Vec<_> = self
                    .announces
                    .values()
                    .filter(|a| {
                        !a.retained
                            && a.last_used.map_or(a.timestamp < unused_before, |t| {
                                t.max(a.timestamp) < used_before
                            })
                            && !self
                                .references
                                .keys()
                                .any(|o| o.destination() == Some(a.dest_hash))
                            && !self
                                .keep
                                .keys()
                                .any(|h| self.packet_destination(h).ok() == Some(a.dest_hash))
                    })
                    .take(limit)
                    .map(|a| a.dest_hash)
                    .collect();
                for k in &keys {
                    self.announces.remove(k);
                }
                Reply::Removed(keys.len())
            }
            Request::Apply(mutations) => {
                let mut staged = self.clone();
                staged.apply(mutations)?;
                *self = staged;
                Reply::Applied
            }
            Request::Announce(dest) => Reply::Announce(self.announces.get(&dest).cloned()),
            Request::Packet(hash) => Reply::Packet(self.packets.get(&hash).cloned()),
            Request::Page(query) => {
                let mut entries = Vec::new();
                let mut size = 0;
                for a in self.announces.values().filter(|a| query.matches(a)) {
                    let bytes = metadata_bytes(a);
                    if entries.len() == query.limit() || size + bytes > MAX_PAGE_BYTES {
                        break;
                    }
                    entries.push(a.clone());
                    size += bytes;
                }
                let next = entries.last().map(|a| a.dest_hash);
                Reply::Page(AnnouncePage { entries, next })
            }
            Request::ExpireAnnounces { before, limit } => {
                let expired: Vec<_> = self
                    .announces
                    .values()
                    .filter(|a| {
                        !a.retained
                            && a.timestamp < before
                            && a.last_used.is_none_or(|t| t < before)
                            && !self
                                .references
                                .keys()
                                .any(|o| o.destination() == Some(a.dest_hash))
                    })
                    .take(limit)
                    .map(|a| a.dest_hash)
                    .collect();
                for d in &expired {
                    self.announces.remove(d);
                }
                Reply::Removed(expired.len())
            }
            Request::CollectPackets { limit } => {
                let dead: Vec<_> = self
                    .packets
                    .keys()
                    .filter(|h| {
                        !self.keep.contains_key(*h)
                            && !self
                                .announces
                                .values()
                                .any(|a| a.packet_hash.as_ref() == Some(h))
                            && !self.references.values().any(|r| r == *h)
                    })
                    .take(limit)
                    .copied()
                    .collect();
                for h in &dead {
                    self.packets.remove(h);
                }
                Reply::Removed(dead.len())
            }
            Request::Stats => Reply::Stats(StorageStats {
                announces: self.announces.len() as u64,
                packets: self.packets.len() as u64,
                packet_bytes: self.packets.values().map(|p| p.len() as u64).sum(),
                references: self.references.len() as u64,
            }),
            Request::Checkpoint => Reply::Checkpoint {
                remaining_frames: 0,
            },
        })
    }
}
