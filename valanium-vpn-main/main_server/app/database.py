from sqlalchemy import text
from sqlalchemy.ext.asyncio import create_async_engine, async_sessionmaker, AsyncSession
from sqlalchemy.orm import declarative_base
from app.config import settings

engine = create_async_engine(settings.DATABASE_URL, echo=False)
async_session_maker = async_sessionmaker(engine, expire_on_commit=False, class_=AsyncSession)

Base = declarative_base()

async def get_db():
    """Dependency for getting async database session"""
    async with async_session_maker() as session:
        yield session

async def _run_lightweight_migrations(conn):
    """Add columns introduced after the initial schema to existing SQLite DB files.

    There's no Alembic in this project; create_all only creates missing tables, so
    columns added to a model later need to be patched onto already-existing tables.
    """
    result = await conn.execute(text("PRAGMA table_info(subscriptions)"))
    columns = {row[1] for row in result.fetchall()}
    if "bonus_gb" not in columns:
        await conn.execute(text("ALTER TABLE subscriptions ADD COLUMN bonus_gb FLOAT DEFAULT 0.0"))
    if "wg_last_raw_bytes" not in columns:
        await conn.execute(text("ALTER TABLE subscriptions ADD COLUMN wg_last_raw_bytes BIGINT DEFAULT 0"))
        # Backfill: seed the counter from what's already accumulated so the
        # very next fetch computes a delta against *something* sane instead
        # of treating the whole historical total as a brand-new delta.
        await conn.execute(text("UPDATE subscriptions SET wg_last_raw_bytes = traffic_used_bytes"))

    # Путь клиента по коду доступа: подписка принадлежит устройству аккаунта,
    # а не пользователю админки. Обе пары колонок nullable, потому что старые
    # строки заполнены по-старому и переезжать им некуда.
    for col in ("account_id", "device_id"):
        if col not in columns:
            await conn.execute(text(f"ALTER TABLE subscriptions ADD COLUMN {col} INTEGER"))

    result = await conn.execute(text("PRAGMA table_info(servers)"))
    server_columns = {row[1] for row in result.fetchall()}
    for col, ddl_type in (
        ("country_name", "VARCHAR(100)"),
        ("country_code", "VARCHAR(2)"),
        ("map_x", "FLOAT"),
        ("map_y", "FLOAT"),
    ):
        if col not in server_columns:
            await conn.execute(text(f"ALTER TABLE servers ADD COLUMN {col} {ddl_type}"))

async def init_db():
    """Create all tables in the database and patch any missing columns."""
    async with engine.begin() as conn:
        await conn.run_sync(Base.metadata.create_all)
        await _run_lightweight_migrations(conn)
