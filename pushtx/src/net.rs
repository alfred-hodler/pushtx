//! Network related types (supported networks,  addresses etc.)

use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    str::FromStr,
};

/// Supported network.
#[derive(Debug, Clone, Copy)]
#[allow(unused)]
pub enum Network {
    /// IPv4.
    Ipv4,
    /// IPv6.
    Ipv6,
    /// Onion V3.
    TorV3,
}

/// Address variant.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Address {
    /// IPv4.
    Ipv4(Ipv4Addr),
    /// IPv6.
    Ipv6(Ipv6Addr),
    /// Onion V3.
    TorV3([u8; 32]),
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Address::Ipv4(ip) => write!(f, "{}", ip),
            Address::Ipv6(ip) => write!(f, "{}", ip),
            Address::TorV3(pk) => write!(f, "{}", tor::v3_pubkey_to_domain(pk)),
        }
    }
}

/// The combination of `Address` and port describing a peer/node/service on the network.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct Service(Address, u16);

impl Service {
    /// Whether the service is on a particular network.
    pub fn on_network(&self, network: Network) -> bool {
        matches!(
            (self.0, network),
            (Address::Ipv4(_), Network::Ipv4)
                | (Address::Ipv6(_), Network::Ipv6)
                | (Address::TorV3(_), Network::TorV3)
        )
    }
}

impl From<SocketAddr> for Service {
    fn from(value: SocketAddr) -> Self {
        match value {
            SocketAddr::V4(socket) => Self(Address::Ipv4(*socket.ip()), socket.port()),
            SocketAddr::V6(socket) => Self(Address::Ipv6(*socket.ip()), socket.port()),
        }
    }
}

impl FromStr for Service {
    type Err = InvalidConnectTarget;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(socket) = s.parse::<SocketAddr>() {
            Ok(socket.into())
        } else {
            let (domain, port) = s.trim().rsplit_once(':').ok_or(InvalidConnectTarget)?;
            if let Some((pk, port)) = tor::v3_domain_to_pk(domain).zip(port.parse().ok()) {
                Ok(Service(Address::TorV3(pk), port))
            } else {
                Err(InvalidConnectTarget)
            }
        }
    }
}

impl peerlink::connector::IntoTarget for Service {
    fn target(&self) -> Option<peerlink::connector::Target> {
        use peerlink::connector::Target;
        let (addr, port) = (self.0, self.1);
        match addr {
            Address::Ipv4(ip) => Some(Target::Socket((ip, port).into())),
            Address::Ipv6(ip) => Some(Target::Socket((ip, port).into())),
            Address::TorV3(pk) => Some(Target::Domain(tor::v3_pubkey_to_domain(&pk), port)),
        }
    }
}

impl std::fmt::Display for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.0, self.1)
    }
}

/// The value cannot be interpreted as a valid connect target.
#[derive(Debug)]
pub struct InvalidConnectTarget;

/// The network type is not supported by the application.
#[derive(Debug)]
pub struct UnsupportedNetworkError;

impl TryFrom<&bitcoin::p2p::Address> for Service {
    type Error = UnsupportedNetworkError;

    fn try_from(value: &bitcoin::p2p::Address) -> Result<Self, Self::Error> {
        match value.socket_addr() {
            Ok(socket) => Ok(socket.into()),
            Err(_) => Err(UnsupportedNetworkError),
        }
    }
}

impl TryFrom<&bitcoin::p2p::address::AddrV2Message> for Service {
    type Error = UnsupportedNetworkError;

    fn try_from(value: &bitcoin::p2p::address::AddrV2Message) -> Result<Self, Self::Error> {
        match value.addr {
            bitcoin::p2p::address::AddrV2::Ipv4(ip) => Ok(Self(Address::Ipv4(ip), value.port)),
            bitcoin::p2p::address::AddrV2::Ipv6(ip) => Ok(Self(Address::Ipv6(ip), value.port)),
            bitcoin::p2p::address::AddrV2::TorV3(pk) => Ok(Self(Address::TorV3(pk), value.port)),
            _ => Err(UnsupportedNetworkError),
        }
    }
}

mod tor {
    const V3_VERSION: u8 = 0x03;
    const TOR_V3_ADDR_LEN: usize = 62;
    const CHECKSUM_CONST: &[u8; 15] = b".onion checksum";

    /// Converts an Onion V3 public key to an .onion domain.
    pub fn v3_pubkey_to_domain(pk: &[u8; 32]) -> String {
        let checksum = calc_checksum(pk);

        let mut address = [0_u8; 35];
        address[0..32].copy_from_slice(pk);
        address[32..34].copy_from_slice(&checksum[0..2]);
        address[34] = V3_VERSION;

        let mut encoded = String::with_capacity(TOR_V3_ADDR_LEN);
        data_encoding::BASE32.encode_append(&address, &mut encoded);
        encoded.make_ascii_lowercase();
        encoded.push_str(".onion");

        encoded
    }

    /// Tries to convert an Onion V3 domain into a V3 public key.
    pub fn v3_domain_to_pk(domain: &str) -> Option<[u8; 32]> {
        let (addr, tld) = domain.trim().rsplit_once('.')?;

        if matches!(tld, "onion" | "ONION") {
            let mut bytes: [u8; 56] = addr.as_bytes().try_into().ok()?;
            bytes.make_ascii_uppercase();

            let mut decoded = [0_u8; 35];
            data_encoding::BASE32
                .decode_mut(&bytes, &mut decoded)
                .ok()?;

            let pk = &decoded[0..32];
            let checksum = &decoded[32..34];
            let version = decoded[34];

            let exp_checksum = &calc_checksum(pk.try_into().unwrap())[0..2];

            if version == V3_VERSION && exp_checksum == checksum {
                Some(pk.try_into().unwrap())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Calculates an Onion address checksum.
    fn calc_checksum(pk: &[u8; 32]) -> [u8; 32] {
        use sha3::{Digest, Sha3_256};

        let mut preimage = [0_u8; 15 + 32 + 1];
        preimage[0..CHECKSUM_CONST.len()].copy_from_slice(CHECKSUM_CONST);
        preimage[CHECKSUM_CONST.len()..CHECKSUM_CONST.len() + pk.len()].copy_from_slice(pk);
        preimage[CHECKSUM_CONST.len() + pk.len()] = 0x03;

        let digest = Sha3_256::digest(preimage);

        digest.into()
    }

    #[test]
    fn onion_pubkey_to_domain_roundtrip() {
        let domain = "2gzyxa5ihm7nsggfxnu52rck2vv4rvmdlkiu3zzui5du4xyclen53wid.onion";

        let pk: &[u8; 32] = &[
            209, 179, 139, 131, 168, 59, 62, 217, 24, 197, 187, 105, 221, 68, 74, 213, 107, 200,
            213, 131, 90, 145, 77, 231, 52, 71, 71, 78, 95, 2, 89, 27,
        ];

        assert_eq!(v3_pubkey_to_domain(pk), domain);
        assert_eq!(v3_domain_to_pk(domain), Some(pk.to_owned()));
    }
}
