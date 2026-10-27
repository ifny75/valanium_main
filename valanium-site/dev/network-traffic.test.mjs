import { test } from 'node:test';
import assert from 'node:assert/strict';
import { networkSnapshot, networkRates } from '../app/network-traffic.ts';
const routes = 'Iface Destination Gateway Flags RefCnt Use Metric Mask\nens3 00000000 0100000A 0003 0 0 0 00000000\nens3 00000000 0100000A 0003 0 0 1 00000000\nwg-mesh 00004D0A 00000000 0001 0 0 0 00FFFFFF';
const devices = (rx, tx) => `Inter-| Receive | Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\nens3: ${rx} 0 0 0 0 0 0 0 ${tx} 0 0 0 0 0 0 0\nlo: 99999999 0 0 0 0 0 0 0 99999999 0 0 0 0 0 0 0\nwg-mesh: 99999999 0 0 0 0 0 0 0 99999999 0 0 0 0 0 0 0`;

test('count WAN once, excluding loopback and tunnel traffic; convert bytes to Mbit/s using measured time', () => {
  const before = networkSnapshot(routes, devices(1000, 2000));
  const after = networkSnapshot(routes, devices(251000, 502000));
  assert.deepEqual(Object.keys(before), ['ens3']);
  assert.deepEqual(networkRates(before, after, 2), { rxMbps: 1, txMbps: 2 });
  assert.deepEqual(networkRates(before, before, 1), { rxMbps: 0, txMbps: 0 });
});

test('missing counters, interface changes and counter resets cannot fabricate zero or negative traffic', () => {
  const first = networkSnapshot(routes, devices(100, 200));
  const reset = networkSnapshot(routes, devices(0, 100));
  const missing = { rxMbps: null, txMbps: null };
  assert.equal(networkSnapshot('', devices(100, 200)), null);
  assert.equal(networkSnapshot(routes, 'ens3: broken'), null);
  assert.deepEqual(networkRates(first, reset, 1), missing);
  assert.deepEqual(networkRates(first, { ens4: { rx: 200, tx: 400 } }, 1), missing);
  assert.deepEqual(networkRates(first, null, 1), missing);
  assert.deepEqual(networkRates(first, first, 0), missing);
});
