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

# These are the TCP ports the router will respond to from the LAN:
export TCP_ACCEPT_LAN=22,53,80,443
# These are the TCP ports the router will respond to from the WAN:
export TCP_ACCEPT_WAN=

# These are the UDP ports the router will respond to from the LAN:
export UDP_ACCEPT_LAN=53,67,68
# These are the UDP ports the router will respond to from the WAN:
export UDP_ACCEPT_WAN=


# These are the egress port forwarding rules for the LAN clients:
# INCOMING_PORT:DESTINATION_IP:DESTINATION_PORT,...
export TCP_FORWARD_LAN=8000:93.184.215.14:80,2222:192.168.1.1:22
export UDP_FORWARD_LAN=53:1.1.1.1:53,5353:1.0.0.1:53

# These are the ingress port forwarding rules for the WAN peers:
# INCOMING_PORT:DESTINATION_IP:DESTINATION_PORT,...
export TCP_FORWARD_WAN=
export UDP_FORWARD_WAN=

## Fix the path for demo purposes:
## (Remove this if nifty-filter is already built and on your PATH.)
if ! command -v nifty-filter; then
    cd $(dirname ${BASH_SOURCE})
    just build
    PATH="../target/debug:${PATH}"
fi

## Print the rendered template to stdout:
nifty-filter

## Run with extra validations (requires nft):
#nifty-filter --validate
