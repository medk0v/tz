import { useEffect, useRef, useState, type FormEvent } from "react";
import { FileText, LoaderCircle, Sparkles, Upload, X } from "lucide-react";
import { ApiRequestError, type AiProvider, type OperatorAuth } from "../api";
import { isChatModelConnection } from "../ai-model-types";
import { generateAiSkill, type AiSkillGenerationResult } from "../ai-skills-api";
import { useI18n } from "../i18n";
import { DemoActionButton, useDemoReadOnly } from "./DemoReadOnly";
import { aiSkillsText } from "./ai-skills-i18n";

interface Props {
  auth: OperatorAuth;
  providers: AiProvider[];
  providersLoading: boolean;
  onGenerated: (result: AiSkillGenerationResult) => void;
  onCancel: () => void;
  onBusyChange: (busy: boolean) => void;
  onDirtyChange: (dirty: boolean) => void;
}

const megabyte = 1024 * 1024;
const sourceKey = (file: File) => `${file.name}:${file.size}:${file.lastModified}`;

export function AISkillCreator({ auth, providers, providersLoading, onGenerated, onCancel, onBusyChange, onDirtyChange }: Props) {
  const { locale } = useI18n();
  const text = aiSkillsText(locale);
  const demoReadOnly = useDemoReadOnly() || auth.isDemo;
  const connections = providers.filter((provider) => provider.status === "active" && provider.default_model.trim()
    && isChatModelConnection(provider) && (provider.provider_kind === "openai" || provider.provider_kind === "openai_compatible"));
  const [connectionId, setConnectionId] = useState("");
  const selectedConnection = connectionId || connections[0]?.id || "";
  const [goal, setGoal] = useState("");
  const [sourceText, setSourceText] = useState("");
  const [files, setFiles] = useState<File[]>([]);
  const [generating, setGenerating] = useState(false);
  const [cancelled, setCancelled] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const request = useRef<AbortController | null>(null);
  const goalInput = useRef<HTMLTextAreaElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const dirty = goal !== "" || sourceText !== "" || files.length > 0;
  const valid = goal.trim() !== "" && [...goal].length <= 10000 && [...sourceText].length <= 1000000
    && connections.some((provider) => provider.id === selectedConnection);

  useEffect(() => { goalInput.current?.focus(); }, []);
  useEffect(() => { onDirtyChange(dirty); }, [dirty, onDirtyChange]);
  useEffect(() => () => {
    request.current?.abort();
    onBusyChange(false);
  }, [onBusyChange]);

  function addFiles(incoming: FileList | null) {
    if (!incoming || demoReadOnly || generating) return;
    const nextFiles = [...files];
    for (const file of incoming) {
      if (!file.size || !/\.(txt|md|pdf|docx|epub|jpe?g|png|webp)$/i.test(file.name)) {
        setError(text("sourceFileInvalid", { name: file.name }));
        return;
      }
      const image = /\.(jpe?g|png|webp)$/i.test(file.name);
      if (file.size > (image ? 10 : 20) * megabyte) {
        setError(text("sourceFileTooLarge", { name: file.name }));
        return;
      }
      if (!nextFiles.some((existing) => sourceKey(existing) === sourceKey(file))) nextFiles.push(file);
    }
    if (nextFiles.length > 10) { setError(text("sourceFilesTooMany")); return; }
    if (nextFiles.reduce((total, file) => total + file.size, 0) > 30 * megabyte) { setError(text("sourceFilesTooLarge")); return; }
    setFiles(nextFiles);
    setError(null);
  }

  function cancelGeneration() {
    request.current?.abort();
    request.current = null;
    setGenerating(false);
    onBusyChange(false);
    setCancelled(true);
    setError(null);
  }

  async function generate(event: FormEvent) {
    event.preventDefault();
    if (demoReadOnly || generating || providersLoading) return;
    if (!goal.trim() || [...goal].length > 10000) { setError(text("invalidGoal")); return; }
    if ([...sourceText].length > 1000000) { setError(text("sourceTextTooLong")); return; }
    if (!valid) return;
    const controller = new AbortController();
    request.current = controller;
    setGenerating(true);
    onBusyChange(true);
    setError(null);
    setCancelled(false);
    try {
      const result = await generateAiSkill(auth, { provider_connection_id: selectedConnection, goal: goal.trim(), source_text: sourceText, files }, controller.signal);
      if (controller.signal.aborted) return;
      request.current = null;
      setGenerating(false);
      onBusyChange(false);
      onGenerated(result);
    } catch (cause) {
      if (!controller.signal.aborted) {
        const detail = cause instanceof ApiRequestError && [400, 413, 415, 422].includes(cause.status) ? ` ${cause.message}` : "";
        setError(`${text("generationError")}${detail}`);
      }
    } finally {
      if (request.current === controller) {
        request.current = null;
        setGenerating(false);
        onBusyChange(false);
      }
    }
  }

  return <section className="ai-skill-creator-panel" aria-labelledby="ai-skill-creator-title" aria-busy={generating}>
    <header className="agent-profile-heading"><div><h2 id="ai-skill-creator-title">{text("creatorTitle")}</h2><p>{text("creatorHelp")}</p></div></header>
    <form className="ai-skill-creator" onSubmit={(event) => void generate(event)}>
      <fieldset className="settings-form ai-skills-form" disabled={generating || demoReadOnly}>
        <label><span>{text("goal")}</span><textarea ref={goalInput} required rows={3} maxLength={10000} placeholder={text("goalPlaceholder")} value={goal} onChange={(event) => setGoal(event.target.value)} /></label>
        <label><span id="ai-skill-source-text-label">{text("sourceText")}</span><textarea rows={5} maxLength={1000000} aria-labelledby="ai-skill-source-text-label" aria-describedby="ai-skill-source-text-help" value={sourceText} onChange={(event) => setSourceText(event.target.value)} /><small id="ai-skill-source-text-help">{text("sourceTextHelp")}</small></label>
        <div className="ai-skill-sources">
          <span id="ai-skill-sources-label">{text("sourceFiles")}</span>
          <input hidden ref={fileInput} type="file" multiple aria-labelledby="ai-skill-sources-label" aria-describedby="ai-skill-source-formats ai-skill-pdf-help" accept=".txt,.md,.pdf,.docx,.epub,.jpg,.jpeg,.png,.webp" onChange={(event) => { addFiles(event.target.files); event.target.value = ""; }} />
          <DemoActionButton className="secondary-button" type="button" onClick={() => fileInput.current?.click()}><Upload size={16} />{text("addSources")}</DemoActionButton>
          <small id="ai-skill-source-formats">{text("sourceFormats")}</small>
          <small id="ai-skill-pdf-help">{text("pdfHelp")}</small>
          {files.length > 0 && <ul className="ai-skill-source-list" aria-labelledby="ai-skill-sources-label">{files.map((file) => <li key={sourceKey(file)}>
            <FileText size={16} aria-hidden="true" /><span>{file.name}</span><small>{new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(file.size / megabyte)} MB</small>
            <DemoActionButton className="icon-button" type="button" aria-label={text("removeSource", { name: file.name })} onClick={() => { setFiles((current) => current.filter((item) => item !== file)); setError(null); }}><X size={16} /></DemoActionButton>
          </li>)}</ul>}
        </div>
        <label><span id="ai-skill-connection-label">{text("connection")}</span><select value={selectedConnection} disabled={providersLoading || connections.length === 0} aria-labelledby="ai-skill-connection-label" aria-describedby="ai-skill-connection-help" onChange={(event) => setConnectionId(event.target.value)}>
          {!selectedConnection && <option value="">{text("chooseConnection")}</option>}
          {connections.map((provider) => <option key={provider.id} value={provider.id}>{provider.name} · {provider.default_model}</option>)}
        </select><small id="ai-skill-connection-help">{text(providersLoading ? "connectionsLoading" : connections.length ? "connectionHelp" : "noConnection")}</small></label>
        <p className="ai-skills-import-help">{text("sourcePrivacy")}</p>
      </fieldset>
      <div className="ai-skill-creator-footer">
        {error && <div className="admin-notice admin-notice--error" role="alert">{error}</div>}
        {cancelled && <p className="ai-skills-import-help" role="status">{text("generationCancelled")}</p>}
        {generating && <p className="ai-skill-generation-status" role="status"><LoaderCircle size={16} aria-hidden="true" />{text("generating")}</p>}
        <div className="ai-actions">
          <DemoActionButton className="primary-button" type="submit" disabled={generating || demoReadOnly || providersLoading || !valid}><Sparkles size={16} />{text("generate")}</DemoActionButton>
          <button className="secondary-button" type="button" onClick={generating ? cancelGeneration : onCancel}>{text(generating ? "stopGeneration" : "cancel")}</button>
        </div>
      </div>
    </form>
  </section>;
}
