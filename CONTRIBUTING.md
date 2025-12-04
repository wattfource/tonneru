# Contributing to tonneru

Thank you for your interest in contributing! This document provides guidelines and information for contributors.

## 📋 Project Overview

**tonneru** is a terminal-based VPN manager for Arch Linux / Omarchy, written in Rust using the [ratatui](https://github.com/ratatui/ratatui) TUI framework.

### Key Technologies

| Component | Technology |
|-----------|------------|
| Language | Rust (2021 edition) |
| TUI Framework | ratatui + crossterm |
| Async Runtime | tokio |
| Config Format | TOML |
| VPN Backend | WireGuard (wg-quick) |
| Firewall | nftables (kill switch) |
| Network Detection | iwd (iwctl) or NetworkManager (nmcli) |

### Project Structure

```
tonneru/
├── src/
│   ├── main.rs          # Entry point, CLI parsing, TUI setup
│   ├── app.rs           # Application state and key handling
│   ├── config/
│   │   └── mod.rs       # User configuration (TOML)
│   ├── network/
│   │   ├── mod.rs       # Network detection (iwd/NetworkManager)
│   │   └── monitor.rs   # Background network monitoring
│   ├── theme.rs         # System theme color loading
│   ├── ui/
│   │   ├── mod.rs       # UI rendering (ratatui)
│   │   └── components.rs # Reusable UI components (placeholder)
│   └── vpn/
│       ├── mod.rs       # VPN module exports
│       ├── wireguard.rs # WireGuard operations (wg-quick)
│       └── killswitch.rs # nftables kill switch
├── packaging/
│   ├── aur/             # Arch User Repository files
│   │   ├── PKGBUILD
│   │   ├── .SRCINFO
│   │   └── tonneru.install
│   ├── hyprland/        # Hyprland window rules
│   ├── polkit/          # PolicyKit policy
│   ├── sudoers/         # Sudoers configuration
│   └── waybar/          # Waybar integration
├── Cargo.toml           # Rust dependencies
├── Cargo.lock           # Locked dependency versions
├── LICENSE              # WTFPL license
└── README.md            # User documentation
```

## 🛠 Development Setup

### Prerequisites

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install runtime dependencies
sudo pacman -S wireguard-tools nftables

# Install ONE of these for network detection:
sudo pacman -S iwd          # Recommended for Omarchy
# OR
sudo pacman -S networkmanager
```

### Building

```bash
# Clone the repository
git clone https://github.com/WattForce/tonneru.git
cd tonneru

# Build debug version (faster compile)
cargo build

# Build release version (optimized)
cargo build --release

# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run
```

### Running Without Install

```bash
# Debug build
cargo run

# Release build
cargo run --release

# With specific log level
RUST_LOG=tonneru=debug cargo run
```

## 🏗 Architecture

### Application Flow

```
main.rs
  └─> Args parsing (clap)
  └─> CLI mode (--status, --connect, --disconnect, --daemon)
  └─> TUI mode
        └─> App::new() - Initialize state
        └─> run_app() - Main event loop
              └─> terminal.draw() - Render UI
              └─> event::poll() - Handle input
              └─> app.tick() - Periodic updates (countdown timer)
```

### Key Components

#### `App` (app.rs)
Central application state including:
- Current section (Networks/Tunnels/Config)
- Network list and rules
- Tunnel list and VPN status
- Pending changes with countdown timer
- UI state (popups, scroll positions)

#### `Theme` (theme.rs)
Reads colors from Omarchy theme files:
- Loads from `~/.config/omarchy/current/theme/kitty.conf`
- Falls back to Catppuccin-inspired defaults
- Maps ANSI colors to UI elements

#### `NetworkInfo` (network/mod.rs)
Detects networks via:
1. `iwctl` (iwd) - primary for Omarchy
2. `nmcli` (NetworkManager) - fallback
3. `ip link` - basic fallback

#### `WireGuard` (vpn/wireguard.rs)
Manages VPN connections:
- `list_profiles()` - Scan /etc/wireguard/*.conf
- `get_status()` - Parse `wg show` output
- `connect()` / `disconnect()` - Call wg-quick

### Pending Change System

When users change rules on active networks, changes are delayed:
1. User makes change → `schedule_change()` called
2. 3-second countdown starts
3. Each subsequent change resets countdown
4. At 0, `apply_pending_change()` executes the action
5. User can press `Esc` to cancel

## 📝 Code Style

### Rust Conventions

- Use `rustfmt` for formatting: `cargo fmt`
- Use `clippy` for linting: `cargo clippy`
- Follow Rust API Guidelines: https://rust-lang.github.io/api-guidelines/

### Naming

- Modules: `snake_case`
- Types: `PascalCase`
- Functions: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`

### Error Handling

- Use `anyhow::Result` for functions that can fail
- Provide context with `.context("message")`
- Log errors with `tracing::error!`

### Comments

- Document public APIs with `///` doc comments
- Use `//` for implementation notes
- Mark TODOs with `// TODO:`

## 🐛 Debugging

### Enable Logging

```bash
# All logs
RUST_LOG=debug cargo run

# Specific module
RUST_LOG=tonneru::vpn=debug cargo run

# Multiple modules
RUST_LOG=tonneru::vpn=debug,tonneru::network=info cargo run
```

### Common Issues

#### "Permission denied" errors
```bash
# Check sudoers file is installed
cat /etc/sudoers.d/tonneru

# Verify wheel group membership
groups | grep wheel
```

#### Network not detected
```bash
# Test iwd
iwctl station wlan0 show

# Test NetworkManager
nmcli connection show
```

#### VPN shows "UP ⚠" (routing issue)
The WireGuard interface is up but traffic isn't routing through it. Check:
- `AllowedIPs` in config includes `0.0.0.0/0`
- No conflicting routes: `ip route`

## 🚀 Release Process

### Version Bump

1. Update version in `Cargo.toml`
2. Update version in `packaging/aur/PKGBUILD`
3. Update version in `packaging/aur/.SRCINFO`
4. Commit: `git commit -am "Bump version to X.Y.Z"`
5. Tag: `git tag vX.Y.Z`
6. Push: `git push origin main --tags`

### AUR Update

See `docs/AUR_DEPLOYMENT.md` for detailed AUR publishing instructions.

## 📄 License

This project is licensed under the WTFPL (Do What The Fuck You Want To Public License). See [LICENSE](LICENSE) for details.

---

**Questions?** Open an issue on GitHub: https://github.com/WattForce/tonneru/issues

