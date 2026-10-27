import { test } from 'node:test';
import assert from 'node:assert/strict';
import { sanitizeMetrics, sanitizeHeartbeat, publicNode } from '../app/network-status.ts';
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
test('stale or future telemetry cannot look operational or retain old load', () => {
  const beat = sanitizeHeartbeat({ checkedAt: new Date(now).toISOString(), latency: 10, metrics: { cpuPercent: 25 } }, now);
  assert.equal(sanitizeHeartbeat({ ...beat, checkedAt: new Date(now + 1).toISOString() }, now), null);
  const stale = publicNode('test', 'Test', 'Relay', 'NL', beat, 45_000, now + 45_000);
  assert.equal(stale.status, 'outage');
  assert.equal(stale.checkedAt, null);
  assert.equal(stale.metrics.cpuPercent, null);
});
