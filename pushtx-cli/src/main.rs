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
/// More verbose (debug) output can be enabled by specifying the
/// -v or --verbose switch up to three times.
#[derive(Parser)]
#[command(version, about, long_about, verbatim_doc_comment, name = "pushtx")]
struct Cli {
    /// Tor mode. Default is `try`.
    #[arg(short = 'm', long)]
    tor_mode: Option<TorMode>,

    /// Dry-run mode. Performs the whole process except the sending part.
    #[arg(short, long)]
    dry_run: bool,

    /// Connect to testnet instead of mainnet.
    #[arg(short, long)]
    testnet: bool,

    /// Zero or one paths to a file containing line-delimited hex encoded transactions
    ///
    /// If not present, stdin is used instead (hex only, one tx per line).
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    txs: Option<PathBuf>,

    /// Print debug info (use multiple times for more verbosity; max 3)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let log_level = match cli.verbose {
        0 => None,
        1 => Some(log::Level::Info),
        2 => Some(log::Level::Debug),
        3.. => Some(log::Level::Trace),
    };

    if let Some(level) = log_level {
        env_logger::Builder::default()
            .filter_level(level.to_level_filter())
            .init();
    }

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
        None => {
            eprintln!("Go ahead and paste some hex-encoded transactions (one per line) ... ");
            std::io::stdin()
                .lines()
                .filter_map(|line| match line {
                    Ok(line) if !line.trim().is_empty() => {
                        Some(pushtx::Transaction::from_hex(line).map_err(Into::into))
                    }
                    Ok(_) => None,
                    Err(err) => Some(Err(Error::Io(err))),
                })
                .collect()
        }
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

    let receiver = broadcast(
        txs,
        Opts {
            use_tor: cli.tor_mode.unwrap_or_default().into(),
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
            Ok(Info::Done(Ok(Report {
                broadcasts,
                rejects,
            }))) => {
                println!("* Done! Broadcast to {broadcasts} peers with {rejects} rejections");
                break Ok(());
            }
            Ok(Info::Done(Err(error))) => {
                break Err(Error::FailedToBroadcast(error).into());
            }
            Err(_) => panic!("worker thread disconnected"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("IO error while reading transaction(s): {0}")]
    Io(#[from] std::io::Error),
    #[error("Error while parsing transaction(s): {0}")]
    Parse(#[from] pushtx::ParseTxError),
    #[error("Empty transaction set, did you pass at least one transaction?")]
    EmptyTxSet,
    #[error("Failed to broadcast: {0}")]
    FailedToBroadcast(pushtx::Error),
}

/// Determines how to use Tor.
#[derive(Debug, Default, Clone, clap::ValueEnum)]
pub enum TorMode {
    /// Use Tor if available. If not available, connect through clearnet.
    #[default]
    Try,
    /// Do not use Tor even if available and running.
    No,
    /// Exclusively use Tor. If not available, do not broadcast.
    Must,
}

impl From<TorMode> for pushtx::TorMode {
    fn from(value: TorMode) -> Self {
        match value {
            TorMode::Try => Self::BestEffort,
            TorMode::No => Self::No,
            TorMode::Must => Self::Must,
        }
    }
}
