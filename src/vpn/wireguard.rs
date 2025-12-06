use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

use super::run_helper;
use super::run_helper_with_stdin;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WgProfile {
    pub name: String,
    pub protocol: String,  // "wireguard"
    pub connected: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WgStatus {
    pub connected: bool,
    pub interface: Option<String>,
    pub endpoint: Option<String>,
    pub latest_handshake: Option<String>,
    pub transfer_rx: Option<String>,
    pub transfer_tx: Option<String>,
    pub handshake_stale: bool,       // True if handshake is too old (>3 min)
    pub has_traffic: bool,           // True if there's been any data transfer
    pub routing_ok: bool,            // True if default route goes through VPN
}

/// List all available WireGuard profiles
pub async fn list_profiles() -> Result<Vec<WgProfile>> {
    let mut profiles = Vec::new();
    let mut seen_names = std::collections::HashSet::new();
    let mut valid_configs = std::collections::HashSet::new();
    let mut could_read_config_dir = false;

    // Get current connection status
    let status = get_status().await.unwrap_or_default();
    let active_interface = status.interface.clone();

    // Get list of config files using helper
    if let Ok(output) = run_helper(&["config-list"]).await {
        if output.status.success() {
            could_read_config_dir = true;
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let name = line.trim();
                if !name.is_empty() {
                    valid_configs.insert(name.to_string());
                }
            }
        }
    }

    // Also check for active interfaces (even if no .conf file - might be manually configured)
    let output = Command::new("ip")
        .args(["link", "show", "type", "wireguard"])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some(name) = line.split(':').nth(1) {
                    let name = name.trim().split('@').next().unwrap_or("").to_string();
                    if !name.is_empty() {
                        valid_configs.insert(name);
                    }
                }
            }
        }
    }

    // Load our config
    if let Ok(mut config) = crate::config::AppConfig::load() {
        // Only clean up orphaned entries if we could actually read the config directory
        if could_read_config_dir {
            let original_len = config.known_tunnels.len();
            
            // Only keep tunnels that have valid configs
            config.known_tunnels.retain(|t| {
                t.protocol != "wireguard" || valid_configs.contains(&t.name)
            });
            
            if config.known_tunnels.len() != original_len {
                let _ = config.save(); // Auto-cleanup orphaned entries
            }
        }
        
        // Add profiles from our config
        for tunnel in &config.known_tunnels {
            if tunnel.protocol == "wireguard" && !seen_names.contains(&tunnel.name) {
                let connected = active_interface.as_ref() == Some(&tunnel.name);
                profiles.push(WgProfile {
                    name: tunnel.name.clone(),
                    protocol: "wireguard".to_string(),
                    connected,
                });
                seen_names.insert(tunnel.name.clone());
            }
        }
    }

    // Add any configs that aren't in our known_tunnels
    for name in &valid_configs {
        if !seen_names.contains(name) {
            let connected = active_interface.as_ref().map(|s| s.as_str()) == Some(name.as_str());
            profiles.push(WgProfile {
                name: name.clone(),
                protocol: "wireguard".to_string(),
                connected,
            });
            seen_names.insert(name.clone());
        }
    }

    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(profiles)
}

/// Get current WireGuard connection status
pub async fn get_status() -> Result<WgStatus> {
    // Use helper to get status
    if let Ok(output) = run_helper(&["status"]).await {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if !stdout.trim().is_empty() {
                return parse_wg_show_output(&stdout);
            }
        }
    }

    // Fallback: check if any wg interface exists via ip link
    let output = Command::new("ip")
        .args(["link", "show", "type", "wireguard"])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if !stdout.trim().is_empty() {
                for line in stdout.lines() {
                    if let Some(name) = line.split(':').nth(1) {
                        let name = name.trim().split('@').next().unwrap_or(name.trim());
                        return Ok(WgStatus {
                            connected: true,
                            interface: Some(name.to_string()),
                            ..Default::default()
                        });
                    }
                }
            }
        }
    }

    Ok(WgStatus::default())
}

fn parse_wg_show_output(stdout: &str) -> Result<WgStatus> {
    let mut status = WgStatus {
        connected: true,
        routing_ok: false,
        has_traffic: false,
        handshake_stale: true,  // Assume stale until proven otherwise
        ..Default::default()
    };

    for line in stdout.lines() {
        let line = line.trim();

        if line.starts_with("interface:") {
            status.interface = Some(line.replace("interface:", "").trim().to_string());
        } else if line.starts_with("endpoint:") {
            status.endpoint = Some(line.replace("endpoint:", "").trim().to_string());
        } else if line.starts_with("latest handshake:") {
            let handshake = line.replace("latest handshake:", "").trim().to_string();
            status.handshake_stale = is_handshake_stale(&handshake);
            status.latest_handshake = Some(handshake);
        } else if line.starts_with("transfer:") {
            let transfer = line.replace("transfer:", "").trim().to_string();
            let parts: Vec<&str> = transfer.split(',').collect();
            if parts.len() >= 2 {
                status.transfer_rx = Some(parts[0].trim().to_string());
                status.transfer_tx = Some(parts[1].trim().to_string());
                // Check if there's been any meaningful traffic
                status.has_traffic = has_meaningful_traffic(parts[0], parts[1]);
            }
        }
    }

    // Check if routing goes through VPN
    if let Some(ref iface) = status.interface {
        status.routing_ok = check_vpn_routing(iface);
    }

    Ok(status)
}

