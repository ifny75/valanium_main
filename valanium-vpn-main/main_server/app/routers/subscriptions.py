from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select
from sqlalchemy.orm import selectinload
from typing import List

from app.database import get_db
from app.models import Subscription
from app.schemas import SubscriptionCreate, SubscriptionOut, BonusGrant
from app.auth import verify_admin
from app.services import subscription_service

router = APIRouter(prefix="/api/subscriptions", tags=["subscriptions"])

def _effective_limit_gb(sub: Subscription) -> float:
    """Plan traffic limit plus any admin-granted bonus GB. 0 (plan unlimited) stays 0."""
    plan_limit = sub.plan.traffic_limit_gb if sub.plan else 0.0
    if plan_limit <= 0:
        return 0.0
    return plan_limit + (sub.bonus_gb or 0.0)

def _wg_config(sub: Subscription) -> str:
    """Ready-to-import client config, or "" if the peer hasn't been
    registered with its node yet (e.g. the node was unreachable at
    creation time — use PUT /{id}/sync once it's back)."""
    if not sub.wg_server_public_key or not sub.wg_endpoint or not sub.wg_allowed_ip:
        return ""
    return (
        "[Interface]\n"
        f"PrivateKey = {sub.wg_private_key}\n"
        f"Address = {sub.wg_allowed_ip}/32\n"
        "DNS = 1.1.1.1, 1.0.0.1\n\n"
        "[Peer]\n"
        f"PublicKey = {sub.wg_server_public_key}\n"
        f"Endpoint = {sub.wg_endpoint}\n"
        "AllowedIPs = 0.0.0.0/0, ::/0\n"
        "PersistentKeepalive = 25\n"
    )

def _to_out(sub: Subscription) -> SubscriptionOut:
    traffic_used_gb = sub.traffic_used_bytes / (1024**3) if sub.traffic_used_bytes else 0.0
    return SubscriptionOut(
        id=sub.id,
        user_id=sub.user_id,
        plan_id=sub.plan_id,
        server_id=sub.server_id,
        wg_public_key=sub.wg_public_key,
        wg_allowed_ip=sub.wg_allowed_ip,
        started_at=sub.started_at,
        expires_at=sub.expires_at,
        is_active=sub.is_active,
        traffic_used_bytes=sub.traffic_used_bytes,
        traffic_used_gb=traffic_used_gb,
        traffic_limit_gb=_effective_limit_gb(sub),
        bonus_gb=sub.bonus_gb or 0.0,
        wg_config=_wg_config(sub),
    )

_WITH_RELATIONS = (
    selectinload(Subscription.user),
    selectinload(Subscription.plan),
    selectinload(Subscription.server),
)

@router.get("/", response_model=List[SubscriptionOut])
async def list_subscriptions(db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """List all subscriptions with joined user, plan, server info."""
    result = await db.execute(
        select(Subscription).options(*_WITH_RELATIONS).order_by(Subscription.id.desc())
    )
    return [_to_out(sub) for sub in result.scalars().all()]

@router.post("/", response_model=SubscriptionOut)
async def create_subscription(sub_create: SubscriptionCreate, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Create a subscription using subscription_service."""
    try:
        sub = await subscription_service.create_subscription(db, sub_create.user_id, sub_create.plan_id, sub_create.server_id)
        result = await db.execute(
            select(Subscription).options(*_WITH_RELATIONS).where(Subscription.id == sub.id)
        )
        return _to_out(result.scalars().first())
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@router.get("/{sub_id}", response_model=SubscriptionOut)
async def get_subscription(sub_id: int, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Get one subscription by ID."""
    result = await db.execute(
        select(Subscription).options(*_WITH_RELATIONS).where(Subscription.id == sub_id)
    )
    sub = result.scalars().first()
    if not sub:
        raise HTTPException(status_code=404, detail="Subscription not found")
    return _to_out(sub)

@router.put("/{sub_id}/bonus", response_model=SubscriptionOut)
async def grant_bonus_traffic(sub_id: int, grant: BonusGrant, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Grant extra free GB on top of the subscription's plan limit. Reactivates the
    subscription if it was auto-blocked for exceeding its (old) limit and hasn't expired."""
    try:
        sub = await subscription_service.grant_bonus_traffic(db, sub_id, grant.gb)
        result = await db.execute(
            select(Subscription).options(*_WITH_RELATIONS).where(Subscription.id == sub.id)
        )
        return _to_out(result.scalars().first())
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))

@router.put("/{sub_id}/deactivate")
async def deactivate_subscription(sub_id: int, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Deactivate a subscription."""
    try:
        await subscription_service.deactivate_subscription(db, sub_id)
        return {"message": "Subscription deactivated successfully"}
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@router.put("/{sub_id}/sync")
async def sync_subscription(sub_id: int, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Re-sync subscription to node."""
    try:
        await subscription_service.sync_subscription_to_node(db, sub_id)
        return {"message": "Subscription synced successfully"}
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))
