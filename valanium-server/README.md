<div align="center">
  <img src="../valanium_main.png" alt="Valanium server" width="100%" />
</div>

# Valanium Server

WebSocket-релей для Valanium: принимает зашифрованные MLS-конверты, хранит
очередь с TTL и доставляет их устройствам до ACK. Ключей личной переписки
у релея нет. Метаданные доставки, открытые каналы и поддержка имеют другие
границы приватности — см. [архитектуру](ARCHITECTURE.md).

## Требования и запуск

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

## Регистрация и совместимость

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

## Анонимная доставка

- HELLO объявляет `sealedSender` и `sealedSenderPasses`.
- Билеты выдаются через основную AUTH-сессию; SEND_ANON идёт по отдельному
  onion-соединению. Пропуск получателя предъявляется там же без AUTH.
- Повторы одного конверта дедуплицируются; ACK проверяет владельца.
- Очередь больше 200 сообщений продолжается после ACK без переподключения.
  Backpressure обслуживается обработчиком `drain`.

Серверу по-прежнему видны адрес получателя, размер и время конверта.
Он не может гарантировать задержку Tor или скрыть глобальную корреляцию трафика.

## HTTP

| Метод | Путь | Назначение |
| --- | --- | --- |
| GET | `/v1/health` | Живость сервера |
| GET | `/v1/releases/latest` | Метаданные клиентских сборок |
| POST | `/v1/support/inbound` | Входящая поддержка с отдельным Bearer-секретом |

Личные сообщения, KeyPackages и операции клиента проходят через WebSocket.
Старых HTTP-эндпоинтов загрузки/скачивания blobs нет.

## Проверки

```bash
npm run check
npm test
npm run smoke
```

Smoke запускает локальный сервер и настоящий WebSocket-клиент. Успех
локальных тестов не подтверждает production-конфигурацию или доступность Tor.

## Связанные репозитории

| Репозиторий | Назначение |
| --- | --- |
| [valanium_main](https://github.com/ifny75/valanium_main) | Rust-ядро, клиенты и сервер в одном дереве |
| [valanium_server](https://github.com/ifny75/valanium_server) | Отдельный репозиторий релея |
| [valanium_android](https://github.com/ifny75/valanium_android) | Android-клиент и JNI |
| [valanium_pc](https://github.com/ifny75/valanium_pc) | Windows-клиент на Tauri |

Wire-форматы определены в `src/proto/frames.ts` и зеркале Rust-ядра.
Изменения протокола нужно согласовывать с клиентами и серверной копией
в `valanium_main/valanium-server`.

## Лицензия

[GNU Affero General Public License v3.0](../LICENSE).
Зависимости сохраняют собственные лицензии. Название и знаки проекта —
[TRADEMARK.md](../TRADEMARK.md).
