pub mod monitor;
pub mod power;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub name: String,           // Display name (SSID for wifi, connection name for ethernet)
    pub network_type: String,   // "wifi", "ethernet"
    pub device: String,         // e.g., "wlan0", "eth0"
    pub connected: bool,
    pub ssid: Option<String>,   // For WiFi - the actual SSID
}

impl NetworkInfo {
    /// Get a unique identifier for this network (for rules)
    pub fn identifier(&self) -> String {
        if let Some(ssid) = &self.ssid {
            format!("wifi:{}", ssid)
        } else if !self.name.is_empty() && self.name != self.device {
            format!("network:{}", self.name)
        } else {
            format!("device:{}", self.device)
        }
    }
}

/// Get all network connections
pub async fn get_networks() -> Result<Vec<NetworkInfo>> {
    let mut networks = Vec::new();

    // Try iwd first (common on Arch/Omarchy)
    if let Ok(iwd_networks) = get_iwd_networks().await {
        if !iwd_networks.is_empty() {
            networks.extend(iwd_networks);
            return Ok(networks);
        }
    }

    // Try NetworkManager
    if let Ok(nm_networks) = get_nm_networks().await {
        if !nm_networks.is_empty() {
            networks.extend(nm_networks);
            return Ok(networks);
        }
    }

    // Fallback to basic detection
    if let Ok(basic) = get_basic_networks().await {
        networks.extend(basic);
    }

    Ok(networks)
}