/// Check if handshake is stale (older than 3 minutes)
fn is_handshake_stale(handshake: &str) -> bool {
    let handshake_lower = handshake.to_lowercase();
    
    // If it says "hour" or "day", definitely stale
    if handshake_lower.contains("hour") || handshake_lower.contains("day") {
        return true;
    }
    
    // Parse minutes
    if handshake_lower.contains("minute") {
        for part in handshake_lower.split_whitespace() {
            if let Ok(mins) = part.parse::<u32>() {
                return mins >= 3;
            }
        }
    }
    
    // If it's only seconds, it's fresh
    if handshake_lower.contains("second") && !handshake_lower.contains("minute") {
        return false;
    }
    
    // Default to stale if we can't parse
    true
}

/// Check if there's been meaningful traffic (not just handshake bytes)
fn has_meaningful_traffic(rx: &str, tx: &str) -> bool {
    let parse_bytes = |s: &str| -> u64 {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() >= 2 {
            let num: f64 = parts[0].parse().unwrap_or(0.0);
            let unit = parts[1].to_uppercase();
            match unit.as_str() {
                "B" => num as u64,
                "KIB" | "KB" => (num * 1024.0) as u64,
                "MIB" | "MB" => (num * 1024.0 * 1024.0) as u64,
                "GIB" | "GB" => (num * 1024.0 * 1024.0 * 1024.0) as u64,
                _ => num as u64,
            }
        } else {
            0
        }
    };
    
    let rx_bytes = parse_bytes(rx);
    let tx_bytes = parse_bytes(tx);
    
    // More than 1KB transferred means real traffic
    (rx_bytes + tx_bytes) > 1024
}

/// Check if the default route goes through the VPN interface
fn check_vpn_routing(vpn_interface: &str) -> bool {
    // Check default route
    let output = Command::new("ip")
        .args(["route", "show", "default"])
        .output();
    
    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains(vpn_interface) {
                return true;
            }
        }
    }
    
    // Also check for WireGuard's split default routes (0.0.0.0/1 and 128.0.0.0/1)
    let output = Command::new("ip")
        .args(["route", "show"])
        .output();
    
    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains(vpn_interface) && 
                   (line.starts_with("0.0.0.0/1") || line.starts_with("128.0.0.0/1") || line.starts_with("default")) {
                    return true;
                }
            }
        }
    }
    
    false
}

/// Connect to a WireGuard profile using the secure helper
pub async fn connect(profile_name: &str) -> Result<()> {
    // First disconnect any existing connection
    let _ = disconnect().await;

    let output = run_helper(&["connect", profile_name]).await
        .context("Failed to execute connect")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to connect: {}", stderr);
    }

    Ok(())
}

/// Disconnect from current WireGuard connection
pub async fn disconnect() -> Result<()> {
    // Helper will auto-detect the active interface
    match run_helper(&["disconnect"]).await {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!("Failed to disconnect: {}", stderr);
            }
        }
        Err(e) => {
            tracing::warn!("Disconnect command failed: {}", e);
        }
    }

    Ok(())
}

/// Add a new WireGuard profile and save to our config
pub async fn add_profile(name: &str, config_content: &str) -> Result<()> {
    // Sanitize the name (helper also validates, but we do it here too)
    let safe_name: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();

    if safe_name.is_empty() {
        anyhow::bail!("Invalid profile name");
    }

    // Validate the config
    if !config_content.contains("[Interface]") || !config_content.contains("[Peer]") {
        anyhow::bail!("Invalid WireGuard config: missing [Interface] or [Peer] section");
    }

    // Write config using helper
    let output = run_helper_with_stdin(&["config-write", &safe_name], config_content).await
        .context("Failed to write config")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to save profile: {}", stderr);
    }

    // Save to our config so we remember it
    let mut config = crate::config::AppConfig::load().unwrap_or_default();
    
    // Preserve kill_switch setting if tunnel existed
    let existing_ks = config.known_tunnels.iter()
        .find(|t| t.name == safe_name)
        .map(|t| t.kill_switch)
        .unwrap_or(false);
    config.known_tunnels.retain(|t| t.name != safe_name);
    config.known_tunnels.push(crate::config::TunnelInfo {
        name: safe_name.clone(),
        protocol: "wireguard".to_string(),
        kill_switch: existing_ks,
    });
    config.save()?;

    tracing::info!("Created WireGuard profile: {}", safe_name);
    Ok(())
}

