//! # Bitcoin Transaction Broadcast Library
//!
//! This is a library that broadcasts Bitcoin transactions directly into the P2P network by
//! connecting to a set of random Bitcoin nodes. This differs from other broadcast tools in that
//! it does not not interact with any centralized services, such as block explorers.
//!
//! If Tor is running on the same system, connectivity to the P2P network is established through a
//! newly created circuit. Having Tor Browser running in the background is sufficient. Tor daemon
//! also works.
//!
//! ## Fine-tuning
//! The broadcast process can be fine-tuned using the `Opts` struct. Please refer to its
//! documentation for details.
//!
//! ## Example
//!
//!```no_run
//! // this is our hex-encoded transaction that we want to parse and broadcast
//! let tx = "6afcc7949dd500000....".parse().unwrap();
//!
//! // we start the broadcast process and acquire a receiver to the info events
//! let receiver = pushtx::broadcast(vec![tx], pushtx::Opts::default());
//!
//! // start reading info events until `Done` is received
//! loop {
//!     match receiver.recv().unwrap() {
//!         pushtx::Info::Done(Ok(report)) => {
//!             println!("we successfully broadcast to {} peers", report.broadcasts);
//!             break;
//!         }
//!         pushtx::Info::Done(Err(err)) => {
//!             println!("we failed to broadcast to any peers, reason = {err}");
//!             break;
//!         }
//!         _ => {}
//!     }
//! }
//!```

mod broadcast;
mod handshake;
mod net;
mod p2p;
mod seeds;

use std::{net::SocketAddr, num::NonZeroUsize, str::FromStr};

use bitcoin::consensus::Decodable;

/// A Bitcoin transaction to be broadcast into the network.
#[derive(Debug)]
pub struct Transaction(bitcoin::Transaction);

impl Transaction {
    /// Tries to parse a hex-encoded string into `Transaction`.
    pub fn from_hex(tx: impl AsRef<str>) -> Result<Self, ParseTxError> {
        tx.as_ref().parse()
    }

    /// Tries to convert raw tx bytes into `Transaction`.
    pub fn from_bytes(tx: impl AsRef<[u8]>) -> Result<Self, ParseTxError> {
        tx.as_ref().try_into()
    }

    /// Returns the txid of this transaction.
    pub fn txid(&self) -> impl std::fmt::Display {
        self.0.txid()
    }
}

impl FromStr for Transaction {
    type Err = ParseTxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s).map_err(|_| ParseTxError::NotHex)?;
        bytes.as_slice().try_into()
    }
}

impl TryFrom<&[u8]> for Transaction {
    type Error = ParseTxError;

    fn try_from(mut value: &[u8]) -> Result<Self, Self::Error> {
        let tx = bitcoin::Transaction::consensus_decode(&mut value)
            .map_err(|_| ParseTxError::InvalidTxBytes)?;
        Ok(Self(tx))
    }
}

/// Why an input could not be interpereted as a valid transaction.
#[derive(Debug)]
pub enum ParseTxError {
    /// The input was not valid hex.
    NotHex,
    /// The provided bytes did not deserialize to a valid transaction.
    InvalidTxBytes,
}

impl std::error::Error for ParseTxError {}

impl std::fmt::Display for ParseTxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseTxError::NotHex => write!(f, "Transaction is not valid hex"),
            ParseTxError::InvalidTxBytes => write!(f, "Transaction bytes are invalid"),
        }
    }
}

/// Determines how to use Tor. The default is `BestEffort`.
#[derive(Debug, Default, Clone)]
pub enum TorMode {
    /// Detects whether Tor is running locally at the usual port and attempts to use it. If no Tor
    /// is detected, the connection to the p2p network is established through clearnet.
    #[default]
    BestEffort,
    /// Do not use Tor even if it is available and running.
    No,
    /// Exclusively use Tor. If it is not available, do not use clearnet.
    Must,
}

