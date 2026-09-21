import { AnimatedDetails } from "../AnimatedDetails";
import { useEffect, useId, useState, type FormEvent } from "react";
import { ArrowLeft, Bot, Briefcase, Globe, Headset, Layers, Linkedin, Mail, MessageCircle, Phone, Save, Send, Trash2, Users, Workflow } from "lucide-react";
import { ApiRequestError, listAiProfiles, type AiProfile, type Inbox, type OperatorAuth } from "../api";
import { createCustomAiChannel, deleteCustomAiChannel, updateCustomAiChannel, type CustomAiChannel, type CustomAiChannelInput } from "../custom-ai-channel-api";
import { } from "../department-api";
import { useI18n } from "../i18n";
import { customAiChannelText } from "./custom-ai-channel-i18n";
import { DemoActionButton } from "./DemoReadOnly";
import "./CustomAiChannelsView.css";

const icons = { bot: Bot, globe: Globe, "message-circle": MessageCircle, mail: Mail, briefcase: Briefcase, users: Users, headset: Headset, send: Send, linkedin: Linkedin, phone: Phone, workflow: Workflow, layers: Layers };

export function CustomAiChannelIcon({ icon, size = 18 }: { icon: string; size?: number }) {
  const Icon = icons[icon as keyof typeof icons] ?? Bot;
  return <Icon size={size} aria-hidden="true" />;
}


