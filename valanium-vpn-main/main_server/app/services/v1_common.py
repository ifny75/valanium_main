"""Мелочи, общие для клиентского и админского путей.

SQLite хранит время без зоны, а `datetime.now(timezone.utc)` возвращает
осведомлённое. Сравнение того и другого бросает TypeError в самом неудобном
месте — на проверке срока, — поэтому вся запись идёт через одну функцию.
"""

from datetime import datetime, timezone


def now_naive() -> datetime:
    return datetime.now(timezone.utc).replace(tzinfo=None)
