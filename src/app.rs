use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use std::time::Instant;

use crate::config::{AppConfig, NetworkRule, TunnelInfo};
use crate::network::{NetworkInfo, ConnectivityStatus};
use crate::vpn::wireguard::{WgProfile, WgStatus, VpnHealthCheck};

/// Pending configuration change that will be applied after countdown
#[derive(Debug, Clone)]
pub struct PendingChange {
    #[allow(dead_code)]
    pub network_id: String,      // Reserved for future logging/display
    #[allow(dead_code)]
    pub network_name: String,    // Reserved for future logging/display
    pub tunnel_name: Option<String>,
    pub action: PendingAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PendingAction {
    Connect,          // Connect to tunnel
    Disconnect,       // Disconnect from tunnel
    Reconnect,        // Disconnect then connect (tunnel changed)
    KillSwitchOn,     // Enable kill switch
    KillSwitchOff,    // Disable kill switch
}

/// Countdown duration in seconds before applying changes
const COUNTDOWN_SECONDS: u64 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Networks,
    Tunnels,
    KillSwitch,    // Internet kill switch box
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Popup {
    None,
    FileBrowser,
    ConfigPreview,
    ManualConfig,  // Manual config creation (name + paste content)
    Help,
    Confirm,
}

pub struct App {
    pub section: Section,
    pub popup: Popup,

    // Network state (top section)
    pub networks: Vec<NetworkInfo>,
    pub selected_network: usize,

    // Tunnel state (middle section) 
    pub tunnels: Vec<WgProfile>,
    pub selected_tunnel: usize,
    pub vpn_status: WgStatus,

    // Network rules (which tunnel for which network)
    pub network_rules: Vec<NetworkRule>,

    // Config
    pub config: AppConfig,

    // Input buffers
    pub input_buffer: String,
    pub config_preview: String,
    pub preview_name: String,
    pub preview_field: usize,  // 0 = name, 1 = save/cancel buttons

    // Status message (shown in info line, auto-clears after timeout)
    pub status_message: Option<String>,
    pub status_message_time: Option<Instant>,

    // Kill switch
    pub kill_switch_enabled: bool,

    // File browser state
    pub browser_path: std::path::PathBuf,
    pub browser_entries: Vec<BrowserEntry>,
    pub browser_selected: usize,

    // Tunnel config viewer (right side of tunnels box)
    pub tunnel_config_content: String,
    pub tunnel_config_scroll: usize,     // Scroll offset for display

    // Pending change countdown (3 second delay before applying rule/tunnel changes)
    pub pending_change: Option<PendingChange>,
    pub countdown_start: Option<Instant>,
    pub countdown_seconds: u8,           // Current countdown value for display

    // Info line content
    pub info_message: Option<String>,    // Current info message (traffic, status, etc.)
    
    // Rate limiting for status refresh
    pub last_status_refresh: Instant,    // When we last refreshed VPN status
    
    // Network connectivity status
    pub connectivity: ConnectivityStatus, // Current internet connectivity
    pub last_connectivity_check: Instant, // When we last checked connectivity
    pub vpn_health: VpnHealthCheck,       // Detailed VPN health status
    pub last_health_check: Instant,       // When we last did a full health check
    
    // Public IP tracking
    pub public_ip: Option<String>,        // Current public IP address
    pub ip_fetch_pending: bool,           // Whether we're waiting to fetch IP
}

#[derive(Debug, Clone)]
pub struct BrowserEntry {
    pub name: String,
    pub is_dir: bool,
    pub path: std::path::PathBuf,
}

impl App {
    pub async fn new() -> Result<Self> {
        let config = AppConfig::load().unwrap_or_default();
        let tunnels = crate::vpn::wireguard::list_profiles().await.unwrap_or_default();
        let vpn_status = crate::vpn::wireguard::get_status().await.unwrap_or_default();
        let networks = crate::network::get_networks().await.unwrap_or_default();
        
        // Get initial connectivity status
        let connectivity = crate::network::check_connectivity().await;
        let vpn_health = crate::vpn::wireguard::health_check().await;

        let mut app = Self {
            section: Section::Networks,
            popup: Popup::None,

            networks,
            selected_network: 0,

            tunnels,
            selected_tunnel: 0,
            vpn_status,

            network_rules: config.network_rules.clone(),

            config,

            input_buffer: String::new(),
            config_preview: String::new(),
            preview_name: String::new(),
            preview_field: 0,

            status_message: None,
            status_message_time: None,
            kill_switch_enabled: false,

            browser_path: dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/")),
            browser_entries: Vec::new(),
            browser_selected: 0,

            tunnel_config_content: String::new(),
            tunnel_config_scroll: 0,

            pending_change: None,
            countdown_start: None,
            countdown_seconds: 0,
            info_message: None,

            last_status_refresh: Instant::now(),
            
            connectivity,
            last_connectivity_check: Instant::now(),
            vpn_health,
            last_health_check: Instant::now(),
            
            public_ip: None,
            ip_fetch_pending: false,
        };

        // Check if kill switch is already enabled (from previous session)
        if crate::vpn::killswitch::is_enabled().await.unwrap_or(false) {
            app.kill_switch_enabled = true;
            tracing::info!("Kill switch already enabled from previous session");
        } else if app.vpn_status.connected {
        // If connected to a tunnel, restore its kill switch setting
            if let Some(iface) = &app.vpn_status.interface {
                let tunnel_ks = app.get_tunnel_info(iface)
                    .map(|t| t.kill_switch)
                    .unwrap_or(false);
                if tunnel_ks {
                    // Enable kill switch for this tunnel (no countdown on startup)
                    if crate::vpn::killswitch::enable().await.is_ok() {
                        app.kill_switch_enabled = true;
                    }
                }
            }
        }

        // Auto-reconnect to last tunnel if enabled and not already connected
        if !app.vpn_status.connected && app.config.auto_reconnect {
            if let Some(ref last_tunnel) = app.config.last_connected {
                // Check if this tunnel still exists
                if app.tunnels.iter().any(|t| &t.name == last_tunnel) {
                    tracing::info!("Auto-reconnecting to last tunnel: {}", last_tunnel);
                    if let Ok(_) = crate::vpn::wireguard::connect(last_tunnel).await {
                        // Refresh status after connecting
                        app.vpn_status = crate::vpn::wireguard::get_status().await.unwrap_or_default();
                        
                        // Enable kill switch if tunnel has it configured
                        let tunnel_ks = app.get_tunnel_info(last_tunnel)
                            .map(|t| t.kill_switch)
                            .unwrap_or(false);
                        if tunnel_ks {
                            if crate::vpn::killswitch::enable().await.is_ok() {
                                app.kill_switch_enabled = true;
                            }
                        }
                    }
                }
            }
        }

        // Load config for the initially selected tunnel
        app.load_selected_tunnel_config().await;

        Ok(app)
    }

