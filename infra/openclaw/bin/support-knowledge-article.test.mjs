#!/usr/bin/env node

import assert from "node:assert/strict";
import {
  chmodSync,
  lstatSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { afterEach, test } from "node:test";

const RUNNER_PATH = join(
  dirname(fileURLToPath(import.meta.url)),
  "support-wrapper-test-runner.mjs",
);
const GRANT_ID = "50f675a8-43bc-4dbd-89ad-7b522419a1e0";
const OTHER_GRANT_ID = "019c9e10-b484-7cc2-b743-fb251615eaed";
const CASH_ARTICLE_ID = "01a03f7d-5aa6-7fb0-b88b-b37032f26ac8";
const temporaryDirectories = [];

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

function setup() {
  const root = mkdtempSync(join(tmpdir(), "support-knowledge-article-"));
  temporaryDirectories.push(root);
  const grantDirectory = join(root, "grants");
  const actionDirectory = join(root, "actions");
  mkdirSync(grantDirectory, { mode: 0o750 });
  mkdirSync(actionDirectory, { mode: 0o770 });
  return { root, grantDirectory, actionDirectory };
}

function writeGrant(grantDirectory, overrides = {}, grantId = GRANT_ID) {
  const path = join(grantDirectory, `${grantId}.json`);
  writeFileSync(
    path,
    JSON.stringify({
      version: 4,
      expires_at_unix_ms: Date.now() + 60_000,
      knowledge_lookup: true,
      ...overrides,
    }),
    { mode: 0o640 },
  );
  chmodSync(path, 0o640);
  return path;
}

function requestPath(actionDirectory, grantId = GRANT_ID) {
  return join(actionDirectory, `${grantId}.knowledge-request`);
}

function responsePath(actionDirectory, grantId = GRANT_ID) {
  return join(actionDirectory, `${grantId}.knowledge-response`);
}

function lockPath(actionDirectory, grantId = GRANT_ID) {
  return join(actionDirectory, `${grantId}.knowledge-lock`);
}

function run(grantDirectory, actionDirectory, ...arguments_) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [
      RUNNER_PATH,
      "knowledge",
      grantDirectory,
      actionDirectory,
      ...arguments_,
    ]);
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("close", (status) => {
      try {
        assert.equal(status, 0, stderr);
        assert.equal(stderr, "");
        assert.match(stdout, /^\{.*\}\n$/);
        resolve(JSON.parse(stdout));
      } catch (error) {
        reject(error);
      }
    });
  });
}

async function waitForRequest(actionDirectory, grantId = GRANT_ID) {
  const path = requestPath(actionDirectory, grantId);
  const deadline = Date.now() + 2_000;
  while (Date.now() < deadline) {
    try {
      const metadata = lstatSync(path);
      assert.equal(metadata.isFile(), true);
      assert.equal(metadata.isSymbolicLink(), false);
      assert.equal(metadata.mode & 0o777, 0o640);
      return JSON.parse(readFileSync(path, "utf8"));
    } catch (error) {
      if (error?.code !== "ENOENT") {
        throw error;
      }
    }
    await delay(10);
  }
  throw new Error("knowledge request marker was not created");
}

function articleResponse(articleId, overrides = {}) {
  return {
    ok: true,
    article: {
      article_id: articleId,
      version: 1,
      knowledge_base: "Product support",
      title: "Наличные",
      source_url: null,
      content: "Порядок обмена USDT на наличные находится только здесь.",
      next_offset: null,
      ...overrides,
    },
  };
}

function writeResponse(actionDirectory, response, grantId = GRANT_ID) {
  const path = responsePath(actionDirectory, grantId);
  const temporaryPath = `${path}.test-tmp`;
  writeFileSync(temporaryPath, `${JSON.stringify(response)}\n`, { mode: 0o640 });
  chmodSync(temporaryPath, 0o640);
  renameSync(temporaryPath, path);
  return path;
}

