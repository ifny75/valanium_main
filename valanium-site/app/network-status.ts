export type Country = 'FI' | 'NL';
export type Metrics = { cpuPercent: number | null; memoryPercent: number | null; uptimeHours: number | null };
export type NodeStatus = 'operational' | 'degraded' | 'outage';
export type PublicNode = {
  id: string; name: string; role: string; status: NodeStatus; latency: number;
  country: Country; checkedAt: string | null; metrics: Metrics;
};
export type Heartbeat = { checkedAt: string; latency: number; healthy: boolean; metrics: Metrics };
export const EMPTY_METRICS: Metrics = { cpuPercent: null, memoryPercent: null, uptimeHours: null };
export const COUNTRIES = {
  FI: { name: 'Финляндия', flag: '/flags/finland.svg' },
  NL: { name: 'Нидерланды', flag: '/flags/netherlands.svg' },
} as const;
function numberInRange(value: unknown, max: number): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= max ? value : null;
}
export function sanitizeMetrics(value: unknown): Metrics {
  const input = value && typeof value === 'object' ? value as Record<string, unknown> : {};
  const rounded = (value: unknown) => { const n = numberInRange(value, 100); return n === null ? null : Math.round(n / 5) * 5; };
  const uptime = numberInRange(input.uptimeHours, 1_000_000);
  return { cpuPercent: rounded(input.cpuPercent), memoryPercent: rounded(input.memoryPercent),
    uptimeHours: uptime === null ? null : Math.floor(uptime) };
}
export function sanitizeHeartbeat(value: unknown, now: number): Heartbeat | null {
  if (!value || typeof value !== 'object') return null;
  const input = value as Record<string, unknown>;
  const timestamp = typeof input.checkedAt === 'string' ? Date.parse(input.checkedAt) : NaN;
  if (!Number.isFinite(timestamp) || timestamp > now || now - timestamp >= 90_000) return null;
  const latency = numberInRange(input.latency, 5000);
  if (latency === null) return null;
  return { checkedAt: new Date(timestamp).toISOString(), latency: Math.round(latency),
    healthy: input.healthy !== false, metrics: sanitizeMetrics(input.metrics) };
}
// Explicit projection: never serialize a received heartbeat, hostname or OS info.
export function publicNode(id: string, name: string, role: string, country: Country,
    heartbeat: Heartbeat | null, freshMs = 90_000, now = Date.now()): PublicNode {
  const fresh = heartbeat !== null && now - Date.parse(heartbeat.checkedAt) >= 0 && now - Date.parse(heartbeat.checkedAt) < freshMs;
  return { id, name, role, country, status: fresh && heartbeat.healthy ? 'operational' : 'outage',
    latency: fresh && heartbeat.healthy ? heartbeat.latency : 0, checkedAt: fresh ? heartbeat.checkedAt : null,
    metrics: fresh ? sanitizeMetrics(heartbeat.metrics) : { ...EMPTY_METRICS } };
}
