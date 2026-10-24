"""HTTPS-masking relay for WireGuard: a real TLS handshake (self-signed cert,
CN is cosmetic only — the client never validates it) sits in front of the
node's real WireGuard UDP port. Passive DPI that only inspects the visible
TLS ClientHello (SNI + cipher list) sees an ordinary-looking HTTPS
connection; it never reaches the WireGuard handshake bytes at all.

Wire format is intentionally trivial: once the TLS connection is up, each
WireGuard UDP datagram is sent as [2-byte big-endian length][payload], in
both directions, over that one TLS/TCP stream. One TCP connection = one
client's whole session (WireGuard's own protocol multiplexes real clients
by public key, not by the loopback source port each session gets here).

Listens on the same public port (443) as the direct UDP path, just on TCP —
UDP:443 (existing WireGuard listener) and TCP:443 (this relay) don't
collide since they're different socket namespaces. Clients that enable
"mask as HTTPS" connect here instead of hitting WireGuard's UDP port
directly.
"""
import asyncio
import ssl
import struct
import logging

logger = logging.getLogger(__name__)

LISTEN_PORT = 443
CERT_PATH = "/etc/valanium-mask/cert.pem"
KEY_PATH = "/etc/valanium-mask/key.pem"
WG_UDP_TARGET = ("127.0.0.1", 443)


class RelayUDPProtocol(asyncio.DatagramProtocol):
    def __init__(self, tcp_writer: asyncio.StreamWriter):
        self.tcp_writer = tcp_writer

    def datagram_received(self, data: bytes, addr):
        try:
            self.tcp_writer.write(struct.pack(">H", len(data)) + data)
        except Exception:
            pass


async def handle_client(reader: asyncio.StreamReader, writer: asyncio.StreamWriter):
    loop = asyncio.get_event_loop()
    peer = writer.get_extra_info("peername")
    logger.info(f"mask relay: TLS client connected from {peer}")

    udp_transport, _ = await loop.create_datagram_endpoint(
        lambda: RelayUDPProtocol(writer),
        remote_addr=WG_UDP_TARGET,
    )
    try:
        while True:
            header = await reader.readexactly(2)
            (length,) = struct.unpack(">H", header)
            payload = await reader.readexactly(length)
            udp_transport.sendto(payload)
    except (asyncio.IncompleteReadError, ConnectionResetError, ssl.SSLError):
        pass
    finally:
        udp_transport.close()
        writer.close()
        logger.info(f"mask relay: TLS client {peer} disconnected")


async def main():
    logging.basicConfig(level=logging.INFO)
    ssl_ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ssl_ctx.load_cert_chain(CERT_PATH, KEY_PATH)
    server = await asyncio.start_server(handle_client, "0.0.0.0", LISTEN_PORT, ssl=ssl_ctx)
    logger.info(f"HTTPS-mask relay listening on :{LISTEN_PORT} -> {WG_UDP_TARGET}")
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    asyncio.run(main())