async function runWithResponse(
  grantDirectory,
  actionDirectory,
  articleId,
  response = articleResponse(articleId),
  grantId = GRANT_ID,
  offset = 0,
  version = 1,
) {
  const arguments_ = offset === 0
    ? [grantId, articleId]
    : [grantId, articleId, String(offset), String(version)];
  const resultPromise = run(grantDirectory, actionDirectory, ...arguments_);
  const request = await waitForRequest(actionDirectory, grantId);
  assert.deepEqual(request, {
    article_id: articleId,
    offset,
    version: offset === 0 ? null : version,
  });
  writeResponse(actionDirectory, response, grantId);
  return resultPromise;
}

function generatedArticleId(index) {
  return `00000000-0000-4000-8000-${index.toString(16).padStart(12, "0")}`;
}

test("requests and returns only the selected cash article", async () => {
  const { grantDirectory, actionDirectory } = setup();
  writeGrant(grantDirectory);
  const resultPromise = run(
    grantDirectory,
    actionDirectory,
    GRANT_ID.toUpperCase(),
    CASH_ARTICLE_ID.toUpperCase(),
  );
  const request = await waitForRequest(actionDirectory);
  assert.deepEqual(request, {
    article_id: CASH_ARTICLE_ID,
    offset: 0,
    version: null,
  });
  writeResponse(actionDirectory, articleResponse(CASH_ARTICLE_ID));

  assert.deepEqual(
    await resultPromise,
    articleResponse(CASH_ARTICLE_ID),
  );
  assert.throws(() => lstatSync(requestPath(actionDirectory)), { code: "ENOENT" });
  assert.throws(() => lstatSync(responsePath(actionDirectory)), { code: "ENOENT" });
});

test("retrieves an article beyond the former 32-article boundary", async () => {
  const { grantDirectory, actionDirectory } = setup();
  writeGrant(grantDirectory);
  const articleId = generatedArticleId(65);

  const result = await runWithResponse(
    grantDirectory,
    actionDirectory,
    articleId,
    articleResponse(articleId, { title: "Статья 65", content: "Содержимое 65" }),
  );
  assert.equal(result.article.article_id, articleId);
  assert.equal(result.article.title, "Статья 65");
  assert.equal(result.article.content, "Содержимое 65");
});

test("uses UUIDs to distinguish articles with duplicate titles", async () => {
  const { grantDirectory, actionDirectory } = setup();
  writeGrant(grantDirectory);
  const firstId = generatedArticleId(1);
  const secondId = generatedArticleId(2);

  const first = await runWithResponse(
    grantDirectory,
    actionDirectory,
    firstId,
    articleResponse(firstId, { content: "Первый материал" }),
  );
  const second = await runWithResponse(
    grantDirectory,
    actionDirectory,
    secondId,
    articleResponse(secondId, { content: "Второй материал" }),
  );
  assert.equal(first.article.content, "Первый материал");
  assert.equal(second.article.content, "Второй материал");
});

test("returns bounded chunks and accepts the advertised next offset", async () => {
  const { grantDirectory, actionDirectory } = setup();
  writeGrant(grantDirectory);
  const escapedContent = "\u0000".repeat(20_000);
  const first = await runWithResponse(
    grantDirectory,
    actionDirectory,
    CASH_ARTICLE_ID,
    articleResponse(CASH_ARTICLE_ID, {
      content: escapedContent,
      next_offset: 20_000,
    }),
  );
  assert.equal(first.article.content, escapedContent);
  assert.equal(first.article.next_offset, 20_000);

  const whitespace = " ".repeat(20_000);
  const middle = await runWithResponse(
    grantDirectory,
    actionDirectory,
    CASH_ARTICLE_ID,
    articleResponse(CASH_ARTICLE_ID, {
      content: whitespace,
      next_offset: 40_000,
    }),
    GRANT_ID,
    20_000,
  );
  assert.equal(middle.article.content, whitespace);
  assert.equal(middle.article.next_offset, 40_000);

  const second = await runWithResponse(
    grantDirectory,
    actionDirectory,
    CASH_ARTICLE_ID,
    articleResponse(CASH_ARTICLE_ID, {
      content: "Конец статьи",
      next_offset: null,
    }),
    GRANT_ID,
    40_000,
  );
  assert.equal(second.article.content, "Конец статьи");
  assert.equal(second.article.next_offset, null);
});

