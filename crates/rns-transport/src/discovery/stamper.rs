//! Pluggable proof-of-work stamper for discovery announces.
//!
//! Python `RNS.Discovery.InterfaceAnnouncer` imports `LXMF.LXStamper`.
//! rsReticulum avoids an upward dependency on rsLXMF, so discovery accepts a
//! trait object. A native default implements the discovery workblock contract;
//! embedding applications can still provide their own implementation.

use sha2::{Digest, Sha256};

/// Native discovery PoW, matching LXStamper's 20-round HKDF workblock.
/// Generation is bounded per attempt; the scheduler retries failed attempts.
#[derive(Default)]
pub struct NativeDiscoveryStamper;

impl NativeDiscoveryStamper {
    fn prefix(infohash: &[u8; 32]) -> Sha256 {
        let mut digest = Sha256::new();
        for round in 0..super::constants::WORKBLOCK_EXPAND_ROUNDS {
            // These rounds are MessagePack positive fixints (0..20).
            let mut material = infohash.to_vec();
            material.push(round as u8);
            let salt = rns_crypto::sha::full_hash(&material);
            let block = rns_crypto::hkdf::hkdf_sha256(256, infohash, Some(&salt), None)
                .expect("fixed HKDF output length is valid");
            digest.update(block);
        }
        digest
    }

    fn meets(digest: &[u8], required: u8) -> bool {
        if required == 0 {
            return true;
        }
        // Python accepts hash <= 2**(256-cost), including exact equality.
        let mut target = [0u8; 32];
        let bit = 256 - required as usize;
        target[31 - bit / 8] = 1 << (bit % 8);
        digest <= target.as_slice()
    }
}

impl DiscoveryStamper for NativeDiscoveryStamper {
    fn generate(&self, infohash: &[u8; 32], target_value: u8) -> Option<Vec<u8>> {
        use rand::RngCore;
        let prefix = Self::prefix(infohash);
        let mut stamp = [0u8; super::constants::STAMP_SIZE];
        rand::thread_rng().fill_bytes(&mut stamp);
        for nonce in 0u64..1_048_576 {
            stamp[..8].copy_from_slice(&nonce.to_be_bytes());
            let mut digest = prefix.clone();
            digest.update(stamp);
            if Self::meets(&digest.finalize(), target_value) {
                return Some(stamp.to_vec());
            }
        }
        None
    }

    fn value(&self, infohash: &[u8; 32], stamp: &[u8]) -> u8 {
        let mut digest = Self::prefix(infohash);
        digest.update(stamp);
        let mut value = 0u16;
        for byte in digest.finalize() {
            value += byte.leading_zeros() as u16;
            if byte != 0 {
                break;
            }
        }
        value.min(255) as u8
    }

    fn valid(&self, infohash: &[u8; 32], stamp: &[u8], required_value: u8) -> bool {
        if stamp.len() != super::constants::STAMP_SIZE {
            return false;
        }
        let mut digest = Self::prefix(infohash);
        digest.update(stamp);
        Self::meets(&digest.finalize(), required_value)
    }
}

/// Minimal PoW stamp interface consumed by the discovery subsystem.
///
/// Validation must be deterministic and independent across calls. Generation
/// may use randomness, as LXStamper does — the announcer caches its last
/// successful stamp per info-hash and reuses it until the payload changes.
pub trait DiscoveryStamper: Send + Sync {
    /// Generate a `STAMP_SIZE`-byte stamp whose SHA-256-derived value meets
    /// or exceeds `target_value` (number of leading zero bits, as in the
    /// Python `LXStamper.generate_stamp` contract).
    ///
    /// Returns `None` if generation was cancelled or failed.
    fn generate(&self, infohash: &[u8; 32], target_value: u8) -> Option<Vec<u8>>;

    /// Compute the stamp's current value (Python `LXStamper.stamp_value`).
    /// Used by the receiver to log the learned stamp quality.
    fn value(&self, infohash: &[u8; 32], stamp: &[u8]) -> u8;

    /// Validate a stamp against `required_value` (Python
    /// `LXStamper.stamp_valid`). Returns true iff the stamp meets the bar.
    fn valid(&self, infohash: &[u8; 32], stamp: &[u8], required_value: u8) -> bool;
}

/// A no-op stamper used when on-network discovery is not enabled.
///
/// Never generates a stamp — so the announcer short-circuits each tick and
/// the receiver rejects every inbound stamp. This keeps discovery silent
/// when no stamper is installed (Python panics in this case; we don't).
pub struct NullStamper;

impl DiscoveryStamper for NullStamper {
    fn generate(&self, _infohash: &[u8; 32], _target_value: u8) -> Option<Vec<u8>> {
        None
    }

    fn value(&self, _infohash: &[u8; 32], _stamp: &[u8]) -> u8 {
        0
    }

    fn valid(&self, _infohash: &[u8; 32], _stamp: &[u8], _required_value: u8) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_lxstamper_workblock_and_validation_vector() {
        // Local LXMF.LXStamper, expand_rounds=20, material=bytes(range(32)).
        let material = std::array::from_fn(|i| i as u8);
        let prefix = NativeDiscoveryStamper::prefix(&material);
        assert_eq!(
            hex::encode(prefix.clone().finalize()),
            "7c06f15571960ba62e26a1d51bc4f9c86ebc44810701d232af0fe9a6c10f9e61"
        );
        let mut stamped = prefix;
        stamped.update([0u8; 32]);
        assert_eq!(
            hex::encode(stamped.finalize()),
            "0e989a139bc03348674d9d4198e34d9ba8e1977ecd67a8d68e13c860f5af426c"
        );
        let stamper = NativeDiscoveryStamper;
        assert_eq!(stamper.value(&material, &[0; 32]), 4);
        assert!(stamper.valid(&material, &[0; 32], 4));
        assert!(!stamper.valid(&material, &[0; 32], 5));
        assert!(!stamper.valid(&material, &[0; 31], 0));
    }

    #[test]
    fn native_stamp_generation_and_exact_threshold() {
        let stamper = NativeDiscoveryStamper;
        let stamp = stamper.generate(&[7; 32], 8).unwrap();
        assert!(stamper.valid(&[7; 32], &stamp, 8));
        let mut threshold = [0u8; 32];
        threshold[0] = 0x10;
        assert!(NativeDiscoveryStamper::meets(&threshold, 4));
        threshold[31] = 1;
        assert!(!NativeDiscoveryStamper::meets(&threshold, 4));
    }
}
