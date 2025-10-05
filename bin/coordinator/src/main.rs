//! Saorsa Gossip Coordinator Node
//!
//! This binary runs a bootstrap/coordinator node for the Saorsa Gossip network.
//! Per SPEC2 §8, coordinators provide:
//! - Bootstrap discovery
//! - Address reflection for NAT traversal
//! - Optional relay services
//! - Optional rendezvous services
//!
//! # Usage
//!
//! ```bash
//! coordinator --bind 0.0.0.0:7000 --role coordinator,reflector,relay
//! ```

use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Saorsa Gossip Coordinator Node
#[derive(Parser, Debug)]
#[command(name = "saorsa-coordinator")]
#[command(about = "Saorsa Gossip Network Coordinator Node", long_about = None)]
struct Args {
    /// Address to bind to (e.g., 0.0.0.0:7000)
    #[arg(short, long, default_value = "0.0.0.0:7000")]
    bind: SocketAddr,

    /// Coordinator roles (comma-separated): coordinator,reflector,relay,rendezvous
    #[arg(short, long, default_value = "coordinator,reflector")]
    roles: String,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Identity file path (ML-DSA keypair)
    #[arg(long, default_value = "~/.saorsa-gossip/coordinator.identity")]
    identity_path: PathBuf,

    /// Publish interval in seconds (coordinator adverts)
    #[arg(long, default_value = "300")]
    publish_interval: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    init_logging(args.verbose)?;

    tracing::info!("Starting Saorsa Gossip Coordinator");
    tracing::info!("Bind address: {}", args.bind);
    tracing::info!("Roles: {}", args.roles);

    // Parse roles
    let coordinator_roles = parse_roles(&args.roles)?;
    tracing::info!("Parsed roles: {:?}", coordinator_roles);

    // 1. Load or create identity
    let identity = load_or_create_identity(&args.identity_path).await?;
    tracing::info!(
        "Loaded identity: {}",
        hex::encode(identity.peer_id.as_bytes())
    );

    // 2. Start coordinator services based on roles
    if coordinator_roles.coordinator {
        tracing::info!("Starting coordinator advertisement service...");
        start_coordinator_service(
            &identity,
            &coordinator_roles,
            args.bind,
            args.publish_interval,
        )
        .await?;
    }

    if coordinator_roles.reflector {
        tracing::info!("Reflector role enabled (address observation)");
        // Address reflection is handled by ant-quic transport automatically
    }

    if coordinator_roles.relay {
        tracing::info!("Relay role enabled (message forwarding)");
        // TODO: Implement relay service with rate limiting
    }

    if coordinator_roles.rendezvous {
        tracing::info!("Rendezvous role enabled (connection coordination)");
        // TODO: Implement rendezvous coordination
    }

    tracing::info!("Coordinator node running. Press Ctrl+C to stop.");

    // 3. Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down coordinator...");

    Ok(())
}

/// Load or create a coordinator identity
async fn load_or_create_identity(path: &Path) -> Result<CoordinatorIdentity> {
    // Expand tilde in path
    let expanded_path = shellexpand::tilde(&path.to_string_lossy()).to_string();
    let identity_path = PathBuf::from(expanded_path);

    // Ensure parent directory exists
    if let Some(parent) = identity_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Try to load existing identity
    if identity_path.exists() {
        tracing::info!("Loading identity from: {}", identity_path.display());
        let identity_data = tokio::fs::read(&identity_path).await?;
        let keypair = saorsa_gossip_identity::MlDsaKeyPair::from_bytes(&identity_data)?;
        let peer_id = saorsa_gossip_types::PeerId::from_pubkey(keypair.public_key());

        Ok(CoordinatorIdentity { peer_id, keypair })
    } else {
        tracing::info!("Creating new identity at: {}", identity_path.display());
        let keypair = saorsa_gossip_identity::MlDsaKeyPair::generate()?;
        let peer_id = saorsa_gossip_types::PeerId::from_pubkey(keypair.public_key());

        // Save to disk
        let identity_data = keypair.to_bytes()?;
        tokio::fs::write(&identity_path, &identity_data).await?;

        tracing::info!("Identity saved to: {}", identity_path.display());
        Ok(CoordinatorIdentity { peer_id, keypair })
    }
}

