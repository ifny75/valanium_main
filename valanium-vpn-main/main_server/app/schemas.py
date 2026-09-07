from pydantic import BaseModel, ConfigDict
from datetime import datetime
from typing import Optional, List

class UserCreate(BaseModel):
    username: str
    email: Optional[str] = None

class UserOut(BaseModel):
    id: int
    username: str
    email: Optional[str]
    is_active: bool
    created_at: datetime
    
    model_config = ConfigDict(from_attributes=True)

class PlanCreate(BaseModel):
    name: str
    duration_days: int
    traffic_limit_gb: float
    max_devices: int = 1
    price: float
    server_ids: List[int] = []  # empty = all servers available

class PlanOut(BaseModel):
    id: int
    name: str
    duration_days: int
    traffic_limit_gb: float
    max_devices: int
    price: float
    is_active: bool
    server_ids: List[int] = []
    
    model_config = ConfigDict(from_attributes=True)

class ServerCreate(BaseModel):
    name: str
    host: str
    api_port: int = 8000
    api_key: str
    domain: str
    location: Optional[str] = None
    max_users: int = 100
    # Display metadata for map-based clients (vpn_client) — all optional
    # since they don't affect node operation, only how it's shown.
    country_name: Optional[str] = None
    country_code: Optional[str] = None
    map_x: Optional[float] = None
    map_y: Optional[float] = None

class ServerOut(BaseModel):
    id: int
    name: str
    host: str
    api_port: int
    domain: str
    location: Optional[str]
    is_active: bool
    max_users: int
    country_name: Optional[str] = None
    country_code: Optional[str] = None
    map_x: Optional[float] = None
    map_y: Optional[float] = None

    model_config = ConfigDict(from_attributes=True)

class SubscriptionCreate(BaseModel):
    user_id: int
    plan_id: int
    server_id: int

class BonusGrant(BaseModel):
    gb: float

class SubscriptionOut(BaseModel):
    id: int
    user_id: int
    plan_id: int
    server_id: int
    wg_public_key: str
    wg_allowed_ip: Optional[str] = None
    started_at: datetime
    expires_at: datetime
    is_active: bool
    traffic_used_bytes: int
    traffic_used_gb: float
    traffic_limit_gb: float
    bonus_gb: float
    # Full client config, built from the subscription's own wg_private_key
    # plus the node's server_public_key/endpoint — nothing else to fetch.
    # wg_private_key itself is deliberately excluded, same as naive_password
    # used to be: this field is the only place it's ever exposed.
    wg_config: str = ""

    model_config = ConfigDict(from_attributes=True)

# --- вход по коду доступа (/v1) ---------------------------------------------

class HelloOut(BaseModel):
    nonce: str          # 32 байта hex
    server_time: datetime


class SignedRequest(BaseModel):
    """Общая форма подписанного запроса: nonce от `/v1/hello` + подпись.

    `device_pub` в отзыве — это ключ отзываемого устройства, а подпись всё
    равно ставит своё: чьё устройство подписало, видно по Bearer-жетону.
    """

    nonce: str
    device_pub: str     # 32 байта hex
    sig: str            # 64 байта hex


class EnrollRequest(SignedRequest):
    code: str           # как человек его ввёл: регистр, дефисы и пробелы не важны


class AccountOut(BaseModel):
    expires_at: datetime
    is_active: bool
    plan_name: str
    traffic_limit_gb: float
    traffic_used_gb: float
    max_devices: int
    devices_used: int


class TokenOut(BaseModel):
    token: str
    expires_in: int
    account: AccountOut


class DeviceOut(BaseModel):
    device_pub: str
    created_at: datetime
    is_current: bool


class ServerBrief(BaseModel):
    """Каталог узлов для клиента: без host, api_port и ключа агента."""

    id: int
    name: str
    country_name: Optional[str] = None
    country_code: Optional[str] = None
    map_x: Optional[float] = None
    map_y: Optional[float] = None


class PeerRequest(BaseModel):
    server_id: int
    wg_public_key: str  # base64, приватная часть остаётся на устройстве


class PeerOut(BaseModel):
    address: str
    server_public_key: str
    endpoint: str
    dns: str
    allowed_ips: str
    persistent_keepalive: int


class AccountCreate(BaseModel):
    plan_id: int
    days: Optional[int] = None   # по умолчанию — срок из тарифа
    note: Optional[str] = None


class AccountCreated(BaseModel):
    """Код показывается ровно один раз: в базе только его хеш."""

    id: int
    code: str
    expires_at: datetime


class AccountAdminOut(BaseModel):
    id: int
    plan_id: int
    created_at: datetime
    expires_at: datetime
    is_active: bool
    note: Optional[str] = None
    devices_used: int
    traffic_used_gb: float

    model_config = ConfigDict(from_attributes=True)


class LoginForm(BaseModel):
    username: str
    password: str

class Token(BaseModel):
    access_token: str
    token_type: str

class DashboardStats(BaseModel):
    total_users: int
    active_subscriptions: int
    total_servers: int
    total_traffic_gb: float
