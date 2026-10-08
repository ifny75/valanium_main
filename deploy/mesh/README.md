# Valanium mesh

Текущая схема (06.10.2026):

```text
Relay: клиент → Cloudflare → hop1 или hop3 → WireGuard → main
Onion: клиент → Tor → скрытый сервис hop1 или hop3 → WireGuard → main
```

В сети WireGuard `10.77.0.0/24` сейчас используются `10.77.0.1` (main),
`10.77.0.2` (hop1) и `10.77.0.4` (hop3). Публичные адреса серверов хранятся
в закрытом инвентаре. Второй этап Multi-hop пока не настроен. Путь
`/multihop/` отвечает 503: он не должен выдавать один relay за два.

На hop1 и hop3 работают `cloudflared`, `nginx`, `tor@default` и WireGuard.
На hop3 Nginx стартует после `wg-quick@wg0`, чтобы listener не опережал
появление адреса `10.77.0.4`.
На main Nginx также ждёт `wg-quick@wg0`: его listener привязан к
`10.77.0.1:8080`. После перезагрузки 08.10.2026 без этой зависимости
Nginx не поднялся, и сайт отвечал 502. Установите
`nginx-main-wg0.conf` как `/etc/systemd/system/nginx.service.d/wg0.conf`,
затем выполните `systemctl daemon-reload` и `systemctl restart nginx`.
Службы прежнего VPN (`vpn_node`, `main_server`, `tls-mask-relay`) остановлены
и отключены. Публичный firewall оставляет SSH и WireGuard от main. Клиенты
мессенджера выбирают Auto, Relay или Onion. При обновлении сохранённый
Multi-hop переносится на Relay с уведомлением пользователя.

`cloudflared` соединяется с nginx на `127.0.0.1:8080`, а скрытые сервисы Tor
направляют запросы на `127.0.0.1:8082`. Оба входа проксируются через
WireGuard на `10.77.0.1:8080`. Main не принимает WebSocket напрямую из
публичной сети. Доступный onion-адрес находится в подписанном HELLO сервера.

Nginx на каждом входном relay заменяет адрес клиента на суточный HMAC-жетон, прежде чем
передать запрос main. Для Onion используется общая метка `onion`. Cloudflare
и выбранный входной relay видят сетевой адрес клиента на Relay; Tor-вход скрывает его от
инфраструктуры Valanium на Onion. Main видит только жетон либо метку.

Секрет `/etc/nginx/valanium-blind.key` уникален для входного узла и не
копируется на main. Nginx не пишет адрес клиента и User-Agent в access log;
для `/ws` access log отключён. Настройки прокси-заголовков находятся в
`valanium-proxy.conf`.

Проверка после изменения маршрута:

```bash
# На hop1 — wg-quick@wg-mesh, на hop3 — wg-quick@wg0.
systemctl is-active cloudflared nginx tor@default
curl -fsS http://10.77.0.1:8080/v1/health
curl -fsS http://127.0.0.1:8080/v1/health
curl -fsS --socks5-hostname 127.0.0.1:9050 \
  http://5kghvwyxzmtzba4foenmg5pkhcoxv6iq2c6wf4pbg5uyjrviwkckvead.onion/v1/health
curl -fsS --socks5-hostname 127.0.0.1:9050 \
  http://5amnu2di3yhtpqcpbcoaabfbzotw3giap2lvoe5bi5juflzhzdrsq4ad.onion/v1/health
```

Снаружи также проверяются `https://valanium.com/v1/health` и WebSocket
`wss://valanium.com/ws`. Подписанный onion-список на main задаётся
`VALANIUM_ONION_HOSTS`, `VALANIUM_ONION_SIG` и `VALANIUM_ONION_ISSUED_AT`.

Ежедневный снимок базы main отправляется зашифрованным на hop3 через
`valanium-offsite.service`. Ключ расшифровки хранится на ПК владельца
в `~/.obsidian-backup/age-identity.txt`; на relay находится только шифротекст.
