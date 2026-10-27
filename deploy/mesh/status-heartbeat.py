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


def metrics():
    try:
        total, idle = cpu_sample()
        time.sleep(0.2)
        after, idle_after = cpu_sample()
        memory = {line.split(':')[0]: int(line.split()[1]) for line in pathlib.Path('/proc/meminfo').read_text().splitlines()}
        rounded = lambda n: max(0, min(100, round(n / 5) * 5))
        return {'cpuPercent': rounded(100 * (1 - (idle_after - idle) / (after - total))) if after > total else None,
                'memoryPercent': rounded(100 * (1 - memory['MemAvailable'] / memory['MemTotal'])),
                'uptimeHours': int(float(pathlib.Path('/proc/uptime').read_text().split()[0]) / 3600)}
    except (OSError, ValueError, KeyError, ZeroDivisionError):
        return {'cpuPercent': None, 'memoryPercent': None, 'uptimeHours': None}


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
