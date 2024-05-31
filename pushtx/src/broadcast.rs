use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr};
use std::time;
use std::time::Duration;

use crate::handshake::{self, Handshake};
use crate::p2p::{self, Outbox, Receiver, Sender};
use crate::{net, seeds, Error, FindPeerStrategy, Info, Opts, Report, Transaction};
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_blockdata::Inventory;
use crossbeam_channel::RecvTimeoutError;

/// Transaction broadcast runner. Needs to be constructed and started to run.
pub(crate) struct Runner {
    info_tx: crossbeam_channel::Sender<Info>,
    tx: Vec<Transaction>,
    opts: Opts,
}

impl Runner {
    /// Constructs a new broadcast runner without actually running it.
    /// The receiver allows the caller to follow the broadcast progress.
    pub fn new(tx: Vec<Transaction>, opts: Opts) -> (Self, crossbeam_channel::Receiver<Info>) {
        let (info_tx, info_rx) = crossbeam_channel::unbounded();
        let runner = Self { info_tx, tx, opts };

        (runner, info_rx)
    }

    /// Runs the broadcast in a background thread.
    pub fn run(self) {
        std::thread::spawn(move || {
            let (must_use_tor, proxy) = match self.opts.use_tor {
                crate::TorMode::No => (false, None),
                crate::TorMode::BestEffort => (false, detect_tor_proxy()),
                crate::TorMode::Must => (true, detect_tor_proxy()),
            };

            if self.opts.dry_run {
                log::warn!("dry run is enabled, broadcast is simulated");
            }

            log::info!("Tor proxy status: {:?}", proxy);
            if proxy.is_none() && must_use_tor {
                log::error!("Tor usage required but local proxy not found");
                let _ = self.info_tx.send(Info::Done(Err(Error::TorNotFound)));
                return;
            }

            let client = p2p::client(proxy, self.opts.network, self.opts.ua);
            let mut state = HashMap::new();

            let _ = self.info_tx.send(Info::ResolvingPeers);
            let networks: &[net::Network] = match proxy {
                Some(_) => &[net::Network::Ipv4, net::Network::Ipv6, net::Network::TorV3],
                None => &[net::Network::Ipv4],
            };
            let addressbook =
                create_node_pool(self.opts.find_peer_strategy, self.opts.network, networks);
            let _ = self.info_tx.send(Info::ResolvedPeers(addressbook.len()));

            let _ = self
                .info_tx
                .send(Info::ConnectingToNetwork { tor_status: proxy });

            let outbox = &client;
            for addr in addressbook.iter().take(self.opts.target_peers.into()) {
                outbox.connect(*addr);
            }
            outbox.send().unwrap();

            let tx_map: HashMap<_, _> = self.tx.into_iter().map(|tx| (tx.0.txid(), tx.0)).collect();
            let mut acks = HashSet::new();
            let mut selected: Option<BroadcastPeer<_>> = None;

            let start = time::Instant::now();
            let mut rejects = HashMap::new();

            loop {
                let mut need_replacements = 0;
                let p2p = client.receiver();

                match p2p.recv_timeout(Duration::from_secs(1)).map(Into::into) {
                    Ok(p2p::Event::ConnectedTo { target, result }) => match result {
                        Ok(id) => {
                            log::info!("connected: peer @ {target}");
                            state.insert(id, Peer::Handshaking(target, Handshake::default()));
                            outbox.version(id);
                        }
                        Err(_) => {
                            log::info!("failed to connect to peer @ {target}");
                            need_replacements += 1;
                        }
                    },

                    Ok(p2p::Event::Message { peer, message }) => match state.get_mut(&peer) {
                        Some(Peer::Handshaking(s, h)) => match h.update(message.payload().into()) {
                            handshake::Event::Wait => {}
                            handshake::Event::SendVerack => outbox.verack(peer),
                            handshake::Event::Violation => {
                                log::warn!("handshake violated: peer @ {}", s);
                                state.remove(&peer);
                                need_replacements += 1;
                            }
                            handshake::Event::Done { .. } => {
                                let service = *s;
                                log::info!("handshake complete: peer @ {}", s);
                                state.insert(peer, Peer::Ready { service });
                            }
                        },
                        Some(Peer::Ready { service }) => match message.payload() {
                            NetworkMessage::Inv(inv) => {
                                for inv in inv {
                                    if let Inventory::Transaction(wanted_txid) = inv {
                                        if tx_map.contains_key(wanted_txid)
                                            && selected.as_ref().map(|s| s.id) != Some(peer)
                                        {
                                            log::info!(
                                                "txid seen: peer @ {}: {}",
                                                service,
                                                wanted_txid
                                            );
                                            acks.insert(*wanted_txid);
                                        }
                                    }
                                }
                            }
                            NetworkMessage::Reject(reject) => {
                                log::warn!(
                                    "reject: peer @ {}: type={}, code={:?}, reason={}",
                                    service,
                                    reject.message,
                                    reject.ccode,
                                    reject.reason
                                );
                                if reject.message == "tx" {
                                    let txid = crate::Txid(reject.hash.into());
                                    rejects.insert(txid, reject.reason.to_string());
                                }
                            }
                            _ => {}
                        },
                        None => panic!("phantom peer {}", peer),
                    },

                    Ok(p2p::Event::Disconnected { peer, reason }) => match state.get_mut(&peer) {
                        Some(Peer::Ready { service } | Peer::Handshaking(service, _)) => {
                            log::info!("disconnected: peer @ {}, reason: {:?}", service, reason);
                            if selected.as_ref().map(|s| s.id) == Some(peer) {
                                selected = None;
                            }
                            need_replacements += 1;
                            state.remove(&peer);
                        }
                        None => panic!("phantom peer {}", peer),
                    },

                    Err(RecvTimeoutError::Disconnected) => panic!("p2p reactor disconnected"),

                    _ => {}
                }

                match &selected {
                    Some(selected) if selected.is_stale() => {
                        log::warn!("rotating broadcast peer");
                        outbox.disconnect(selected.id);
                    }
                    _ => {}
                }

                if selected.is_none() {
                    let new_selected = state
                        .iter()
                        .filter_map(|(id, p)| match p {
                            Peer::Handshaking(_, _) => None,
                            Peer::Ready { service } => Some((*service, *id)),
                        })
                        .next();

                    if let Some((service, id)) = new_selected {
                        log::info!("selected broadcast peer @ {service}");
                        selected = Some(BroadcastPeer::new(id));
                        for tx in tx_map.values() {
                            log::info!("broadcasting to {}", service);
                            if !self.opts.dry_run {
                                outbox.tx(id, tx.to_owned());
                            }
                        }
                        let _ = self.info_tx.send(Info::Broadcast {
                            peer: service.to_string(),
                        });
                    }
                }

                let elapsed = time::Instant::now() - start;

                if self.opts.dry_run && elapsed.as_secs() > 3 {
                    acks.extend(tx_map.keys());
                }

                if acks.len() == tx_map.len() || elapsed >= self.opts.max_time {
                    log::info!("broadcast stop");
                    break;
                }

                for _ in 0..need_replacements {
                    let replacement = fastrand::choice(addressbook.iter()).unwrap();
                    outbox.connect(*replacement);
                    log::info!("picked replacement peer @ {replacement}");
                }
                client.send().unwrap();
            }

            client.shutdown().join().unwrap().unwrap();
            let report = Ok(Report {
                success: acks.into_iter().map(crate::Txid).collect(),
                rejects,
            });
            let _ = self.info_tx.send(Info::Done(report));
        });
    }
}

