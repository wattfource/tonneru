//! Power state detection for handling sleep/wake events
//! 
//! This module detects when the system has resumed from sleep/suspend
//! so the VPN can be verified and reconnected if needed.

use std::process::Command;
use std::time::{Duration, Instant};

/// System power state information
#[derive(Debug, Clone, Default)]
pub struct PowerState {
    /// True if we detected a resume from sleep/suspend
    pub just_resumed: bool,
    /// Time since last successful check (large gap indicates sleep)
    pub time_gap_ms: u64,
    /// True if system is currently idle (screen locked, etc.)
    #[allow(dead_code)]
    pub is_idle: bool,
    /// Uptime in seconds (used to detect reboots)
    pub uptime_secs: u64,
}

/// Power state tracker that maintains state between checks
pub struct PowerStateTracker {
    last_check: Instant,
    last_uptime: u64,
    expected_interval_ms: u64,
    /// Threshold for detecting a resume (time gap much larger than expected)
    resume_threshold_factor: f64,
}

impl Default for PowerStateTracker {
    fn default() -> Self {
        Self::new(Duration::from_secs(5))
    }
}

impl PowerStateTracker {
    /// Create a new tracker with the expected polling interval
    pub fn new(expected_interval: Duration) -> Self {
        Self {
            last_check: Instant::now(),
            last_uptime: get_uptime_secs().unwrap_or(0),
            expected_interval_ms: expected_interval.as_millis() as u64,
            // If the actual interval is 3x the expected, we probably resumed from sleep
            resume_threshold_factor: 3.0,
        }
    }
    
    /// Check current power state and detect if we just resumed from sleep
    pub fn check(&mut self) -> PowerState {
        let now = Instant::now();
        let elapsed_ms = now.duration_since(self.last_check).as_millis() as u64;
        let current_uptime = get_uptime_secs().unwrap_or(0);
        
        // Detect resume: elapsed time >> expected interval
        // This happens because Instant::now() doesn't advance during sleep
        let just_resumed = elapsed_ms > (self.expected_interval_ms as f64 * self.resume_threshold_factor) as u64;
        
        // Also check if uptime is much less than before (system rebooted)
        let rebooted = current_uptime < self.last_uptime.saturating_sub(10);
        
        // Get idle state
        let is_idle = check_session_idle();
        
        // Update state for next check
        self.last_check = now;
        self.last_uptime = current_uptime;
        
        PowerState {
            just_resumed: just_resumed || rebooted,
            time_gap_ms: elapsed_ms,
            is_idle,
            uptime_secs: current_uptime,
        }
    }
    
    /// Force a refresh of the baseline (call after handling a resume event)
    pub fn reset_baseline(&mut self) {
        self.last_check = Instant::now();
        self.last_uptime = get_uptime_secs().unwrap_or(0);
    }
}

/// Get system uptime in seconds
fn get_uptime_secs() -> Option<u64> {
    // Method 1: Read from /proc/uptime (most reliable on Linux)
    if let Ok(content) = std::fs::read_to_string("/proc/uptime") {
        if let Some(first) = content.split_whitespace().next() {
            if let Ok(secs) = first.parse::<f64>() {
                return Some(secs as u64);
            }
        }
    }
    
    // Method 2: Use uptime command as fallback
    if let Ok(output) = Command::new("cat")
        .arg("/proc/uptime")
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(first) = stdout.split_whitespace().next() {
                if let Ok(secs) = first.parse::<f64>() {
                    return Some(secs as u64);
                }
            }
        }
    }
    
    None
}

/// Check if the user session is idle (screen locked, screensaver active, etc.)
fn check_session_idle() -> bool {
    // Method 1: Check loginctl for idle hint
    if let Ok(output) = Command::new("loginctl")
        .args(["show-session", "self", "--property=IdleHint"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("IdleHint=yes") {
                return true;
            }
        }
    }
    
    // Method 2: Check for common screen lockers
    // swaylock, hyprlock, waylock, etc.
    let lockers = ["swaylock", "hyprlock", "waylock", "gtklock"];
    if let Ok(output) = Command::new("pgrep")
        .args(["-x", &lockers.join("|")])
        .output()
    {
        if output.status.success() && !output.stdout.is_empty() {
            return true;
        }
    }
    
    // Also check with individual pgrep calls (more reliable)
    for locker in lockers {
        if let Ok(output) = Command::new("pgrep")
            .args(["-x", locker])
            .output()
        {
            if output.status.success() && !output.stdout.is_empty() {
                return true;
            }
        }
    }
    
    false
}

/// Check if the system is preparing to sleep or is inhibited
#[allow(dead_code)]
pub fn check_sleep_inhibited() -> bool {
    // Check systemd-inhibit for active inhibitors
    if let Ok(output) = Command::new("systemd-inhibit")
        .args(["--list", "--no-legend"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Check for sleep inhibitors (not handle-lid-switch which is common)
            return stdout.lines().any(|line| {
                line.contains("sleep") && !line.contains("handle-lid-switch")
            });
        }
    }
    
    false
}

/// Wait for network to be ready after resume
/// Returns true if network came up within timeout, false otherwise
pub async fn wait_for_network_ready(timeout_secs: u64) -> bool {
    use tokio::time::{sleep, Duration};
    
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(timeout_secs);
    
    while start.elapsed() < timeout {
        // Check if we have any UP interface
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
                        // Check for actual network interfaces (not loopback, docker, etc.)
                        if (device.starts_with("wl") || device.starts_with("en") || device.starts_with("eth"))
                           && !device.starts_with("wg")
                        {
                            // Also check if it has an IP
                            if let Ok(addr_output) = Command::new("ip")
                                .args(["-4", "addr", "show", device])
                                .output()
                            {
                                if addr_output.status.success() {
                                    let addr_stdout = String::from_utf8_lossy(&addr_output.stdout);
                                    if addr_stdout.contains("inet ") {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        sleep(Duration::from_millis(500)).await;
    }
    
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_get_uptime() {
        let uptime = get_uptime_secs();
        assert!(uptime.is_some(), "Should be able to get uptime");
        assert!(uptime.unwrap() > 0, "Uptime should be positive");
    }
    
    #[test]
    fn test_power_state_tracker() {
        let mut tracker = PowerStateTracker::new(Duration::from_millis(100));
        
        // First check shouldn't indicate resume
        let state = tracker.check();
        assert!(!state.just_resumed, "First check shouldn't be a resume");
        
        // Second immediate check shouldn't indicate resume
        let state = tracker.check();
        assert!(!state.just_resumed, "Immediate second check shouldn't be a resume");
    }
}

