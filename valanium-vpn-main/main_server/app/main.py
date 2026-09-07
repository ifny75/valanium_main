import os
import logging
from contextlib import asynccontextmanager
from fastapi import APIRouter, FastAPI
from fastapi.staticfiles import StaticFiles
from app.config import settings
from app.database import init_db
from app.services.scheduler import start_scheduler

logger = logging.getLogger(__name__)


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Startup and shutdown events."""
    if settings.ADMIN_PASSWORD == "admin":
        logger.warning(
            "SECURITY: ADMIN_PASSWORD is still the default 'admin'. "
            "Set ADMIN_USERNAME/ADMIN_PASSWORD in .env before exposing this panel."
        )
    await init_db()
    start_scheduler()
    yield


app = FastAPI(title="ObsyxGen VPN Panel", lifespan=lifespan)

# No CORS middleware: the admin UI and API are always same-origin (server-rendered
# Jinja2 pages calling relative /api/... paths), so there is no legitimate cross-origin
# use case. A wildcard origin combined with credentialed cookies would let any site
# ride the admin's session (CSRF), so we deliberately don't add it back.

# Ensure static directory exists
static_dir = os.path.join(os.path.dirname(__file__), "..", "static")
os.makedirs(static_dir, exist_ok=True)
app.mount("/static", StaticFiles(directory=static_dir), name="static")

# Include routers — all mounted under PANEL_PATH_PREFIX (see config.py) so
# the login page, /admin/*, and /api/* are all unreachable without knowing
# the prefix, not just the port.
from app.routers import admin, users, subscriptions, servers, plans, accounts, v1

panel_router = APIRouter(prefix=settings.PANEL_PATH_PREFIX)
panel_router.include_router(admin.router)
panel_router.include_router(users.router)
panel_router.include_router(subscriptions.router)
panel_router.include_router(servers.router)
panel_router.include_router(plans.router)
panel_router.include_router(accounts.router)
app.include_router(panel_router)

# Клиентский API — намеренно вне PANEL_PATH_PREFIX. Путь админки прячут,
# потому что за ним пароль; путь клиента прятать не от кого — по нему ходит
# каждая установленная копия, и его знание не даёт ничего: внутри всё равно
# нужна подпись устройства.
app.include_router(v1.router)


@app.get("/")
async def root():
    """Root endpoint redirects info."""
    return {"message": "Welcome to ObsyxGen VPN Panel API"}
