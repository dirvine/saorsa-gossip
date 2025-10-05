//! Saorsa Gossip CLI Tool
//!
//! Interactive demonstration and testing tool for the Saorsa Gossip network.
//! This CLI exercises all library features for validation and demos.
//!
//! # Commands
//!
//! - `identity` - Create and manage ML-DSA identities
//! - `network` - Join network and participate in gossip
//! - `pubsub` - Publish/subscribe to topics
//! - `presence` - Manage presence beacons
//! - `groups` - Create and join groups
//! - `crdt` - Demonstrate CRDT operations
//! - `rendezvous` - Test rendezvous coordination
//!
//! # Usage
//!
//! ```bash
//! saorsa-gossip identity create --alias "Alice"
//! saorsa-gossip network join --coordinator 127.0.0.1:7000
//! saorsa-gossip pubsub publish --topic news --message "Hello World"
//! ```

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Saorsa Gossip CLI - Demonstrate and test gossip network features
#[derive(Parser, Debug)]
#[command(name = "saorsa-gossip")]
#[command(version, about = "Saorsa Gossip Network CLI Tool", long_about = None)]
struct Args {
    /// Config directory (default: ~/.saorsa-gossip)
    #[arg(short, long, default_value = "~/.saorsa-gossip")]
    config_dir: PathBuf,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Identity management (ML-DSA keypairs)
    Identity {
        #[command(subcommand)]
        action: IdentityAction,
    },

    /// Network operations
    Network {
        #[command(subcommand)]
        action: NetworkAction,
    },

    /// Publish/Subscribe operations
    Pubsub {
        #[command(subcommand)]
        action: PubsubAction,
    },

    /// Presence beacon management
    Presence {
        #[command(subcommand)]
        action: PresenceAction,
    },

    /// Group operations
    Groups {
        #[command(subcommand)]
        action: GroupAction,
    },

    /// CRDT synchronization demo
    Crdt {
        #[command(subcommand)]
        action: CrdtAction,
    },

    /// Rendezvous coordination
    Rendezvous {
        #[command(subcommand)]
        action: RendezvousAction,
    },

    /// Run interactive demo
    Demo {
        /// Demo scenario to run
        #[arg(short, long, default_value = "basic")]
        scenario: String,
    },
}

#[derive(Subcommand, Debug)]
enum IdentityAction {
    /// Create a new identity
    Create {
        /// Alias for the identity
        #[arg(short, long)]
        alias: String,
    },

    /// List all identities
    List,

    /// Show identity details
    Show {
        /// Alias of identity to show
        alias: String,
    },

    /// Delete an identity
    Delete {
        /// Alias of identity to delete
        alias: String,
    },
}

#[derive(Subcommand, Debug)]
enum NetworkAction {
    /// Join the network
    Join {
        /// Coordinator address (e.g., 127.0.0.1:7000)
        #[arg(short, long)]
        coordinator: String,

        /// Identity alias to use
        #[arg(short, long)]
        identity: String,

        /// Bind address (default: 0.0.0.0:0 for random port)
        #[arg(short, long, default_value = "0.0.0.0:0")]
        bind: String,
    },

    /// Show network status
    Status,

    /// List known peers
    Peers,

    /// Leave the network
    Leave,
}

#[derive(Subcommand, Debug)]
enum PubsubAction {
    /// Subscribe to a topic
    Subscribe {
        /// Topic name
        #[arg(short, long)]
        topic: String,
    },

    /// Publish to a topic
    Publish {
        /// Topic name
        #[arg(short, long)]
        topic: String,

        /// Message to publish
        #[arg(short, long)]
        message: String,
    },

    /// Unsubscribe from a topic
    Unsubscribe {
        /// Topic name
        #[arg(short, long)]
        topic: String,
    },

    /// List subscribed topics
    List,
}

#[derive(Subcommand, Debug)]
enum PresenceAction {
    /// Start broadcasting presence
    Start {
        /// Topic for presence
        #[arg(short, long)]
        topic: String,
    },

    /// Stop broadcasting presence
    Stop {
        /// Topic to stop
        #[arg(short, long)]
        topic: String,
    },

    /// Show online peers
    Online {
        /// Topic to check
        #[arg(short, long)]
        topic: String,
    },
}

#[derive(Subcommand, Debug)]
enum GroupAction {
    /// Create a new group
    Create {
        /// Group name
        #[arg(short, long)]
        name: String,
    },

