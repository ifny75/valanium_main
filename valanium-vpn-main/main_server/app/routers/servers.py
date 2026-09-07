from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select
from typing import List

from app.database import get_db
from app.models import Server
from app.schemas import ServerCreate, ServerOut
from app.auth import verify_admin
from app.services.node_client import NodeClient

router = APIRouter(prefix="/api/servers", tags=["servers"])

@router.get("/", response_model=List[ServerOut])
async def list_servers(db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """List all servers."""
    result = await db.execute(select(Server).order_by(Server.id))
    return result.scalars().all()

@router.post("/", response_model=ServerOut)
async def create_server(server: ServerCreate, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Create a new server."""
    db_server = Server(
        name=server.name,
        host=server.host,
        api_port=server.api_port,
        api_key=server.api_key,
        domain=server.domain,
        location=server.location,
        max_users=server.max_users,
        country_name=server.country_name,
        country_code=server.country_code,
        map_x=server.map_x,
        map_y=server.map_y,
    )
    db.add(db_server)
    await db.commit()
    await db.refresh(db_server)
    return db_server

@router.get("/{server_id}", response_model=ServerOut)
async def get_server(server_id: int, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Get a server by ID."""
    result = await db.execute(select(Server).where(Server.id == server_id))
    server = result.scalars().first()
    if not server:
        raise HTTPException(status_code=404, detail="Server not found")
    return server

@router.get("/{server_id}/health")
async def check_server_health(server_id: int, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Check node agent health via NodeClient."""
    result = await db.execute(select(Server).where(Server.id == server_id))
    server = result.scalars().first()
    if not server:
        raise HTTPException(status_code=404, detail="Server not found")
        
    is_healthy = await NodeClient.health_check(server)
    return {"status": "ok" if is_healthy else "unreachable", "server": server.name}

@router.put("/{server_id}/toggle")
async def toggle_server(server_id: int, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Toggle server active status."""
    result = await db.execute(select(Server).where(Server.id == server_id))
    server = result.scalars().first()
    if not server:
        raise HTTPException(status_code=404, detail="Server not found")
        
    server.is_active = not server.is_active
    await db.commit()
    return {"message": "Server toggled successfully", "is_active": server.is_active}

@router.delete("/{server_id}")
async def delete_server(server_id: int, db: AsyncSession = Depends(get_db), admin: str = Depends(verify_admin)):
    """Delete a server."""
    result = await db.execute(select(Server).where(Server.id == server_id))
    server = result.scalars().first()
    if not server:
        raise HTTPException(status_code=404, detail="Server not found")
        
    await db.delete(server)
    await db.commit()
    return {"message": "Server deleted successfully"}
