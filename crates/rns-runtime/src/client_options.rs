//! Per-instance client policy; independent of Cargo feature unification.
pub use rns_transport::actor::ClientAnnouncePolicy;

#[derive(Debug, Clone, Copy, Default)]
pub struct ClientOptions {
    pub announce_policy: ClientAnnouncePolicy,
}

impl ClientOptions {
    /// Server applications that do not browse the network opt in explicitly.
    pub fn server() -> Self {
        Self {
            announce_policy: ClientAnnouncePolicy::Requested,
        }
    }
}
