#!/usr/bin/env node

import {
  closeSync,
  constants,
  fchmodSync,
  fstatSync,
  fsyncSync,
  linkSync,
  lstatSync,
  openSync,
  readFileSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const MAX_GRANT_BYTES = 3 * 1024 * 1024;
const MAX_RESPONSE_BYTES = 2 * 1024 * 1024;
const MAX_STDOUT_CHARS = 150_000;
const MAX_ARTICLE_OFFSET = 200_000;
const MAX_CONTENT_UTF16_UNITS = 40_000;
const RESPONSE_TIMEOUT_MS = 30_000;
const POLL_INTERVAL_MS = 25;
const GRANT_DIRECTORY = "/run/support-agent-secrets";
const ACTION_DIRECTORY = "/run/support-agent-actions";
let activeLockPath;

async function finish(payload) {
  if (activeLockPath !== undefined) {
    try {
      removeFileIfPresent(activeLockPath);
    } catch {
      payload = { ok: false, error: "article_unavailable" };
    }
    activeLockPath = undefined;
  }
  let output = `${JSON.stringify(payload)}\n`;
  if (output.length > MAX_STDOUT_CHARS) {
    output = '{"ok":false,"error":"article_unavailable"}\n';
  }
  await new Promise((resolveWrite, rejectWrite) => {
    process.stdout.write(output, (error) => {
      if (error) rejectWrite(error);
      else resolveWrite();
    });
  });
  process.exit(0);
}

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function readSafeFile(path, maximumBytes) {
  let handle;
  try {
    handle = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW);
    const metadata = fstatSync(handle);
    if (
      !metadata.isFile()
      || (metadata.mode & 0o022) !== 0
      || metadata.size === 0
      || metadata.size > maximumBytes
    ) {
      return undefined;
    }
    return readFileSync(handle, "utf8");
  } catch {
    return undefined;
  } finally {
    if (handle !== undefined) {
      try {
        closeSync(handle);
      } catch {
        // The caller receives a generic unavailable result.
      }
    }
  }
}

function removeFileIfPresent(path) {
  try {
    unlinkSync(path);
  } catch (error) {
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }
}

function writeAtomicMarker(temporaryPath, markerPath, content) {
  let handle;
  try {
    handle = openSync(
      temporaryPath,
      constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW,
      0o640,
    );
    writeFileSync(handle, content, "utf8");
    fchmodSync(handle, 0o640);
    fsyncSync(handle);
    closeSync(handle);
    handle = undefined;
    linkSync(temporaryPath, markerPath);
    unlinkSync(temporaryPath);
  } catch (error) {
    if (handle !== undefined) {
      try {
        closeSync(handle);
      } catch {
        // The original write error is the actionable failure.
      }
    }
    try {
      removeFileIfPresent(temporaryPath);
    } catch {
      // The caller receives a generic action error.
    }
    throw error;
  }
}

async function acquireKnowledgeLock(path) {
  const deadline = Date.now() + RESPONSE_TIMEOUT_MS;
  while (Date.now() < deadline) {
    let handle;
    try {
      handle = openSync(
        path,
        constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW,
        0o640,
      );
      writeFileSync(handle, `${process.pid}\n`, "utf8");
      fchmodSync(handle, 0o640);
      fsyncSync(handle);
      closeSync(handle);
      activeLockPath = path;
      return;
    } catch (error) {
      if (handle !== undefined) {
        try {
          closeSync(handle);
        } catch {
          // The original lock error is the actionable failure.
        }
        try {
          removeFileIfPresent(path);
        } catch {
          // The caller receives a generic action error.
        }
      }
      if (error?.code !== "EEXIST") {
        throw error;
      }
      await delay(POLL_INTERVAL_MS);
    }
  }
  throw new Error("knowledge_lock_timeout");
}

function isValidArticle(article, articleId, offset, version) {
  return isObject(article)
    && typeof article.article_id === "string"
    && article.article_id.toLowerCase() === articleId
    && Number.isSafeInteger(article.version)
    && article.version > 0
    && (version === null || article.version === version)
    && typeof article.knowledge_base === "string"
    && article.knowledge_base.length > 0
    && article.knowledge_base.length <= 400
    && typeof article.title === "string"
    && article.title.length > 0
    && article.title.length <= 600
    && (article.source_url === null
      || (typeof article.source_url === "string" && article.source_url.length <= 8_000))
    && typeof article.content === "string"
    && article.content.length > 0
    && article.content.length <= MAX_CONTENT_UTF16_UNITS
    && (
      article.next_offset === null
      || (
        Number.isSafeInteger(article.next_offset)
        && article.next_offset > offset
        && article.next_offset <= MAX_ARTICLE_OFFSET
      )
    );
}

