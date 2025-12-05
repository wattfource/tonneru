#!/bin/bash
# Waybar status script for tonneru
# Install to ~/.config/waybar/scripts/tonneru-status.sh
# Make executable: chmod +x ~/.config/waybar/scripts/tonneru-status.sh

# Check if tonneru exists
if ! command -v tonneru &> /dev/null; then
    echo '{"text": "󰒍", "tooltip": "tonneru not installed", "class": "error"}'
    exit 0
fi

# Get status from tonneru
STATUS=$(tonneru --status 2>/dev/null)

if [ $? -ne 0 ] || [ -z "$STATUS" ]; then
    echo '{"text": "󰒍", "tooltip": "VPN status unavailable", "class": "error"}'
    exit 0
fi

# Parse the JSON to get connected status
CONNECTED=$(echo "$STATUS" | jq -r '.connected // false')
INTERFACE=$(echo "$STATUS" | jq -r '.interface // ""')
TOOLTIP=$(echo "$STATUS" | jq -r '.tooltip // "VPN"')

if [ "$CONNECTED" = "true" ]; then
    # VPN is connected - show tunnel name
    TEXT="󰒘"
    if [ -n "$INTERFACE" ] && [ "$INTERFACE" != "null" ]; then
        TEXT="󰒘 $INTERFACE"
    fi
    echo "{\"text\": \"$TEXT\", \"tooltip\": \"$TOOLTIP\", \"class\": \"connected\", \"alt\": \"connected\"}"
else
    # VPN is disconnected
    echo '{"text": "󰒙", "tooltip": "VPN disconnected\nClick to manage", "class": "disconnected", "alt": "disconnected"}'
fi
