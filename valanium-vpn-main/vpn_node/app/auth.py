import secrets
from fastapi import Header, HTTPException
from .config import settings

async def verify_api_key(x_api_key: str = Header(...)) -> str:
    """FastAPI dependency to verify the provided API key."""
    if not secrets.compare_digest(x_api_key, settings.API_KEY):
        raise HTTPException(status_code=403, detail="Invalid API Key")
    return x_api_key
