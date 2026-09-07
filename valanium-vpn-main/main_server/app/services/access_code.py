"""Код доступа — единственное, что человек получает при покупке.

Как в Mullvad: ни почты, ни пароля, ни имени. Отличие одно, и оно
существенное: у Mullvad номер счёта — постоянный пароль, он едет на сервер
при каждом входе и годится любому, кто его подсмотрел. Здесь код тратится
один раз, чтобы привязать устройство (`POST /v1/devices`), а дальше
устройство входит своей подписью (`ed25519.py`), и код по проводу больше не
ходит. Подсмотренный код всё ещё позволяет привязать своё устройство — от
этого спасает только лимит устройств и их список в клиенте, — но не
превращается в вечный ключ ко всем сессиям.

Формат: 20 знаков, четыре группы по пять.

    VLNM7-3KQXA-9PW2H-TRF4M

16 знаков — случайные (80 бит), 4 — контрольная сумма. Алфавит
кроукфордовский base32 без `I`, `L`, `O`, `U`: первые три путают с `1` и `0`
при переписывании от руки, последняя выпадает, чтобы код не складывался в
слово. При разборе `I`/`L` читаются как `1`, `O` как `0` — человек, который
всё-таки написал букву, не получит «неверный код».

Контрольная сумма нужна не ради криптографии, а чтобы опечатка называлась
опечаткой: без неё любой промах выглядит как «такого счёта нет», и человек
идёт в поддержку вместо того, чтобы перечитать четвёртую группу.
"""

import hashlib
import hmac
import secrets
from pathlib import Path

ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"
DATA_CHARS = 16          # 16 * 5 бит = 80 бит энтропии
CHECK_CHARS = 4
GROUP = 5

# Что человек мог написать вместо знака алфавита.
CONFUSABLE = {"I": "1", "L": "1", "O": "0", "U": "V"}

# scrypt на 16 MiB: ~50 мс на том железе, где это крутится (1 vCPU).
_SCRYPT_N = 2 ** 14
_SCRYPT_R = 8
_SCRYPT_P = 1


def _checksum(data: str) -> str:
    """Первые 20 бит SHA-256 от значащей части, записанные четырьмя знаками."""
    digest = hashlib.sha256(data.encode("ascii")).digest()
    value = int.from_bytes(digest[:3], "big") >> 4  # ровно 20 бит
    return "".join(ALPHABET[(value >> (5 * i)) & 31] for i in reversed(range(CHECK_CHARS)))


def generate() -> str:
    """Новый код в человекочитаемом виде, с дефисами."""
    data = "".join(secrets.choice(ALPHABET) for _ in range(DATA_CHARS))
    return format_code(data + _checksum(data))


def normalize(raw: str) -> str | None:
    """Приводит введённое человеком к канонической форме без дефисов.

    Возвращает None, если это не код: не та длина, чужие знаки или не сошлась
    контрольная сумма. Регистр, пробелы, дефисы и путаница букв с цифрами
    ошибкой не считаются.
    """
    if not raw:
        return None
    cleaned = []
    for ch in raw.upper():
        if ch in " -_\t\n\r":
            continue
        ch = CONFUSABLE.get(ch, ch)
        if ch not in ALPHABET:
            return None
        cleaned.append(ch)
    code = "".join(cleaned)
    if len(code) != DATA_CHARS + CHECK_CHARS:
        return None
    data, check = code[:DATA_CHARS], code[DATA_CHARS:]
    if not hmac.compare_digest(check, _checksum(data)):
        return None
    return code


def format_code(code: str) -> str:
    """Каноническая форма → то, что показывают человеку."""
    return "-".join(code[i:i + GROUP] for i in range(0, len(code), GROUP))


def hash_code(code: str, pepper: bytes) -> str:
    """Детерминированный хеш для поиска по коду.

    Детерминированный — потому что искать надо по самому коду: соль на запись
    заставила бы перебирать всю таблицу. Отсюда и перец: без него одинаковые
    коды в разных установках дают одинаковый хеш, и украденную таблицу можно
    перебирать заранее посчитанным словарём. С ним украденная таблица без
    файла перца бесполезна, а 80 бит энтропии делают перебор безнадёжным
    и при известном перце.

    scrypt, а не Argon2id как в мессенджере: он есть в стандартной библиотеке,
    а лишняя зависимость на узле — лишний повод сборке не собраться.
    """
    canonical = normalize(code)
    if canonical is None:
        raise ValueError("not a valid access code")
    return hashlib.scrypt(
        canonical.encode("ascii"),
        salt=pepper,
        n=_SCRYPT_N,
        r=_SCRYPT_R,
        p=_SCRYPT_P,
        dklen=32,
        maxmem=64 * 1024 * 1024,
    ).hex()


def load_or_create_pepper(path: str | Path) -> bytes:
    """Перец живёт в файле рядом с базой и создаётся при первом запуске.

    Он не в .env намеренно: потеря перца — это потеря всех выданных кодов,
    а переменные окружения теряются заметно легче файла, который лежит в
    одном каталоге с базой и попадает в тот же бэкап.
    """
    p = Path(path)
    if p.exists():
        value = p.read_text(encoding="ascii").strip()
        if value:
            return bytes.fromhex(value)
    pepper = secrets.token_bytes(32)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(pepper.hex(), encoding="ascii")
    try:
        p.chmod(0o600)
    except (OSError, NotImplementedError):  # Windows-разработка, не боевой путь
        pass
    return pepper
