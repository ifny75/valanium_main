from datetime import datetime, timezone
from sqlalchemy import Column, Integer, String, Boolean, DateTime, Float, ForeignKey, BigInteger, Table
from sqlalchemy.orm import relationship
from app.database import Base


# Many-to-many: Plan <-> Server (which servers are available for a plan)
plan_servers = Table(
    'plan_servers',
    Base.metadata,
    Column('plan_id', Integer, ForeignKey('plans.id', ondelete='CASCADE'), primary_key=True),
    Column('server_id', Integer, ForeignKey('servers.id', ondelete='CASCADE'), primary_key=True),
)


class User(Base):
    __tablename__ = 'users'

    id = Column(Integer, primary_key=True, autoincrement=True)
    username = Column(String(100), unique=True, nullable=False)
    email = Column(String(255), unique=True, nullable=True)
    is_active = Column(Boolean, default=True)
    created_at = Column(DateTime, default=lambda: datetime.now(timezone.utc).replace(tzinfo=None))

    subscriptions = relationship("Subscription", back_populates="user")


class Plan(Base):
    __tablename__ = 'plans'

    id = Column(Integer, primary_key=True)
    name = Column(String(100), nullable=False)
    duration_days = Column(Integer, nullable=False)
    traffic_limit_gb = Column(Float, nullable=False)
    max_devices = Column(Integer, default=1)
    price = Column(Float, nullable=False)
    is_active = Column(Boolean, default=True)

    subscriptions = relationship("Subscription", back_populates="plan")
    # M2M: servers available for this plan (empty = ALL servers available)
    servers = relationship("Server", secondary=plan_servers, back_populates="plans", lazy="selectin")


class Server(Base):
    __tablename__ = 'servers'

    id = Column(Integer, primary_key=True)
    name = Column(String(100), nullable=False)
    host = Column(String(255), nullable=False)
    api_port = Column(Integer, default=8000)
    api_key = Column(String(255), nullable=False)
    domain = Column(String(255), nullable=False)
    location = Column(String(100), nullable=True)
    is_active = Column(Boolean, default=True)
    max_users = Column(Integer, default=100)
    # Display metadata for map-based clients (vpn_client) — map_x/map_y are
    # a percentage position on assets/map-fill.svg's viewBox, computed the
    # same way as vpn_client/tools/build_map.py's Mercator projection.
    country_name = Column(String(100), nullable=True)
    country_code = Column(String(2), nullable=True)
    map_x = Column(Float, nullable=True)
    map_y = Column(Float, nullable=True)

    subscriptions = relationship("Subscription", back_populates="server")
    # M2M: plans that include this server
    plans = relationship("Plan", secondary=plan_servers, back_populates="servers", lazy="selectin")


class Account(Base):
    """Учётная запись, выданная кодом доступа. Ни имени, ни почты, ни пароля.

    Отдельная сущность от `User` (тот остался у админского пути и у старых
    подписок): у аккаунта нет ничего, что указывало бы на человека, — только
    хеш кода, тариф и срок. Сам код не хранится нигде; показывается один раз
    в момент выпуска (`services/access_code.py`).
    """

    __tablename__ = 'accounts'

    id = Column(Integer, primary_key=True)
    # scrypt(код, перец) — детерминированно, чтобы искать по предъявленному коду.
    code_hash = Column(String(64), unique=True, nullable=False, index=True)
    plan_id = Column(Integer, ForeignKey('plans.id'), nullable=False)
    created_at = Column(DateTime, default=lambda: datetime.now(timezone.utc).replace(tzinfo=None))
    expires_at = Column(DateTime, nullable=False)
    is_active = Column(Boolean, default=True)
    # Пометка для владельца («выдан на встрече 3 сентября»). Необязательна и
    # пуста по умолчанию: любое поле, куда можно записать имя, рано или поздно
    # им заполнят, а потом изымут вместе с базой.
    note = Column(String(200), nullable=True)

    plan = relationship("Plan")
    devices = relationship("Device", back_populates="account", cascade="all, delete-orphan")


