use std::cell::RefCell;
use std::net::SocketAddr;
use std::thread::JoinHandle;

use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::p2p::message_blockdata::Inventory;
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::Network;
use peerlink::PeerId;

use crate::net;

use super::protocol;

pub fn client(socks_proxy: Option<SocketAddr>, network: crate::Network) -> Client {
    let (handle, join_handle) = match socks_proxy {
        Some(proxy) => {
            let (reactor, handle) = peerlink::Reactor::with_connector(
                Default::default(),
                peerlink::connector::Socks5Connector {
                    proxy,
                    // random proxy credentials to get an isolated Tor circuit
                    credentials: Some((
                        fastrand::u32(..).to_string(),
                        fastrand::u32(..).to_string(),
                    )),
                },
            )
            .unwrap();
            (handle, reactor.run())
        }
        None => {
            let (reactor, handle) = peerlink::Reactor::new(Default::default()).unwrap();
            (handle, reactor.run())
        }
    };

    Client {
        peerlink: handle,
        commands: Default::default(),
        network: network.into(),
        join_handle,
        our_version: VersionMessage {
            version: 70015,
            services: bitcoin::p2p::ServiceFlags::NONE,
            timestamp: crate::posix_time() as i64,
            receiver: bitcoin::p2p::Address {
                services: bitcoin::p2p::ServiceFlags::default(),
                address: [0; 8],
                port: 8333,
            },
            sender: bitcoin::p2p::Address {
                services: bitcoin::p2p::ServiceFlags::default(),
                address: [0; 8],
                port: 0,
            },
            nonce: fastrand::u64(..),
            user_agent: "".to_string(),
            start_height: 0,
            relay: false,
        },
    }
}

pub struct Client {
    peerlink: peerlink::Handle<protocol::Message, net::Service>,
    commands: RefCell<Vec<peerlink::Command<protocol::Message, net::Service>>>,
    network: Network,
    join_handle: JoinHandle<std::io::Result<()>>,
    our_version: VersionMessage,
}

impl super::Peerlike for PeerId {}

impl super::Outbox<PeerId> for Client {
    fn connect(&self, target: net::Service) {
        self.queue(peerlink::Command::Connect(target));
    }

    fn disconnect(&self, peer: PeerId) {
        self.queue(peerlink::Command::Disconnect(peer));
    }

    fn version(&self, peer: PeerId) {
        self.queue(self.message(peer, NetworkMessage::Version(self.our_version.clone())));
    }

    fn verack(&self, peer: PeerId) {
        self.queue(self.message(peer, NetworkMessage::Verack));
    }

    fn tx_inv(&self, peer: PeerId, txids: impl Iterator<Item = bitcoin::Txid>) {
        self.queue(self.message(
            peer,
            NetworkMessage::Inv(txids.map(Inventory::Transaction).collect()),
        ))
    }

    fn tx(&self, peer: PeerId, tx: bitcoin::Transaction) {
        self.queue(self.message(peer, NetworkMessage::Tx(tx)))
    }
}

impl super::Sender for Client {
    fn send(&self) -> std::io::Result<()> {
        self.commands.borrow_mut().drain(..).try_for_each(|cmd| {
            log::debug!(">> P2P: {:?}", cmd);
            self.peerlink.send(cmd)
        })
    }

    fn shutdown(self) -> JoinHandle<std::io::Result<()>> {
        let _ = self.peerlink.shutdown();
        self.join_handle
    }
}

impl super::Receiver<PeerId> for Client {
    fn receiver(&self) -> &crossbeam_channel::Receiver<impl Into<super::Event<PeerId>>> {
        self.peerlink.receiver()
    }
}

impl Client {
    /// Queues a command for the p2p reactor.
    fn queue(&self, cmd: peerlink::Command<protocol::Message, net::Service>) {
        self.commands.borrow_mut().push(cmd);
    }

    /// Constructs a message with the correct magic.
    fn message(
        &self,
        peer_id: PeerId,
        message: NetworkMessage,
    ) -> peerlink::Command<protocol::Message, net::Service> {
        peerlink::Command::Message(
            peer_id,
            protocol::Message(RawNetworkMessage::new(self.network.magic(), message)),
        )
    }
}

impl From<peerlink::Event<protocol::Message, net::Service>> for super::Event<PeerId> {
    fn from(value: peerlink::Event<protocol::Message, net::Service>) -> Self {
        match value {
            peerlink::Event::ConnectedTo { target, result } => Self::ConnectedTo {
                target,
                result: result.map(From::from),
            },

            peerlink::Event::ConnectedFrom {
                peer,
                addr,
                interface,
            } => Self::ConnectedFrom {
                peer,
                addr,
                interface,
            },

            peerlink::Event::Disconnected { peer, reason } => Self::Disconnected {
                peer,
                reason: reason.into(),
            },

            peerlink::Event::Message { peer, message } => Self::Message {
                peer,
                message: message.0,
            },

            peerlink::Event::NoPeer(peer) => Self::NoPeer(peer),

            peerlink::Event::SendBufferFull { peer, message } => Self::SendBufferFull {
                peer,
                message: message.0,
            },
        }
    }
}

impl From<peerlink::reactor::DisconnectReason> for super::DisconnectReason {
    fn from(value: peerlink::reactor::DisconnectReason) -> Self {
        match value {
            peerlink::reactor::DisconnectReason::Requested => Self::Requested,
            peerlink::reactor::DisconnectReason::Left => Self::Left,
            peerlink::reactor::DisconnectReason::CodecViolation => Self::CodecViolation,
            peerlink::reactor::DisconnectReason::WriteStale => Self::WriteStale,
            peerlink::reactor::DisconnectReason::Error(err) => {
                log::debug!("peer disconnect: IO error: {}", err);
                Self::Error
            }
        }
    }
}
