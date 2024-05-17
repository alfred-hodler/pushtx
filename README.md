[![Documentation](https://img.shields.io/docsrs/pushtx)](https://docs.rs/pushtx/latest/pushtx/)
[![Crates.io](https://img.shields.io/crates/v/pushtx.svg)](https://crates.io/crates/pushtx)
[![License](https://img.shields.io/crates/l/pushtx.svg)](https://github.com/alfred-hodler/pushtx/blob/master/LICENSE)

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

```bash
$ pushtx -f ~/path_to_tx_file.hex

* The following transactions will be broadcast:
  - fc0b9ac3a5734cdcbb3e693094c54da2b2d315dc4fd36d8122782f78e3a59f4f
  - ea9d588eeeaff1d691cfdabd5fd0a0f70777375191348de90047c5ea300f402b
  - c30d8f90456f39175dbdd3c96779014f6e3fb6fd9d10eb518fc35c889c9e1912
* Resolving peers from DNS...
* Resolved 291 peers
* Connecting to the P2P network (testnet)...
  - using Tor proxy found at 127.0.0.1:9050
* Successful broadcast to peer 57.128.16.147:18333
* Successful broadcast to peer 2001:638:a000:4140::ffff:47:18333
* Successful broadcast to peer 135.181.78.217:18333
* Successful broadcast to peer 2600:3c02::f03c:93ff:fe4b:c543:18333
* Successful broadcast to peer 2600:3c02::f03c:93ff:fe4b:c543:18333
* Successful broadcast to peer 71.13.92.62:18333
* Done! Broadcast to 6 peers with 0 rejections
```

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
