#!/usr/bin/env python3
"""Public aggregate telemetry only. Never collect addresses or user counts."""
import json
import pathlib
import subprocess
import sys
import time
import urllib.request


def cpu_sample():
    values = [int(n) for n in pathlib.Path('/proc/stat').read_text().splitlines()[0].split()[1:9]]
    return sum(values), values[3] + values[4]


def network_sample(routes=None, devices=None):
    try:
        routes = pathlib.Path('/proc/net/route').read_text() if routes is None else routes
        devices = pathlib.Path('/proc/net/dev').read_text() if devices is None else devices
        interfaces = set()
        for line in routes.splitlines()[1:]:
            fields = line.split()
            if len(fields) >= 8 and fields[1] == '00000000' and fields[7] == '00000000' and int(fields[3], 16) & 1 and fields[0] != 'lo':
                interfaces.add(fields[0])
        sample = {}
        for line in devices.splitlines():
            if ':' not in line:
                continue
            name, counters = line.rsplit(':', 1)
            name = name.strip()
            if name not in interfaces:
                continue
            fields = counters.split()
            rx, tx = int(fields[0]), int(fields[8])
            if len(fields) < 16 or rx < 0 or tx < 0:
                return None
            sample[name] = (rx, tx)
        return sample if interfaces and sample.keys() == interfaces else None
    except (OSError, ValueError, IndexError):
        return None


def network_rates(before, after, seconds):
    missing = {'rxMbps': None, 'txMbps': None}
    if not before or not after or before.keys() != after.keys() or not 0 < seconds <= 10:
        return missing
    rx = tx = 0
    for name in before:
        received, sent = after[name][0] - before[name][0], after[name][1] - before[name][1]
        if received < 0 or sent < 0:
            return missing
        rx += received
        tx += sent
    rate = lambda count: round(count * 8 / seconds / 1_000_000, 2) if count * 8 / seconds / 1_000_000 <= 1_000_000 else None
    return {'rxMbps': rate(rx), 'txMbps': rate(tx)}


def metrics():
    try:
        total, idle = cpu_sample()
        network_before = network_sample()
        started = time.monotonic()
        time.sleep(1)
        network_after = network_sample()
        rates = network_rates(network_before, network_after, time.monotonic() - started)
        after, idle_after = cpu_sample()
        memory = {line.split(':')[0]: int(line.split()[1]) for line in pathlib.Path('/proc/meminfo').read_text().splitlines()}
        rounded = lambda n: max(0, min(100, round(n / 5) * 5))
        return {'cpuPercent': rounded(100 * (1 - (idle_after - idle) / (after - total))) if after > total else None,
                'memoryPercent': rounded(100 * (1 - memory['MemAvailable'] / memory['MemTotal'])),
                'uptimeHours': int(float(pathlib.Path('/proc/uptime').read_text().split()[0]) / 3600), **rates}
    except (OSError, ValueError, KeyError, ZeroDivisionError):
        return {'cpuPercent': None, 'memoryPercent': None, 'uptimeHours': None, 'rxMbps': None, 'txMbps': None}


def main():
    website = 'website' in pathlib.Path(sys.argv[0]).name
    node_file = '/etc/valanium-website-node-id' if website else '/etc/valanium-status-node-id'
    token_file = '/etc/valanium-website-status-token' if website else '/etc/valanium-status-token'
    node = pathlib.Path(node_file).read_text().strip()
    if node not in ('website-one', 'website-two', 'relay-alpha', 'relay-beta'):
        raise ValueError('Unknown node')
    services = ['nginx.service', 'cloudflared.service' if website else 'tor@default.service']
    healthy = subprocess.run(['systemctl', 'is-active', '--quiet', *services], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0
    started = time.monotonic()
    try:
        target = 'http://127.0.0.1:8080/' if website else 'http://10.77.0.1:8080/v1/health'
        with urllib.request.urlopen(urllib.request.Request(target, method='HEAD' if website else 'GET'), timeout=5) as response:
            healthy = healthy and response.status == 200
        latency = max(1, min(5000, round((time.monotonic() - started) * 1000)))
    except OSError:
        healthy, latency = False, 0
    payload = {'node': node, 'latency': latency, 'healthy': healthy, 'metrics': metrics()}
    token = pathlib.Path(token_file).read_text().strip()
    request = urllib.request.Request('http://10.77.0.1:8080/api/status', method='POST',
        data=json.dumps(payload).encode(), headers={'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'})
    with urllib.request.urlopen(request, timeout=8) as response:
        if response.status != 200:
            raise OSError('Report rejected')


if __name__ == '__main__':
    try:
        main()
    except Exception:
        print('Node telemetry report failed', file=sys.stderr)
        raise SystemExit(1)
