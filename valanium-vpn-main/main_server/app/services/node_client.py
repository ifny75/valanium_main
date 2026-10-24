import httpx
import logging
from typing import Optional, Dict, Any
from app.models import Server

logger = logging.getLogger(__name__)

class NodeClient:
    """Async HTTP client for communicating with VPN Node Agents (WireGuard)."""

    @staticmethod
    def _get_base_url(server: Server) -> str:
        return f"http://{server.host}:{server.api_port}"

    @staticmethod
    def _get_headers(server: Server) -> dict:
        return {"X-API-Key": server.api_key}

    @classmethod
    async def add_peer(cls, server: Server, public_key: str, label: str = "") -> Optional[Dict[str, Any]]:
        """Registers a client public key with the node. Returns the node's
        response ({id, allowed_ip, server_public_key, endpoint}) or None."""
        url = f"{cls._get_base_url(server)}/peers"
        payload = {"public_key": public_key, "label": label}
        try:
            async with httpx.AsyncClient(timeout=10.0) as client:
                response = await client.post(url, headers=cls._get_headers(server), json=payload)
                response.raise_for_status()
                return response.json()
        except Exception as e:
            logger.error(f"Failed to add peer to server {server.name}: {e}")
            return None

    @classmethod
    async def remove_peer(cls, server: Server, peer_id: str) -> bool:
        url = f"{cls._get_base_url(server)}/peers/{peer_id}"
        try:
            async with httpx.AsyncClient(timeout=10.0) as client:
                response = await client.delete(url, headers=cls._get_headers(server))
                response.raise_for_status()
                return True
        except Exception as e:
            logger.error(f"Failed to remove peer {peer_id} from server {server.name}: {e}")
            return False

    @classmethod
    async def get_traffic(cls, server: Server, peer_id: str) -> Optional[Dict[str, int]]:
        url = f"{cls._get_base_url(server)}/traffic/{peer_id}"
        try:
            async with httpx.AsyncClient(timeout=10.0) as client:
                response = await client.get(url, headers=cls._get_headers(server))
                response.raise_for_status()
                data = response.json()
                return {
                    "bytes_up": data.get("bytes_up", 0),
                    "bytes_down": data.get("bytes_down", 0)
                }
        except Exception as e:
            logger.error(f"Failed to get traffic for peer {peer_id} on server {server.name}: {e}")
            return None

    @classmethod
    async def health_check(cls, server: Server) -> bool:
        url = f"{cls._get_base_url(server)}/health"
        try:
            async with httpx.AsyncClient(timeout=10.0) as client:
                response = await client.get(url, headers=cls._get_headers(server))
                response.raise_for_status()
                return True
        except Exception as e:
            logger.error(f"Health check failed for server {server.name}: {e}")
            return False
