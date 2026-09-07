from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from app.services import ed25519

_L = 2 ** 252 + 27742317777372353535851937790883648493


def keypair():
    priv = Ed25519PrivateKey.generate()
    pub = priv.public_key().public_bytes(
        encoding=serialization.Encoding.Raw, format=serialization.PublicFormat.Raw
    )
    return priv, pub


def test_valid_signature_passes():
    priv, pub = keypair()
    nonce = b"\x01" * 32
    msg = ed25519.auth_message(nonce, pub)
    assert ed25519.verify(priv.sign(msg), msg, pub) is True


def test_small_order_keys_are_rejected():
    """Девять кодировок точек малого порядка: их подпись подделает кто угодно.

    Тот же список, что в мессенджере (valanium-server/test/protocol.test.ts).
    Ключ из нулей с подписью из нулей проходит проверку в режиме ZIP-215 —
    и вместе с ним уезжает чужой оплаченный доступ.
    """
    assert len(ed25519.SMALL_ORDER) == 9
    for pub in ed25519.SMALL_ORDER:
        assert ed25519.verify(bytes(64), b"anything", pub) is False
        assert ed25519.verify(pub + bytes(32), b"anything", pub) is False


def test_domain_separation():
    """Подпись для входа не должна годиться для отзыва устройства."""
    priv, pub = keypair()
    nonce = b"\x02" * 32
    sig = priv.sign(ed25519.auth_message(nonce, pub))
    assert ed25519.verify(sig, ed25519.auth_message(nonce, pub), pub) is True
    assert ed25519.verify(sig, ed25519.revoke_message(nonce, pub), pub) is False
    assert ed25519.verify(sig, ed25519.enroll_message(nonce, pub), pub) is False


def test_non_canonical_scalar_is_rejected():
    """S + L даёт вторую подпись того же сообщения. Строгий режим её не берёт."""
    priv, pub = keypair()
    msg = ed25519.auth_message(b"\x03" * 32, pub)
    sig = priv.sign(msg)
    s = int.from_bytes(sig[32:], "little")
    malleable = sig[:32] + (s + _L).to_bytes(32, "little")
    assert ed25519.verify(sig, msg, pub) is True
    assert ed25519.verify(malleable, msg, pub) is False


def test_wrong_key_and_malformed_input():
    priv, pub = keypair()
    _, other_pub = keypair()
    msg = ed25519.auth_message(b"\x04" * 32, pub)
    sig = priv.sign(msg)
    assert ed25519.verify(sig, msg, other_pub) is False
    assert ed25519.verify(sig[:-1], msg, pub) is False
    assert ed25519.verify(sig, msg, pub[:-1]) is False
    assert ed25519.verify(b"", b"", b"") is False


def test_non_canonical_y_is_rejected():
    """y >= p — вторая запись того же ключа. Одна личность, две строки в базе."""
    non_canonical = (2 ** 255 - 19 + 1).to_bytes(32, "little")
    assert ed25519.verify(bytes(64), b"x", non_canonical) is False
