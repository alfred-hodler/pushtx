use std::collections::HashMap;
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

            let start = time::Instant::now();
            let mut broadcasts = 0;
            let mut rejects = 0;

            loop {
                let mut need_replacements = 0;
                let p2p = client.receiver();

                match p2p.recv_timeout(Duration::from_secs(1)).map(Into::into) {
                    Ok(p2p::Event::ConnectedTo { target, result }) => match result {
                        Ok(id) => {
                            log::info!("connected to peer @ {target}");
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
                                log::warn!("peer {} violated handshake", s);
                                state.remove(&peer);
                                need_replacements += 1;
                            }
                            handshake::Event::Done { .. } => {
                                log::info!("handshake with {} done", s);
                                let service = *s;
                                let used;
                                if self.opts.send_unsolicited {
                                    used = true;
                                    for tx in tx_map.values() {
                                        log::info!("sending tx to {}", service);
                                        if !self.opts.dry_run {
                                            outbox.tx(peer, tx.to_owned());
                                        }
                                        broadcasts += 1;
                                        let _ = self.info_tx.send(Info::Broadcast {
                                            peer: service.to_string(),
                                        });
                                    }
                                } else {
                                    used = false;
                                    outbox.tx_inv(peer, tx_map.keys().cloned());
                                }
                                state.insert(peer, Peer::Ready { service, used });
                            }
                        },
                        Some(Peer::Ready { service, used }) => match message.payload() {
                            NetworkMessage::GetData(inv) => {
                                for inv in inv {
                                    if let Inventory::Transaction(wanted_txid) = inv {
                                        if let Some(tx) = tx_map.get(wanted_txid) {
                                            if !self.opts.dry_run {
                                                outbox.tx(peer, tx.to_owned());
                                            }
                                            *used = true;
                                            broadcasts += 1;
                                            let _ = self.info_tx.send(Info::Broadcast {
                                                peer: service.to_string(),
                                            });
                                        }
                                    }
                                }
                            }
                            NetworkMessage::Reject(reject) => {
                                log::warn!(
                                    "got a reject from {}: type={}, code={:?}, reason={}",
                                    service,
                                    reject.message,
                                    reject.ccode,
                                    reject.reason
                                );
                                if reject.message == "tx" {
                                    rejects += 1;
                                }
                            }
                            _ => {}
                        },
                        None => panic!("phantom peer {}", peer),
                    },

                    Ok(p2p::Event::Disconnected { peer, .. }) => match state.get_mut(&peer) {
                        Some(
                            Peer::Ready {
                                service,
                                used: false,
                            }
                            | Peer::Handshaking(service, _),
                        ) => {
                            log::info!("peer @ {service} left without letting us broadcast");
                            need_replacements += 1;
                            state.remove(&peer);
                        }
                        Some(_) => {
                            state.remove(&peer);
                        }
                        None => panic!("phantom peer {}", peer),
                    },

                    Err(RecvTimeoutError::Disconnected) => panic!("p2p reactor disconnected"),

                    _ => {}
                }

                // The strategy is as follows: we exponentially care less about each subsequent
                // broadcast, so we add 2^broadcasts to the elapsed time.
                let now = time::Instant::now();
                let elapsed = (now - start) + Duration::from_secs(1 << broadcasts);
                if elapsed >= self.opts.max_time {
                    log::info!(
                        "spent {} secs with {} broadcasts, exit",
                        (now - start).as_secs(),
                        broadcasts
                    );
                    break;
                }

                for _ in 0..need_replacements {
                    let replacement = fastrand::choice(addressbook.iter()).unwrap();
                    outbox.connect(*replacement);
                    log::info!("picked replacement peer @ {replacement}");
                }
                client.send().unwrap();
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
            client.shutdown().join().unwrap().unwrap();
            let done = match broadcasts.try_into() {
                Ok(broadcasts) => Ok(Report {
                    broadcasts,
                    rejects,
                }),
                Err(_) => Err(Error::Timeout),
            };
            let _ = self.info_tx.send(Info::Done(done));
        });
    }
}

/// Peer status.
enum Peer {
    /// Currently handshaking.
    Handshaking(net::Service, Handshake),
    /// Handshake established, ready for interaction.
    Ready { service: net::Service, used: bool },
}

/// Tries to detect a local Tor proxy on the usual ports.
fn detect_tor_proxy() -> Option<SocketAddr> {
    // Tor daemon has a SOCKS proxy on port 9050
    if port_check::is_port_reachable((Ipv4Addr::LOCALHOST, 9050)) {
        return Some((Ipv4Addr::LOCALHOST, 9050).into());
    }

    // Tor browser has a SOCKS proxy on port 9150
    if port_check::is_port_reachable((Ipv4Addr::LOCALHOST, 9150)) {
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
