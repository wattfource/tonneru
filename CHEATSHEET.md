# 󰖂 Tonneru - Install & Update Cheat Sheet

## First Time Install

```bash
cd /path/to/tonneru
./install.sh
systemctl --user enable --now tonneru.service
```

## Update (after code changes)

```bash
cd /path/to/tonneru
./update.sh
```

Quick mode (skip rebuild if binary exists):
```bash
./update.sh quick
```

## What update.sh Does

1. Builds release binary
2. Installs `/usr/bin/tonneru`
3. Installs `/etc/sudoers.d/tonneru`
4. Installs `/usr/lib/systemd/user/tonneru.service`
5. Reloads systemd daemon
6. Restarts tonneru service (if running)
7. Reloads waybar (if running)

*This mirrors AUR package installation behavior.*

## Service Management

```bash
systemctl --user status tonneru      # Check status
systemctl --user restart tonneru     # Restart
systemctl --user stop tonneru        # Stop
systemctl --user start tonneru       # Start
systemctl --user disable tonneru     # Disable autostart
systemctl --user enable tonneru      # Enable autostart
```

## Debugging

```bash
journalctl --user -u tonneru -f              # Live logs
journalctl --user -u tonneru --since "5m"    # Last 5 minutes
tonneru --status | jq .                      # Test JSON output
RUST_LOG=debug tonneru --daemon              # Run daemon with debug logging
```

## Quick Commands

```bash
tonneru                  # Launch TUI
tonneru --status         # JSON status (for waybar)
tonneru --connect NAME   # Connect to tunnel
tonneru --disconnect     # Disconnect VPN
```

## File Locations

| File | Location |
|------|----------|
| Binary | `/usr/bin/tonneru` |
| Sudoers | `/etc/sudoers.d/tonneru` |
| Systemd service | `/usr/lib/systemd/user/tonneru.service` |
| User config | `~/.config/tonneru/config.toml` |
| WireGuard configs | `/etc/wireguard/*.conf` |

## AUR Package

When installed from AUR, updates via:
```bash
yay -Syu tonneru
systemctl --user restart tonneru.service
```
