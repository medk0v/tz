import { AnimatedDisclosure } from "../AnimatedDisclosure";
import { AnimatedDetails } from "../AnimatedDetails";
import { DemoActionButton } from "./DemoReadOnly";
import { useEffect, useRef, useState } from "react";
import { Code2, Copy, KeyRound, Plus, RefreshCw, Save, Trash2 } from "lucide-react";
import {
  aiAgentApiUrl,
  ApiRequestError,
  createAiAgentApiEndpoint,
  createAiAgentApiKey,
  deleteAiAgentApiEndpoint,
  getAiAgentApiRun,
  getAiAgentApiSettings,
  listAiAgentApiRuns,
  revokeAiAgentApiKey,
  testAiAgentApiEndpoint,
  updateAiAgentApiEndpoint,
  updateAiAgentApiSettings,
  type AiAgentApiEndpoint,
  type AiAgentApiEndpointInput,
  type AiAgentApiKey,
  type AiAgentApiRun,
  type AiAgentApiSettings,
  type OperatorAuth,
} from "../api";
import { localizedError, useI18n, type MessageKey, type Translate } from "../i18n";
import "./AgentApi.css";

interface Props {
  auth: OperatorAuth;
  profileId: string;
  profileDirty: boolean;
}

interface EndpointDraft {
  name: string;
  slug: string;
  instructions: string;
  input: string;
  output: string;
  enabled: boolean;
  timeout: string;
  responseMode: "live" | "cached";
  refresh: string;
}

