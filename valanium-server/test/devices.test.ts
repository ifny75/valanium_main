/**
 * Несколько устройств у одной личности.
 *
 * Сервер умел это с самого начала: сертификат устройства подписан ключом
 * личности, очереди конвертов раздельные, потолок — восемь. Схлопывалось всё
 * в одном месте: и поиск, и профиль отдавали ровно одно устройство, самое
 * свежее. Сообщение уходило туда, а до телефона, лежавшего в кармане, не
 * доезжало никогда — и выглядело это не как потеря, а как «ему не пришло».
 *
 * Здесь проверяется, что список отдаётся целиком и что старое поле осталось
 * на месте: выпущенные клиенты читают именно его.
 */
import test from "node:test";
import assert from "node:assert/strict";

import { randomBytes } from "node:crypto";

import { Store } from "../src/db/index.ts";

/** Ключи здесь не проверяются — важен только их состав, поэтому просто байты. */
function keys() {
  return { identity: randomBytes(32), device: randomBytes(32) };
}

test("каталог отдаёт все устройства личности, а не одно", () => {
  const store = new Store(":memory:");
  const alice = keys();
  const now = Date.now();
  store.createUser(alice.identity, now);
  store.createDevice(alice.identity, alice.device, randomBytes(64), now);
  // Второе устройство той же личности: тот же identity, свой ключ устройства.
  store.createDevice(alice.identity, randomBytes(32), randomBytes(64), now);

  assert.equal(store.listDevices(alice.identity).length, 2,
    "оба устройства обязаны остаться в каталоге");
  // А выбор «самого свежего» по-прежнему возвращает одно: он для выпущенных
  // клиентов, и ломать его нельзя.
  assert.ok(store.activeDevice(alice.identity), "одно устройство для выпущенных клиентов");
  store.close();
});

test("отозванное устройство из списка уходит", () => {
  /*
    Отзыв обязан убирать устройство из выдачи, иначе «выйти на других
    устройствах» перестаёт что-либо значить: отправитель продолжит слать копии
    туда, откуда человек только что вышел.
  */
  const store = new Store(":memory:");
  const alice = keys();
  const now = Date.now();
  store.createUser(alice.identity, now);
  store.createDevice(alice.identity, alice.device, randomBytes(64), now);
  store.createDevice(alice.identity, randomBytes(32), randomBytes(64), now);
  assert.equal(store.listDevices(alice.identity).length, 2);

  store.revokeOtherDevices(alice.identity, alice.device, now);
  const left = store.listDevices(alice.identity);
  assert.equal(left.length, 1, "остаётся только то, на котором вышли");
  assert.deepEqual(Buffer.from(left[0]!.device_pub), alice.device);
  store.close();
});

test("потолок устройств считает только живые", () => {
  // Отозванные остаются надгробием, чтобы прежний cert не завёл устройство
  // заново. В потолок они входить не должны, иначе человек, сменивший восемь
  // телефонов, больше не заведёт ни одного.
  const store = new Store(":memory:");
  const alice = keys();
  const now = Date.now();
  store.createUser(alice.identity, now);
  store.createDevice(alice.identity, alice.device, randomBytes(64), now);
  store.createDevice(alice.identity, randomBytes(32), randomBytes(64), now);
  assert.equal(store.countActiveDevices(alice.identity), 2);

  store.revokeOtherDevices(alice.identity, alice.device, now);
  assert.equal(store.countActiveDevices(alice.identity), 1, "отозванное не занимает место");
  store.close();
});
