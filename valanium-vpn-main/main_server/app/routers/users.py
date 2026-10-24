from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select
from typing import List

from app.database import get_db
from app.models import User, Subscription
from app.schemas import UserCreate, UserOut
from app.auth import verify_admin

router = APIRouter(prefix="/api/users", tags=["users"])

@router.get("/", response_model=List[UserOut])
async def list_users(db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """List all users."""
    result = await db.execute(select(User).order_by(User.created_at.desc()))
    return result.scalars().all()

@router.post("/", response_model=UserOut)
async def create_user(user: UserCreate, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Create a new user."""
    db_user = User(username=user.username, email=user.email)
    db.add(db_user)
    await db.commit()
    await db.refresh(db_user)
    return db_user

@router.get("/{user_id}", response_model=UserOut)
async def get_user(user_id: int, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Get a user by ID."""
    result = await db.execute(select(User).where(User.id == user_id))
    user = result.scalars().first()
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
    return user

@router.put("/{user_id}/toggle")
async def toggle_user(user_id: int, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Toggle user active status. If deactivating, also deactivate their active subscriptions."""
    result = await db.execute(select(User).where(User.id == user_id))
    user = result.scalars().first()
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
    
    user.is_active = not user.is_active
    if not user.is_active:
        sub_result = await db.execute(select(Subscription).where(Subscription.user_id == user_id, Subscription.is_active == True))
        for sub in sub_result.scalars().all():
            sub.is_active = False
            
    await db.commit()
    return {"message": "User toggled successfully", "is_active": user.is_active}

@router.delete("/{user_id}")
async def delete_user(user_id: int, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Delete a user, deactivating subscriptions first."""
    result = await db.execute(select(User).where(User.id == user_id))
    user = result.scalars().first()
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
        
    sub_result = await db.execute(select(Subscription).where(Subscription.user_id == user_id))
    for sub in sub_result.scalars().all():
        sub.is_active = False
        
    await db.delete(user)
    await db.commit()
    return {"message": "User deleted successfully"}
