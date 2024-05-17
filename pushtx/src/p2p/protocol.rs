use bitcoin::consensus::{encode, Encodable};
use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use peerlink::DecodeError;

#[derive(Debug)]
pub struct Message(pub RawNetworkMessage);

impl peerlink::Message for Message {
    fn encode(&self, dest: &mut impl std::io::Write) -> usize {
        self.0.consensus_encode(dest).unwrap()
    }

    fn decode(buffer: &[u8]) -> Result<(Self, usize), peerlink::DecodeError> {
        let payload_size = buffer.get(16..20).ok_or(DecodeError::NotEnoughData)?;

        let payload_size =
            encode::deserialize::<u32>(payload_size).expect("4 bytes -> u32 cannot fail") as usize;

        if 24 + payload_size > bitcoin::p2p::message::MAX_MSG_SIZE {
            Err(DecodeError::MalformedMessage)
        } else if buffer.len() < 24 + payload_size {
            Err(DecodeError::NotEnoughData)
        } else {
            match encode::deserialize_partial(buffer) {
                Ok((msg, consumed)) => Ok((Self(msg), consumed)),
                Err(_) => Err(DecodeError::MalformedMessage),
            }
        }
    }
}

impl From<(bitcoin::Network, NetworkMessage)> for Message {
    fn from((network, message): (bitcoin::Network, NetworkMessage)) -> Self {
        Self(RawNetworkMessage::new(network.magic(), message))
    }
}
