//! Network and VPN monitoring daemon
//! 
//! This module provides resilient monitoring that:
//! - Detects sleep/wake events and re-establishes VPN connections
//! - Monitors network connectivity changes
//! - Verifies VPN health and reconnects if needed
//! - Applies network rules based on current connection

use anyhow::Result;
use std::time::Duration;
use tokio::time::{interval, sleep};

use crate::config::AppConfig;
use crate::network::{get_active_connection, check_connectivity, has_internet};
use crate::network::power::{PowerStateTracker, wait_for_network_ready};
use crate::vpn::wireguard;

/// Monitoring configuration
const CHECK_INTERVAL_SECS: u64 = 5;
const NETWORK_READY_TIMEOUT_SECS: u64 = 30;
const VPN_RECONNECT_DELAY_MS: u64 = 2000;
const VPN_HEALTH_CHECK_INTERVAL: u64 = 30; // Check VPN health every 30 seconds
const MAX_RECONNECT_ATTEMPTS: u32 = 3;

/// Monitoring state
struct MonitorState {
    last_network_id: Option<String>,
    last_vpn_connected: bool,
    last_vpn_interface: Option<String>,
    health_check_counter: u64,
    reconnect_attempts: u32,
    power_tracker: PowerStateTracker,
}

impl MonitorState {
    fn new() -> Self {
        Self {
            last_network_id: None,
            last_vpn_connected: false,
            last_vpn_interface: None,
            health_check_counter: 0,
            reconnect_attempts: 0,
            power_tracker: PowerStateTracker::new(Duration::from_secs(CHECK_INTERVAL_SECS)),
        }
    }
}

/// Start monitoring network changes and auto-connect/disconnect VPN based on rules
/// 
/// This is the main daemon loop that provides resilience to:
/// - Sleep/wake events
/// - Network changes
/// - VPN connection drops
/// - Internet connectivity changes
pub async fn start_monitoring() -> Result<()> {
    let mut config = AppConfig::load()?;
    let mut check_interval = interval(Duration::from_secs(CHECK_INTERVAL_SECS));
    let mut state = MonitorState::new();

    tracing::info!("Starting tonneru daemon with resilient monitoring");

    // Initial status check
    let vpn_status = wireguard::get_status().await.unwrap_or_default();
    state.last_vpn_connected = vpn_status.connected;
    state.last_vpn_interface = vpn_status.interface.clone();

    loop {
        check_interval.tick().await;

        // Reload config to pick up changes
        if let Ok(new_config) = AppConfig::load() {
            config = new_config;
        }

        // Check for power state changes (sleep/wake)
        let power_state = state.power_tracker.check();
        
        if power_state.just_resumed {
            tracing::info!(
                "System resumed from sleep (time gap: {}ms, uptime: {}s)",
                power_state.time_gap_ms,
                power_state.uptime_secs
            );
            handle_resume(&config, &mut state).await;
            continue; // Skip normal processing this cycle
        }

        // Normal monitoring cycle
        if let Err(e) = run_monitoring_cycle(&config, &mut state).await {
            tracing::error!("Monitoring cycle error: {}", e);
        }
    }
}

