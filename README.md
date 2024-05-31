[![Documentation](https://img.shields.io/docsrs/pushtx)](https://docs.rs/pushtx/latest/pushtx/)
[![Crates.io](https://img.shields.io/crates/v/pushtx.svg)](https://crates.io/crates/pushtx)
[![License](https://img.shields.io/crates/l/pushtx.svg)](https://github.com/alfred-hodler/pushtx/blob/master/LICENSE)
[![Test Status](https://github.com/alfred-hodler/pushtx/actions/workflows/rust.yml/badge.svg?branch=master)](https://github.com/alfred-hodler/pushtx/actions)

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
4. Broadcast the transaction to a single peer.
5. Wait until the transaction is seen on the network.
6. Disconnect.

### Executable

Install with Cargo: `cargo install pushtx-cli`

![Demo](pushtx-cli/demo.gif)

### Library

```rust
 // this is our hex-encoded transaction that we want to parse and broadcast
 let tx = "6afcc7949dd500000....".parse().unwrap();

 // we start the broadcast process and acquire a receiver to the info events
 let receiver = pushtx::broadcast(vec![tx], pushtx::Opts::default());

 // start reading info events until `Done` is received
 loop {
     match receiver.recv().unwrap() {
         pushtx::Info::Done(Ok(report)) => {
             println!("{} transactions broadcast successfully", report.success.len());
             break;
         }
         pushtx::Info::Done(Err(err)) => {
             println!("we failed to broadcast to any peers, reason = {err}");
             break;
         }
         _ => {}
     }
 }
```

### Disclaimer

This project comes with no warranty whatsoever. Please refer to the license for details.