export function CustomAiChannelsView({ auth, inboxes, channel, canManage, onBack, onSaved, onDeleted }: {
  auth: OperatorAuth;
  inboxes: Inbox[];
  channel: CustomAiChannel | null;
  canManage: boolean;
  onBack: () => void;
  onSaved: (channel: CustomAiChannel) => void;
  onDeleted: () => void;
}) {
  const { locale } = useI18n();
  const fieldId = useId();
  const text = customAiChannelText(locale);
  const [form, setForm] = useState<CustomAiChannelInput>(() => ({
    visibility: channel?.visibility,
    name: channel?.name ?? "", icon: channel?.icon ?? "bot", inbox_id: channel?.inbox_id ?? inboxes[0]?.id ?? "",
    connection_type: channel?.connection_type ?? "ai_agent", destination: channel?.destination ?? "conversations",
    mode: channel?.mode ?? "agent", source_url: channel?.source_url ?? "", source_kind: channel?.source_kind ?? "website",
    instructions: channel?.instructions ?? "", ai_profile_id: channel?.ai_profile_id ?? null,
  }));
  const projectId = inboxes[0]?.project_id;
  const [profiles, setProfiles] = useState<AiProfile[] | null>(null);
  const [profilesError, setProfilesError] = useState(false);
  const [profilesRevision, setProfilesRevision] = useState(0);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const busy = saving || deleting;
  const [error, setError] = useState<string | null>(null);
  const isExternalApi = form.connection_type === "external_api";
  const instructionsRequired = form.destination === "custom" || (!isExternalApi && form.mode === "instructions");

  useEffect(() => {
    let active = true;
    listAiProfiles(auth)
      .then((items) => { if (active) { setProfiles(items); setProfilesError(false); } })
      .catch(() => { if (active) setProfilesError(true); });
    return () => { active = false; };
  }, [auth, profilesRevision]);

  useEffect(() => {
    return;
    
  }, [auth, projectId]);

  async function remove() {
    if (!channel || !canManage || busy || !window.confirm(`${text("deleteConfirm")}\n${channel.name}`)) return;
    setDeleting(true);
    setError(null);
    try { await deleteCustomAiChannel(auth, channel.id); onDeleted(); }
    catch (cause) { setError(cause instanceof ApiRequestError ? cause.message : text("deleteError")); }
    finally { setDeleting(false); }
  }

  function change<K extends keyof CustomAiChannelInput>(key: K, value: CustomAiChannelInput[K]) {
    setForm((current) => ({ ...current, [key]: value }));
  }

  function changeConnectionType(connectionType: CustomAiChannelInput["connection_type"]) {
    setForm((current) => ({ ...current, connection_type: connectionType, mode: connectionType === "external_api" ? "instructions" : "agent", ai_profile_id: null, source_kind: connectionType === "external_api" ? "api" : "website" }));
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canManage || busy) return;
    let validation: string | null = null;
    if (!form.name.trim()) validation = text("validName");
    else if (form.inbox_id !== channel?.inbox_id && !inboxes.some((item) => item.id === form.inbox_id)) validation = text("validInbox");
    else if (form.source_url.trim()) {
      try {
        const source = new URL(form.source_url.trim());
        if (source.protocol !== "https:" || !source.hostname || source.username || source.password) validation = text("validUrl");
      } catch { validation = text("validUrl"); }
    }
    if (!validation && instructionsRequired && form.instructions.trim().length < 10) validation = text("validInstructions");
    if (!validation && !isExternalApi && form.mode === "agent" && !profiles?.some((item) => item.id === form.ai_profile_id)) validation = text("validAgent");
    if (validation) { setError(validation); return; }

    const input: CustomAiChannelInput = {
      ...form, name: form.name.trim(), source_url: form.source_url.trim(), instructions: form.instructions.trim(),
      mode: isExternalApi ? "instructions" : form.mode, source_kind: isExternalApi ? "api" : form.source_kind,
      ai_profile_id: !isExternalApi && form.mode === "agent" ? form.ai_profile_id : null,
    };
    setSaving(true);
    setError(null);
    try {
      const saved = channel ? await updateCustomAiChannel(auth, channel.id, input) : await createCustomAiChannel(auth, input);
      onSaved(saved);
    } catch (cause) {
      setError(cause instanceof ApiRequestError ? cause.message : text("saveError"));
    } finally { setSaving(false); }
  }

  const chosenProfileMissing = form.ai_profile_id && profiles && !profiles.some((item) => item.id === form.ai_profile_id);
  return <div className="page channels-page channels-page--settings custom-ai-channels-page">
    <button className="back-button" type="button" disabled={busy} onClick={onBack}><ArrowLeft size={16} />{text("backToChannels")}</button>
    <header className="page-toolbar"><div><h1>{text(channel ? "editTitle" : "newTitle")}</h1><p>{text("runtimeHelp")}</p></div><span className="status-text">{text("draft")}</span></header>
    <section className="settings-panel custom-ai-channel-editor">
      <form onSubmit={(event) => void submit(event)} noValidate>
        <fieldset disabled={!canManage || busy} className="custom-ai-channel-fields">
          <label><span>{text("name")}</span><input value={form.name} maxLength={200} required onChange={(event) => change("name", event.currentTarget.value)} /></label>
          <fieldset className="custom-ai-icon-field"><legend>{text("icon")}</legend><div className="custom-ai-icon-options">{Object.entries(icons).map(([key, Icon]) => <button key={key} type="button" aria-label={`${text("icon")}: ${text(key as keyof typeof icons)}`} title={text(key as keyof typeof icons)} aria-pressed={form.icon === key} onClick={() => change("icon", key)}><Icon size={20} aria-hidden="true" /></button>)}</div></fieldset>
          <fieldset className="custom-ai-channel-mode custom-ai-connection-type">
            <legend>{text("connectionType")}</legend>
            <label><input type="radio" name="custom-connection-type" value="ai_agent" checked={!isExternalApi} onChange={() => changeConnectionType("ai_agent")} />{text("aiConnection")}</label>
            <label><input type="radio" name="custom-connection-type" value="external_api" checked={isExternalApi} onChange={() => changeConnectionType("external_api")} />{text("apiConnection")}</label>
          </fieldset>
          {isExternalApi && <p className="field-help">{text("apiHelp")}</p>}
          {inboxes.length === 0 && <p className="field-help">{text("noInboxes")}</p>}
          <label><span id={`${fieldId}-source-label`}>{text(isExternalApi ? "apiSource" : "source")}</span><input aria-labelledby={`${fieldId}-source-label`} aria-describedby={`${fieldId}-source-help`} type="url" maxLength={4096} autoCapitalize="none" autoComplete="off" placeholder={isExternalApi ? "https://example.com/api/docs" : "https://www.linkedin.com/messaging/"} value={form.source_url} onChange={(event) => change("source_url", event.currentTarget.value)} /><span className="field-help" id={`${fieldId}-source-help`}>{text("sourceHelp")}</span></label>
          {!isExternalApi && form.mode === "agent" && <div className="custom-ai-agent-field">
            <label><span>{text("agent")}</span><select required value={form.ai_profile_id ?? ""} disabled={profiles === null || profilesError} onChange={(event) => change("ai_profile_id", event.currentTarget.value || null)}>
              <option value="">{text(profiles === null && !profilesError ? "loading" : "chooseAgent")}</option>
              {chosenProfileMissing && <option value={form.ai_profile_id!} disabled>{text("missingAgent")}</option>}
              {profiles?.map((profile) => <option key={profile.id} value={profile.id}>{profile.name}{profile.status === "disabled" ? ` · ${text("agentDisabled")}` : ""}</option>)}
            </select></label>
            {profilesError && <div className="custom-ai-load-error" role="alert"><span>{text("agentsError")}</span><button type="button" className="secondary-button" onClick={() => { setProfilesError(false); setProfilesRevision((value) => value + 1); }}>{text("retry")}</button></div>}
            {profiles?.length === 0 && <p className="field-help">{text("noAgents")}</p>}
          </div>}
          <label><span id={`${fieldId}-destination-label`}>{text("destination")}</span><select aria-labelledby={`${fieldId}-destination-label`} aria-describedby={`${fieldId}-destination-help`} value={form.destination} onChange={(event) => change("destination", event.currentTarget.value as CustomAiChannelInput["destination"])}><option value="conversations">{text("destinationConversations")}</option><option value="tasks">{text("destinationTasks")}</option><option value="custom">{text("destinationCustom")}</option></select><span id={`${fieldId}-destination-help`} className="field-help">{text(form.destination === "conversations" ? "conversationsHelp" : form.destination === "tasks" ? "tasksHelp" : "customHelp")}</span></label>
          <label><span id={`${fieldId}-task-label`}>{text(instructionsRequired ? "instructions" : isExternalApi ? "apiInstructions" : "extraInstructions")}</span><textarea aria-labelledby={`${fieldId}-task-label`} aria-describedby={`${fieldId}-task-help`} rows={7} required={instructionsRequired} maxLength={50000} value={form.instructions} placeholder={text(isExternalApi ? "apiInstructionsExample" : "instructionsExample")} onChange={(event) => change("instructions", event.currentTarget.value)} /><span className="field-help" id={`${fieldId}-task-help`}>{text("instructionsHelp")}</span></label>
          {!isExternalApi && <AnimatedDetails className="custom-ai-mode-options" open={form.mode === "instructions"}>
            <summary>{text("alternativeMode")}</summary>
          <fieldset className="custom-ai-channel-mode">
            <legend>{text("mode")}</legend>
            <label><input type="radio" name="custom-ai-mode" value="agent" checked={form.mode === "agent"} onChange={() => change("mode", "agent")} />{text("agentMode")}</label>
            <label><input type="radio" name="custom-ai-mode" value="instructions" checked={form.mode === "instructions"} onChange={() => change("mode", "instructions")} />{text("instructionsMode")}</label>
          </fieldset>
          </AnimatedDetails>}
          <label><span>{text("inbox")}</span><select value={form.inbox_id} required onChange={(event) => change("inbox_id", event.currentTarget.value)}>
            {!inboxes.some((item) => item.id === form.inbox_id) && <option value={form.inbox_id}>{channel?.inbox_name ?? text("validInbox")}</option>}
            {inboxes.map((inbox) => <option key={inbox.id} value={inbox.id}>{inbox.name}</option>)}
          </select></label>
        </fieldset>
        {error && <div className="admin-notice admin-notice--error" role="alert">{error}</div>}
        <footer className="custom-ai-channel-actions">
          {canManage && <DemoActionButton type="submit" className="primary-button" disabled={busy || inboxes.length === 0}><Save size={16} />{text(saving ? "saving" : "save")}</DemoActionButton>}
          <button type="button" className="secondary-button" disabled={busy} onClick={onBack}>{text("cancel")}</button>
          {channel && canManage && <DemoActionButton type="button" className="table-link table-danger-link custom-ai-delete" disabled={busy} onClick={() => void remove()}><Trash2 size={15} />{text(deleting ? "deleting" : "delete")}</DemoActionButton>}
        </footer>
      </form>
    </section>
  </div>;
}
