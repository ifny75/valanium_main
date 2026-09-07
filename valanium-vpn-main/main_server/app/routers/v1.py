"""Публичный API клиента: вход по коду доступа и подписи устройства.

Смонтирован **не** под `PANEL_PATH_PREFIX` (см. `main.py`): путь админки —
секрет, путь клиента — контракт, и прятать его бессмысленно, туда ходят все
установленные клиенты.

Что здесь принципиально:

- админских учётных данных у клиента нет вообще. Раньше он логинился в
  панель под `admin` с паролем, зашитым в бинарник (`main_server_client.rs`);
- код доступа тратится один раз, на привязку устройства. Дальше — только
  подпись по свежему nonce;
- приватный ключ WireGuard сервер не видит: устройство присылает публичный;
- ни адреса, ни имени, ни версии клиента здесь не сохраняется (§7).
"""

import logging
from datetime import datetime, timedelta, timezone

from fastapi import APIRouter, Depends, HTTPException, Request
from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.auth import create_device_token, device_pub_from_token
from app.config import settings
from app.database import get_db
from app.models import Account, Device, Plan, Server, Subscription
from app.schemas import (
    AccountOut,
    DeviceOut,
    EnrollRequest,
    HelloOut,
    PeerOut,
    PeerRequest,
    ServerBrief,
    SignedRequest,
    TokenOut,
)
from app.services import access_code, ed25519
from app.services.node_client import NodeClient
from app.services.nonce import NonceStore, RateLimiter

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/v1", tags=["client"])

_nonces = NonceStore(ttl_sec=30)
_pepper = access_code.load_or_create_pepper(settings.ACCESS_CODE_PEPPER_FILE)

# Частота — единственное, для чего здесь нужен адрес клиента. Он живёт в
# памяти ограничителя и не попадает ни в базу, ни в журнал.
_enroll_limit = RateLimiter(limit=10, window_sec=3600)
_auth_limit = RateLimiter(limit=60, window_sec=60)


def _client_key(request: Request) -> str:
    return request.client.host if request.client else "unknown"


def _now() -> datetime:
    return datetime.now(timezone.utc).replace(tzinfo=None)


def _unhex(value: str, length: int) -> bytes:
    try:
        raw = bytes.fromhex(value)
    except ValueError:
        raise HTTPException(status_code=400, detail="malformed hex")
    if len(raw) != length:
        raise HTTPException(status_code=400, detail="wrong length")
    return raw


async def _account_out(db: AsyncSession, account: Account) -> AccountOut:
    # Только явная загрузка: обращение к account.plan подтянуло бы связь
    # лениво, а ленивая загрузка в async-сессии — это MissingGreenlet на
    # ровном месте, в зависимости от того, каким запросом сюда пришли.
    plan = await db.get(Plan, account.plan_id)
    used = await db.scalar(
        select(func.coalesce(func.sum(Subscription.traffic_used_bytes), 0)).where(
            Subscription.account_id == account.id
        )
    )
    devices = await db.scalar(
        select(func.count(Device.id)).where(
            Device.account_id == account.id, Device.revoked_at.is_(None)
        )
    )
    return AccountOut(
        expires_at=account.expires_at,
        is_active=account.is_active and account.expires_at > _now(),
        plan_name=plan.name if plan else "",
        traffic_limit_gb=plan.traffic_limit_gb if plan else 0.0,
        traffic_used_gb=round((used or 0) / (1024 ** 3), 3),
        max_devices=plan.max_devices if plan else 0,
        devices_used=devices or 0,
    )


async def verify_device(request: Request, db: AsyncSession = Depends(get_db)) -> Device:
    """Bearer-жетон, выданный `/v1/auth`. Отозванное устройство не проходит."""
    header = request.headers.get("Authorization", "")
    if not header.startswith("Bearer "):
        raise HTTPException(status_code=401, detail="not authenticated")
    device_pub = device_pub_from_token(header[7:])
    if not device_pub:
        raise HTTPException(status_code=401, detail="invalid token")

    result = await db.execute(
        select(Device).options(selectinload(Device.account)).where(Device.device_pub == device_pub)
    )
    device = result.scalar_one_or_none()
    if not device or device.revoked_at is not None:
        raise HTTPException(status_code=401, detail="device revoked")
    if not device.account.is_active or device.account.expires_at <= _now():
        raise HTTPException(status_code=403, detail="account inactive")
    return device