/// Strip ANSI escape codes from a string (iwctl outputs colored text)
fn strip_ansi(s: &str) -> String {
    let mut result = String::new();
    let mut in_escape = false;
    
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Normalize SSID for comparison (trim whitespace, remove control chars, strip ANSI)
fn normalize_ssid(ssid: &str) -> String {
    strip_ansi(ssid)
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
}

/// Get networks from iwd (iwctl)
async fn get_iwd_networks() -> Result<Vec<NetworkInfo>> {
    use std::process::Command;

    let mut networks = Vec::new();
    let mut seen_ssids = std::collections::HashSet::new();

    // Get list of WiFi devices
    let devices = get_iwd_devices();
    
    for device in &devices {
        // Get current connection status for this device
        let output = Command::new("iwctl")
            .args(["station", device, "show"])
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                // Strip ANSI escape codes from iwctl output
                let raw_stdout = String::from_utf8_lossy(&output.stdout);
                let stdout = strip_ansi(&raw_stdout);
                let mut connected_ssid: Option<String> = None;
                let mut is_connected = false;

                for line in stdout.lines() {
                    let line = line.trim();
                    
                    // Check for connection state
                    if line.contains("State") && line.contains("connected") {
                        is_connected = true;
                    }
                    
                    // Extract SSID - handle multi-word SSIDs
                    // Format: "Connected network   My WiFi Name" 
                    if line.contains("Connected network") {
                        // Find "Connected network" and extract everything after
                        if let Some(idx) = line.find("Connected network") {
                            let after = &line[idx + "Connected network".len()..];
                            let ssid = normalize_ssid(after);
                            if !ssid.is_empty() {
                                connected_ssid = Some(ssid);
                                is_connected = true;
                            }
                        }
                    }
                }

                // Add connected network
                if let Some(ssid) = connected_ssid {
                    let normalized = normalize_ssid(&ssid);
                    if !seen_ssids.contains(&normalized) {
                        seen_ssids.insert(normalized);
                        networks.push(NetworkInfo {
                            name: ssid.clone(),
                            network_type: "wifi".to_string(),
                            device: device.clone(),
                            connected: is_connected,
                            ssid: Some(ssid),
                        });
                    }
                }
            }
        }

        // Get known/saved networks from iwd
        let known = Command::new("iwctl")
            .args(["known-networks", "list"])
            .output();

        if let Ok(output) = known {
            if output.status.success() {
                // Strip ANSI escape codes from iwctl output
                let raw_stdout = String::from_utf8_lossy(&output.stdout);
                let stdout = strip_ansi(&raw_stdout);
                
                // Parse iwctl known-networks output
                // Format: "  Name                              Security     Hidden..."
                // We need to extract just the SSID name, not the security type
                let lines: Vec<&str> = stdout.lines().collect();
                
                // Find the header line to determine column positions
                let mut ssid_start_col = 2;  // Usually starts after 2 spaces
                let mut security_start_col = 34; // Where "Security" column typically starts
                
                for line in &lines {
                    if line.contains("Name") && line.contains("Security") {
                        // Find where columns start
                        if let Some(name_idx) = line.find("Name") {
                            ssid_start_col = name_idx;
                        }
                        if let Some(sec_idx) = line.find("Security") {
                            security_start_col = sec_idx;
                        }
                        break;
                    }
                }
                
                for line in lines.iter().skip(4) { // Skip header lines
                    if line.is_empty() || line.trim().starts_with('-') || line.contains("---") {
                        continue;
                    }
                    
                    let line_str = *line;
                    if line_str.len() <= ssid_start_col {
                        continue;
                    }
                    
                    // Extract SSID: from name column start to just before security column
                    // But trim trailing whitespace to get clean SSID
                    let ssid_end = security_start_col.min(line_str.len());
                    let raw_ssid = if ssid_start_col < ssid_end {
                        &line_str[ssid_start_col..ssid_end]
                        } else {
                        line_str
                        };
                        
                    // Trim the SSID properly (removes trailing spaces before Security column)
                    let ssid = normalize_ssid(raw_ssid.trim());
                        
                        if ssid.is_empty() || ssid == "Name" {
                            continue;
                        }
                    
                    // Extra validation: SSIDs shouldn't contain common security type strings
                    if ssid.ends_with("psk") || ssid.ends_with("open") || ssid.ends_with("8021x") {
                        // Probably parsed incorrectly, try to fix
                        let clean_ssid = ssid
                            .trim_end_matches("psk")
                            .trim_end_matches("open")
                            .trim_end_matches("8021x")
                            .trim();
                        if !clean_ssid.is_empty() && !seen_ssids.contains(clean_ssid) {
                            seen_ssids.insert(clean_ssid.to_string());
                            networks.push(NetworkInfo {
                                name: clean_ssid.to_string(),
                                network_type: "wifi".to_string(),
                                device: "-".to_string(),
                                connected: false,
                                ssid: Some(clean_ssid.to_string()),
                            });
                        }
                        continue;
                    }
                        
                        // Skip if we already have this network (connected takes priority)
                        if seen_ssids.contains(&ssid) {
                            continue;
                        }
                        
                        seen_ssids.insert(ssid.clone());
                        networks.push(NetworkInfo {
                            name: ssid.clone(),
                            network_type: "wifi".to_string(),
                            device: "-".to_string(),
                            connected: false,
                            ssid: Some(ssid),
                        });
                }
            }
        }
    }

    // Also get ethernet interfaces
    if let Ok(eth_networks) = get_ethernet_interfaces().await {
        networks.extend(eth_networks);
    }

    // Sort: connected first, then alphabetically
    networks.sort_by(|a, b| {
        match (a.connected, b.connected) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });

    Ok(networks)
}

/// Get list of iwd WiFi devices
fn get_iwd_devices() -> Vec<String> {
    use std::process::Command;
    
    let mut devices = Vec::new();
    
    let output = Command::new("iwctl")
        .args(["device", "list"])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().skip(4) { // Skip header
                let parts: Vec<&str> = line.split_whitespace().collect();
                if !parts.is_empty() && parts[0].starts_with("wl") {
                    devices.push(parts[0].to_string());
                }
            }
        }
    }
    
    // Fallback: check for common device names
    if devices.is_empty() {
        for name in ["wlan0", "wlp0s20f3", "wlp2s0", "wlp3s0"] {
            if std::path::Path::new(&format!("/sys/class/net/{}", name)).exists() {
                devices.push(name.to_string());
                break;
            }
        }
    }
    
    devices
}

