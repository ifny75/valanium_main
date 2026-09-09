/**
 * Sealed sender: сервер не должен уметь связать отправителя с получателем
 * даже в моменте, не только в базе (ARCHITECTURE.md §7a, Claude/CONTRACT.md).
 *
 * Билет запрашивается на одном, аутентифицированном соединении, а тратится
 * на другом — которое `AUTH` никогда не проходило. Это и проверяют тесты
 * ниже: не «функция вернула то, что нужно», а именно то, что соединение,
 * доставившее конверт, ничем не отличимо от анонимного.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { ed25519 } from "@noble/curves/ed25519";
import { sha256 } from "@noble/hashes/sha2";

import { Store } from "../src/db/index.ts";
import { SupportStore } from "../src/support/store.ts";
import { NonceStore } from "../src/auth/nonce.ts";
import { TicketKey, TicketStore, TICKET_LEN } from "../src/auth/tickets.ts";
import { Registry, type Socket } from "../src/ws/registry.ts";
import { RateLimiter } from "../src/util/ratelimit.ts";
import { ConnectionCounter } from "../src/util/connections.ts";
import { authMessage, deviceCertMessage } from "../src/auth/verify.ts";
import { handleMessage, handleOpen, newConnData, type ConnData, type Deps } from "../src/ws/session.ts";
import { ID_LEN, KEY_LEN, OP, frame, jsonFrame } from "../src/proto/frames.ts";
import { ascii, concat, fromHex, random, toHex } from "../src/util/bytes.ts";

class FakeSocket implements Socket {
  readonly sent: Uint8Array[] = [];
  closed: { code: number; reason: string } | null = null;
  send(data: Uint8Array): number {
    this.sent.push(data.slice());
    return 1;
  }
  getBufferedAmount(): number {
    return 0;
  }
  end(code: number, reason = ""): void {
    this.closed = { code, reason };
  }
  close(): void {
    this.closed = { code: 1006, reason: "" };
  }
  take(op: number): Uint8Array {
    const found = this.sent.find((f) => f[0] === op);
    assert.ok(found, `no frame with opcode 0x${op.toString(16)}, got ${this.opcodes()}`);
    return found.subarray(1);
  }
  has(op: number): boolean {
    return this.sent.some((f) => f[0] === op);
  }
  opcodes(): string {
    return this.sent.map((f) => `0x${f[0]!.toString(16)}`).join(",");
  }
  json(op: number): any {
    return JSON.parse(new TextDecoder().decode(this.take(op)));
  }
}

interface Identity {
  idPriv: Uint8Array;
  idPub: Uint8Array;
  devPriv: Uint8Array;
  devPub: Uint8Array;
  cert: Uint8Array;
}

function makeIdentity(): Identity {
  const idPriv = ed25519.utils.randomPrivateKey();
  const idPub = ed25519.getPublicKey(idPriv);
  const devPriv = ed25519.utils.randomPrivateKey();
  const devPub = ed25519.getPublicKey(devPriv);
  return { idPriv, idPub, devPriv, devPub, cert: ed25519.sign(deviceCertMessage(idPub, devPub), idPriv) };
}

function makeDeps(store: Store): Deps {
  return {
    store,
    support: new SupportStore(":memory:"),
    nonces: new NonceStore(30),
    registry: new Registry(),
    authLimiter: new RateLimiter(1000, 60_000),
    recoveryLimiter: new RateLimiter(1000, 3_600_000),
    searchLimiter: new RateLimiter(1000, 3_600_000),
    sendLimiter: new RateLimiter(1000, 60_000),
    postLimiter: new RateLimiter(1000, 60_000),
    claimLimiter: new RateLimiter(1000, 3_600_000),
    ticketLimiter: new RateLimiter(1000, 60_000),
    tickets: new TicketStore(TicketKey.load(":memory:"), 300),
    connections: new ConnectionCounter(),
    now: () => Date.now(),
  };
}

/** Новый сокет, никогда не проходивший AUTH — ровно то, чем и должно быть SEND_ANON. */
function anonConnect(deps: Deps): { sock: FakeSocket; conn: ConnData } {
  const sock = new FakeSocket();
  const conn = newConnData("127.0.0.1");
  handleOpen(deps, sock, conn);
  sock.take(OP.HELLO); // HELLO приходит любому, это ещё не AUTH.
  return { sock, conn };
}

function newInvite(store: Store): string {
  const code = toHex(random(12));
  store.createInvite(sha256(ascii(code)), Date.now(), Date.now() + 3600_000);
  return code;
}

