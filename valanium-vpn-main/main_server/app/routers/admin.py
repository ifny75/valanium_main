import os
from fastapi import APIRouter, Depends, Form, Request, Response
from fastapi.responses import HTMLResponse, RedirectResponse
from fastapi.templating import Jinja2Templates
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select, func
from sqlalchemy.orm import selectinload

from app.database import get_db
from app.models import User, Plan, Server, Subscription, Account, Device
from app.config import settings
from app.services.subscription_service import accumulate_traffic
from app.auth import (
    verify_admin,
    authenticate_admin,
    create_access_token,
    is_login_locked,
    register_failed_login,
    clear_failed_logins,
)

router = APIRouter()
templates = Jinja2Templates(directory=os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(__file__))), 'templates'))
# So templates can build absolute links/fetch URLs that still work once
# every route is mounted under PANEL_PATH_PREFIX (see main.py/config.py).
templates.env.globals['url_prefix'] = settings.PANEL_PATH_PREFIX


@router.get("/login", response_class=HTMLResponse)
async def login_page(request: Request):
    """Render login page."""
    return templates.TemplateResponse(request, "login.html")


@router.post("/login")
async def login(request: Request, response: Response, username: str = Form(...), password: str = Form(...)):
    """Authenticate admin and set cookie."""
    client_ip = request.client.host if request.client else "unknown"

    if is_login_locked(client_ip):
        return templates.TemplateResponse(
            request, "login.html",
            {"error": "Too many failed attempts. Try again in a few minutes."},
            status_code=429,
        )

    user = await authenticate_admin(username, password)
    if not user:
        register_failed_login(client_ip)
        return templates.TemplateResponse(request, "login.html", {"error": "Invalid credentials"})

    clear_failed_logins(client_ip)
    access_token = create_access_token(data={"sub": username})
    resp = RedirectResponse(url=f"{settings.PANEL_PATH_PREFIX}/admin/dashboard", status_code=303)
    resp.set_cookie(
        key="access_token",
        value=f"Bearer {access_token}",
        httponly=True,
        secure=settings.COOKIE_SECURE,
        samesite="lax",
    )
    return resp


@router.get("/logout")
async def logout(request: Request, response: Response):
    """Clear cookie and redirect to login."""
    resp = RedirectResponse(url=f"{settings.PANEL_PATH_PREFIX}/login", status_code=303)
    resp.delete_cookie(key="access_token")
    return resp


