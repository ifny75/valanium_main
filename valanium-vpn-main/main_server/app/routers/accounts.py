"""Выпуск кодов доступа. Только для владельца, за админским жетоном.

Код показывается один раз — в ответе на выпуск. Повторно его взять неоткуда:
в базе лежит scrypt-хеш, обратного преобразования нет и не предполагается.
Потерянный код — это выпуск нового и отзыв старого, а не «посмотрите в
панели».
"""

from datetime import timedelta

from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession

from app.auth import verify_admin
from app.config import settings
from app.database import get_db
from app.models import Account, Device, Plan, Server, Subscription
from app.schemas import AccountAdminOut, AccountCreate, AccountCreated
from app.services import access_code
from app.services.node_client import NodeClient
from app.services.v1_common import now_naive

router = APIRouter(prefix="/api/accounts", tags=["accounts"])

_pepper = access_code.load_or_create_pepper(settings.ACCESS_CODE_PEPPER_FILE)


@router.post("/", response_model=AccountCreated)
async def create_account(
    payload: AccountCreate,
    db: AsyncSession = Depends(get_db),
    admin: str = Depends(verify_admin),
) -> AccountCreated:
    plan = await db.get(Plan, payload.plan_id)
    if not plan or not plan.is_active:
        raise HTTPException(status_code=404, detail="no such plan")

    days = payload.days if payload.days and payload.days > 0 else plan.duration_days
    code = access_code.generate()
    account = Account(
        code_hash=access_code.hash_code(code, _pepper),
        plan_id=plan.id,
        expires_at=now_naive() + timedelta(days=days),
        note=payload.note,
        is_active=True,
    )
    db.add(account)
    await db.commit()
    await db.refresh(account)
    return AccountCreated(id=account.id, code=code, expires_at=account.expires_at)


@router.get("/", response_model=list[AccountAdminOut])
async def list_accounts(
    db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)
) -> list[AccountAdminOut]:
    result = await db.execute(select(Account).order_by(Account.created_at.desc()))
    accounts = result.scalars().all()

    out = []
    for a in accounts:
        devices = await db.scalar(
            select(func.count(Device.id)).where(
                Device.account_id == a.id, Device.revoked_at.is_(None)
            )
        )
        used = await db.scalar(
            select(func.coalesce(func.sum(Subscription.traffic_used_bytes), 0)).where(
                Subscription.account_id == a.id
            )
        )
        out.append(
            AccountAdminOut(
                id=a.id,
                plan_id=a.plan_id,
                created_at=a.created_at,
                expires_at=a.expires_at,
                is_active=a.is_active,
                note=a.note,
                devices_used=devices or 0,
                traffic_used_gb=round((used or 0) / (1024 ** 3), 3),
            )
        )
    return out


@router.put("/{account_id}/toggle")
async def toggle_account(
    account_id: int, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)
) -> dict:
    """Включить или выключить аккаунт.

    Выключение снимает пиры со всех узлов немедленно: иначе «заблокирован» в
    панели означает лишь «не сможет войти в следующий раз», а тоннель у него
    продолжает работать.
    """
    account = await db.get(Account, account_id)
    if not account:
        raise HTTPException(status_code=404, detail="no such account")

    account.is_active = not account.is_active
    if not account.is_active:
        await _drop_all_peers(db, account_id)

    await db.commit()
    return {"id": account.id, "is_active": account.is_active}


@router.delete("/{account_id}")
async def delete_account(
    account_id: int, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)
) -> dict:
    account = await db.get(Account, account_id)
    if not account:
        raise HTTPException(status_code=404, detail="no such account")

    await _drop_all_peers(db, account_id)
    result = await db.execute(select(Subscription).where(Subscription.account_id == account_id))
    for sub in result.scalars().all():
        await db.delete(sub)
    await db.delete(account)   # устройства уходят каскадом
    await db.commit()
    return {"status": "deleted"}


async def _drop_all_peers(db: AsyncSession, account_id: int) -> None:
    result = await db.execute(select(Subscription).where(Subscription.account_id == account_id))
    for sub in result.scalars().all():
        sub.is_active = False
        if sub.wg_peer_id:
            server = await db.get(Server, sub.server_id)
            if server:
                await NodeClient.remove_peer(server, sub.wg_peer_id)
            sub.wg_peer_id = None
            sub.wg_allowed_ip = None
