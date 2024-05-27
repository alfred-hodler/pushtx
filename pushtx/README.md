## Privacy-focused Bitcoin Transaction Broadcaster

This is a Rust crate that broadcasts Bitcoin transactions **directly into the P2P network** by
connecting to a set of random Bitcoin nodes. This differs from other broadcast tools in that it
does not not interact with any centralized services, such as block explorers.

The library is entirely self-contained and does not require Bitcoin Core or other dependencies.

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

```rust
 // this is our hex-encoded transaction that we want to parse and broadcast
 let tx = "6afcc7949dd500000....".parse().unwrap();

 // we start the broadcast process and acquire a receiver to the info events
 let receiver = pushtx::broadcast(vec![tx], pushtx::Opts::default());

 // start reading info events until `Done` is received
 loop {
     match receiver.recv().unwrap() {
         pushtx::Info::Done(Ok(report)) => {
             println!("we successfully broadcast to {} peers", report.broadcasts);
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

An executable is also available (`pushtx-cli`).

### Disclaimer

This project comes with no warranty whatsoever. Please refer to the license for details.