# --------------------------------------------------------------------------
# вход
# --------------------------------------------------------------------------


@router.post("/hello", response_model=HelloOut)
async def hello() -> HelloOut:
    """Свежий challenge. Ничего не знает о том, кто спросил."""
    nonce = _nonces.issue()
    if nonce is None:
        raise HTTPException(status_code=503, detail="busy, retry")
    return HelloOut(nonce=nonce.hex(), server_time=_now())


@router.post("/devices", response_model=TokenOut)
async def enroll(req: EnrollRequest, request: Request, db: AsyncSession = Depends(get_db)) -> TokenOut:
    """Потратить код доступа и привязать устройство.

    Единственное место, где код вообще появляется на проводе. Дальше
    устройство живёт своей подписью, а код можно забыть — он понадобится
    только чтобы привязать следующее устройство.
    """
    if not _enroll_limit.allow(_client_key(request)):
        raise HTTPException(status_code=429, detail="too many attempts")

    nonce = _unhex(req.nonce, 32)
    device_pub = _unhex(req.device_pub, 32)
    signature = _unhex(req.sig, 64)

    if not _nonces.consume(nonce):
        raise HTTPException(status_code=400, detail="stale nonce")
    if not ed25519.verify(signature, ed25519.enroll_message(nonce, device_pub), device_pub):
        raise HTTPException(status_code=401, detail="bad signature")

    canonical = access_code.normalize(req.code)
    if canonical is None:
        # Опечатку от несуществующего кода отличаем намеренно: контрольная
        # сумма для того и добавлена, чтобы человек перечитал свою бумажку,
        # а не пошёл выяснять, жив ли его доступ.
        raise HTTPException(status_code=400, detail="malformed code")

    code_hash = access_code.hash_code(canonical, _pepper)
    result = await db.execute(
        select(Account).options(selectinload(Account.plan)).where(Account.code_hash == code_hash)
    )
    account = result.scalar_one_or_none()
    if not account or not account.is_active or account.expires_at <= _now():
        raise HTTPException(status_code=403, detail="code not valid")

    existing = await db.execute(select(Device).where(Device.device_pub == device_pub.hex()))
    device = existing.scalar_one_or_none()
    if device is not None:
        if device.account_id != account.id:
            raise HTTPException(status_code=409, detail="device already enrolled")
        # Повтор привязки того же ключа тем же кодом — не ошибка: так
        # выглядит клиент, потерявший жетон, но сохранивший ключ.
        device.revoked_at = None
    else:
        active = await db.scalar(
            select(func.count(Device.id)).where(
                Device.account_id == account.id, Device.revoked_at.is_(None)
            )
        )
        plan = await db.get(Plan, account.plan_id)
        limit = plan.max_devices if plan else 1
        if (active or 0) >= limit:
            raise HTTPException(status_code=409, detail="device limit reached")
        device = Device(account_id=account.id, device_pub=device_pub.hex())
        db.add(device)

    await db.commit()
    await db.refresh(device)

    return TokenOut(
        token=create_device_token(device.device_pub),
        expires_in=settings.DEVICE_TOKEN_EXPIRE_MINUTES * 60,
        account=await _account_out(db, account),
    )


@router.post("/auth", response_model=TokenOut)
async def auth(req: SignedRequest, request: Request, db: AsyncSession = Depends(get_db)) -> TokenOut:
    """Вход уже привязанного устройства. Кода здесь нет."""
    if not _auth_limit.allow(_client_key(request)):
        raise HTTPException(status_code=429, detail="too many attempts")

    nonce = _unhex(req.nonce, 32)
    device_pub = _unhex(req.device_pub, 32)
    signature = _unhex(req.sig, 64)

    if not _nonces.consume(nonce):
        raise HTTPException(status_code=400, detail="stale nonce")
    if not ed25519.verify(signature, ed25519.auth_message(nonce, device_pub), device_pub):
        raise HTTPException(status_code=401, detail="bad signature")

    result = await db.execute(
        select(Device).options(selectinload(Device.account).selectinload(Account.plan))
        .where(Device.device_pub == device_pub.hex())
    )
    device = result.scalar_one_or_none()
    if not device or device.revoked_at is not None:
        raise HTTPException(status_code=401, detail="device revoked")
    if not device.account.is_active or device.account.expires_at <= _now():
        raise HTTPException(status_code=403, detail="account inactive")

    return TokenOut(
        token=create_device_token(device.device_pub),
        expires_in=settings.DEVICE_TOKEN_EXPIRE_MINUTES * 60,
        account=await _account_out(db, device.account),
    )


