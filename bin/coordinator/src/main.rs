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
    let roles = parse_roles(&args.roles)?;
    tracing::info!("Parsed roles: {:?}", roles);

    // TODO: Start coordinator node
    // This will be implemented in GREEN phase

    Ok(())
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
