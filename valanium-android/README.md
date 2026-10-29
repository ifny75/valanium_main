<a id="top"></a>

<div align="center">

<img src="../valanium_main.png" alt="Valanium для Android" width="100%" />

<h1>Valanium для Android</h1>

<p>Личная переписка в нативном интерфейсе.<br />Java, Rust-ядро и встроенный Tor.</p>

<p>
  <a href="#build"><img src="docs/assets/badges/android.svg" alt="Android: 8.0+" /></a>
  <a href="#build"><img src="docs/assets/badges/abi.svg" alt="ABI: arm64--v8a" /></a>
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

## Переписка, которая всегда под рукой.

<table>
<tr>
<td colspan="2" valign="top">
<h3>Нативный интерфейс</h3>
<p>Java UI, переписка, фотографии и голосовые сообщения. Жесты остаются внутри границ строки.</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<h3>Локальные секреты</h3>
<p>Случайный ключ запечатанной базы защищён <b>Android Keystore</b>.</p>
</td>
<td width="50%" valign="top">
<h3>Общее ядро</h3>
<p>MLS, очередь доставки и Tor работают в Rust. JSON-команды связывают интерфейс с ядром через JNI.</p>
</td>
</tr>
</table>

<p align="center">
  <img src="../resource/valaniumChat_phone.png" alt="Визуальный концепт мобильного интерфейса Valanium" width="460" />
</p>

<p align="center"><sub>Концепт интерфейса из материалов проекта. Внешний вид зависит от сборки.</sub></p>

<a id="build"></a>

## Собрать Android-клиент

Android 8.0/API 26 или новее; текущая конфигурация APK — `arm64-v8a`.
Для сборки нужны JDK 17+, Android SDK 35, NDK, Rust и cargo-ndk.

```bash
sdkmanager "platforms;android-35" "build-tools;35.0.0" "ndk;27.2.12479018"
rustup target add aarch64-linux-android
cargo install cargo-ndk
./gradlew :app:assembleDebug
```

Gradle вызывает задачу `cargoNdk`, собирает библиотеку с `tor-embedded`
и копирует её в `app/src/main/jniLibs`. APK появляется в
`app/build/outputs/apk/debug/`. Наличие этого пути не означает, что сборка
уже выполнена и проверена на устройстве.

Release собирается неподписанным. Подпись и сохранение совместимой родословной
ключей выполняются отдельно по процедуре выпуска. Не заменяйте рабочую
установку отладочной сборкой и не удаляйте её данные ради обновления.

<a id="experience"></a>

## В повседневном использовании

Своё сообщение сдвигается влево, входящее — вправо, к центру экрана.
Область жеста занимает всю строку; смещение пузыря ограничено её границами.
Расстояние для выбора действия считается отдельно от видимого смещения:
длинный пузырь не вылезает за край, короткий входящий не мешает жесту.
Вертикальное движение остаётся прокруткой, отмена не запускает действие.

В Android-копии монорепозитория расстояние свайпа выбирает действие: ответ, реакция, копирование, пересылка, редактирование своего текста или удаление. Доступные пункты зависят от типа сообщения и правил приватности.

<details>
<summary><b>Вход, восстановление личности и сеть</b></summary>

Новая регистрация использует инвайт и ключи личности/устройства.
Восстановление личности — по секретной фразе из 24 слов; она не возвращает
историю, если утрачена локальная база. При миграции старой базы может
потребоваться прежний локальный пароль. Это не серверный вход по паролю.

Ядро поддерживает onion через SOCKS5 и встроенный Tor; доступные пункты
маршрутизации зависят от версии интерфейса. Старые названия Basic/Multi-hop
не доказывают, что соответствующий relay-путь работает. Анонимная доставка
не переходит на AUTH при неудаче: конверт сохраняется и повторяется отдельно.
Foreground-сервис помогает сохранять связь, но Doze и отсутствие сети
по-прежнему могут задерживать сообщения.

</details>

<a id="checks"></a>

## Проверки и структура

<details>
<summary><b>Карта исходников</b></summary>

| Путь | Назначение |
| --- | --- |
| `app/src/main/java/app/valanium/MainActivity.java` | Экраны и строки сообщений |
| `app/src/main/java/app/valanium/SwipeMessageLayout.java` | Область жеста и визуальные границы |
| `app/src/main/java/app/valanium/MessageSwipe.java` | Логика направления, выбора и отмены без Android API |
| `app/src/main/java/app/valanium/ValaniumService.java` | Жизненный цикл ядра |
| `app/src/main/java/app/valanium/LocalSecretStore.java` | Секрет базы и Keystore |
| `rust` | JNI-мост; JSON-команды и опрос событий |

</details>

```bash
./gradlew :app:testDebugUnitTest
# Тесты на отдельной QA-установке, а не на личной базе:
./gradlew -PisolatedTest :app:connectedDebugAndroidTest
```

Обе команды требуют настроенного SDK/NDK из-за задачи сборки JNI.
`MessageSwipeTest` также можно выполнить обычным JUnit 4: ему нужен только
`MessageSwipe.java`, без устройства. Проверка `cargo check` JNI на компьютере
не заменяет сборку Android ABI и тесты на настоящем телефоне.

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

Копии интерфейса в монорепозитории и отдельном репозитории могут различаться.
Изменение файлов на GitHub не заменяет установленный APK.

<a id="license"></a>

## Лицензия

[GNU Affero General Public License v3.0](../LICENSE).
Авторские права — [COPYRIGHT](../COPYRIGHT).
Названия и знаки проекта — [../TRADEMARK.md](../TRADEMARK.md).

<div align="center">
<sub>Valanium · Android · Windows · Rust</sub><br />
<a href="#top">Вернуться к началу ↑</a>
</div>
