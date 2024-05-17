use pushtx::*;

use core::panic;
use std::io::Read;
use std::path::PathBuf;

use clap::Parser;

/// Bitcoin P2P Transaction Broadcaster.
///
/// This program connects directly to the Bitcoin P2P network,
/// selects a number of random peers through DNS and broadcasts
/// one or more transactions. If Tor is running on the same
/// system, by default it will attempt to connect through a
/// fresh Tor circuit. Running the Tor browser in the background
/// is usually sufficient for this to work.
///
/// Logging can be enabled by running the program with the
/// following environment variable: RUST_LOG=debug.
/// Available log levels are: trace, debug, info, warn, error.
#[derive(Parser)]
#[command(version, about, long_about, verbatim_doc_comment)]
struct Cli {
    /// Connect through clearnet even if Tor is available.
    #[arg(short, long)]
    no_tor: bool,

    /// Dry-run mode. Performs the whole process except the sending part.
    #[arg(short, long)]
    dry_run: bool,

    /// Connect to testnet instead of mainnet.
    #[arg(short, long)]
    testnet: bool,

    /// Zero or one paths to a file containing line-delimited hex encoded or binary transactions
    ///
    /// If not present, stdin is used instead (hex only, one tx per line).
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    txs: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    let txs: Result<Vec<_>, Error> = match cli.txs {
        Some(path) => {
            let mut contents = String::new();
            let mut file = std::fs::File::open(path)?;
            file.read_to_string(&mut contents)?;
            contents
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| pushtx::Transaction::from_hex(line).map_err(Into::into))
                .collect()
        }
        None => std::io::stdin()
            .lines()
            .filter_map(|line| match line {
                Ok(line) if !line.trim().is_empty() => {
                    Some(pushtx::Transaction::from_hex(line).map_err(Into::into))
                }
                Ok(_) => None,
                Err(err) => Some(Err(Error::Io(err))),
            })
            .collect(),
    };

    if cli.dry_run {
        println!("! ** DRY RUN MODE **");
    }

    let txs = match txs {
        Ok(txs) => {
            if !txs.is_empty() {
                println!("* The following transactions will be broadcast:");
                for tx in &txs {
                    println!("  - {}", tx.txid())
                }
                Ok(txs)
            } else {
                Err(Error::EmptyTxSet)
            }
        }
        Err(err) => Err(err),
    }?;

    let use_tor = if cli.no_tor {
        pushtx::UseTor::No
    } else {
        pushtx::UseTor::BestEffort
    };

    let receiver = broadcast(
        txs,
        Opts {
            use_tor,
            network: if cli.testnet {
                Network::Testnet
            } else {
                Network::Mainnet
            },
            send_unsolicited: true,
            dry_run: cli.dry_run,
            ..Default::default()
        },
    );

    loop {
        match receiver.recv() {
            Ok(Info::ResolvingPeers) => println!("* Resolving peers from DNS..."),
            Ok(Info::ResolvedPeers(n)) => println!("* Resolved {n} peers"),
            Ok(Info::ConnectingToNetwork { tor_status }) => {
                let network = if cli.testnet { "testnet" } else { "mainnet" };
                println!("* Connecting to the P2P network ({network})...");
                match tor_status {
                    Some(proxy) => println!("  - using Tor proxy found at {proxy}"),
                    None => println!("  - not using Tor"),
                }
            }
            Ok(Info::Broadcast { peer }) => println!("* Successful broadcast to peer {}", peer),
            Ok(Info::Done {
                broadcasts,
                rejects,
            }) => {
                if broadcasts > 0 {
                    println!("* Done! Broadcast to {broadcasts} peers with {rejects} rejections");
                    break Ok(());
                } else {
                    break Err(Error::FailedToBroadcast.into());
                }
            }
            Err(_) => panic!("worker thread disconnected"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("IO error while parsing transaction(s): {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Parse(#[from] pushtx::ParseTxError),
    #[error("Empty transaction set, did you pass at least one transaction?")]
    EmptyTxSet,
    #[error("Failed to broadcast to any peers")]
    FailedToBroadcast,
}
