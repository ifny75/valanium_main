import { timingSafeEqual } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';

const TOKEN_FILE = '/etc/valanium-status-token';
const HEARTBEAT_FILES: Record<string, string> = {
  'relay-alpha': '/tmp/valanium-status-relay-alpha.json',
  'relay-beta': '/tmp/valanium-status-relay-beta.json',
};

type Heartbeat = { checkedAt: string; latency: number };

function safeTokenEqual(received: string, expected: string) {
  const left = Buffer.from(received);
  const right = Buffer.from(expected);
  return left.length === right.length && timingSafeEqual(left, right);
}

async function readHeartbeat(node: string): Promise<Heartbeat | null> {
  try {
    const value = JSON.parse(await readFile(HEARTBEAT_FILES[node], 'utf8')) as Heartbeat;
    return typeof value.checkedAt === 'string' && typeof value.latency === 'number' ? value : null;
  } catch {
    return null;
  }
}

async function checkCore() {
  const started = performance.now();
  try {
    const response = await fetch('http://127.0.0.1:8787/', { cache: 'no-store', signal: AbortSignal.timeout(2500) });
    return { online: response.status < 500, latency: Math.max(1, Math.round(performance.now() - started)) };
  } catch {
    return { online: false, latency: 0 };
  }
}

export async function POST(request: Request) {
  const authorization = request.headers.get('authorization') ?? '';
  const receivedToken = authorization.startsWith('Bearer ') ? authorization.slice(7) : '';
  let expectedToken = '';
  try { expectedToken = (await readFile(TOKEN_FILE, 'utf8')).trim(); } catch { return new Response(null, { status: 503 }); }
  if (!receivedToken || !safeTokenEqual(receivedToken, expectedToken)) return new Response(null, { status: 401 });

  const body = await request.json().catch(() => null) as { node?: string; latency?: number } | null;
  if (!body?.node || !(body.node in HEARTBEAT_FILES)) return new Response(null, { status: 400 });
  const latency = Number.isFinite(body.latency) ? Math.max(0, Math.min(5000, Math.round(body.latency ?? 0))) : 0;
  await writeFile(HEARTBEAT_FILES[body.node], JSON.stringify({ checkedAt: new Date().toISOString(), latency }), { mode: 0o600 });
  return Response.json({ ok: true });
}

export async function GET() {
  const [core, alpha, beta] = await Promise.all([checkCore(), readHeartbeat('relay-alpha'), readHeartbeat('relay-beta')]);
  const now = Date.now();
  const relay = (heartbeat: Heartbeat | null) => {
    const age = heartbeat ? now - Date.parse(heartbeat.checkedAt) : Infinity;
    return { online: age < 90_000, latency: age < 90_000 ? heartbeat?.latency ?? 0 : 0 };
  };
  const alphaState = relay(alpha);
  const betaState = relay(beta);
  const operational = core.online && alphaState.online && betaState.online;
  const outage = !core.online || (!alphaState.online && !betaState.online);

  return Response.json({
    overall: outage ? 'outage' : operational ? 'operational' : 'degraded',
    checkedAt: new Date(now).toISOString(),
    nodes: [
      { id: 'web', name: 'Public Website', role: 'Сайт и загрузки', status: 'operational', latency: 0 },
      { id: 'core', name: 'Messaging Core', role: 'Доставка сообщений', status: core.online ? 'operational' : 'outage', latency: core.latency },
      { id: 'alpha', name: 'Relay Alpha', role: 'Relay · Multihop · Tor', status: alphaState.online ? 'operational' : 'outage', latency: alphaState.latency },
      { id: 'beta', name: 'Relay Beta', role: 'Relay · Multihop · Tor', status: betaState.online ? 'operational' : 'outage', latency: betaState.latency },
    ],
  }, { headers: { 'Cache-Control': 'public, max-age=15, stale-while-revalidate=30' } });
}
