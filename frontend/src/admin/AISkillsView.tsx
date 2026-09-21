import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { BookOpen, ChevronRight, CircleHelp, Plus, RefreshCw, Save, Search, Sparkles, Trash2, Upload } from "lucide-react";
import type { AiProfile, AiProvider, OperatorAuth } from "../api";
import { createAiSkill, deleteAiSkill, listAiSkills, updateAiSkill, type AiProfileSkillAssignment, type AiSkill, type AiSkillInput } from "../ai-skills-api";
import { useI18n } from "../i18n";
import { DemoActionButton, useDemoReadOnly } from "./DemoReadOnly";
import { aiSkillsText } from "./ai-skills-i18n";
import { AISkillCreator } from "./AISkillCreator";
import { useCanonicalPageRoute, usePageRoute } from "./page-route";
import "./AISkillsView.css";

interface Props {
  auth: OperatorAuth;
  profiles: AiProfile[];
  profilesLoading?: boolean;
  providers?: AiProvider[];
  providersLoading?: boolean;
  onBusyChange?: (busy: boolean) => void;
  assignmentChange?: AiProfileSkillAssignment | null;
}

/** The library's part of the AI page URL: `skills[/<id|new>]`. */
const SKILLS = "skills";
const NEW_SKILL = "new";
const skillSegments = (skillId: string | null) => skillId ? [SKILLS, skillId] : [SKILLS];

const emptyDraft = (): AiSkillInput => ({ name: "", description: "", instructions: "", ai_profile_ids: [] });
const skillInput = ({ name, description, instructions, ai_profile_ids }: AiSkillInput): AiSkillInput => ({ name, description, instructions, ai_profile_ids: [...ai_profile_ids] });

function readFile(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error);
    reader.readAsText(file, "UTF-8");
  });
}

