#!/bin/bash
# tonneru update script
# Mirrors AUR package update behavior for local development
#
# Usage: ./update.sh         - Full rebuild and install
#        ./update.sh quick   - Install only (skip rebuild if binary exists)

set -e

cd "$(dirname "$0")"

QUICK_MODE="${1:-}"

echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║                    tonneru update                            ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

# Build (unless quick mode and binary exists)
if [[ "$QUICK_MODE" != "quick" ]] || [[ ! -f "target/release/tonneru" ]]; then
    echo "󰖂 Building..."
    cargo build --release
else
    echo "󰖂 Skipping build (quick mode)"
fi

# Create tonneru group if it doesn't exist
if ! getent group tonneru >/dev/null 2>&1; then
    echo "󰖂 Creating tonneru group..."
    sudo groupadd -r tonneru
fi

# Add current user to tonneru group if not already a member
if ! groups | grep -q '\btonneru\b'; then
    echo "󰖂 Adding $USER to tonneru group..."
    sudo usermod -aG tonneru "$USER"
    echo "   NOTE: Log out and back in for group membership to take effect"
fi

# Install binary (same as PKGBUILD)
echo "󰖂 Installing binary to /usr/bin/tonneru..."
if ! sudo install -Dm755 target/release/tonneru /usr/bin/tonneru; then
    echo "ERROR: Failed to install binary. Are you running from a terminal with sudo access?"
    exit 1
fi

# Install secure helper script
echo "󰖂 Installing secure helper script..."
if ! sudo install -Dm755 packaging/usr/lib/tonneru/tonneru-sudo /usr/lib/tonneru/tonneru-sudo; then
    echo "ERROR: Failed to install helper script."
    exit 1
fi

# Install sudoers (same as PKGBUILD)
echo "󰖂 Installing sudoers rules..."
if ! sudo install -Dm440 packaging/sudoers/tonneru /etc/sudoers.d/tonneru; then
    echo "ERROR: Failed to install sudoers file. Run manually:"
    echo "  sudo install -Dm440 $PWD/packaging/sudoers/tonneru /etc/sudoers.d/tonneru"
    exit 1
fi

# Install systemd user service to system location (same as PKGBUILD)
echo "󰖂 Installing systemd user service..."
sudo install -Dm644 packaging/systemd/tonneru.service /usr/lib/systemd/user/tonneru.service

# Remove local override if exists (system location takes precedence)
if [[ -f "$HOME/.config/systemd/user/tonneru.service" ]]; then
    echo "󰖂 Removing local service override (using system-wide)..."
    rm -f "$HOME/.config/systemd/user/tonneru.service"
fi

# Reload systemd to pick up service changes
echo "󰖂 Reloading systemd..."
systemctl --user daemon-reload

# Restart service if running
if systemctl --user is-active --quiet tonneru.service 2>/dev/null; then
    echo "󰖂 Restarting tonneru service..."
    systemctl --user restart tonneru.service
else
    echo "󰖂 Service not running (start with: systemctl --user enable --now tonneru.service)"
fi

# Gracefully reload waybar if running
if pgrep -x waybar > /dev/null; then
    echo "󰖂 Reloading waybar..."
    pkill -SIGUSR2 waybar 2>/dev/null || true
fi

echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║                    󰄬 Update complete                         ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

# Show status
echo "Service status:"
systemctl --user status tonneru.service --no-pager -l 2>/dev/null | head -5 || echo "  (not running)"

echo ""
echo "Status output:"
tonneru --status 2>/dev/null || echo "  (VPN disconnected or error)"
echo ""
