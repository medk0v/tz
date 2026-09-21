import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdtempSync, rmSync } from "node:fs";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { browserConcurrency, closeBrowserServer, startBrowserRunner } from "./support-browser-runner.mjs";

const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
async function until(check) {
  for (let i = 0; i < 100; i++) { if (check()) return; await tick(); }
  assert.fail("runner did not reach expected state");
}

test("browser concurrency validates bounded runtime configuration", () => {
  assert.equal(browserConcurrency(), 2);
  assert.equal(browserConcurrency("16"), 16);
  for (const value of ["0", "17", "-1", "2.5", "02", "2x", ""]) {
    assert.throws(() => browserConcurrency(value), /invalid_browser_concurrency/);
  }
});

test("independent browser operations overlap while queued work waits through cleanup", async (t) => {
  const root = mkdtempSync(join(tmpdir(), "tz-browser-pool-"));
  const socketPath = join(root, "r.sock");
  const running = [];
  const clients = [];
  let active = 0;
  let peak = 0;
  const server = startBrowserRunner(null, socketPath, { concurrency: 2,
    serve: (socket) => new Promise((resolve) => {
      peak = Math.max(peak, ++active);
      running.push({ socket, finish: () => { active--; socket.destroy(); resolve(); } });
    }),
  });
  t.after(async () => {
    for (const client of clients) client.destroy();
    for (const run of running) run.socket.destroy();
    await new Promise((resolve) => server.close(resolve));
    rmSync(root, { recursive: true, force: true });
  });
  await once(server, "listening");
  for (let i = 0; i < 3; i++) {
    const client = connect(socketPath); clients.push(client); await once(client, "connect");
  }
  await until(() => running.length === 2);
  clients[0].destroy();
  await tick();
  assert.equal(running.length, 2, "disconnect must not release a slot before cleanup");
  assert.equal(running[1].socket.destroyed, false, "another session remains connected");
  running[0].finish();
  await until(() => running.length === 3);
  assert.equal(peak, 2);
  running[1].finish();
  running[2].finish();
});

test("a browser cleanup timeout kills only the owning browser", async () => {
  const killed = [];
  const first = { close: () => new Promise(() => {}), kill: async () => killed.push("first") };
  const second = { close: async () => {}, kill: async () => killed.push("second") };
  await Promise.all([closeBrowserServer(first, 10), closeBrowserServer(second, 10)]);
  assert.deepEqual(killed, ["first"]);
});

test("browser queue waiting is bounded and expired work never starts", async (t) => {
  const root = mkdtempSync(join(tmpdir(), "tz-browser-queue-"));
  const socketPath = join(root, "r.sock");
  let finish;
  let started = 0;
  const server = startBrowserRunner(null, socketPath, { concurrency: 1, queueTimeoutMs: 25,
    serve: (socket) => { started++; return new Promise((resolve) => { finish = () => { socket.destroy(); resolve(); }; }); },
  });
  await once(server, "listening");
  const first = connect(socketPath); await once(first, "connect");
  const second = connect(socketPath);
  const [data] = await once(second, "data");
  assert.equal(JSON.parse(data).result.error, "browser_timeout");
  finish();
  await tick();
  assert.equal(started, 1);
  first.destroy(); second.destroy();
  await new Promise((resolve) => server.close(resolve));
  t.after(() => rmSync(root, { recursive: true, force: true }));
});
