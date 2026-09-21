import { chmodSync, existsSync, lstatSync, unlinkSync } from "node:fs";
import { createServer } from "node:net";
import { pathToFileURL } from "node:url";
import { browserChannel } from "./support-browser-protocol.mjs";

const RUNNER_SOCKET = "/run/support-browser/runner.sock";
const LOADABLE_TYPES = new Set(["document", "stylesheet", "script", "xhr", "fetch", "other"]);

export function browserConcurrency(value = "2") {
  if (!/^(?:[1-9]|1[0-6])$/.test(String(value))) throw new Error("invalid_browser_concurrency");
  return Number(value);
}

export async function closeBrowserServer(server, timeoutMs = 5000) {
  let timer;
  try {
    await Promise.race([
      server.close(),
      new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("cleanup_timeout")), timeoutMs); }),
    ]);
  } catch {
    // Kill only this invocation's browser, leaving other sessions intact.
    await server.kill();
  } finally { clearTimeout(timer); }
}

export async function serveBrowserPage(socket, chromium) {
  let browser;
  let launchPromise;
  let context;
  let started = false;
  let closing = false;
  let nextId = 0;
  let active = 0;
  const pending = new Map();
  const queue = [];
  const blockedHosts = new Set();
  let blockedResources = 0;
  let completed;
  const completion = new Promise((resolve) => { completed = resolve; });
  const close = async () => {
    if (closing) return;
    closing = true;
    clearTimeout(deadline);
    for (const resolve of pending.values()) resolve({ error: "browser_closed" });
    pending.clear();
    for (const resolve of queue.splice(0)) resolve();
    try {
      const running = await launchPromise?.catch(() => undefined);
      if (running) await closeBrowserServer(running);
      socket.destroy();
      completed();
    } catch {
      // Retain this slot when process cleanup could not be confirmed.
      socket.destroy();
      process.stderr.write("browser_cleanup_failed\n");
    }
  };
  const deadline = setTimeout(() => {
    send({ type: "result", result: { ok: false, error: "browser_timeout" } });
    void close();
  }, 35_000);
  const send = browserChannel(socket, async (message) => {
    if (message.type === "resource") {
      pending.get(message.id)?.(message);
      pending.delete(message.id);
      return;
    }
    if (started || message.type !== "open" || typeof message.url !== "string"
        || message.url.length > 4096 || !message.url.startsWith("https://")) {
      await close();
      return;
    }
    started = true;
    try {
      // Use the full browser's headless mode rather than the separate headless shell.
      launchPromise = chromium.launchServer({ host: "127.0.0.1", channel: "chromium", headless: true, timeout: 15_000, args: ["--disable-dev-shm-usage"] });
      const server = await launchPromise;
      if (closing) return;
      browser = await chromium.connect(server.wsEndpoint(), { timeout: 15_000 });
      if (closing) return;
      const browserSession = await browser.newBrowserCDPSession();
      const { userAgent } = await browserSession.send("Browser.getVersion");
      await browserSession.detach();
      context = await browser.newContext({
        acceptDownloads: false, serviceWorkers: "block", viewport: { width: 1280, height: 900 },
        userAgent: userAgent.replace("HeadlessChrome/", "Chrome/"), locale: "en-US",
      });
      // Pages do have a transport (the Gateway broker). Chromium's OS-level
      // network is intentionally absent; do not let navigator.onLine suppress
      // application GETs before they reach that broker.
      await context.addInitScript(() => {
        Object.defineProperty(Navigator.prototype, "onLine", { configurable: true, get: () => true });
      });
      const page = await context.newPage();
      const session = await context.newCDPSession(page);
      const recordFailure = (url) => {
        blockedResources++;
        try { blockedHosts.add(new URL(url).hostname); } catch { /* No hostname. */ }
      };
      // Cross-process frames/workers and streaming sockets have no direct network
      // in this container. Mark their failures instead of reporting a full load.
      page.on("websocket", (socket) => recordFailure(socket.url()));
      page.on("requestfailed", (request) => {
        if (LOADABLE_TYPES.has(request.resourceType())
            && request.failure()?.errorText !== "net::ERR_BLOCKED_BY_CLIENT") recordFailure(request.url());
      });
      // Fetch interception runs again at every redirect hop. Playwright route()
      // deliberately skips later hops, which cannot work in an offline runner.
      await session.send("Fetch.enable", { patterns: [{ urlPattern: "*", requestStage: "Request" }] });
      session.on("Fetch.requestPaused", async ({ requestId, request, resourceType }) => {
        const abort = () => session.send("Fetch.failRequest", { requestId, errorReason: "BlockedByClient" }).catch(() => {});
        if (!LOADABLE_TYPES.has(resourceType.toLowerCase())) {
          await abort();
          return;
        }
        if (active >= 8) await new Promise((resolve) => queue.push(resolve));
        if (closing) { await abort(); return; }
        active++;
        try {
          const id = ++nextId;
          const response = await new Promise((resolve) => {
            pending.set(id, resolve);
            send({ type: "fetch", id, url: request.url, method: request.method, headers: request.headers });
          });
          if (response.error) {
            recordFailure(request.url);
            await abort();
          } else {
            await session.send("Fetch.fulfillRequest", {
              requestId, responseCode: response.status, body: response.body,
              responseHeaders: [
                ...Object.entries(response.headers).map(([name, value]) => ({ name, value })),
                ...(response.setCookies ?? []).map((value) => ({ name: "set-cookie", value })),
              ],
            });
          }
        } catch { await abort(); }
        finally { active--; queue.shift()?.(); }
      });
      context.on("page", (other) => { if (other !== page) void other.close(); });
      page.on("dialog", (dialog) => void dialog.dismiss());
      const response = await page.goto(message.url, { waitUntil: "domcontentloaded", timeout: 25_000 });
      let settled = true;
      try { await page.waitForLoadState("networkidle", { timeout: 5000 }); }
      catch { settled = false; }
      const content = await page.evaluate(() => {
        const text = document.body?.innerText ?? "";
        return {
          title: document.title.slice(0, 500), text: text.slice(0, 32_000), truncated: text.length > 32_000,
          links: Array.from(document.querySelectorAll("a[href]"))
            .filter((link) => link.innerText.trim() && link.href.startsWith("https://"))
            .slice(0, 80).map((link) => ({ text: link.innerText.trim().slice(0, 200), url: link.href.slice(0, 4096) })),
        };
      });
      send({ type: "result", result: {
        ok: true, url: page.url(), status: response?.status() ?? null, ...content,
        partial: !settled || blockedResources > 0,
        blocked_resources: blockedResources, blocked_hosts: [...blockedHosts].slice(0, 64),
      } });
    } catch {
      send({ type: "result", result: { ok: false, error: "page_unavailable", blocked_hosts: [...blockedHosts].slice(0, 64) } });
    } finally { await close(); }
  });
  socket.once("error", () => void close());
  socket.once("close", () => void close());
  return completion;
}

