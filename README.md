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
4. Broadcast the transaction.
5. Disconnect.

### Executable

Install with Cargo: `cargo install pushtx-cli`

![Demo](pushtx-cli/demo.gif)

### Library

```rust
 // our hex-encoded transaction that we want to parse and broadcast
 let tx = "6afcc7949dd500000....".parse().unwrap();

 // we start the broadcast process and acquire a receiver to the info events
 let receiver = pushtx::broadcast(vec![tx], pushtx::Opts::default());

 // start reading info events until `Done` is received
 let how_many = loop {
     match receiver.recv().unwrap() {    
         pushtx::Info::Done { broadcasts, .. } => break broadcasts,
         _ => {}
     }
 };

 println!("we successfully broadcast to {how_many} peers");
```

### Disclaimer

This project comes with no warranty whatsoever. Please refer to the license for details.
