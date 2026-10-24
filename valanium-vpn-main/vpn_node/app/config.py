from pydantic_settings import BaseSettings


class Settings(BaseSettings):
    """Configuration settings for the VPN Node Agent (WireGuard)."""

    API_KEY: str
    WG_INTERFACE: str = "wg0"
    WG_CONFIG_PATH: str = "/etc/wireguard/wg0.conf"
    WG_PEERS_STORE: str = "/etc/wireguard/peers.json"
    WG_SERVER_KEY_PATH: str = "/etc/wireguard/server_private.key"
    # Client subnet handed out to peers. .1 is reserved for the server itself.
    WG_SUBNET: str = "10.10.0.0/24"
    WG_LISTEN_PORT: int = 51820
    # Public endpoint clients dial — usually the node's public IP.
    SERVER_ENDPOINT_HOST: str
    EGRESS_INTERFACE: str = "eth0"
    HOST: str = "0.0.0.0"
    PORT: int = 8000

    class Config:
        env_file = ".env"


settings = Settings()