/// Get ethernet interfaces
async fn get_ethernet_interfaces() -> Result<Vec<NetworkInfo>> {
    use std::process::Command;
    
    let mut networks = Vec::new();
    
    let output = Command::new("ip")
        .args(["-o", "link", "show"])
        .output()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 {
                let device = parts[1].trim().split('@').next().unwrap_or("").to_string();
                
                // Only ethernet devices
                if !device.starts_with("en") && !device.starts_with("eth") {
                    continue;
                }
                
                let connected = line.contains("state UP");
                
                networks.push(NetworkInfo {
                    name: device.clone(),
                    network_type: "ethernet".to_string(),
                    device: device.clone(),
                    connected,
                    ssid: None,
                });
            }
        }
    }
    
    Ok(networks)
}

/// Get networks from NetworkManager via nmcli
async fn get_nm_networks() -> Result<Vec<NetworkInfo>> {
    use std::process::Command;
    use std::collections::HashMap;

    let mut networks: Vec<NetworkInfo> = Vec::new();
    let mut seen_ssids: HashMap<String, usize> = HashMap::new();

    // Check if nmcli exists
    let nmcli_check = Command::new("which").arg("nmcli").output();
    if nmcli_check.is_err() || !nmcli_check.unwrap().status.success() {
        return Ok(networks);
    }

    // Get all saved connections
    let output = Command::new("nmcli")
        .args(["-t", "-f", "NAME,TYPE,DEVICE,STATE", "connection", "show"])
        .output()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 4 {
                let conn_name = parts[0].to_string();
                let conn_type = parts[1].to_string();
                let device = parts[2].to_string();
                let state = parts[3].to_string();

                if conn_type == "loopback" {
                    continue;
                }

                let network_type = match conn_type.as_str() {
                    "wifi" | "802-11-wireless" => "wifi",
                    "ethernet" | "802-3-ethernet" => "ethernet",
                    _ => continue,
                };

                let connected = state == "activated";

                let (display_name, ssid) = if network_type == "wifi" {
                    (conn_name.clone(), Some(conn_name.clone()))
                } else {
                    (conn_name.clone(), None)
                };

                if let Some(ssid_str) = &ssid {
                    if let Some(&idx) = seen_ssids.get(ssid_str) {
                        if connected && !networks[idx].connected {
                            networks[idx].connected = true;
                            networks[idx].device = device.clone();
                        }
                        continue;
                    }
                    seen_ssids.insert(ssid_str.clone(), networks.len());
                }

                networks.push(NetworkInfo {
                    name: display_name,
                    network_type: network_type.to_string(),
                    device: if device.is_empty() { "-".to_string() } else { device },
                    connected,
                    ssid,
                });
            }
        }
    }

    networks.sort_by(|a, b| {
        match (a.connected, b.connected) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });

    Ok(networks)
}

/// Fallback: get basic network interfaces with iw for SSID
async fn get_basic_networks() -> Result<Vec<NetworkInfo>> {
    use std::process::Command;

    let mut networks = Vec::new();

    // Try to get SSID using iw
    let active_ssid = get_ssid_from_iw();

    let output = Command::new("ip")
        .args(["-o", "link", "show"])
        .output()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 {
                let device = parts[1].trim().split('@').next().unwrap_or("").to_string();

                if device == "lo" || device.starts_with("veth") || device.starts_with("docker") || device.starts_with("br-") {
                    continue;
                }

                let is_wifi = device.starts_with("wl");
                let is_ethernet = device.starts_with("en") || device.starts_with("eth");
                
                if !is_wifi && !is_ethernet {
                    continue;
                }

                let network_type = if is_wifi { "wifi" } else { "ethernet" };
                let connected = line.contains("state UP");

                let (name, ssid) = if is_wifi && connected {
                    if let Some(ref ssid) = active_ssid {
                        (ssid.clone(), Some(ssid.clone()))
                    } else {
                        (device.clone(), None)
                    }
                } else {
                    (device.clone(), None)
                };

                networks.push(NetworkInfo {
                    name,
                    network_type: network_type.to_string(),
                    device,
                    connected,
                    ssid,
                });
            }
        }
    }

    Ok(networks)
}

