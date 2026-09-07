import secrets
from pydantic_settings import BaseSettings, SettingsConfigDict

class Settings(BaseSettings):
    DATABASE_URL: str = 'sqlite+aiosqlite:///./obsyxgen.db'
    SECRET_KEY: str = secrets.token_urlsafe(32)
    ADMIN_USERNAME: str = 'admin'
    ADMIN_PASSWORD: str = 'admin'
    ACCESS_TOKEN_EXPIRE_MINUTES: int = 60
    # Set to true once the panel is served over HTTPS so the auth cookie is never sent in the clear
    COOKIE_SECURE: bool = False
    # Obscurity layer on top of real auth: every route (login/admin/api) is
    # mounted under this prefix, so a bare port scan + "/admin" guess finds
    # nothing. Empty string keeps everything at the root, for local dev.
    PANEL_PATH_PREFIX: str = ''

    # --- вход по коду доступа (/v1) ---
    # Перец для хеша кода. Пустой = взять из файла рядом с базой, создав его
    # при первом запуске (services/access_code.py). Задавать в .env нужно
    # только если базу и перец разносят по разным машинам.
    ACCESS_CODE_PEPPER_FILE: str = './access_code_pepper'
    # Жетон устройства живёт коротко: он же и есть вся сессия, а продлить её
    # стоит одну подпись — устройство всегда может подписать новый nonce.
    DEVICE_TOKEN_EXPIRE_MINUTES: int = 60
    # Адрес DNS-резолвера, который клиент ставит в тоннель. По умолчанию —
    # адрес самого узла в туннельной сети: третья сторона, видящая все
    # запросы, не нужна (ARCHITECTURE.md §8).
    TUNNEL_DNS: str = ''

    model_config = SettingsConfigDict(env_file='.env', env_file_encoding='utf-8')

settings = Settings()
