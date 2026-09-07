#!/bin/bash
# Superseded by tor_torrent_block.rules, which is spliced directly into
# /etc/ufw/before.rules (ufw manages persistence across reboots on its own —
# see the deployment notes in the repo). Kept only as a from-scratch
# reference for setting up a fresh node outside of ufw's rule files.
# Idempotent: safe to re-run after a reboot or when adjusting rules.
set -euo pipefail

WG_IFACE="wg0"

# --- Tor: drop connections to any known Tor relay IP -----------------------
# The ipset starts empty here; block_tor.py (run once below, then daily via
# the timer) is what actually populates it from Tor's own relay directory.
ipset create tor_nodes hash:ip family inet -exist

if ! iptables -C FORWARD -i "$WG_IFACE" -m set --match-set tor_nodes dst -j DROP 2>/dev/null; then
    iptables -I FORWARD 1 -i "$WG_IFACE" -m set --match-set tor_nodes dst -j DROP
fi

# --- BitTorrent: classic default-port block only ----------------------------
# A `-m string` match on the literal "BitTorrent protocol" handshake and a
# per-client `-m connlimit` cap were both tried and dropped: modern clients
# encrypt the handshake and use randomized UDP (uTP) ports, so neither ever
# matched real torrent traffic — they only added CPU cost per forwarded
# packet and, in connlimit's case, throttled legitimate multi-connection
# browsing. This port range is the cheap, low-risk remainder.
if ! iptables -C FORWARD -i "$WG_IFACE" -p tcp --dport 6881:6889 -j DROP 2>/dev/null; then
    iptables -I FORWARD 1 -i "$WG_IFACE" -p tcp --dport 6881:6889 -j DROP
fi
if ! iptables -C FORWARD -i "$WG_IFACE" -p udp --dport 6881:6889 -j DROP 2>/dev/null; then
    iptables -I FORWARD 1 -i "$WG_IFACE" -p udp --dport 6881:6889 -j DROP
fi

echo "iptables rules in place. Populating tor_nodes ipset now..."
python3 /opt/vpn_node/block_tor.py

netfilter-persistent save
echo "Saved with netfilter-persistent — rules and the (initial) ipset survive a reboot."
echo "NOTE: ipset contents aren't captured by netfilter-persistent — the systemd timer re-populates it after boot too."