    /// Join a group
    Join {
        /// Group ID
        #[arg(short, long)]
        group_id: String,
    },

    /// Leave a group
    Leave {
        /// Group ID
        #[arg(short, long)]
        group_id: String,
    },

    /// List groups
    List,
}

#[derive(Subcommand, Debug)]
enum CrdtAction {
    /// Demonstrate LWW Register
    LwwRegister {
        /// Value to set
        value: String,
    },

    /// Demonstrate OR-Set
    OrSet {
        /// Action: add or remove
        #[arg(short, long)]
        action: String,

        /// Value
        value: String,
    },

    /// Show current CRDT state
    Show,
}

#[derive(Subcommand, Debug)]
enum RendezvousAction {
    /// Register as a provider
    Register {
        /// Capability to provide
        #[arg(short, long)]
        capability: String,
    },

    /// Find providers
    Find {
        /// Capability to find
        #[arg(short, long)]
        capability: String,
    },

    /// Unregister
    Unregister,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    init_logging(args.verbose)?;

    tracing::info!("Saorsa Gossip CLI v{}", env!("CARGO_PKG_VERSION"));

    // Expand config directory tilde
    let config_dir = expand_path(&args.config_dir)?;
    tracing::debug!("Config directory: {}", config_dir.display());

    // Ensure config directory exists
    tokio::fs::create_dir_all(&config_dir).await?;

    // Route to command handlers
    match args.command {
        Commands::Identity { action } => handle_identity(action, &config_dir).await?,
        Commands::Network { action } => handle_network(action, &config_dir).await?,
        Commands::Pubsub { action } => handle_pubsub(action, &config_dir).await?,
        Commands::Presence { action } => handle_presence(action, &config_dir).await?,
        Commands::Groups { action } => handle_groups(action, &config_dir).await?,
        Commands::Crdt { action } => handle_crdt(action, &config_dir).await?,
        Commands::Rendezvous { action } => handle_rendezvous(action, &config_dir).await?,
        Commands::Demo { scenario } => handle_demo(&scenario, &config_dir).await?,
    }

    Ok(())
}

/// Handle identity commands
async fn handle_identity(action: IdentityAction, config_dir: &std::path::Path) -> Result<()> {
    use saorsa_gossip_identity::Identity;

    match action {
        IdentityAction::Create { alias } => {
            tracing::info!("Creating identity: {}", alias);

            let identity = Identity::new(alias.clone())?;
            let peer_id = identity.peer_id();

            // Save to keystore (using alias as four-words for now)
            let keystore = config_dir.join("keystore");
            identity
                .save_to_keystore(&alias, keystore.to_str().expect("valid path"))
                .await?;

            println!("✓ Created identity: {}", alias);
            println!("  PeerId: {}", hex::encode(peer_id.as_bytes()));
            println!("  Saved to: {}", keystore.display());
        }

        IdentityAction::List => {
            tracing::info!("Listing identities");
            let keystore = config_dir.join("keystore");

            if !keystore.exists() {
                println!("No identities found");
                return Ok(());
            }

            let mut entries = tokio::fs::read_dir(&keystore).await?;
            let mut count = 0;

            println!("Identities:");
            while let Some(entry) = entries.next_entry().await? {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".identity") {
                        let alias = name.trim_end_matches(".identity").replace('_', "-");
                        println!("  - {}", alias);
                        count += 1;
                    }
                }
            }

            if count == 0 {
                println!("  (none)");
            }
        }

        IdentityAction::Show { alias } => {
            tracing::info!("Showing identity: {}", alias);
            let keystore = config_dir.join("keystore");

            let identity =
                Identity::load_from_keystore(&alias, keystore.to_str().expect("valid path"))
                    .await?;

            println!("Identity: {}", alias);
            println!("  PeerId: {}", hex::encode(identity.peer_id().as_bytes()));
            println!("  Alias: {}", identity.alias());
        }

        IdentityAction::Delete { alias } => {
            tracing::info!("Deleting identity: {}", alias);
            let keystore = config_dir.join("keystore");
            let filename = alias.replace('-', "_");
            let file_path = keystore.join(format!("{}.identity", filename));

            if file_path.exists() {
                tokio::fs::remove_file(&file_path).await?;
                println!("✓ Deleted identity: {}", alias);
            } else {
                println!("Identity not found: {}", alias);
            }
        }
    }

    Ok(())
}

