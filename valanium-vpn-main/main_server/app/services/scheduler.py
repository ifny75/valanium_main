from apscheduler.schedulers.asyncio import AsyncIOScheduler
from app.database import async_session_maker
from app.services.subscription_service import check_expired_subscriptions, check_traffic_limits
import logging

logger = logging.getLogger(__name__)

async def run_check_expired_subscriptions():
    logger.info("Running scheduled job: check_expired_subscriptions")
    async with async_session_maker() as db:
        await check_expired_subscriptions(db)

async def run_check_traffic_limits():
    logger.info("Running scheduled job: check_traffic_limits")
    async with async_session_maker() as db:
        await check_traffic_limits(db)

def start_scheduler():
    scheduler = AsyncIOScheduler()
    scheduler.add_job(run_check_expired_subscriptions, 'interval', minutes=5)
    scheduler.add_job(run_check_traffic_limits, 'interval', minutes=1)
    scheduler.start()
    logger.info("APScheduler started")
