import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';

const source = readFileSync(new URL('../src/main.js', import.meta.url), 'utf8');

test('delivery labels distinguish local save, queue, server ACK and peer receipt', () => {
  const context = vm.createContext({ tr: value => value, trTemplate: (parts,...values) => parts.reduce((out,part,i)=>out+part+(i<values.length?values[i]:""),""), getLocale:()=>"ru-RU", });
  const start = source.indexOf('function deliveryLabel(');
  const end = source.indexOf('\nfunction buildMessage(', start);
  vm.runInContext(source.slice(start, end), context);
  assert.equal(vm.runInContext('deliveryLabel(undefined)', context), 'сохраняется…');
  assert.equal(vm.runInContext('deliveryLabel("queued")', context), 'в очереди · ожидает Tor');
  assert.equal(vm.runInContext('deliveryLabel("accepted")', context), '✓ принято сервером');
  assert.equal(vm.runInContext('deliveryLabel("delivered")', context), '✓✓ доставлено');
  assert.equal(vm.runInContext('deliveryLabel("unknown")', context), 'статус неизвестен');
});

test('late key verification results cannot confirm a different open conversation', () => {
  const start = source.indexOf('  verification_checked(event) {');
  const end = source.indexOf('\n  },', start);
  const method = 'function ' + source.slice(start, end + 4).trim().replace(/,$/, '');
  const sent = [];
  const context = vm.createContext({ tr: value => value, trTemplate: (parts,...values) => parts.reduce((out,part,i)=>out+part+(i<values.length?values[i]:""),""), getLocale:()=>"ru-RU",  state: { current: 'bob' }, toast() {}, submit: event => sent.push(event) });
  vm.runInContext(method, context);
  vm.runInContext('verification_checked({peer_device:"mallory",verified:true})', context);
  assert.equal(sent.length, 0);
  vm.runInContext('verification_checked({peer_device:"bob",verified:true})', context);
  assert.equal(sent[0].peer_device, 'bob');
});
function functionText(name, next) {
  return source.slice(source.indexOf(`function ${name}(`), source.indexOf(next, source.indexOf(`function ${name}(`)));
}
function preferences(saved) {
  const storage = new Map([['valanium.preferences', JSON.stringify(saved)]]);
  const context = vm.createContext({ tr: value => value, trTemplate: (parts,...values) => parts.reduce((out,part,i)=>out+part+(i<values.length?values[i]:""),""), getLocale:()=>"ru-RU",  preferenceDefaults: { transport: 'onion', accent: '#7b2cff' },
    ROUTE_MIGRATION_NOTICE: 'migration', localStorage: {
      getItem: key => storage.get(key), setItem: (key, value) => storage.set(key, value),
    } });
  vm.runInContext(functionText('loadPreferences', '\nlet preferences ='), context);
  return vm.runInContext('loadPreferences()', context);
}
test('new and previous default installations use onion; explicit choices survive migration', () => {
  assert.equal(preferences({}).transport, 'onion');
  assert.equal(preferences({ transport: 'auto' }).transport, 'onion');
  assert.equal(preferences({ transport: 'basic' }).transport, 'onion');
  assert.equal(preferences({ transport: 'multihop' }).transport, 'onion');
  assert.equal(preferences({ transport: 'invalid' }).transport, 'onion');
  assert.equal(preferences({ transport: 'auto', onionDefaultVersion: 1 }).transport, 'auto');
});
test('starting Tor without a local installation cannot initiate a clearnet download', async () => {
  const calls = [];
  const context = vm.createContext({ tr: value => value, trTemplate: (parts,...values) => parts.reduce((out,part,i)=>out+part+(i<values.length?values[i]:""),""), getLocale:()=>"ru-RU",  onionizeStartup: null, invoke: async name => {
    calls.push(name); return { installed: false };
  } });
  vm.runInContext(functionText('ensureOnionize', '\nasync function prewarmOnionize'), context);
  await assert.rejects(vm.runInContext('ensureOnionize()', context), /Tor не установлен/);
  assert.deepEqual(calls, ['onionize_status']);
});
test('activity signals stay disabled while privacy settings are still loading', () => {
  const context = vm.createContext({ tr: value => value, trTemplate: (parts,...values) => parts.reduce((out,part,i)=>out+part+(i<values.length?values[i]:""),""), getLocale:()=>"ru-RU",  privacy: null });
  vm.runInContext(functionText('permits', '\n/** Какое правило'), context);
  for (const rule of ['typing', 'presence', 'read_receipts', 'voice_recording_hint']) {
    assert.equal(vm.runInContext(`permits('${rule}', 'peer')`, context), false);
  }
});

test('only the dedicated preset acknowledgement activates onion and reconnects', async () => {
  const start = source.indexOf('  privacy(event) {');
  const end = source.indexOf('\n  },', start);
  const method = 'function ' + source.slice(start, end + 4).trim().replace(/,$/, '').replace('privacy(event)', 'receivePrivacy(event)');
  const button = { disabled: true };
  const calls = [];
  const context = vm.createContext({ tr: value => value, trTemplate: (parts,...values) => parts.reduce((out,part,i)=>out+part+(i<values.length?values[i]:""),""), getLocale:()=>"ru-RU",  privacy: null, maximumPrivacyPending: true,
    preferences: { transport: 'basic' }, ownEcho: false, exceptionsFor: null,
    $: () => button, savePreferences() {}, applyPreferences() {}, toast() {},
    renderPrivacy() {}, updatePrivacyCounts() {}, renderExceptions() {},
    connectSelectedRoute: async options => { calls.push(options); },
    submit: event => calls.push(event), SERVER_URLS: { onion: 'valanium://onion' },
  });
  vm.runInContext(method, context);
  vm.runInContext('receivePrivacy({type:"privacy",privacy:{}})', context);
  assert.equal(vm.runInContext('maximumPrivacyPending', context), true);
  assert.equal(vm.runInContext('preferences.transport', context), 'basic');
  vm.runInContext('receivePrivacy({type:"privacy_maximized",privacy:{}})', context);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(vm.runInContext('maximumPrivacyPending', context), false);
  assert.equal(vm.runInContext('preferences.transport', context), 'onion');
  assert.equal(button.disabled, false);
  assert.equal(calls[0].disconnect, true);
});