/** Логинится по-настоящему: этим соединением просят билеты. */
function register(deps: Deps, store: Store, id: Identity, handle: string) {
  const sock = new FakeSocket();
  const conn = newConnData("127.0.0.1");
  handleOpen(deps, sock, conn);
  const nonce = fromHex(sock.json(OP.HELLO).nonce, 32);
  handleMessage(deps, sock, conn, jsonFrame(OP.AUTH, {
    v: 1,
    identity: toHex(id.idPub),
    device: toHex(id.devPub),
    deviceCert: toHex(id.cert),
    sig: toHex(ed25519.sign(authMessage(nonce, id.idPub, id.devPub), id.devPriv)),
    invite: newInvite(store),
    handle,
  }));
  assert.ok(sock.has(OP.AUTH_OK), `вход не прошёл: ${sock.opcodes()}`);
  sock.sent.length = 0;
  return { sock, conn };
}

function requestTicket(deps: Deps, from: { sock: FakeSocket; conn: ConnData }): Uint8Array {
  handleMessage(deps, from.sock, from.conn, frame(OP.TICKET_REQUEST, new Uint8Array([1])));
  const grant = from.sock.take(OP.TICKET_GRANT);
  assert.equal(grant[0], 1, "должен приехать ровно один билет");
  return grant.subarray(1, 1 + TICKET_LEN);
}

function sendAnonFrame(ticket: Uint8Array, recipientDevice: Uint8Array, ciphertext: Uint8Array): Uint8Array {
  const ttl = new Uint8Array([0, 0, 0x0e, 0x10]); // 3600 с
  return frame(OP.SEND_ANON, concat(random(ID_LEN), ticket, recipientDevice, ttl, ciphertext));
}

test("HELLO объявляет sealed sender заранее", () => {
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  const sock = new FakeSocket();
  handleOpen(deps, sock, newConnData("127.0.0.1"));
  assert.equal(sock.json(OP.HELLO).features.sealedSender, true);
  store.close();
});

test("билет доставляет конверт по соединению, ни разу не прошедшему AUTH", () => {
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  const alice = makeIdentity();
  const bob = makeIdentity();
  const a = register(deps, store, alice, "alice");
  const b = register(deps, store, bob, "bob");

  // Билет просит своё, уже аутентифицированное соединение.
  const ticket = requestTicket(deps, a);

  const anon = anonConnect(deps);
  assert.equal(anon.conn.identity, null, "анонимное соединение личности не имеет");

  const ciphertext = ascii("sealed-payload");
  handleMessage(deps, anon.sock, anon.conn, sendAnonFrame(ticket, bob.devPub, ciphertext));

  assert.ok(anon.sock.has(OP.SEND_OK), `доставка не подтвердилась: ${anon.sock.opcodes()}`);
  const delivered = b.sock.take(OP.ENVELOPE);
  assert.equal(toHex(delivered.subarray(ID_LEN + 8)), toHex(ciphertext));

  // Соединение так и осталось анонимным — SEND_ANON его не аутентифицирует.
  assert.equal(anon.conn.identity, null);
  assert.equal(anon.conn.devicePub, null);
  store.close();
});

test("билет тратится один раз", () => {
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  const alice = register(deps, store, makeIdentity(), "alice");
  const bob = makeIdentity();
  register(deps, store, bob, "bob");

  const ticket = requestTicket(deps, alice);

  const first = anonConnect(deps);
  handleMessage(deps, first.sock, first.conn, sendAnonFrame(ticket, bob.devPub, ascii("one")));
  assert.ok(first.sock.has(OP.SEND_OK), "первая трата обязана пройти");

  const second = anonConnect(deps);
  handleMessage(deps, second.sock, second.conn, sendAnonFrame(ticket, bob.devPub, ascii("two")));
  assert.equal(second.sock.json(OP.ERROR).code, "bad_ticket", "повторная трата обязана быть отказана");
  store.close();
});

test("просроченный билет не проходит", () => {
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  let clock = Date.now();
  deps.now = () => clock;
  deps.tickets = new TicketStore(TicketKey.load(":memory:"), 1); // TTL 1 с

  const alice = register(deps, store, makeIdentity(), "alice");
  const bob = makeIdentity();
  register(deps, store, bob, "bob");

  const ticket = requestTicket(deps, alice);
  clock += 2_000; // дальше истёкшего срока

  const anon = anonConnect(deps);
  handleMessage(deps, anon.sock, anon.conn, sendAnonFrame(ticket, bob.devPub, ascii("late")));
  assert.equal(anon.sock.json(OP.ERROR).code, "bad_ticket");
  store.close();
});

test("подделанная подпись билета не проходит", () => {
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  const alice = register(deps, store, makeIdentity(), "alice");
  const bob = makeIdentity();
  register(deps, store, bob, "bob");

  const ticket = requestTicket(deps, alice).slice();
  ticket[ticket.length - 1] = (ticket[ticket.length - 1]! ^ 0xff) & 0xff; // портим последний байт подписи

  const anon = anonConnect(deps);
  handleMessage(deps, anon.sock, anon.conn, sendAnonFrame(ticket, bob.devPub, ascii("forged")));
  assert.equal(anon.sock.json(OP.ERROR).code, "bad_ticket");
  store.close();
});

