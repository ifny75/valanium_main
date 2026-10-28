import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';

const source = readFileSync(new URL('../src/main.js', import.meta.url), 'utf8');
function listener(id, event) {
  const start = source.indexOf(`$("${id}").addEventListener("${event}"`);
  assert.ok(start >= 0);
  const end = source.indexOf('\n});', start);
  return source.slice(start, end + 4);
}
function registration(torReady) {
  const commands = [];
  const nodes = Object.fromEntries(['form-entry', 'entry-submit', 'entry-error'].map(id => [id, {}]));
  nodes['form-entry'].addEventListener = (_, handler) => { nodes.submit = handler; };
  const context = vm.createContext({
    $: id => { assert.ok(nodes[id], `Unexpected credential field: ${id}`); return nodes[id]; },
    state: {}, preferences: { transport: 'onion' }, savePreferences() {},
    serverUrl: () => 'valanium://auto',
    ensureOnionize: async () => { if (!torReady) throw Error('Tor unavailable'); },
    submit: async command => { commands.push(command); return true; },
  });
  vm.runInContext(listener('form-entry', 'submit'), context);
  return { commands, nodes, context };
}

test('registration needs no credential fields and sends no account handle or password', async () => {
  const { nodes, commands, context } = registration(true);
  await nodes.submit({ preventDefault() {} });
  assert.equal(commands.length, 1);
  assert.equal(commands[0].type, 'register');
  assert.equal(commands[0].handle, null);
  assert.equal(commands[0].url, 'valanium://onion');
  assert.equal('password' in commands[0], false);
  assert.equal(context.preferences.transport, 'onion');
});

test('seed-only registration stays retryable and sends nothing when Tor fails', async () => {
  const { nodes, commands, context } = registration(false);
  await nodes.submit({ preventDefault() {} });
  assert.equal(commands.length, 0);
  assert.equal(context.state.registrationPending, false);
  assert.equal(nodes['entry-submit'].disabled, false);
  assert.match(nodes['entry-error'].textContent, /Tor unavailable/);
});

test('phrase recovery requires all 24 words and uses only the seed command', () => {
  const words = Array(24).fill('abandon');
  const calls = [];
  const nodes = { 'form-recover-code': {}, 'recover-error': {}, 'recover-code': {} };
  nodes['form-recover-code'].addEventListener = (_, handler) => { nodes.submit = handler; };
  const context = vm.createContext({ $: id => nodes[id], recoveryWords: words,
    storeRecoveryPage() {}, serverUrl: () => 'valanium://onion',
    runRecovery: (...args) => calls.push(args) });
  vm.runInContext(listener('form-recover-code', 'submit'), context);
  words[23] = '';
  nodes.submit({ preventDefault() {} });
  assert.equal(calls.length, 0);
  words[23] = 'art';
  nodes.submit({ preventDefault() {} });
  assert.equal(calls.length, 1);
  assert.equal(calls[0][1].type, 'recover');
  assert.equal(calls[0][1].code.split(' ').length, 24);
  assert.equal('login' in calls[0][1], false);
});
