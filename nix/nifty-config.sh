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

# --- Editor functions ---

edit_hostname() {
    local current=$(get_val HOSTNAME)
    local val
    val=$(script-wizard ask "Hostname" "$current") || return
    if [[ "$val" =~ ^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$ ]]; then
        set_val HOSTNAME "$val"
        echo "  Set HOSTNAME=$val"
    else
        echo "  Invalid hostname."
    fi
}

edit_subnet() {
    local current=$(get_val SUBNET_LAN)
    local val
    val=$(script-wizard ask "LAN subnet (IP/prefix)" "$current") || return
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

edit_subnet_ipv6() {
    local current=$(get_val SUBNET_LAN_IPV6)
    local val
    val=$(script-wizard ask "LAN IPv6 subnet (IP/prefix)" "$current") || return
    set_val SUBNET_LAN_IPV6 "$val"
    echo "  Set SUBNET_LAN_IPV6=$val"
}

toggle_ipv6() {
    local current=$(get_val ENABLE_IPV6)
    if [ "$current" = "true" ]; then
        set_val ENABLE_IPV6 false
        echo "  IPv6 disabled."
    else
        set_val ENABLE_IPV6 true
        local subnet=$(get_val SUBNET_LAN_IPV6)
        if [ -z "$subnet" ]; then
            echo "  IPv6 requires a LAN subnet."
            edit_subnet_ipv6
        fi
        echo "  IPv6 enabled."
    fi
}

edit_egress_ipv4() {
    local current=$(get_val LAN_EGRESS_ALLOWED_IPV4)
    [ -z "$current" ] && current="0.0.0.0/0"
    local val
    val=$(script-wizard ask "Allowed IPv4 egress CIDRs (comma-separated)" "$current") || return
    set_val LAN_EGRESS_ALLOWED_IPV4 "$val"
    echo "  Set LAN_EGRESS_ALLOWED_IPV4=$val"
}

edit_egress_ipv6() {
    local current=$(get_val LAN_EGRESS_ALLOWED_IPV6)
    [ -z "$current" ] && current="::/0"
    local val
    val=$(script-wizard ask "Allowed IPv6 egress CIDRs (comma-separated)" "$current") || return
    set_val LAN_EGRESS_ALLOWED_IPV6 "$val"
    echo "  Set LAN_EGRESS_ALLOWED_IPV6=$val"
}

edit_dhcp_pool() {
    local start=$(get_dhcp_val DHCP_POOL_START)
    local end=$(get_dhcp_val DHCP_POOL_END)
    start=$(script-wizard ask "DHCP pool start" "$start") || return
    end=$(script-wizard ask "DHCP pool end" "$end") || return
    set_dhcp_val DHCP_POOL_START "$start"
    set_dhcp_val DHCP_POOL_END "$end"
    echo "  Set pool: $start - $end"
}

edit_dns() {
    local current=$(get_dhcp_val DHCP_DNS)
    local val
    val=$(script-wizard ask "DNS servers (comma-separated)" "$current") || return
    set_dhcp_val DHCP_DNS "$val"
    echo "  Set DNS=$val"
}

toggle_dhcpv6() {
    local current=$(get_dhcp_val DHCPV6_ENABLED)
    if [ "$current" = "true" ]; then
        set_dhcp_val DHCPV6_ENABLED false
        # Remove DHCPv6 ports from UDP_ACCEPT_LAN
        local udp_lan=$(get_val UDP_ACCEPT_LAN)
        udp_lan=$(echo "$udp_lan" | sed 's/,546,547//;s/^546,547,//;s/^546,547$//')
        set_val UDP_ACCEPT_LAN "$udp_lan"
        echo "  DHCPv6 disabled. Removed ports 546,547 from UDP_ACCEPT_LAN."
    else
        set_dhcp_val DHCPV6_ENABLED true
        # Add DHCPv6 ports to UDP_ACCEPT_LAN if not already present
        local udp_lan=$(get_val UDP_ACCEPT_LAN)
        if ! echo "$udp_lan" | grep -q '546'; then
            if [ -n "$udp_lan" ]; then
                set_val UDP_ACCEPT_LAN "${udp_lan},546,547"
            else
                set_val UDP_ACCEPT_LAN "546,547"
            fi
            echo "  Added ports 546,547 to UDP_ACCEPT_LAN."
        fi
        local pool_start=$(get_dhcp_val DHCPV6_POOL_START)
        if [ -z "$pool_start" ]; then
            echo "  DHCPv6 requires a pool range."
            edit_dhcpv6_pool
        fi
        echo "  DHCPv6 enabled."
    fi
}

edit_dhcpv6_pool() {
    local start=$(get_dhcp_val DHCPV6_POOL_START)
    local end=$(get_dhcp_val DHCPV6_POOL_END)
    # Derive defaults from the IPv6 subnet if pool is not yet set
    if [ -z "$start" ]; then
        local subnet=$(get_val SUBNET_LAN_IPV6)
        local prefix=$(echo "$subnet" | cut -d/ -f1 | sed 's/:[^:]*$//')
        start="${prefix}:100"
        end="${prefix}:1ff"
    fi
    start=$(script-wizard ask "DHCPv6 pool start" "$start") || return
    end=$(script-wizard ask "DHCPv6 pool end" "$end") || return
    set_dhcp_val DHCPV6_POOL_START "$start"
    set_dhcp_val DHCPV6_POOL_END "$end"
    echo "  Set DHCPv6 pool: $start - $end"
}

edit_ports() {
    local key="$1" label="$2"
    local current=$(get_val "$key")
    local val
    val=$(script-wizard ask "$label (comma-separated)" "$current" --allow-blank) || return
    set_val "$key" "$val"
    echo "  Set ${key}=$val"
}

edit_forwards() {
    local key="$1" label="$2"
    local current=$(get_val "$key")
    echo "  Format: incoming_port:dest_ip:dest_port (comma-separated)"
    echo "  IPv6:   incoming_port:[ipv6_addr]:dest_port"
    local val
    val=$(script-wizard ask "$label" "$current" --allow-blank) || return
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
    echo "  Restarting nifty-network..."
    sudo systemctl restart nifty-network && echo "  Network applied." || echo "  Failed! Check: journalctl -u nifty-network"
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
    echo "  ENABLE_IPV4:    $(get_val ENABLE_IPV4)"
    echo "  ENABLE_IPV6:    $(get_val ENABLE_IPV6)"
    echo "  SUBNET_LAN:     $(get_val SUBNET_LAN)"
    local ipv6_subnet=$(get_val SUBNET_LAN_IPV6)
    [ -n "$ipv6_subnet" ] && echo "  SUBNET_LAN_IPV6: $ipv6_subnet"
    echo "  TCP_ACCEPT_LAN: $(get_val TCP_ACCEPT_LAN)"
    echo "  UDP_ACCEPT_LAN: $(get_val UDP_ACCEPT_LAN)"
    echo "  TCP_ACCEPT_WAN: $(get_val TCP_ACCEPT_WAN)"
    echo "  UDP_ACCEPT_WAN: $(get_val UDP_ACCEPT_WAN)"
    local egress4=$(get_val LAN_EGRESS_ALLOWED_IPV4)
    [ -n "$egress4" ] && echo "  EGRESS_IPV4:    $egress4"
    local egress6=$(get_val LAN_EGRESS_ALLOWED_IPV6)
    [ -n "$egress6" ] && echo "  EGRESS_IPV6:    $egress6"
    echo "  TCP_FORWARD_LAN: $(get_val TCP_FORWARD_LAN)"
    echo "  UDP_FORWARD_LAN: $(get_val UDP_FORWARD_LAN)"
    echo "  TCP_FORWARD_WAN: $(get_val TCP_FORWARD_WAN)"
    echo "  UDP_FORWARD_WAN: $(get_val UDP_FORWARD_WAN)"
    echo "  DHCP_POOL:      $(get_dhcp_val DHCP_POOL_START) - $(get_dhcp_val DHCP_POOL_END)"
    echo "  DHCP_DNS:       $(get_dhcp_val DHCP_DNS)"
    local dhcpv6=$(get_dhcp_val DHCPV6_ENABLED)
    if [ "$dhcpv6" = "true" ]; then
        echo "  DHCPV6:         $(get_dhcp_val DHCPV6_POOL_START) - $(get_dhcp_val DHCPV6_POOL_END)"
    fi
    echo ""
}

# --- Submenus ---

menu_network() {
    while true; do
        local IPV6_ENABLED=$(get_val ENABLE_IPV6)
        local IPV6_LABEL="Enable IPv6"
        [ "$IPV6_ENABLED" = "true" ] && IPV6_LABEL="Disable IPv6"

        local items=(
            "Hostname ($(get_val HOSTNAME))"
            "LAN IPv4 subnet ($(get_val SUBNET_LAN))"
        )
        if [ "$IPV6_ENABLED" = "true" ]; then
            items+=("LAN IPv6 subnet ($(get_val SUBNET_LAN_IPV6))")
        fi
        items+=(
            "$IPV6_LABEL"
            "Back"
        )

        local choice
        choice=$(script-wizard choose "Network:" "${items[@]}") || break
        case "$choice" in
            Hostname*) edit_hostname ;;
            "LAN IPv4 subnet"*) edit_subnet ;;
            "LAN IPv6 subnet"*) edit_subnet_ipv6 ;;
            "Enable IPv6"|"Disable IPv6") toggle_ipv6 ;;
            "Back") break ;;
        esac
    done
}