/// Start the coordinator advertisement service
async fn start_coordinator_service(
    identity: &CoordinatorIdentity,
    roles: &CoordinatorRoles,
    bind_addr: SocketAddr,
    publish_interval_secs: u64,
) -> Result<()> {
    use saorsa_gossip_coordinator::{CoordinatorPublisher, NatClass, PeriodicPublisher};

    // Create publisher
    let publisher = CoordinatorPublisher::new(
        identity.peer_id,
        roles.clone().into(),
        vec![bind_addr],
        NatClass::Eim, // Default to EIM, can be detected via STUN
    );

    // Set signing key
    let secret_key = identity.keypair.get_secret_key_typed()?;
    publisher.set_signing_key(secret_key).await;

    // Start periodic publishing
    let periodic = PeriodicPublisher::new(publisher, publish_interval_secs);
    let mut advert_rx = periodic.start().await;

    // Spawn task to handle published adverts
    tokio::spawn(async move {
        tracing::info!(
            "Coordinator advert publisher started (interval: {}s)",
            publish_interval_secs
        );

        while let Some(advert_bytes) = advert_rx.recv().await {
            tracing::debug!(
                "Published coordinator advert ({} bytes)",
                advert_bytes.len()
            );
            // In a full implementation, this would be sent via pubsub transport
            // For now, we just log that it was published
            // TODO: Wire to actual transport pubsub.publish(coordinator_topic(), advert_bytes)
        }

        tracing::info!("Coordinator advert publisher stopped");
    });

    Ok(())
}

/// Coordinator identity wrapper
struct CoordinatorIdentity {
    peer_id: saorsa_gossip_types::PeerId,
    keypair: saorsa_gossip_identity::MlDsaKeyPair,
}

/// Parse coordinator roles from comma-separated string
fn parse_roles(roles_str: &str) -> Result<CoordinatorRoles> {
    let mut roles = CoordinatorRoles::default();

    for role in roles_str.split(',').map(|s| s.trim()) {
        match role.to_lowercase().as_str() {
            "coordinator" => roles.coordinator = true,
            "reflector" => roles.reflector = true,
            "relay" => roles.relay = true,
            "rendezvous" => roles.rendezvous = true,
            "" => {} // Skip empty
            unknown => {
                return Err(anyhow::anyhow!("Unknown role: {}", unknown));
            }
        }
    }

    Ok(roles)
}

/// Coordinator role flags
#[derive(Debug, Default, Clone)]
struct CoordinatorRoles {
    coordinator: bool,
    reflector: bool,
    relay: bool,
    rendezvous: bool,
}

impl From<CoordinatorRoles> for saorsa_gossip_coordinator::CoordinatorRoles {
    fn from(roles: CoordinatorRoles) -> Self {
        Self {
            coordinator: roles.coordinator,
            reflector: roles.reflector,
            rendezvous: roles.rendezvous,
            relay: roles.relay,
        }
    }
}

/// Initialize logging based on verbosity
fn init_logging(verbose: bool) -> Result<()> {
    use tracing_subscriber::EnvFilter;

    let filter = if verbose {
        EnvFilter::new("trace")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // TDD RED: These tests will fail initially

    #[test]
    fn test_parse_roles_coordinator_only() {
        let roles = parse_roles("coordinator").expect("should parse");
        assert!(roles.coordinator);
        assert!(!roles.reflector);
        assert!(!roles.relay);
        assert!(!roles.rendezvous);
    }

    #[test]
    fn test_parse_roles_multiple() {
        let roles = parse_roles("coordinator,reflector,relay").expect("should parse");
        assert!(roles.coordinator);
        assert!(roles.reflector);
        assert!(roles.relay);
        assert!(!roles.rendezvous);
    }

    #[test]
    fn test_parse_roles_all() {
        let roles = parse_roles("coordinator,reflector,relay,rendezvous").expect("should parse");
        assert!(roles.coordinator);
        assert!(roles.reflector);
        assert!(roles.relay);
        assert!(roles.rendezvous);
    }

    #[test]
    fn test_parse_roles_case_insensitive() {
        let roles = parse_roles("COORDINATOR,Reflector,RELAY").expect("should parse");
        assert!(roles.coordinator);
        assert!(roles.reflector);
        assert!(roles.relay);
    }

    #[test]
    fn test_parse_roles_with_spaces() {
        let roles = parse_roles("coordinator, reflector , relay").expect("should parse");
        assert!(roles.coordinator);
        assert!(roles.reflector);
        assert!(roles.relay);
    }

    #[test]
    fn test_parse_roles_unknown_fails() {
        let result = parse_roles("coordinator,unknown");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown role"));
    }

    #[test]
    fn test_parse_roles_empty_string() {
        let roles = parse_roles("").expect("should parse empty");
        assert!(!roles.coordinator);
        assert!(!roles.reflector);
        assert!(!roles.relay);
        assert!(!roles.rendezvous);
    }
}
