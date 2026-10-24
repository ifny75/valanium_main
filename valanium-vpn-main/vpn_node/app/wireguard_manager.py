"""Manages a single WireGuard interface's peer list.

Replaces caddy_manager.py — instead of Caddy's admin JSON API, this shells
out to the `wg`/`wg-quick` CLI, the standard way to manage WireGuard on
Linux. Peer metadata (which client owns which allowed-IP, a human label,
when it was added) lives in a small JSON sidecar file next to the conf,
since raw public keys are awkward to use as URL path segments (they contain
`/`, `+`, `=`) and the wg conf format itself has no room for metadata.

wg0.conf is treated as generated output, not hand-edited: on every
mutation we rewrite it in full from (interface template + peers.json) and
push the change live with `wg syncconf`, which updates only what changed
and does not drop other peers' active sessions — unlike `wg-quick down/up`.
"""

import asyncio
import ipaddress
import json
import os
import time
import uuid
from pathlib import Path
from typing import Any, Dict, List, Optional

from .config import settings


class WireGuardManager:
    def __init__(
        self,
        interface: str,
        config_path: str,
        peers_store: str,
        server_key_path: str,
        subnet: str,
        listen_port: int,
    ) -> None:
        self.interface = interface
        self.config_path = Path(config_path)
        self.interface_template_path = self.config_path.with_suffix(".interface.conf")
        self.peers_store = Path(peers_store)
        self.server_key_path = Path(server_key_path)
        self.subnet = ipaddress.ip_network(subnet)
        self.listen_port = listen_port
        self._lock = asyncio.Lock()

    # ---------- process helpers ----------

    async def _run(self, *args: str, input_text: Optional[str] = None) -> str:
        proc = await asyncio.create_subprocess_exec(
            *args,
            stdin=asyncio.subprocess.PIPE if input_text is not None else None,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await proc.communicate(input_text.encode() if input_text else None)
        if proc.returncode != 0:
            raise RuntimeError(f"{' '.join(args)} failed: {stderr.decode().strip()}")
        return stdout.decode()

    async def _sync(self) -> None:
        """Push the on-disk conf to the live interface without dropping peers."""
        stripped = await self._run("wg-quick", "strip", str(self.config_path))
        # `wg syncconf` calls fopen() on its path argument — a real temp file,
        # not a pipe, since /dev/stdin under an asyncio-piped subprocess isn't
        # a seekable/openable path the way a shell heredoc's would be.
        tmp_path = self.config_path.with_suffix(".sync.tmp")
        tmp_path.write_text(stripped)
        try:
            await self._run("wg", "syncconf", self.interface, str(tmp_path))
        finally:
            tmp_path.unlink(missing_ok=True)

    # ---------- bootstrap ----------

    async def ensure_base_config(self) -> bool:
        """Create server keys / conf / peers store if this is a fresh node,
        then make sure the interface is actually up. Never overwrites an
        existing server private key."""
        try:
            self.config_path.parent.mkdir(parents=True, exist_ok=True)

            if not self.server_key_path.exists():
                private_key = (await self._run("wg", "genkey")).strip()
                self.server_key_path.write_text(private_key + "\n")
                os.chmod(self.server_key_path, 0o600)

            if not self.peers_store.exists():
                self.peers_store.write_text("[]")

            if not self.interface_template_path.exists():
                private_key = self.server_key_path.read_text().strip()
                server_ip = next(self.subnet.hosts())
                self.interface_template_path.write_text(
                    "[Interface]\n"
                    f"PrivateKey = {private_key}\n"
                    f"Address = {server_ip}/{self.subnet.prefixlen}\n"
                    f"ListenPort = {self.listen_port}\n"
                    f"PostUp = iptables -A FORWARD -i {self.interface} -j ACCEPT; "
                    f"iptables -t nat -A POSTROUTING -o {settings.EGRESS_INTERFACE} -j MASQUERADE\n"
                    f"PostDown = iptables -D FORWARD -i {self.interface} -j ACCEPT; "
                    f"iptables -t nat -D POSTROUTING -o {settings.EGRESS_INTERFACE} -j MASQUERADE\n"
                )
                os.chmod(self.interface_template_path, 0o600)

            await self._render_conf()

            # Bring the interface up if it isn't already (fresh node); if it's
            # already running, leave it alone and just sync — avoids dropping
            # existing peers on an agent restart.
            up = True
            try:
                await self._run("wg", "show", self.interface)
            except RuntimeError:
                up = False
            if not up:
                await self._run("wg-quick", "up", str(self.config_path))
            else:
                await self._sync()
            return True
        except Exception as e:  # pragma: no cover - startup diagnostics only
            print(f"Warning: failed to ensure base WireGuard config: {e}")
            return False

    # ---------- peer store ----------

    def _load_peers(self) -> List[Dict[str, Any]]:
        if not self.peers_store.exists():
            return []
        return json.loads(self.peers_store.read_text() or "[]")

    def _save_peers(self, peers: List[Dict[str, Any]]) -> None:
        self.peers_store.write_text(json.dumps(peers, indent=2))

    async def _render_conf(self) -> None:
        interface_block = self.interface_template_path.read_text()
        peers = self._load_peers()
        blocks = [interface_block.rstrip()]
        for p in peers:
            block = f"\n\n[Peer]\n# id: {p['id']} label: {p.get('label', '')}\nPublicKey = {p['public_key']}\n"
            if p.get("preshared_key"):
                block += f"PresharedKey = {p['preshared_key']}\n"
            block += f"AllowedIPs = {p['allowed_ip']}/32\n"
            blocks.append(block)
        self.config_path.write_text("".join(blocks) + "\n")
        os.chmod(self.config_path, 0o600)

    def _next_free_ip(self, peers: List[Dict[str, Any]]) -> str:
        used = {ipaddress.ip_interface(f"{p['allowed_ip']}/32").ip for p in peers}
        used.add(next(self.subnet.hosts()))  # server's own address
        for host in self.subnet.hosts():
            if host not in used:
                return str(host)
        raise RuntimeError("WireGuard subnet exhausted — no free client IPs left")

    # ---------- public API ----------

    async def add_peer(
        self, public_key: str, label: str = "", preshared_key: Optional[str] = None
    ) -> Dict[str, Any]:
        async with self._lock:
            peers = self._load_peers()
            if any(p["public_key"] == public_key for p in peers):
                raise ValueError("That public key is already registered")
            peer = {
                "id": uuid.uuid4().hex,
                "public_key": public_key,
                "preshared_key": preshared_key,
                "allowed_ip": self._next_free_ip(peers),
                "label": label,
                "created_at": int(time.time()),
            }
            peers.append(peer)
            self._save_peers(peers)
            await self._render_conf()
            await self._sync()
            server_public_key = (
                await self._run("wg", "pubkey", input_text=self.server_key_path.read_text().strip())
            ).strip()
            return {
                "id": peer["id"],
                "allowed_ip": peer["allowed_ip"],
                "server_public_key": server_public_key,
                "endpoint": f"{settings.SERVER_ENDPOINT_HOST}:{self.listen_port}",
            }

    async def remove_peer(self, peer_id: str) -> None:
        async with self._lock:
            peers = self._load_peers()
            remaining = [p for p in peers if p["id"] != peer_id]
            if len(remaining) == len(peers):
                raise ValueError("Unknown peer id")
            self._save_peers(remaining)
            await self._render_conf()
            await self._sync()

    async def list_peers(self) -> List[Dict[str, Any]]:
        return [
            {k: v for k, v in p.items() if k != "preshared_key"} for p in self._load_peers()
        ]

    async def get_transfer(self, peer_id: str) -> Dict[str, int]:
        peers = self._load_peers()
        peer = next((p for p in peers if p["id"] == peer_id), None)
        if peer is None:
            raise ValueError("Unknown peer id")
        dump = await self._run("wg", "show", self.interface, "dump")
        for line in dump.splitlines()[1:]:  # first line is the interface itself
            cols = line.split("\t")
            if len(cols) >= 7 and cols[0] == peer["public_key"]:
                return {"bytes_down": int(cols[5]), "bytes_up": int(cols[6])}
        return {"bytes_down": 0, "bytes_up": 0}