menu_firewall() {
    while true; do
        local IPV6_ENABLED=$(get_val ENABLE_IPV6)

        local items=(
            "TCP ports LAN ($(get_val TCP_ACCEPT_LAN))"
            "UDP ports LAN ($(get_val UDP_ACCEPT_LAN))"
            "TCP ports WAN ($(get_val TCP_ACCEPT_WAN))"
            "UDP ports WAN ($(get_val UDP_ACCEPT_WAN))"
            "Egress filter IPv4 ($(get_val LAN_EGRESS_ALLOWED_IPV4))"
        )
        if [ "$IPV6_ENABLED" = "true" ]; then
            items+=("Egress filter IPv6 ($(get_val LAN_EGRESS_ALLOWED_IPV6))")
        fi
        items+=("Back")

        local choice
        choice=$(script-wizard choose "Firewall:" "${items[@]}") || break
        case "$choice" in
            "TCP ports LAN"*) edit_ports TCP_ACCEPT_LAN "TCP ports LAN" ;;
            "UDP ports LAN"*) edit_ports UDP_ACCEPT_LAN "UDP ports LAN" ;;
            "TCP ports WAN"*) edit_ports TCP_ACCEPT_WAN "TCP ports WAN" ;;
            "UDP ports WAN"*) edit_ports UDP_ACCEPT_WAN "UDP ports WAN" ;;
            "Egress filter IPv4"*) edit_egress_ipv4 ;;
            "Egress filter IPv6"*) edit_egress_ipv6 ;;
            "Back") break ;;
        esac
    done
}

