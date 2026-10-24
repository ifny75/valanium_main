/**
 * Самопроверка сервера: то, что ломалось молча.
 *
 * # Зачем
 *
 * За время работы четыре отказа прошли незамеченными, и ни один не был виден
 * снаружи: служба `active`, сайт открывается, сообщения ходят.
 *
 * 1. Таймер бэкапа падал на несуществующем пути — двое суток без копий.
 * 2. Приёмник копий забанил сам себя через fail2ban — отправка встала.
 * 3. После переименования таймер оказался выключен — четыре дня без копий.
 * 4. База подменилась пустой, и пятнадцать аккаунтов исчезли.
 *
 * Общее у всех: **поломка не мешает работе**. Мессенджер продолжает
 * доставлять сообщения и с мёртвым бэкапом, и с пустой базой. Поэтому
 * замечают такое не по журналу, а по жалобам людей — то есть слишком поздно.
 *
 * # Что проверяем
 *
 * Не «работает ли сервер» — на это отвечает `/v1/health`. Здесь проверяется
 * то, чего работающий сервер о себе не сообщает: свежесть копий, отправку их
 * наружу, и что база не потеряла население.
 *
 * # Про порог по числу аккаунтов
 *
 * Помним максимум и поднимаем тревогу, если осталось меньше половины. Именно
 * половина, а не любое уменьшение: люди удаляют аккаунты, и алерт на каждое
 * удаление отучает смотреть на алерты. Зато подмена базы пустой — падение до
 * нуля — не проходит незамеченной ни при каком пороге.
 *
 * Планка ниже пяти не действует: на новом сервере, где людей единицы, любое
 * колебание было бы «половиной».
 *
 *   node --env-file-if-exists=.env src/tools/selfcheck.ts [--json <файл>]
 */
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

import { config } from "../config.ts";
import { Store } from "../db/index.ts";

/** Копия старше этого — уже не защита, а иллюзия защиты. */
const FRESH_HOURS = 26;

/** Ниже этого числа аккаунтов порог по населению не применяется. */
const POPULATION_FLOOR = 5;

export type Check = { id: string; ok: boolean; detail: string };

/** Возраст самого свежего содержимого каталога в часах; null — пусто или нет. */
export function newestAgeHours(dir: string, now = Date.now()): number | null {
  if (!existsSync(dir)) return null;
  const entries = readdirSync(dir)
    .map((name) => {
      try {
        return statSync(join(dir, name)).mtimeMs;
      } catch {
        return 0;
      }
    })
    .filter((value) => value > 0);
  if (entries.length === 0) return null;
  return (now - Math.max(...entries)) / 3_600_000;
}

/** Тревога по населению: осталось меньше половины прежнего максимума. */
export function populationLost(users: number, seen: number): boolean {
  if (seen < POPULATION_FLOOR) return false;
  return users * 2 < seen;
}

function freshness(id: string, dir: string, now: number): Check {
  const age = newestAgeHours(dir, now);
  if (age === null) {
    return { id, ok: false, detail: `нет ни одной копии в ${dir}` };
  }
  const hours = age.toFixed(1);
  return age <= FRESH_HOURS
    ? { id, ok: true, detail: `свежая, ${hours} ч назад` }
    : { id, ok: false, detail: `последняя ${hours} ч назад, ожидалось не старше ${FRESH_HOURS} ч` };
}

export function runChecks(now = Date.now()): Check[] {
  const checks: Check[] = [];
  const data = dirname(config.dbPath);

  checks.push(freshness("backup", "/var/backups/valanium", now));
  // Отметку об удачной отправке ставит сам отправитель: прочитать приёмник мы
  // не можем и не должны — канал туда односторонний намеренно.
  checks.push(freshness("offsite", join(data, "offsite"), now));

  // Население. Планку храним рядом с базой, а не в самой базе: подменённая
  // база принесла бы с собой и подменённую планку, и проверка промолчала бы.
  const markPath = join(data, "population.json");
  let seen = 0;
  try {
    seen = Number(JSON.parse(readFileSync(markPath, "utf8")).seen) || 0;
  } catch {
    seen = 0;
  }

  const store = new Store(config.dbPath);
  let users = 0;
  try {
    users = store.countUsers();
  } finally {
    store.close();
  }

  if (populationLost(users, seen)) {
    checks.push({
      id: "population",
      ok: false,
      detail: `аккаунтов ${users}, а было ${seen} — похоже на подмену базы`,
    });
  } else {
    checks.push({ id: "population", ok: true, detail: `аккаунтов ${users}` });
    if (users > seen) {
      writeFileSync(markPath, JSON.stringify({ seen: users, at: new Date(now).toISOString() }));
    }
  }

  return checks;
}

// --- запуск ------------------------------------------------------------------

if (process.argv[1] && import.meta.filename === process.argv[1]) {
  const checks = runChecks();
  const healthy = checks.every((check) => check.ok);

  for (const check of checks) {
    process.stdout.write(`${check.ok ? "ok  " : "СБОЙ"} ${check.id}: ${check.detail}\n`);
  }

  const target = process.argv.indexOf("--json");
  if (target > 0 && process.argv[target + 1]) {
    writeFileSync(process.argv[target + 1] as string, JSON.stringify({
      ok: healthy,
      checkedAt: new Date().toISOString(),
      // Наружу — только идентификатор и признак: подробности содержат числа,
      // которых публичной странице состояния знать незачем.
      checks: checks.map((check) => ({ id: check.id, ok: check.ok })),
    }), { mode: 0o644 });
  }

  // Ненулевой код — чтобы systemd считал запуск неудачным и это было видно
  // в `systemctl is-failed`, даже если журнал никто не читает.
  process.exit(healthy ? 0 : 1);
}
