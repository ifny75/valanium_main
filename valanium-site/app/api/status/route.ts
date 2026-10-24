import { timingSafeEqual } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';

const TOKEN_FILE = '/etc/valanium-status-token';
/*
  Узлы, которые сами отчитываются о себе.

  Имя приходит от узла из /etc/valanium-status-node-id, и список здесь —
  единственное, что решает, чей отчёт мы примем. Добавить узел значит добавить
  строку сюда и положить ему файл с этим же именем: чужой узел записаться сам
  не сможет, даже зная секрет.

  Узел, который перестал отчитываться, показывается недоступным — и это
  правильно: страница состояния для того и нужна. А вот узла, которого нет в
  строю вовсе, здесь быть не должно: постоянно красная строка приучает не
  смотреть на страницу.
*/
const HEARTBEAT_FILES: Record<string, string> = {
  'relay-alpha': '/tmp/valanium-status-relay-alpha.json',
  'relay-beta': '/tmp/valanium-status-relay-beta.json',
  'relay-gamma': '/tmp/valanium-status-relay-gamma.json',
};

/** Как узлы называются на странице. Порядок здесь — порядок в списке. */
const RELAYS = [
  { id: 'alpha', node: 'relay-alpha', name: 'Relay Alpha' },
  { id: 'beta', node: 'relay-beta', name: 'Relay Beta' },
  { id: 'gamma', node: 'relay-gamma', name: 'Relay Gamma' },
] as const;

type Heartbeat = { checkedAt: string; latency: number };

/*
  Отчёт самопроверки сервера.

  Она смотрит на то, чего работающий сервер о себе не сообщает: свежесть копий,
  уехали ли они наружу, не потеряла ли база население. Все четыре молчаливых
  отказа за время работы были именно такими — служба `active`, сообщения ходят,
  а копий нет уже двое суток.

  Наружу отдаём только «в порядке или нет». Подробности — числа: сколько
  аккаунтов, когда была копия. Публичной странице состояния знать их незачем,
  а любопытному наблюдателю они сказали бы больше, чем следует.
*/
/*
  Не в /tmp: у службы сайта PrivateTmp=yes, то есть свой собственный /tmp, и
  файл, записанный самопроверкой, ей попросту не виден. Строка молча
  показывала бы деградацию всегда — худший вид неверного сигнала, потому что
  на него перестают смотреть.

  /run приватизация не затрагивает, и он же чистится при перезагрузке — что
  здесь правильно: отчёт старше перезагрузки ничего не значит.
*/
const GUARD_FILE = '/run/valanium/guard.json';

type Guard = { ok: boolean; checkedAt: string };

async function readGuard(): Promise<Guard | null> {
  try {
    const value = JSON.parse(await readFile(GUARD_FILE, 'utf8')) as Guard;
    return typeof value.ok === 'boolean' && typeof value.checkedAt === 'string' ? value : null;
  } catch {
    return null;
  }
}

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
  const [core, guard, ...beats] = await Promise.all([
    checkCore(),
    readGuard(),
    ...RELAYS.map((relay) => readHeartbeat(relay.node)),
  ]);
  const now = Date.now();
  const relay = (heartbeat: Heartbeat | null) => {
    const age = heartbeat ? now - Date.parse(heartbeat.checkedAt) : Infinity;
    return { online: age < 90_000, latency: age < 90_000 ? heartbeat?.latency ?? 0 : 0 };
  };
  const states = beats.map((beat) => relay(beat as Heartbeat | null));

  // Один упавший relay — деградация, а не авария: маршрут переносится на
  // соседний и человек этого не замечает. Аварией это становится, когда
  // недоступно ядро либо не осталось ни одного входа.
  const liveRelays = states.filter((state) => state.online).length;

  /*
    Сохранность данных — деградация, но не авария: переписка идёт, люди этого
    не замечают. В том и беда, ради которой строка здесь и появилась: незаметно
    как раз то, что обнаружится слишком поздно.

    Молчание самопроверки считаем отказом наравне с её жалобой. Отсутствующий
    отчёт означает, что и сама проверка не работает, — а это ровно тот случай,
    который мы и пытаемся перестать пропускать.
  */
  const guardAge = guard ? now - Date.parse(guard.checkedAt) : Infinity;
  const dataSafe = guard?.ok === true && guardAge < 3 * 3_600_000;

  const operational = core.online && liveRelays === states.length && dataSafe;
  const outage = !core.online || liveRelays === 0;

  return Response.json({
    overall: outage ? 'outage' : operational ? 'operational' : 'degraded',
    checkedAt: new Date(now).toISOString(),
    nodes: [
      { id: 'web', name: 'Public Website', role: 'Сайт и загрузки', status: 'operational', latency: 0 },
      { id: 'core', name: 'Messaging Core', role: 'Доставка сообщений', status: core.online ? 'operational' : 'outage', latency: core.latency },
      {
        id: 'data',
        name: 'Data Safety',
        role: 'Копии и сохранность базы',
        status: dataSafe ? 'operational' : 'degraded',
        latency: 0,
      },
      ...RELAYS.map((entry, index) => ({
        id: entry.id,
        name: entry.name,
        role: 'Relay · Multihop · Tor',
        status: states[index]!.online ? 'operational' : 'outage',
        latency: states[index]!.latency,
      })),
    ],
  }, { headers: { 'Cache-Control': 'public, max-age=15, stale-while-revalidate=30' } });
}
