import { useContentMotion } from "./useContentMotion";
import { DemoActionButton } from "./DemoReadOnly";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { Plus, Save, Trash2 } from "lucide-react";
import {
  createApiIntegration,
  deleteApiIntegration,
  listApiIntegrations,
  updateApiIntegration,
  type ApiIntegration,
  type ApiIntegrationActionInput,
  type ApiIntegrationAiProfile,
  type ApiIntegrationAuthKind,
  type ApiIntegrationInput,
  type OperatorAuth,
} from "../api";
import { useI18n, type MessageKey } from "../i18n";
import { sameSegments, useCanonicalPageRoute, usePageRoute } from "./page-route";
import "./IntegrationsView.css";

interface IntegrationsViewProps {
  auth: OperatorAuth;
  canManageAi: boolean;
}

interface Notice {
  kind: "success" | "error";
  message: MessageKey;
}

interface ActionDraft extends ApiIntegrationActionInput {
  rowId: string;
}

type IntegrationDraft = Omit<ApiIntegrationInput, "actions" | "clear_token" | "token"> & {
  actions: ActionDraft[];
};

let actionRowSequence = 0;
const integrationKeyPattern = /^[a-z][a-z0-9_]{1,63}$/;
const actionKeyPattern = /^[a-z][a-z0-9_]{1,63}$/;
const parameterNamePattern = /^[a-z][a-z0-9_]{0,63}$/;
const controlCharacterPattern = /\p{Cc}/u;
const maximumActions = 32;
const maximumParameters = 32;

function actionRowId() {
  actionRowSequence += 1;
  return `integration-action-${actionRowSequence}`;
}

function emptyAction(): ActionDraft {
  return {
    rowId: actionRowId(),
    key: "",
    name: "",
    description: "",
    path_template: "",
  };
}

function emptyDraft(): IntegrationDraft {
  return {
    name: "",
    key: "",
    description: "",
    base_url: "",
    status: "active",
    auth_kind: "none",
    actions: [emptyAction()],
    ai_profile_ids: [],
  };
}

function draftFromIntegration(integration: ApiIntegration): IntegrationDraft {
  return {
    visibility: integration.visibility,
    name: integration.name,
    key: integration.key,
    description: integration.description,
    base_url: integration.base_url,
    status: integration.status,
    auth_kind: integration.auth_kind,
    actions: integration.actions.map((action) => ({
      rowId: `stored-action-${action.id}`,
      key: action.key,
      name: action.name,
      description: action.description,
      path_template: action.path_template,
    })),
    ai_profile_ids: [...integration.ai_profile_ids],
  };
}

function httpsBaseUrlIsValid(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "https:"
      && url.username === ""
      && url.password === ""
      && url.search === ""
      && url.hash === ""
      && (url.port === "" || url.port === "443");
  } catch {
    return false;
  }
}

