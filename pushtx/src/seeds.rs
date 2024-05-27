use std::net::SocketAddr;

use crate::{net::Service, Network};

const FIXED_MAINNET: &str = include_str!("../seeds/mainnet.txt");
const FIXED_TESTNET: &str = include_str!("../seeds/testnet.txt");
const FIXED_SIGNET: &str = include_str!("../seeds/signet.txt");

const DNS_MAINNET: &[&str] = &[
    "dnsseed.bluematt.me.",
    "dnsseed.bitcoin.dashjr-list-of-p2p-nodes.us.",
    "seed.bitcoinstats.com.",
    "seed.bitcoin.jonasschnelli.ch.",
    "seed.btc.petertodd.net.",
    "seed.bitcoin.sprovoost.nl.",
    "dnsseed.emzy.de.",
    "seed.bitcoin.wiz.biz.",
];

const DNS_TESTNET: &[&str] = &[
    "testnet-seed.bluematt.me",
    "testnet-seed.bitcoin.jonasschnelli.ch",
    "seed.tbtc.petertodd.org",
    "seed.testnet.bitcoin.sprovoost.nl",
];

const DNS_SIGNET: &[&str] = &["seed.signet.bitcoin.sprovoost.nl"];

/// Returns nodes returned by DNS seeds.
pub fn dns(network: Network) -> Vec<Service> {
    let (seeds, port): (&[_], _) = match network {
        Network::Mainnet => (DNS_MAINNET, 8333),
        Network::Testnet => (DNS_TESTNET, 18333),
        Network::Regtest => (&[], 18444),
        Network::Signet => (DNS_SIGNET, 38333),
    };

    seeds
        .iter()
        .map(|seed| {
            std::thread::spawn(move || {
                let mut addrs: Vec<Service> = Vec::with_capacity(128);
                if let Ok(iter) = dns_lookup::getaddrinfo(Some(seed), None, None) {
                    for addr in iter.filter_map(Result::ok) {
                        let socket_addr: SocketAddr = (addr.sockaddr.ip(), port).into();
                        addrs.push(socket_addr.into());
                    }
                }
                addrs
            })
        })
        .filter_map(|h| h.join().ok())
        .fold(Vec::with_capacity(1024), |mut acc, val| {
            acc.extend(val);
            acc
        })
}

/// Returns an iterator over hardcoded seed nodes.
pub fn fixed(network: Network) -> impl Iterator<Item = Service> {
    match network {
        Network::Mainnet => parse_fixed(FIXED_MAINNET),
        Network::Testnet => parse_fixed(FIXED_TESTNET),
        Network::Regtest => parse_fixed(""),
        Network::Signet => parse_fixed(FIXED_SIGNET),
    }
}

/// Parses a string containing seed nodes, one per line, and returns an iterator over it.
fn parse_fixed(s: &'static str) -> impl Iterator<Item = Service> {
    s.lines().filter_map(|line| {
        line.split_whitespace()
            .next()
            .and_then(|addr| addr.parse().ok())
    })
}
