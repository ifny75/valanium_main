import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';

const source = readFileSync(new URL('../src/main.js', import.meta.url), 'utf8');
function functionText(name, next) {
  return source.slice(source.indexOf(`function ${name}(`), source.indexOf(next, source.indexOf(`function ${name}(`)));
}
function preferences(saved) {
  const storage = new Map([['valanium.preferences', JSON.stringify(saved)]]);
  const context = vm.createContext({ preferenceDefaults: { transport: 'onion', accent: '#7b2cff' },
    ROUTE_MIGRATION_NOTICE: 'migration', localStorage: {
      getItem: key => storage.get(key), setItem: (key, value) => storage.set(key, value),
    } });
  vm.runInContext(functionText('loadPreferences', '\nlet preferences ='), context);
  return vm.runInContext('loadPreferences()', context);
}
test('new and previous default installations use onion; explicit choices survive migration', () => {
  assert.equal(preferences({}).transport, 'onion');
  assert.equal(preferences({ transport: 'auto' }).transport, 'onion');
  assert.equal(preferences({ transport: 'basic' }).transport, 'basic');
  assert.equal(preferences({ transport: 'auto', onionDefaultVersion: 1 }).transport, 'auto');
});
test('starting Tor without a local installation cannot initiate a clearnet download', async () => {
  const calls = [];
  const context = vm.createContext({ onionizeStartup: null, invoke: async name => {
    calls.push(name); return { installed: false };
  } });
  vm.runInContext(functionText('ensureOnionize', '\nasync function prewarmOnionize'), context);
  await assert.rejects(vm.runInContext('ensureOnionize()', context), /Tor не установлен/);
  assert.deepEqual(calls, ['onionize_status']);
});
test('activity signals stay disabled while privacy settings are still loading', () => {
  const context = vm.createContext({ privacy: null });
  vm.runInContext(functionText('permits', '\n/** Какое правило'), context);
  for (const rule of ['typing', 'presence', 'read_receipts', 'voice_recording_hint']) {
    assert.equal(vm.runInContext(`permits('${rule}', 'peer')`, context), false);
  }
});
