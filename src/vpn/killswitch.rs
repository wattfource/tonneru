use anyhow::Result;
use std::process::Command;

use super::run_command_with_timeout;

/// Enable the kill switch using nftables
/// This blocks all traffic except through the VPN interface
pub async fn enable() -> Result<()> {
    // Get the current WireGuard interface
    let status = super::wireguard::get_status().await?;
    let interface = status.interface.unwrap_or_else(|| "wg0".to_string());

    // Use nftables (modern approach for Arch)
    let rules = format!(
        r#"
table inet tonneru_killswitch {{
    chain input {{
        type filter hook input priority 0; policy drop;
        iif lo accept
        iif {} accept
        ct state established,related accept
    }}
    chain output {{
        type filter hook output priority 0; policy drop;
        oif lo accept
        oif {} accept
        ct state established,related accept
        # Allow DHCP
        udp dport 67 accept
        udp sport 68 accept
        # Allow DNS (might want to restrict this to VPN DNS only)
        udp dport 53 accept
        tcp dport 53 accept
    }}
    chain forward {{
        type filter hook forward priority 0; policy drop;
    }}
}}
"#,
        interface, interface
    );

    // Write rules to temp file
    let temp_path = "/tmp/tonneru-killswitch.nft";
    tokio::fs::write(temp_path, &rules).await?;

    // Apply rules using sudo (with timeout to prevent hanging)
    let output = run_command_with_timeout("sudo", &["nft", "-f", temp_path]).await;

    // Cleanup temp file
    let _ = tokio::fs::remove_file(temp_path).await;

    match output {
        Ok(output) => {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to enable kill switch: {}", stderr);
            }
        }
        Err(e) => anyhow::bail!("Failed to enable kill switch: {}", e),
    }

    tracing::info!("Kill switch enabled for interface: {}", interface);
    Ok(())
}

/// Disable the kill switch
pub async fn disable() -> Result<()> {
    match run_command_with_timeout("sudo", &["nft", "delete", "table", "inet", "tonneru_killswitch"]).await {
        Ok(output) => {
    // It's okay if the table doesn't exist
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("No such file or directory") && !stderr.contains("does not exist") {
            tracing::warn!("Kill switch disable warning: {}", stderr);
                }
            }
        }
        Err(e) => {
            tracing::warn!("Kill switch disable failed: {}", e);
        }
    }

    tracing::info!("Kill switch disabled");
    Ok(())
}

/// Check if kill switch is currently enabled
#[allow(dead_code)]  // Reserved for future status checking
pub async fn is_enabled() -> Result<bool> {
    // Try with sudo first (with timeout)
    if let Ok(output) = run_command_with_timeout("sudo", &["nft", "list", "table", "inet", "tonneru_killswitch"]).await {
        if output.status.success() {
            return Ok(true);
        }
    }

    // Fallback: try without sudo (no timeout needed, quick operation)
    let output = Command::new("nft")
        .args(["list", "table", "inet", "tonneru_killswitch"])
        .output();

    match output {
        Ok(output) => Ok(output.status.success()),
        Err(_) => Ok(false),
    }
}
