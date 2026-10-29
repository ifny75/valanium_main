<a id="top"></a>

<div align="center">

<img src="../valanium_main.png" alt="Valanium Server" width="100%" />

<h1>Инфраструктура зашифрованной доставки.</h1>

<p>WebSocket-релей Valanium.<br />Одноразовые билеты, очередь с TTL и подтверждения сообщений.</p>

<p>
  <a href="#quickstart"><img src="docs/assets/badges/node-js.svg" alt="Node.js: 24+" /></a>
  <a href="ARCHITECTURE.md"><img src="docs/assets/badges/transport.svg" alt="Transport: WebSocket" /></a>
  <a href="ARCHITECTURE.md"><img src="docs/assets/badges/storage.svg" alt="Storage: SQLite" /></a>
  <a href="../LICENSE"><img src="docs/assets/badges/license.svg" alt="License: AGPL v3" /></a>
</p>

<p>
  <a href="#features">Возможности</a> &nbsp; · &nbsp;
  <a href="#quickstart">Запуск</a> &nbsp; · &nbsp;
  <a href="../deploy/README.md">Развёртывание</a> &nbsp; · &nbsp;
  <a href="ARCHITECTURE.md">Архитектура</a>
</p>

</div>

<br />

<a id="features"></a>

## Принять. Сохранить. Доставить.

<table>
<tr>
<td colspan="2" valign="top">
<h3>Зашифрованные конверты</h3>
<p>Клиенты шифруют сообщения через MLS. Релей хранит очередь, не получая ключи личной переписки.</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<h3>Анонимная отправка</h3>
<p>Билеты для <b>SEND_ANON</b>, одноразовое onion-соединение и пропуска получателя без AUTH.</p>
</td>
<td width="50%" valign="top">
<h3>Управляемая очередь</h3>
<p>TTL, квоты, дедупликация повторов и ACK. Большая очередь продолжается без переподключения.</p>
</td>
</tr>
</table>

<a id="quickstart"></a>

## Запустить релей

Node.js **24 или новее**, npm и каталог данных, доступный на запись.
TypeScript исполняется средствами Node; `node:sqlite` входит в платформу.

```bash
npm ci --ignore-scripts
cp .env.example .env
# Настройте .env, пути хранения, публичные входы и секреты под своё окружение.
npm start
```

Конфигурация описана в `.env.example` и `src/config.ts`. Не добавляйте `.env`,
базы, ключи билетов и ключи подписи релизов в git. Production-настройка,
nginx, WireGuard и systemd — в [deploy/README.md](../deploy/README.md).

<a id="delivery"></a>

## Как проходит анонимная отправка

<table>
<tr>
<td colspan="2" valign="top">
<h3>01 · Билет</h3>
<p>Основная AUTH-сессия получает одноразовые билеты. Поддержка протокола объявляется в HELLO.</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<h3>02 · Конверт</h3>
<p>Отдельный onion-сокет предъявляет билет и, если нужен, пропуск. Сообщение отправляется без AUTH.</p>
</td>
<td width="50%" valign="top">
<h3>03 · Подтверждение</h3>
<p>SEND_OK подтверждает принятие сервером. ACK получателя удаляет его конверт из очереди.</p>
</td>
</tr>
</table>

<details>
<summary><b>Протокол доставки и наблюдаемые метаданные</b></summary>

- HELLO объявляет `sealedSender` и `sealedSenderPasses`.
- Билеты выдаются через основную AUTH-сессию; SEND_ANON идёт по отдельному
  onion-соединению. Пропуск получателя предъявляется там же без AUTH.
- Повторы одного конверта дедуплицируются; ACK проверяет владельца.
- Очередь больше 200 сообщений продолжается после ACK без переподключения.
  Backpressure обслуживается обработчиком `drain`.

Серверу по-прежнему видны адрес получателя, размер и время конверта.
Он не может гарантировать задержку Tor или скрыть глобальную корреляцию трафика.

</details>

<a id="registration"></a>

## Доступ и регистрация

Новый пользователь регистрируется по инвайту. Выпуск кода на машине сервера:

```bash
npm run invite
```

Код передаётся при AUTH; в БД хранится его хеш. TON — опциональный путь,
не включённый пустой конфигурацией. Установка без TON-зависимостей:

```bash
npm ci --ignore-scripts --omit=optional
```

Текущий Rust-клиент восстанавливает личность по seed-фразе. Исторические
серверные обработчики логина/пароля не означают доступность этой функции
в UI: старые команды клиента отклоняются с `seed_only`.

<a id="api"></a>

## HTTP API

| Метод <details>
<summary><b>Карта исходников</b></summary>

| Путь | Назначение |
| --- | --- | --- |
| GET | `/v1/health` | Живость сервера |
| GET | `/v1/releases/latest` | Метаданные клиентских сборок |
| POST | `/v1/support/inbound` | Входящая поддержка с отдельным Bearer-секретом |

</details>

Личные сообщения, KeyPackages и операции клиента проходят через WebSocket.
Старых HTTP-эндпоинтов загрузки/скачивания blobs нет.

<a id="checks"></a>

## Проверки перед развёртыванием

```bash
npm run check
npm test
npm run smoke
```

Smoke запускает локальный сервер и настоящий WebSocket-клиент. Успех
локальных тестов не подтверждает production-конфигурацию или доступность Tor.

<a id="ecosystem"></a>

## Часть экосистемы Valanium

<table>
<tr>
<td width="50%" valign="top">
<strong><a href="https://github.com/ifny75/valanium_main">Valanium Core</a></strong><br />
<sub>Rust-ядро, протокол и все компоненты проекта.</sub>
</td>
<td width="50%" valign="top">
<strong><a href="https://github.com/ifny75/valanium_server">Valanium Server</a></strong><br />
<sub>WebSocket-релей, очередь и развёртывание.</sub>
</td>
</tr>
<tr>
<td valign="top">
<strong><a href="https://github.com/ifny75/valanium_android">Valanium для Android</a></strong><br />
<sub>Java-интерфейс, JNI и встроенный Tor.</sub>
</td>
<td valign="top">
<strong><a href="https://github.com/ifny75/valanium_pc">Valanium для Windows</a></strong><br />
<sub>Tauri 2, WebView2 и общее Rust-ядро.</sub>
</td>
</tr>
</table>

Wire-форматы определены в `src/proto/frames.ts` и зеркале Rust-ядра.
Изменения протокола нужно согласовывать с клиентами и серверной копией
в `valanium_main/valanium-server`.

<a id="license"></a>

## Лицензия

[GNU Affero General Public License v3.0](../LICENSE).
Зависимости сохраняют собственные лицензии. Название и знаки проекта —
[TRADEMARK.md](../TRADEMARK.md).

<div align="center">
<sub>Valanium · Android · Windows · Rust</sub><br />
<a href="#top">Вернуться к началу ↑</a>
</div>