test("билет одной личности не годится для чужого бюджета: отдельный ограничитель", () => {
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  deps.ticketLimiter = new RateLimiter(1, 60_000); // одна пачка в окно

  const alice = register(deps, store, makeIdentity(), "alice");
  handleMessage(deps, alice.sock, alice.conn, frame(OP.TICKET_REQUEST, new Uint8Array([1])));
  assert.ok(alice.sock.has(OP.TICKET_GRANT), "первая пачка обязана пройти");
  alice.sock.sent.length = 0;

  handleMessage(deps, alice.sock, alice.conn, frame(OP.TICKET_REQUEST, new Uint8Array([1])));
  assert.equal(alice.sock.json(OP.ERROR).code, "ticket_rate_limited");
  store.close();
});

test("TICKET_REQUEST требует входа — билет неоткуда взять анонимно", () => {
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  const anon = anonConnect(deps);

  handleMessage(deps, anon.sock, anon.conn, frame(OP.TICKET_REQUEST, new Uint8Array([1])));
  assert.equal(anon.sock.closed?.code, 1008, "неаутентифицированный запрос билета обязан закрыть соединение");
  store.close();
});

test("получатель с политикой «только пропуска» недоступен по билету", () => {
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  const alice = register(deps, store, makeIdentity(), "alice");
  const bobId = makeIdentity();
  const bob = register(deps, store, bobId, "bob");

  handleMessage(deps, bob.sock, bob.conn, jsonFrame(OP.ACCESS_SET, { dmPolicy: "passes" }));
  bob.sock.sent.length = 0;

  const ticket = requestTicket(deps, alice);
  const anon = anonConnect(deps);
  handleMessage(deps, anon.sock, anon.conn, sendAnonFrame(ticket, bobId.devPub, ascii("hi")));

  assert.equal(anon.sock.json(OP.ERROR).code, "dm_not_allowed");
  store.close();
});

test("неизвестному получателю билет не помогает: получатель должен существовать", () => {
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  const alice = register(deps, store, makeIdentity(), "alice");
  const ghost = makeIdentity();

  const ticket = requestTicket(deps, alice);
  const anon = anonConnect(deps);
  handleMessage(deps, anon.sock, anon.conn, sendAnonFrame(ticket, ghost.devPub, ascii("nobody")));

  assert.equal(anon.sock.json(OP.ERROR).code, "unknown_recipient");
  store.close();
});

test("пачка больше разрешённой отклоняется как битый кадр", () => {
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  const alice = register(deps, store, makeIdentity(), "alice");

  handleMessage(deps, alice.sock, alice.conn, frame(OP.TICKET_REQUEST, new Uint8Array([255])));
  assert.equal(alice.sock.closed?.code, 1002, "перебор count обязан закрыть соединение как битый кадр");
  store.close();
});

test("два билета из одной пачки независимы", () => {
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  const alice = register(deps, store, makeIdentity(), "alice");
  const bob = makeIdentity();
  register(deps, store, bob, "bob");

  handleMessage(deps, alice.sock, alice.conn, frame(OP.TICKET_REQUEST, new Uint8Array([2])));
  const grant = alice.sock.take(OP.TICKET_GRANT);
  assert.equal(grant[0], 2);
  const first = grant.subarray(1, 1 + TICKET_LEN);
  const second = grant.subarray(1 + TICKET_LEN, 1 + 2 * TICKET_LEN);
  assert.notEqual(toHex(first), toHex(second));

  const a1 = anonConnect(deps);
  handleMessage(deps, a1.sock, a1.conn, sendAnonFrame(first, bob.devPub, ascii("a")));
  assert.ok(a1.sock.has(OP.SEND_OK));

  const a2 = anonConnect(deps);
  handleMessage(deps, a2.sock, a2.conn, sendAnonFrame(second, bob.devPub, ascii("b")));
  assert.ok(a2.sock.has(OP.SEND_OK), "второй билет из той же пачки обязан остаться рабочим");
  store.close();
});

test("SEND_ANON с телом неверной длины — битый кадр, не тихий отказ", () => {
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  const anon = anonConnect(deps);
  handleMessage(deps, anon.sock, anon.conn, frame(OP.SEND_ANON, new Uint8Array(4)));
  assert.equal(anon.sock.closed?.code, 1002);
  store.close();
});

test("KEY_LEN/ID_LEN честные размеры используются в разборе — регресс на будущее", () => {
  // Просто фиксирует константы, которыми оперирует remainder этого файла:
  // если формат билета когда-нибудь изменится, этот тест первым укажет, где.
  assert.equal(ID_LEN, 16);
  assert.equal(KEY_LEN, 32);
  assert.equal(TICKET_LEN, 16 + 8 + 64);
});
