use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_network::VersionMessage;

/// Types of updates that an in-progress handshake wants to know about.
#[derive(Debug)]
pub enum Update {
    /// The peer sent a `Version` message.
    Version(VersionMessage),
    /// The peer sent a `Verack` message.
    Verack,
    /// The peer sent a `SendAddrV2` message (BIP-155).
    SendAddrV2,
    /// The peer sent a `WtxidRelay` message (BIP-0339).
    WtxidRelay,
    /// The peer sent another message.
    Other,
}

impl From<&NetworkMessage> for Update {
    fn from(value: &NetworkMessage) -> Self {
        match value {
            NetworkMessage::Version(v) => Self::Version(v.clone()),
            NetworkMessage::Verack => Self::Verack,
            NetworkMessage::SendAddrV2 => Self::SendAddrV2,
            NetworkMessage::WtxidRelay => Self::WtxidRelay,
            _ => Self::Other,
        }
    }
}

#[derive(Debug)]
pub enum Event<'a> {
    Wait,
    /// Send a `Verack` message to the peer.
    SendVerack,
    /// The peer violated the handshake protocol.
    Violation,
    /// The handshake is done.
    Done {
        /// The peer's advertised version.
        version: &'a VersionMessage,
        /// Whether the peer prefers AddrV2 messages.
        wants_addr_v2: bool,
        /// Wtxid relay
        wtxid_relay: bool,
    },
}

/// Contains the state of a handshake with a peer.
#[derive(Debug, Default)]
pub struct Handshake {
    /// The version message maybe received from the peer.
    their_version: Option<VersionMessage>,
    ///  Whether their verack has been received.
    their_verack: bool,
    /// Whether the peer prefers AddrV2 messages.
    wants_addr_v2: bool,
    /// Wtxid relay
    wtxid_relay: bool,
}

impl Handshake {
    /// Updates the handshake.
    pub fn update(&mut self, update: Update) -> Event {
        match (self, update) {
            (
                Self {
                    their_version: their_version @ None,
                    their_verack: false,
                    ..
                },
                Update::Version(v),
            ) => {
                *their_version = Some(v);
                Event::SendVerack
            }

            (
                Self {
                    their_version: Some(_),
                    their_verack: false,
                    wants_addr_v2: wants_addr_v2 @ false,
                    ..
                },
                Update::SendAddrV2,
            ) => {
                *wants_addr_v2 = true;
                Event::Wait
            }

            (
                Self {
                    their_version: Some(_),
                    their_verack: false,
                    wtxid_relay: wtxid_relay @ false,
                    ..
                },
                Update::WtxidRelay,
            ) => {
                *wtxid_relay = true;
                Event::Wait
            }

            (
                Self {
                    their_version: Some(v),
                    their_verack: their_verack @ false,
                    wants_addr_v2,
                    wtxid_relay,
                },
                Update::Verack,
            ) => {
                *their_verack = true;
                Event::Done {
                    version: v,
                    wants_addr_v2: *wants_addr_v2,
                    wtxid_relay: *wtxid_relay,
                }
            }

            _ => Event::Violation,
        }
    }
}
