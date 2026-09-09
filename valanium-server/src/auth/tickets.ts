/**
 * Sealed sender: анонимные разовые билеты на доставку (ARCHITECTURE.md §7a).
 *
 * Проблема, которую это закрывает: `SEND` сегодня идёт по уже
 * аутентифицированному соединению — сервер в момент отправки знает и личность
 * отправителя (из `AUTH`), и адрес получателя (из тела кадра). В базу и в
 * логи эта связка не попадает (`onSend` кладёт в очередь только получателя и
 * шифротекст), но в оперативной памяти процесса она существует, пока длится
 * соединение — а значит видна тому, кто имеет доступ к этому процессу.
 *
 * Билет разрывает эту связь. Устройство запрашивает пачку билетов по
 * обычному, аутентифицированному соединению — здесь личность известна и
 * ограничитель частоты считает как раньше, по личности. Билет — это
 * случайный nonce, срок жизни и подпись сервера над ними; личности в нём нет.
 * Отправка идёт уже по-другому, отдельному соединению (`SEND_ANON`), которое
 * никогда не проходит `AUTH`: оно предъявляет билет вместо подписи, и связать
 * его с личностью, выпросившей билет, серверу нечем — они разделены и по
 * времени, и по соединению.
 *
 * Кто получатель, доказывает не сервер, а MLS: сообщение внутри шифротекста
 * подписано ключом устройства-участника группы, и получатель проверяет это
 * сам, как и раньше. Серверу тут проверять нечего и не нужно.
 */
import { chmodSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

import { ed25519 } from "@noble/curves/ed25519";
import { ticketMessage } from "./verify.ts";
import { concat, random, readU64BE, toHex, writeU64BE } from "../util/bytes.ts";

export const NONCE_LEN = 16;
export const SIG_LEN = 64;
/** `[16B nonce][8B expiry][64B sig]` */
export const TICKET_LEN = NONCE_LEN + 8 + SIG_LEN;

/**
 * Ключ живёт только на этом сервере: билет и выпускает, и проверяет один и
 * тот же процесс, наружу подпись никогда не показывается. Поэтому, в отличие
 * от `SecretBox` или подписи onion-входов, держать ключ офлайн незачем —
 * онлайн-ключ здесь не ослабляет свойство, которое билет обеспечивает.
 */
export class TicketKey {
  readonly #priv: Uint8Array;
  readonly #pub: Uint8Array;

  constructor(priv: Uint8Array) {
    this.#priv = priv;
    this.#pub = ed25519.getPublicKey(priv);
  }

  /** Тот же приём, что у `SecretBox.load`: файл рядом с базой, заводится сам. */
  static load(dbPath: string): TicketKey {
    if (dbPath === ":memory:") return new TicketKey(ed25519.utils.randomSecretKey());

    const fromEnv = process.env.VALANIUM_TICKET_KEY;
    if (fromEnv !== undefined && fromEnv !== "") {
      return new TicketKey(Buffer.from(fromEnv, "hex"));
    }

    const dir = dirname(dbPath);
    const keyPath = join(dir, "ticket.key");
    try {
      const raw = readFileSync(keyPath, "utf8").trim();
      return new TicketKey(Buffer.from(raw, "hex"));
    } catch {
      mkdirSync(dir, { recursive: true });
      const key = Buffer.from(ed25519.utils.randomSecretKey());
      writeFileSync(keyPath, key.toString("hex"), { mode: 0o600 });
      try {
        chmodSync(keyPath, 0o600);
      } catch {
        // Windows прав не знает — на разработческой машине это нормально.
      }
      return new TicketKey(key);
    }
  }

  sign(nonce: Uint8Array, expiry: Uint8Array): Uint8Array {
    return ed25519.sign(ticketMessage(nonce, expiry), this.#priv);
  }

  verify(sig: Uint8Array, nonce: Uint8Array, expiry: Uint8Array): boolean {
    try {
      return ed25519.verify(sig, ticketMessage(nonce, expiry), this.#pub, { zip215: false });
    } catch {
      return false;
    }
  }
}

/**
 * Выпуск и разовое погашение билетов.
 *
 * Погашённые nonce держим в памяти до истечения их же срока — тем же приёмом,
 * что `NonceStore`: карта ограничена сверху, и упёршись в потолок, билет лучше
 * отказаться принять, чем расти без границ. Билетов на руках всегда мало —
 * их выпуск сам ограничен ниже, в `onTicketRequest` — так что предела на
 * практике никогда не достичь.
 */
export class TicketStore {
  readonly #key: TicketKey;
  readonly #ttlMs: number;
  readonly #max: number;
  readonly #spent = new Map<string, number>();

  constructor(key: TicketKey, ttlSec: number, max = 50_000) {
    this.#key = key;
    this.#ttlMs = ttlSec * 1000;
    this.#max = max;
  }

  /** Пачка билетов с общим сроком жизни. */
  issue(count: number, now: number): Uint8Array[] {
    const expiry = writeU64BE(BigInt(now + this.#ttlMs));
    const tickets: Uint8Array[] = [];
    for (let i = 0; i < count; i++) {
      const nonce = random(NONCE_LEN);
      const sig = this.#key.sign(nonce, expiry);
      tickets.push(concat(nonce, expiry, sig));
    }
    return tickets;
  }

  /**
   * `true` — билет настоящий, не протух и погашается прямо сейчас; второй раз
   * тот же билет `true` уже не вернёт. Все причины отказа — просрочен, подпись
   * не сошлась, уже потрачен, в памяти не осталось места — снаружи неразличимы
   * намеренно: рассказывать, чем именно билет плох, некому, кроме того, кто
   * его подделывает.
   */
  redeem(ticket: Uint8Array, now: number): boolean {
    if (ticket.byteLength !== TICKET_LEN) return false;
    const nonce = ticket.subarray(0, NONCE_LEN);
    const expiryBytes = ticket.subarray(NONCE_LEN, NONCE_LEN + 8);
    const sig = ticket.subarray(NONCE_LEN + 8);

    const expiry = readU64BE(ticket, NONCE_LEN);
    if (expiry <= BigInt(now)) return false;
    if (!this.#key.verify(sig, nonce, expiryBytes)) return false;

    const key = toHex(nonce);
    if (this.#spent.has(key)) return false;
    if (this.#spent.size >= this.#max) {
      this.#sweep(now);
      if (this.#spent.size >= this.#max) return false;
    }
    this.#spent.set(key, Number(expiry));
    return true;
  }

  sweep(now: number): void {
    this.#sweep(now);
  }

  #sweep(now: number): void {
    for (const [key, expiry] of this.#spent) {
      if (expiry <= now) this.#spent.delete(key);
    }
  }

  get size(): number {
    return this.#spent.size;
  }
}
