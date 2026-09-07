#!/usr/bin/env python3
"""Refreshes the `tor_nodes` ipset from Tor's own relay directory (onionoo)
so the FORWARD-chain DROP rule in setup_tor_torrent_block.sh always matches
the current relay set — the list rotates constantly, so this is meant to
run on a daily timer, not just once at setup.
"""
import ipaddress
import json
import subprocess
import urllib.request

ONIONOO_URL = "https://onionoo.torproject.org/summary?type=relay&running=true"
SET_NAME = "tor_nodes"


def fetch_relay_ips() -> set[str]:
    req = urllib.request.Request(ONIONOO_URL, headers={"User-Agent": "valanium-vpn-node/1.0"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.load(resp)
    ips = set()
    for relay in data.get("relays", []):
        for addr in relay.get("a", []):
            # onionoo mixes in IPv6 addresses (and occasional bracketed/port
            # forms) — the ipset below is IPv4-only (hash:ip, family inet),
            # matching how the rest of this node's networking is IPv4-only.
            try:
                ip = ipaddress.ip_address(addr.split(":")[0] if addr.count(":") <= 1 else addr)
                if ip.version == 4:
                    ips.add(str(ip))
            except ValueError:
                continue
    return ips


def main():
    ips = fetch_relay_ips()
    if not ips:
        print("no relay IPs fetched — leaving existing ipset untouched")
        return

    tmp_set = f"{SET_NAME}_new"
    subprocess.run(["ipset", "create", tmp_set, "hash:ip", "family", "inet", "-exist"], check=True)
    subprocess.run(["ipset", "flush", tmp_set], check=True)
    restore_lines = [f"add {tmp_set} {ip}" for ip in ips]
    subprocess.run(
        ["ipset", "restore", "-exist"],
        input="\n".join(restore_lines) + "\n",
        text=True,
        check=True,
    )
    # Atomic swap: the live set is replaced in one step, so the FORWARD rule
    # (which references SET_NAME) never sees a half-populated set.
    subprocess.run(["ipset", "create", SET_NAME, "hash:ip", "family", "inet", "-exist"], check=True)
    subprocess.run(["ipset", "swap", tmp_set, SET_NAME], check=True)
    subprocess.run(["ipset", "destroy", tmp_set], check=True)
    print(f"tor_nodes refreshed: {len(ips)} relay IPs")


if __name__ == "__main__":
    main()