# --------------------------------------------------------------------------
# состояние аккаунта
# --------------------------------------------------------------------------


@router.get("/account", response_model=AccountOut)
async def account_state(device: Device = Depends(verify_device), db: AsyncSession = Depends(get_db)) -> AccountOut:
    return await _account_out(db, device.account)


@router.get("/devices", response_model=list[DeviceOut])
async def list_devices(device: Device = Depends(verify_device), db: AsyncSession = Depends(get_db)) -> list[DeviceOut]:
    result = await db.execute(
        select(Device).where(Device.account_id == device.account_id, Device.revoked_at.is_(None))
        .order_by(Device.created_at)
    )
    return [
        DeviceOut(
            device_pub=d.device_pub,
            created_at=d.created_at,
            is_current=d.id == device.id,
        )
        for d in result.scalars().all()
    ]


@router.post("/devices/revoke")
async def revoke_device(
    req: SignedRequest,
    device: Device = Depends(verify_device),
    db: AsyncSession = Depends(get_db),
) -> dict:
    """Отвязать устройство аккаунта — своё или соседнее.

    Жетона мало: он живёт час и мог утечь из памяти процесса, а отзыв
    выкидывает человека из сервиса. Поэтому нужна свежая подпись ключом,
    который никуда из устройства не уезжает (`revoke_message`).
    """
    nonce = _unhex(req.nonce, 32)
    target = _unhex(req.device_pub, 32)
    signature = _unhex(req.sig, 64)

    if not _nonces.consume(nonce):
        raise HTTPException(status_code=400, detail="stale nonce")
    own_pub = bytes.fromhex(device.device_pub)
    if not ed25519.verify(signature, ed25519.revoke_message(nonce, target), own_pub):
        raise HTTPException(status_code=401, detail="bad signature")

    result = await db.execute(
        select(Device).where(
            Device.device_pub == target.hex(), Device.account_id == device.account_id
        )
    )
    victim = result.scalar_one_or_none()
    if not victim or victim.revoked_at is not None:
        raise HTTPException(status_code=404, detail="no such device")

    victim.revoked_at = _now()

    # Пиры снимаются немедленно: отозванное устройство не должно доходить
    # до узла ни секунды дольше, чем нужно на запрос.
    subs = await db.execute(select(Subscription).where(Subscription.device_id == victim.id))
    for sub in subs.scalars().all():
        sub.is_active = False
        if sub.wg_peer_id:
            server = await db.get(Server, sub.server_id)
            if server:
                await NodeClient.remove_peer(server, sub.wg_peer_id)
            sub.wg_peer_id = None
            sub.wg_allowed_ip = None

    await db.commit()
    return {"status": "revoked"}


# --------------------------------------------------------------------------
# узлы и тоннель
# --------------------------------------------------------------------------


@router.get("/servers", response_model=list[ServerBrief])
async def list_servers(
    device: Device = Depends(verify_device), db: AsyncSession = Depends(get_db)
) -> list[ServerBrief]:
    """Каталог узлов. Ключа агента здесь нет — он не дело клиента."""
    result = await db.execute(select(Server).where(Server.is_active == True))  # noqa: E712
    return [
        ServerBrief(
            id=s.id,
            name=s.name,
            country_name=s.country_name,
            country_code=s.country_code,
            map_x=s.map_x,
            map_y=s.map_y,
        )
        for s in result.scalars().all()
    ]


