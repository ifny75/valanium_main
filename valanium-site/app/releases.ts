/**
 * Что именно раздаётся с сайта.
 *
 * Один источник на все страницы: раньше номера версий стояли по месту, и
 * страница канала уже успела сослаться на сборку, которой на сервере нет.
 * Файлы лежат в /opt/valanium-releases и раздаются nginx по /downloads/.
 */
export const RELEASES = {
  windows: { version: '0.13.13', size: '15,8 МБ', file: '/downloads/Valanium-0.13.13.exe' },
  android: { version: '0.7.12', size: '8,3 МБ', file: '/downloads/Valanium-0.7.12.apk' },
};
