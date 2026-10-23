import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Store } from "../src/db/index.ts";

test("lost sender ACK, recipient ACK and server restart do not duplicate an envelope", () => {
  const dir = mkdtempSync(join(tmpdir(), "valanium-retry-"));
  const path = join(dir, "test.db");
  const recipient = new Uint8Array(32).fill(2);
  const ref = new Uint8Array(16).fill(3);
  const payload = new Uint8Array(256).fill(4);
  try {
    let store = new Store(path);
    const digest = store.retryDigest(recipient, ref, payload);
    const id = store.enqueueOnce(digest, recipient, payload, 100, 10000);
    assert.deepEqual(store.enqueueOnce(digest, recipient, payload, 200, 10000), id);
    assert.equal(store.countQueued(recipient, 200), 1);
    assert.equal(store.ack(id, recipient), true);
    store.close();
    store = new Store(path);
    assert.deepEqual(store.enqueueOnce(digest, recipient, payload, 300, 10000), id);
    assert.equal(store.countQueued(recipient, 300), 0);
    assert.notDeepEqual(store.retryDigest(new Uint8Array(32).fill(5), ref, payload), digest);
    assert.notDeepEqual(store.retryDigest(recipient, ref, new Uint8Array(256).fill(6)), digest);
    assert.notDeepEqual(store.retryDigest(recipient, new Uint8Array(16).fill(7), payload), digest);
    store.sweep(10000);
    assert.equal(store.acceptedRetry(digest, 10000), undefined);
    store.close();
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
