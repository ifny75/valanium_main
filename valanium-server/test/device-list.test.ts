/**
 * Свой список устройств — и только свой.
 *
 * Список решает, кому уйдёт копия сообщения: отправитель шифрует каждому
 * устройству собеседника отдельно. Поэтому у него две несовместимые на первый
 * взгляд роли.
 *
 * Собеседнику список нужен — но брать его у сервера он не должен: сервер,
 * вписавший туда своё устройство, получал бы открытые копии переписки. Своему
 * же владельцу сервер список отдать обязан: иначе тот не сможет разослать его
 * собеседникам по шифрованному каналу, где сервер уже ничего не решает.
 *
 * Разводятся эти роли одним: личность берётся из сессии, а не из тела запроса.
 * Назвать в этом кадре чужую личность попросту нечем.
 */
import test from "node:test";
import assert from "node:assert/strict";

import { ed25519 } from "@noble/curves/ed25519";
import { sha256 } from "@noble/hashes/sha2";

import { Store } from "../src/db/index.ts";
import { SupportStore } from "../src/support/store.ts";
import { NonceStore } from "../src/auth/nonce.ts";
import { Registry, type Socket } from "../src/ws/registry.ts";
import { RateLimiter } from "../src/util/ratelimit.ts";
import { ConnectionCounter } from "../src/util/connections.ts";
import { authMessage, deviceCertMessage, verify } from "../src/auth/verify.ts";
import { handleMessage, handleOpen, newConnData, type Deps } from "../src/ws/session.ts";
import { OP, jsonFrame } from "../src/proto/frames.ts";
import { ascii, fromHex, random, toHex } from "../src/util/bytes.ts";

class FakeSocket implements Socket {
  readonly sent: Uint8Array[] = [];
  closed: { code: number } | null = null;
  send(data: Uint8Array): number {
    this.sent.push(data.slice());
    return 1;
  }
  getBufferedAmount(): number {
    return 0;
  }
  end(code: number): void {
    this.closed = { code };
  }
  close(): void {
    this.closed = { code: 1006 };
  }
  take(op: number): Uint8Array {
    const found = this.sent.find((f) => f[0] === op);
    assert.ok(found, `нет кадра 0x${op.toString(16)}`);
    return found;
  }
  json(op: number): any {
    return JSON.parse(new TextDecoder().decode(this.take(op).subarray(1)));
  }
  has(op: number): boolean {
    return this.sent.some((f) => f[0] === op);
  }
  clear(): void {
    this.sent.length = 0;
  }
}

function makeDeps(store: Store): Deps {
  return {
    store,
    support: new SupportStore(":memory:"),
    nonces: new NonceStore(30_000),
    registry: new Registry(),
    authLimiter: new RateLimiter(100, 60_000),
    recoveryLimiter: new RateLimiter(100, 3600_000),
    searchLimiter: new RateLimiter(100, 60_000),
    sendLimiter: new RateLimiter(1000, 60_000),
    postLimiter: new RateLimiter(1000, 60_000),
    claimLimiter: new RateLimiter(1000, 3600_000),
    connections: new ConnectionCounter(),
    now: () => Date.now(),
  };
}

function keys() {
  const idPriv = ed25519.utils.randomSecretKey();
  const devPriv = ed25519.utils.randomSecretKey();
  return {
    idPriv,
    devPriv,
    idPub: ed25519.getPublicKey(idPriv),
    devPub: ed25519.getPublicKey(devPriv),
  };
}