/// Peer status.
enum Peer {
    /// Currently handshaking.
    Handshaking(net::Service, Handshake),
    /// Handshake established, ready for interaction.
    Ready { service: net::Service },
}

/// A single peer that we have selected for our transaction broadcast.
struct BroadcastPeer<P: p2p::Peerlike> {
    /// The id of the peer.
    id: P,
    /// The time the broadcast took place.
    when: std::time::Instant,
}

impl<P: p2p::Peerlike> BroadcastPeer<P> {
    fn new(id: P) -> Self {
        Self {
            id,
            when: std::time::Instant::now(),
        }
    }
    /// Whether the peer is stale and should be rotated.
    fn is_stale(&self) -> bool {
        std::time::Instant::now() - self.when > Duration::from_secs(10)
    }
}

/// Tries to detect a local Tor proxy on the usual ports.
fn detect_tor_proxy() -> Option<SocketAddr> {
    fn is_port_reachable(addr: SocketAddr) -> bool {
        std::net::TcpStream::connect(addr).is_ok()
    }

    // Tor daemon has a SOCKS proxy on port 9050
    if is_port_reachable((Ipv4Addr::LOCALHOST, 9050).into()) {
        return Some((Ipv4Addr::LOCALHOST, 9050).into());
    }

    // Tor browser has a SOCKS proxy on port 9150
    if is_port_reachable((Ipv4Addr::LOCALHOST, 9150).into()) {
        return Some((Ipv4Addr::LOCALHOST, 9150).into());
    }

    None
}

/// Creates a pool of nodes from where peers can be found.
fn create_node_pool(
    strategy: FindPeerStrategy,
    p2p_network: crate::Network,
    allowed_networks: &[net::Network],
) -> Vec<net::Service> {
    match strategy {
        FindPeerStrategy::DnsSeedWithFixedFallback | FindPeerStrategy::DnsSeedOnly => {
            let mut nodes = seeds::dns(p2p_network);
            if matches!(strategy, FindPeerStrategy::DnsSeedWithFixedFallback) && nodes.len() < 20 {
                nodes.extend(seeds::fixed(p2p_network));
            }
            fastrand::shuffle(&mut nodes);
            nodes
                .into_iter()
                .filter(|node| allowed_networks.iter().any(|net| node.on_network(*net)))
                .collect()
        }
        FindPeerStrategy::Custom(custom) => custom.into_iter().map(Into::into).collect(),
    }
}
