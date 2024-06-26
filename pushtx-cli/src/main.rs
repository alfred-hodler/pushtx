use pushtx::*;

use core::panic;
use std::collections::HashSet;
use std::io::{IsTerminal, Read};
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
///
/// Copyright (c) 2024 Alfred Hodler <alfred_hodler@protonmail.com>
#[derive(Parser)]
#[command(version, about, long_about, verbatim_doc_comment, name = "pushtx")]
struct Cli {
    /// Tor mode.
    #[arg(short = 'm', long, default_value_t = TorMode::Try)]
    tor_mode: TorMode,

    /// Dry-run mode. Performs the whole process except the sending part.
    #[arg(short, long)]
    dry_run: bool,

    /// The network to use.
    #[arg(short, long, default_value_t = Network::Mainnet)]
    network: Network,

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
            let stdin = std::io::stdin();
            if stdin.is_terminal() {
                eprintln!("Enter some hex-encoded transactions (one per line, Ctrl + {EOF_CHR} when done) ... ");
            }
            stdin
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

    let txids: HashSet<_> = txs.iter().map(|tx| tx.txid()).collect();

    let receiver = broadcast(
        txs,
        Opts {
            use_tor: cli.tor_mode.into(),
            network: cli.network.into(),
            dry_run: cli.dry_run,
            ..Default::default()
        },
    );

    loop {
        match receiver.recv() {
            Ok(Info::ResolvingPeers) => println!("* Resolving peers from DNS..."),
            Ok(Info::ResolvedPeers(n)) => println!("* Resolved {n} peers"),
            Ok(Info::ConnectingToNetwork { tor_status }) => {
                println!("* Connecting to the P2P network ({})...", cli.network);
                match tor_status {
                    Some(proxy) => println!("  - using Tor proxy found at {proxy}"),
                    None => println!("  - not using Tor"),
                }
            }
            Ok(Info::Broadcast { peer }) => println!("* Broadcast to peer {}", peer),
            Ok(Info::Done(Ok(Report { success, rejects }))) => {
                let difference: Vec<_> = txids.difference(&success).collect();
                if difference.is_empty() {
                    println!("* Done! Broadcast successful");
                    break Ok(());
                } else {
                    println!("* Failed to broadcast one or more transactions");
                    for missing in difference {
                        println!("  - failed: {missing}");
                    }
                    for (r_txid, r_reason) in rejects {
                        println!("  - reject: {r_txid}: {r_reason}");
                    }
                    break Err(Error::Partial.into());
                }
            }
            Ok(Info::Done(Err(error))) => {
                break Err(Error::Broadcast(error).into());
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
    Broadcast(pushtx::Error),
    #[error("Failed to broadcast one or more transactions")]
    Partial,
}

/// Determines how to use Tor.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum TorMode {
    /// Use Tor if available. If not available, connect through clearnet.
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

impl std::fmt::Display for TorMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            TorMode::Try => "try",
            TorMode::No => "no",
            TorMode::Must => "must",
        };
        write!(f, "{}", name)
    }
}

/// The Bitcoin network to connect to.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Network {
    Mainnet,
    Testnet,
    Signet,
}

impl From<Network> for pushtx::Network {
    fn from(value: Network) -> Self {
        match value {
            Network::Mainnet => Self::Mainnet,
            Network::Testnet => Self::Testnet,
            Network::Signet => Self::Signet,
        }
    }
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
            Network::Signet => "signet",
        };
        write!(f, "{}", name)
    }
}

const EOF_CHR: char = if cfg!(target_family = "windows") {
    'Z'
} else {
    'D'
};