/// Handle system resume from sleep
async fn handle_resume(config: &AppConfig, state: &mut MonitorState) {
    tracing::info!("Handling system resume...");
    
    // Wait for network to come back up
    tracing::debug!("Waiting for network to be ready...");
    let network_ready = wait_for_network_ready(NETWORK_READY_TIMEOUT_SECS).await;
    
    if !network_ready {
        tracing::warn!("Network did not come up within {}s timeout", NETWORK_READY_TIMEOUT_SECS);
        notify_network_issue("Network not available after resume");
        state.power_tracker.reset_baseline();
        return;
    }
    
    tracing::info!("Network is ready after resume");
    
    // Small delay for network to fully stabilize
    sleep(Duration::from_millis(VPN_RECONNECT_DELAY_MS)).await;
    
    // Check internet connectivity
    let connectivity = check_connectivity().await;
    if !connectivity.has_internet {
        tracing::warn!("No internet connectivity after resume (has_ip: {}, gateway: {})",
            connectivity.has_ip_address, connectivity.can_reach_gateway);
        
        // If we have IP but no internet, might be a captive portal
        if connectivity.has_ip_address && connectivity.can_reach_gateway {
            notify_network_issue("Connected but no internet - captive portal?");
        }
        state.power_tracker.reset_baseline();
        return;
    }
    
    // Get current network and VPN status
    let current_network = get_active_connection().await.ok().flatten();
    let vpn_status = wireguard::get_status().await.unwrap_or_default();
    
    // Update last known network
    state.last_network_id = current_network.as_ref().map(|n| n.identifier());
    
    // Determine what VPN state we should be in
    if let Some(network) = &current_network {
        let rule = config.network_rules.iter()
            .find(|r| r.identifier == network.identifier());
        
        match rule {
            Some(r) if r.always_vpn => {
                let expected_tunnel = r.tunnel_name.as_ref().or(config.default_profile.as_ref());
                
                // Should be connected to VPN
                if let Some(tunnel) = expected_tunnel {
                    // Check if we need to reconnect
                    if !vpn_status.connected || vpn_status.interface.as_ref() != Some(tunnel) {
                        tracing::info!("Reconnecting VPN after resume (Always rule): {}", tunnel);
                        reconnect_vpn(tunnel, state).await;
                    } else if !verify_vpn_health(&vpn_status).await {
                        // Connected but unhealthy
                        tracing::warn!("VPN connected but unhealthy after resume - reconnecting");
                        reconnect_vpn(tunnel, state).await;
                    } else {
                        tracing::info!("VPN {} verified working after resume", tunnel);
                        notify_resume_ok(tunnel);
                    }
                }
            }
            Some(r) if r.session_vpn => {
                // User requested: Session ends on sleep/hibernation
                tracing::info!("Ending Session VPN after resume (sleep ended session)");
                // Clear the session flag so it doesn't try to reconnect later
                clear_session_rule(&network.identifier()).await;
                if vpn_status.connected {
                    let _ = wireguard::disconnect().await;
                    notify_session_ended();
                }
            }
            Some(r) if r.never_vpn => {
                // Should NOT be connected
                if vpn_status.connected {
                    tracing::info!("Disconnecting VPN per 'never' rule after resume");
                    if let Err(e) = wireguard::disconnect().await {
                        tracing::error!("Failed to disconnect: {}", e);
                    } else {
                        notify_disconnect();
                    }
                }
            }
            _ => {
                // No rule - leave VPN in current state but verify if connected
                if vpn_status.connected {
                    if !verify_vpn_health(&vpn_status).await {
                        tracing::warn!("VPN unhealthy after resume, disconnecting");
                        let _ = wireguard::disconnect().await;
                    }
                }
            }
        }
    }
    
    // Update state
    let new_status = wireguard::get_status().await.unwrap_or_default();
    state.last_vpn_connected = new_status.connected;
    state.last_vpn_interface = new_status.interface.clone();
    state.reconnect_attempts = 0;
    state.power_tracker.reset_baseline();
}

/// Run a normal monitoring cycle
async fn run_monitoring_cycle(config: &AppConfig, state: &mut MonitorState) -> Result<()> {
    // Get current network
    let current_network = get_active_connection().await.ok().flatten();
    let current_id = current_network.as_ref().map(|n| n.identifier());

    // Check if network changed
    if current_id != state.last_network_id {
        handle_network_change(config, state, &current_network, &current_id).await?;
    }

    // Periodic VPN health check (every VPN_HEALTH_CHECK_INTERVAL seconds)
    state.health_check_counter += CHECK_INTERVAL_SECS;
    if state.health_check_counter >= VPN_HEALTH_CHECK_INTERVAL {
        state.health_check_counter = 0;
        check_vpn_health(config, state, &current_network).await?;
    }

    Ok(())
}

