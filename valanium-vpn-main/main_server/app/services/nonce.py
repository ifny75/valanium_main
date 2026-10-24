"""Одноразовые challenge-nonce. Только в памяти.

На диск им не нужно: живут тридцать секунд, а запись на диск — это след,
по которому видно, кто и когда подходил к двери. Перезапуск сервиса стоит
одного повторного `/v1/hello` со стороны клиента.

Потолок нужен потому, что nonce висит свои тридцать секунд независимо от
того, ушёл ли запросивший: забрать challenge и оборвать связь можно быстрее,
чем протухает предыдущий. Упершись в потолок, `issue` возвращает None —
осознанный отказ: лучше сказать «занято» одному, чем остаться без памяти
для всех.

Ключей клиента здесь нет вовсе — nonce не привязан ни к кому. Он одноразовый
и короткоживущий, а к кому он относится, доказывает подпись поверх него.
"""

import secrets
import time


class NonceStore:
    def __init__(self, ttl_sec: int = 30, max_outstanding: int = 50_000) -> None:
        self._issued: dict[bytes, float] = {}
        self._ttl = ttl_sec
        self._max = max_outstanding

    def issue(self, now: float | None = None) -> bytes | None:
        now = time.monotonic() if now is None else now
        if len(self._issued) >= self._max:
            self.sweep(now)
            if len(self._issued) >= self._max:
                return None
        nonce = secrets.token_bytes(32)
        self._issued[nonce] = now + self._ttl
        return nonce

    def consume(self, nonce: bytes, now: float | None = None) -> bool:
        """True ровно один раз на каждый выданный nonce."""
        now = time.monotonic() if now is None else now
        expires = self._issued.pop(nonce, None)
        return expires is not None and expires > now

    def sweep(self, now: float | None = None) -> None:
        now = time.monotonic() if now is None else now
        for key in [k for k, exp in self._issued.items() if exp <= now]:
            self._issued.pop(key, None)

    def __len__(self) -> int:
        return len(self._issued)


class RateLimiter:
    """Скользящее окно в памяти. Ключ — что угодно строковое.

    Адрес клиента сюда попадает и никуда больше: считать частоту попыток
    привязки без него нельзя, а писать его на диск — нельзя тем более.
    Ключи исчезают вместе с окном, так что карта не растёт по числу
    увиденных адресов и не превращается в свою собственную утечку.
    """

    def __init__(self, limit: int, window_sec: int) -> None:
        self._hits: dict[str, list[float]] = {}
        self._limit = limit
        self._window = window_sec

    def allow(self, key: str, now: float | None = None) -> bool:
        now = time.monotonic() if now is None else now
        cutoff = now - self._window
        hits = [t for t in self._hits.get(key, []) if t > cutoff]
        if len(hits) >= self._limit:
            self._hits[key] = hits
            return False
        hits.append(now)
        self._hits[key] = hits
        return True

    def sweep(self, now: float | None = None) -> None:
        now = time.monotonic() if now is None else now
        cutoff = now - self._window
        for key in list(self._hits):
            kept = [t for t in self._hits[key] if t > cutoff]
            if kept:
                self._hits[key] = kept
            else:
                del self._hits[key]