test("serializes parallel article requests without mixing their responses", async () => {
  const { grantDirectory, actionDirectory } = setup();
  writeGrant(grantDirectory);
  const firstId = generatedArticleId(1);
  const secondId = generatedArticleId(2);
  const firstPromise = run(grantDirectory, actionDirectory, GRANT_ID, firstId);
  const secondPromise = run(grantDirectory, actionDirectory, GRANT_ID, secondId);

  const firstRequest = await waitForRequest(actionDirectory);
  const firstRequestedId = firstRequest.article_id;
  assert.ok(firstRequestedId === firstId || firstRequestedId === secondId);
  writeResponse(
    actionDirectory,
    articleResponse(firstRequestedId, { content: `Содержимое ${firstRequestedId}` }),
  );
  const firstResult = firstRequestedId === firstId
    ? await firstPromise
    : await secondPromise;
  assert.equal(firstResult.article.article_id, firstRequestedId);
  assert.equal(firstResult.article.content, `Содержимое ${firstRequestedId}`);

  const secondRequest = await waitForRequest(actionDirectory);
  const secondRequestedId = secondRequest.article_id;
  assert.notEqual(secondRequestedId, firstRequestedId);
  writeResponse(
    actionDirectory,
    articleResponse(secondRequestedId, { content: `Содержимое ${secondRequestedId}` }),
  );
  const secondResult = secondRequestedId === firstId
    ? await firstPromise
    : await secondPromise;
  assert.equal(secondResult.article.article_id, secondRequestedId);
  assert.equal(secondResult.article.content, `Содержимое ${secondRequestedId}`);
  assert.throws(() => lstatSync(lockPath(actionDirectory)), { code: "ENOENT" });
});

test("rejects a continuation from a changed article version", async () => {
  const { grantDirectory, actionDirectory } = setup();
  writeGrant(grantDirectory);
  const resultPromise = run(
    grantDirectory,
    actionDirectory,
    GRANT_ID,
    CASH_ARTICLE_ID,
    "20000",
    "1",
  );
  assert.deepEqual(await waitForRequest(actionDirectory), {
    article_id: CASH_ARTICLE_ID,
    offset: 20_000,
    version: 1,
  });
  writeResponse(
    actionDirectory,
    articleResponse(CASH_ARTICLE_ID, { version: 2, content: "Новая версия" }),
  );
  assert.deepEqual(await resultPromise, {
    ok: false,
    error: "article_unavailable",
  });
});

test("returns the worker's generic result for an unknown or cross-grant article", async () => {
  const { grantDirectory, actionDirectory } = setup();
  writeGrant(grantDirectory);
  writeGrant(grantDirectory, {}, OTHER_GRANT_ID);

  assert.deepEqual(
    await runWithResponse(
      grantDirectory,
      actionDirectory,
      CASH_ARTICLE_ID,
      { ok: false, error: "article_unavailable" },
    ),
    { ok: false, error: "article_unavailable" },
  );
});