/// Get SSID using iw command
fn get_ssid_from_iw() -> Option<String> {
    use std::process::Command;
    
    // Find WiFi device
    for device in ["wlan0", "wlp0s20f3", "wlp2s0", "wlp3s0"] {
        let output = Command::new("iw")
            .args(["dev", device, "link"])
            .output()
            .ok()?;
        
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.trim().starts_with("SSID:") {
                    return Some(line.trim().replace("SSID:", "").trim().to_string());
                }
            }
        }
    }
    
    None
}

/// Get the currently active network connection
pub async fn get_active_connection() -> Result<Option<NetworkInfo>> {
    let networks = get_networks().await?;
    Ok(networks.into_iter().find(|n| n.connected))
}

/// Internet connectivity status
#[derive(Debug, Clone, Default)]
pub struct ConnectivityStatus {
    pub has_interface: bool,        // Network interface is up
    pub has_ip_address: bool,       // Has an IP address assigned
    pub can_reach_gateway: bool,    // Can ping the gateway
    pub has_internet: bool,         // Can reach external hosts
    pub latency_ms: Option<u32>,    // Round-trip time to test host
}

impl ConnectivityStatus {
    #[allow(dead_code)]
    pub fn is_online(&self) -> bool {
        self.has_interface && self.has_ip_address && self.has_internet
    }
    
    #[allow(dead_code)]
    pub fn is_partial(&self) -> bool {
        self.has_interface && self.has_ip_address && !self.has_internet
    }
}

/// Check internet connectivity status
/// This is more thorough than just checking if an interface is up
pub async fn check_connectivity() -> ConnectivityStatus {
    use std::process::Command;
    use std::time::Instant;
    
    let mut status = ConnectivityStatus::default();
    
    // Check if any network interface is up (excluding loopback and wireguard)
    if let Ok(output) = Command::new("ip")
        .args(["-o", "link", "show", "up"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 2 {
                    let device = parts[1].trim().split('@').next().unwrap_or("");
                    // Skip loopback, wireguard, docker, and virtual interfaces
                    if device != "lo" 
                       && !device.starts_with("wg") 
                       && !device.starts_with("docker")
                       && !device.starts_with("br-")
                       && !device.starts_with("veth")
                    {
                        status.has_interface = true;
                        break;
                    }
                }
            }
        }
    }
    
    if !status.has_interface {
        return status;
    }
    
    // Check if we have an IP address on a non-VPN interface
    if let Ok(output) = Command::new("ip")
        .args(["-4", "-o", "addr", "show"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                // Skip loopback and VPN interfaces
                if !line.contains(" lo ") 
                   && !line.contains(" wg")
                   && !line.contains("127.0.0.1")
                {
                    status.has_ip_address = true;
                    break;
                }
            }
        }
    }
    
    if !status.has_ip_address {
        return status;
    }
    
    // Try to reach the gateway
    if let Ok(output) = Command::new("ip")
        .args(["route", "show", "default"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Extract gateway IP - format: "default via 192.168.1.1 dev wlan0"
            for line in stdout.lines() {
                if line.starts_with("default via ") {
                    if let Some(gateway) = line.split_whitespace().nth(2) {
                        // Quick ping to gateway (1 packet, 1 second timeout)
                        if let Ok(ping_output) = Command::new("ping")
                            .args(["-c", "1", "-W", "1", gateway])
                            .output()
                        {
                            status.can_reach_gateway = ping_output.status.success();
                        }
                        break;
                    }
                }
            }
        }
    }
    
    // Check actual internet connectivity
    // Try multiple methods for reliability
    let start = Instant::now();
    
    // Method 1: Try to reach common DNS servers (fast, reliable)
    let dns_hosts = ["1.1.1.1", "8.8.8.8", "9.9.9.9"];
    for host in dns_hosts {
        if let Ok(output) = Command::new("ping")
            .args(["-c", "1", "-W", "2", host])
            .output()
        {
            if output.status.success() {
                status.has_internet = true;
                status.latency_ms = Some(start.elapsed().as_millis() as u32);
                return status;
            }
        }
    }
    
    // Method 2: Try HTTP connectivity check (fallback if ICMP is blocked)
    // Use curl with timeout to check connectivity
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
                status.has_internet = true;
                status.latency_ms = Some(start.elapsed().as_millis() as u32);
            }
        }
    }
    
    status
}

