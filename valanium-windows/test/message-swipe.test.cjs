const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

function fixture(outgoing, left, right) {
  const handlers = {};
  let captured = false, menus = 0;
  const item = { style: {}, classList: { contains: () => outgoing },
    getBoundingClientRect: () => ({ left, right, width: right - left, bottom: 100 }),
    hasPointerCapture: () => captured, setPointerCapture: () => { captured = true; },
    releasePointerCapture: () => { captured = false; } };
  const list = { clientLeft: 0, clientWidth: 360,
    getBoundingClientRect: () => ({ left: 0, right: 360 }),
    addEventListener: (name, fn) => { handlers[name] = fn; } };
  const source = fs.readFileSync(path.join(__dirname, "../src/main.js"), "utf8");
  const start = source.indexOf("// Swipe toward the center;");
  assert.ok(start >= 0);
  vm.runInNewContext(source.slice(start, source.indexOf("function openMessageMenu(", start)), {
    $: () => list, document: { addEventListener: (name, fn) => { handlers[name] = fn; } },
    getComputedStyle: () => ({ paddingLeft: "0px", paddingRight: "0px" }),
    openMessageMenu: () => { menus++; },
  });
  const target = { closest: selector => selector === "li[data-message-id]" ? item : null };
  const emit = (type, x, y = 0, pointerId = 1) => handlers[type]({ type, clientX: x,
    clientY: y, pointerId, button: 0, isPrimary: true, target, preventDefault() {} });
  return { item, emit, menus: () => menus };
}

test("outgoing bubble stays in bounds while a long logical swipe opens actions", () => {
  const f = fixture(true, 48, 360);
  f.emit("pointerdown", 240); f.emit("pointermove", 60);
  assert.equal(f.item.style.translate, "-48px 0");
  f.emit("pointerup", 60);
  assert.equal(f.item.style.translate, ""); assert.equal(f.menus(), 1);
});
test("short incoming message swipes right and opens its actions", () => {
  const f = fixture(false, 0, 40);
  f.emit("pointerdown", 20); f.emit("pointermove", 180);
  assert.equal(f.item.style.translate, "70px 0");
  f.emit("pointerup", 180); assert.equal(f.menus(), 1);
});
test("a wide incoming message cannot leave the viewport", () => {
  const f = fixture(false, 0, 312);
  f.emit("pointerdown", 100); f.emit("pointermove", 290);
  assert.equal(f.item.style.translate, "48px 0");
});
test("cancel resets the bubble without opening the menu", () => {
  const f = fixture(true, 100, 360);
  f.emit("pointerdown", 200); f.emit("pointermove", 100); f.emit("pointercancel", 100);
  assert.equal(f.item.style.translate, ""); assert.equal(f.menus(), 0);
});
test("native vertical scroll and opposite swipe do not trigger actions", () => {
  for (const vertical of [true, false]) {
    const f = fixture(false, 0, 40);
    f.emit("pointerdown", 20);
    f.emit("pointermove", vertical ? 22 : -100, vertical ? 50 : 0);
    f.emit("pointerup", 180, vertical ? 50 : 0); assert.equal(f.menus(), 0);
  }
});
test("an unrelated pointer release cannot cancel the active gesture", () => {
  const f = fixture(true, 100, 360);
  f.emit("pointerdown", 200); f.emit("pointermove", 100);
  f.emit("pointerup", 100, 0, 2); f.emit("pointerup", 100);
  assert.equal(f.menus(), 1);
});
