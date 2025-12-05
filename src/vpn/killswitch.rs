use anyhow::Result;

use super::run_helper;

/// Enable the kill switch using the secure helper
/// This blocks all traffic except through the VPN interface
pub async fn enable() -> Result<()> {
    // Get the current WireGuard interface
    let status = super::wireguard::get_status().await?;
    let interface = status.interface.unwrap_or_else(|| "wg0".to_string());

    // Use the secure helper to enable kill switch
    let output = run_helper(&["killswitch-on", &interface]).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to enable kill switch: {}", stderr);
    }

    tracing::info!("Kill switch enabled for interface: {}", interface);
    Ok(())
}

/// Disable the kill switch using the secure helper
/// This is robust - it will retry and verify the kill switch is actually off
pub async fn disable() -> Result<()> {
    // First attempt
    match run_helper(&["killswitch-off"]).await {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // It's okay if the table doesn't exist
                if !stderr.contains("No such file") && !stderr.contains("does not exist") {
                    tracing::warn!("Kill switch disable warning: {}", stderr);
                }
            }
        }
        Err(e) => {
            tracing::warn!("Kill switch disable attempt 1 failed: {}", e);
        }
    }

    // Verify it's actually disabled
    if is_enabled().await.unwrap_or(false) {
        tracing::warn!("Kill switch still enabled after first attempt, retrying...");
        
        // Second attempt
        if let Ok(output) = run_helper(&["killswitch-off"]).await {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::error!("Kill switch disable retry failed: {}", stderr);
            }
        }
        
        // Final check
        if is_enabled().await.unwrap_or(false) {
            tracing::error!("CRITICAL: Kill switch could not be disabled!");
            anyhow::bail!("Failed to disable kill switch after multiple attempts");
        }
    }

    tracing::info!("Kill switch disabled successfully");
    Ok(())
}

/// Check if kill switch is currently enabled
#[allow(dead_code)]
pub async fn is_enabled() -> Result<bool> {
    if let Ok(output) = run_helper(&["killswitch-status"]).await {
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Ok(stdout.trim() == "enabled");
    }
    Ok(false)
}
