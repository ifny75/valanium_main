from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select
from sqlalchemy.orm import selectinload
from typing import List

from app.database import get_db
from app.models import Plan, Server
from app.schemas import PlanCreate, PlanOut
from app.auth import verify_admin

router = APIRouter(prefix="/api/plans", tags=["plans"])


def _plan_to_out(plan: Plan) -> PlanOut:
    """Convert Plan ORM to PlanOut with server_ids."""
    return PlanOut(
        id=plan.id,
        name=plan.name,
        duration_days=plan.duration_days,
        traffic_limit_gb=plan.traffic_limit_gb,
        max_devices=plan.max_devices,
        price=plan.price,
        is_active=plan.is_active,
        server_ids=[s.id for s in plan.servers] if plan.servers else [],
    )


@router.get("/", response_model=List[PlanOut])
async def list_plans(db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """List all plans."""
    result = await db.execute(select(Plan).options(selectinload(Plan.servers)).order_by(Plan.id))
    plans = result.scalars().all()
    return [_plan_to_out(p) for p in plans]


@router.post("/", response_model=PlanOut)
async def create_plan(plan: PlanCreate, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Create a new plan with optional server selection."""
    db_plan = Plan(
        name=plan.name,
        duration_days=plan.duration_days,
        traffic_limit_gb=plan.traffic_limit_gb,
        max_devices=plan.max_devices,
        price=plan.price,
    )

    # Attach selected servers (empty = all available)
    if plan.server_ids:
        result = await db.execute(select(Server).where(Server.id.in_(plan.server_ids)))
        servers = result.scalars().all()
        db_plan.servers = list(servers)

    db.add(db_plan)
    await db.commit()
    await db.refresh(db_plan)
    # Reload servers relationship
    result = await db.execute(select(Plan).options(selectinload(Plan.servers)).where(Plan.id == db_plan.id))
    db_plan = result.scalars().first()
    return _plan_to_out(db_plan)


@router.get("/{plan_id}", response_model=PlanOut)
async def get_plan(plan_id: int, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Get a plan by ID."""
    result = await db.execute(select(Plan).options(selectinload(Plan.servers)).where(Plan.id == plan_id))
    plan = result.scalars().first()
    if not plan:
        raise HTTPException(status_code=404, detail="Plan not found")
    return _plan_to_out(plan)


@router.put("/{plan_id}", response_model=PlanOut)
async def update_plan(plan_id: int, plan_update: PlanCreate, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Update a plan's fields and server selection."""
    result = await db.execute(select(Plan).options(selectinload(Plan.servers)).where(Plan.id == plan_id))
    plan = result.scalars().first()
    if not plan:
        raise HTTPException(status_code=404, detail="Plan not found")

    plan.name = plan_update.name
    plan.duration_days = plan_update.duration_days
    plan.traffic_limit_gb = plan_update.traffic_limit_gb
    plan.max_devices = plan_update.max_devices
    plan.price = plan_update.price

    # Update server associations
    if plan_update.server_ids:
        srv_result = await db.execute(select(Server).where(Server.id.in_(plan_update.server_ids)))
        plan.servers = list(srv_result.scalars().all())
    else:
        plan.servers = []

    await db.commit()
    await db.refresh(plan)
    result = await db.execute(select(Plan).options(selectinload(Plan.servers)).where(Plan.id == plan.id))
    plan = result.scalars().first()
    return _plan_to_out(plan)


@router.put("/{plan_id}/toggle")
async def toggle_plan(plan_id: int, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Toggle plan active status."""
    result = await db.execute(select(Plan).where(Plan.id == plan_id))
    plan = result.scalars().first()
    if not plan:
        raise HTTPException(status_code=404, detail="Plan not found")

    plan.is_active = not plan.is_active
    await db.commit()
    return {"message": "Plan toggled successfully", "is_active": plan.is_active}
