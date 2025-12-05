# 󰖂 tonneru

A WireGuard VPN manager for Omarchy (Arch Linux). Built with Rust and ratatui.

![tonneru](https://img.shields.io/badge/arch-linux-1793d1?logo=archlinux&logoColor=white)
![Rust](https://img.shields.io/badge/rust-stable-orange?logo=rust)
![License](https://img.shields.io/badge/license-WTFPL-green)

---

## 📸 Features

- **WireGuard VPN management** via wg-quick
- **Per-network rules** - auto-connect/disconnect VPN based on WiFi SSID
  - **Always** - auto-connect when on this network
  - **Never** - auto-disconnect on this network  
  - **Session** - use VPN for this session only (clears on disconnect)
- **Smart status detection** - shows routing health, not just interface state
- **3-second countdown** - preview changes before they apply
- **Kill switch** - block all traffic if VPN drops (using nftables)
- **System theme support** - automatically uses Omarchy/Hyprland theme colors
- **TUI interface** - keyboard-driven, no mouse needed
- **Waybar integration** - show VPN status in your status bar
- **Hyprland-ready** - floating window rules included
- **Desktop notifications** - get notified on connect/disconnect
- **iwd & NetworkManager support** - works with either network backend

---

## 💡 Prerequisites

- Arch Linux (or any Linux with the required packages)
- **Either** iwd (recommended for Omarchy) **or** NetworkManager
- WireGuard tools (`wireguard-tools`)
- nftables (for kill switch)

---

## 🚀 Installation

### From AUR

```bash
# Using yay
yay -S tonneru

# Or using paru
paru -S tonneru
```

### From Source

```bash
git clone https://github.com/WattForce/tonneru.git
cd tonneru
cargo build --release

# Install binary
sudo install -Dm755 target/release/tonneru /usr/bin/tonneru

# Install sudoers for passwordless VPN management (recommended)
sudo install -Dm440 packaging/sudoers/tonneru /etc/sudoers.d/tonneru
```

### Enable Passwordless Operation

The sudoers file allows members of the `wheel` group to manage VPN connections without password prompts.

**Make sure your user is in the wheel group:**

```bash
# Check if you're in wheel group
groups | grep wheel

# Add yourself to wheel group (if not already)
sudo usermod -aG wheel $USER
# Log out and back in for changes to take effect
```

---

## 🪄 Usage

### Launch TUI

```bash
tonneru
```

### CLI Commands

```bash
# Show current VPN status (JSON, for waybar)
tonneru --status

# Connect to a profile
tonneru --connect my-vpn

# Disconnect
tonneru --disconnect

# Run as daemon (auto-connect based on network rules)
tonneru --daemon
```

---

## ⌨️ Keybindings

### Navigation

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Switch between sections |
| `j` / `↓` | Move down |
| `↑` | Move up |

### Actions (Tunnels Section)

| Key | Action |
|-----|--------|
| `Enter` / `Space` | Connect/Disconnect VPN |
| `f` | Import WireGuard .conf file |
| `c` | Edit tunnel config |
| `k` | Toggle kill switch |
| `d` | Delete tunnel |

### Network Rules (Networks Section)

| Key | Action |
|-----|--------|
| `r` | Cycle rule (Always → Never → Session → None) |
| `t` | Cycle tunnel assignment |
| `d` | Remove rule for network |

### General

| Key | Action |
|-----|--------|
| `?` | Show help |
| `Esc` | Cancel pending change / Close popup |
| `q` / `Ctrl+C` | Quit |

---

## 🌐 Network Rules

Set per-network VPN behavior:

1. Navigate to the **Networks** section (`Tab`)
2. Select a network (WiFi SSID)
3. Press `r` to cycle through rules:
   - **Always** - Auto-connect VPN when on this network
   - **Never** - Auto-disconnect VPN on this network
   - **Session** - Use VPN for current session only (clears when network changes)
   - *(none)* - No automatic action
4. Press `t` to assign which tunnel to use

**Countdown Timer:** When changing rules on an active network, a 3-second countdown appears. Make another change to reset the timer, or press `Esc` to cancel.

Run `tonneru --daemon` to enable auto-connect behavior in the background.

---

## 🔄 Sleep/Wake Resilience

The daemon mode (`tonneru --daemon`) includes resilient handling of power state changes:

- **Sleep/Resume Detection** - Automatically detects when your computer wakes from sleep
- **Network Reconnection** - Waits for network to stabilize after resume
- **VPN Verification** - Checks if VPN is still connected and working after wake
- **Auto-Reconnect** - Reconnects VPN based on network rules if disconnected
- **Health Monitoring** - Periodically verifies VPN is actually passing traffic

When the computer resumes from sleep:
1. The daemon detects the time gap indicating a resume event
2. Waits up to 30 seconds for network interfaces to come up
3. Verifies internet connectivity (checks gateway and external hosts)
4. Checks if VPN should be connected based on current network rules
5. Reconnects or verifies VPN as needed
6. Sends desktop notification with status

---

## 📊 VPN Status Indicators

### Tunnel Status
| Icon | Meaning |
|------|---------|
| `󰒘` | Connected and healthy - VPN working correctly |
| `󰒙` | Connected but degraded - may need attention |
| `󰒍` | Connected but issues - routing or handshake problems |

### Info Bar Status
| Indicator | Meaning |
|-----------|---------|
| `⚠ no route` | VPN interface up but traffic not routing through it |
| `⏳ stale` | Handshake is old - connection may be dead |
| `⚠ no internet` | VPN connected but can't reach internet |

### Network Status (when VPN disconnected)
| Icon | Meaning |
|------|---------|
| `󰖩` | Online - network connected, no VPN |
| `󰤭` | No network interface available |
| `󰤫` | Network up but no IP address |
| `󰤩` | No internet - may be captive portal |

---

## 🛡️ Kill Switch

The kill switch blocks all network traffic except through the VPN tunnel. This prevents data leaks if the VPN disconnects unexpectedly.

Press `k` in the Tunnels section to toggle the kill switch.

**Note:** The kill switch uses nftables rules. Make sure nftables is installed.

---

## 🎨 Theme Support

tonneru automatically reads your Omarchy/Hyprland theme colors from:
```
~/.config/omarchy/current/theme/kitty.conf
```

The UI will match your system theme (Matte Black, etc.) automatically.

---

## 📊 Waybar Integration

Add to your waybar config (`~/.config/waybar/config`):

```json
"custom/vpn": {
    "exec": "tonneru --status 2>/dev/null",
    "return-type": "json",
    "interval": 5,
    "on-click": "tonneru",
    "format": "{icon}",
    "format-icons": {
        "connected": "󰒘",
        "disconnected": "󰒙"
    },
    "tooltip": true,
    "exec-if": "which tonneru"
}
```

Add to your waybar style (`~/.config/waybar/style.css`):

```css
#custom-vpn.connected {
    color: #a6e3a1;
}

#custom-vpn.disconnected {
    color: #f38ba8;
}
```

---

## 🪟 Hyprland Window Rules

Add to your `hyprland.conf`:

```conf
windowrule = float, title:(tonneru)
windowrule = size 700 500, title:(tonneru)
windowrule = center, title:(tonneru)
```

---

## 📂 Configuration

### User Config

User configuration is stored in:

```
~/.config/tonneru/config.toml
```

Example:

```toml
kill_switch = false
notifications = true

[[known_tunnels]]
name = "work-vpn"
protocol = "wireguard"

[[network_rules]]
identifier = "wifi:HomeNetwork"
tunnel_name = "work-vpn"
always_vpn = false
never_vpn = true
session_vpn = false

[[network_rules]]
identifier = "wifi:CoffeeShop"
tunnel_name = "work-vpn"
always_vpn = true
never_vpn = false
session_vpn = false
```

### WireGuard Configs

WireGuard configuration files are stored in:

```
/etc/wireguard/*.conf
```

---

## 🔧 Troubleshooting

### "Permission denied" errors

Make sure the sudoers file is installed:

```bash
sudo install -Dm440 packaging/sudoers/tonneru /etc/sudoers.d/tonneru
```

And that you're in the `wheel` group:

```bash
sudo usermod -aG wheel $USER
# Log out and back in
```

### WiFi SSID not showing

If using iwd (Omarchy default), check:

```bash
iwctl station wlan0 show
```

If using NetworkManager:

```bash
nmcli connection show
```

### VPN shows "UP ⚠" (routing issue)

This means the WireGuard interface is up but traffic isn't routing through it. Check your WireGuard config has proper `AllowedIPs`:

```ini
[Peer]
AllowedIPs = 0.0.0.0/0, ::/0
```

---

## 🚧 Roadmap

- [x] WireGuard support
- [x] iwd support
- [x] NetworkManager support
- [x] Waybar integration
- [x] System theme support
- [x] Session-only rules
- [x] Smart status detection
- [x] Sleep/wake resilience - auto-reconnect after suspend/resume
- [x] Connectivity verification - detect and report actual internet status
- [x] VPN health checking - verify tunnel is actually working
- [ ] OpenVPN support
- [ ] Split tunneling
- [ ] Import from QR code

---

## 🧾 License

[WTFPL](LICENSE) - Do What The Fuck You Want To Public License

---

## ❤️ Credits

Built for [Omarchy](https://github.com/omarchy) by Sean Fournier.

Inspired by:
- [netpala](https://github.com/joel-sgc/netpala) - NetworkManager TUI
- [bluepala](https://github.com/joel-sgc/bluepala) - Bluetooth TUI