/// Handle network connection changes
async fn handle_network_change(
    config: &AppConfig,
    state: &mut MonitorState,
    current_network: &Option<crate::network::NetworkInfo>,
    current_id: &Option<String>,
) -> Result<()> {
    tracing::info!("Network changed: {:?} -> {:?}", state.last_network_id, current_id);

    // Clear session rules for the OLD network
    if let Some(old_id) = &state.last_network_id {
        clear_session_rule(old_id).await;
    }

    if let Some(network) = current_network {
        // Find matching rule
        let rule = config.network_rules.iter()
            .find(|r| r.identifier == network.identifier());

        match rule {
            Some(r) if r.always_vpn => {
                tracing::info!("Auto-connecting VPN for network: {}", network.name);
                let tunnel = r.tunnel_name.as_ref().or(config.default_profile.as_ref());
                if let Some(profile) = tunnel {
                    if let Err(e) = wireguard::connect(profile).await {
                        tracing::error!("Failed to auto-connect VPN: {}", e);
                    } else {
                        notify_connect(profile);
                        state.reconnect_attempts = 0;
                    }
                }
            }
            Some(r) if r.session_vpn => {
                tracing::info!("Session VPN for network: {}", network.name);
                let tunnel = r.tunnel_name.as_ref().or(config.default_profile.as_ref());
                if let Some(profile) = tunnel {
                    if let Err(e) = wireguard::connect(profile).await {
                        tracing::error!("Failed to connect session VPN: {}", e);
                    } else {
                        notify_connect_session(profile);
                        state.reconnect_attempts = 0;
                    }
                }
            }
            Some(r) if r.never_vpn => {
                tracing::info!("Auto-disconnecting VPN for network: {}", network.name);
                if let Err(e) = wireguard::disconnect().await {
                    tracing::error!("Failed to auto-disconnect VPN: {}", e);
                } else {
                    notify_disconnect();
                }
            }
            _ => {
                tracing::debug!("No VPN rule for network: {}", network.name);
            }
        }
    } else {
        tracing::info!("Network disconnected, ending VPN sessions");
    }

    state.last_network_id = current_id.clone();
    
    // Update VPN state
    let vpn_status = wireguard::get_status().await.unwrap_or_default();
    state.last_vpn_connected = vpn_status.connected;
    state.last_vpn_interface = vpn_status.interface.clone();
    
    Ok(())
}

/// Periodic VPN health check
async fn check_vpn_health(
    config: &AppConfig,
    state: &mut MonitorState,
    current_network: &Option<crate::network::NetworkInfo>,
) -> Result<()> {
    let vpn_status = wireguard::get_status().await.unwrap_or_default();
    
    // Check for unexpected disconnection
    if state.last_vpn_connected && !vpn_status.connected {
        tracing::warn!("VPN disconnected unexpectedly!");
        
        // Check if we should reconnect based on rules
        if let Some(network) = current_network {
            let rule = config.network_rules.iter()
                .find(|r| r.identifier == network.identifier());
            
            if let Some(r) = rule {
                if (r.always_vpn || r.session_vpn) && state.reconnect_attempts < MAX_RECONNECT_ATTEMPTS {
                    let tunnel = r.tunnel_name.clone()
                        .or_else(|| config.default_profile.clone())
                        .or_else(|| state.last_vpn_interface.clone());
                    
                    if let Some(profile) = tunnel {
                        tracing::info!("Attempting to reconnect VPN: {} (attempt {})", 
                            profile, state.reconnect_attempts + 1);
                        reconnect_vpn(&profile, state).await;
                    }
                } else if state.reconnect_attempts >= MAX_RECONNECT_ATTEMPTS {
                    tracing::error!("Max reconnect attempts reached, giving up");
                    notify_vpn_failed("Max reconnect attempts reached");
                    state.reconnect_attempts = 0;
                }
            }
        }
    }
    
    // Check VPN health if connected
    if vpn_status.connected {
        if !verify_vpn_health(&vpn_status).await {
            tracing::warn!("VPN appears unhealthy (handshake stale: {}, routing ok: {})",
                vpn_status.handshake_stale, vpn_status.routing_ok);
            
            // Only try to fix if we should be connected
            if let Some(network) = current_network {
                let rule = config.network_rules.iter()
                    .find(|r| r.identifier == network.identifier());
                
                if let Some(r) = rule {
                    if (r.always_vpn || r.session_vpn) && state.reconnect_attempts < MAX_RECONNECT_ATTEMPTS {
                        if let Some(iface) = &vpn_status.interface {
                            tracing::info!("Attempting VPN health recovery: {}", iface);
                            reconnect_vpn(iface, state).await;
                        }
                    }
                }
            }
        }
    }
    
    // Update state
    state.last_vpn_connected = vpn_status.connected;
    state.last_vpn_interface = vpn_status.interface.clone();
    
    Ok(())
}