    /// Set a status message (auto-clears after 3 seconds)
    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
        self.status_message_time = Some(Instant::now());
    }
    
    /// Get TunnelInfo for a tunnel by name
    fn get_tunnel_info(&self, name: &str) -> Option<&TunnelInfo> {
        self.config.known_tunnels.iter().find(|t| t.name == name)
    }

    /// Ensure a tunnel exists in known_tunnels and return mutable reference
    fn ensure_tunnel_info(&mut self, name: &str) -> &mut TunnelInfo {
        if !self.config.known_tunnels.iter().any(|t| t.name == name) {
            self.config.known_tunnels.push(TunnelInfo {
                name: name.to_string(),
                protocol: "wireguard".to_string(),
                kill_switch: false,
            });
        }
        self.config.known_tunnels.iter_mut().find(|t| t.name == name).unwrap()
    }

    /// Set kill switch for a specific tunnel
    fn set_tunnel_kill_switch(&mut self, name: &str, enabled: bool) {
        let tunnel = self.ensure_tunnel_info(name);
        tunnel.kill_switch = enabled;
        let _ = self.config.save();
    }

    /// Load the config file for the currently selected tunnel
    pub async fn load_selected_tunnel_config(&mut self) {
        if let Some(tunnel) = self.tunnels.get(self.selected_tunnel) {
            let tunnel_name = tunnel.name.clone();
            
            // Use the helper to read config (passwordless sudo)
            match crate::vpn::run_helper(&["config-read", &tunnel_name]).await {
                Ok(output) if output.status.success() => {
                    self.tunnel_config_content = String::from_utf8_lossy(&output.stdout).to_string();
                    self.tunnel_config_scroll = 0;
                }
                _ => {
                    self.tunnel_config_content = "# Unable to load config\n# Check permissions".to_string();
                }
            }
        } else {
            self.tunnel_config_content.clear();
        }
    }

    pub async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        // Handle popups first
        if self.popup != Popup::None {
            return self.handle_popup_key(key).await;
        }

