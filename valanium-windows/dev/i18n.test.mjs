import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createTranslator } from '../src/i18n-core.js';
import english from '../src/locales/en.js';

test('English settings and message actions use the application catalog', () => {
  const { tr } = createTranslator(english, () => 'en');
  assert.equal(tr('Настройки'), 'Settings');
  assert.equal(tr('Копировать'), 'Copy');
  assert.equal(tr('Кто может обращаться'), 'Who can contact me');
});
test('switching back to Russian restores canonical interface strings', () => {
  let language = 'en';
  const { tr } = createTranslator(english, () => language);
  assert.equal(tr('Отмена'), 'Cancel');
  language = 'ru';
  assert.equal(tr('Отмена'), 'Отмена');
});
test('translated UI templates do not translate or rescan interpolated user content', () => {
  const { trTemplate } = createTranslator(english, () => 'en');
  const userMessage = 'Отмена Приватность {0} seed words stay unchanged';
  assert.equal(trTemplate(['<b>Отмена</b><p>', '</p>'], userMessage), `<b>Cancel</b><p>${userMessage}</p>`);
});
test('Android English resources cover every default resource and retain format placeholders', () => {
  const parse = file => new Map([...readFileSync(file, 'utf8').matchAll(/<string name="([^"]+)"[^>]*>([\s\S]*?)<\/string>/g)].map(m => [m[1], m[2]]));
  const root = new URL('../../valanium-android/app/src/main/res/', import.meta.url);
  const ru = parse(new URL('values/strings.xml', root)), en = parse(new URL('values-en/strings.xml', root));
  for (const [key, value] of ru) {
    assert.ok(en.has(key), `Missing English resource: ${key}`);
    const placeholders = text => [...text.matchAll(/%\d+\$[ds]/g)].map(m => m[0]).sort();
    assert.deepEqual(placeholders(en.get(key)), placeholders(value), key);
  }
});