/// Delete a WireGuard profile
pub async fn delete_profile(name: &str) -> Result<()> {
    // Disconnect if connected
    let status = get_status().await.unwrap_or_default();
    if status.interface.as_deref() == Some(name) {
        let _ = disconnect().await;
    }

    let output = run_helper(&["config-delete", name]).await
        .context("Failed to delete WireGuard config")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to delete profile: {}", stderr);
    }

    // Remove from our config
    let mut config = crate::config::AppConfig::load().unwrap_or_default();
    config.known_tunnels.retain(|t| t.name != name);
    config.save()?;

    Ok(())
}

/// Extended health check result
#[derive(Debug, Clone, Default)]
pub struct VpnHealthCheck {
    pub interface_exists: bool,
    pub has_peer: bool,
    pub handshake_recent: bool,
    pub routing_configured: bool,
    pub can_reach_internet: bool,
    pub latency_ms: Option<u32>,
}

impl VpnHealthCheck {
    /// Returns true if the VPN is fully operational
    pub fn is_healthy(&self) -> bool {
        self.interface_exists 
            && self.has_peer 
            && self.routing_configured 
            && self.can_reach_internet
    }
    
    /// Returns true if the VPN is partially working (might need attention)
    pub fn is_degraded(&self) -> bool {
        self.interface_exists 
            && self.has_peer 
            && (!self.handshake_recent || !self.routing_configured)
    }
}

/// Perform a comprehensive health check on the VPN connection
pub async fn health_check() -> VpnHealthCheck {
    let mut result = VpnHealthCheck::default();
    
    // Get current status
    let status = get_status().await.unwrap_or_default();
    
    if !status.connected {
        return result;
    }
    
    result.interface_exists = true;
    result.has_peer = status.endpoint.is_some();
    result.handshake_recent = !status.handshake_stale;
    result.routing_configured = status.routing_ok;
    
    // Try to reach the internet through the VPN
    let start = std::time::Instant::now();
    
    // Use ping to 1.1.1.1 with a short timeout
    if let Ok(output) = Command::new("ping")
        .args(["-c", "1", "-W", "3", "1.1.1.1"])
        .output()
    {
        if output.status.success() {
            result.can_reach_internet = true;
            result.latency_ms = Some(start.elapsed().as_millis() as u32);
        }
    }
    
    // If ping failed, try curl as fallback (ICMP might be blocked)
    if !result.can_reach_internet {
        if let Ok(output) = Command::new("curl")
            .args([
                "-s", "-o", "/dev/null",
                "-w", "%{http_code}",
                "--connect-timeout", "3",
                "--max-time", "5",
                "http://detectportal.firefox.com/success.txt"
            ])
            .output()
        {
            if output.status.success() {
                let response = String::from_utf8_lossy(&output.stdout);
                if response.starts_with("200") || response.starts_with("204") {
                    result.can_reach_internet = true;
                    result.latency_ms = Some(start.elapsed().as_millis() as u32);
                }
            }
        }
    }
    
    result
}

/// Get the uptime of a network interface in seconds
/// Reads the modification time of /sys/class/net/<interface>/uevent which corresponds
/// to when the interface was created
pub fn get_interface_uptime(interface: &str) -> Option<u64> {
    let path = format!("/sys/class/net/{}/uevent", interface);
    let metadata = std::fs::metadata(&path).ok()?;
    let modified = metadata.modified().ok()?;
    let elapsed = modified.elapsed().ok()?;
    Some(elapsed.as_secs())
}

/// Quick check if VPN interface exists and has recent handshake
#[allow(dead_code)]
pub async fn is_alive() -> bool {
    let status = get_status().await.unwrap_or_default();
    status.connected && !status.handshake_stale
}

/// Force a handshake refresh by sending a ping through the tunnel
#[allow(dead_code)]
pub async fn refresh_connection() -> Result<()> {
    let status = get_status().await?;
    
    if !status.connected {
        anyhow::bail!("VPN not connected");
    }
    
    // Get the endpoint IP and ping it to force traffic
    if let Some(endpoint) = &status.endpoint {
        if let Some(ip) = endpoint.split(':').next() {
            let _ = Command::new("ping")
                .args(["-c", "1", "-W", "2", ip])
                .output();
        }
    }
    
    // Also ping a public IP to force handshake if needed
    let _ = Command::new("ping")
        .args(["-c", "1", "-W", "2", "1.1.1.1"])
        .output();
    
    Ok(())
}
