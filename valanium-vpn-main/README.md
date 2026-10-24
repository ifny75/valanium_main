# ObsyxGen VPN — WireGuard Management Panel

VPN-панель управления WireGuard-серверами с админ-панелью, подписками, лимитами трафика и контролем сроков.

## Архитектура

```
┌──────────────────────────────┐     ┌─────────────────────────┐
│        Main Server           │     │    VPN Node 1            │
│  FastAPI + SQLAlchemy + UI   │────▶│  Node Agent (FastAPI)    │
│  - Админ-панель (Bootstrap)  │     │  WireGuard (wg0)         │
│  - REST API                  │     └─────────────────────────┘
│  - Scheduler                 │     ┌─────────────────────────┐
│  - SQLite / PostgreSQL       │────▶│    VPN Node N            │
└──────────────────────────────┘     │  Node Agent (FastAPI)    │
                                     │  WireGuard (wg0)         │
                                     └─────────────────────────┘
```

Живой пример: `valanium-hop1` (2.27.205.9) — main_server на порту 8080,
node agent на 8000 (localhost-only), WireGuard на 51820/udp.

## Быстрый старт

### Main Server

```bash
cd main_server
pip install -r requirements.txt
python scripts/vendor_assets.py  # one-time: downloads Bootstrap/Icons into static/vendor
                                  # so the admin panel doesn't hit cdn.jsdelivr.net on every load
uvicorn app.main:app --host 0.0.0.0 --port 8080 --reload
```

Админка доступна по: `http://localhost:8080/login`
- Логин/пароль задаются в `.env` (см. ниже) — не оставляйте дефолтный `admin`/`admin` на сервере, смотрящем в интернет.

### VPN Node

На каждом VPN-сервере:

```bash
# 1. Установка WireGuard + агента
cd vpn_node
chmod +x setup_node.sh
export API_KEY="shared-secret-with-main-server"
export SERVER_ENDPOINT_HOST="1.2.3.4"   # этот сервер, публичный IP
sudo -E ./setup_node.sh   # -E обязателен, иначе PATH под sudo не увидит /usr/sbin/ip
```

Агент сам сгенерирует ключ сервера и поднимет `wg0` при первом старте
(`/etc/wireguard/wg0.conf` — управляется агентом, руками не редактировать;
пиры добавляются/удаляются через `wg syncconf`, без разрыва существующих
соединений).

## Конфигурация

### Main Server (.env)

```env
DATABASE_URL=sqlite+aiosqlite:///./obsyxgen.db
SECRET_KEY=your-secret-key-here
ADMIN_USERNAME=admin
ADMIN_PASSWORD=your-secure-password
ACCESS_TOKEN_EXPIRE_MINUTES=60
```

### VPN Node

Настраивается переменными окружения в systemd-юните
(`/etc/systemd/system/vpn_node.service`, генерируется `setup_node.sh`):

```env
API_KEY=shared-secret-with-main-server
SERVER_ENDPOINT_HOST=1.2.3.4
WG_LISTEN_PORT=51820
EGRESS_INTERFACE=eth0
```

## Вход по коду доступа (`/v1`)

Клиентский API. Админских учётных данных для него не нужно и не бывает —
см. `ARCHITECTURE.md` §5.

Владелец выпускает код в панели (`POST /api/accounts/`, показывается один
раз):

```
VLNM7-3KQXA-9PW2H-TRF4M
```

Клиент тратит его один раз, чтобы привязать устройство, и дальше входит
подписью:

| Метод | Путь | Что делает |
| --- | --- | --- |
| POST | `/v1/hello` | выдаёт nonce на 30 секунд |
| POST | `/v1/devices` | привязать устройство: `{code, device_pub, sig, nonce}` |
| POST | `/v1/auth` | вход привязанного устройства: `{device_pub, sig, nonce}` |
| GET | `/v1/account` | срок, тариф, трафик, сколько устройств |
| GET | `/v1/devices` | свои устройства |
| POST | `/v1/devices/revoke` | отозвать своё или соседнее, свежей подписью |
| GET | `/v1/servers` | каталог узлов (без адресов и ключей агентов) |
| POST | `/v1/peer` | зарегистрировать **публичный** ключ WireGuard на узле |
| DELETE | `/v1/peer/{server_id}` | снять свой пир |

Приватный ключ WireGuard генерирует клиент; сервер его не запрашивает и
хранить не умеет. На каждый узел — своя пара: один ключ на все узлы позволил
бы двум операторам сшить по нему две сессии одного человека.

Тесты:

```bash
cd main_server
pip install -r requirements-dev.txt
python -m pytest tests -q
```

### Обновление боевого узла

Юнит — `main_server/deploy/main_server.service`. Порядок, который стоит
соблюдать: сначала копия, потом файлы, и только потом перезапуск.

```bash
ts=$(date +%Y%m%d-%H%M%S)
mkdir -p /root/main_server-backups/$ts
cp -a /opt/main_server/obsyxgen.db /opt/main_server/app /opt/main_server/.env /root/main_server-backups/$ts/

cd /opt/main_server && tar -xzf /tmp/deploy.tar.gz
find app -name __pycache__ -type d -exec rm -rf {} +
venv/bin/python -c "import sys; sys.path.insert(0,'.'); import app.main"   # импорт до рестарта
systemctl restart main_server.service
curl -s -X POST http://127.0.0.1:62740/v1/hello                            # живой?
```

Недостающие колонки и таблицы досоздаются на старте
(`database.py: _run_lightweight_migrations`); Alembic в проекте нет.
Перец для кодов доступа создаётся сам при первом запуске и лежит рядом с
базой — **его потеря обесценивает все выданные коды**, поэтому он должен
попадать в те же копии, что и база.

## Функционал

- **Юзеры**: CRUD, активация/блокировка
- **Тарифные планы**: срок (дни), лимит трафика (ГБ), кол-во устройств, цена
- **Подписки**: привязка юзера к плану и серверу, авто-генерация WireGuard-ключей и готового клиентского конфига
- **Серверы**: добавление VPN-нод, проверка здоровья, вкл/выкл
- **Автоматика**:
  - Проверка сроков подписок каждые 5 минут
  - Проверка лимитов трафика каждые 15 минут
  - Автоблокировка при превышении

## Клиент WireGuard

После создания подписки `GET /api/subscriptions/{id}` (или карточка в
админке) отдаёт готовый `wg_config` — просто сохранить как `.conf` и
импортировать в клиент WireGuard:

```ini
[Interface]
PrivateKey = <из wg_config>
Address = 10.10.0.2/32
DNS = 1.1.1.1, 1.0.0.1

[Peer]
PublicKey = <публичный ключ сервера>
Endpoint = 1.2.3.4:51820
AllowedIPs = 0.0.0.0/0, ::/0
PersistentKeepalive = 25
```

## Стек

- **Backend**: Python, FastAPI, SQLAlchemy (async), APScheduler
- **Frontend**: Jinja2, Bootstrap 5, HTMX
- **VPN**: WireGuard (`wg`/`wg-quick`, управляется через `wg syncconf`)
- **БД**: SQLite (по умолчанию) / PostgreSQL
