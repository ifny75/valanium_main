import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
const source = readFileSync(new URL('../src/main.js', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const start = source.indexOf('async function connectSelectedRoute(');
const end = source.indexOf('\n/*', start);
function context(transport, ready = async () => {}) {
  const calls = [], statuses = [];
  const scope = vm.createContext({ connectionGeneration: 0, state: { device: 'qa-device' },
    preferences: { transport }, SERVER_URLS: { auto: 'valanium://auto', onion: 'valanium://onion' },
    setConnection: (...args) => statuses.push(args), ensureOnionize: ready,
    invoke: async () => ({ installed: true }), submit: async command => { calls.push(command); return true; } });
  vm.runInContext(source.slice(start, end), scope);
  return { scope, calls, statuses };
}

test('Tor bootstrap failure stays offline without issuing any direct or Auto connection', async () => {
  const { scope, calls, statuses } = context('onion', async () => { throw Error('Tor unavailable'); });
  await vm.runInContext('connectSelectedRoute({disconnect:true})', scope);
  assert.deepEqual(calls.map(c => c.type), ['disconnect']);
  assert.deepEqual(statuses.at(-1), ['offline', 'Tor unavailable']);
});
test('a late Tor startup cannot reconnect the previous route after switching to Auto', async () => {
  let resolve;
  const pending = new Promise(done => { resolve = done; });
  const { scope, calls } = context('onion', () => pending);
  const first = vm.runInContext('connectSelectedRoute()', scope);
  scope.preferences.transport = 'auto';
  await vm.runInContext('connectSelectedRoute({disconnect:true})', scope);
  resolve(); await first;
  assert.deepEqual(calls.filter(c => c.type === 'connect').map(c => c.url), ['valanium://auto']);
});
test('Auto connects without waiting for Tor, while preparing the installed fallback', async () => {
  let warmed = false;
  const { scope, calls } = context('auto', async () => { warmed = true; });
  await vm.runInContext('connectSelectedRoute()', scope);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(calls[0].url, 'valanium://auto');
  assert.equal(warmed, true);
});
test('local account opens the chat UI before waiting on the network', () => {
  const a = source.indexOf('  status(event) {'), b = source.indexOf('\n  },', a);
  const screens = [], events = [];
  const scope = vm.createContext({ state: {}, $: () => ({}), short: x => x,
    showScreen: id => screens.push(id), submit: e => events.push(e), setConnection() {},
    connectSelectedRoute: () => { events.push('network'); return new Promise(() => {}); } });
  vm.runInContext('function ' + source.slice(a,b + 4).trim().replace(/,$/, ''), scope);
  vm.runInContext('status({has_identity:true,device:"test",identity:"test"})', scope);
  assert.equal(screens[0], 'screen-main');
  assert.equal(events.at(-1), 'network');
});
