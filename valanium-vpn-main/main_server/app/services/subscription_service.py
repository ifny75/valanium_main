from datetime import datetime, timedelta, timezone
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.future import select
from sqlalchemy.orm import selectinload
from app.models import Subscription, User, Plan, Server
from app.services.node_client import NodeClient
from app.services.wireguard_keys import generate_keypair
import logging

logger = logging.getLogger(__name__)

async def accumulate_traffic(sub: Subscription, server: Server) -> bool:
    """Pull the node's raw (since-last-restart) counter and fold its delta
    into the subscription's lifetime total, instead of overwriting it.

    The node's own counter resets to 0 whenever wg0/the peer gets recreated
    (a reboot, a service restart) — mirroring it directly, as earlier code
    did, silently erases a user's history every time that happens. Tracking
    wg_last_raw_bytes lets us add only what changed since we last looked,
    and treat a counter that's now *smaller* than last time as a reset (add
    the whole current value, since it started over from 0).

    Returns True if it actually updated the subscription (caller commits).
    """
    if not sub.wg_peer_id:
        return False
    traffic = await NodeClient.get_traffic(server, sub.wg_peer_id)
    if not traffic:
        return False

    raw_now = traffic["bytes_up"] + traffic["bytes_down"]
    last_raw = sub.wg_last_raw_bytes or 0
    delta = raw_now - last_raw if raw_now >= last_raw else raw_now
    sub.traffic_used_bytes = (sub.traffic_used_bytes or 0) + delta
    sub.wg_last_raw_bytes = raw_now
    return True

async def create_subscription(db: AsyncSession, user_id: int, plan_id: int, server_id: int) -> Subscription:
    # Get relations
    plan_result = await db.execute(
        select(Plan).options(selectinload(Plan.servers)).where(Plan.id == plan_id)
    )
    plan = plan_result.scalar_one_or_none()

    server_result = await db.execute(select(Server).where(Server.id == server_id))
    server = server_result.scalar_one_or_none()

    if not plan or not server:
        raise ValueError("Plan or Server not found")

    # Empty plan.servers means all servers are allowed; otherwise restrict
    if plan.servers and server_id not in {s.id for s in plan.servers}:
        raise ValueError(f"Server '{server.name}' is not available for plan '{plan.name}'")

    wg_private_key, wg_public_key = generate_keypair()

    now = datetime.now(timezone.utc).replace(tzinfo=None)
    expires_at = now + timedelta(days=plan.duration_days)

    new_subscription = Subscription(
        user_id=user_id,
        plan_id=plan_id,
        server_id=server_id,
        wg_public_key=wg_public_key,
        wg_private_key=wg_private_key,
        started_at=now,
        expires_at=expires_at,
        is_active=True,
        traffic_used_bytes=0
    )

    db.add(new_subscription)
    await db.commit()
    await db.refresh(new_subscription)

    # Register the peer with the node — it allocates the tunnel IP and
    # confirms its own public key/endpoint for the client config.
    peer = await NodeClient.add_peer(server, wg_public_key, label=f"sub-{new_subscription.id}")
    if peer:
        new_subscription.wg_peer_id = peer["id"]
        new_subscription.wg_allowed_ip = peer["allowed_ip"]
        new_subscription.wg_server_public_key = peer["server_public_key"]
        new_subscription.wg_endpoint = peer["endpoint"]
        await db.commit()
    else:
        logger.warning(f"Created subscription {new_subscription.id} but failed to push to node {server.name}")

    return new_subscription

async def deactivate_subscription(db: AsyncSession, subscription_id: int):
    result = await db.execute(
        select(Subscription).where(Subscription.id == subscription_id)
    )
    subscription = result.scalar_one_or_none()
    if not subscription:
        return

    subscription.is_active = False
    await db.commit()

    server_result = await db.execute(select(Server).where(Server.id == subscription.server_id))
    server = server_result.scalar_one_or_none()

    if server and subscription.wg_peer_id:
        await NodeClient.remove_peer(server, subscription.wg_peer_id)
        subscription.wg_peer_id = None
        subscription.wg_allowed_ip = None
        await db.commit()