const statusKeys: Record<AiAgentApiRun["status"], MessageKey> = {
  pending: "ai.api.pending",
  processing: "ai.api.processing",
  completed: "ai.api.completed",
  failed: "ai.api.failed",
  cancelled: "ai.api.cancelled",
};
const activeRun = (run: AiAgentApiRun) => run.status === "pending" || run.status === "processing";
const json = (value: unknown) => JSON.stringify(value, null, 2);
const shellQuote = (value: string) => `'${value.replaceAll("'", `'\\''`)}'`;

async function copyText(value: string): Promise<void> {
  await navigator.clipboard.writeText(value);
}

function endpointDraft(endpoint: AiAgentApiEndpoint | null): EndpointDraft {
  return {
    name: endpoint?.name ?? "",
    slug: endpoint?.slug ?? "",
    instructions: endpoint?.instructions ?? "",
    input: json(endpoint?.input_example ?? { address: "SOL_ADDRESS" }),
    output: json(endpoint?.output_example ?? { address_empty: true }),
    enabled: endpoint?.enabled ?? true,
    timeout: String(endpoint?.timeout_seconds ?? 180),
    responseMode: endpoint?.cache_refresh_seconds != null ? "cached" : "live",
    refresh: String(endpoint?.cache_refresh_seconds ?? 300),
  };
}

function parseObject(text: string, field: MessageKey, t: Translate): Record<string, unknown> {
  try {
    const parsed: unknown = JSON.parse(text);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error();
    return parsed as Record<string, unknown>;
  } catch {
    throw new Error(t("ai.api.invalidJson", { field: t(field) }));
  }
}

function apiError(cause: unknown, t: Translate, fallback: MessageKey): string {
  if (cause instanceof ApiRequestError && [400, 409, 422, 429].includes(cause.status)) return cause.message;
  return localizedError(cause, t, fallback);
}

function RunResult({ run }: { run: AiAgentApiRun }) {
  const { t } = useI18n();
  const duration = run.started_at && run.completed_at
    ? Math.max(0, (Date.parse(run.completed_at) - Date.parse(run.started_at)) / 1000)
    : null;
  return <div className="agent-api-run-result">
    {activeRun(run) && <p role="status">{t("ai.api.running")}</p>}
    {duration !== null && <p>{t("ai.api.duration", { seconds: duration.toFixed(1) })}</p>}
    <AnimatedDetails><summary>{t("ai.api.input")}</summary><pre>{json(run.input)}</pre></AnimatedDetails>
    {run.result !== null && <div><strong>{t("ai.api.result")}</strong><pre>{json(run.result)}</pre></div>}
    {run.error && <div className="form-error" role="alert"><strong>{t("ai.api.error")}: {run.error.code}</strong><p>{run.error.message}</p></div>}
  </div>;
}

function EndpointEditor({ auth, endpoint, profileId, profileDirty, busy, onSave, onCancel, onTest }: {
  auth: OperatorAuth;
  endpoint: AiAgentApiEndpoint | null;
  profileId: string;
  profileDirty: boolean;
  busy: boolean;
  onSave: (input: AiAgentApiEndpointInput) => void;
  onCancel: () => void;
  onTest: (endpointId: string, input: Record<string, unknown>) => Promise<AiAgentApiRun | null>;
}) {
  const { t } = useI18n();
  const [draft, setDraft] = useState(() => endpointDraft(endpoint));
  const [error, setError] = useState("");
  const [testInput, setTestInput] = useState(() => json(endpoint?.input_example ?? {}));
  const [testRun, setTestRun] = useState<AiAgentApiRun | null>(null);
  const [pollError, setPollError] = useState("");
  const [copyNotice, setCopyNotice] = useState("");
  const dirty = json(draft) !== json(endpointDraft(endpoint));
  const pendingRunId = testRun && activeRun(testRun) ? testRun.id : null;
  const url = endpoint ? aiAgentApiUrl(profileId, endpoint.slug) : "";
  const curl = endpoint ? [
    `curl --request POST ${shellQuote(url)}`,
    "  --header 'Authorization: Bearer YOUR_API_KEY'",
    ...(endpoint.cache_refresh_seconds == null ? ["  --header 'Idempotency-Key: 7b792dd9-2ee5-4f42-b7e2-989070598209'"] : []),
    "  --header 'Content-Type: application/json'",
    `  --data ${shellQuote(json(endpoint.input_example))}`,
  ].join(" \\\n") : "";

  useEffect(() => {
    if (!pendingRunId) return;
    const runId = pendingRunId;
    let disposed = false;
    let timer: number | undefined;
    async function poll() {
      try {
        const next = await getAiAgentApiRun(auth, profileId, runId);
        if (!disposed) { setTestRun(next); setPollError(""); }
      } catch (cause) {
        if (!disposed) setPollError(apiError(cause, t, "ai.api.historyError"));
      } finally {
        if (!disposed) timer = window.setTimeout(() => void poll(), 3000);
      }
    }
    timer = window.setTimeout(() => void poll(), 2000);
    return () => { disposed = true; window.clearTimeout(timer); };
  }, [auth, profileId, pendingRunId, t]);

  function save() {
    setError("");
    const timeout = Number(draft.timeout);
    if (!draft.name.trim() || !/^[a-z][a-z0-9_-]{0,63}$/.test(draft.slug)
      || !draft.instructions.trim() || !Number.isInteger(timeout) || timeout < 10 || timeout > 300) {
      setError(t("ai.api.invalidFields"));
      return;
    }
    const refresh = Number(draft.refresh);
    if (draft.responseMode === "cached" && (!Number.isInteger(refresh) || refresh < 60 || refresh > 86400)) {
      setError(t("ai.api.invalidRefresh"));
      return;
    }
    try {
      onSave({
        name: draft.name.trim(), slug: draft.slug, instructions: draft.instructions.trim(),
        enabled: draft.enabled, timeout_seconds: timeout,
        cache_refresh_seconds: draft.responseMode === "cached" ? refresh : null,
        response_wait_seconds: endpoint?.response_wait_seconds ?? 180,
        input_example: parseObject(draft.input, "ai.api.inputExample", t),
        output_example: parseObject(draft.output, "ai.api.outputExample", t),
      });
    } catch (cause) { setError(cause instanceof Error ? cause.message : t("ai.api.invalidFields")); }
  }

  async function test() {
    if (!endpoint || dirty || profileDirty || busy) return;
    setError("");
    try {
      const started = await onTest(endpoint.id, parseObject(testInput, "ai.api.testInput", t));
      if (started) { setTestRun(started); setPollError(""); }
    } catch (cause) { setError(cause instanceof Error ? cause.message : t("ai.api.testError")); }
  }

  return <div className="agent-api-editor">
    <h4>{t(endpoint ? "ai.api.editMethod" : "ai.api.newMethod")}</h4>
    <div className="agent-api-fields">
      <label><span>{t("ai.api.name")}</span><input maxLength={100} value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
      <label><span>{t("ai.api.slug")}</span><input maxLength={64} spellCheck={false} value={draft.slug} placeholder="check-sol-address" onChange={(event) => setDraft({ ...draft, slug: event.target.value })} /></label>
    </div>
    <p>{t("ai.api.slugHelp")}</p>
    <label><span>{t("ai.api.instructions")}</span><textarea rows={5} maxLength={50000} value={draft.instructions} onChange={(event) => setDraft({ ...draft, instructions: event.target.value })} /></label>
    <p>{t("ai.api.instructionsHelp")}</p>
    <div className="agent-api-fields">
      <label><span>{t("ai.api.inputExample")}</span><textarea className="agent-api-json" rows={6} spellCheck={false} value={draft.input} onChange={(event) => setDraft({ ...draft, input: event.target.value })} /></label>
      <label><span>{t("ai.api.outputExample")}</span><textarea className="agent-api-json" rows={6} spellCheck={false} value={draft.output} onChange={(event) => setDraft({ ...draft, output: event.target.value })} /></label>
    </div>
    <p>{t("ai.api.exampleHelp")}</p>
    <div className="agent-api-toolbar">
      <label className="agent-api-timeout"><span>{t("ai.api.timeout")}</span><input inputMode="numeric" value={draft.timeout} onChange={(event) => setDraft({ ...draft, timeout: event.target.value })} /></label>
      <label className="agent-api-checkbox"><input type="checkbox" checked={draft.enabled} onChange={(event) => setDraft({ ...draft, enabled: event.target.checked })} /><span>{t("ai.api.methodEnabled")}</span></label>
    </div>
    <div className="agent-api-toolbar">
      <label><span>{t("ai.api.responseMode")}</span><select value={draft.responseMode} onChange={(event) => setDraft({ ...draft, responseMode: event.target.value === "cached" ? "cached" : "live" })}>
        <option value="live">{t("ai.api.live")}</option>
        <option value="cached">{t("ai.api.cached")}</option>
      </select></label>
      {draft.responseMode === "cached" && <label className="agent-api-timeout"><span>{t("ai.api.refreshInterval")}</span><input inputMode="numeric" value={draft.refresh} onChange={(event) => setDraft({ ...draft, refresh: event.target.value })} /></label>}
    </div>
    <p>{t(draft.responseMode === "cached" ? "ai.api.cacheHelp" : "ai.api.waitHelp")}</p>
    {error && <p className="form-error" role="alert">{error}</p>}
    <div className="agent-api-toolbar">
      <DemoActionButton type="button" className="primary-button" disabled={busy} onClick={save}><Save size={16} />{t("ai.api.saveMethod")}</DemoActionButton>
      <button type="button" className="secondary-button" disabled={busy} onClick={onCancel}>{t("ai.api.cancel")}</button>
    </div>
    {endpoint && <>
      <section className="agent-api-request" aria-label={t("ai.api.request")}>
        <h4>{t("ai.api.request")}</h4>
        <code className="agent-api-url">POST {url}</code>
        <pre>{curl}</pre>
        <button type="button" className="secondary-button" onClick={() => {
          void copyText(curl).then(() => setCopyNotice(t("ai.api.copied"))).catch(() => setCopyNotice(t("ai.api.copyError")));
        }}><Copy size={15} />{t("ai.api.copy")}</button>
        {copyNotice && <p role="status">{copyNotice}</p>}
        {endpoint.cache_refresh_seconds != null
          ? <p>{t("ai.api.cachedResponseHelp")}</p>
          : <><p>{t("ai.api.asyncHelp")}</p><p>{t("ai.api.retryHelp")}</p></>}
      </section>
      <section className="agent-api-test" aria-label={t("ai.api.test")}>
        <h4>{t("ai.api.test")}</h4>
        <p>{t("ai.api.testHelp")}</p>
        <label><span>{t("ai.api.testInput")}</span><textarea className="agent-api-json" spellCheck={false} rows={5} value={testInput} onChange={(event) => setTestInput(event.target.value)} /></label>
        {(dirty || profileDirty) && <p role="status">{t("ai.api.saveFirst")}</p>}
        <DemoActionButton type="button" className="secondary-button" disabled={busy || dirty || profileDirty || Boolean(pendingRunId)} onClick={() => void test()}>{t("ai.api.test")}</DemoActionButton>
        {pollError && <p className="form-error" role="alert">{pollError}</p>}
        {testRun && <RunResult run={testRun} />}
      </section>
    </>}
  </div>;
}

function AgentApiSettings({ auth, profileId, profileDirty }: Props) {
  const { t, formatDate } = useI18n();
  const [settings, setSettings] = useState<AiAgentApiSettings | null>(null);
  const [runs, setRuns] = useState<AiAgentApiRun[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [secret, setSecret] = useState<AiAgentApiKey | null>(null);
  const [busy, setBusy] = useState(true);
  const [loadCause, setLoadCause] = useState<unknown>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [historyError, setHistoryError] = useState("");
  const alive = useRef(false);
  const selectedEndpoint = settings?.endpoints.find((item) => item.id === selectedId) ?? null;
  const hasActiveRuns = runs.some(activeRun);
  const visibleSecret = secret && settings?.key_configured && settings.can_manage_key && settings.key_prefix === secret.key_prefix ? secret.key : null;

  useEffect(() => {
    alive.current = true;
    let disposed = false;
    Promise.all([getAiAgentApiSettings(auth, profileId), listAiAgentApiRuns(auth, profileId)])
      .then(([nextSettings, nextRuns]) => {
        if (!disposed) { setSettings(nextSettings); setRuns(nextRuns); }
      })
      .catch((cause: unknown) => { if (!disposed) setLoadCause(cause); })
      .finally(() => { if (!disposed) setBusy(false); });
    return () => { disposed = true; alive.current = false; };
  }, [auth, profileId]);

  useEffect(() => {
    if (!hasActiveRuns) return;
    let disposed = false;
    let timer: number | undefined;
    async function poll() {
      try {
        const next = await listAiAgentApiRuns(auth, profileId);
        if (!disposed) { setRuns(next); setHistoryError(""); }
      } catch (cause) {
        if (!disposed) setHistoryError(apiError(cause, t, "ai.api.historyError"));
      } finally {
        if (!disposed) timer = window.setTimeout(() => void poll(), 3000);
      }
    }
    timer = window.setTimeout(() => void poll(), 2000);
    return () => { disposed = true; window.clearTimeout(timer); };
  }, [auth, profileId, t, hasActiveRuns]);

  async function action(work: () => Promise<void>) {
    if (busy) return;
    setBusy(true); setError(""); setNotice("");
    try { await work(); }
    catch (cause) { if (alive.current) setError(apiError(cause, t, "ai.api.saveError")); }
    finally { if (alive.current) setBusy(false); }
  }

  async function refresh() {
    setLoadCause(null);
    await action(async () => {
      const [nextSettings, nextRuns] = await Promise.all([getAiAgentApiSettings(auth, profileId), listAiAgentApiRuns(auth, profileId)]);
      if (alive.current) { setSettings(nextSettings); setRuns(nextRuns); setHistoryError(""); }
    });
  }

  async function refreshRuns() {
    try {
      const next = await listAiAgentApiRuns(auth, profileId);
      if (alive.current) { setRuns(next); setHistoryError(""); }
    } catch (cause) { if (alive.current) setHistoryError(apiError(cause, t, "ai.api.historyError")); }
  }

  async function saveEndpoint(input: AiAgentApiEndpointInput) {
    await action(async () => {
      const saved = selectedEndpoint
        ? await updateAiAgentApiEndpoint(auth, profileId, selectedEndpoint.id, input)
        : await createAiAgentApiEndpoint(auth, profileId, input);
      if (!alive.current) return;
      setSettings((current) => current && ({ ...current, endpoints: current.endpoints.some((item) => item.id === saved.id)
        ? current.endpoints.map((item) => item.id === saved.id ? saved : item) : [...current.endpoints, saved] }));
      setSelectedId(saved.id);
      setNotice(t("ai.api.saved"));
    });
  }

  async function startTest(endpointId: string, input: Record<string, unknown>): Promise<AiAgentApiRun | null> {
    if (busy || profileDirty) return null;
    setBusy(true); setError(""); setNotice("");
    try {
      const started = await testAiAgentApiEndpoint(auth, profileId, endpointId, input);
      if (!alive.current) return null;
      setRuns((current) => [started, ...current.filter((run) => run.id !== started.id)]);
      return started;
    } catch (cause) {
      if (alive.current) setError(apiError(cause, t, "ai.api.testError"));
      return null;
    } finally { if (alive.current) setBusy(false); }
  }

  return <div className="agent-api-content">
    <p>{t("ai.api.help")}</p>
    {loadCause !== null && <p className="form-error" role="alert">{apiError(loadCause, t, "ai.api.loadError")}</p>}
    {error && <p className="form-error" role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    <div className="agent-api-toolbar">
      {settings && <label className="agent-api-checkbox"><input type="checkbox" checked={settings.enabled} disabled={auth.isDemo || busy} onChange={(event) => {
        const enabled = event.target.checked;
        void action(async () => {
          const next = await updateAiAgentApiSettings(auth, profileId, enabled);
          if (alive.current) { setSettings(next); setNotice(t("ai.api.saved")); }
        });
      }} /><span>{t("ai.api.enabled")}</span></label>}
      <button type="button" className="secondary-button" disabled={busy} onClick={() => void refresh()}><RefreshCw size={15} />{t("ai.api.refresh")}</button>
    </div>
    {!settings && !error && loadCause === null && <p role="status">{t("ai.api.loading")}</p>}
    {settings && <>
      <section className="agent-api-key" aria-label={t("ai.api.key")}>
        <h4>{t("ai.api.key")}</h4>
        <p>{t("ai.api.keyHelp")}</p>
        {settings.key_configured ? <code>{settings.key_prefix}…</code> : <p>{t("ai.api.noKey")}</p>}
        {settings.can_manage_key ? <div className="agent-api-toolbar">
          <DemoActionButton type="button" className="secondary-button" disabled={busy} onClick={() => {
            if (settings.key_configured && !window.confirm(t("ai.api.rotateConfirm"))) return;
            void action(async () => {
              setSecret(null);
              const created = await createAiAgentApiKey(auth, profileId);
              if (!alive.current) return;
              setSecret(created);
              setSettings((current) => current && ({ ...current, key_configured: true, key_prefix: created.key_prefix }));
            });
          }}><KeyRound size={15} />{t(settings.key_configured ? "ai.api.rotateKey" : "ai.api.createKey")}</DemoActionButton>
          {settings.key_configured && <DemoActionButton type="button" className="secondary-button table-danger-link" disabled={busy} onClick={() => {
            if (!window.confirm(t("ai.api.revokeConfirm"))) return;
            void action(async () => {
              setSecret(null);
              await revokeAiAgentApiKey(auth, profileId);
              if (!alive.current) return;
              setSettings((current) => current && ({ ...current, key_configured: false, key_prefix: null }));
              setNotice(t("ai.api.saved"));
            });
          }}>{t("ai.api.revokeKey")}</DemoActionButton>}
        </div> : <p>{t("ai.api.keyRestricted")}</p>}
        {visibleSecret && <div className="agent-api-secret"><p role="status">{t("ai.api.keyOnce")}</p><code>{visibleSecret}</code>
          <button type="button" className="secondary-button" onClick={() => {
            void copyText(visibleSecret).then(() => setNotice(t("ai.api.copied"))).catch(() => setError(t("ai.api.copyError")));
          }}><Copy size={15} />{t("ai.api.copy")}</button>
        </div>}
      </section>
      <section className="agent-api-methods" aria-label={t("ai.api.methods")}>
        <div className="agent-api-toolbar"><h4>{t("ai.api.methods")}</h4><button type="button" className="secondary-button" disabled={busy || settings.endpoints.length >= 32} onClick={() => setSelectedId("new")}><Plus size={15} />{t("ai.api.addMethod")}</button></div>
        {settings.endpoints.length === 0 && <p>{t("ai.api.noMethods")}</p>}
        <ul className="agent-api-method-list">{settings.endpoints.map((endpoint) => <li key={endpoint.id}>
          <button type="button" className="agent-api-method-name" disabled={busy} aria-expanded={selectedId === endpoint.id} onClick={() => setSelectedId(endpoint.id)}>
            <strong>{endpoint.name}</strong>{" "}<code>/{endpoint.slug}</code>{!endpoint.enabled && <span>{t("ai.api.disabled")}</span>}
          </button>
          <DemoActionButton type="button" className="icon-button table-danger-link" disabled={busy} aria-label={`${t("ai.api.deleteMethod")}: ${endpoint.name}`} onClick={() => {
            if (!window.confirm(t("ai.api.deleteConfirm", { name: endpoint.name }))) return;
            void action(async () => {
              await deleteAiAgentApiEndpoint(auth, profileId, endpoint.id);
              if (!alive.current) return;
              setSettings((current) => current && ({ ...current, endpoints: current.endpoints.filter((item) => item.id !== endpoint.id) }));
              if (selectedId === endpoint.id) setSelectedId(null);
            });
          }}><Trash2 size={16} /></DemoActionButton>
        </li>)}</ul>
        {(selectedEndpoint || selectedId === "new") && <EndpointEditor key={selectedEndpoint ? `${selectedEndpoint.id}:${selectedEndpoint.version}` : "new"}
          auth={auth} endpoint={selectedEndpoint} profileId={profileId} profileDirty={profileDirty} busy={busy}
          onSave={(input) => void saveEndpoint(input)} onCancel={() => setSelectedId(null)} onTest={startTest} />}
      </section>
      <section className="agent-api-history" aria-label={t("ai.api.history")}>
        <div className="agent-api-toolbar"><h4>{t("ai.api.history")}</h4><button type="button" className="secondary-button" disabled={busy} onClick={() => void refreshRuns()}><RefreshCw size={15} />{t("ai.api.refresh")}</button></div>
        {historyError && <p className="form-error" role="alert">{historyError}</p>}
        {runs.length === 0 && <p>{t("ai.api.historyEmpty")}</p>}
        {runs.map((run) => <AnimatedDetails key={run.id} className="agent-api-history-run"><summary>
          {run.endpoint_name} · {t(statusKeys[run.status])} · <time dateTime={run.created_at}>{formatDate(run.created_at, { dateStyle: "short", timeStyle: "medium" })}</time>
        </summary><RunResult run={run} /></AnimatedDetails>)}
      </section>
    </>}
  </div>;
}

export function AgentApi(props: Props) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [scope, setScope] = useState({ auth: props.auth, profileId: props.profileId });
  if (scope.profileId !== props.profileId || scope.auth.kind !== props.auth.kind
    || (scope.auth.kind === "access_token" && props.auth.kind === "access_token" && scope.auth.token !== props.auth.token)) {
    setScope({ auth: props.auth, profileId: props.profileId });
    setOpen(false);
  }
  return <fieldset className="ai-assignment agent-api"><legend>{t("ai.api.title")}</legend>
    <button type="button" className="secondary-button" aria-expanded={open} onClick={() => setOpen(!open)}><Code2 size={16} />{t("ai.api.open")}</button>
    <AnimatedDisclosure open={open}><AgentApiSettings key={props.profileId} {...props} auth={scope.auth} /></AnimatedDisclosure>
  </fieldset>;
}
