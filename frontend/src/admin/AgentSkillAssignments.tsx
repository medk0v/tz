import { useEffect, useState } from "react";
import type { OperatorAuth } from "../api";
import { listAiSkills, setAiProfileSkills, type AiProfileSkillAssignment, type AiSkill } from "../ai-skills-api";
import { useI18n } from "../i18n";
import { useDemoReadOnly } from "./DemoReadOnly";
import { aiSkillsText } from "./ai-skills-i18n";

interface Props {
  auth: OperatorAuth;
  profileId?: string;
  disabled?: boolean;
  onSaved: (assignment: AiProfileSkillAssignment) => void;
  onBusyChange?: (busy: boolean) => void;
}

export function AgentSkillAssignments({ auth, profileId, disabled = false, onSaved, onBusyChange }: Props) {
  const { locale } = useI18n();
  const text = aiSkillsText(locale);
  const readOnly = useDemoReadOnly() || auth.isDemo;
  const [skills, setSkills] = useState<AiSkill[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [loading, setLoading] = useState(Boolean(profileId));
  const [loadFailed, setLoadFailed] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  const [saved, setSaved] = useState(false);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (!profileId) return;
    const controller = new AbortController();
    void listAiSkills(auth, controller.signal).then((items) => {
      if (controller.signal.aborted) return;
      setSkills(items);
      setSelected(items.filter((skill) => skill.ai_profile_ids.includes(profileId)).map((skill) => skill.id));
      setLoadFailed(false);
    }).catch(() => {
      if (!controller.signal.aborted) setLoadFailed(true);
    }).finally(() => {
      if (!controller.signal.aborted) setLoading(false);
    });
    return () => controller.abort();
  }, [auth, profileId, attempt]);

  async function toggle(skillId: string, checked: boolean) {
    if (!profileId || disabled || readOnly || loading || saving || loadFailed) return;
    const next = checked ? [...selected, skillId] : selected.filter((id) => id !== skillId);
    setSaving(true);
    onBusyChange?.(true);
    setSaveFailed(false);
    setSaved(false);
    try {
      const result = await setAiProfileSkills(auth, profileId, next);
      setSelected(result.skill_ids);
      setSaved(true);
      onSaved({ profileId, skillIds: result.skill_ids });
    } catch {
      setSaveFailed(true);
    } finally {
      setSaving(false);
      onBusyChange?.(false);
    }
  }

  return <fieldset className="ai-assignment" aria-busy={loading || saving} disabled={disabled || readOnly || loading || saving}>
    <legend>{text("title")}</legend>
    {!profileId ? <p>{text("assignmentSaveAgentFirst")}</p> : <>
      <p>{text("assignmentHelp")}</p>
      {loading && <p role="status">{text("loading")}</p>}
      {loadFailed ? <p role="alert">{text("loadError")} <button className="table-link" type="button" onClick={() => {
        setLoading(true);
        setAttempt((value) => value + 1);
      }}>{text("retry")}</button></p> : !loading && skills.length === 0 ? <p>{text("assignmentEmpty")}</p> : skills.map((skill) => <label key={skill.id}>
        <input type="checkbox" checked={selected.includes(skill.id)} onChange={(event) => void toggle(skill.id, event.target.checked)} />
        <span><strong>{skill.name}</strong>{skill.description && <small>{skill.description}</small>}</span>
      </label>)}
      {saveFailed && <p role="alert">{text("assignmentSaveError")}</p>}
      {saving ? <p role="status">{text("saving")}</p> : saved && <p role="status">{text("assignmentSaved")}</p>}
    </>}
  </fieldset>;
}