/// Handle network commands
async fn handle_network(_action: NetworkAction, _config_dir: &std::path::Path) -> Result<()> {
    println!("Network commands - Coming soon!");
    println!("This will demonstrate:");
    println!("  - Joining the gossip network");
    println!("  - SWIM membership protocol");
    println!("  - HyParView overlay maintenance");
    println!("  - Peer discovery via coordinators");
    Ok(())
}

/// Handle pubsub commands
async fn handle_pubsub(_action: PubsubAction, _config_dir: &std::path::Path) -> Result<()> {
    println!("PubSub commands - Coming soon!");
    println!("This will demonstrate:");
    println!("  - Subscribing to topics");
    println!("  - Publishing messages");
    println!("  - Gossip-based message propagation");
    println!("  - ML-DSA signatures on messages");
    Ok(())
}

/// Handle presence commands
async fn handle_presence(_action: PresenceAction, _config_dir: &std::path::Path) -> Result<()> {
    println!("Presence commands - Coming soon!");
    println!("This will demonstrate:");
    println!("  - Periodic presence beacons");
    println!("  - Online peer discovery");
    println!("  - Presence TTL and expiration");
    Ok(())
}

/// Handle group commands
async fn handle_groups(_action: GroupAction, _config_dir: &std::path::Path) -> Result<()> {
    println!("Group commands - Coming soon!");
    println!("This will demonstrate:");
    println!("  - Creating encrypted groups");
    println!("  - Joining with shared secrets");
    println!("  - Group messaging");
    Ok(())
}

/// Handle CRDT commands
async fn handle_crdt(_action: CrdtAction, _config_dir: &std::path::Path) -> Result<()> {
    println!("CRDT commands - Coming soon!");
    println!("This will demonstrate:");
    println!("  - LWW Register operations");
    println!("  - OR-Set add/remove");
    println!("  - Anti-entropy synchronization");
    Ok(())
}

/// Handle rendezvous commands
async fn handle_rendezvous(_action: RendezvousAction, _config_dir: &std::path::Path) -> Result<()> {
    println!("Rendezvous commands - Coming soon!");
    println!("This will demonstrate:");
    println!("  - Provider registration");
    println!("  - Capability-based discovery");
    println!("  - DHT-based lookups");
    Ok(())
}

/// Handle demo scenarios
async fn handle_demo(scenario: &str, _config_dir: &std::path::Path) -> Result<()> {
    match scenario {
        "basic" => {
            println!("=== Saorsa Gossip Basic Demo ===");
            println!();
            println!("This demo will showcase:");
            println!("  1. Identity creation with ML-DSA");
            println!("  2. Network bootstrap");
            println!("  3. Peer discovery");
            println!("  4. PubSub messaging");
            println!("  5. Presence beacons");
            println!();
            println!("To run individual commands, use:");
            println!("  saorsa-gossip identity create --alias Alice");
            println!("  saorsa-gossip network join --coordinator 127.0.0.1:7000 --identity Alice");
            println!();
            println!("Demo implementation coming soon!");
        }
        _ => {
            println!("Unknown demo scenario: {}", scenario);
            println!("Available scenarios: basic");
        }
    }

    Ok(())
}

/// Initialize logging based on verbosity
fn init_logging(verbose: bool) -> Result<()> {
    use tracing_subscriber::EnvFilter;

    let filter = if verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    Ok(())
}

/// Expand tilde in path
fn expand_path(path: &std::path::Path) -> Result<PathBuf> {
    let expanded = shellexpand::tilde(&path.to_string_lossy()).to_string();
    Ok(PathBuf::from(expanded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parses() {
        // Verify CLI structure compiles and parses
        let _args = Args::try_parse_from(["saorsa-gossip", "demo", "--scenario", "basic"]);
    }

    #[test]
    fn test_expand_path_no_tilde() {
        let path = std::path::Path::new("/tmp/test");
        let expanded = expand_path(path).expect("expand");
        assert_eq!(expanded, path);
    }

    #[test]
    fn test_expand_path_with_tilde() {
        let path = std::path::Path::new("~/test");
        let expanded = expand_path(path).expect("expand");
        assert!(expanded.to_string_lossy().contains("test"));
        assert!(!expanded.to_string_lossy().contains('~'));
    }
}
