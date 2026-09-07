"""Сквозной путь клиента: код → устройство → вход → пир на узле."""

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from app.services import ed25519

def wg_key() -> str:
    """Свежий 32-байтовый ключ в base64 — как его сгенерировал бы клиент.

    Каждый раз новый намеренно: на каждый узел у устройства своя пара
    (см. /v1/peer), и общая константа в тестах скрыла бы ровно то, что эта
    развязка проверяет.
    """
    import base64
    import os

    return base64.b64encode(os.urandom(32)).decode()


def keypair():
    priv = Ed25519PrivateKey.generate()
    pub = priv.public_key().public_bytes(
        encoding=serialization.Encoding.Raw, format=serialization.PublicFormat.Raw
    )
    return priv, pub


def nonce_of(client) -> bytes:
    resp = client.post("/v1/hello")
    assert resp.status_code == 200
    return bytes.fromhex(resp.json()["nonce"])


def make_plan(admin_client, *, max_devices=2, days=30, gb=0.0):
    resp = admin_client.post(
        "/api/plans/",
        json={
            "name": "test",
            "duration_days": days,
            "traffic_limit_gb": gb,
            "max_devices": max_devices,
            "price": 0,
            "server_ids": [],
        },
    )
    assert resp.status_code == 200, resp.text
    return resp.json()["id"]


def make_server(admin_client, name="node-1"):
    resp = admin_client.post(
        "/api/servers/",
        json={
            "name": name,
            "host": "10.0.0.9",
            "api_port": 8000,
            "api_key": "node-key",
            "domain": "node.example",
            "country_name": "Netherlands",
            "country_code": "nl",
        },
    )
    assert resp.status_code == 200, resp.text
    return resp.json()["id"]


def issue_code(admin_client, plan_id, **kw):
    resp = admin_client.post("/api/accounts/", json={"plan_id": plan_id, **kw})
    assert resp.status_code == 200, resp.text
    return resp.json()["code"]


def enroll(client, code, priv=None, pub=None):
    if priv is None:
        priv, pub = keypair()
    nonce = nonce_of(client)
    sig = priv.sign(ed25519.enroll_message(nonce, pub))
    resp = client.post(
        "/v1/devices",
        json={
            "code": code,
            "nonce": nonce.hex(),
            "device_pub": pub.hex(),
            "sig": sig.hex(),
        },
    )
    return resp, priv, pub


def login(client, priv, pub):
    nonce = nonce_of(client)
    sig = priv.sign(ed25519.auth_message(nonce, pub))
    return client.post(
        "/v1/auth",
        json={"nonce": nonce.hex(), "device_pub": pub.hex(), "sig": sig.hex()},
    )


def bearer(token):
    return {"Authorization": f"Bearer {token}"}


# --------------------------------------------------------------------------


def test_full_flow_code_to_tunnel(admin_client):
    client = admin_client
    plan_id = make_plan(client)
    server_id = make_server(client)
    code = issue_code(client, plan_id)

    resp, priv, pub = enroll(client, code)
    assert resp.status_code == 200, resp.text
    body = resp.json()
    assert body["account"]["devices_used"] == 1
    assert body["account"]["max_devices"] == 2

    # Вход второй раз — уже без кода, одной подписью.
    resp = login(client, priv, pub)
    assert resp.status_code == 200, resp.text
    token = resp.json()["token"]

    resp = client.get("/v1/servers", headers=bearer(token))
    assert resp.status_code == 200
    assert [s["name"] for s in resp.json()] == ["node-1"]
    # Ключ агента и адрес узла клиенту не отдаём.
    assert "api_key" not in resp.json()[0] and "host" not in resp.json()[0]

    resp = client.post(
        "/v1/peer",
        json={"server_id": server_id, "wg_public_key": wg_key()},
        headers=bearer(token),
    )
    assert resp.status_code == 200, resp.text
    peer = resp.json()
    assert peer["address"] == "10.10.0.2/32"
    assert peer["endpoint"] == "10.0.0.9:443"
    assert peer["allowed_ips"] == "0.0.0.0/0, ::/0"
    # Приватного ключа в ответе нет и быть не может — его сервер не видел.
    assert "private" not in resp.text.lower()


def test_wrong_and_malformed_codes_are_refused(admin_client):
    client = admin_client
    plan_id = make_plan(client)
    issue_code(client, plan_id)

    resp, _, _ = enroll(client, "AAAAA-BBBBB-CCCCC-DDDDD")
    assert resp.status_code == 400  # не сошлась контрольная сумма

    from app.services import access_code

    other = access_code.generate()  # валидный по форме, но не выпускался
    resp, _, _ = enroll(client, other)
    assert resp.status_code == 403


def test_signature_must_match_the_device_key(admin_client):
    client = admin_client
    code = issue_code(client, make_plan(client))
    priv, pub = keypair()
    _, other_pub = keypair()

    nonce = nonce_of(client)
    sig = priv.sign(ed25519.enroll_message(nonce, other_pub))
    resp = client.post(
        "/v1/devices",
        json={"code": code, "nonce": nonce.hex(), "device_pub": other_pub.hex(), "sig": sig.hex()},
    )
    assert resp.status_code == 401


def test_nonce_is_single_use(admin_client):
    client = admin_client
    code = issue_code(client, make_plan(client))
    priv, pub = keypair()

    nonce = nonce_of(client)
    payload = {
        "code": code,
        "nonce": nonce.hex(),
        "device_pub": pub.hex(),
        "sig": priv.sign(ed25519.enroll_message(nonce, pub)).hex(),
    }
    assert client.post("/v1/devices", json=payload).status_code == 200
    # Повтор того же кадра — перехваченный AUTH не должен работать дважды.
    assert client.post("/v1/devices", json=payload).status_code == 400


