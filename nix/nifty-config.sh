#!/usr/bin/env bash
# nifty-config — interactive menu to configure nifty-filter
set -euo pipefail

ENV_FILE="/var/nifty-filter/router.env"
DHCP_FILE="/var/nifty-filter/dhcp.env"

get_val() {
    grep -oP "^${1}=\K.*" "$ENV_FILE" 2>/dev/null || echo ""
}

set_val() {
    local key="$1" val="$2"
    if grep -q "^${key}=" "$ENV_FILE"; then
        sed -i "s|^${key}=.*|${key}=${val}|" "$ENV_FILE"
    else
        echo "${key}=${val}" >> "$ENV_FILE"
    fi
}

get_dhcp_val() {
    grep -oP "^${1}=\K.*" "$DHCP_FILE" 2>/dev/null || echo ""
}

set_dhcp_val() {
    local key="$1" val="$2"
    if grep -q "^${key}=" "$DHCP_FILE"; then
        sed -i "s|^${key}=.*|${key}=${val}|" "$DHCP_FILE"
    else
        echo "${key}=${val}" >> "$DHCP_FILE"
    fi
}

edit_hostname() {
    local current=$(get_val HOSTNAME)
    local val=$(script-wizard ask "Hostname" "$current")
    if [[ "$val" =~ ^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$ ]]; then
        set_val HOSTNAME "$val"
        echo "  Set HOSTNAME=$val"
    else
        echo "  Invalid hostname."
    fi
}

edit_subnet() {
    local current=$(get_val SUBNET_LAN)
    local val=$(script-wizard ask "LAN subnet (IP/prefix)" "$current")
    set_val SUBNET_LAN "$val"
    echo "  Set SUBNET_LAN=$val"
    # Update DHCP defaults to match
    local router_ip=$(echo "$val" | cut -d/ -f1)
    local base=$(echo "$router_ip" | sed 's/\.[0-9]*$//')
    set_dhcp_val DHCP_SUBNET "$val"
    set_dhcp_val DHCP_ROUTER "$router_ip"
    set_dhcp_val DHCP_POOL_START "${base}.100"
    set_dhcp_val DHCP_POOL_END "${base}.250"
    echo "  Updated DHCP pool to match."
}

edit_dhcp_pool() {
    local start=$(get_dhcp_val DHCP_POOL_START)
    local end=$(get_dhcp_val DHCP_POOL_END)
    start=$(script-wizard ask "DHCP pool start" "$start")
    end=$(script-wizard ask "DHCP pool end" "$end")
    set_dhcp_val DHCP_POOL_START "$start"
    set_dhcp_val DHCP_POOL_END "$end"
    echo "  Set pool: $start - $end"
}

edit_dns() {
    local current=$(get_dhcp_val DHCP_DNS)
    local val=$(script-wizard ask "DNS servers (comma-separated)" "$current")
    set_dhcp_val DHCP_DNS "$val"
    echo "  Set DNS=$val"
}

edit_ports() {
    local key="$1" label="$2"
    local current=$(get_val "$key")
    local val=$(script-wizard ask "$label (comma-separated)" "$current" --allow-blank)
    set_val "$key" "$val"
    echo "  Set ${key}=$val"
}

edit_forwards() {
    local key="$1" label="$2"
    local current=$(get_val "$key")
    echo "  Format: incoming_port:dest_ip:dest_port (comma-separated)"
    local val=$(script-wizard ask "$label" "$current" --allow-blank)
    set_val "$key" "$val"
    echo "  Set ${key}=$val"
}

toggle_enabled() {
    local current=$(get_val ENABLED)
    if [ "$current" = "true" ]; then
        set_val ENABLED false
        echo "  Disabled."
    else
        set_val ENABLED true
        echo "  Enabled."
    fi
}

apply_changes() {
    echo "  Restarting nifty-filter..."
    sudo systemctl restart nifty-filter && echo "  Firewall rules applied." || echo "  Failed! Check: journalctl -u nifty-filter"
    echo "  Restarting dnsmasq..."
    sudo systemctl restart nifty-dnsmasq && echo "  DHCP/DNS applied." || echo "  Failed! Check: journalctl -u nifty-dnsmasq"
    echo "  Setting hostname..."
    sudo hostname "$(get_val HOSTNAME)" 2>/dev/null || true
    echo "  Done."
}