class Device(Base):
    """Устройство, привязанное к аккаунту одноразовой тратой кода.

    Ни имени, ни модели, ни версии клиента, ни адреса. Понятное имя клиент
    выводит из отпечатка ключа сам — серверу оно не нужно, а значит и не
    хранится.
    """

    __tablename__ = 'devices'

    id = Column(Integer, primary_key=True)
    account_id = Column(Integer, ForeignKey('accounts.id', ondelete='CASCADE'), nullable=False)
    device_pub = Column(String(64), unique=True, nullable=False, index=True)  # hex Ed25519
    created_at = Column(DateTime, default=lambda: datetime.now(timezone.utc).replace(tzinfo=None))
    revoked_at = Column(DateTime, nullable=True)

    account = relationship("Account", back_populates="devices")


class Subscription(Base):
    __tablename__ = 'subscriptions'

    id = Column(Integer, primary_key=True)
    # user_id — старый админский путь; account_id/device_id — путь клиента по
    # коду доступа. Заполнена всегда ровно одна из двух пар, и обе оставлены
    # нулевыми, чтобы существующие строки пережили миграцию.
    user_id = Column(Integer, ForeignKey('users.id'), nullable=True)
    account_id = Column(Integer, ForeignKey('accounts.id'), nullable=True)
    device_id = Column(Integer, ForeignKey('devices.id'), nullable=True)
    plan_id = Column(Integer, ForeignKey('plans.id'))
    server_id = Column(Integer, ForeignKey('servers.id'))

    # WireGuard-личность подписки. Публичный ключ — единственное, что нужно
    # серверу: у подписок, заведённых клиентом по коду доступа (`/v1/peer`),
    # приватного ключа здесь нет и быть не может, он остаётся на устройстве.
    #
    # wg_private_key оставлен только ради старых подписок админской панели,
    # которая до сих пор раздаёт готовый конфиг одним нажатием. Он подлежит
    # удалению вместе с этим путём (ARCHITECTURE.md, фаза 1): пока он в базе,
    # изъятие диска — это возможность расшифровать записанный трафик.
    wg_public_key = Column(String(64), unique=True, nullable=False)
    wg_private_key = Column(String(64), nullable=True)
    wg_allowed_ip = Column(String(45), nullable=True)
    wg_peer_id = Column(String(64), nullable=True)  # id the node agent assigned this peer
    wg_server_public_key = Column(String(64), nullable=True)
    wg_endpoint = Column(String(255), nullable=True)  # host:port the node handed back

    started_at = Column(DateTime, default=lambda: datetime.now(timezone.utc).replace(tzinfo=None))
    expires_at = Column(DateTime, nullable=False)
    is_active = Column(Boolean, default=True)
    traffic_used_bytes = Column(BigInteger, default=0)
    # Raw cumulative counter last read from the node's `wg show` — the node
    # resets to 0 whenever wg0 (or the peer) is recreated (reboot, service
    # restart), so traffic_used_bytes can't just mirror it directly. Instead
    # we track the delta since this last-seen value and add it on, so a
    # server reboot never erases a user's lifetime usage. See
    # subscription_service.accumulate_traffic().
    wg_last_raw_bytes = Column(BigInteger, default=0)
    bonus_gb = Column(Float, default=0.0)  # admin-granted free traffic on top of the plan's limit

    user = relationship("User", back_populates="subscriptions")
    plan = relationship("Plan", back_populates="subscriptions")
    server = relationship("Server", back_populates="subscriptions")
    traffic_logs = relationship("TrafficLog", back_populates="subscription")


class TrafficLog(Base):
    __tablename__ = 'traffic_logs'

    id = Column(Integer, primary_key=True)
    subscription_id = Column(Integer, ForeignKey('subscriptions.id'))
    bytes_up = Column(BigInteger, default=0)
    bytes_down = Column(BigInteger, default=0)
    logged_at = Column(DateTime, default=lambda: datetime.now(timezone.utc).replace(tzinfo=None))

    subscription = relationship("Subscription", back_populates="traffic_logs")