export function startBrowserRunner(chromium, socketPath = RUNNER_SOCKET, options = {}) {
  const concurrency = browserConcurrency(options.concurrency ?? process.env.SUPPORT_BROWSER_CONCURRENCY ?? "2");
  const serve = options.serve ?? serveBrowserPage;
  const queueTimeoutMs = options.queueTimeoutMs ?? 35_000;
  if (existsSync(socketPath)) {
    if (!lstatSync(socketPath).isSocket()) throw new Error("unsafe_browser_socket");
    unlinkSync(socketPath);
  }
  let active = 0;
  const waiting = [];
  const start = (socket) => {
    active++;
    // Ownership includes browser cleanup, also after the client disconnects.
    const execution = serve(socket, chromium);
    socket.resume();
    Promise.resolve(execution).catch(() => socket.destroy()).finally(() => {
      active--;
      while (waiting.length && active < concurrency) {
        const next = waiting.shift();
        clearTimeout(next.timer);
        if (!next.socket.destroyed) start(next.socket);
      }
    });
  };
  const server = createServer((socket) => {
    if (active < concurrency) { start(socket); return; }
    if (waiting.length >= 16) {
      socket.end(`${JSON.stringify({ type: "result", result: { ok: false, error: "browser_busy" } })}\n`);
      return;
    }
    socket.pause();
    const entry = { socket, timer: null };
    const remove = () => {
      clearTimeout(entry.timer);
      const index = waiting.indexOf(entry);
      if (index !== -1) waiting.splice(index, 1);
    };
    entry.timer = setTimeout(() => {
      remove();
      socket.end(`${JSON.stringify({ type: "result", result: { ok: false, error: "browser_timeout" } })}\n`);
    }, queueTimeoutMs);
    socket.once("close", remove);
    socket.once("error", remove);
    waiting.push(entry);
  });
  server.listen(socketPath, () => chmodSync(socketPath, 0o660));
  return server;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const { chromium } = await import("playwright");
  process.umask(0o007);
  startBrowserRunner(chromium);
}
