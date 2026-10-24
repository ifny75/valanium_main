"""Generates WireGuard-compatible X25519 keypairs.

Pure Python (via `cryptography`, already pulled in by python-jose) rather
than shelling out to the `wg` CLI — main_server doesn't need WireGuard
installed locally just to hand out client keys.
"""

import base64

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.x25519 import X25519PrivateKey


def generate_keypair() -> tuple[str, str]:
    """Returns (private_key_b64, public_key_b64) in the same base64 format
    `wg genkey`/`wg pubkey` produce."""
    private_key = X25519PrivateKey.generate()
    private_bytes = private_key.private_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PrivateFormat.Raw,
        encryption_algorithm=serialization.NoEncryption(),
    )
    public_bytes = private_key.public_key().public_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PublicFormat.Raw,
    )
    return (
        base64.b64encode(private_bytes).decode(),
        base64.b64encode(public_bytes).decode(),
    )
