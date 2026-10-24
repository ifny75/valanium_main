/**
 * Самопроверка: ловит ли она то, что уже ломалось молча.
 *
 * Каждый случай здесь — не выдумка, а произошедший отказ. Проверка, которая их
 * не ловит, бесполезна: именно эти четыре прошли мимо журналов и мимо людей.
 */
import { mkdtempSync, rmSync, writeFileSync, utimesSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

import { newestAgeHours, populationLost } from "../src/tools/selfcheck.ts";

function dir(): string {
  return mkdtempSync(join(tmpdir(), "valanium-selfcheck-"));
}

/** Кладёт файл с заданным возрастом в часах. */
function aged(home: string, name: string, hours: number): void {
  const path = join(home, name);
  writeFileSync(path, "");
  const when = new Date(Date.now() - hours * 3_600_000);
  utimesSync(path, when, when);
}

test("свежая копия видна как свежая", () => {
  const home = dir();
  aged(home, "2026-09-06T00-00-00Z", 2);
  const age = newestAgeHours(home);
  assert.ok(age !== null && age < 3, `ожидали около 2 ч, вышло ${age}`);
  rmSync(home, { recursive: true, force: true });
});

test("остановившийся таймер бэкапа виден", () => {
  /*
    Так и было: таймер падал на несуществующем пути двое суток. Служба при
    этом оставалась active, сообщения ходили, снаружи всё выглядело здоровым.
  */
  const home = dir();
  aged(home, "2026-09-01T00-00-00Z", 50);
  const age = newestAgeHours(home);
  assert.ok(age !== null && age > 26, `устаревшая копия обязана быть заметна: ${age}`);
  rmSync(home, { recursive: true, force: true });
});

test("отсутствие копий вообще — тоже отказ, а не пустота", () => {
  // После переименования каталог оказался другим, и копий не стало вовсе.
  // Молчание тут было бы худшим ответом: «нет копий» неотличимо от «всё ок».
  const home = dir();
  assert.equal(newestAgeHours(home), null, "пустой каталог");
  assert.equal(newestAgeHours(join(home, "нет-такого")), null, "каталога нет");
  rmSync(home, { recursive: true, force: true });
});

test("подмена базы пустой не проходит", () => {
  // Пятнадцать аккаунтов исчезли, сервер продолжил работать как ни в чём не
  // бывало. Падение до нуля обязано ловиться при любом пороге.
  assert.ok(populationLost(0, 15), "пустая база после пятнадцати аккаунтов");
  assert.ok(populationLost(4, 15), "осталось меньше половины");
});

test("обычное удаление аккаунтов тревогу не поднимает", () => {
  /*
    Порог — половина, а не любое уменьшение. Алерт на каждое удаление отучает
    смотреть на алерты, и тогда настоящую пропажу тоже пропустят.
  */
  assert.ok(!populationLost(14, 15), "один ушёл");
  assert.ok(!populationLost(8, 15), "ушла половина без малого");
  assert.ok(!populationLost(15, 15), "никто не уходил");
});

test("на новом сервере порог не действует", () => {
  // Пока людей единицы, любое колебание было бы «половиной», и проверка
  // кричала бы с первого дня.
  assert.ok(!populationLost(0, 4), "четверо — ещё не население");
  assert.ok(!populationLost(1, 2), "двое");
});

test("рост населения тревоги не вызывает", () => {
  assert.ok(!populationLost(30, 15));
});
