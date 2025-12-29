use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkRule {
    pub identifier: String,  // "wifi:SSID" or "device:eth0"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel_name: Option<String>,  // Which tunnel to use
    #[serde(default)]
    pub always_vpn: bool,
    #[serde(default)]
    pub never_vpn: bool,
    #[serde(default)]
    pub session_vpn: bool,  // Only for this session (cleared on network change/sleep)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    /// Network rules for auto-connect/disconnect
    #[serde(default)]
    pub network_rules: Vec<NetworkRule>,

    /// Default VPN profile to use for auto-connect
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,

    /// Last connected tunnel (for auto-reconnect on wake/startup)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_connected: Option<String>,

    /// Auto-reconnect to last tunnel on startup/wake
    #[serde(default)]
    pub auto_reconnect: bool,

    /// Kill switch enabled
    #[serde(default)]
    pub kill_switch: bool,

    /// Show notifications
    #[serde(default)]
    pub notifications: bool,

    /// Known/imported tunnels (we track these since /etc/wireguard needs root to read)
    #[serde(default)]
    pub known_tunnels: Vec<TunnelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelInfo {
    pub name: String,
    pub protocol: String,  // "wireguard", "openvpn", etc.
    #[serde(default)]
    pub kill_switch: bool,  // Per-tunnel kill switch setting
}

impl AppConfig {
    /// Get the config file path
    fn config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?
            .join("tonneru");

        if let Err(e) = std::fs::create_dir_all(&config_dir) {
            tracing::warn!("Could not create config directory: {}", e);
        }

        Ok(config_dir.join("config.toml"))
    }

    /// Load config from file, or create default
    pub fn load() -> Result<Self> {
        let path = match Self::config_path() {
            Ok(p) => p,
            Err(_) => return Ok(AppConfig::default()),
        };

        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    match toml::from_str(&content) {
                        Ok(config) => return Ok(config),
                        Err(e) => tracing::warn!("Failed to parse config: {}", e),
                    }
                }
                Err(e) => tracing::warn!("Failed to read config: {}", e),
            }
        }
        
        let config = AppConfig::default();
        let _ = config.save();
        Ok(config)
    }

    /// Save config to file
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        
        // Clean up the config before saving
        let mut clean_config = self.clone();
        
        // Remove rules with invalid identifiers (escape codes, etc.)
        clean_config.network_rules.retain(|r| {
            !r.identifier.contains('\x1b') && // No escape codes
            !r.identifier.is_empty() &&
            r.identifier.len() > 5 // Must have prefix + name
        });
        
        // Convert empty tunnel names to None
        for rule in &mut clean_config.network_rules {
            if rule.tunnel_name.as_ref().map(|s| s.is_empty()).unwrap_or(false) {
                rule.tunnel_name = None;
            }
        }
        
        let content = toml::to_string_pretty(&clean_config)?;
        std::fs::write(path, content)?;
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_serialization() {
        let config = AppConfig {
            network_rules: vec![NetworkRule {
                identifier: "wifi:MyNetwork".to_string(),
                tunnel_name: Some("my-vpn".to_string()),
                always_vpn: true,
                never_vpn: false,
                session_vpn: false,
            }],
            default_profile: Some("work-vpn".to_string()),
            last_connected: None,
            auto_reconnect: false,
            kill_switch: false,
            notifications: true,
            known_tunnels: vec![TunnelInfo {
                name: "my-vpn".to_string(),
                protocol: "wireguard".to_string(),
                kill_switch: false,
            }],
        };

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: AppConfig = toml::from_str(&serialized).unwrap();

        assert_eq!(config.network_rules.len(), deserialized.network_rules.len());
        assert_eq!(config.default_profile, deserialized.default_profile);
    }
}
