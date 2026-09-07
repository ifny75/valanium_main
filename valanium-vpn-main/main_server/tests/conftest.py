"""Общая обвязка тестов.

База — свой файл на каждый прогон, узел — подделка: настоящий агент
WireGuard в тестах не нужен и не должен быть нужен, иначе проверять вход
можно будет только там, где поднят `wg`.
"""

import os
import sys
import tempfile
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

_tmp = tempfile.mkdtemp(prefix="valanium-vpn-tests-")
os.environ["DATABASE_URL"] = f"sqlite+aiosqlite:///{Path(_tmp) / 'test.db'}"
os.environ["SECRET_KEY"] = "test-secret-key-not-for-production"
os.environ["ADMIN_USERNAME"] = "admin"
os.environ["ADMIN_PASSWORD"] = "test-admin-password"
os.environ["ACCESS_CODE_PEPPER_FILE"] = str(Path(_tmp) / "pepper")
os.environ["PANEL_PATH_PREFIX"] = ""


@pytest.fixture()
def client(monkeypatch):
    from fastapi.testclient import TestClient

    from app.main import app
    from app.routers import v1
    from app.services.node_client import NodeClient

    # Ограничители живут в модуле и переживают тест: без сброса десятый
    # подряд тест ловит 429, потому что предыдущие девять ходили с того же
    # адреса. В бою это ровно то поведение, которое нужно.
    v1._enroll_limit._hits.clear()
    v1._auth_limit._hits.clear()

    # Подделка агента узла: раздаёт адреса по порядку и помнит, кого сняли.
    state = {"next": 2, "peers": {}}

    async def add_peer(cls_or_self, server, public_key, label=""):
        if any(p["public_key"] == public_key for p in state["peers"].values()):
            return None
        peer_id = f"peer{state['next']}"
        state["peers"][peer_id] = {"public_key": public_key, "label": label}
        allowed = f"10.10.0.{state['next']}"
        state["next"] += 1
        return {
            "id": peer_id,
            "allowed_ip": allowed,
            "server_public_key": "c2VydmVyLXB1YmxpYy1rZXktMzJiLWJhc2U2NC1wYWQ=",
            "endpoint": f"{server.host}:443",
        }

    async def remove_peer(cls_or_self, server, peer_id):
        state["peers"].pop(peer_id, None)
        return True

    monkeypatch.setattr(NodeClient, "add_peer", classmethod(add_peer))
    monkeypatch.setattr(NodeClient, "remove_peer", classmethod(remove_peer))

    with TestClient(app) as c:
        c.node_state = state
        yield c


@pytest.fixture()
def admin_client(client):
    """Тот же клиент, но с админской сессией в куках."""
    resp = client.post(
        "/login",
        data={"username": "admin", "password": "test-admin-password"},
        follow_redirects=False,
    )
    assert resp.status_code == 303, resp.text
    return client