async def check_expired_subscriptions(db: AsyncSession):
    now = datetime.now(timezone.utc).replace(tzinfo=None)
    result = await db.execute(
        select(Subscription).where(
            Subscription.is_active == True,
            Subscription.expires_at < now
        )
    )
    expired_subs = result.scalars().all()

    for sub in expired_subs:
        await deactivate_subscription(db, sub.id)

async def check_traffic_limits(db: AsyncSession):
    # Fetch active subscriptions
    result = await db.execute(select(Subscription).where(Subscription.is_active == True))
    active_subs = result.scalars().all()

    for sub in active_subs:
        if not sub.wg_peer_id:
            continue

        # fetch server and plan
        server_result = await db.execute(select(Server).where(Server.id == sub.server_id))
        server = server_result.scalar_one_or_none()

        plan_result = await db.execute(select(Plan).where(Plan.id == sub.plan_id))
        plan = plan_result.scalar_one_or_none()

        if not server or not plan:
            continue

        if await accumulate_traffic(sub, server):
            await db.commit()

            if plan.traffic_limit_gb > 0:
                limit_bytes = (plan.traffic_limit_gb + (sub.bonus_gb or 0.0)) * (1024 ** 3)
                if sub.traffic_used_bytes > limit_bytes:
                    await deactivate_subscription(db, sub.id)

async def grant_bonus_traffic(db: AsyncSession, subscription_id: int, gb: float) -> Subscription:
    """Add gb (can be negative to revoke) to a subscription's bonus allowance.

    If the subscription had been auto-deactivated for exceeding its old limit and
    hasn't expired yet, the extra GB reactivates it and re-registers its peer.
    """
    if gb == 0:
        raise ValueError("gb must be non-zero")

    result = await db.execute(select(Subscription).options(selectinload(Subscription.plan)).where(Subscription.id == subscription_id))
    sub = result.scalar_one_or_none()
    if not sub:
        raise ValueError("Subscription not found")

    sub.bonus_gb = (sub.bonus_gb or 0.0) + gb
    if sub.bonus_gb < 0:
        sub.bonus_gb = 0.0

    now = datetime.now(timezone.utc).replace(tzinfo=None)
    was_inactive = not sub.is_active
    if was_inactive and sub.expires_at and sub.expires_at > now and sub.plan:
        limit_bytes = (sub.plan.traffic_limit_gb + sub.bonus_gb) * (1024 ** 3) if sub.plan.traffic_limit_gb > 0 else None
        if limit_bytes is None or sub.traffic_used_bytes < limit_bytes:
            sub.is_active = True

    await db.commit()
    await db.refresh(sub)

    if was_inactive and sub.is_active:
        server_result = await db.execute(select(Server).where(Server.id == sub.server_id))
        server = server_result.scalar_one_or_none()
        if server:
            peer = await NodeClient.add_peer(server, sub.wg_public_key, label=f"sub-{sub.id}")
            if peer:
                sub.wg_peer_id = peer["id"]
                sub.wg_allowed_ip = peer["allowed_ip"]
                sub.wg_server_public_key = peer["server_public_key"]
                sub.wg_endpoint = peer["endpoint"]
                await db.commit()

    return sub

async def sync_subscription_to_node(db: AsyncSession, subscription_id: int):
    result = await db.execute(select(Subscription).where(Subscription.id == subscription_id))
    sub = result.scalar_one_or_none()

    if not sub or not sub.is_active:
        return

    server_result = await db.execute(select(Server).where(Server.id == sub.server_id))
    server = server_result.scalar_one_or_none()

    if server:
        peer = await NodeClient.add_peer(server, sub.wg_public_key, label=f"sub-{sub.id}")
        if peer:
            sub.wg_peer_id = peer["id"]
            sub.wg_allowed_ip = peer["allowed_ip"]
            sub.wg_server_public_key = peer["server_public_key"]
            sub.wg_endpoint = peer["endpoint"]
            await db.commit()
