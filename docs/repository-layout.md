# Полный проект в main

Ветка main содержит проект целиком. Ядро находится в `valanium-core/`; команды
для него запускаются с `--manifest-path valanium-core/Cargo.toml` либо из этого
каталога. Android, Windows, сайт, relay, Onionize и VPN лежат в соседних каталогах,
перечисленных в основном README.

При объединении сохранены обе истории: прежний main с отдельным ядром и ветка
клиентских изменений. Исходники ядра из прежнего main проверены на соответствие
локальной базе до изменений приватности; они уже включены в `valanium-core/`.
LICENSE, COPYRIGHT, TRADEMARK.md и изображение из main сохранены.

Отдельные репозитории компонентов:

- https://github.com/ifny75/valanium_server
- https://github.com/ifny75/valanium_android
- https://github.com/ifny75/valanium_pc
- https://github.com/valanium-project/valanium-onionize

Локальные снимки телефона, дампы экранов, установочные APK, базы и каталоги сборки
не входят в исходники. Вложенный `.git` Onionize остаётся локальным; его исходники
в main отслеживаются как обычные файлы.