/// Defines how the initial pool of peers that we broadcast to is found.
#[derive(Debug, Default, Clone)]
pub enum FindPeerStrategy {
    /// First resolve peers from DNS seeds (same as Bitcoin Core). Fall back on a fixed peer list
    /// (also taken from Bitcoin Core) if that fails. Failure is defined a finding less than 20 peers.
    #[default]
    DnsSeedWithFixedFallback,
    /// Resolve peers from DNS seeds only.
    DnsSeedOnly,
    /// Use a user provided list of nodes.
    Custom(Vec<SocketAddr>),
}

/// The network to connect to.
#[derive(Debug, Default, Clone, Copy)]
pub enum Network {
    #[default]
    Mainnet,
    Testnet,
    Regtest,
    Signet,
}

impl From<Network> for bitcoin::Network {
    fn from(value: Network) -> Self {
        match value {
            Network::Mainnet => bitcoin::Network::Bitcoin,
            Network::Testnet => bitcoin::Network::Testnet,
            Network::Regtest => bitcoin::Network::Regtest,
            Network::Signet => bitcoin::Network::Signet,
        }
    }
}

/// Various options
#[derive(Debug, Clone)]
pub struct Opts {
    /// Which Bitcoin network to connect to.
    pub network: Network,
    /// Whether to broadcast through Tor if a local instance of it is found running.
    pub use_tor: TorMode,
    /// Which strategy to use to find the pool to draw peers from.
    pub find_peer_strategy: FindPeerStrategy,
    /// The maximum allowed duration for broadcasting regardless of the result. Terminates afterward.
    pub max_time: std::time::Duration,
    /// Normally, no transaction should be sent to a peer without first sending an `Inv` message
    /// advertising the transaction and then waiting for the peer to respond with a `GetData`
    /// message indicating that it does not indeed have the transaction. However, if we are certain
    /// that our transactions have not been seen by the network, we can short-circuit this process
    /// and simply send them out without the `Inv`-`GetData` exchange.
    pub send_unsolicited: bool,
    /// Whether to simulate the broadcast. This means that every part of the process will be
    /// executed as normal, including connecting to actual peers, but the final part where the tx
    /// is sent out is omitted (we pretend that the transaction really did go out.)
    pub dry_run: bool,
    /// How many peers to connect to.
    pub target_peers: u8,
    /// Custom user agent, POSIX time (secs) and block height to send during peer handshakes.
    /// Exercise caution modifying this.
    pub ua: Option<(String, u64, u64)>,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            network: Network::default(),
            use_tor: Default::default(),
            find_peer_strategy: Default::default(),
            max_time: std::time::Duration::from_secs(40),
            send_unsolicited: false,
            dry_run: false,
            target_peers: 10,
            ua: None,
        }
    }
}

/// Informational messages about the broadcast process.
#[derive(Debug, Clone)]
pub enum Info {
    /// Resolving peers from DNS or fixed peer list.
    ResolvingPeers,
    /// How many peers were resolved.
    ResolvedPeers(usize),
    /// Connecting to the p2p network.
    ConnectingToNetwork { tor_status: Option<SocketAddr> },
    /// A tx broadcast to a particular peer was completed.
    Broadcast { peer: String },
    /// The broadcast process is done.
    Done(Result<Report, Error>),
}

/// An informational report on a successful broadcast process.
#[derive(Debug, Clone)]
pub struct Report {
    /// How many peers we managed to broadcast to.
    pub broadcasts: NonZeroUsize,
    /// How many rejects we got back.
    pub rejects: usize,
}

/// Possible error variants while broadcasting.
#[derive(Debug, Clone)]
pub enum Error {
    TorNotFound,
    Timeout,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::TorNotFound => write!(f, "Tor was required but a Tor proxy was not found"),
            Error::Timeout => write!(f, "Time out"),
        }
    }
}

/// Connects to the p2p network and broadcasts a series of transactions. This runs fully in the
/// background. Network and other parameters can be set through the `opts` argument.
///
/// Returns a channel where status updates may be read.
pub fn broadcast(tx: Vec<Transaction>, opts: Opts) -> crossbeam_channel::Receiver<Info> {
    let (broadcaster, event_rx) = broadcast::Runner::new(tx, opts);
    broadcaster.run();
    event_rx
}
