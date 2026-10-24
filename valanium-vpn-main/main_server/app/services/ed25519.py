"""Строгая проверка подписи Ed25519 и доменные префиксы.

Проверка обязана быть строгой по RFC 8032. Распространённый режим ZIP-215
принимает точки малого порядка: ключ из одних нулей вместе с подписью из
одних нулей проходит проверку. Такую «личность» подпишет кто угодно — а
значит, кто угодно заберёт себе чужой оплаченный доступ, зарегистрировав
устройство на этот ключ.

Три проверки до вызова OpenSSL закрывают ровно это:

1. публичный ключ не из списка девяти кодировок точек малого порядка;
2. `y` в кодировке ключа меньше `p` — неканоническая запись отвергается,
   иначе один и тот же ключ имеет два представления;
3. `S < L` — иначе к валидной подписи можно прибавить порядок группы и
   получить вторую, тоже проходящую, подпись того же сообщения.

Список из девяти кодировок — тот же, что в мессенджере
(`valanium-server/test/protocol.test.ts`). Он общий намеренно: две
реализации, расходящиеся в том, какой ключ считать настоящим, — это дыра,
которую не видно ни с одной стороны по отдельности.
"""

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

# Порядок подгруппы и простое поля.
_L = 2 ** 252 + 27742317777372353535851937790883648493
_P = 2 ** 255 - 19

SMALL_ORDER = frozenset(
    bytes.fromhex(h)
    for h in (
        "0000000000000000000000000000000000000000000000000000000000000000",
        "0100000000000000000000000000000000000000000000000000000000000000",
        "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
        "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
        "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85",
        "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa",
        "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
        "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
        "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
    )
)

# Домены разные у каждого действия и не совпадают с мессенджерскими:
# подпись, открывающая VPN, не должна открывать переписку, и наоборот.
DOMAIN_ENROLL = b"valanium-vpn-enroll-v1"
DOMAIN_AUTH = b"valanium-vpn-auth-v1"
DOMAIN_REVOKE = b"valanium-vpn-device-revoke-v1"


def verify(signature: bytes, message: bytes, public_key: bytes) -> bool:
    """Ничего не бросает: кривой ключ, кривая подпись, чужая подпись — False."""
    if len(signature) != 64 or len(public_key) != 32:
        return False
    if public_key in SMALL_ORDER:
        return False
    y = int.from_bytes(public_key, "little") & ((1 << 255) - 1)
    if y >= _P:
        return False
    s = int.from_bytes(signature[32:], "little")
    if s >= _L:
        return False
    try:
        Ed25519PublicKey.from_public_bytes(public_key).verify(signature, message)
        return True
    except (InvalidSignature, ValueError):
        return False


def enroll_message(nonce: bytes, device_pub: bytes) -> bytes:
    """`sign(device_priv, "valanium-vpn-enroll-v1" || nonce || device_pub)`"""
    return DOMAIN_ENROLL + nonce + device_pub


def auth_message(nonce: bytes, device_pub: bytes) -> bytes:
    """`sign(device_priv, "valanium-vpn-auth-v1" || nonce || device_pub)`"""
    return DOMAIN_AUTH + nonce + device_pub


def revoke_message(nonce: bytes, target_pub: bytes) -> bytes:
    """`sign(device_priv, "valanium-vpn-device-revoke-v1" || nonce || target_pub)`

    Отзыв подписывается ключом устройства, которое отзывает: корневого ключа
    личности здесь нет — корень доступа это код, а он потрачен. Цена честная
    и такая же, как у Mullvad: укравший разлоченное устройство может выкинуть
    остальные. Защита от этого — не подпись, а то, что список устройств виден
    в клиенте, и чужое там заметно.
    """
    return DOMAIN_REVOKE + nonce + target_pub
