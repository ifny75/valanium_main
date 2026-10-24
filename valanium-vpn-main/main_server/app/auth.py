from datetime import datetime, timedelta, timezone
from typing import Optional
from fastapi import Depends, HTTPException, status, Request
from jose import JWTError, jwt
from app.config import settings

# Simple in-memory login throttle (per-process, no external deps needed on low-end hosts).
# Not shared across workers/restarts, but that's fine for a single-worker admin panel.
_MAX_FAILED_ATTEMPTS = 5
_LOCKOUT_SECONDS = 300
_failed_logins: dict[str, tuple[int, datetime]] = {}


def is_login_locked(client_ip: str) -> bool:
    """Check whether client_ip is currently locked out after too many failed logins."""
    entry = _failed_logins.get(client_ip)
    if not entry:
        return False
    count, last_attempt = entry
    if count < _MAX_FAILED_ATTEMPTS:
        return False
    if datetime.now(timezone.utc) - last_attempt > timedelta(seconds=_LOCKOUT_SECONDS):
        _failed_logins.pop(client_ip, None)
        return False
    return True


def register_failed_login(client_ip: str) -> None:
    """Record a failed login attempt for client_ip."""
    count, _ = _failed_logins.get(client_ip, (0, datetime.now(timezone.utc)))
    _failed_logins[client_ip] = (count + 1, datetime.now(timezone.utc))


def clear_failed_logins(client_ip: str) -> None:
    """Reset failed-login counter for client_ip after a successful login."""
    _failed_logins.pop(client_ip, None)


def create_access_token(data: dict, expires_delta: Optional[timedelta] = None) -> str:
    """Create a JWT access token."""
    to_encode = data.copy()
    if expires_delta:
        expire = datetime.now(timezone.utc) + expires_delta
    else:
        expire = datetime.now(timezone.utc) + timedelta(minutes=settings.ACCESS_TOKEN_EXPIRE_MINUTES)
    # typ отделяет админскую сессию от сессии устройства (`create_device_token`):
    # секрет подписи один, и без метки роли жетон одной роли предъявляется в
    # другой.
    to_encode.update({"exp": expire, "typ": "admin"})
    encoded_jwt = jwt.encode(to_encode, settings.SECRET_KEY, algorithm="HS256")
    return encoded_jwt


def create_device_token(device_pub: str) -> str:
    """Жетон сессии устройства (`/v1`).

    `typ` обязателен и проверяется: без него админский жетон, подписанный тем
    же ключом, сошёл бы за устройство — и наоборот. Две роли, один секрет,
    разные аудитории.
    """
    expire = datetime.now(timezone.utc) + timedelta(minutes=settings.DEVICE_TOKEN_EXPIRE_MINUTES)
    return jwt.encode(
        {"sub": device_pub, "typ": "vpn-device", "exp": expire},
        settings.SECRET_KEY,
        algorithm="HS256",
    )


def device_pub_from_token(token: str) -> Optional[str]:
    """Публичный ключ устройства из жетона, либо None. Не бросает."""
    try:
        payload = jwt.decode(token, settings.SECRET_KEY, algorithms=["HS256"])
    except JWTError:
        return None
    if payload.get("typ") != "vpn-device":
        return None
    sub = payload.get("sub")
    return sub if isinstance(sub, str) else None


async def authenticate_admin(username: str, password: str) -> Optional[str]:
    """Check admin credentials against config. Returns username if valid, None otherwise."""
    if username == settings.ADMIN_USERNAME and password == settings.ADMIN_PASSWORD:
        return username
    return None


def verify_admin(request: Request) -> str:
    """Dependency: verify admin JWT from cookie or Authorization header."""
    token = request.cookies.get("access_token")

    # Cookie may contain "Bearer <token>" prefix
    if token and token.startswith("Bearer "):
        token = token[7:]

    if not token:
        # Fallback to Authorization header
        auth_header = request.headers.get("Authorization")
        if auth_header and auth_header.startswith("Bearer "):
            token = auth_header.split(" ")[1]
        else:
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="Not authenticated",
            )

    try:
        payload = jwt.decode(token, settings.SECRET_KEY, algorithms=["HS256"])
        username: str = payload.get("sub")
        if payload.get("typ") != "admin" or username is None or username != settings.ADMIN_USERNAME:
            raise HTTPException(
                status_code=status.HTTP_403_FORBIDDEN,
                detail="Could not validate credentials",
            )
        return username
    except JWTError:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid token",
        )