menu_port_forwarding() {
    while true; do
        local choice
        choice=$(script-wizard choose "Port Forwarding:" \
            "TCP forward LAN ($(get_val TCP_FORWARD_LAN))" \
            "UDP forward LAN ($(get_val UDP_FORWARD_LAN))" \
            "TCP forward WAN ($(get_val TCP_FORWARD_WAN))" \
            "UDP forward WAN ($(get_val UDP_FORWARD_WAN))" \
            "Back" \
        ) || break
        case "$choice" in
            "TCP forward LAN"*) edit_forwards TCP_FORWARD_LAN "TCP forward LAN" ;;
            "UDP forward LAN"*) edit_forwards UDP_FORWARD_LAN "UDP forward LAN" ;;
            "TCP forward WAN"*) edit_forwards TCP_FORWARD_WAN "TCP forward WAN" ;;
            "UDP forward WAN"*) edit_forwards UDP_FORWARD_WAN "UDP forward WAN" ;;
            "Back") break ;;
        esac
    done
}

menu_dhcp_dns() {
    while true; do
        local IPV6_ENABLED=$(get_val ENABLE_IPV6)
        local DHCPV6_ENABLED=$(get_dhcp_val DHCPV6_ENABLED)

        local items=(
            "DHCP pool ($(get_dhcp_val DHCP_POOL_START) - $(get_dhcp_val DHCP_POOL_END))"
            "DNS servers ($(get_dhcp_val DHCP_DNS))"
        )
        if [ "$IPV6_ENABLED" = "true" ]; then
            local DHCPV6_LABEL="Enable DHCPv6"
            [ "$DHCPV6_ENABLED" = "true" ] && DHCPV6_LABEL="Disable DHCPv6"
            items+=("$DHCPV6_LABEL")
            if [ "$DHCPV6_ENABLED" = "true" ]; then
                items+=("DHCPv6 pool ($(get_dhcp_val DHCPV6_POOL_START) - $(get_dhcp_val DHCPV6_POOL_END))")
            fi
        fi
        items+=("Back")

        local choice
        choice=$(script-wizard choose "DHCP / DNS:" "${items[@]}") || break
        case "$choice" in
            "DHCP pool"*) edit_dhcp_pool ;;
            "DNS servers"*) edit_dns ;;
            "Enable DHCPv6"|"Disable DHCPv6") toggle_dhcpv6 ;;
            "DHCPv6 pool"*) edit_dhcpv6_pool ;;
            "Back") break ;;
        esac
    done
}

# --- Main menu ---

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
        "Network" \
        "Firewall" \
        "Port forwarding" \
        "DHCP / DNS" \
        "$ENABLED_LABEL" \
        "Apply changes" \
        "Edit router.env" \
        "Edit dhcp.env" \
        "Quit" \
    ) || break

    case "$CHOICE" in
        "Show status") show_status ;;
        "Network") menu_network ;;
        "Firewall") menu_firewall ;;
        "Port forwarding") menu_port_forwarding ;;
        "DHCP / DNS") menu_dhcp_dns ;;
        "Enable firewall"|"Disable firewall") toggle_enabled ;;
        "Apply changes") apply_changes ;;
        "Edit router.env") ${EDITOR:-nano} "$ENV_FILE" ;;
        "Edit dhcp.env") ${EDITOR:-nano} "$DHCP_FILE" ;;
        "Quit") break ;;
    esac
done