show_status() {
    echo ""
    echo "  === Current Configuration ==="
    echo "  ENABLED:        $(get_val ENABLED)"
    echo "  HOSTNAME:       $(get_val HOSTNAME)"
    echo "  INTERFACE_WAN:  $(get_val INTERFACE_WAN)"
    echo "  INTERFACE_LAN:  $(get_val INTERFACE_LAN)"
    echo "  SUBNET_LAN:     $(get_val SUBNET_LAN)"
    echo "  TCP_ACCEPT_LAN: $(get_val TCP_ACCEPT_LAN)"
    echo "  UDP_ACCEPT_LAN: $(get_val UDP_ACCEPT_LAN)"
    echo "  TCP_ACCEPT_WAN: $(get_val TCP_ACCEPT_WAN)"
    echo "  UDP_ACCEPT_WAN: $(get_val UDP_ACCEPT_WAN)"
    echo "  TCP_FORWARD_LAN: $(get_val TCP_FORWARD_LAN)"
    echo "  UDP_FORWARD_LAN: $(get_val UDP_FORWARD_LAN)"
    echo "  TCP_FORWARD_WAN: $(get_val TCP_FORWARD_WAN)"
    echo "  UDP_FORWARD_WAN: $(get_val UDP_FORWARD_WAN)"
    echo "  DHCP_POOL:      $(get_dhcp_val DHCP_POOL_START) - $(get_dhcp_val DHCP_POOL_END)"
    echo "  DHCP_DNS:       $(get_dhcp_val DHCP_DNS)"
    echo ""
}

if [ ! -f "$ENV_FILE" ]; then
    echo "ERROR: $ENV_FILE not found."
    exit 1
fi

while true; do
    ENABLED=$(get_val ENABLED)
    ENABLED_LABEL="Enable firewall"
    [ "$ENABLED" = "true" ] && ENABLED_LABEL="Disable firewall"

    CHOICE=$(script-wizard choose "nifty-filter configuration:" \
        "Show status" \
        "Hostname ($(get_val HOSTNAME))" \
        "LAN subnet ($(get_val SUBNET_LAN))" \
        "DHCP pool ($(get_dhcp_val DHCP_POOL_START)-$(get_dhcp_val DHCP_POOL_END))" \
        "DNS servers ($(get_dhcp_val DHCP_DNS))" \
        "TCP ports LAN ($(get_val TCP_ACCEPT_LAN))" \
        "UDP ports LAN ($(get_val UDP_ACCEPT_LAN))" \
        "TCP ports WAN ($(get_val TCP_ACCEPT_WAN))" \
        "UDP ports WAN ($(get_val UDP_ACCEPT_WAN))" \
        "TCP forward LAN ($(get_val TCP_FORWARD_LAN))" \
        "UDP forward LAN ($(get_val UDP_FORWARD_LAN))" \
        "TCP forward WAN ($(get_val TCP_FORWARD_WAN))" \
        "UDP forward WAN ($(get_val UDP_FORWARD_WAN))" \
        "$ENABLED_LABEL" \
        "Apply changes" \
        "Edit router.env" \
        "Edit dhcp.env" \
        "Quit" \
    ) || break

    case "$CHOICE" in
        "Show status") show_status ;;
        Hostname*) edit_hostname ;;
        "LAN subnet"*) edit_subnet ;;
        "DHCP pool"*) edit_dhcp_pool ;;
        "DNS servers"*) edit_dns ;;
        "TCP ports LAN"*) edit_ports TCP_ACCEPT_LAN "TCP ports LAN" ;;
        "UDP ports LAN"*) edit_ports UDP_ACCEPT_LAN "UDP ports LAN" ;;
        "TCP ports WAN"*) edit_ports TCP_ACCEPT_WAN "TCP ports WAN" ;;
        "UDP ports WAN"*) edit_ports UDP_ACCEPT_WAN "UDP ports WAN" ;;
        "TCP forward LAN"*) edit_forwards TCP_FORWARD_LAN "TCP forward LAN" ;;
        "UDP forward LAN"*) edit_forwards UDP_FORWARD_LAN "UDP forward LAN" ;;
        "TCP forward WAN"*) edit_forwards TCP_FORWARD_WAN "TCP forward WAN" ;;
        "UDP forward WAN"*) edit_forwards UDP_FORWARD_WAN "UDP forward WAN" ;;
        "Enable firewall"|"Disable firewall") toggle_enabled ;;
        "Apply changes") apply_changes ;;
        "Edit router.env") ${EDITOR:-nano} "$ENV_FILE" ;;
        "Edit dhcp.env") ${EDITOR:-nano} "$DHCP_FILE" ;;
        "Quit") break ;;
    esac
done