@router.get("/admin/dashboard", response_class=HTMLResponse)
async def dashboard_page(request: Request, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Render dashboard page with stats, recent subscriptions and server status."""
    users_count = await db.scalar(select(func.count()).select_from(User))
    servers_count = await db.scalar(select(func.count()).select_from(Server))
    active_subs_count = await db.scalar(select(func.count()).select_from(Subscription).where(Subscription.is_active == True))

    total_bytes = await db.scalar(select(func.sum(Subscription.traffic_used_bytes))) or 0
    total_traffic_gb = round(total_bytes / (1024**3), 2)

    stats = {
        "total_users": users_count,
        "active_subscriptions": active_subs_count,
        "total_servers": servers_count,
        "total_traffic_gb": total_traffic_gb,
    }

    # Last few subscriptions for a quick activity glance
    recent_result = await db.execute(
        select(Subscription).options(
            selectinload(Subscription.user),
            selectinload(Subscription.plan),
            selectinload(Subscription.server)
        ).order_by(Subscription.id.desc()).limit(8)
    )
    recent_subscriptions = recent_result.scalars().all()

    # Servers with their count of active subscribers
    servers_result = await db.execute(select(Server).order_by(Server.name))
    servers = servers_result.scalars().all()
    counts_result = await db.execute(
        select(Subscription.server_id, func.count())
        .where(Subscription.is_active == True)
        .group_by(Subscription.server_id)
    )
    active_counts = {row[0]: row[1] for row in counts_result.all()}
    servers_status = [
        {"server": s, "active_count": active_counts.get(s.id, 0)}
        for s in servers
    ]

    return templates.TemplateResponse(request, "dashboard.html", {
        "stats": stats,
        "recent_subscriptions": recent_subscriptions,
        "servers_status": servers_status,
    })


@router.get("/admin/users", response_class=HTMLResponse)
async def users_page(request: Request, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Render users page with each user's active subscription/traffic and quick-action data."""
    result = await db.execute(select(User).order_by(User.created_at.desc()))
    users = result.scalars().all()

    subs_result = await db.execute(
        select(Subscription).options(selectinload(Subscription.plan))
        .where(Subscription.is_active == True)
        .order_by(Subscription.id.desc())
    )
    # Most recent active subscription per user
    active_sub_by_user = {}
    for s in subs_result.scalars().all():
        active_sub_by_user.setdefault(s.user_id, s)

    plans_result = await db.execute(select(Plan).where(Plan.is_active == True).order_by(Plan.name))
    plans = plans_result.scalars().all()
    servers_result = await db.execute(select(Server).where(Server.is_active == True).order_by(Server.name))
    servers = servers_result.scalars().all()

    users_data = []
    for u in users:
        sub = active_sub_by_user.get(u.id)
        used_gb = round(sub.traffic_used_bytes / (1024**3), 2) if sub and sub.traffic_used_bytes else 0.0
        limit_gb = 0.0
        if sub and sub.plan and sub.plan.traffic_limit_gb > 0:
            limit_gb = round(sub.plan.traffic_limit_gb + (sub.bonus_gb or 0.0), 2)
        users_data.append({"user": u, "sub": sub, "used_gb": used_gb, "limit_gb": limit_gb})

    return templates.TemplateResponse(request, "users.html", {
        "users_data": users_data,
        "plans": plans,
        "servers": servers,
    })


@router.get("/admin/subscriptions", response_class=HTMLResponse)
async def subscriptions_page(request: Request, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Render subscriptions page."""
    result = await db.execute(
        select(Subscription).options(
            selectinload(Subscription.user),
            selectinload(Subscription.plan),
            selectinload(Subscription.server)
        ).order_by(Subscription.id.desc())
    )
    subscriptions = result.scalars().all()

    # Pull live counters from the node instead of showing whatever the
    # 1-minute background job last cached — the whole point of opening this
    # page is usually to check current usage, so waiting on the next cron
    # tick isn't good enough.
    for sub in subscriptions:
        if not sub.is_active or not sub.wg_peer_id or not sub.server:
            continue
        await accumulate_traffic(sub, sub.server)
    await db.commit()

    # Data for the "create subscription" modal dropdowns
    users_result = await db.execute(select(User).where(User.is_active == True).order_by(User.username))
    users = users_result.scalars().all()
    plans_result = await db.execute(select(Plan).where(Plan.is_active == True).order_by(Plan.name))
    plans = plans_result.scalars().all()
    servers_result = await db.execute(select(Server).where(Server.is_active == True).order_by(Server.name))
    servers = servers_result.scalars().all()

    return templates.TemplateResponse(request, "subscriptions.html", {
        "subscriptions": subscriptions,
        "users": users,
        "plans": plans,
        "servers": servers,
    })


@router.get("/admin/servers", response_class=HTMLResponse)
async def servers_page(request: Request, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Render servers page."""
    result = await db.execute(select(Server).order_by(Server.id))
    servers = result.scalars().all()
    return templates.TemplateResponse(request, "servers.html", {"servers": servers})


@router.get("/admin/accounts", response_class=HTMLResponse)
async def accounts_page(request: Request, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Render the access-code accounts page: issue new codes, list existing ones.

    Codes themselves are created via POST /api/accounts (see routers/accounts.py) —
    this page just gives that endpoint a form and shows the returned code once,
    instead of requiring curl.
    """
    result = await db.execute(
        select(Account).options(selectinload(Account.plan)).order_by(Account.created_at.desc())
    )
    accounts = result.scalars().all()

    accounts_data = []
    for a in accounts:
        devices_used = await db.scalar(
            select(func.count(Device.id)).where(Device.account_id == a.id, Device.revoked_at.is_(None))
        )
        used_bytes = await db.scalar(
            select(func.coalesce(func.sum(Subscription.traffic_used_bytes), 0)).where(Subscription.account_id == a.id)
        )
        accounts_data.append({
            "account": a,
            "devices_used": devices_used or 0,
            "traffic_used_gb": round((used_bytes or 0) / (1024 ** 3), 3),
        })

    plans_result = await db.execute(select(Plan).where(Plan.is_active == True).order_by(Plan.name))
    plans = plans_result.scalars().all()

    return templates.TemplateResponse(request, "accounts.html", {
        "accounts_data": accounts_data,
        "plans": plans,
    })


@router.get("/admin/plans", response_class=HTMLResponse)
async def plans_page(request: Request, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Render plans page."""
    result = await db.execute(select(Plan).options(selectinload(Plan.servers)).order_by(Plan.id))
    plans = result.scalars().all()
    servers_result = await db.execute(select(Server).where(Server.is_active == True).order_by(Server.name))
    servers = servers_result.scalars().all()
    return templates.TemplateResponse(request, "plans.html", {"plans": plans, "servers": servers})
