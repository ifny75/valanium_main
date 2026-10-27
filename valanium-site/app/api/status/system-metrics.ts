import { readFile } from 'node:fs/promises';
import { EMPTY_METRICS, sanitizeMetrics, type Metrics } from '../../network-status';
import { networkSnapshot, networkRates } from '../../network-traffic';
let cached: { until: number; value: Promise<Metrics> } | null = null;
export function systemMetrics(): Promise<Metrics> {
  if (cached && cached.until > Date.now()) return cached.value;
  const value = collect().catch(() => ({ ...EMPTY_METRICS }));
  cached = { until: Date.now() + 15_000, value };
  return value;
}
async function cpu() {
  const line = (await readFile('/proc/stat', 'utf8')).split('\n')[0].trim().split(/\s+/).slice(1).map(Number);
  return { total: line.slice(0, 8).reduce((a, b) => a + b, 0), idle: line[3] + line[4] };
}
async function collect(): Promise<Metrics> {
  const [before, networkBefore] = await Promise.all([cpu(), network()]);
  const started = performance.now();
  await new Promise(resolve => setTimeout(resolve, 1000));
  const [after, networkAfter, memory, uptime] = await Promise.all([cpu(), network(), readFile('/proc/meminfo', 'utf8'), readFile('/proc/uptime', 'utf8')]);
  const field = (name: string) => Number(memory.match(new RegExp(`^${name}:\\s+(\\d+)`, 'm'))?.[1]);
  const elapsed = after.total - before.total;
  return sanitizeMetrics({ cpuPercent: elapsed > 0 ? 100 * (1 - (after.idle - before.idle) / elapsed) : null,
    memoryPercent: 100 * (1 - field('MemAvailable') / field('MemTotal')), uptimeHours: Number(uptime.split(' ')[0]) / 3600,
    ...networkRates(networkBefore, networkAfter, (performance.now() - started) / 1000) });
}
async function network() {
  try {
    const [routes, devices] = await Promise.all([readFile('/proc/net/route', 'utf8'), readFile('/proc/net/dev', 'utf8')]);
    return networkSnapshot(routes, devices);
  } catch { return null; }
}
