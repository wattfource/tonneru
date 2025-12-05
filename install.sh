#!/bin/bash
# tonneru installation script
# Run: ./install.sh

set -e

echo "󰖂 Installing tonneru..."

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if running as root (shouldn't be)
if [ "$EUID" -eq 0 ]; then
    echo -e "${RED}Error: Don't run this script as root${NC}"
    echo "Run as your normal user - it will ask for sudo when needed"
    exit 1
fi

# Check for required dependencies
echo "Checking dependencies..."

# Check for wireguard-tools
if ! command -v wg &> /dev/null; then
    echo -e "${YELLOW}wireguard-tools not found. Installing...${NC}"
    sudo pacman -S --noconfirm wireguard-tools
fi

# Check for nftables (required for kill switch)
if ! command -v nft &> /dev/null; then
    echo -e "${YELLOW}nftables not found. Installing...${NC}"
    sudo pacman -S --noconfirm nftables
fi

# Build if not already built
if [ ! -f "target/release/tonneru" ]; then
    echo "Building tonneru..."
    cargo build --release
fi

# Create tonneru group if it doesn't exist
if ! getent group tonneru >/dev/null 2>&1; then
    echo "Creating tonneru group..."
    sudo groupadd -r tonneru
fi

# Add current user to tonneru group if not already a member
if ! groups | grep -q '\btonneru\b'; then
    echo "Adding $USER to tonneru group..."
    sudo usermod -aG tonneru "$USER"
    echo -e "${YELLOW}NOTE: You must log out and back in for group membership to take effect${NC}"
fi

# Install binary
echo "Installing binary to /usr/bin/tonneru..."
sudo install -Dm755 target/release/tonneru /usr/bin/tonneru

# Install secure helper script
echo "Installing secure helper script..."
sudo install -Dm755 packaging/usr/lib/tonneru/tonneru-sudo /usr/lib/tonneru/tonneru-sudo

# Install sudoers for passwordless VPN management
echo "Installing sudoers rule..."
sudo install -Dm440 packaging/sudoers/tonneru /etc/sudoers.d/tonneru

# Install systemd user service
echo "Installing systemd user service..."
mkdir -p ~/.config/systemd/user
cp packaging/systemd/tonneru.service ~/.config/systemd/user/

# Reload systemd
systemctl --user daemon-reload

# Ask about enabling the service
echo ""
echo -e "${YELLOW}Would you like to enable the tonneru daemon service?${NC}"
echo "This will auto-manage VPN connections based on your network rules."
read -p "Enable service? [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    systemctl --user enable tonneru.service
    systemctl --user start tonneru.service
    echo -e "${GREEN}✓ Service enabled and started${NC}"
else
    echo "You can enable it later with: systemctl --user enable --now tonneru.service"
fi

# Waybar setup
echo ""
echo -e "${YELLOW}Would you like to set up waybar integration?${NC}"
read -p "Set up waybar? [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    mkdir -p ~/.config/waybar/scripts
    cp packaging/waybar/tonneru-status.sh ~/.config/waybar/scripts/
    chmod +x ~/.config/waybar/scripts/tonneru-status.sh
    
    echo ""
    echo -e "${GREEN}✓ Waybar script installed${NC}"
    echo ""
    echo "Add this to your ~/.config/waybar/config modules:"
    echo ""
    echo '  "modules-right": ["custom/vpn", ...],'
    echo ""
    echo "And add this module definition:"
    echo ""
    cat << 'EOF'
  "custom/vpn": {
      "exec": "tonneru --status 2>/dev/null",
      "return-type": "json",
      "interval": 3,
      "on-click": "kitty --title 'tonneru' tonneru",
      "on-click-right": "tonneru --disconnect",
      "format": "{icon}",
      "format-icons": {
          "connected": "󰒘",
          "disconnected": "󰒙"
      },
      "tooltip": true,
      "exec-if": "which tonneru"
  }
EOF
    echo ""
    echo "Add this to your ~/.config/waybar/style.css:"
    echo ""
    cat << 'EOF'
#custom-vpn {
    padding: 0 10px;
}
#custom-vpn.connected {
    color: #a6e3a1;
}
#custom-vpn.disconnected {
    color: #f38ba8;
}
EOF
    echo ""
fi

# Hyprland setup
if command -v hyprctl &> /dev/null; then
    echo ""
    echo -e "${YELLOW}Hyprland detected. Would you like to add window rules?${NC}"
    read -p "Add Hyprland rules? [y/N] " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        echo ""
        echo "Add these to your ~/.config/hypr/hyprland.conf:"
        echo ""
        cat packaging/hyprland/windowrules.conf
        echo ""
    fi
fi

echo ""
echo -e "${GREEN}═══════════════════════════════════════════${NC}"
echo -e "${GREEN}󰖂 tonneru installed successfully!${NC}"
echo -e "${GREEN}═══════════════════════════════════════════${NC}"
echo ""
echo -e "${YELLOW}IMPORTANT: Log out and back in for group membership to take effect${NC}"
echo ""
echo "Quick start:"
echo "  tonneru              - Launch TUI"
echo "  tonneru --daemon     - Run daemon (or use systemd service)"
echo "  tonneru --status     - Get JSON status for scripts"
echo ""
echo "Service management:"
echo "  systemctl --user status tonneru   - Check service status"
echo "  systemctl --user restart tonneru  - Restart daemon"
echo "  journalctl --user -u tonneru -f   - View logs"
echo ""
echo "Security:"
echo "  - Uses dedicated 'tonneru' group (not wheel)"
echo "  - Single auditable helper script for privileged ops"
echo "  - All operations logged to: journalctl -t tonneru-sudo"
echo ""
