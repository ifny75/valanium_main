export type Country = 'FI' | 'NL';
export type Metrics = { cpuPercent: number | null; memoryPercent: number | null; uptimeHours: number | null;
  rxMbps: number | null; txMbps: number | null };
export type NodeStatus = 'operational' | 'degraded' | 'outage';
export type PublicNode = {
  id: string; name: string; role: string; status: NodeStatus; latency: number;
  country: Country; checkedAt: string | null; metrics: Metrics;
};
export type PublicService = Omit<PublicNode, 'country' | 'metrics'>;
export type StatusData = { overall: NodeStatus; checkedAt: string; servers: PublicNode[]; services: PublicService[] };
export type Heartbeat = { checkedAt: string; latency: number; healthy: boolean; metrics: Metrics };
export const EMPTY_METRICS: Metrics = { cpuPercent: null, memoryPercent: null, uptimeHours: null, rxMbps: null, txMbps: null };
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
  const rate = (value: unknown) => { const n = numberInRange(value, 1_000_000); return n === null ? null : Math.round(n * 100) / 100; };
  return { cpuPercent: rounded(input.cpuPercent), memoryPercent: rounded(input.memoryPercent),
    uptimeHours: uptime === null ? null : Math.floor(uptime), rxMbps: rate(input.rxMbps), txMbps: rate(input.txMbps) };
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

// Several agents run on the same machine. Count the machine once and use its
// newest fresh report, independently of the health of the service it checks.
export function physicalServers(metrics: Metrics, alpha: (Heartbeat | null)[], beta: (Heartbeat | null)[], now = Date.now()): PublicNode[] {
  const server = (id: string, name: string, country: Country, reports: (Heartbeat | null)[]) => {
    const latest = reports.map(report => sanitizeHeartbeat(report, now)).filter((report): report is Heartbeat => report !== null)
      .sort((a, b) => Date.parse(b.checkedAt) - Date.parse(a.checkedAt))[0] ?? null;
    return publicNode(id, name, 'Сетевой сервер', country, latest ? { ...latest, healthy: true, latency: 0 } : null, 90_000, now);
  };
  return [
    { id: 'main', name: 'Главный сервер', role: 'Основной сервер', country: 'NL', status: 'operational',
      latency: 0, checkedAt: new Date(now).toISOString(), metrics: sanitizeMetrics(metrics) },
    server('server-alpha', 'Сервер Alpha', 'NL', alpha),
    server('server-beta', 'Сервер Beta', 'FI', beta),
  ];
}

export function serviceFromNode(node: PublicNode): PublicService {
  return { id: node.id, name: node.name, role: node.role, status: node.status, latency: node.latency, checkedAt: node.checkedAt };
}