@router.post("/peer", response_model=PeerOut)
async def register_peer(
    req: PeerRequest,
    device: Device = Depends(verify_device),
    db: AsyncSession = Depends(get_db),
) -> PeerOut:
    """Зарегистрировать публичный ключ устройства на выбранном узле.

    Приватный ключ остаётся на устройстве — сервер его не запрашивает, не
    получает и хранить не умеет. В ответе всё, чего клиенту не хватает для
    сборки собственного конфига.
    """
    wg_pub = req.wg_public_key.strip()
    try:
        # WireGuard-ключ — 32 байта в base64, ровно 44 знака с '=' на конце.
        import base64

        if len(wg_pub) != 44 or len(base64.b64decode(wg_pub, validate=True)) != 32:
            raise ValueError
    except Exception:
        raise HTTPException(status_code=400, detail="malformed WireGuard key")

    server = await db.get(Server, req.server_id)
    if not server or not server.is_active:
        raise HTTPException(status_code=404, detail="no such server")

    account = device.account
    plan = await db.get(Plan, account.plan_id)

    result = await db.execute(
        select(Subscription).where(
            Subscription.device_id == device.id, Subscription.server_id == server.id
        )
    )
    sub = result.scalar_one_or_none()

    # На каждый узел — своя пара ключей. Это не прихоть схемы: один ключ на
    # все узлы означает, что операторы двух узлов видят одно и то же
    # публичное значение и сшивают по нему две сессии одного человека.
    # Уникальность колонки это заодно и стережёт, но упасть она должна
    # понятной ошибкой, а не 500 из глубины ORM.
    taken = await db.execute(
        select(Subscription.id).where(
            Subscription.wg_public_key == wg_pub,
            Subscription.id != (sub.id if sub else -1),
        )
    )
    if taken.first() is not None:
        raise HTTPException(status_code=409, detail="that WireGuard key is already in use")

    if sub is None:
        sub = Subscription(
            account_id=account.id,
            device_id=device.id,
            plan_id=account.plan_id,
            server_id=server.id,
            wg_public_key=wg_pub,
            # Пустая строка, а не NULL: в уже существующих базах колонка
            # объявлена NOT NULL, а перестроить таблицу ради поля, которое
            # подлежит удалению (ARCHITECTURE.md, фаза 1), — лишний риск.
            wg_private_key="",
            expires_at=account.expires_at,
            is_active=True,
        )
        db.add(sub)
    else:
        # Ключ сменился — старый пир снимаем с узла до того, как заводить
        # новый. Иначе он остаётся жить: тоннель по выброшенному ключу
        # продолжает работать, а адрес из пула не возвращается.
        if sub.wg_peer_id and sub.wg_public_key != wg_pub:
            await NodeClient.remove_peer(server, sub.wg_peer_id)
            sub.wg_peer_id = None
            sub.wg_allowed_ip = None
        sub.wg_public_key = wg_pub
        sub.expires_at = account.expires_at
        sub.is_active = True

    if plan and plan.traffic_limit_gb > 0:
        limit = (plan.traffic_limit_gb + (sub.bonus_gb or 0.0)) * (1024 ** 3)
        if (sub.traffic_used_bytes or 0) > limit:
            raise HTTPException(status_code=402, detail="traffic limit reached")

    await db.commit()
    await db.refresh(sub)

    peer = await NodeClient.add_peer(server, wg_pub, label=f"dev-{device.id}")
    if not peer:
        # Ключ мог остаться на узле с прошлой попытки: снимаем и пробуем ещё
        # раз, иначе устройство залипнет на «уже зарегистрирован» навсегда.
        if sub.wg_peer_id:
            await NodeClient.remove_peer(server, sub.wg_peer_id)
            peer = await NodeClient.add_peer(server, wg_pub, label=f"dev-{device.id}")
    if not peer:
        raise HTTPException(status_code=502, detail="node did not accept the peer")

    sub.wg_peer_id = peer["id"]
    sub.wg_allowed_ip = peer["allowed_ip"]
    sub.wg_server_public_key = peer["server_public_key"]
    sub.wg_endpoint = peer["endpoint"]
    await db.commit()

    return PeerOut(
        address=f"{peer['allowed_ip']}/32",
        server_public_key=peer["server_public_key"],
        endpoint=peer["endpoint"],
        dns=settings.TUNNEL_DNS or peer["allowed_ip"].rsplit(".", 1)[0] + ".1",
        allowed_ips="0.0.0.0/0, ::/0",
        persistent_keepalive=25,
    )


@router.delete("/peer/{server_id}")
async def drop_peer(
    server_id: int,
    device: Device = Depends(verify_device),
    db: AsyncSession = Depends(get_db),
) -> dict:
    """Снять свой пир с узла — например, при выходе из аккаунта."""
    result = await db.execute(
        select(Subscription).where(
            Subscription.device_id == device.id, Subscription.server_id == server_id
        )
    )
    sub = result.scalar_one_or_none()
    if not sub:
        return {"status": "ok"}

    if sub.wg_peer_id:
        server = await db.get(Server, server_id)
        if server:
            await NodeClient.remove_peer(server, sub.wg_peer_id)
    sub.wg_peer_id = None
    sub.wg_allowed_ip = None
    sub.is_active = False
    await db.commit()
    return {"status": "ok"}