/** Заводит устройство и входит им. Личность можно переиспользовать. */
function login(deps: Deps, store: Store, id: ReturnType<typeof keys>, handle: string) {
  const sock = new FakeSocket();
  const conn = newConnData("127.0.0.1");
  handleOpen(deps, sock, conn);
  const nonce = fromHex(sock.json(OP.HELLO).nonce, 32);

  const code = toHex(random(12));
  store.createInvite(sha256(ascii(code)), Date.now(), Date.now() + 3600_000);

  handleMessage(deps, sock, conn, jsonFrame(OP.AUTH, {
    v: 1,
    identity: toHex(id.idPub),
    device: toHex(id.devPub),
    deviceCert: toHex(ed25519.sign(deviceCertMessage(id.idPub, id.devPub), id.idPriv)),
    sig: toHex(ed25519.sign(authMessage(nonce, id.idPub, id.devPub), id.devPriv)),
    invite: code,
    handle,
  }));
  assert.ok(sock.has(OP.AUTH_OK), "вход не прошёл");
  sock.clear();
  return { sock, conn };
}

/** Второе устройство той же личности: тот же identity, свой ключ устройства. */
function second(id: ReturnType<typeof keys>) {
  const devPriv = ed25519.utils.randomSecretKey();
  return { ...id, devPriv, devPub: ed25519.getPublicKey(devPriv) };
}

test("владелец получает все свои устройства с подписями", () => {
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  const alice = keys();

  login(deps, store, alice, "alice");
  const phone = login(deps, store, second(alice), "alice");

  handleMessage(deps, phone.sock, phone.conn, jsonFrame(OP.DEVICE_LIST, {}));
  const answer = phone.sock.json(OP.DEVICE_OK);

  assert.equal(answer.identity, toHex(alice.idPub));
  assert.equal(answer.devices.length, 2, "оба устройства обязаны приехать");

  // Подпись обязана проверяться ключом личности: клиент не верит серверу на
  // слово даже про собственные устройства — иначе в список можно дописать.
  for (const entry of answer.devices) {
    assert.ok(
      verify(fromHex(entry.cert, 64), deviceCertMessage(alice.idPub, fromHex(entry.device, 32)), alice.idPub),
      `подпись не сошлась: ${entry.device}`,
    );
  }
  store.close();
});

test("чужой список этим кадром не спросить", () => {
  /*
    Ровно то свойство, из-за которого этот кадр безопасен, а тот же список в
    ответе на поиск — нет. Здесь называть чужую личность нечем: она берётся из
    сессии. Всё, что положено в тело, ответ не меняет.
  */
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  const alice = keys();
  const mallory = keys();

  login(deps, store, alice, "alice");
  login(deps, store, second(alice), "alice");
  const attacker = login(deps, store, mallory, "mallory");

  handleMessage(deps, attacker.sock, attacker.conn, jsonFrame(OP.DEVICE_LIST, {
    identity: toHex(alice.idPub),
  }));
  const answer = attacker.sock.json(OP.DEVICE_OK);

  assert.equal(answer.identity, toHex(mallory.idPub), "ответ обязан быть про себя");
  assert.equal(answer.devices.length, 1, "чужие устройства просочились в ответ");
  assert.equal(answer.devices[0].device, toHex(mallory.devPub));
  store.close();
});

test("отозванное устройство в свой же список не попадает", () => {
  // Иначе «выйти на других устройствах» ничего не значит: владелец разошлёт
  // собеседникам список, где отозванное всё ещё числится, и копии продолжат
  // уходить туда, откуда человек только что вышел.
  const store = new Store(":memory:");
  const deps = makeDeps(store);
  const alice = keys();

  login(deps, store, alice, "alice");
  const phone = login(deps, store, second(alice), "alice");

  handleMessage(deps, phone.sock, phone.conn, jsonFrame(OP.DEVICE_LIST, {}));
  assert.equal(phone.sock.json(OP.DEVICE_OK).devices.length, 2);
  phone.sock.clear();

  store.revokeOtherDevices(alice.idPub, fromHex(toHex(phone.conn.devicePub!), 32), Date.now());

  handleMessage(deps, phone.sock, phone.conn, jsonFrame(OP.DEVICE_LIST, {}));
  const left = phone.sock.json(OP.DEVICE_OK);
  assert.equal(left.devices.length, 1, "отозванное осталось в списке");
  store.close();
});