/// Verify VPN is actually working (not just interface up)
async fn verify_vpn_health(status: &wireguard::WgStatus) -> bool {
    if !status.connected {
        return false;
    }
    
    // Check basic indicators
    if !status.routing_ok {
        return false;
    }
    
    // Handshake being stale is a warning but not necessarily fatal
    // Only fail if handshake is very stale (handled by handshake_stale flag)
    if status.handshake_stale {
        // Try a connectivity check through the VPN
        // If we can reach the internet, the VPN is working despite stale handshake
        if has_internet().await {
            return true;
        }
        return false;
    }
    
    true
}

/// Reconnect to VPN with exponential backoff
async fn reconnect_vpn(profile: &str, state: &mut MonitorState) {
    state.reconnect_attempts += 1;
    
    // Exponential backoff: 2s, 4s, 8s, etc.
    let delay_ms = VPN_RECONNECT_DELAY_MS * (1 << state.reconnect_attempts.min(4));
    
    // First disconnect cleanly
    let _ = wireguard::disconnect().await;
    sleep(Duration::from_millis(500)).await;
    
    // Try to connect
    match wireguard::connect(profile).await {
        Ok(_) => {
            // Verify the connection actually works
            sleep(Duration::from_millis(1000)).await;
            let status = wireguard::get_status().await.unwrap_or_default();
            
            if status.connected && verify_vpn_health(&status).await {
                tracing::info!("VPN reconnected successfully: {}", profile);
                notify_reconnect(profile);
                state.reconnect_attempts = 0;
            } else {
                tracing::warn!("VPN connected but health check failed");
                if state.reconnect_attempts < MAX_RECONNECT_ATTEMPTS {
                    sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
        Err(e) => {
            tracing::error!("VPN reconnect failed: {}", e);
            if state.reconnect_attempts < MAX_RECONNECT_ATTEMPTS {
                sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }
}

/// Clear session rule for a network (called when network changes/disconnects)
async fn clear_session_rule(network_id: &str) {
    if let Ok(mut config) = AppConfig::load() {
        let had_session = config.network_rules.iter().any(|r| 
            r.identifier == network_id && r.session_vpn
        );
        
        if had_session {
            config.network_rules.retain(|r| 
                !(r.identifier == network_id && r.session_vpn)
            );
            
            if let Err(e) = config.save() {
                tracing::error!("Failed to clear session rule: {}", e);
            } else {
                tracing::info!("Cleared session rule for network: {}", network_id);
                
                if let Err(e) = wireguard::disconnect().await {
                    tracing::error!("Failed to disconnect session VPN: {}", e);
                } else {
                    notify_session_ended();
                }
            }
        }
    }
}

// Notification helpers
fn notify_connect(profile: &str) {
    let _ = notify_rust::Notification::new()
        .summary("tonneru")
        .body(&format!("Connected to {}", profile))
        .icon("network-vpn")
        .show();
}

fn notify_connect_session(profile: &str) {
    let _ = notify_rust::Notification::new()
        .summary("tonneru")
        .body(&format!("Session VPN: {}", profile))
        .icon("network-vpn")
        .show();
}

fn notify_disconnect() {
    let _ = notify_rust::Notification::new()
        .summary("tonneru")
        .body("VPN disconnected")
        .icon("network-vpn-disconnected")
        .show();
}

fn notify_session_ended() {
    let _ = notify_rust::Notification::new()
        .summary("tonneru")
        .body("Session ended, VPN disconnected")
        .icon("network-vpn-disconnected")
        .show();
}

fn notify_reconnect(profile: &str) {
    let _ = notify_rust::Notification::new()
        .summary("tonneru")
        .body(&format!("VPN reconnected: {}", profile))
        .icon("network-vpn")
        .show();
}

fn notify_resume_ok(profile: &str) {
    let _ = notify_rust::Notification::new()
        .summary("tonneru")
        .body(&format!("VPN {} active after resume", profile))
        .icon("network-vpn")
        .show();
}

fn notify_network_issue(message: &str) {
    let _ = notify_rust::Notification::new()
        .summary("tonneru")
        .body(message)
        .icon("network-error")
        .urgency(notify_rust::Urgency::Normal)
        .show();
}

fn notify_vpn_failed(message: &str) {
    let _ = notify_rust::Notification::new()
        .summary("tonneru")
        .body(&format!("VPN failed: {}", message))
        .icon("network-vpn-disconnected")
        .urgency(notify_rust::Urgency::Critical)
        .show();
}
