mod client;
mod protocol;

use std::io;
use std::net::SocketAddr;
use std::thread::JoinHandle;

use bitcoin::p2p::message::RawNetworkMessage;

use crate::net;

/// Provides common functionality that uniquely identifies a peer.
pub trait Peerlike:
    Clone + Copy + Eq + PartialEq + std::fmt::Debug + std::fmt::Display + std::hash::Hash
{
}

/// Describes a type that queues commands for outbound delivery.
pub trait Outbox<P: Peerlike> {
    /// Queues a command to connect to a peer.
    fn connect(&self, target: net::Service);

    /// Queues a command to disconnect from a peer.
    #[allow(unused)]
    fn disconnect(&self, peer: P);

    /// Queues a `Version` message for sending.
    fn version(&self, peer: P);

    /// Queues a `VerAck` message for sending.
    fn verack(&self, peer: P);

    /// Queues a `Tx` message for sending.
    fn tx(&self, peer: P, tx: bitcoin::Transaction);
}

/// Describes a type capable of receiving p2p events.
pub trait Receiver<P: Peerlike, T: Into<Event<P>>> {
    fn receiver(&self) -> &crossbeam_channel::Receiver<T>;
}

/// Describes a type that sends queued commands outbound.
pub trait Sender {
    /// Sends all the queued commands to the delivery subsystem.
    fn send(&self) -> io::Result<()>;

    /// Shuts down the client.
    fn shutdown(self) -> JoinHandle<std::io::Result<()>>;
}

/// Possible p2p network events.
#[derive(Debug)]
pub enum Event<P: Peerlike> {
    /// The result of connecting to a remote peer.
    ConnectedTo {
        /// The remote host that was connected to.
        target: net::Service,
        /// The result of the connection attempt.
        result: io::Result<P>,
    },
    /// Inbound connection received.
    ConnectedFrom {
        /// The peer associated with the event.
        peer: P,
        /// The address of the remote peer.
        addr: SocketAddr,
        /// The address of the local interface that accepted the connection.
        interface: SocketAddr,
    },
    /// A peer disconnected.
    Disconnected {
        /// The peer associated with the event.
        peer: P,
        /// The reason the peer left.
        reason: DisconnectReason,
    },
    /// A peer produced a message.
    Message {
        /// The peer associated with the event.
        peer: P,
        /// The message received from the peer.
        message: RawNetworkMessage,
    },
    /// No peer exists with the specified id. Sent when an operation was specified using a peer id
    /// that is not present.
    NoPeer(P),
    /// The send buffer associated with the peer is full. It means the peer is probably not
    /// reading data from the wire in a timely manner.
    SendBufferFull {
        /// The peer associated with the event.
        peer: P,
        /// The message that could not be sent to the peer.
        message: RawNetworkMessage,
    },
}

/// Explains why a client connection was disconnected.
#[derive(Debug)]
pub enum DisconnectReason {
    /// The disconnect was requested.
    Requested,
    /// The peer left.
    Left,
    /// The peer violated the protocol in some way.
    CodecViolation,
    /// The write side is stale, i.e. the peer is not reading the data we are sending.
    WriteStale,
    /// A network related error occurred.
    Error,
}

pub fn client(
    socks_proxy: Option<SocketAddr>,
    network: crate::Network,
    ua: Option<(String, u64, u64)>,
) -> impl Sender
       + Receiver<peerlink::PeerId, peerlink::Event<protocol::Message, net::Service>>
       + Outbox<peerlink::PeerId> {
    client::client(socks_proxy, network, ua)
}
