//! T2-12: Jump host chain resolution with cycle detection.
//!
//! Recursively resolves ProxyJump chains to build the full path of hops
//! from the target host back to the first entry point. Detects cycles
//! (e.g. A -> B -> A) and returns an error instead of infinitely recursing.

use anyhow::{anyhow, Result};
use std::collections::HashSet;

use crate::{store::load_config, types::HostProfile};

/// Maximum chain depth (in addition to the existing `depth > 4` guard in
/// `embedded_ssh.rs`). This is a semantic limit — the chain must be at most
/// this many hops long.
const MAX_CHAIN_DEPTH: usize = 8;

/// One hop in a resolved jump chain.
#[derive(Debug, Clone)]
pub struct JumpHop {
    /// Profile name of this hop.
    pub name: String,
    /// Target host address.
    pub host: String,
    /// Target port.
    pub port: u16,
}

/// Resolve the full jump chain for a host profile.
///
/// Returns a vector of hops from the target host (index 0) to the first
/// jump host (last index). The first element is always the target host
/// itself.
///
/// # Errors
/// - `CycleDetected`: if the chain contains a cycle (e.g. A -> B -> A)
/// - `ChainTooDeep`: if the chain exceeds `MAX_CHAIN_DEPTH`
/// - `UnknownJumpHost`: if a referenced jump host profile doesn't exist
pub fn resolve_jump_chain(host: &HostProfile) -> Result<Vec<JumpHop>> {
    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    resolve_jump_chain_recursive(host, &mut chain, &mut visited, 0)
        .map_err(|e| anyhow!("jump chain resolution failed: {e}"))?;
    Ok(chain)
}

fn resolve_jump_chain_recursive(
    host: &HostProfile,
    chain: &mut Vec<JumpHop>,
    visited: &mut HashSet<String>,
    depth: usize,
) -> Result<()> {
    if depth > MAX_CHAIN_DEPTH {
        return Err(anyhow!(
            "jump chain too deep (>{MAX_CHAIN_DEPTH} hops) while resolving '{}'",
            host.name
        ));
    }

    // T2-12: Cycle detection — if we've already visited this host name,
    // there's a cycle in the chain.
    // Finding 15: Show the full path that caused the cycle, not just the
    // duplicate host name.
    if !visited.insert(host.name.clone()) {
        let path: Vec<String> = chain.iter().map(|h| h.name.clone()).collect();
        let path_str = path.join(" -> ");
        return Err(anyhow!(
            "jump chain cycle detected: '{}' appears twice in the chain. Full path: {} -> {}",
            host.name,
            path_str,
            host.name
        ));
    }

    // Add this hop to the chain
    chain.push(JumpHop {
        name: host.name.clone(),
        host: host.host.clone(),
        port: host.port.unwrap_or(22),
    });

    // If this host has a jump_host, resolve it recursively
    if let Some(jump_name) = host
        .jump_host
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let jump_host = load_config()?
            .hosts
            .into_iter()
            .find(|h| h.name == jump_name)
            .ok_or_else(|| anyhow!("unknown jump host profile: '{jump_name}'"))?;
        resolve_jump_chain_recursive(&jump_host, chain, visited, depth + 1)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_host(name: &str, jump: Option<&str>) -> HostProfile {
        HostProfile {
            name: name.into(),
            host: format!("10.0.0.{}", name.len()),
            user: Some("root".into()),
            port: Some(22),
            key_path: None,
            password: None,
            jump_host: jump.map(String::from),
            proxy_id: None,
            risk_override: None,
            tags: vec![],
            group: "default".into(),
            env: None,
            role: None,
            owner: None,
            init_command: None,
            passphrase: None,
        }
    }

    #[test]
    fn no_jump_host_returns_single_hop() {
        let host = make_host("target", None);
        let chain = resolve_jump_chain(&host).unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].name, "target");
    }

    #[test]
    fn unknown_jump_host_returns_error() {
        let host = make_host("target", Some("nonexistent"));
        let result = resolve_jump_chain(&host);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unknown jump host"));
    }
}