function pathTemplateIsValid(value: string): boolean {
  if (
    value.length === 0
    || value.length > 2000
    || !value.startsWith("/")
    || value.includes("//")
    || value.includes("://")
    || /[\\#%]/.test(value)
    || controlCharacterPattern.test(value)
  ) return false;
  const [path] = value.split("?", 1);
  if (!path.split("/").every((segment) => segment !== "." && segment !== "..")) return false;

  const parameters = new Set<string>();
  let remaining = value;
  while (remaining.length > 0) {
    const markerIndex = remaining.search(/[{}]/);
    if (markerIndex === -1) break;
    if (remaining[markerIndex] === "}") return false;
    const closeIndex = remaining.indexOf("}", markerIndex + 1);
    if (closeIndex === -1) return false;
    const parameter = remaining.slice(markerIndex + 1, closeIndex);
    if (!parameterNamePattern.test(parameter)) return false;
    parameters.add(parameter);
    if (parameters.size > maximumParameters) return false;
    remaining = remaining.slice(closeIndex + 1);
  }
  return true;
}

function actionIsValid(action: ActionDraft): boolean {
  return actionKeyPattern.test(action.key.trim())
    && action.name.trim() !== ""
    && action.description.trim() !== ""
    && pathTemplateIsValid(action.path_template.trim());
}

function profileStatusKey(status: ApiIntegrationAiProfile["status"]): MessageKey {
  if (status === "active") return "integrations.agentStatusActive";
  if (status === "disabled") return "integrations.agentStatusDisabled";
  return "integrations.agentStatusDraft";
}

/** Integration URLs: `/integrations`, `/integrations/new` and `/integrations/<id>`. */
export function IntegrationsView({ auth, canManageAi }: IntegrationsViewProps) {
  const { t } = useI18n();
  const route = usePageRoute();
  const navigateRoute = route.navigate;
  const routeSegmentsRef = useRef(route.segments);
  const routedId = route.segments[0] ?? null;
  const [integrations, setIntegrations] = useState<ApiIntegration[]>([]);
  const [profiles, setProfiles] = useState<ApiIntegrationAiProfile[]>([]);
  const selectedId: string | "new" | null = routedId === "new" ? "new" : integrations.find((item) => item.id === routedId)?.id ?? null;
  const editorMotion = useContentMotion<HTMLDivElement>(selectedId ?? "none");
  const [draft, setDraft] = useState<IntegrationDraft>(emptyDraft);
  const [token, setToken] = useState("");
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const selectedIntegration = integrations.find((item) => item.id === selectedId) ?? null;
  // Links and Back/Forward open connections through the URL; the draft follows the open connection.
  const [shownId, setShownId] = useState(selectedId);
  if (selectedId !== shownId) {
    setShownId(selectedId);
    setDraft(selectedIntegration ? draftFromIntegration(selectedIntegration) : emptyDraft());
    setToken("");
  }
  const actionKeys = draft.actions.map((action) => action.key.trim());
  const actionsAreValid = draft.actions.length > 0
    && draft.actions.length <= maximumActions
    && draft.actions.every(actionIsValid)
    && new Set(actionKeys).size === actionKeys.length;
  const storedTokenIsAvailable = Boolean(selectedIntegration?.token_configured);
  const authIsValid = draft.auth_kind === "none"
    || token.trim().length >= 8
    || storedTokenIsAvailable;
  const draftIsValid = draft.name.trim() !== ""
    && integrationKeyPattern.test(draft.key.trim())
    && draft.description.trim() !== ""
    && httpsBaseUrlIsValid(draft.base_url.trim())
    && actionsAreValid
    && authIsValid;

  useEffect(() => {
    let active = true;
    listApiIntegrations(auth)
      .then((response) => {
        if (!active) return;
        setIntegrations(response.items);
        setProfiles(response.ai_profiles);
      })
      .catch(() => {
        if (active) {
          setLoadFailed(true);
          setNotice({ kind: "error", message: "integrations.loadError" });
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [auth]);

  useEffect(() => { routeSegmentsRef.current = route.segments; }, [route.segments]);
  // A link to a missing connection returns to the list; a failed load keeps the link for a retry.
  useCanonicalPageRoute(route, selectedId ? [selectedId] : [], !loading && !loadFailed);

  function startIntegration() {
    setDraft(emptyDraft());
    setToken("");
    setNotice(null);
    void navigateRoute(["new"]);
  }

  function selectIntegration(integration: ApiIntegration) {
    setDraft(draftFromIntegration(integration));
    setToken("");
    setNotice(null);
    void navigateRoute([integration.id]);
  }

  function updateAction(rowId: string, field: keyof ApiIntegrationActionInput, value: string) {
    setDraft((current) => ({
      ...current,
      actions: current.actions.map((action) => (
        action.rowId === rowId ? { ...action, [field]: value } : action
      )),
    }));
  }

  function removeAction(rowId: string) {
    setDraft((current) => ({
      ...current,
      actions: current.actions.filter((action) => action.rowId !== rowId),
    }));
  }

  function changeAuthKind(authKind: ApiIntegrationAuthKind) {
    setDraft((current) => ({ ...current, auth_kind: authKind }));
    setToken("");
  }

  function toggleProfile(profileId: string) {
    setDraft((current) => ({
      ...current,
      ai_profile_ids: current.ai_profile_ids.includes(profileId)
        ? current.ai_profile_ids.filter((id) => id !== profileId)
        : [...current.ai_profile_ids, profileId],
    }));
  }

  async function saveIntegration(event: FormEvent) {
    event.preventDefault();
    if (!selectedId || !draftIsValid || saving) return;
    setSaving(true);
    setNotice(null);
    const opened = route.segments;
    try {
      const payload: ApiIntegrationInput = {
        ...(draft.visibility ? { visibility: draft.visibility } : {}),
        name: draft.name.trim(),
        key: draft.key.trim(),
        description: draft.description.trim(),
        base_url: draft.base_url.trim(),
        status: draft.status,
        auth_kind: draft.auth_kind,
        ...(draft.auth_kind !== "none" && token.trim() ? { token: token.trim() } : {}),
        clear_token: draft.auth_kind === "none" && Boolean(selectedIntegration?.token_configured),
        actions: draft.actions.map((action) => ({
          key: action.key.trim(),
          name: action.name.trim(),
          description: action.description.trim(),
          path_template: action.path_template.trim(),
        })),
        ai_profile_ids: [...draft.ai_profile_ids],
      };
      const saved = selectedId === "new"
        ? await createApiIntegration(auth, payload)
        : await updateApiIntegration(auth, selectedId, payload);
      setIntegrations((current) => selectedId === "new"
        ? [saved, ...current.filter((item) => item.id !== saved.id)]
        : current.map((item) => item.id === saved.id ? saved : item));
      // Back/Forward during the request opened another connection, which keeps its own draft.
      if (!sameSegments(routeSegmentsRef.current, opened)) return;
      setDraft(draftFromIntegration(saved));
      setToken("");
      setNotice({ kind: "success", message: "integrations.saved" });
      if (selectedId === "new") void navigateRoute([saved.id], { replace: true });
    } catch {
      setNotice({ kind: "error", message: "integrations.saveError" });
    } finally {
      setSaving(false);
    }
  }

  async function removeIntegration() {
    if (
      !selectedIntegration
      || saving
      || !window.confirm(t("integrations.deleteConfirm", { name: selectedIntegration.name }))
    ) return;
    setSaving(true);
    setNotice(null);
    const opened = route.segments;
    try {
      await deleteApiIntegration(auth, selectedIntegration.id);
      setIntegrations((current) => current.filter((item) => item.id !== selectedIntegration.id));
      if (sameSegments(routeSegmentsRef.current, opened)) void navigateRoute([], { replace: true });
      setNotice({ kind: "success", message: "integrations.deleted" });
    } catch {
      setNotice({ kind: "error", message: "integrations.deleteError" });
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="page integrations-page">
      <header className="page-toolbar">
        <div>
          <h1>{t("integrations.title")}</h1>
          <p>{t("integrations.description")}</p>
        </div>
        <button className="primary-button" type="button" disabled={saving} onClick={startIntegration}>
          <Plus size={16} />{t("integrations.new")}
        </button>
      </header>

      {notice && (
        <div className={`admin-notice admin-notice--${notice.kind}`} role={notice.kind === "error" ? "alert" : "status"}>
          {t(notice.message)}
        </div>
      )}

      <div className="settings-layout integrations-layout">
        <aside className="settings-list" aria-label={t("integrations.listLabel")}>
          <header className="integration-list-heading">
            <h2>{t("integrations.listLabel")}</h2>
            {!loading && <span>{integrations.length}</span>}
          </header>
          {integrations.map((integration) => (
            <button
              className={`settings-row ${selectedId === integration.id ? "settings-row--active" : ""}`}
              type="button"
              aria-pressed={selectedId === integration.id}
              disabled={saving}
              key={integration.id}
              onClick={() => selectIntegration(integration)}
            >
              <strong>{integration.name}</strong>
              <span>{t(integration.status === "active" ? "integrations.statusActive" : "integrations.statusDisabled")} · {t("integrations.actionCount", { count: integration.actions.length })}</span>
            </button>
          ))}
          {loading && <div className="empty-state">{t("integrations.loading")}</div>}
          {!loading && integrations.length === 0 && <div className="empty-state">{t("integrations.empty")}</div>}
        </aside>

        <div ref={editorMotion} className="settings-panel integrations-editor-panel">
          {selectedId ? (
            <form onSubmit={saveIntegration}>
              <header>
                <h2>{selectedId === "new" ? t("integrations.newTitle") : draft.name}</h2>
                <span>{draft.auth_kind === "none"
                  ? t("integrations.authNotRequired")
                  : t(selectedIntegration?.token_configured ? "integrations.tokenConfigured" : "integrations.tokenMissing")}</span>
              </header>
              <div className="settings-form integrations-form">
                <p className="integration-boundary-note">{t("integrations.readOnlyNote")}</p>

                <div className="integration-form-grid">
                  <label>
                    <span>{t("integrations.connectionName")}</span>
                    <input required maxLength={200} value={draft.name} onChange={(event) => setDraft((current) => ({ ...current, name: event.target.value }))} />
                  </label>
                  <label>
                    <span>{t("integrations.connectionKey")}</span>
                    <input
                      aria-label={t("integrations.connectionKey")}
                      required
                      maxLength={64}
                      pattern="[a-z][a-z0-9_]{1,63}"
                      autoCapitalize="none"
                      spellCheck={false}
                      disabled={selectedId !== "new"}
                      value={draft.key}
                      onChange={(event) => setDraft((current) => ({ ...current, key: event.target.value }))}
                    />
                    <small>{t("integrations.connectionKeyHelp")}</small>
                  </label>

                  <label className="integration-wide-field">
                    <span>{t("integrations.baseUrl")}</span>
                    <input
                      aria-label={t("integrations.baseUrl")}
                      required
                      type="url"
                      maxLength={2000}
                      autoCapitalize="none"
                      spellCheck={false}
                      placeholder="https://api.example.com"
                      aria-invalid={draft.base_url !== "" && !httpsBaseUrlIsValid(draft.base_url.trim())}
                      value={draft.base_url}
                      onChange={(event) => setDraft((current) => ({ ...current, base_url: event.target.value }))}
                    />
                    <small>{t("integrations.baseUrlHelp")}</small>
                  </label>
                  <label className="integration-wide-field">
                    <span>{t("integrations.aiDescription")}</span>
                    <textarea aria-label={t("integrations.aiDescription")} required rows={4} maxLength={2000} value={draft.description} onChange={(event) => setDraft((current) => ({ ...current, description: event.target.value }))} />
                    <small>{t("integrations.aiDescriptionHelp")}</small>
                  </label>
                  <label>
                    <span>{t("integrations.status")}</span>
                    <select value={draft.status} onChange={(event) => setDraft((current) => ({ ...current, status: event.target.value as IntegrationDraft["status"] }))}>
                      <option value="active">{t("integrations.statusActive")}</option>
                      <option value="disabled">{t("integrations.statusDisabled")}</option>
                    </select>
                  </label>
                  <label>
                    <span>{t("integrations.authKind")}</span>
                    <select value={draft.auth_kind} onChange={(event) => changeAuthKind(event.target.value as ApiIntegrationAuthKind)}>
                      <option value="none">{t("integrations.authNone")}</option>
                      <option value="bearer">{t("integrations.authBearer")}</option>
                      <option value="x_api_key">{t("integrations.authApiKey")}</option>
                    </select>
                  </label>
                  {draft.auth_kind !== "none" && (
                    <label className="integration-wide-field">
                      <span>{t("integrations.token")}</span>
                      <input
                        aria-label={t("integrations.token")}
                        required={!storedTokenIsAvailable}
                        type="password"
                        minLength={8}
                        maxLength={8192}
                        autoComplete="new-password"
                        value={token}
                        placeholder={selectedIntegration?.token_configured ? t("integrations.tokenKeepPlaceholder") : t("integrations.tokenPlaceholder")}
                        onChange={(event) => setToken(event.target.value)}
                      />
                      <small>{t("integrations.tokenHelp")}</small>
                    </label>
                  )}
                </div>

                <section className="integration-actions" aria-labelledby="integration-actions-title">
                  <header>
                    <div>
                      <h3 id="integration-actions-title">{t("integrations.actions")}</h3>
                      <p>{t("integrations.actionsHelp")}</p>
                    </div>
                    <button
                      className="secondary-button"
                      type="button"
                      disabled={draft.actions.length >= maximumActions}
                      onClick={() => setDraft((current) => ({ ...current, actions: [...current.actions, emptyAction()] }))}
                    >
                      <Plus size={15} />{t("integrations.addAction")}
                    </button>
                  </header>
                  <div className="integration-action-list">
                    {draft.actions.map((action, index) => (
                      <article className="integration-action" key={action.rowId}>
                        <header>
                          <strong>GET</strong>
                          <span>{t("integrations.actionNumber", { number: index + 1 })}</span>
                          <DemoActionButton
                            className="danger-icon-button"
                            type="button"
                            aria-label={t("integrations.removeAction", { name: action.name || String(index + 1) })}
                            disabled={draft.actions.length === 1}
                            onClick={() => removeAction(action.rowId)}
                          >
                            <Trash2 size={15} />
                          </DemoActionButton>
                        </header>
                        <div className="integration-action-fields">
                          <label>
                            <span>{t("integrations.actionKey")}</span>
                            <input required maxLength={64} pattern="[a-z][a-z0-9_]{1,63}" autoCapitalize="none" spellCheck={false} value={action.key} onChange={(event) => updateAction(action.rowId, "key", event.target.value)} />
                          </label>
                          <label>
                            <span>{t("integrations.actionName")}</span>
                            <input required maxLength={200} value={action.name} onChange={(event) => updateAction(action.rowId, "name", event.target.value)} />
                          </label>
                          <label className="integration-wide-field">
                            <span>{t("integrations.actionDescription")}</span>
                            <textarea required rows={3} maxLength={2000} value={action.description} onChange={(event) => updateAction(action.rowId, "description", event.target.value)} />
                          </label>
                          <label className="integration-wide-field">
                            <span>{t("integrations.pathTemplate")}</span>
                            <input
                              aria-label={t("integrations.pathTemplate")}
                              required
                              maxLength={2000}
                              autoCapitalize="none"
                              spellCheck={false}
                              placeholder="/orders/{order_id}"
                              aria-invalid={action.path_template !== "" && !pathTemplateIsValid(action.path_template.trim())}
                              value={action.path_template}
                              onChange={(event) => updateAction(action.rowId, "path_template", event.target.value)}
                            />
                            <small>{t("integrations.pathTemplateHelp")}</small>
                          </label>
                        </div>
                      </article>
                    ))}
                  </div>
                </section>

                <fieldset className="integration-agents">
                  <legend>{t("integrations.agents")}</legend>
                  <p>{t("integrations.agentsHelp")}</p>
                  {profiles.map((profile) => (
                    <label key={profile.id}>
                      <input
                        type="checkbox"
                        checked={draft.ai_profile_ids.includes(profile.id)}
                        disabled={
                          !canManageAi
                          || (
                            !profile.runtime_available
                            && !draft.ai_profile_ids.includes(profile.id)
                          )
                        }
                        onChange={() => toggleProfile(profile.id)}
                      />
                      <span>
                        <strong>{profile.name}</strong>
                        <small>{t(profileStatusKey(profile.status))}</small>
                        <small>{t(profile.runtime_available ? "integrations.runtimeAvailable" : "integrations.runtimeUnavailable")}</small>
                      </span>
                    </label>
                  ))}
                  {profiles.length === 0 && <p>{t("integrations.noAgents")}</p>}
                </fieldset>

                <p className="integration-security-note">{t("integrations.securityNote")}</p>

                <div className="integration-form-actions">
                  <DemoActionButton className="primary-button" type="submit" disabled={!draftIsValid || saving}>
                    <Save size={16} />{saving ? t("integrations.saving") : t("integrations.save")}
                  </DemoActionButton>
                  {selectedIntegration && (
                    <DemoActionButton className="secondary-button table-danger-link" type="button" disabled={saving} onClick={() => void removeIntegration()}>
                      <Trash2 size={16} />{t("integrations.delete")}
                    </DemoActionButton>
                  )}
                </div>
              </div>
            </form>
          ) : (
            <div className="empty-state">{t("integrations.select")}</div>
          )}
        </div>
      </div>
    </section>
  );
}
