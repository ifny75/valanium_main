import { test } from 'node:test';
import assert from 'node:assert/strict';
import { sanitizeMetrics, sanitizeHeartbeat, publicNode, physicalServers, serviceFromNode } from '../app/network-status.ts';
const now = Date.now();
test('only aggregate numeric telemetry can reach the public DTO', () => {
  const beat = sanitizeHeartbeat({ checkedAt: new Date(now).toISOString(), latency: 24, healthy: true,
    ip: '192.0.2.1', hostname: 'secret-host', users: 42,
    metrics: { cpuPercent: 12.4, memoryPercent: 46, uptimeHours: 18.9, address: '192.0.2.1', sessions: 7 } }, now);
  const node = publicNode('test', 'Test', 'Relay', 'FI', beat, 90_000, now);
  assert.deepEqual(node.metrics, { cpuPercent: 10, memoryPercent: 45, uptimeHours: 18 });
  assert.ok(!JSON.stringify(node).includes('192.0.2.1'));
  assert.ok(!JSON.stringify(node).includes('secret-host'));
  assert.deepEqual(sanitizeMetrics({ cpuPercent: -1, memoryPercent: Infinity, uptimeHours: 'password' }),
    { cpuPercent: null, memoryPercent: null, uptimeHours: null });
});

test('multiple service reports describe exactly three machines with one set of metrics per machine', () => {
  const beat = (offset, load, healthy = true) => ({ checkedAt: new Date(now - offset).toISOString(), latency: 20, healthy,
    metrics: { cpuPercent: load, memoryPercent: 40, uptimeHours: 100 }, ip: '192.0.2.1' });
  const servers = physicalServers({ cpuPercent: 4, memoryPercent: 30, uptimeHours: 70 },
    [beat(1000, 15, false), beat(2000, 5)], [beat(3000, 20), beat(500, 10)], now);
  assert.deepEqual(servers.map(server => [server.id, server.country]), [['main', 'NL'], ['server-alpha', 'NL'], ['server-beta', 'FI']]);
  assert.equal(servers[1].metrics.cpuPercent, 15);
  assert.equal(servers[2].metrics.cpuPercent, 10);
  assert.equal(servers[1].status, 'operational', 'a failed service check does not imply the server is down');
  const service = serviceFromNode(publicNode('alpha', 'Relay Alpha', 'Onion', 'NL', beat(1000, 15, false), 90000, now));
  assert.equal(service.status, 'outage');
  assert.equal('metrics' in service, false);
  assert.equal('country' in service, false);
  assert.ok(!JSON.stringify({ servers, service }).includes('192.0.2.1'));
});

test('server freshness uses any fresh agent on that machine, and never retains stale load', () => {
  const stale = { checkedAt: new Date(now - 90000).toISOString(), latency: 20, healthy: true, metrics: { cpuPercent: 90 } };
  const fresh = { ...stale, checkedAt: new Date(now - 1000).toISOString() };
  const servers = physicalServers({}, [stale, fresh], [stale, null], now);
  assert.equal(servers[1].status, 'operational');
  assert.equal(servers[2].status, 'outage');
  assert.equal(servers[2].metrics.cpuPercent, null);
  assert.equal(servers[2].checkedAt, null);
});
test('stale or future telemetry cannot look operational or retain old load', () => {
  const beat = sanitizeHeartbeat({ checkedAt: new Date(now).toISOString(), latency: 10, metrics: { cpuPercent: 25 } }, now);
  assert.equal(sanitizeHeartbeat({ ...beat, checkedAt: new Date(now + 1).toISOString() }, now), null);
  const stale = publicNode('test', 'Test', 'Relay', 'NL', beat, 45_000, now + 45_000);
  assert.equal(stale.status, 'outage');
  assert.equal(stale.checkedAt, null);
  assert.equal(stale.metrics.cpuPercent, null);
});