        // Handle normal key input
        self.handle_normal_key(key).await
    }

    async fn handle_normal_key(&mut self, key: KeyEvent) -> Result<()> {
        // Escape cancels pending change
        if key.code == KeyCode::Esc && self.pending_change.is_some() {
            self.cancel_pending_change();
            self.set_status("Change cancelled");
            return Ok(());
        }

        match key.code {
            // Navigation between sections (Networks ↔ Tunnels ↔ KillSwitch)
            KeyCode::Tab => {
                self.section = match self.section {
                    Section::Networks => Section::Tunnels,
                    Section::Tunnels => Section::KillSwitch,
                    Section::KillSwitch => Section::Networks,
                };
            }
            KeyCode::BackTab => {
                self.section = match self.section {
                    Section::Networks => Section::KillSwitch,
                    Section::Tunnels => Section::Networks,
                    Section::KillSwitch => Section::Tunnels,
                };
            }

            // Vertical navigation (j/down, up only - 'k' is for kill switch)
            KeyCode::Char('j') | KeyCode::Down => self.move_down().await,
            KeyCode::Up => self.move_up().await,

            // Actions based on section
            KeyCode::Char(' ') | KeyCode::Enter => {
                match self.section {
                    Section::Tunnels => {
                // Space/Enter = connect/use tunnel now
                self.use_tunnel_now().await?;
                    }
                    Section::KillSwitch => {
                        // Space/Enter = toggle kill switch
                        self.toggle_kill_switch().await?;
                    }
                    _ => {}
                }
            }

            // Edit config in external editor (only in Tunnels section)
            KeyCode::Char('e') => {
                if self.section == Section::Tunnels && !self.tunnels.is_empty() {
                    self.edit_tunnel_config_external().await?;
                }
            }

            // New manual config creation (only in Tunnels section)
            KeyCode::Char('n') => {
                if self.section == Section::Tunnels {
                    self.start_manual_config();
                }
            }

            // Import config from file browser
            KeyCode::Char('i') => self.start_file_browser(),
            
            // Delete/remove
            KeyCode::Char('d') | KeyCode::Delete | KeyCode::Backspace => {
                self.delete_selection().await?;
            }
            
            // Refresh
            KeyCode::Char('R') => self.refresh().await?,
            
            // Toggle rule (cycle through: none -> always -> never -> none)
            KeyCode::Char('r') => self.cycle_tunnel_rule().await?,
            
            // Cycle through tunnels for selected network
            KeyCode::Char('t') => self.cycle_network_tunnel().await?,
            
            // Kill switch toggle (only when KillSwitch section is active)
            KeyCode::Char('k') => {
                if self.section == Section::KillSwitch {
                    self.toggle_kill_switch().await?;
                }
            }
            
            // Help (? or h)
            KeyCode::Char('?') | KeyCode::Char('h') => self.popup = Popup::Help,

            _ => {}
        }
        Ok(())
    }

    async fn handle_popup_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.popup {
            Popup::FileBrowser => self.handle_browser_key(key).await,
            Popup::ConfigPreview => self.handle_preview_key(key).await,
            Popup::ManualConfig => self.handle_manual_config_key(key).await,
            Popup::Help => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('h') | KeyCode::Enter | KeyCode::Char('q')) {
                    self.popup = Popup::None;
                }
                Ok(())
            }
            Popup::Confirm => {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Enter => {
                        self.confirm_action().await?;
                        self.popup = Popup::None;
                    }
                    KeyCode::Char('n') | KeyCode::Esc => {
                        self.popup = Popup::None;
                    }
                    _ => {}
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn move_down(&mut self) {
        match self.section {
            Section::Networks => {
                if !self.networks.is_empty() {
                    self.selected_network = (self.selected_network + 1) % self.networks.len();
                }
            }
            Section::Tunnels => {
                if !self.tunnels.is_empty() {
                    let old_selection = self.selected_tunnel;
                    self.selected_tunnel = (self.selected_tunnel + 1) % self.tunnels.len();
                    // Load config if selection changed
                    if old_selection != self.selected_tunnel {
                        self.load_selected_tunnel_config().await;
                    }
                }
            }
            Section::KillSwitch => {
                // No navigation in kill switch box (it's a single toggle)
            }
        }
    }

    async fn move_up(&mut self) {
        match self.section {
            Section::Networks => {
                if !self.networks.is_empty() {
                    self.selected_network = self.selected_network.checked_sub(1).unwrap_or(self.networks.len() - 1);
                }
            }
            Section::Tunnels => {
                if !self.tunnels.is_empty() {
                    let old_selection = self.selected_tunnel;
                    self.selected_tunnel = self.selected_tunnel.checked_sub(1).unwrap_or(self.tunnels.len() - 1);
                    // Load config if selection changed
                    if old_selection != self.selected_tunnel {
                        self.load_selected_tunnel_config().await;
                    }
                }
            }
            Section::KillSwitch => {
                // No navigation in kill switch box (it's a single toggle)
            }
        }
    }

    /// Edit tunnel config in external editor (opens new terminal window)
    async fn edit_tunnel_config_external(&mut self) -> Result<()> {
        if let Some(tunnel) = self.tunnels.get(self.selected_tunnel) {
            let tunnel_name = tunnel.name.clone();
            let was_connected = self.vpn_status.connected 
                && self.vpn_status.interface.as_deref() == Some(&tunnel_name);
            let config_path = format!("/etc/wireguard/{}.conf", tunnel_name);
            
            self.set_status(format!("Opening {} in editor...", tunnel_name));
            
            // Open a new terminal window for editing
            // This keeps the TUI intact and gives a clean prompt for sudo password
            let edit_cmd = format!("sudoedit '{}'", config_path);
            let title = format!("Edit {}", tunnel_name);
            
            // Try different terminal emulators (foot is common on Wayland/Omarchy)
            let terminals = [
                ("foot", vec!["--title", &title, "-W", "80x24", "-e", "sh", "-c", &edit_cmd]),
                ("kitty", vec!["--title", &title, "-e", "sh", "-c", &edit_cmd]),
                ("alacritty", vec!["--title", &title, "-e", "sh", "-c", &edit_cmd]),
                ("gnome-terminal", vec!["--title", &title, "--geometry=80x24", "--", "sh", "-c", &edit_cmd]),
                ("xterm", vec!["-title", &title, "-geometry", "80x24", "-e", "sh", "-c", &edit_cmd]),
            ];
            
            let mut spawned = false;
            for (term, args) in &terminals {
                if let Ok(mut child) = std::process::Command::new(term)
                    .args(args)
                    .spawn()
                {
                    // Wait for the terminal/editor to close
                    let _ = child.wait();
                    spawned = true;
                    break;
                }
            }
            
            if spawned {
                // Reload the config content
                self.load_selected_tunnel_config().await;
                
                // If tunnel was connected, reconnect to apply changes
                if was_connected {
                    self.set_status(format!("Reconnecting {} to apply changes...", tunnel_name));
                    let _ = crate::vpn::wireguard::disconnect().await;
                    match crate::vpn::wireguard::connect(&tunnel_name).await {
                        Ok(_) => {
                            self.set_status(format!("Config updated & {} reconnected", tunnel_name));
                        }
                        Err(e) => {
                            self.set_status(format!("Reconnect failed: {}", e));
                        }
                    }
                    self.refresh().await?;
                } else {
                    self.set_status(format!("Config reloaded for {}", tunnel_name));
                }
            } else {
                self.set_status("No terminal emulator found (tried foot, kitty, alacritty, gnome-terminal, xterm)");
            }
        }
        Ok(())
    }

    /// Connect to the selected tunnel now (one-time)
    async fn use_tunnel_now(&mut self) -> Result<()> {
        if self.section != Section::Tunnels {
            return Ok(());
        }

        if let Some(tunnel) = self.tunnels.get(self.selected_tunnel) {
            let tunnel_name = tunnel.name.clone();
            if self.vpn_status.connected && self.vpn_status.interface.as_deref() == Some(&tunnel_name) {
                // Already connected, disconnect
                // Disable kill switch when disconnecting
                if self.kill_switch_enabled {
                    let _ = crate::vpn::killswitch::disable().await;
                    self.kill_switch_enabled = false;
                }
                crate::vpn::wireguard::disconnect().await?;
                self.set_status("Disconnected");
            } else {
                // Disconnect any existing first (and their kill switch)
                if self.vpn_status.connected {
                    if self.kill_switch_enabled {
                        let _ = crate::vpn::killswitch::disable().await;
                        self.kill_switch_enabled = false;
                    }
                    crate::vpn::wireguard::disconnect().await?;
                }
                crate::vpn::wireguard::connect(&tunnel_name).await?;
                
                // Save last connected tunnel for auto-reconnect
                self.config.last_connected = Some(tunnel_name.clone());
                let _ = self.config.save();
                
                // Apply the tunnel's kill switch setting
                let tunnel_ks = self.get_tunnel_info(&tunnel_name)
                    .map(|t| t.kill_switch)
                    .unwrap_or(false);
                if tunnel_ks {
                    if let Ok(_) = crate::vpn::killswitch::enable().await {
                        self.kill_switch_enabled = true;
                        self.set_status(format!("Connected to {} (kill switch on)", tunnel_name));
                    } else {
                        self.set_status(format!("Connected to {}", tunnel_name));
                    }
                } else {
                    self.set_status(format!("Connected to {}", tunnel_name));
                }
            }
            self.refresh().await?;
        }
        Ok(())
    }

    /// Cycle through tunnel rules: none -> always -> never -> session -> none
    /// Works from Networks section, preserves tunnel selection
    /// For active networks, schedules a pending change with 3-second countdown
    async fn cycle_tunnel_rule(&mut self) -> Result<()> {
        // Only works in Networks section
        if self.section != Section::Networks {
            return Ok(());
        }

        let network = match self.networks.get(self.selected_network) {
            Some(n) => n.clone(),
            None => return Ok(()),
        };

        let identifier = network.identifier();
        let is_active = network.connected;

        // Find current rule
        let current_rule = self.network_rules
            .iter()
            .find(|r| r.identifier == identifier)
            .cloned();

        // Remove old rule
        self.network_rules.retain(|rule| rule.identifier != identifier);

        // Determine the current tunnel (preserve it across rule changes)
        let current_tunnel = current_rule.as_ref().and_then(|r| r.tunnel_name.clone());

        // Determine new rule and what action to take
        let (new_rule, action, status_text) = match current_rule {
            None => {
                // No rule -> Always (with first tunnel if none set)
                let tunnel_name = current_tunnel.or_else(|| {
                    self.tunnels.first().map(|t| t.name.clone())
                });
                let rule = NetworkRule {
                    identifier: identifier.clone(),
                    tunnel_name: tunnel_name.clone(),
                    always_vpn: true,
                    never_vpn: false,
                    session_vpn: false,
                };
                let action = if tunnel_name.is_some() { Some(PendingAction::Connect) } else { None };
                (Some(rule), action, format!("{}: Always", network.name))
            }
            Some(r) if r.always_vpn => {
                // Always -> Never (preserve tunnel, disconnect)
                let rule = NetworkRule {
                    identifier: identifier.clone(),
                    tunnel_name: current_tunnel,
                    always_vpn: false,
                    never_vpn: true,
                    session_vpn: false,
                };
                (Some(rule), Some(PendingAction::Disconnect), format!("{}: Never", network.name))
            }
            Some(r) if r.never_vpn => {
                // Never -> Session (preserve tunnel, connect)
                let tunnel = current_tunnel.clone();
                let rule = NetworkRule {
                    identifier: identifier.clone(),
                    tunnel_name: tunnel.clone(),
                    always_vpn: false,
                    never_vpn: false,
                    session_vpn: true,
                };
                let action = if tunnel.is_some() { Some(PendingAction::Connect) } else { None };
                (Some(rule), action, format!("{}: Session", network.name))
            }
            Some(_) => {
                // Session -> None (remove rule, disconnect)
                (None, Some(PendingAction::Disconnect), format!("{}: No rule", network.name))
            }
        };

        // Apply the new rule to config
        if let Some(rule) = new_rule {
            self.network_rules.push(rule);
        }
        self.config.network_rules = self.network_rules.clone();
        self.config.save()?;

        // For active networks, schedule the action with countdown
        if is_active {
            if let Some(act) = action {
                let tunnel_name = self.network_rules
                    .iter()
                    .find(|r| r.identifier == identifier)
                    .and_then(|r| r.tunnel_name.clone());
                
                self.schedule_change(PendingChange {
                    network_id: identifier,
                    network_name: network.name.clone(),
                    tunnel_name,
                    action: act,
                });
            }
        }

        self.set_status(status_text);
        Ok(())
    }

    /// Cycle through available tunnels for the selected network
    /// Preserves the Always/Never/Session rule setting
    /// For active networks with active rules, schedules reconnect with countdown
    async fn cycle_network_tunnel(&mut self) -> Result<()> {
        // Only works in Networks section
        if self.section != Section::Networks {
            return Ok(());
        }

        let network = match self.networks.get(self.selected_network) {
            Some(n) => n.clone(),
            None => return Ok(()),
        };

        if self.tunnels.is_empty() {
            self.set_status("No tunnels. Press 'f' to import.");
            return Ok(());
        }

        let identifier = network.identifier();
        let is_active = network.connected;

        // Find current rule
        let current_rule = self.network_rules
            .iter()
            .find(|r| r.identifier == identifier)
            .cloned();

        // Get current tunnel index
        let current_tunnel_idx = current_rule
            .as_ref()
            .and_then(|r| r.tunnel_name.as_ref())
            .and_then(|name| self.tunnels.iter().position(|t| &t.name == name));

        // Calculate next tunnel index (cycle through all tunnels, no "none" option)
        let next_tunnel_idx = match current_tunnel_idx {
            Some(idx) => (idx + 1) % self.tunnels.len(),
            None => 0,
        };

        let tunnel = &self.tunnels[next_tunnel_idx];
        let new_tunnel_name = tunnel.name.clone();

        // Preserve rule settings, default to Always if no rule exists
        let (always_vpn, never_vpn, session_vpn) = current_rule
            .as_ref()
            .map(|r| (r.always_vpn, r.never_vpn, r.session_vpn))
            .unwrap_or((true, false, false)); // Default to Always when first selecting tunnel

        // Remove old rule and add new one
        self.network_rules.retain(|r| r.identifier != identifier);
        self.network_rules.push(NetworkRule {
            identifier: identifier.clone(),
            tunnel_name: Some(new_tunnel_name.clone()),
            always_vpn,
            never_vpn,
            session_vpn,
        });

        let rule_text = if always_vpn { "Always" } else if session_vpn { "Session" } else if never_vpn { "Never" } else { "-" };
        self.set_status(format!("{}: {} → {}", network.name, rule_text, new_tunnel_name));

        self.config.network_rules = self.network_rules.clone();
        self.config.save()?;

        // For active networks with a "connect" rule (Always or Session), schedule reconnect
        if is_active && (always_vpn || session_vpn) {
            self.schedule_change(PendingChange {
                network_id: identifier,
                network_name: network.name.clone(),
                tunnel_name: Some(new_tunnel_name),
                action: PendingAction::Reconnect,
            });
        }

        Ok(())
    }

    fn start_file_browser(&mut self) {
        self.popup = Popup::FileBrowser;
        self.browser_path = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/"));
        self.browser_selected = 0;
        self.refresh_browser();
    }

    /// Start manual config creation popup
    fn start_manual_config(&mut self) {
        self.popup = Popup::ManualConfig;
        self.input_buffer.clear();  // Will hold the tunnel name
        self.config_preview.clear();  // Will hold the config content
        self.preview_field = 0;  // 0 = name field, 1 = content field
    }

    /// Handle key input for manual config creation popup
    async fn handle_manual_config_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                // Cancel and close
                self.popup = Popup::None;
                self.input_buffer.clear();
                self.config_preview.clear();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                // Toggle between name field (0) and content field (1)
                self.preview_field = if self.preview_field == 0 { 1 } else { 0 };
            }
            KeyCode::F(2) => {
                // F2 to save (when content is entered)
                if !self.input_buffer.is_empty() && !self.config_preview.is_empty() {
                    self.save_manual_config().await?;
                } else {
                    self.set_status("Enter name and config content first");
                }
            }
            KeyCode::Enter => {
                if self.preview_field == 0 {
                    // Move from name to content field
                    self.preview_field = 1;
                } else {
                    // In content field, Enter adds newline
                    self.config_preview.push('\n');
                }
            }
            KeyCode::Backspace => {
                if self.preview_field == 0 {
                    self.input_buffer.pop();
                } else {
                    self.config_preview.pop();
                }
            }
            KeyCode::Char(c) => {
                if self.preview_field == 0 {
                    // Name field: only valid filename characters
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        self.input_buffer.push(c);
                    }
                } else {
                    // Content field: any character
                    self.config_preview.push(c);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Save the manually created config
    async fn save_manual_config(&mut self) -> Result<()> {
        let name = self.input_buffer.clone();
        let content = self.config_preview.clone();

        match crate::vpn::wireguard::add_profile(&name, &content).await {
            Ok(_) => {
                self.set_status(format!("Created tunnel: {}", name));
                let _ = self.refresh().await;
                self.popup = Popup::None;
                self.input_buffer.clear();
                self.config_preview.clear();
            }
            Err(e) => {
                self.set_status(format!("Failed: {}", e));
                // Don't close popup on error
            }
        }
        Ok(())
    }

    fn refresh_browser(&mut self) {
        self.browser_entries.clear();
        
        // Add parent directory entry if not at root
        if self.browser_path.parent().is_some() {
            self.browser_entries.push(BrowserEntry {
                name: "..".to_string(),
                is_dir: true,
                path: self.browser_path.parent().unwrap().to_path_buf(),
            });
        }

        // Read directory contents
        if let Ok(entries) = std::fs::read_dir(&self.browser_path) {
            let mut dirs: Vec<BrowserEntry> = Vec::new();
            let mut files: Vec<BrowserEntry> = Vec::new();

            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                
                // Skip hidden files
                if name.starts_with('.') {
                    continue;
                }

                if path.is_dir() {
                    dirs.push(BrowserEntry {
                        name,
                        is_dir: true,
                        path,
                    });
                } else if name.ends_with(".conf") {
                    files.push(BrowserEntry {
                        name,
                        is_dir: false,
                        path,
                    });
                }
            }

            // Sort alphabetically
            dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

            self.browser_entries.extend(dirs);
            self.browser_entries.extend(files);
        }

        if self.browser_selected >= self.browser_entries.len() {
            self.browser_selected = 0;
        }
    }

    async fn handle_browser_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.popup = Popup::None;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.browser_entries.is_empty() {
                    self.browser_selected = (self.browser_selected + 1) % self.browser_entries.len();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !self.browser_entries.is_empty() {
                    self.browser_selected = self.browser_selected.checked_sub(1)
                        .unwrap_or(self.browser_entries.len() - 1);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(entry) = self.browser_entries.get(self.browser_selected).cloned() {
                    if entry.is_dir {
                        self.browser_path = entry.path;
                        self.browser_selected = 0;
                        self.refresh_browser();
                    } else {
                        // Load file and show preview
                        self.load_config_preview(&entry.path)?;
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(parent) = self.browser_path.parent() {
                    self.browser_path = parent.to_path_buf();
                    self.browser_selected = 0;
                    self.refresh_browser();
                }
            }
            KeyCode::Char('h') => {
                self.browser_path = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/"));
                self.browser_selected = 0;
                self.refresh_browser();
            }
            _ => {}
        }
        Ok(())
    }

    fn load_config_preview(&mut self, path: &std::path::Path) -> Result<()> {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                if content.contains("[Interface]") && content.contains("[Peer]") {
                    self.config_preview = content;
                    self.preview_name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("tunnel")
                        .to_string();
                    self.input_buffer = self.preview_name.clone();
                    self.popup = Popup::ConfigPreview;
                    self.preview_field = 0;  // Start on name field
                } else {
                    self.set_status("Not a valid WireGuard config");
                }
            }
            Err(e) => {
                self.set_status(format!("Cannot read: {}", e));
            }
        }
        Ok(())
    }

    async fn handle_preview_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.popup = Popup::FileBrowser;
                self.config_preview.clear();
                self.input_buffer.clear();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                // Toggle between name field (0) and action buttons (1)
                self.preview_field = if self.preview_field == 0 { 1 } else { 0 };
            }
            KeyCode::Enter => {
                if self.preview_field == 1 {
                    // On action bar, Enter = save
                    self.save_imported_config().await?;
                } else {
                    // On name field, Enter moves to action bar
                    self.preview_field = 1;
                }
            }
            KeyCode::Backspace => {
                if self.preview_field == 0 {
                    self.input_buffer.pop();
                }
            }
            KeyCode::Char(c) => {
                if self.preview_field == 0 {
                    // Only allow valid filename characters in name field
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        self.input_buffer.push(c);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn save_imported_config(&mut self) -> Result<()> {
        let name = if self.input_buffer.is_empty() {
            self.preview_name.clone()
        } else {
            self.input_buffer.clone()
        };

        match crate::vpn::wireguard::add_profile(&name, &self.config_preview).await {
            Ok(_) => {
                self.set_status(format!("Saved tunnel: {}", name));
                let _ = self.refresh().await;
            }
            Err(e) => {
                self.set_status(format!("Failed: {}", e));
                return Ok(()); // Don't close popup on error
            }
        }

        self.popup = Popup::None;
        self.config_preview.clear();
        self.input_buffer.clear();
        Ok(())
    }

    async fn delete_selection(&mut self) -> Result<()> {
        match self.section {
            Section::Tunnels => {
                if let Some(tunnel) = self.tunnels.get(self.selected_tunnel) {
                    self.input_buffer = tunnel.name.clone(); // Store name for confirm
                    self.set_status(format!("Delete '{}'? (y/n)", tunnel.name));
                    self.popup = Popup::Confirm;
                }
            }
            Section::Networks => {
                // Forget network entirely
                if let Some(network) = self.networks.get(self.selected_network) {
                    self.input_buffer = network.name.clone(); // Store name for confirm
                    self.set_status(format!("Forget network '{}'? (y/n)", network.name));
                    self.popup = Popup::Confirm;
                }
            }
            Section::KillSwitch => {
                // No delete action for kill switch
            }
        }
        Ok(())
    }

    async fn confirm_action(&mut self) -> Result<()> {
        // Delete the tunnel OR forget network
        if self.section == Section::Networks {
             let network_name = self.input_buffer.clone();
             self.input_buffer.clear();
             
             if let Some(network) = self.networks.iter().find(|n| n.name == network_name) {
                 // 1. Remove rules
                 let identifier = network.identifier();
                 self.network_rules.retain(|r| r.identifier != identifier);
                 self.config.network_rules = self.network_rules.clone();
                 self.config.save()?;
                 
                 // 2. Forget network from system
                 match crate::network::forget_network(network).await {
                     Ok(_) => {
                         self.set_status(format!("Forgot network '{}'", network_name));
                         self.refresh().await?;
                     }
                     Err(e) => {
                         self.set_status(format!("Error: {}", e));
                     }
                 }
             }
             return Ok(());
        }

        // Tunnels section handling
        let tunnel_name = self.input_buffer.clone();
        self.input_buffer.clear();
        
        if tunnel_name.is_empty() {
            return Ok(());
        }

        // Delete the tunnel from /etc/wireguard and our config
        match crate::vpn::wireguard::delete_profile(&tunnel_name).await {
            Ok(_) => {
                // Also remove this tunnel from any network rules
                for rule in &mut self.network_rules {
                    if rule.tunnel_name.as_ref() == Some(&tunnel_name) {
                        rule.tunnel_name = None;
                    }
                }
                self.config.network_rules = self.network_rules.clone();
                self.config.save()?;
                
                self.refresh().await?;
                self.set_status(format!("Deleted '{}'", tunnel_name));
                
                // Adjust selection if needed
                if self.selected_tunnel >= self.tunnels.len() && !self.tunnels.is_empty() {
                    self.selected_tunnel = self.tunnels.len() - 1;
                }
            }
            Err(e) => {
                self.set_status(format!("Delete failed: {}", e));
            }
        }
        Ok(())
    }

    async fn refresh(&mut self) -> Result<()> {
        self.tunnels = crate::vpn::wireguard::list_profiles().await.unwrap_or_default();
        self.vpn_status = crate::vpn::wireguard::get_status().await.unwrap_or_default();
        self.networks = crate::network::get_networks().await.unwrap_or_default();
        Ok(())
    }

    async fn toggle_kill_switch(&mut self) -> Result<()> {
        // Toggle the visual state immediately for feedback
        let new_state = !self.kill_switch_enabled;
        
        // Schedule the change with countdown
        let action = if new_state {
            PendingAction::KillSwitchOn
        } else {
            PendingAction::KillSwitchOff
        };
        
        self.schedule_change(PendingChange {
            network_id: String::new(),
            network_name: String::new(),
            tunnel_name: None,
            action,
        });
        
        // Show immediate feedback
        self.set_status(format!(
            "Kill switch → {} ({}s)",
            if new_state { "ON" } else { "OFF" },
            COUNTDOWN_SECONDS
        ));
        
        Ok(())
    }


    pub async fn tick(&mut self) -> Result<()> {
        // Handle pending change countdown
        if let Some(start) = self.countdown_start {
            let elapsed = start.elapsed().as_secs();
            let remaining = COUNTDOWN_SECONDS.saturating_sub(elapsed);
            self.countdown_seconds = remaining as u8;

            if remaining == 0 {
                // Time's up - apply the pending change
                self.apply_pending_change().await?;
            }
        }
        
        // Clear status message after 3 seconds
        if let Some(time) = self.status_message_time {
            if time.elapsed().as_secs() >= 3 {
                self.status_message = None;
                self.status_message_time = None;
            }
        }

        // Refresh VPN status for live traffic stats (every 1 second to avoid too many sudo calls)
        if self.last_status_refresh.elapsed().as_millis() >= 1000 {
            let was_connected = self.vpn_status.connected;
            self.vpn_status = crate::vpn::wireguard::get_status().await.unwrap_or_default();
            self.last_status_refresh = Instant::now();
            
            // Trigger IP fetch when VPN just connected
            if !was_connected && self.vpn_status.connected {
                self.ip_fetch_pending = true;
            }
            
            // Clear IP when VPN disconnects
            if was_connected && !self.vpn_status.connected {
                self.public_ip = None;
            }
        }
        
        // Fetch public IP if pending (do this after a short delay to allow connection to stabilize)
        // Skip if kill switch is enabled (traffic is blocked, will timeout)
        if self.ip_fetch_pending && self.vpn_status.connected && !self.kill_switch_enabled {
            self.ip_fetch_pending = false;
            // Spawn IP fetch - don't block the UI
            if let Some(ip) = crate::network::get_public_ip().await {
                self.public_ip = Some(ip);
            }
        }
        
        // Periodic connectivity check (every 10 seconds)
        // Skip if kill switch is enabled (we know traffic is blocked except through VPN)
        if !self.kill_switch_enabled && self.last_connectivity_check.elapsed().as_secs() >= 10 {
            self.connectivity = crate::network::check_connectivity().await;
            self.last_connectivity_check = Instant::now();
        }
        
        // Periodic VPN health check (every 30 seconds when connected)
        // Skip if kill switch is enabled (health check requires network access)
        if self.vpn_status.connected && !self.kill_switch_enabled && self.last_health_check.elapsed().as_secs() >= 30 {
            self.vpn_health = crate::vpn::wireguard::health_check().await;
            self.last_health_check = Instant::now();
        }

        // Update info message with VPN traffic stats if connected
        if self.pending_change.is_none() {
            self.update_info_message();
        }

        Ok(())
    }

    /// Parse transfer string like "1.23 GiB" or "1.23 GiB received" to bytes
    fn parse_transfer_to_bytes(s: &str) -> u64 {
        let parts: Vec<&str> = s.trim().split_whitespace().collect();
        if parts.len() < 2 {
            return 0;
        }
        
        let value: f64 = parts[0].parse().unwrap_or(0.0);
        let unit = parts[1].to_lowercase();
        
        let multiplier: u64 = match unit.as_str() {
            "b" => 1,
            "kib" => 1024,
            "mib" => 1024 * 1024,
            "gib" => 1024 * 1024 * 1024,
            "tib" => 1024 * 1024 * 1024 * 1024,
            // Also handle SI units
            "kb" => 1000,
            "mb" => 1000 * 1000,
            "gb" => 1000 * 1000 * 1000,
            "tb" => 1000 * 1000 * 1000 * 1000,
            _ => 1,
        };
        
        (value * multiplier as f64) as u64
    }

    /// Format bytes to human-readable string
    fn format_bytes(bytes: u64) -> String {
        const KIB: u64 = 1024;
        const MIB: u64 = KIB * 1024;
        const GIB: u64 = MIB * 1024;
        const TIB: u64 = GIB * 1024;
        
        if bytes >= TIB {
            format!("{:.2} TiB", bytes as f64 / TIB as f64)
        } else if bytes >= GIB {
            format!("{:.2} GiB", bytes as f64 / GIB as f64)
        } else if bytes >= MIB {
            format!("{:.2} MiB", bytes as f64 / MIB as f64)
        } else if bytes >= KIB {
            format!("{:.2} KiB", bytes as f64 / KIB as f64)
        } else {
            format!("{} B", bytes)
        }
    }

    /// Format duration to human-readable string
    fn format_duration(secs: u64) -> String {
        if secs < 60 {
            format!("{}s", secs)
        } else if secs < 3600 {
            let mins = secs / 60;
            let secs = secs % 60;
            if secs == 0 {
                format!("{}m", mins)
            } else {
                format!("{}m {}s", mins, secs)
            }
        } else {
            let hours = secs / 3600;
            let mins = (secs % 3600) / 60;
            if mins == 0 {
                format!("{}h", hours)
            } else {
                format!("{}h {}m", hours, mins)
            }
        }
    }

    /// Update the info message with current status/traffic
    fn update_info_message(&mut self) {
        if self.vpn_status.connected {
            let mut parts = Vec::new();
            
            // VPN health indicator
            let health_icon = if self.vpn_health.is_healthy() {
                "󰒘" // Connected and healthy
            } else if self.vpn_health.is_degraded() {
                "󰒙" // Connected but degraded
            } else {
                "󰒍" // Connected but issues
            };
            
            // Interface name with health indicator
            if let Some(ref iface) = self.vpn_status.interface {
                parts.push(format!("{} {}", health_icon, iface));
            }
            
            // Public IP address (if available)
            if let Some(ref ip) = self.public_ip {
                parts.push(format!("󰩟 {}", ip));
            }
            
            // Session duration - use actual interface uptime from system
            if let Some(ref iface) = self.vpn_status.interface {
                if let Some(uptime_secs) = crate::vpn::wireguard::get_interface_uptime(iface) {
                    parts.push(format!("󰔟 {}", Self::format_duration(uptime_secs)));
                }
            }
            
            // Cumulative session traffic (total since connection established)
            if let (Some(ref rx), Some(ref tx)) = (&self.vpn_status.transfer_rx, &self.vpn_status.transfer_tx) {
                let total_rx = Self::parse_transfer_to_bytes(rx);
                let total_tx = Self::parse_transfer_to_bytes(tx);
                
                parts.push(format!("↓{} ↑{}", 
                    Self::format_bytes(total_rx), 
                    Self::format_bytes(total_tx)
                ));
            }
            
            // Tunnel type indicator
            if self.vpn_status.routing_ok {
                parts.push("󰒘 Full".to_string());  // All traffic through VPN
            } else {
                parts.push("󰒙 Split".to_string()); // Only specific IPs through VPN
            }
            
            // Status warnings - skip when kill switch is on (expected behavior)
            if !self.kill_switch_enabled {
                if self.vpn_status.handshake_stale {
                    parts.push("⏳ stale".to_string());
                } else if !self.vpn_health.can_reach_internet && self.vpn_health.interface_exists {
                    parts.push("⚠ no internet".to_string());
                }
            }
            
            self.info_message = if parts.is_empty() {
                None
            } else {
                Some(parts.join(" │ "))
            };
        } else {
            // Show network connectivity status
            if !self.connectivity.has_interface {
                self.info_message = Some("󰤭 No network".to_string());
            } else if !self.connectivity.has_ip_address {
                self.info_message = Some("󰤫 No IP address".to_string());
            } else if !self.connectivity.has_internet {
                if self.connectivity.can_reach_gateway {
                    self.info_message = Some("󰤩 No internet (captive portal?)".to_string());
                } else {
                    self.info_message = Some("󰤩 No internet".to_string());
                }
            } else if let Some(network) = self.networks.iter().find(|n| n.connected) {
                // Online but no VPN
                self.info_message = Some(format!("󰖩 {} (no VPN)", network.name));
            } else {
                self.info_message = Some("󰖩 Online (no VPN)".to_string());
            }
        }
    }

    /// Apply the pending configuration change
    async fn apply_pending_change(&mut self) -> Result<()> {
        if let Some(change) = self.pending_change.take() {
            self.countdown_start = None;
            self.countdown_seconds = 0;

            match change.action {
                PendingAction::Connect => {
                    if let Some(tunnel) = &change.tunnel_name {
                        self.set_status(format!("Connecting to {}...", tunnel));
                        match crate::vpn::wireguard::connect(tunnel).await {
                            Ok(_) => {
                                // Save last connected tunnel for auto-reconnect
                                self.config.last_connected = Some(tunnel.clone());
                                let _ = self.config.save();
                                
                                // Apply tunnel's kill switch setting
                                let tunnel_ks = self.get_tunnel_info(tunnel)
                                    .map(|t| t.kill_switch)
                                    .unwrap_or(false);
                                if tunnel_ks {
                                    if let Ok(_) = crate::vpn::killswitch::enable().await {
                                        self.kill_switch_enabled = true;
                                        self.set_status(format!("Connected to {} (kill switch on)", tunnel));
                                    } else {
                                        self.set_status(format!("Connected to {}", tunnel));
                                    }
                                } else {
                                    self.set_status(format!("Connected to {}", tunnel));
                                }
                            }
                            Err(e) => {
                                self.set_status(format!("Error: {}", e));
                            }
                        }
                    }
                }
                PendingAction::Disconnect => {
                    self.set_status("Disconnecting...");
                    // Disable kill switch when disconnecting
                    if self.kill_switch_enabled {
                        let _ = crate::vpn::killswitch::disable().await;
                        self.kill_switch_enabled = false;
                    }
                    match crate::vpn::wireguard::disconnect().await {
                        Ok(_) => {
                            self.set_status("Disconnected");
                        }
                        Err(e) => {
                            self.set_status(format!("Error: {}", e));
                        }
                    }
                }
                PendingAction::Reconnect => {
                    if let Some(tunnel) = &change.tunnel_name {
                        self.set_status(format!("Switching to {}...", tunnel));
                        // Disable old kill switch before switching
                        if self.kill_switch_enabled {
                            let _ = crate::vpn::killswitch::disable().await;
                            self.kill_switch_enabled = false;
                        }
                        let _ = crate::vpn::wireguard::disconnect().await;
                        match crate::vpn::wireguard::connect(tunnel).await {
                            Ok(_) => {
                                // Save last connected tunnel for auto-reconnect
                                self.config.last_connected = Some(tunnel.clone());
                                let _ = self.config.save();
                                
                                // Apply new tunnel's kill switch setting
                                let tunnel_ks = self.get_tunnel_info(tunnel)
                                    .map(|t| t.kill_switch)
                                    .unwrap_or(false);
                                if tunnel_ks {
                                    if let Ok(_) = crate::vpn::killswitch::enable().await {
                                        self.kill_switch_enabled = true;
                                        self.set_status(format!("Connected to {} (kill switch on)", tunnel));
                                    } else {
                                        self.set_status(format!("Connected to {}", tunnel));
                                    }
                                } else {
                                    self.set_status(format!("Connected to {}", tunnel));
                                }
                            }
                            Err(e) => {
                                self.set_status(format!("Error: {}", e));
                            }
                        }
                    }
                }
                PendingAction::KillSwitchOn => {
                    self.set_status("Enabling kill switch...");
                    match crate::vpn::killswitch::enable().await {
                        Ok(_) => {
                            self.kill_switch_enabled = true;
                            // Save per-tunnel if connected, otherwise global
                            if let Some(iface) = self.vpn_status.interface.clone() {
                                self.set_tunnel_kill_switch(&iface, true);
                                self.set_status(format!("Kill switch enabled for {}", iface));
                            } else {
                                self.config.kill_switch = true;
                                let _ = self.config.save();
                                self.set_status("Kill switch enabled");
                            }
                        }
                        Err(e) => {
                            self.set_status(format!("Error: {}", e));
                        }
                    }
                }
                PendingAction::KillSwitchOff => {
                    self.set_status("Disabling kill switch...");
                    match crate::vpn::killswitch::disable().await {
                        Ok(_) => {
                            self.kill_switch_enabled = false;
                            // Save per-tunnel if connected, otherwise global
                            if let Some(iface) = self.vpn_status.interface.clone() {
                                self.set_tunnel_kill_switch(&iface, false);
                                self.set_status(format!("Kill switch disabled for {}", iface));
                            } else {
                                self.config.kill_switch = false;
                                let _ = self.config.save();
                                self.set_status("Kill switch disabled");
                            }
                        }
                        Err(e) => {
                            self.set_status(format!("Error: {}", e));
                        }
                    }
                }
            }

            // Refresh status
            self.refresh().await?;
        }
        Ok(())
    }

    /// Schedule a pending change with countdown (resets if already pending)
    fn schedule_change(&mut self, change: PendingChange) {
        self.pending_change = Some(change);
        self.countdown_start = Some(Instant::now());
        self.countdown_seconds = COUNTDOWN_SECONDS as u8;
    }

    /// Cancel any pending change
    pub fn cancel_pending_change(&mut self) {
        self.pending_change = None;
        self.countdown_start = None;
        self.countdown_seconds = 0;
    }

    /// Get the rule for a specific network
    pub fn get_network_rule(&self, network: &NetworkInfo) -> Option<&NetworkRule> {
        self.network_rules.iter().find(|r| r.identifier == network.identifier())
    }
}
