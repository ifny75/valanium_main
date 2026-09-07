#!/bin/bash
# Setup script for VPN Node Agent (WireGuard) + Python
#
# Idempotent-ish: re-running won't regenerate the server's WireGuard key
# (the agent's ensure_base_config() also refuses to), but it WILL overwrite
# the systemd unit and venv. Existing peers.json / wg0.conf are left alone.

set -e

if [ "$EUID" -ne 0 ]; then
  echo "Please run as root"
  exit 1
fi

: "${API_KEY:?Set API_KEY in the environment before running this script}"
: "${SERVER_ENDPOINT_HOST:?Set SERVER_ENDPOINT_HOST (this node's public IP) before running}"
EGRESS_INTERFACE="${EGRESS_INTERFACE:-$(ip route show default | awk '/default/ {print $5; exit}')}"
WG_LISTEN_PORT="${WG_LISTEN_PORT:-51820}"

echo "Updating system..."
apt-get update

echo "Installing prerequisites..."
apt-get install -y wireguard wireguard-tools python3 python3-pip python3-venv iptables

echo "Enabling IPv4 forwarding..."
if ! grep -q '^net.ipv4.ip_forward=1' /etc/sysctl.conf; then
  echo 'net.ipv4.ip_forward=1' >> /etc/sysctl.conf
fi
sysctl -p

echo "Opening ${WG_LISTEN_PORT}/udp in ufw (if installed and active)..."
if command -v ufw >/dev/null && ufw status | grep -q "Status: active"; then
  ufw allow "${WG_LISTEN_PORT}/udp" comment "WireGuard"
fi

echo "Installing the tunnel's own resolver..."
# Без своего резолвера пришлось бы отдавать клиентам чужой, и все их запросы
# видел бы третий — ровно та связка «кто куда ходил», которой у сервиса быть
# не должно. Слушает только адрес узла в туннельной сети: открытый резолвер
# в интернете становится усилителем для чужих DDoS-атак.
WG_SUBNET_BASE="$(echo "${WG_SUBNET:-10.10.0.0/24}" | cut -d/ -f1 | cut -d. -f1-3)"
TUNNEL_DNS_ADDR="${WG_SUBNET_BASE}.1"
DEBIAN_FRONTEND=noninteractive apt-get install -y unbound
cat <<EOF > /etc/unbound/unbound.conf.d/valanium-tunnel.conf
server:
    interface: ${TUNNEL_DNS_ADDR}
    port: 53
    access-control: 0.0.0.0/0 refuse
    access-control: 127.0.0.0/8 allow
    access-control: ${WG_SUBNET_BASE}.0/24 allow

    do-ip4: yes
    do-ip6: no
    do-udp: yes
    do-tcp: yes

    hide-identity: yes
    hide-version: yes
    qname-minimisation: yes
    harden-glue: yes
    harden-dnssec-stripped: yes

    # Журнала запросов нет и не будет: он и есть та история, которой у
    # сервиса быть не должно.
    verbosity: 0
    log-queries: no
    use-syslog: yes

    cache-min-ttl: 60
    prefetch: yes
    num-threads: 1
    rrset-cache-size: 16m
    msg-cache-size: 8m
EOF
unbound-checkconf /etc/unbound/unbound.conf
systemctl enable --now unbound
systemctl restart unbound
if command -v ufw >/dev/null && ufw status | grep -q "Status: active"; then
  ufw allow in on "${WG_INTERFACE:-wg0}" from "${WG_SUBNET_BASE}.0/24" to "${TUNNEL_DNS_ADDR}" port 53 comment "tunnel DNS"
fi

echo "Deploying Node Agent..."
mkdir -p /opt/vpn_node
cp -r app /opt/vpn_node/
cp requirements.txt /opt/vpn_node/

cd /opt/vpn_node
python3 -m venv venv
./venv/bin/pip install -r requirements.txt

cat <<EOF > /etc/systemd/system/vpn_node.service
[Unit]
Description=VPN Node Agent (WireGuard)
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/vpn_node
Environment="API_KEY=${API_KEY}"
Environment="SERVER_ENDPOINT_HOST=${SERVER_ENDPOINT_HOST}"
Environment="EGRESS_INTERFACE=${EGRESS_INTERFACE}"
Environment="WG_LISTEN_PORT=${WG_LISTEN_PORT}"
ExecStart=/opt/vpn_node/venv/bin/uvicorn app.main:app --host 0.0.0.0 --port 8000
Restart=always

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable vpn_node
systemctl restart vpn_node

echo "Enabling BBR..."
if ! grep -q '^net.core.default_qdisc=fq' /etc/sysctl.conf; then
  cat <<EOF >> /etc/sysctl.conf
net.core.default_qdisc=fq
net.ipv4.tcp_congestion_control=bbr
EOF
  sysctl -p
fi

echo "Setup complete. vpn_node.service is running on :8000 and will create"
echo "/etc/wireguard/${WG_INTERFACE:-wg0}.conf + bring the interface up on first start."
systemctl status vpn_node --no-pager -l || true
