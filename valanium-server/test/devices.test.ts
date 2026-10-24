/**
 * Несколько устройств у одной личности.
 *
 * Сервер умел это с самого начала: сертификат устройства подписан ключом
 * личности, очереди конвертов раздельные, потолок — восемь. Схлопывалось всё
 * в одном месте: и поиск, и профиль отдавали ровно одно устройство, самое
 * свежее. Сообщение уходило туда, а до телефона, лежавшего в кармане, не
 * доезжало никогда — и выглядело это не как потеря, а как «ему не пришло».
 *
 * Здесь проверяется сам каталог: что он отдаёт все живые устройства личности,
 * что отозванное из него уходит и что рядом с каждым ключом лежит подпись
 * личности под ним. Наружу этот список сервер не отдаёт и отдавать не должен —
 * почему, объяснено в тесте про подпись.
 */
import test from "node:test";
import assert from "node:assert/strict";

import { randomBytes } from "node:crypto";

import { ed25519 } from "@noble/curves/ed25519";

import { deviceCertMessage, verify } from "../src/auth/verify.ts";

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
  // Выбор «самого свежего» остаётся: по нему сервер отвечает на поиск и
  // профиль, и ломать его нельзя.
  assert.ok(store.activeDevice(alice.identity), "самое свежее устройство");
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

test("каталог несёт подпись личности под каждым устройством", () => {
  /*
    Без подписи каталог верить нельзя.

    Отправитель шифрует каждому устройству из списка отдельно. Если список
    принимать на слово, сервер вписывает в него своё устройство и получает
    открытые копии переписки: сквозное шифрование формально не нарушено,
    просто получателей стало на одного больше, и заметить это неоткуда.

    Поэтому рядом с ключом лежит та самая подпись, которой устройство
    доказывало право войти. Проверять её обязан отправитель, а не сервер.

    Из этого же следует, что каталог не годится для выдачи наружу по запросу:
    чтобы проверить подпись, к списку пришлось бы приложить ключ личности, а он
    — постоянный опознаватель, переживающий и смену устройств, и смену
    юзернейма, и блокировку. Отдавать такое любому, кто набрал имя в поиске,
    нельзя (см. «поиск не раскрывает ничего сверх профиля» в username.test.ts).
    Свой список устройств человек рассылает сам — по уже установленному
    шифрованному каналу, как это делается с пропусками и ключом от аватара.
  */
  const store = new Store(":memory:");
  const identity = ed25519.utils.randomSecretKey();
  const identityPub = ed25519.getPublicKey(identity);
  const devicePub = randomBytes(32);
  const cert = ed25519.sign(deviceCertMessage(identityPub, devicePub), identity);
  const now = Date.now();

  store.createUser(identityPub, now);
  store.createDevice(identityPub, devicePub, cert, now);

  const row = store.listDevices(identityPub)[0]!;
  assert.ok(
    verify(row.cert, deviceCertMessage(identityPub, row.device_pub), identityPub),
    "подпись из каталога обязана проверяться ключом личности",
  );

  // А подделка — не проходит: сервер, вписавший своё устройство, приложить к
  // нему верную подпись не может, не имея приватного ключа личности.
  const intruder = randomBytes(32);
  assert.ok(
    !verify(row.cert, deviceCertMessage(identityPub, intruder), identityPub),
    "подпись одного устройства не годится для другого",
  );
  store.close();
});
