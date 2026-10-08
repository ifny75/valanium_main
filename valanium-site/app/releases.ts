/**
 * Что именно раздаётся с сайта.
 *
 * Один источник на все страницы: раньше номера версий стояли по месту, и
 * страница канала уже успела сослаться на сборку, которой на сервере нет.
 * Файлы лежат в /opt/valanium-releases и раздаются nginx по /downloads/.
 */
export const RELEASES = {
  windows: { version: '0.13.4', size: '15,7 МБ', file: '/downloads/Valanium-0.13.4.exe' },
  android: { version: '0.7.3', size: '8,0 МБ', file: '/downloads/Valanium-0.7.3.apk' },
};
