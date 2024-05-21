## Privacy-focused Bitcoin Transaction Broadcaster

This is a Rust program that broadcasts Bitcoin transactions **directly into the P2P network** by
connecting to a set of random Bitcoin nodes. This differs from other broadcast tools in that it
does not not interact with any centralized services, such as block explorers.

The program is entirely self-contained and does not require Bitcoin Core or other dependencies.

If Tor is running on the same system, connectivity to the P2P network is established through a
newly created circuit. Having Tor Browser running in the background is sufficient. Tor daemon
also works.

### Broadcast Process

1. Resolve peers through DNS seeds.
2. Detect if Tor is present.
3. Connect to 10 random peers, through Tor if possible.
4. Broadcast the transaction.
5. Disconnect.

### Usage

Install with Cargo: `cargo install pushtx-cli`

![Demo](demo.gif)

A library is also available (`pushtx`).

### Disclaimer

This project comes with no warranty whatsoever. Please refer to the license for details.
