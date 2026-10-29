<a id="top"></a>

<div align="center">

<img src="../valanium_main.png" alt="Valanium для Windows" width="100%" />

<h1>Valanium для Windows</h1>

<p>Переписка на большом экране.<br />WebView2 для интерфейса. Rust для ключей, истории и доставки.</p>

<p>
  <a href="#build"><img src="docs/assets/badges/desktop.svg" alt="Desktop: Windows" /></a>
  <a href="ARCHITECTURE.md"><img src="docs/assets/badges/ui.svg" alt="UI: Tauri 2" /></a>
  <a href="ARCHITECTURE.md"><img src="docs/assets/badges/e2ee.svg" alt="E2EE: MLS" /></a>
  <a href="../LICENSE"><img src="docs/assets/badges/license.svg" alt="License: AGPL v3" /></a>
</p>

<p>
  <a href="#features">Возможности</a> &nbsp; · &nbsp;
  <a href="#build">Сборка</a> &nbsp; · &nbsp;
  <a href="#checks">Проверки</a> &nbsp; · &nbsp;
  <a href="ARCHITECTURE.md">Архитектура</a>
</p>

</div>

<br />

<a id="features"></a>

## Больше пространства для общения.

<table>
<tr>
<td colspan="2" valign="top">
<h3>На рабочем столе</h3>
<p>Tauri 2 и WebView2. Сообщения, история и меню действий в одном интерфейсе.</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<h3>Криптография в Rust</h3>
<p>Общее ядро отвечает за MLS, ключи, запечатанную SQLite и сетевые задачи.</p>
</td>
<td width="50%" valign="top">
<h3>Управление сообщениями</h3>
<p>Контекстное меню и свайп к центру. Редактирование доступно для своих подходящих сообщений.</p>
</td>
</tr>
</table>

<p align="center">
  <img src="../resource/laptop_valanium.png" alt="Фирменный визуальный материал Valanium для Windows" width="600" />
</p>

<a id="build"></a>

## Собрать Windows-клиент

Windows, WebView2, Rust со средой сборки MSVC, Node.js и npm.
Для согласованности инструментов проекта рекомендуется Node.js 24+.

```powershell
npm ci
npm run dev
```

Сборка приложения:

```powershell
npm run build
```

Скрипт использует `tauri build --no-bundle`: MSI/NSIS им не создаются.
Создание установщиков и подпись релиза — отдельные шаги выпуска.
Фронтенд находится в `src/`, отдельного сборщика JavaScript нет.
Версия клиента задана в `src-tauri/Cargo.toml`.

<details>
<summary><b>Проверить двух независимых клиентов на одной машине</b></summary>

Для двух независимых личностей нужны разные базы. В отдельных консолях:

```powershell
$env:VALANIUM_DB = "$env:TEMP\valanium-a.db"
.\valanium-windows.exe
```

```powershell
$env:VALANIUM_DB = "$env:TEMP\valanium-b.db"
.\valanium-windows.exe
```

Зарегистрируйте каждую тестовую личность по инвайту, обменяйтесь адресами и
сверьте отпечатки по независимому каналу. Не используйте личную базу как тестовую.
Восстановление личности по seed-фразе не восстанавливает потерянную историю.

</details>

<a id="experience"></a>

## Сообщения и подключение

Onion использует встроенный компонент Tor либо настроенный SOCKS5;
имена `.onion` разрешает Tor, не системный DNS. Наличие старого пункта
Multi-hop в интерфейсе не подтверждает работоспособность этого маршрута.
Анонимные сообщения используют отдельное одноразовое соединение без AUTH,
запечатанную очередь и дедлайны повторов, независимые от heartbeat.
Подробности и ограничения — в архитектуре ядра.

Меню сообщения открывается правой кнопкой или свайпом к центру:
исходящие — влево, входящие — вправо. Сдвиг ограничен видимой областью
списка, даже если логический жест длиннее. Короткие входящие сообщения
поддерживают тот же жест. Отмена и вертикальная прокрутка не открывают меню.
Редактирование предлагается только для своих подходящих сообщений.

<a id="checks"></a>

## Проверки и структура

```bash
node --check src/main.js
npm test
```

Тесты pointer-жестов запускаются на Node без Windows. Они проверяют границы,
короткие входящие сообщения, отмену, направление и идентификатор указателя.
Это не проверка WebView2, системного трея или готового Windows-приложения.

<details>
<summary><b>Карта исходников</b></summary>

| Путь | Назначение |
| --- | --- |
| `src/index.html` | Разметка экранов |
| `src/styles.css` | Тема и адаптивная вёрстка |
| `src/main.js` | Команды, события, отрисовка и действия сообщений |
| `src-tauri/src/main.rs` | Tauri, ядро и интеграция с Windows |
| `test/message-swipe.test.cjs` | Регрессии жестов сообщений |

</details>

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

Ядро подключено локально из `../valanium-core`; отдельный git-репозиторий клиента использует закреплённый SHA.

Исправление интерфейса переносится также в монорепозиторий. Изменения в git
не заменяют установленное приложение: нужна новая проверенная сборка.

<a id="license"></a>

## Лицензия

[GNU Affero General Public License v3.0](../LICENSE).
Авторские права — [COPYRIGHT](../COPYRIGHT).
Названия и знаки проекта — [../TRADEMARK.md](../TRADEMARK.md).

<div align="center">
<sub>Valanium · Android · Windows · Rust</sub><br />
<a href="#top">Вернуться к началу ↑</a>
</div>