function importedDraft(text: string, filename: string): AiSkillInput {
  // Read only simple metadata for field suggestions. Preserve the entire source,
  // including unsupported YAML, references and code blocks, in the instructions.
  const frontmatter = text.match(/^\uFEFF?---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/)?.[1];
  function metadata(key: string): string {
    const value = frontmatter?.match(new RegExp(`^${key}:[ \\t]*(.+)$`, "m"))?.[1]?.trim() ?? "";
    if (/^[>|[\]{}]/.test(value)) return "";
    return value.replace(/^(['"])(.*)\1$/, "$2");
  }
  const heading = text.match(/^#\s+(.+)$/m)?.[1]?.trim();
  const fileName = filename.replace(/\.(md|txt)$/i, "");
  return {
    name: (metadata("name") || heading || (fileName.toLowerCase() === "skill" ? "" : fileName)).slice(0, 200),
    description: metadata("description").slice(0, 2000),
    instructions: text,
    ai_profile_ids: [],
  };
}

export function AISkillsView({ auth, profiles, profilesLoading = false, providers = [], providersLoading = false, onBusyChange, assignmentChange = null }: Props) {
  const { locale } = useI18n();
  const text = aiSkillsText(locale);
  const demoReadOnly = useDemoReadOnly() || auth.isDemo;
  const [skills, setSkills] = useState<AiSkill[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<AiSkillInput>(emptyDraft);
  const [appliedAssignmentChange, setAppliedAssignmentChange] = useState(assignmentChange);
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [creatingWithAi, setCreatingWithAi] = useState(false);
  const [creatorDirty, setCreatorDirty] = useState(false);
  const [notice, setNotice] = useState<{ kind: "success" | "error"; key: Parameters<typeof text>[0] } | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const requestSequence = useRef(0);
  const route = usePageRoute();
  const navigateRoute = route.navigate;
  const routeSegmentsRef = useRef(route.segments);
  // The skill the URL opens once the library has loaded; `undefined` while another AI tab or loading hides it.
  const linkedSkillId = route.segments[0] === SKILLS && !loading && !loadFailed ? route.segments[1] ?? null : undefined;
  const [routedSkillId, setRoutedSkillId] = useState<string | null>();
  const selected = skills.find((skill) => skill.id === selectedId);
  const dirty = selectedId !== null && JSON.stringify(draft) !== JSON.stringify(selected ? skillInput(selected) : emptyDraft());
  const query = search.trim().toLocaleLowerCase();
  const matchingSkills = skills.filter((skill) => `${skill.name} ${skill.description}`.toLocaleLowerCase().includes(query));
  const valid = draft.name.trim() !== "" && draft.instructions.trim() !== ""
    && [...draft.name].length <= 200 && [...draft.description].length <= 2000 && [...draft.instructions].length <= 50000
    && draft.ai_profile_ids.length <= 64;

  // Reconcile a completed agent-side assignment without discarding the library draft.
  if (assignmentChange !== appliedAssignmentChange) {
    setAppliedAssignmentChange(assignmentChange);
    if (assignmentChange) {
      const { profileId, skillIds } = assignmentChange;
      const assignedIds = (id: string, ids: string[]) => skillIds.includes(id)
        ? [...new Set([...ids, profileId])] : ids.filter((value) => value !== profileId);
      setSkills((items) => items.map((skill) => ({ ...skill, ai_profile_ids: assignedIds(skill.id, skill.ai_profile_ids) })));
      if (selectedId && selectedId !== "new") setDraft((current) => ({ ...current, ai_profile_ids: assignedIds(selectedId, current.ai_profile_ids) }));
    }
  }

  // Links and Back/Forward open skills through the URL; clicks open them and then update the URL.
  if (linkedSkillId !== undefined && linkedSkillId !== routedSkillId) {
    setRoutedSkillId(linkedSkillId);
    if (linkedSkillId !== selectedId) {
      const skill = skills.find((item) => item.id === linkedSkillId);
      setCreatingWithAi(false);
      setCreatorDirty(false);
      setSelectedId(skill || linkedSkillId === NEW_SKILL ? linkedSkillId : null);
      setDraft(skill ? skillInput(skill) : emptyDraft());
      setNotice(null);
    }
  }
  const canonicalSkillId = linkedSkillId && (linkedSkillId === NEW_SKILL || skills.some((skill) => skill.id === linkedSkillId)) ? linkedSkillId : null;
  useCanonicalPageRoute(route, skillSegments(canonicalSkillId), linkedSkillId !== undefined);

  useEffect(() => { routeSegmentsRef.current = route.segments; }, [route.segments]);

  function openSkill(skillId: string | null, replace = false) {
    void navigateRoute(skillSegments(skillId), { replace });
  }

  // Requests can finish after Back/Forward opened another skill; they update only the editor the URL still shows.
  function stillEditing(skillId: string | null) {
    const [section, routedId = null] = routeSegmentsRef.current;
    return section !== SKILLS || routedId === skillId;
  }

  function replaceSkillRoute(fromId: string | null, toId: string | null) {
    const [section, routedId = null] = routeSegmentsRef.current;
    if (section === SKILLS && routedId === fromId) void navigateRoute(skillSegments(toId), { replace: true });
  }

  const reload = useCallback((signal?: AbortSignal) => {
    const sequence = ++requestSequence.current;
    return listAiSkills(auth, signal).then((items) => {
      if (!signal?.aborted && sequence === requestSequence.current) {
        setSkills(items);
        setLoadFailed(false);
      }
    }).catch(() => {
      if (!signal?.aborted && sequence === requestSequence.current) setLoadFailed(true);
    }).finally(() => {
      if (!signal?.aborted && sequence === requestSequence.current) setLoading(false);
    });
  }, [auth]);

  useEffect(() => {
    const controller = new AbortController();
    void reload(controller.signal);
    return () => controller.abort();
  }, [reload]);

  function refresh() {
    setLoading(true);
    setLoadFailed(false);
    void reload();
  }

  const setWorking = useCallback((value: boolean) => {
    setBusy(value);
    onBusyChange?.(value);
  }, [onBusyChange]);

  function canDiscard() {
    return !(creatorDirty || dirty) || window.confirm(text(creatorDirty ? "discardSources" : "discard"));
  }

  function selectSkill(skill?: AiSkill) {
    if (busy || !canDiscard()) return;
    setCreatingWithAi(false);
    setCreatorDirty(false);
    setSelectedId(skill?.id ?? "new");
    setDraft(skill ? skillInput(skill) : emptyDraft());
    setNotice(null);
    openSkill(skill?.id ?? NEW_SKILL);
    requestAnimationFrame(() => nameInput.current?.focus());
  }

  async function importSkill(file?: File) {
    if (!file || busy || demoReadOnly) return;
    if (!/\.(md|txt)$/i.test(file.name) || file.size === 0 || file.size > 200000) {
      setNotice({ kind: "error", key: "invalidFile" });
      return;
    }
    setWorking(true);
    try {
      const source = await readFile(file);
      if (!source.trim() || source.includes("\0") || [...source].length > 50000) {
        setNotice({ kind: "error", key: "invalidFile" });
        return;
      }
      if (!canDiscard()) return;
      setCreatingWithAi(false);
      setCreatorDirty(false);
      setSelectedId("new");
      setDraft(importedDraft(source, file.name));
      setNotice({ kind: "success", key: "imported" });
      openSkill(NEW_SKILL);
      requestAnimationFrame(() => nameInput.current?.focus());
    } catch {
      setNotice({ kind: "error", key: "importError" });
    } finally {
      setWorking(false);
    }
  }

  async function save(event: FormEvent) {
    event.preventDefault();
    if (busy || loading || demoReadOnly || !selectedId) return;
    if (!valid) {
      setNotice({ kind: "error", key: "invalidFields" });
      return;
    }
    setWorking(true);
    setNotice(null);
    try {
      const input = { ...draft, name: draft.name.trim(), description: draft.description.trim() };
      const saved = selectedId === "new" ? await createAiSkill(auth, input) : await updateAiSkill(auth, selectedId, input);
      setSkills((items) => [saved, ...items.filter((item) => item.id !== saved.id)]);
      if (stillEditing(selectedId)) {
        setSelectedId(saved.id);
        setDraft(skillInput(saved));
      }
      replaceSkillRoute(selectedId, saved.id);
      setSearch("");
      setNotice({ kind: "success", key: "saved" });
    } catch {
      setNotice({ kind: "error", key: "saveError" });
    } finally {
      setWorking(false);
    }
  }

  async function remove() {
    if (!selected || busy || demoReadOnly || !window.confirm(text("deleteConfirm", { name: selected.name }))) return;
    setWorking(true);
    setNotice(null);
    try {
      await deleteAiSkill(auth, selected.id);
      setSkills((items) => items.filter((item) => item.id !== selected.id));
      if (stillEditing(selected.id)) {
        setSelectedId(null);
        setDraft(emptyDraft());
      }
      replaceSkillRoute(selected.id, null);
      setNotice({ kind: "success", key: "deleted" });
    } catch {
      setNotice({ kind: "error", key: "deleteError" });
    } finally {
      setWorking(false);
    }
  }

  function toggleAgent(id: string, checked: boolean) {
    setDraft((current) => ({ ...current, ai_profile_ids: checked ? [...current.ai_profile_ids, id] : current.ai_profile_ids.filter((value) => value !== id) }));
  }

  return <div className="ai-skills-workspace">
    <div className="ai-skills-toolbar">
      <DemoActionButton className="primary-button" type="button" disabled={busy || demoReadOnly} onClick={() => {
        if (creatingWithAi || !canDiscard()) return;
        setCreatingWithAi(true);
        setSelectedId(null);
        setDraft(emptyDraft());
        setNotice(null);
        openSkill(null);
      }}><Sparkles size={16} />{text("createWithAi")}</DemoActionButton>
      <DemoActionButton className="secondary-button" type="button" disabled={busy || demoReadOnly} onClick={() => selectSkill()}><Plus size={16} />{text("create")}</DemoActionButton>
      <DemoActionButton className="secondary-button" type="button" disabled={busy || demoReadOnly} onClick={() => fileInput.current?.click()}><Upload size={16} />{text("import")}</DemoActionButton>
      <input hidden ref={fileInput} type="file" accept=".md,.txt,text/markdown,text/plain" aria-label={text("importFile")} disabled={busy || demoReadOnly} onChange={(event) => {
        const file = event.target.files?.[0];
        event.target.value = "";
        void importSkill(file);
      }} />
      <SkillsHint label={text("hintLabel")} title={text("hintTitle")} body={text("hintBody")} example={text("hintExample")} />
      <button className="secondary-button ai-skills-refresh" type="button" aria-label={text("refresh")} title={text("refresh")} disabled={busy || loading} onClick={refresh}><RefreshCw size={16} /></button>
    </div>
    <p className="ai-skills-import-help">{text("importHelp")}</p>
    {notice && <div className={`admin-notice admin-notice--${notice.kind}`} role={notice.kind === "error" ? "alert" : "status"}>{text(notice.key)}</div>}
    {loadFailed && <div className="admin-notice admin-notice--error ai-skills-load-error" role="alert"><span>{text("loadError")}</span><button className="secondary-button" type="button" disabled={busy} onClick={refresh}>{text("retry")}</button></div>}
    <div className="settings-layout ai-settings-layout ai-skills-layout">
      <aside className="settings-list agent-profile-list" aria-label={text("list")}>
        <label className="agent-profile-search"><Search size={17} aria-hidden="true" /><input type="search" aria-label={text("search")} placeholder={text("search")} value={search} onChange={(event) => setSearch(event.target.value)} /></label>
        <section className="agent-profile-group" aria-label={text("list")}>
          <h2>{text("list")}</h2>
          {matchingSkills.map((skill) => <button className={`agent-profile-row ai-skill-row${selectedId === skill.id ? " agent-profile-row--active" : ""}`} type="button" key={skill.id} aria-pressed={selectedId === skill.id} disabled={busy} onClick={() => selectSkill(skill)}>
            <BookOpen size={20} aria-hidden="true" />
            <span className="agent-profile-row-copy"><strong>{skill.name}</strong>{skill.description && <span className="agent-profile-row-description">{skill.description}</span>}<span className="agent-profile-row-status">{skill.ai_profile_ids.length ? text("assigned", { count: skill.ai_profile_ids.length }) : text("unassigned")}</span></span>
            <ChevronRight size={16} aria-hidden="true" />
          </button>)}
          {loading && <div className="empty-state" role="status">{text("loading")}</div>}
          {!loading && !loadFailed && matchingSkills.length === 0 && <p className="agent-profile-list-empty" role="status">{text(query ? "noMatches" : "empty")}</p>}
        </section>
      </aside>
      <div className="settings-panel ai-editor-panel">
        {creatingWithAi ? <AISkillCreator auth={auth} providers={providers} providersLoading={providersLoading} onBusyChange={setWorking} onDirtyChange={setCreatorDirty} onCancel={() => {
          if (!canDiscard()) return;
          setCreatingWithAi(false);
          setCreatorDirty(false);
        }} onGenerated={(result) => {
          setCreatingWithAi(false);
          setCreatorDirty(false);
          setSelectedId("new");
          setDraft({ ...result, ai_profile_ids: [] });
          setNotice({ kind: "success", key: "generated" });
          openSkill(NEW_SKILL);
          requestAnimationFrame(() => nameInput.current?.focus());
        }} /> : selectedId ? <>
          <header className="agent-profile-heading"><div><h2>{selectedId === "new" ? text("newTitle") : selected?.name ?? draft.name}</h2>{dirty && <p>{text("unsaved")}</p>}</div></header>
          <form onSubmit={(event) => void save(event)}>
            <fieldset className="settings-form ai-skills-form" disabled={busy || demoReadOnly}>
              <label><span>{text("name")}</span><input ref={nameInput} required maxLength={200} value={draft.name} onChange={(event) => setDraft((current) => ({ ...current, name: event.target.value }))} /></label>
              <label><span>{text("purpose")}</span><textarea rows={2} maxLength={2000} value={draft.description} onChange={(event) => setDraft((current) => ({ ...current, description: event.target.value }))} /></label>
              <label><span id="ai-skill-instructions-label">{text("instructions")}</span><textarea className="ai-skill-instructions" required rows={13} maxLength={50000} aria-labelledby="ai-skill-instructions-label" aria-describedby="ai-skill-instructions-help" value={draft.instructions} onChange={(event) => setDraft((current) => ({ ...current, instructions: event.target.value }))} /><small id="ai-skill-instructions-help">{text("instructionsHelp")}</small></label>
              <fieldset className="ai-assignment" aria-describedby="ai-skill-agents-help" disabled={profilesLoading}>
                <legend>{text("agents")}</legend>
                <p id="ai-skill-agents-help">{text("agentsHelp")}</p>
                {profilesLoading ? <p role="status">{text("agentsLoading")}</p> : profiles.length === 0 ? <p>{text("noAgents")}</p> : profiles.map((profile) => <label key={profile.id}>
                  <input type="checkbox" checked={draft.ai_profile_ids.includes(profile.id)} disabled={!draft.ai_profile_ids.includes(profile.id) && draft.ai_profile_ids.length >= 64} onChange={(event) => toggleAgent(profile.id, event.target.checked)} />
                  <span><strong>{profile.name}</strong>{profile.description && <small>{profile.description}</small>}</span>
                </label>)}
                {!profilesLoading && draft.ai_profile_ids.filter((id) => !profiles.some((profile) => profile.id === id)).map((id) => <label key={id}><input type="checkbox" checked aria-label={text("removeUnavailable")} onChange={() => toggleAgent(id, false)} /><span>{text("unavailable")}</span></label>)}
              </fieldset>
              <div className="ai-actions">
                <DemoActionButton className="primary-button" type="submit" disabled={loading || profilesLoading || !valid}><Save size={16} />{busy ? text("saving") : text("save")}</DemoActionButton>
                {selected && <DemoActionButton className="secondary-button table-danger-link" type="button" onClick={() => void remove()}><Trash2 size={16} />{text("delete")}</DemoActionButton>}
                <button className="secondary-button" type="button" onClick={() => { if (!dirty || window.confirm(text("discard"))) { setSelectedId(null); setDraft(emptyDraft()); setNotice(null); openSkill(null); } }}>{text("cancel")}</button>
              </div>
            </fieldset>
          </form>
        </> : <div className="empty-state">{text("select")}</div>}
      </div>
    </div>
  </div>;
}

function SkillsHint({ label, title, body, example }: { label: string; title: string; body: string; example: string }) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent | KeyboardEvent) => {
      if (event instanceof KeyboardEvent ? event.key === "Escape" : !root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", close);
    document.addEventListener("keydown", close);
    return () => { document.removeEventListener("pointerdown", close); document.removeEventListener("keydown", close); };
  }, [open]);
  return <span ref={root} className="ai-skills-hint" data-open={open || undefined}>
    <button type="button" className="ai-skills-hint-button" aria-label={label} aria-expanded={open} aria-describedby="ai-skills-hint" onClick={() => setOpen((value) => !value)}><CircleHelp size={18} /></button>
    <span id="ai-skills-hint" role="tooltip" className="ai-skills-hint-popover"><strong>{title}</strong><span>{body}</span><span>{example}</span></span>
  </span>;
}