/// Quick connectivity check - just verifies we can reach the internet
/// Faster than full check_connectivity()
pub async fn has_internet() -> bool {
    use std::process::Command;
    
    // Quick ping to 1.1.1.1 (Cloudflare DNS - very reliable)
    if let Ok(output) = Command::new("ping")
        .args(["-c", "1", "-W", "2", "1.1.1.1"])
        .output()
    {
        return output.status.success();
    }
    
    false
}

/// IP lookup endpoints - randomized to avoid rate limiting and for privacy
const IP_ENDPOINTS: &[&str] = &[
    "https://ifconfig.io",
    "https://api.ipify.org",
    "https://ipinfo.io/ip",
    "https://icanhazip.com",
    "https://ipecho.net/plain",
    "https://checkip.amazonaws.com",
    "https://wtfismyip.com/text",
    "https://api.my-ip.io/ip",
];

/// Fetch public IP address from a random endpoint
/// Returns the IP as a string, or None if all attempts fail
pub async fn get_public_ip() -> Option<String> {
    use std::process::Command;
    use std::time::SystemTime;
    
    // Simple randomization using system time
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as usize)
        .unwrap_or(0);
    
    // Shuffle order by starting at random position
    let start_idx = seed % IP_ENDPOINTS.len();
    
    // Try endpoints in pseudo-random order (starting from random position, wrapping around)
    for i in 0..IP_ENDPOINTS.len() {
        let idx = (start_idx + i) % IP_ENDPOINTS.len();
        let endpoint = IP_ENDPOINTS[idx];
        
        if let Ok(output) = Command::new("curl")
            .args([
                "-4",               // IPv4 only
                "-s",               // Silent
                "-f",               // Fail silently on HTTP errors
                "--connect-timeout", "3",
                "--max-time", "5",
                endpoint,
            ])
            .output()
        {
            if output.status.success() {
                let ip = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .to_string();
                
                // Validate it looks like an IPv4 address
                if is_valid_ipv4(&ip) {
                    tracing::debug!("Got public IP {} from {}", ip, endpoint);
                    return Some(ip);
                }
            }
        }
    }
    
    tracing::warn!("Failed to fetch public IP from all endpoints");
    None
}

/// Simple IPv4 validation
fn is_valid_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    
    for part in parts {
        match part.parse::<u8>() {
            Ok(_) => continue,
            Err(_) => return false,
        }
    }
    
    true
}

/// Forget/Delete a known network connection
pub async fn forget_network(network: &NetworkInfo) -> Result<()> {
    use std::process::Command;
    
    // 1. Try to forget using iwctl (if it's a wifi network)
    if network.network_type == "wifi" {
        if let Some(ssid) = &network.ssid {
            tracing::info!("Attempting to forget network '{}' using iwctl", ssid);
            let output = Command::new("iwctl")
                .args(["known-networks", ssid, "forget"])
                .output();
                
            if let Ok(output) = output {
                if output.status.success() {
                    return Ok(());
                }
            }
        }
    }
    
    // 2. Try to forget using nmcli (NetworkManager)
    // Works for both wifi and ethernet if managed by NM
    let id_to_delete = if let Some(ssid) = &network.ssid {
        ssid
    } else {
        &network.name
    };
    
    tracing::info!("Attempting to delete connection '{}' using nmcli", id_to_delete);
    let output = Command::new("nmcli")
        .args(["connection", "delete", id_to_delete])
        .output();
        
    if let Ok(output) = output {
        if output.status.success() {
            return Ok(());
        }
    }
    
    // If we get here, we couldn't delete it
    anyhow::bail!("Could not forget network '{}'. Is it a known network?", network.name)
}