test("requires two UUID arguments and an active knowledge grant", async () => {
  for (const arguments_ of [
    [],
    [GRANT_ID],
    ["not-a-uuid", CASH_ARTICLE_ID],
    [GRANT_ID, "not-a-uuid"],
    [GRANT_ID, CASH_ARTICLE_ID, "-1"],
    [GRANT_ID, CASH_ARTICLE_ID, "200001"],
    [GRANT_ID, CASH_ARTICLE_ID, "1"],
    [GRANT_ID, CASH_ARTICLE_ID, "1", "0"],
    [GRANT_ID, CASH_ARTICLE_ID, "1", "9007199254740992"],
    [GRANT_ID, CASH_ARTICLE_ID, "1", "1", "extra"],
  ]) {
    const { grantDirectory, actionDirectory } = setup();
    assert.deepEqual(await run(grantDirectory, actionDirectory, ...arguments_), {
      ok: false,
      error: "invalid_arguments",
    });
  }

  for (const overrides of [
    { version: 2 },
    { version: 3 },
    { expires_at_unix_ms: Date.now() - 1 },
    { expires_at_unix_ms: "never" },
    { knowledge_lookup: false },
    { knowledge_lookup: undefined },
  ]) {
    const { grantDirectory, actionDirectory } = setup();
    writeGrant(grantDirectory, overrides);
    assert.deepEqual(
      await run(grantDirectory, actionDirectory, GRANT_ID, CASH_ARTICLE_ID),
      { ok: false, error: "not_allowed" },
    );
  }
});

test("rejects malformed, oversized, writable, and symlinked grant files", async () => {
  {
    const { grantDirectory, actionDirectory } = setup();
    writeFileSync(join(grantDirectory, `${GRANT_ID}.json`), "not-json", { mode: 0o640 });
    assert.deepEqual(
      await run(grantDirectory, actionDirectory, GRANT_ID, CASH_ARTICLE_ID),
      { ok: false, error: "invalid_grant" },
    );
  }
  {
    const { grantDirectory, actionDirectory } = setup();
    writeGrant(grantDirectory, { padding: "x".repeat(3 * 1024 * 1024) });
    assert.deepEqual(
      await run(grantDirectory, actionDirectory, GRANT_ID, CASH_ARTICLE_ID),
      { ok: false, error: "invalid_grant" },
    );
  }
  {
    const { grantDirectory, actionDirectory } = setup();
    const path = writeGrant(grantDirectory);
    chmodSync(path, 0o660);
    assert.deepEqual(
      await run(grantDirectory, actionDirectory, GRANT_ID, CASH_ARTICLE_ID),
      { ok: false, error: "invalid_grant" },
    );
  }
  {
    const { root, grantDirectory, actionDirectory } = setup();
    const targetPath = join(root, "grant.json");
    writeFileSync(targetPath, JSON.stringify({
      version: 4,
      expires_at_unix_ms: Date.now() + 60_000,
      knowledge_lookup: true,
    }));
    symlinkSync(targetPath, join(grantDirectory, `${GRANT_ID}.json`));
    assert.deepEqual(
      await run(grantDirectory, actionDirectory, GRANT_ID, CASH_ARTICLE_ID),
      { ok: false, error: "invalid_grant" },
    );
  }
});

test("rejects malformed, oversized, writable, and symlinked responses", async () => {
  const cases = [
    (actionDirectory) => {
      writeResponse(actionDirectory, articleResponse(generatedArticleId(999)));
    },
    (actionDirectory) => {
      writeFileSync(responsePath(actionDirectory), "not-json", { mode: 0o640 });
    },
    (actionDirectory) => {
      writeFileSync(responsePath(actionDirectory), "x".repeat(2 * 1024 * 1024 + 1), {
        mode: 0o640,
      });
    },
    (actionDirectory) => {
      const path = writeResponse(actionDirectory, articleResponse(CASH_ARTICLE_ID));
      chmodSync(path, 0o660);
    },
    (actionDirectory, root) => {
      const targetPath = join(root, "article-response.json");
      writeFileSync(targetPath, JSON.stringify(articleResponse(CASH_ARTICLE_ID)), { mode: 0o640 });
      symlinkSync(targetPath, responsePath(actionDirectory));
    },
  ];

  for (const publishResponse of cases) {
    const { root, grantDirectory, actionDirectory } = setup();
    writeGrant(grantDirectory);
    const resultPromise = run(grantDirectory, actionDirectory, GRANT_ID, CASH_ARTICLE_ID);
    await waitForRequest(actionDirectory);
    publishResponse(actionDirectory, root);
    assert.deepEqual(await resultPromise, { ok: false, error: "article_unavailable" });
  }
});
