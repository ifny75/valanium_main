from fastapi import FastAPI, Depends, HTTPException
from pydantic import BaseModel
from contextlib import asynccontextmanager
from typing import Any, Dict, List, Optional

from .config import settings
from .auth import verify_api_key
from .wireguard_manager import WireGuardManager

wg_manager = WireGuardManager(
    interface=settings.WG_INTERFACE,
    config_path=settings.WG_CONFIG_PATH,
    peers_store=settings.WG_PEERS_STORE,
    server_key_path=settings.WG_SERVER_KEY_PATH,
    subnet=settings.WG_SUBNET,
    listen_port=settings.WG_LISTEN_PORT,
)


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Startup and shutdown events."""
    success = await wg_manager.ensure_base_config()
    if not success:
        print("Warning: Failed to ensure base WireGuard config on startup.")
    yield


app = FastAPI(title="VPN Node Agent (WireGuard)", lifespan=lifespan)


class PeerCreate(BaseModel):
    """A client submits its own WireGuard public key — the private key
    never leaves the client, matching how WireGuard is meant to work."""

    public_key: str
    label: str = ""
    preshared_key: Optional[str] = None


@app.post("/peers", dependencies=[Depends(verify_api_key)])
async def add_peer(peer: PeerCreate) -> Dict[str, Any]:
    """Register a client's public key and hand back what it needs to connect."""
    try:
        return await wg_manager.add_peer(peer.public_key, peer.label, peer.preshared_key)
    except ValueError as e:
        raise HTTPException(status_code=409, detail=str(e))
    except RuntimeError as e:
        raise HTTPException(status_code=500, detail=str(e))


@app.delete("/peers/{peer_id}", dependencies=[Depends(verify_api_key)])
async def remove_peer(peer_id: str) -> Dict[str, str]:
    try:
        await wg_manager.remove_peer(peer_id)
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))
    except RuntimeError as e:
        raise HTTPException(status_code=500, detail=str(e))
    return {"status": "ok"}


@app.get("/peers", dependencies=[Depends(verify_api_key)], response_model=List[Dict[str, Any]])
async def list_peers() -> List[Dict[str, Any]]:
    return await wg_manager.list_peers()


@app.get("/traffic/{peer_id}", dependencies=[Depends(verify_api_key)])
async def get_peer_traffic(peer_id: str) -> Dict[str, int]:
    try:
        return await wg_manager.get_transfer(peer_id)
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))


@app.get("/health", dependencies=[Depends(verify_api_key)])
async def health_check() -> Dict[str, Any]:
    peers = await wg_manager.list_peers()
    return {"status": "ok", "peers_count": len(peers), "interface": settings.WG_INTERFACE}