export async function runKnowledgeArticle(
  arguments_,
  {
    grantDirectory: grantDirectoryInput = GRANT_DIRECTORY,
    actionDirectory: actionDirectoryInput = ACTION_DIRECTORY,
  } = {},
) {
const [grantIdInput, articleIdInput, offsetInput, versionInput, ...extraArguments] = arguments_;
if (
  extraArguments.length !== 0
  || !UUID_PATTERN.test(grantIdInput ?? "")
  || !UUID_PATTERN.test(articleIdInput ?? "")
  || (
    offsetInput !== undefined
    && (!/^(0|[1-9][0-9]{0,5})$/.test(offsetInput) || Number(offsetInput) > MAX_ARTICLE_OFFSET)
  )
  || (offsetInput === undefined && versionInput !== undefined)
  || (
    Number(offsetInput ?? "0") > 0
    && (
      !/^[1-9][0-9]{0,18}$/.test(versionInput ?? "")
      || !Number.isSafeInteger(Number(versionInput))
    )
  )
  || (Number(offsetInput ?? "0") === 0 && versionInput !== undefined)
) {
  await finish({ ok: false, error: "invalid_arguments" });
}

const grantId = grantIdInput.toLowerCase();
const articleId = articleIdInput.toLowerCase();
const offset = Number(offsetInput ?? "0");
const version = versionInput === undefined ? null : Number(versionInput);
const grantDirectory = resolve(grantDirectoryInput);
const actionDirectory = resolve(actionDirectoryInput);
const grantPath = resolve(join(grantDirectory, `${grantId}.json`));
const requestPath = resolve(join(actionDirectory, `${grantId}.knowledge-request`));
const requestTemporaryPath = resolve(join(actionDirectory, `${grantId}.knowledge-request.tmp`));
const responsePath = resolve(join(actionDirectory, `${grantId}.knowledge-response`));
const responseTemporaryPath = resolve(join(actionDirectory, `${grantId}.knowledge-response.tmp`));
const lockPath = resolve(join(actionDirectory, `${grantId}.knowledge-lock`));
if (
  dirname(grantPath) !== grantDirectory
  || dirname(requestPath) !== actionDirectory
  || dirname(requestTemporaryPath) !== actionDirectory
  || dirname(responsePath) !== actionDirectory
  || dirname(responseTemporaryPath) !== actionDirectory
  || dirname(lockPath) !== actionDirectory
) {
  await finish({ ok: false, error: "invalid_grant" });
}

let grant;
const grantContent = readSafeFile(grantPath, MAX_GRANT_BYTES);
if (grantContent === undefined) {
  await finish({ ok: false, error: "invalid_grant" });
}
try {
  grant = JSON.parse(grantContent);
} catch {
  await finish({ ok: false, error: "invalid_grant" });
}
if (
  !isObject(grant)
  || grant.version !== 4
  || !Number.isSafeInteger(grant.expires_at_unix_ms)
  || grant.expires_at_unix_ms <= Date.now()
  || grant.knowledge_lookup !== true
) {
  await finish({ ok: false, error: "not_allowed" });
}

try {
  const metadata = lstatSync(actionDirectory);
  if (!metadata.isDirectory() || metadata.isSymbolicLink()) {
    await finish({ ok: false, error: "action_unavailable" });
  }
  await acquireKnowledgeLock(lockPath);
  try {
    lstatSync(requestPath);
    await finish({ ok: false, error: "action_unavailable" });
  } catch (error) {
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }
  removeFileIfPresent(requestTemporaryPath);
  removeFileIfPresent(responsePath);
  removeFileIfPresent(responseTemporaryPath);
  writeAtomicMarker(
    requestTemporaryPath,
    requestPath,
    `${JSON.stringify({ article_id: articleId, offset, version })}\n`,
  );
} catch {
  await finish({ ok: false, error: "action_unavailable" });
}

const deadline = Date.now() + RESPONSE_TIMEOUT_MS;
let response;
while (Date.now() < deadline) {
  const responseContent = readSafeFile(responsePath, MAX_RESPONSE_BYTES);
  if (responseContent !== undefined) {
    try {
      response = JSON.parse(responseContent);
    } catch {
      response = undefined;
    }
    break;
  }
  try {
    lstatSync(responsePath);
    break;
  } catch (error) {
    if (error?.code !== "ENOENT") {
      break;
    }
  }
  await delay(POLL_INTERVAL_MS);
}

try {
  removeFileIfPresent(requestPath);
  removeFileIfPresent(requestTemporaryPath);
  removeFileIfPresent(responsePath);
  removeFileIfPresent(responseTemporaryPath);
} catch {
  await finish({ ok: false, error: "article_unavailable" });
}

if (isObject(response) && response.ok === false && response.error === "article_unavailable") {
  await finish({ ok: false, error: "article_unavailable" });
}
if (
  !isObject(response)
  || response.ok !== true
  || !isValidArticle(response.article, articleId, offset, version)
) {
  await finish({ ok: false, error: "article_unavailable" });
}

await finish({
  ok: true,
  article: {
    article_id: articleId,
    version: response.article.version,
    knowledge_base: response.article.knowledge_base,
    title: response.article.title,
    source_url: response.article.source_url,
    content: response.article.content,
    next_offset: response.article.next_offset,
  },
});
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await runKnowledgeArticle(process.argv.slice(2));
}
