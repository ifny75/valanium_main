import { timingSafeEqual } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { physicalServers, publicNode, sanitizeHeartbeat, sanitizeMetrics, type Heartbeat, type PublicService } from '../../network-status';
import { systemMetrics } from './system-metrics';

const HEARTBEAT_FILES = {
  'website-one': '/tmp/valanium-status-website-one.json',
  'website-two': '/tmp/valanium-status-website-two.json',
  'relay-alpha': '/tmp/valanium-status-relay-alpha.json',
  'relay-beta': '/tmp/valanium-status-relay-beta.json',
} as const;
async function heartbeat(node: keyof typeof HEARTBEAT_FILES): Promise<Heartbeat | null> {
  try { return sanitizeHeartbeat(JSON.parse(await readFile(HEARTBEAT_FILES[node], 'utf8')), Date.now()); }
  catch { return null; }
}
async function coreHealth() {
  const started = performance.now();
  try {
    const response = await fetch('http://127.0.0.1:8787/v1/health', { cache: 'no-store', signal: AbortSignal.timeout(2500) });
    const body = await response.json() as { ok?: boolean };
    return { online: response.ok && body?.ok === true, latency: Math.max(1, Math.round(performance.now() - started)) };
  } catch { return { online: false, latency: 0 }; }
}
export async function POST(request: Request) {
  const token = request.headers.get('authorization')?.replace(/^Bearer /, '') ?? '';
  let expected: string;
  try { expected = (await readFile('/etc/valanium-status-token', 'utf8')).trim(); }
  catch { return new Response(null, { status: 503 }); }
  const a = Buffer.from(token), b = Buffer.from(expected);
  if (!a.length || a.length !== b.length || !timingSafeEqual(a, b)) return new Response(null, { status: 401 });
  if (Number(request.headers.get('content-length')) > 4096) return new Response(null, { status: 413 });
  const raw = await request.text();
  if (raw.length > 4096) return new Response(null, { status: 413 });
  let input;
  try { input = JSON.parse(raw); } catch { return new Response(null, { status: 400 }); }
  if (!input || typeof input.node !== 'string' || !Object.hasOwn(HEARTBEAT_FILES, input.node)
      || typeof input.latency !== 'number' || !Number.isFinite(input.latency)) return new Response(null, { status: 400 });
  const safe = { checkedAt: new Date().toISOString(), latency: Math.max(0, Math.min(5000, Math.round(input.latency))),
    healthy: input.healthy !== false, metrics: sanitizeMetrics(input.metrics) };
  await writeFile(HEARTBEAT_FILES[input.node as keyof typeof HEARTBEAT_FILES], JSON.stringify(safe), { mode: 0o600 });
  return Response.json({ ok: true });
}
export async function GET() {
  const [core, metrics, one, two, alpha, beta, guard] = await Promise.all([
    coreHealth(), systemMetrics(), heartbeat('website-one'), heartbeat('website-two'),
    heartbeat('relay-alpha'), heartbeat('relay-beta'),
    readFile('/run/valanium/guard.json', 'utf8').then(raw => JSON.parse(raw)).catch(() => null),
  ]);
  const now = Date.now(), checkedAt = new Date(now).toISOString();
  const guardAge = now - Date.parse(guard?.checkedAt);
  const safe = guard?.ok === true && guardAge >= 0 && guardAge < 3 * 3_600_000;
  const servers = physicalServers(metrics, [one, alpha], [two, beta], now);
  // Route probes still affect overall health, but are not separate public services.
  const routes = [
    publicNode('website-1', '', '', 'NL', one, 45_000, now),
    publicNode('website-2', '', '', 'FI', two, 45_000, now),
    publicNode('alpha', '', '', 'NL', alpha, 90_000, now),
    publicNode('beta', '', '', 'FI', beta, 90_000, now),
  ];
  const services: PublicService[] = [
    { id: 'core', name: 'Messaging Core', role: 'Доставка сообщений',
      status: core.online ? 'operational' : 'outage', latency: core.latency, checkedAt },
    { id: 'data', name: 'Data Safety', role: 'Проверка резервных копий и сохранности базы',
      status: safe ? 'operational' : 'degraded', latency: 0,
      checkedAt: Number.isFinite(guardAge) && guardAge >= 0 ? new Date(Date.parse(guard.checkedAt)).toISOString() : null },
  ];
  const online = (id: string) => routes.find(route => route.id === id)?.status === 'operational';
  const outage = !core.online || (!online('alpha') && !online('beta')) || (!online('website-1') && !online('website-2'));
  return Response.json({ overall: outage ? 'outage' : [...services, ...routes].every(service => service.status === 'operational') ? 'operational' : 'degraded',
    checkedAt, servers, services }, { headers: { 'Cache-Control': 'no-store' } });
}
