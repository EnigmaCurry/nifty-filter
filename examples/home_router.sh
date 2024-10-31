#!/bin/bash
# This is an example nifty-router config for a home router.
# This is one of two configuration styles.
# 1) This example is a shell script that uses env vars only.
# 2) The other example is in home_router.sh
export INTERFACE_LAN=lan
export INTERFACE_WAN=wan
export SUBNET_LAN=192.168.10.1/24
export ICMP_ACCEPT_LAN=echo-request,echo-reply,destination-unreachable,time-exceeded
export ICMP_ACCEPT_WAN=
export TCP_ACCEPT_LAN=22,80,443
export TCP_ACCEPT_WAN=

# These are the port forwarding rules for the LAN clients:
export TCP_FORWARD_LAN=8000:93.184.215.14:80,2222:192.168.1.1:22
export UDP_FORWARD_LAN=53:1.1.1.1:53,5353:1.0.0.1:53

# These are the port forwarding rules for the WAN peers:
export TCP_FORWARD_WAN=1234:192.168.1.1:1234
export UDP_FORWARD_WAN=79:192.168.1.1:7654

# Ensure nifty-router is found on your path, this is a kludge for this
# example that you can remove if you have already configured the PATH:
if ! command -v nifty-filter; then
    SCRIPT_DIR=$(realpath $(dirname ${BASH_SOURCE}))
    BIN_DIR=$(realpath ${SCRIPT_DIR}/../target/debug)
    if [[ -f ${SCRIPT_DIR}/../target/debug/nifty-filter ]]; then
        PATH=${BIN_DIR}:${PATH}
    fi
fi

nifty-filter