def test_device_limit_is_enforced_server_side(admin_client):
    client = admin_client
    code = issue_code(client, make_plan(client, max_devices=2))

    assert enroll(client, code)[0].status_code == 200
    assert enroll(client, code)[0].status_code == 200
    resp, _, _ = enroll(client, code)
    assert resp.status_code == 409
    assert resp.json()["detail"] == "device limit reached"


def test_reenroll_of_same_key_is_not_a_new_device(admin_client):
    client = admin_client
    code = issue_code(client, make_plan(client, max_devices=1))
    resp, priv, pub = enroll(client, code)
    assert resp.status_code == 200

    resp, _, _ = enroll(client, code, priv, pub)
    assert resp.status_code == 200
    assert resp.json()["account"]["devices_used"] == 1


def test_revoked_device_loses_token_and_peer(admin_client):
    client = admin_client
    plan_id = make_plan(client, max_devices=3)
    server_id = make_server(client)
    code = issue_code(client, plan_id)

    keep_resp, keep_priv, keep_pub = enroll(client, code)
    gone_resp, gone_priv, gone_pub = enroll(client, code)
    keep_token = keep_resp.json()["token"]
    gone_token = gone_resp.json()["token"]

    client.post(
        "/v1/peer",
        json={"server_id": server_id, "wg_public_key": wg_key()},
        headers=bearer(gone_token),
    )
    assert len(client.node_state["peers"]) == 1

    nonce = nonce_of(client)
    sig = keep_priv.sign(ed25519.revoke_message(nonce, gone_pub))
    resp = client.post(
        "/v1/devices/revoke",
        json={"nonce": nonce.hex(), "device_pub": gone_pub.hex(), "sig": sig.hex()},
        headers=bearer(keep_token),
    )
    assert resp.status_code == 200, resp.text

    # Пир снят с узла сразу, а не «в следующий раз».
    assert client.node_state["peers"] == {}
    assert client.get("/v1/account", headers=bearer(gone_token)).status_code == 401
    assert login(client, gone_priv, gone_pub).status_code == 401
    assert client.get("/v1/account", headers=bearer(keep_token)).status_code == 200


def test_revoke_needs_a_fresh_signature_not_just_a_token(admin_client):
    """Жетон могли украсть из памяти; отзыв требует ключа устройства."""
    client = admin_client
    code = issue_code(client, make_plan(client, max_devices=2))
    a_resp, a_priv, a_pub = enroll(client, code)
    b_resp, _, b_pub = enroll(client, code)

    nonce = nonce_of(client)
    wrong = a_priv.sign(ed25519.auth_message(nonce, b_pub))  # не тот домен
    resp = client.post(
        "/v1/devices/revoke",
        json={"nonce": nonce.hex(), "device_pub": b_pub.hex(), "sig": wrong.hex()},
        headers=bearer(a_resp.json()["token"]),
    )
    assert resp.status_code == 401


def test_account_disabled_by_admin_stops_working(admin_client):
    client = admin_client
    plan_id = make_plan(client)
    server_id = make_server(client)
    code = issue_code(client, plan_id)
    resp, priv, pub = enroll(client, code)
    token = resp.json()["token"]

    client.post(
        "/v1/peer",
        json={"server_id": server_id, "wg_public_key": wg_key()},
        headers=bearer(token),
    )
    assert len(client.node_state["peers"]) == 1

    account_id = client.get("/api/accounts/").json()[0]["id"]
    assert client.put(f"/api/accounts/{account_id}/toggle").status_code == 200

    # Не «не сможет войти в следующий раз», а тоннеля больше нет.
    assert client.node_state["peers"] == {}
    assert client.get("/v1/account", headers=bearer(token)).status_code == 403
    assert login(client, priv, pub).status_code == 403


def test_client_api_needs_no_admin_credentials(client):
    """Без жетона устройства /v1 не отдаёт ничего, а админский путь закрыт."""
    assert client.get("/v1/account").status_code == 401
    assert client.get("/v1/servers").status_code == 401
    assert client.get("/api/accounts/").status_code == 401
    assert client.get("/api/servers/").status_code == 401


def test_device_token_is_not_an_admin_token(admin_client):
    client = admin_client
    code = issue_code(client, make_plan(client))
    token = enroll(client, code)[0].json()["token"]

    client.cookies.clear()
    resp = client.get("/api/accounts/", headers=bearer(token))
    assert resp.status_code in (401, 403)


def test_malformed_wireguard_key_is_refused(admin_client):
    client = admin_client
    plan_id = make_plan(client)
    server_id = make_server(client)
    token = enroll(client, issue_code(client, plan_id))[0].json()["token"]

    for bad in ("", "not-base64!", "c2hvcnQ="):
        resp = client.post(
            "/v1/peer",
            json={"server_id": server_id, "wg_public_key": bad},
            headers=bearer(token),
        )
        assert resp.status_code == 400, bad


def test_peer_can_be_rotated_and_dropped(admin_client):
    client = admin_client
    plan_id = make_plan(client)
    server_id = make_server(client)
    token = enroll(client, issue_code(client, plan_id))[0].json()["token"]

    first = client.post(
        "/v1/peer", json={"server_id": server_id, "wg_public_key": wg_key()}, headers=bearer(token)
    )
    assert first.status_code == 200

    # Смена ключа устройством не должна упираться в «уже зарегистрирован».
    second = client.post(
        "/v1/peer", json={"server_id": server_id, "wg_public_key": wg_key()}, headers=bearer(token)
    )
    assert second.status_code == 200, second.text
    assert len(client.node_state["peers"]) == 1

    assert client.delete(f"/v1/peer/{server_id}", headers=bearer(token)).status_code == 200
    assert client.node_state["peers"] == {}
