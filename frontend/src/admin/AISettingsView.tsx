import { AnimatedDetails } from "../AnimatedDetails";
import { useContentMotion } from "./useContentMotion";
import { DemoActionButton } from "./DemoReadOnly";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { AgentBackups } from "./AgentBackups";
import { AgentTests } from "./AgentTests";
import { AgentApi } from "./AgentApi";
import { AgentProxy } from "./AgentProxy";
import { AgentProfileList, AgentProfileMark } from "./AgentProfileList";
import { AITasksWorkspace, type TaskAccessFlags } from "./AITasksWorkspace";
import { DateTimeField } from "./DateTimeField";
import { AISkillsView } from "./AISkillsView";
import { AgentSkillAssignments } from "./AgentSkillAssignments";
import type { AiProfileSkillAssignment } from "../ai-skills-api";
import { aiSkillsText } from "./ai-skills-i18n";
import { agentProfileTabs, agentProfileTabLabel, type AgentProfileTab } from "./agent-profile-layout-i18n";
import { PageRouteContext, useCanonicalPageRoute, usePageRoute } from "./page-route";
import { taskWorkspaceText } from "./task-workspace-i18n";
import { BookOpen, Bot, CalendarClock, ChevronRight, KeyRound, Plus, RefreshCw, Save, Search, Trash2 } from "lucide-react";
import {
  createAiTask,
  createAiProfile,
  createAiProvider,
  deleteAiTask,
  deleteAiProfile,
  deleteAiProfileAvatar,
  deleteAiProfilePublicIdentityAvatar,
  deleteAiProvider,
  listAiChannelOptions,
  listAiProfiles,
  listAiProviders,
  listAiTaskRuns,
  listAiTasks,
  listKnowledgeBases,
  updateAiTask,
  updateAiProfile,
  updateAiProvider,
  uploadAiProfileAvatar,
  uploadAiProfilePublicIdentityAvatar,
  type AiProfile,
  type AiProfileInput,
  type AiProfilePublicIdentity,
  type AiProvider,
  type AiProviderInput,
  type AiChannelOption,
  type Inbox,
  type KnowledgeBase,
  type OperatorAuth,
  type AiTask,
  type AiTaskInput,
  type AiTaskRun,
  type AiTaskSchedule,
} from "../api";
import { channelKindLabel, channelStatusLabel, useI18n, type MessageKey } from "../i18n";
import { AvatarUploadField } from "./AvatarUploadField";
import { AgentVoiceSettings } from "./AgentVoiceSettings";
import { modelTypeLabel } from "./voice-i18n";
import { isChatModelConnection } from "../ai-model-types";

interface Props {
  auth: OperatorAuth;
  inboxes: Inbox[];
  canManageApi?: boolean;
  access?: TaskAccessFlags;
}

interface Notice {
  kind: "success" | "error";
  message: MessageKey;
}

type Tab = "profiles" | "tasks" | "providers" | "skills";
type SettingsTab = Exclude<Tab, "tasks">;
type View = "settings" | "tasks";
type TaskRunsState = "idle" | "loading" | "ready" | "error";

type ProfileDraft = Omit<AiProfileInput, "custom_fields" | "secrets" | "public_identities">;

interface PublicIdentityDraft extends AiProfilePublicIdentity {
  file: File | null;
  removed: boolean;
}

interface CustomFieldDraft {
  id: string;
  key: string;
  value: string;
}

interface SecretDraft {
  id: string;
  key: string;
  value: string;
  configured: boolean;
  originalKey: string | null;
}

let credentialRowSequence = 0;

const weekdayOptions: ReadonlyArray<{ value: number; label: MessageKey }> = [
  { value: 1, label: "ai.taskWeekdayMonday" },
  { value: 2, label: "ai.taskWeekdayTuesday" },
  { value: 3, label: "ai.taskWeekdayWednesday" },
  { value: 4, label: "ai.taskWeekdayThursday" },
  { value: 5, label: "ai.taskWeekdayFriday" },
  { value: 6, label: "ai.taskWeekdaySaturday" },
  { value: 7, label: "ai.taskWeekdaySunday" },
];

const taskRunStatusKeys: Record<AiTaskRun["status"], MessageKey> = {
  pending: "ai.taskRunStatusPending",
  processing: "ai.taskRunStatusProcessing",
  completed: "ai.taskRunStatusCompleted",
  failed: "ai.taskRunStatusFailed",
  cancelled: "ai.taskRunStatusCancelled",
};

const monthDays = Array.from({ length: 31 }, (_, index) => index + 1);
const maximumTaskAgents = 32;

function browserTimezone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
}

function taskAgentIsAvailable(profile: AiProfile, providers: AiProvider[]): boolean {
  if (profile.status !== "active" || !profile.provider_connection_id) return false;
  const provider = providers.find((item) => item.id === profile.provider_connection_id);
  return provider?.status === "active" && isChatModelConnection(provider)
    && (provider.provider_kind === "openai" || provider.provider_kind === "openai_compatible");
}

function emptyTaskInput(): AiTaskInput {
  return { text: "", agent_ids: [], execution_mode: "independent", expected_result: "", schedule: { kind: "once", run_at: "" } };
}

function taskInput(task: AiTask): AiTaskInput {
  return {
    text: task.text,
    execution_mode: task.execution_mode,
    coordinator_id: task.coordinator_id,
    expected_result: task.expected_result,
    agent_roles: task.agent_roles,
    agent_ids: [...task.agent_ids],
    schedule: task.schedule.kind === "weekly"
      ? { ...task.schedule, weekdays: [...task.schedule.weekdays] }
      : task.schedule.kind === "monthly"
        ? { ...task.schedule, month_days: [...task.schedule.month_days] }
        : task.schedule.kind === "yearly"
          ? { ...task.schedule, dates: task.schedule.dates.map((date) => ({ ...date })) }
          : { ...task.schedule },
  };
}

function recurringSchedule(
  kind: Exclude<AiTaskSchedule["kind"], "once">,
  current: AiTaskSchedule,
): AiTaskSchedule {
  if (kind === "manual") return { kind };
  const time = "time" in current ? current.time : "09:00";
  const timezone = "timezone" in current ? current.timezone : browserTimezone();
  if (kind === "weekly") return { kind, time, timezone, weekdays: [1] };
  if (kind === "monthly") return { kind, time, timezone, month_days: [1] };
  if (kind === "yearly") return { kind, time, timezone, dates: [{ month: 1, day: 1 }] };
  return { kind, time, timezone };
}

function datetimeLocalValue(value: string): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  const local = new Date(date.getTime() - date.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}

function datetimeLocalIso(value: string): string {
  if (!value) return "";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "" : date.toISOString();
}

function minimumTaskRunAt(): string {
  const date = new Date();
  date.setSeconds(0, 0);
  date.setMinutes(date.getMinutes() + 1);
  return datetimeLocalValue(date.toISOString());
}

function annualDateIsValid(month: number, day: number): boolean {
  if (!Number.isInteger(month) || !Number.isInteger(day)) return false;
  const date = new Date(Date.UTC(2000, month - 1, day));
  return date.getUTCMonth() === month - 1 && date.getUTCDate() === day;
}

function annualDatesAreValid(dates: Array<{ month: number; day: number }>): boolean {
  const keys = dates.map(({ month, day }) => `${month}-${day}`);
  return dates.length > 0
    && dates.every(({ month, day }) => annualDateIsValid(month, day))
    && new Set(keys).size === keys.length;
}

function integerChoicesAreValid(values: number[], minimum: number, maximum: number): boolean {
  return values.length > 0
    && values.every((value) => Number.isInteger(value) && value >= minimum && value <= maximum)
    && new Set(values).size === values.length;
}

function taskDraftIsValid(draft: AiTaskInput): boolean {
  if (!draft.text.trim() || draft.agent_ids.length === 0 || draft.agent_ids.length > maximumTaskAgents) return false;
  const { schedule } = draft;
  if (schedule.kind === "manual") return true;
  if (schedule.kind === "once") {
    return schedule.run_at !== "" && new Date(schedule.run_at).getTime() > Date.now();
  }
  if (!/^(?:[01]\d|2[0-3]):[0-5]\d$/.test(schedule.time) || !schedule.timezone) return false;
  if (schedule.kind === "weekly") return integerChoicesAreValid(schedule.weekdays, 1, 7);
  if (schedule.kind === "monthly") return integerChoicesAreValid(schedule.month_days, 1, 31);
  if (schedule.kind === "yearly") {
    return annualDatesAreValid(schedule.dates);
  }
  return true;
}

function normalizedTaskInput(draft: AiTaskInput): AiTaskInput {
  const schedule = draft.schedule.kind === "weekly"
    ? { ...draft.schedule, weekdays: [...draft.schedule.weekdays].sort((a, b) => a - b) }
    : draft.schedule.kind === "monthly"
      ? { ...draft.schedule, month_days: [...draft.schedule.month_days].sort((a, b) => a - b) }
      : draft.schedule.kind === "yearly"
        ? {
            ...draft.schedule,
            dates: [...draft.schedule.dates].sort((a, b) => a.month - b.month || a.day - b.day),
          }
        : draft.schedule;
  return {
    ...draft,
    text: draft.text.trim(),
    agent_ids: [...new Set(draft.agent_ids)],
    schedule,
  };
}

function credentialRowId(prefix: "field" | "secret") {
  credentialRowSequence += 1;
  return `${prefix}-${credentialRowSequence}`;
}

const emptyProfile: ProfileDraft = {
  name: "",
  description: "",
  status: "draft",
  provider_connection_id: null,
  model: null,
  instructions: "",
  tool_instructions: "",
  blacklist_reply_text: "",
  blacklist_reply_match_language: true,
  http_allowed_hosts: [],
  capabilities: {
    http_get: false,
    http_post: false,
    shell: false,
  },
  language: "ru",
  max_output_tokens: 800,
  max_concurrent_runs: 2,
  auto_join_new_conversations: false,
  can_resolve_conversations: false,
  telegram_notifications: {
    new_visitor: false,
    new_message: false,
    operator_request: true,
  },
  knowledge_base_ids: [],
  channel_ids: [],
  ...({}),
};

const emptyProvider: AiProviderInput = {
  name: "",
  provider_kind: "openai",
  model_type: "chat",
  base_url: "",
  default_model: "",
  status: "active",
};

function profileInput(profile: AiProfile): ProfileDraft {
  return {
    visibility: profile.visibility,
    name: profile.name,
    description: profile.description ?? "",
    status: profile.status,
    provider_connection_id: profile.provider_connection_id,
    model: profile.model,
    instructions: profile.instructions,
    tool_instructions: profile.tool_instructions,
    blacklist_reply_text: profile.blacklist_reply_text ?? "",
    blacklist_reply_match_language: profile.blacklist_reply_match_language ?? true,
    http_allowed_hosts: profile.http_allowed_hosts ?? [],
    capabilities: { ...profile.capabilities },
    language: profile.language,
    max_output_tokens: profile.max_output_tokens,
    max_concurrent_runs: profile.max_concurrent_runs ?? 2,
    auto_join_new_conversations: profile.auto_join_new_conversations,
    can_resolve_conversations: profile.can_resolve_conversations,
    telegram_notifications: profile.telegram_notifications,
    knowledge_base_ids: profile.knowledge_base_ids,
    channel_ids: profile.channel_ids,
    ...({}),
  };
}

function publicIdentityDrafts(profile: AiProfile): PublicIdentityDraft[] {
  return reconcilePublicIdentities(
    profile.public_identities.map((identity) => ({
      ...identity,
      file: null,
      removed: false,
    })),
    profile.language,
    profile.name,
  );
}

function replyLanguages(value: string): string[] {
  const seen = new Set<string>();
  return value
    .split(",")
    .map((language) => language.trim())
    .filter((language) => /^[a-z0-9]{1,8}(?:-[a-z0-9]{1,8})*$/i.test(language))
    .filter((language) => {
      const key = language.toLowerCase();
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
}

function reconcilePublicIdentities(
  identities: PublicIdentityDraft[],
  languageList: string,
  fallbackName: string,
): PublicIdentityDraft[] {
  return replyLanguages(languageList).map((language) => {
    const existing = identities.find(
      (identity) => identity.language.toLowerCase() === language.toLowerCase(),
    );
    return existing ?? {
      language,
      display_name: fallbackName,
      avatar_url: null,
      file: null,
      removed: false,
    };
  });
}

function customFieldDrafts(profile: AiProfile): CustomFieldDraft[] {
  return profile.custom_fields.map((field) => ({
    id: credentialRowId("field"),
    key: field.key,
    value: field.value,
  }));
}

function secretDrafts(profile: AiProfile): SecretDraft[] {
  return profile.secrets.map((secret) => ({
    id: credentialRowId("secret"),
    key: secret.key,
    value: "",
    configured: secret.configured,
    originalKey: secret.key,
  }));
}

function providerInput(provider: AiProvider): AiProviderInput {
  return {
    visibility: provider.visibility,
    name: provider.name,
    provider_kind: provider.provider_kind,
    model_type: provider.model_type ?? "chat",
    base_url: provider.base_url,
    default_model: provider.default_model,
    status: provider.status,
  };
}

const NEW_RECORD = "new";

/**
 * AI URLs: `/ai` and `/ai/agents/<id|new>[/<agent section>]`, `/ai/providers[/<id|new>]` and `/ai/skills[/<id|new>]`,
 * whose skill part AISkillsView reads itself.
 */
function aiSettingsRoute(segments: readonly string[]) {
  const [section, recordId = null, profileSection] = segments;
  const tab: SettingsTab = section === "providers" || section === "skills" ? section : "profiles";
  return {
    tab,
    profileId: section === "agents" ? recordId : null,
    profileTab: agentProfileTabs.find((value) => value === profileSection) ?? "settings",
    providerId: tab === "providers" ? recordId : null,
  };
}

function aiSettingsSegments(tab: SettingsTab, recordId: string | null = null, profileTab: AgentProfileTab = "settings"): string[] {
  if (tab !== "profiles") return recordId ? [tab, recordId] : [tab];
  if (!recordId) return [];
  return profileTab === "settings" ? ["agents", recordId] : ["agents", recordId, profileTab];
}

function AIWorkspaceView({ auth, inboxes, view, canManageApi = false }: Props & { view: View }) {
  const instanceId = useId();
  const { locale, t, formatDate } = useI18n();
  const route = usePageRoute();
  const navigateRoute = route.navigate;
  const routed = aiSettingsRoute(route.segments);
  const tab = routed.tab;
  const routeSegmentsRef = useRef(route.segments);
  // What each tab last showed, so that switching tabs returns to it.
  const tabRoutesRef = useRef<Partial<Record<SettingsTab, readonly string[]>>>({});
  const [skillsOpened, setSkillsOpened] = useState(false);
  const [skillsBusy, setSkillsBusy] = useState(false);
  const [skillAssignmentChange, setSkillAssignmentChange] = useState<AiProfileSkillAssignment | null>(null);
  const [profiles, setProfiles] = useState<AiProfile[]>([]);
  const [tasks, setTasks] = useState<AiTask[]>([]);
  const [taskRuns, setTaskRuns] = useState<AiTaskRun[]>([]);
  const [taskRunsState, setTaskRunsState] = useState<TaskRunsState>("idle");
  const [providers, setProviders] = useState<AiProvider[]>([]);
  const [providerSearch, setProviderSearch] = useState("");
  const [channels, setChannels] = useState<AiChannelOption[]>([]);
  const [knowledgeBases, setKnowledgeBases] = useState<KnowledgeBase[]>([]);
  const [selectedProfileId, setSelectedProfileId] = useState<string | "new" | null>(null);
  const [selectedTaskId, setSelectedTaskId] = useState<string | "new" | null>(null);
  const [selectedProviderId, setSelectedProviderId] = useState<string | "new" | null>(null);
  // The agent and connection last opened from the URL; `undefined` until the records have loaded.
  const [routedProfileId, setRoutedProfileId] = useState<string | null>();
  const [routedProviderId, setRoutedProviderId] = useState<string | null>();
  const profileMotion = useContentMotion<HTMLDivElement>(`${tab}:${selectedProfileId}`);
  const providerMotion = useContentMotion<HTMLDivElement>(`${tab}:${selectedProviderId}`);
  const [profileDraft, setProfileDraft] = useState<ProfileDraft>(emptyProfile);
  const [taskDraft, setTaskDraft] = useState<AiTaskInput>(emptyTaskInput);
  // Set when a task form opens, not on every render: a limit that moves with the clock would rewrite the field's error each minute.
  const [minimumRunAt, setMinimumRunAt] = useState(minimumTaskRunAt);
  const [profileAvatarFile, setProfileAvatarFile] = useState<File | null>(null);
  const [profileAvatarRemoved, setProfileAvatarRemoved] = useState(false);
  const [publicIdentities, setPublicIdentities] = useState<PublicIdentityDraft[]>([]);
  const [customFields, setCustomFields] = useState<CustomFieldDraft[]>([]);
  const [secrets, setSecrets] = useState<SecretDraft[]>([]);
  const [providerDraft, setProviderDraft] = useState<AiProviderInput>(emptyProvider);
  const [providerApiKey, setProviderApiKey] = useState("");
  const [clearProviderApiKey, setClearProviderApiKey] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [tasksLoading, setTasksLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [instructionSourceSaving, setInstructionSourceSaving] = useState(false);
  const [skillAssignmentSaving, setSkillAssignmentSaving] = useState(false);
  const sourceSaving = instructionSourceSaving || skillAssignmentSaving;
  const [notice, setNotice] = useState<Notice | null>(null);
  const profileProviderRef = useRef<HTMLSelectElement>(null);
  const profileInstructionsRef = useRef<HTMLTextAreaElement>(null);
  const profileProviderError = notice?.message === "ai.profileProviderRequired"
    && profileDraft.status === "active" && !profileDraft.provider_connection_id;
  const profileInstructionsError = notice?.message === "ai.profileInstructionsRequired"
    && profileDraft.status === "active" && !profileDraft.instructions.trim();
  const taskListRequestSequence = useRef(0);
  const taskRunsRequestSequence = useRef(0);
  const activeTab: Tab = view === "tasks" ? "tasks" : tab;

  const selectedProfile = useMemo(
    () => profiles.find((profile) => profile.id === selectedProfileId) ?? null,
    [profiles, selectedProfileId],
  );
  // Tests run against a saved agent only.
  const profileTab: AgentProfileTab = routed.profileTab === "tests" && !selectedProfile ? "settings" : routed.profileTab;
  const selectedProvider = useMemo(
    () => providers.find((provider) => provider.id === selectedProviderId) ?? null,
    [providers, selectedProviderId],
  );
  const providerQuery = providerSearch.trim().toLocaleLowerCase();
  const matchingProviders = providers.filter((provider) =>
    `${provider.name} ${provider.default_model} ${t(`ai.providerKind.${provider.provider_kind}` as MessageKey)} ${modelTypeLabel(locale, provider.model_type)}`.toLocaleLowerCase().includes(providerQuery),
  );
  const selectedTask = useMemo(
    () => tasks.find((task) => task.id === selectedTaskId) ?? null,
    [tasks, selectedTaskId],
  );
  const providerTarget = providerDraft.default_model.trim();
  const providerTargetIsValid = providerTarget !== "";
  const taskAgentOptions = profiles.filter((profile) => (
    taskAgentIsAvailable(profile, providers) || taskDraft.agent_ids.includes(profile.id)
  ));
  const taskAssignedAgentsAreAvailable = taskDraft.agent_ids.every((id) => {
    const profile = profiles.find((item) => item.id === id);
    return profile ? taskAgentIsAvailable(profile, providers) : false;
  });
  const hasSelectedActiveWidgetChannel = channels.some((channel) => (
    channel.kind === "widget"
    && channel.status === "active"
    && profileDraft.channel_ids.includes(channel.id)
  ));

  async function reload(preferred?: { profileId?: string; providerId?: string }) {
    const [nextProfiles, nextProviders, nextKnowledgeBases, nextChannels] = await Promise.all([
      listAiProfiles(auth),
      listAiProviders(auth),
      listKnowledgeBases(auth),
      listAiChannelOptions(auth),
    ]);
    setProfiles(nextProfiles);
    setProviders(nextProviders);
    setKnowledgeBases(nextKnowledgeBases);
    setChannels(nextChannels);
    setLoadFailed(false);
    if (preferred?.profileId) {
      const profile = nextProfiles.find((item) => item.id === preferred.profileId);
      if (profile) {
        setSelectedProfileId(profile.id);
        setProfileDraft(profileInput(profile));
        setCustomFields(customFieldDrafts(profile));
        setSecrets(secretDrafts(profile));
        setPublicIdentities(publicIdentityDrafts(profile));
        setProfileAvatarFile(null);
        setProfileAvatarRemoved(false);
      } else {
        // Its saved visibility can exclude this workspace.
        closeProfile();
      }
    }
    if (preferred?.providerId) {
      const provider = nextProviders.find((item) => item.id === preferred.providerId);
      setSelectedProviderId(provider?.id ?? null);
      setProviderDraft(provider ? providerInput(provider) : emptyProvider);
    }
    return { profiles: nextProfiles, providers: nextProviders };
  }

  const reloadTasks = useCallback(async (preferredTaskId?: string) => {
    const requestSequence = taskListRequestSequence.current + 1;
    taskListRequestSequence.current = requestSequence;
    await Promise.resolve();
    if (taskListRequestSequence.current !== requestSequence) return;
    setTasksLoading(true);
    try {
      const nextTasks = await listAiTasks(auth);
      if (taskListRequestSequence.current !== requestSequence) return;
      setTasks(nextTasks);
      if (preferredTaskId) {
        const task = nextTasks.find((item) => item.id === preferredTaskId);
        if (task) {
          setSelectedTaskId(task.id);
          setTaskDraft(taskInput(task));
        }
        return;
      }
      setSelectedTaskId((current) => (
        current && current !== "new" && !nextTasks.some((task) => task.id === current)
          ? null
          : current
      ));
    } catch (error) {
      if (taskListRequestSequence.current === requestSequence) throw error;
    } finally {
      if (taskListRequestSequence.current === requestSequence) setTasksLoading(false);
    }
  }, [auth]);

  function clearTaskRuns() {
    taskRunsRequestSequence.current += 1;
    setTaskRuns([]);
    setTaskRunsState("idle");
  }

  async function reloadTaskRuns(taskId: string) {
    const requestSequence = taskRunsRequestSequence.current + 1;
    taskRunsRequestSequence.current = requestSequence;
    setTaskRuns([]);
    setTaskRunsState("loading");
    try {
      const items = await listAiTaskRuns(auth, taskId);
      if (taskRunsRequestSequence.current !== requestSequence) return;
      setTaskRuns(items);
      setTaskRunsState("ready");
    } catch {
      if (taskRunsRequestSequence.current !== requestSequence) return;
      setTaskRunsState("error");
    }
  }

  useEffect(() => {
    let active = true;
    if (view === "tasks") {
      Promise.all([listAiProfiles(auth), listAiProviders(auth)])
        .then(([nextProfiles, nextProviders]) => {
          if (!active) return;
          setProfiles(nextProfiles);
          setProviders(nextProviders);
        })
        .catch(() => {
          if (active) setNotice({ kind: "error", message: "ai.loadError" });
        })
        .finally(() => {
          if (active) setLoading(false);
        });
      return () => { active = false; };
    }
    Promise.all([listAiProfiles(auth), listAiProviders(auth), listKnowledgeBases(auth), listAiChannelOptions(auth)])
      .then(([nextProfiles, nextProviders, nextKnowledgeBases, nextChannels]) => {
        if (!active) return;
        setProfiles(nextProfiles);
        setProviders(nextProviders);
        setKnowledgeBases(nextKnowledgeBases);
        setChannels(nextChannels);
      })
      .catch(() => {
        if (!active) return;
        setLoadFailed(true);
        setNotice({ kind: "error", message: "ai.loadError" });
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => { active = false; };
  }, [auth, view]);

  useEffect(() => {
    if (activeTab !== "tasks") return undefined;
    let active = true;
    let requestSequence: number | null = null;
    void Promise.resolve()
      .then(() => {
        if (!active) return undefined;
        const request = reloadTasks();
        requestSequence = taskListRequestSequence.current;
        return request;
      })
      .catch(() => {
        if (active) setNotice({ kind: "error", message: "ai.taskLoadError" });
      });
    return () => {
      active = false;
      if (
        requestSequence !== null
        && taskListRequestSequence.current === requestSequence
      ) {
        taskListRequestSequence.current += 1;
      }
    };
  }, [activeTab, reloadTasks]);

  useEffect(() => {
    routeSegmentsRef.current = route.segments;
    tabRoutesRef.current[tab] = route.segments;
  }, [route.segments, tab]);

  function openTab(next: SettingsTab) {
    const last = tabRoutesRef.current;
    if (next === "skills") void navigateRoute(last.skills ?? ["skills"]);
    else if (next === "providers") void navigateRoute(aiSettingsSegments("providers", selectedProviderId));
    else void navigateRoute(aiSettingsSegments("profiles", selectedProfileId, aiSettingsRoute(last.profiles ?? []).profileTab));
  }

  function openProfile(profileId: string | null, section: AgentProfileTab = "settings", replace = false) {
    return navigateRoute(aiSettingsSegments("profiles", profileId, section), { replace });
  }

  function openProvider(providerId: string | null, replace = false) {
    return navigateRoute(aiSettingsSegments("providers", providerId), { replace });
  }

  // Requests can finish after Back/Forward opened another record; they update only the editor the URL still shows.
  function routedRecord(section: SettingsTab) {
    const current = aiSettingsRoute(routeSegmentsRef.current);
    if (current.tab !== section) return undefined;
    return section === "profiles" ? current.profileId : current.providerId;
  }

  function stillEditing(section: SettingsTab, recordId: string) {
    const shown = routedRecord(section);
    return shown === undefined || shown === recordId;
  }

  /** Moves the URL from a saved or deleted record, e.g. `new`, to what its editor shows now. */
  function replaceRecordRoute(section: SettingsTab, fromId: string, toId: string | null) {
    if (routedRecord(section) !== fromId) return;
    void navigateRoute(aiSettingsSegments(section, toId, aiSettingsRoute(routeSegmentsRef.current).profileTab), { replace: true });
  }

  /** Shows the agent section with an invalid field, then focuses the field. */
  function focusProfileField(section: AgentProfileTab, field: () => HTMLElement | null) {
    void Promise.resolve(openProfile(selectedProfileId, section, true)).then(() => requestAnimationFrame(() => field()?.focus()));
  }

  function showNewProfile(draft: ProfileDraft) {
    setSelectedProfileId("new");
    setProfileDraft(draft);
    setCustomFields([]);
    setSecrets([]);
    setPublicIdentities(reconcilePublicIdentities([], draft.language, draft.name));
    setProfileAvatarFile(null);
    setProfileAvatarRemoved(false);
    setNotice(null);
  }

  function startProfile() {
    showNewProfile(emptyProfile);
    void openProfile(NEW_RECORD);
  }


  function showProfile(profile: AiProfile) {
    setSelectedProfileId(profile.id);
    setProfileDraft(profileInput(profile));
    setCustomFields(customFieldDrafts(profile));
    setSecrets(secretDrafts(profile));
    setPublicIdentities(publicIdentityDrafts(profile));
    setProfileAvatarFile(null);
    setProfileAvatarRemoved(false);
    setNotice(null);
  }

  function selectProfile(profile: AiProfile) {
    showProfile(profile);
    void openProfile(profile.id);
  }

  function closeProfile() {
    setSelectedProfileId(null);
    setProfileDraft(emptyProfile);
    setCustomFields([]);
    setSecrets([]);
    setPublicIdentities([]);
    setProfileAvatarFile(null);
    setProfileAvatarRemoved(false);
  }

  function startTask() {
    setSelectedTaskId("new");
    setTaskDraft(emptyTaskInput());
    setMinimumRunAt(minimumTaskRunAt());
    clearTaskRuns();
    setNotice(null);
  }

  function selectTask(task: AiTask) {
    setSelectedTaskId(task.id);
    setTaskDraft(taskInput(task));
    setMinimumRunAt(minimumTaskRunAt());
    void reloadTaskRuns(task.id);
    setNotice(null);
  }

  function setTaskType(type: "once" | "recurring") {
    setTaskDraft((current) => ({
      ...current,
      schedule: type === "once"
        ? { kind: "once", run_at: "" }
        : recurringSchedule("daily", current.schedule),
    }));
  }

  function setTaskRecurrence(kind: Exclude<AiTaskSchedule["kind"], "once">) {
    setTaskDraft((current) => ({
      ...current,
      schedule: recurringSchedule(kind, current.schedule),
    }));
  }

  function toggleTaskAgent(profileId: string) {
    setTaskDraft((current) => {
      const isSelected = current.agent_ids.includes(profileId);
      if (!isSelected && current.agent_ids.length >= maximumTaskAgents) return current;
      return {
        ...current,
        agent_ids: isSelected
          ? current.agent_ids.filter((id) => id !== profileId)
          : [...current.agent_ids, profileId],
      };
    });
  }

  function toggleTaskScheduleValue(field: "weekdays" | "month_days", value: number) {
    setTaskDraft((current) => {
      const { schedule } = current;
      if (field === "weekdays" && schedule.kind === "weekly") {
        return {
          ...current,
          schedule: {
            ...schedule,
            weekdays: schedule.weekdays.includes(value)
              ? schedule.weekdays.filter((item) => item !== value)
              : [...schedule.weekdays, value],
          },
        };
      }
      if (field === "month_days" && schedule.kind === "monthly") {
        return {
          ...current,
          schedule: {
            ...schedule,
            month_days: schedule.month_days.includes(value)
              ? schedule.month_days.filter((item) => item !== value)
              : [...schedule.month_days, value],
          },
        };
      }
      return current;
    });
  }

  function addTaskAnnualDate() {
    setTaskDraft((current) => {
      if (current.schedule.kind !== "yearly") return current;
      const occupied = new Set(
        current.schedule.dates.map(({ month, day }) => `${month}-${day}`),
      );
      let nextDate: { month: number; day: number } | null = null;
      for (let month = 1; month <= 12 && !nextDate; month += 1) {
        for (let day = 1; day <= 31; day += 1) {
          if (annualDateIsValid(month, day) && !occupied.has(`${month}-${day}`)) {
            nextDate = { month, day };
            break;
          }
        }
      }
      if (!nextDate) return current;
      return {
        ...current,
        schedule: { ...current.schedule, dates: [...current.schedule.dates, nextDate] },
      };
    });
  }

  function updateTaskAnnualDate(index: number, field: "month" | "day", value: number) {
    setTaskDraft((current) => current.schedule.kind === "yearly"
      ? {
          ...current,
          schedule: {
            ...current.schedule,
            dates: current.schedule.dates.map((date, dateIndex) => (
              dateIndex === index ? { ...date, [field]: value } : date
            )),
          },
        }
      : current);
  }

  function removeTaskAnnualDate(index: number) {
    setTaskDraft((current) => current.schedule.kind === "yearly"
      ? {
          ...current,
          schedule: {
            ...current.schedule,
            dates: current.schedule.dates.filter((_, dateIndex) => dateIndex !== index),
          },
        }
      : current);
  }

  function startProvider() {
    showProvider(NEW_RECORD);
    void openProvider(NEW_RECORD);
  }

  function selectProvider(provider: AiProvider) {
    showProvider(provider);
    void openProvider(provider.id);
  }

  function showProvider(provider: AiProvider | typeof NEW_RECORD | null) {
    setSelectedProviderId(provider === NEW_RECORD ? NEW_RECORD : provider?.id ?? null);
    setProviderDraft(provider && provider !== NEW_RECORD ? providerInput(provider) : emptyProvider);
    setProviderApiKey("");
    setClearProviderApiKey(false);
    setNotice(null);
  }

  // Links and Back/Forward open records through the URL once they have loaded; clicks open them and then update the URL.
  const routeReady = view === "settings" && !loading && !loadFailed;
  const linkedProfileId = routeReady && tab === "profiles" ? routed.profileId : undefined;
  if (linkedProfileId !== undefined && linkedProfileId !== routedProfileId) {
    setRoutedProfileId(linkedProfileId);
    const profile = profiles.find((item) => item.id === linkedProfileId);
    if (linkedProfileId !== selectedProfileId) {
      if (profile) showProfile(profile);
      else if (linkedProfileId === NEW_RECORD) showNewProfile(emptyProfile);
      else { closeProfile(); setNotice(null); }
    }
  }
  const linkedProviderId = routeReady && tab === "providers" ? routed.providerId : undefined;
  if (linkedProviderId !== undefined && linkedProviderId !== routedProviderId) {
    setRoutedProviderId(linkedProviderId);
    if (linkedProviderId !== selectedProviderId) showProvider(linkedProviderId === NEW_RECORD ? NEW_RECORD : providers.find((item) => item.id === linkedProviderId) ?? null);
  }
  // The skills library loads when first shown and then stays mounted with its drafts.
  if (activeTab === "skills" && !skillsOpened) setSkillsOpened(true);

  const canonicalProfileId = routed.profileId === NEW_RECORD || profiles.some((profile) => profile.id === routed.profileId) ? routed.profileId : null;
  const canonicalProviderId = routed.providerId === NEW_RECORD || providers.some((provider) => provider.id === routed.providerId) ? routed.providerId : null;
  useCanonicalPageRoute(route, tab === "skills"
    ? route.segments // The skills library corrects its own part.
    : tab === "providers" ? aiSettingsSegments("providers", canonicalProviderId)
      : aiSettingsSegments("profiles", canonicalProfileId, canonicalProfileId === NEW_RECORD && routed.profileTab === "tests" ? "settings" : routed.profileTab), routeReady);

  function toggleProfileList(field: "knowledge_base_ids" | "channel_ids", id: string) {
    setProfileDraft((current) => ({
      ...current,
      [field]: current[field].includes(id)
        ? current[field].filter((value) => value !== id)
        : [...current[field], id],
    }));
  }

  function addCustomField() {
    if (customFields.length >= 32) return;
    setCustomFields((current) => [
      ...current,
      { id: credentialRowId("field"), key: "", value: "" },
    ]);
  }

  function updateCustomField(id: string, field: "key" | "value", value: string) {
    setCustomFields((current) => current.map((item) => (
      item.id === id ? { ...item, [field]: value } : item
    )));
  }

  function addSecret() {
    if (secrets.length >= 32) return;
    setSecrets((current) => [
      ...current,
      {
        id: credentialRowId("secret"),
        key: "",
        value: "",
        configured: false,
        originalKey: null,
      },
    ]);
  }

  function updateSecret(id: string, field: "key" | "value", value: string) {
    setSecrets((current) => current.map((item) => (
      item.id === id ? { ...item, [field]: value } : item
    )));
  }

  async function saveProfile(event: React.FormEvent) {
    event.preventDefault();
    if (!selectedProfileId || !profileDraft.name.trim() || saving || sourceSaving) return;
    if (profileDraft.status === "active" && !profileDraft.provider_connection_id) {
      setNotice({ kind: "error", message: "ai.profileProviderRequired" });
      focusProfileField("settings", () => profileProviderRef.current);
      return;
    }
    if (profileDraft.status === "active" && !profileDraft.instructions.trim()) {
      setNotice({ kind: "error", message: "ai.profileInstructionsRequired" });
      focusProfileField("instructions", () => profileInstructionsRef.current);
      return;
    }
    setSaving(true);
    setNotice(null);
    try {
      const payload: AiProfileInput = {
        ...profileDraft,
        name: profileDraft.name.trim(),
        model: profileDraft.model?.trim() || null,
        http_allowed_hosts: profileDraft.http_allowed_hosts.map((host) => host.trim()).filter(Boolean),
        capabilities: { ...profileDraft.capabilities },
        public_identities: publicIdentities.map((identity) => ({
          language: identity.language,
          display_name: identity.display_name.trim(),
        })),
        custom_fields: customFields.map((field) => ({
          key: field.key.trim(),
          value: field.value,
        })),
        secrets: secrets.map((secret) => {
          const key = secret.key.trim();
          const preservesStoredValue = secret.configured && key === secret.originalKey;
          return {
            key,
            value: secret.value === "" && preservesStoredValue ? null : secret.value,
          };
        }),
      };
      let saved = selectedProfileId === "new"
        ? await createAiProfile(auth, payload)
        : await updateAiProfile(auth, selectedProfileId, payload);
      if (profileAvatarFile) {
        saved = await uploadAiProfileAvatar(auth, saved.id, profileAvatarFile);
      } else if (profileAvatarRemoved && saved.avatar_url) {
        saved = await deleteAiProfileAvatar(auth, saved.id);
      }
      await Promise.all(publicIdentities.map(async (identity) => {
        if (identity.file) {
          await uploadAiProfilePublicIdentityAvatar(
            auth,
            saved.id,
            identity.language,
            identity.file,
          );
        } else if (identity.removed && identity.avatar_url) {
          await deleteAiProfilePublicIdentityAvatar(auth, saved.id, identity.language);
        }
      }));
      const next = await reload(stillEditing("profiles", selectedProfileId) ? { profileId: saved.id } : undefined);
      replaceRecordRoute("profiles", selectedProfileId, next.profiles.some((profile) => profile.id === saved.id) ? saved.id : null);
      setNotice({ kind: "success", message: "ai.profileSaved" });
    } catch {
      setNotice({ kind: "error", message: "ai.profileSaveError" });
    } finally {
      setSaving(false);
    }
  }

  async function removeProfile() {
    if (!selectedProfile || saving || sourceSaving || !window.confirm(t("ai.profileDeleteConfirm", { name: selectedProfile.name }))) return;
    setSaving(true);
    setNotice(null);
    try {
      await deleteAiProfile(auth, selectedProfile.id);
      if (stillEditing("profiles", selectedProfile.id)) closeProfile();
      replaceRecordRoute("profiles", selectedProfile.id, null);
      await reload();
      setNotice({ kind: "success", message: "ai.profileDeleted" });
    } catch {
      setNotice({ kind: "error", message: "ai.profileDeleteError" });
    } finally {
      setSaving(false);
    }
  }

  async function saveTask(event: React.FormEvent) {
    event.preventDefault();
    if (!selectedTaskId || !taskDraftIsValid(taskDraft) || !taskAssignedAgentsAreAvailable || saving) return;
    setSaving(true);
    setNotice(null);
    try {
      const payload = normalizedTaskInput(taskDraft);
      const creating = selectedTaskId === "new";
      const saved = creating
        ? await createAiTask(auth, payload)
        : await updateAiTask(auth, selectedTaskId, payload);
      setTasks((current) => creating
        ? [saved, ...current.filter((task) => task.id !== saved.id)]
        : current.map((task) => task.id === saved.id ? saved : task));
      setSelectedTaskId(saved.id);
      setTaskDraft(taskInput(saved));
      try {
        await reloadTasks(saved.id);
      } catch {
        // The mutation already succeeded; retain its response if list refresh is temporarily unavailable.
      }
      await reloadTaskRuns(saved.id);
      setNotice({ kind: "success", message: "ai.taskSaved" });
    } catch {
      setNotice({ kind: "error", message: "ai.taskSaveError" });
    } finally {
      setSaving(false);
    }
  }

  async function removeTask() {
    if (!selectedTask || saving || !window.confirm(t("ai.taskDeleteConfirm"))) return;
    setSaving(true);
    setNotice(null);
    try {
      await deleteAiTask(auth, selectedTask.id);
      setTasks((current) => current.filter((task) => task.id !== selectedTask.id));
      setSelectedTaskId(null);
      setTaskDraft(emptyTaskInput());
      clearTaskRuns();
      try {
        await reloadTasks();
      } catch {
        // The deletion already succeeded; keep the local list consistent until a later refresh.
      }
      setNotice({ kind: "success", message: "ai.taskDeleted" });
    } catch {
      setNotice({ kind: "error", message: "ai.taskDeleteError" });
    } finally {
      setSaving(false);
    }
  }

  async function saveProvider(event: React.FormEvent) {
    event.preventDefault();
    if (!selectedProviderId || !providerDraft.name.trim() || !providerDraft.base_url.trim() || !providerTargetIsValid || saving) return;
    setSaving(true);
    setNotice(null);
    try {
      const payload: AiProviderInput = {
        ...providerDraft,
        name: providerDraft.name.trim(),
        base_url: providerDraft.base_url.trim(),
        default_model: providerDraft.default_model.trim(),
        ...(providerApiKey.trim() ? { api_key: providerApiKey.trim() } : {}),
        ...(clearProviderApiKey ? { clear_api_key: true } : {}),
      };
      const saved = selectedProviderId === "new"
        ? await createAiProvider(auth, payload)
        : await updateAiProvider(auth, selectedProviderId, payload);
      const editing = stillEditing("providers", selectedProviderId);
      const next = await reload(editing ? { providerId: saved.id } : undefined);
      replaceRecordRoute("providers", selectedProviderId, next.providers.some((provider) => provider.id === saved.id) ? saved.id : null);
      if (editing) {
        setProviderApiKey("");
        setClearProviderApiKey(false);
      }
      setNotice({ kind: "success", message: "ai.providerSaved" });
    } catch {
      setNotice({ kind: "error", message: "ai.providerSaveError" });
    } finally {
      setSaving(false);
    }
  }

  async function removeProvider() {
    if (!selectedProvider || saving || !window.confirm(t("ai.providerDeleteConfirm", { name: selectedProvider.name }))) return;
    setSaving(true);
    setNotice(null);
    try {
      await deleteAiProvider(auth, selectedProvider.id);
      if (stillEditing("providers", selectedProvider.id)) {
        setSelectedProviderId(null);
        setProviderDraft(emptyProvider);
      }
      replaceRecordRoute("providers", selectedProvider.id, null);
      await reload();
      setNotice({ kind: "success", message: "ai.providerDeleted" });
    } catch {
      setNotice({ kind: "error", message: "ai.providerDeleteError" });
    } finally {
      setSaving(false);
    }
  }

  function taskScheduleLabel(schedule: AiTaskSchedule): string {
    if (schedule.kind === "manual") return taskWorkspaceText(locale)("manual");
    if (schedule.kind === "once") {
      return t("ai.taskSummaryOnce", {
        date: formatDate(schedule.run_at, {
          year: "numeric",
          month: "short",
          day: "numeric",
          hour: "2-digit",
          minute: "2-digit",
        }),
      });
    }
    const [hour, minute] = schedule.time.split(":").map(Number);
    const time = formatDate(new Date(Date.UTC(2000, 0, 1, hour, minute)), {
      hour: "2-digit",
      minute: "2-digit",
      timeZone: "UTC",
    });
    if (schedule.kind === "daily") {
      return t("ai.taskSummaryDaily", { time, timezone: schedule.timezone });
    }
    if (schedule.kind === "weekly") {
      const days = weekdayOptions
        .filter(({ value }) => schedule.weekdays.includes(value))
        .map(({ label }) => t(label))
        .join(", ");
      return t("ai.taskSummaryWeekly", {
        days,
        time,
        timezone: schedule.timezone,
      });
    }
    if (schedule.kind === "monthly") {
      return t("ai.taskSummaryMonthly", {
        days: schedule.month_days.join(", "),
        time,
        timezone: schedule.timezone,
      });
    }
    const dates = schedule.dates.map(({ month, day }) => formatDate(
      new Date(Date.UTC(2000, month - 1, day)),
      { month: "short", day: "numeric", timeZone: "UTC" },
    )).join(", ");
    return t("ai.taskSummaryYearly", {
      dates,
      time,
      timezone: schedule.timezone,
    });
  }

  function taskAgentsLabel(agentIds: string[]): string {
    const agents = agentIds
      .map((id) => profiles.find((profile) => profile.id === id)?.name ?? t("ai.taskUnknownAgent"))
      .join(", ");
    return t("ai.taskAgentsSummary", { agents });
  }

  function taskAgentAvailabilityLabel(profile: AiProfile): string {
    if (taskAgentIsAvailable(profile, providers)) return t("ai.taskAgentAvailable");
    const status = t(profile.status === "active" ? "ai.statusActive" : profile.status === "disabled" ? "ai.statusDisabled" : "ai.statusDraft");
    return t("ai.taskAgentUnavailable", { status });
  }

  function taskRunDate(value: string | null): string {
    if (!value) return "—";
    return formatDate(value, {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  return (
    <section className={`page ai-settings-page ai-settings-page--${view}`}>
      <header className="page-toolbar">
        <div>
          <h1>{t(view === "tasks" ? "ai.tasks" : "ai.title")}</h1>
          <p>{view === "tasks" ? t("ai.tasksDescription") : aiSkillsText(locale)("description")}</p>
        </div>
        {activeTab !== "skills" && <button className="primary-button" type="button" disabled={saving || sourceSaving} onClick={activeTab === "profiles" ? startProfile : activeTab === "tasks" ? startTask : startProvider}>
          <Plus size={16} />{t(activeTab === "profiles" ? "ai.newProfile" : activeTab === "tasks" ? "ai.newTask" : "ai.newProvider")}
        </button>}
      </header>

      {view === "settings" && (
        <nav className="section-tabs" aria-label={t("ai.sections")} role="tablist">
          <button id={`${instanceId}-ai-settings-tab-profiles`} aria-controls={`${instanceId}-ai-settings-panel-profiles`} aria-selected={activeTab === "profiles"} className={activeTab === "profiles" ? "section-tab--active" : ""} role="tab" type="button" disabled={sourceSaving || skillsBusy} onClick={() => openTab("profiles")}><Bot size={16} />{t("ai.profiles")}</button>
          <button id={`${instanceId}-ai-settings-tab-providers`} aria-controls={`${instanceId}-ai-settings-panel-providers`} aria-selected={activeTab === "providers"} className={activeTab === "providers" ? "section-tab--active" : ""} role="tab" type="button" disabled={sourceSaving || skillsBusy} onClick={() => openTab("providers")}><KeyRound size={16} />{t("ai.providers")}</button>
          <button id={`${instanceId}-ai-settings-tab-skills`} aria-controls={`${instanceId}-ai-settings-panel-skills`} aria-selected={activeTab === "skills"} className={activeTab === "skills" ? "section-tab--active" : ""} role="tab" type="button" disabled={saving || sourceSaving || skillsBusy} onClick={() => openTab("skills")}><BookOpen size={16} />{aiSkillsText(locale)("title")}</button>
        </nav>
      )}

      {notice && <div className={`admin-notice admin-notice--${notice.kind}`} role={notice.kind === "error" ? "alert" : "status"}>{t(notice.message)}</div>}

      {/* The library reads its part of this page's route, also where no router provides one. */}
      {skillsOpened && <div className="ai-skills-panel" id={`${instanceId}-ai-settings-panel-skills`} role="tabpanel" aria-labelledby={`${instanceId}-ai-settings-tab-skills`} hidden={activeTab !== "skills"}><PageRouteContext.Provider value={route}><AISkillsView auth={auth} profiles={profiles} profilesLoading={loading} providers={providers} providersLoading={loading} onBusyChange={setSkillsBusy} assignmentChange={skillAssignmentChange} /></PageRouteContext.Provider></div>}

      {activeTab === "profiles" ? (
        <div className="settings-layout ai-settings-layout" id={`${instanceId}-ai-settings-panel-profiles`} role="tabpanel" aria-labelledby={`${instanceId}-ai-settings-tab-profiles`}>
          <AgentProfileList profiles={profiles} selectedId={selectedProfileId} loading={loading} disabled={saving || sourceSaving} onSelect={selectProfile} onCreate={startProfile} />

          <div ref={profileMotion} className="settings-panel ai-editor-panel">
            {selectedProfileId ? (
              <>
                <header className="agent-profile-heading">
                  {selectedProfile && <AgentProfileMark profile={{ ...selectedProfile, name: profileDraft.name }} />}
                  <div><h2>{selectedProfileId === "new" && !profileDraft.preset_key ? t("ai.newProfileTitle") : profileDraft.name}</h2><p>{t(selectedProfileId === "new" && profileDraft.preset_key ? "ai.presetUnsaved" : selectedProfile?.preset_key ? "ai.presetEditable" : "ai.customAgent")}</p></div>
                </header>
                <nav className="section-tabs ai-profile-tabs" aria-label={t("ai.profileSections")} role="tablist">
                  {agentProfileTabs.map((value) => (
                    <button
                      key={value}
                      id={`${instanceId}-ai-profile-tab-${value}`}
                      aria-controls={`${instanceId}-ai-profile-panel-${value}`}
                      aria-selected={profileTab === value}
                      tabIndex={profileTab === value ? 0 : -1}
                      className={profileTab === value ? "section-tab--active" : ""}
                      role="tab"
                      type="button"
                      disabled={skillAssignmentSaving || (value === "tests" && !selectedProfile)}
                      onClick={() => void openProfile(selectedProfileId, value)}
                      onKeyDown={(event) => {
                        if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
                        event.preventDefault();
                        const available = agentProfileTabs.filter((tab) => tab !== "tests" || selectedProfile);
                        const index = available.indexOf(profileTab);
                        const next = event.key === "Home" ? available[0]
                          : event.key === "End" ? available[available.length - 1]
                          : available[(index + (event.key === "ArrowRight" ? 1 : -1) + available.length) % available.length];
                        void openProfile(selectedProfileId, next);
                        document.getElementById(`${instanceId}-ai-profile-tab-${next}`)?.focus();
                      }}
                    >{agentProfileTabLabel(locale, value)}</button>
                  ))}
                </nav>
                <form className="agent-profile-config-form" hidden={profileTab === "tests"} onSubmit={saveProfile} onInvalidCapture={(event) => {
                  const target = event.target as HTMLInputElement;
                  const panel = target.closest<HTMLElement>("[data-profile-panel]")?.dataset.profilePanel as AgentProfileTab | undefined;
                  if (panel) focusProfileField(panel, () => target);
                }}>
                  <fieldset disabled={saving || sourceSaving} className="ai-profile-edit-fields">
                    <div className="settings-form ai-profile-form">
                      <div className="agent-config-panel" data-profile-panel="settings" id={`${instanceId}-ai-profile-panel-settings`} role="tabpanel" aria-labelledby={`${instanceId}-ai-profile-tab-settings`} hidden={profileTab !== "settings"}>
                        <div className="ai-form-grid">
                          <label><span>{t("ai.profileName")}</span><input value={profileDraft.name} maxLength={200} onChange={(event) => {
                            const name = event.target.value;
                            setProfileDraft((current) => ({ ...current, name }));
                            setPublicIdentities((current) => current.map((identity) => (
                              identity.display_name.trim() === "" || identity.display_name === profileDraft.name
                                ? { ...identity, display_name: name }
                                : identity
                            )));
                          }} /></label>
                          <label><span>{t("ai.profileStatus")}</span><select value={profileDraft.status} onChange={(event) => setProfileDraft((current) => ({ ...current, status: event.target.value as AiProfileInput["status"] }))}><option value="draft">{t("ai.statusDraft")}</option><option value="active">{t("ai.statusActive")}</option><option value="disabled">{t("ai.statusDisabled")}</option></select></label>
                          <label className="agent-profile-description"><span>{t("ai.profileDescription")}</span><textarea rows={2} maxLength={1000} value={profileDraft.description ?? ""} onChange={(event) => setProfileDraft((current) => ({ ...current, description: event.target.value }))} /></label>
                          <label><span id={`${instanceId}-ai-profile-provider-label`}>{t("ai.provider")}</span><select ref={profileProviderRef} aria-labelledby={`${instanceId}-ai-profile-provider-label`} aria-required={profileDraft.status === "active"} aria-invalid={profileProviderError || undefined} aria-describedby={profileProviderError ? `${instanceId}-ai-profile-provider-error` : undefined} value={profileDraft.provider_connection_id ?? ""} onChange={(event) => setProfileDraft((current) => ({ ...current, provider_connection_id: event.target.value || null }))}><option value="">{t("ai.noProvider")}</option>{selectedProfile?.provider_connection_id && !providers.some((provider) => provider.id === selectedProfile.provider_connection_id && provider.status === "active") && <option value={selectedProfile.provider_connection_id}>{t("ai.assignedProvider")}</option>}{providers.filter((provider) => provider.status === "active" && isChatModelConnection(provider)).map((provider) => <option value={provider.id} key={provider.id}>{provider.name}</option>)}</select>{profileProviderError && <span id={`${instanceId}-ai-profile-provider-error`} className="field-error">{t("ai.profileProviderRequired")}</span>}{selectedProfile?.preset_key && !profileDraft.provider_connection_id && <small>{t("ai.presetConnectModel")}</small>}</label>
                          <label>
                            <span>{t("ai.modelOverride")}</span>
                            <input
                              aria-label={t("ai.modelOverride")}
                              value={profileDraft.model ?? ""}
                              maxLength={200}
                              placeholder={t("ai.defaultModelPlaceholder")}
                              autoCapitalize="none"
                              spellCheck={false}
                              onChange={(event) => setProfileDraft((current) => ({ ...current, model: event.target.value || null }))}
                            />
                          </label>
                          <label>
                            <span id={`${instanceId}-ai-reply-languages-label`}>{t("ai.language")}</span>
                            <input required aria-labelledby={`${instanceId}-ai-reply-languages-label`} aria-describedby={`${instanceId}-ai-reply-languages-help`} value={profileDraft.language} maxLength={255} placeholder="ru, en, de" autoCapitalize="none" spellCheck={false} onChange={(event) => {
                              const language = event.target.value;
                              setProfileDraft((current) => ({ ...current, language }));
                              setPublicIdentities((current) => reconcilePublicIdentities(
                                current,
                                language,
                                profileDraft.name.trim(),
                              ));
                            }} />
                            <small id={`${instanceId}-ai-reply-languages-help`}>{t("ai.languagesHelp")}</small>
                          </label>
                          <label><span>{t("ai.maxTokens")}</span><input type="number" min="64" max="32000" step="1" value={profileDraft.max_output_tokens} onChange={(event) => setProfileDraft((current) => ({ ...current, max_output_tokens: Number(event.target.value) }))} /></label>
                          <label>
                            <span id={`${instanceId}-ai-parallel-runs-label`}>{t("ai.maxConcurrentRuns")}</span>
                            <input required type="number" min="1" max="16" step="1"
                              aria-labelledby={`${instanceId}-ai-parallel-runs-label`} aria-describedby={`${instanceId}-ai-parallel-runs-help`}
                              value={profileDraft.max_concurrent_runs ?? 2}
                              onChange={(event) => setProfileDraft((current) => ({ ...current, max_concurrent_runs: Number(event.target.value) }))} />
                            <small id={`${instanceId}-ai-parallel-runs-help`}>{t("ai.maxConcurrentRunsHelp")}</small>
                          </label>

                        </div>

                        {profileDraft.voice && <AgentVoiceSettings locale={locale} providers={providers} value={profileDraft.voice} onChange={(voice) => setProfileDraft((current) => ({ ...current, voice }))} />}

                        <section className="agent-profile-usage" aria-label={t("ai.profileUsage")}>
                          <h3>{t("ai.profileUsage")}</h3>
                          <div><p><strong>{t("ai.profileUsageSolo")}</strong><span>{t("ai.profileUsageSoloHelp")}</span></p><p><strong>{t("ai.profileUsageTeam")}</strong><span>{t("ai.profileUsageTeamHelp")}</span></p></div>
                        </section>

                        <AnimatedDetails className="agent-profile-avatar-settings"><summary>{t("ai.internalAvatar")}</summary>
                        <AvatarUploadField
                          displayName={profileDraft.name}
                          avatarUrl={selectedProfile?.avatar_url ?? null}
                          label={t("ai.internalAvatar")}
                          file={profileAvatarFile}
                          removed={profileAvatarRemoved}
                          disabled={saving}
                          onFileChange={(file) => {
                            setProfileAvatarFile(file);
                            setProfileAvatarRemoved(false);
                          }}
                          onRemove={() => {
                            setProfileAvatarFile(null);
                            setProfileAvatarRemoved(true);
                          }}
                        />
                        </AnimatedDetails>
                      </div>
                      <div className="agent-config-panel" data-profile-panel="instructions" id={`${instanceId}-ai-profile-panel-instructions`} role="tabpanel" aria-labelledby={`${instanceId}-ai-profile-tab-instructions`} hidden={profileTab !== "instructions"}>
                        <label><span id={`${instanceId}-ai-instructions-label`}>{t("ai.instructions")}</span><textarea ref={profileInstructionsRef} aria-labelledby={`${instanceId}-ai-instructions-label`} aria-required={profileDraft.status === "active"} aria-invalid={profileInstructionsError || undefined} aria-describedby={profileInstructionsError ? `${instanceId}-ai-profile-instructions-error` : undefined} value={profileDraft.instructions} maxLength={50000} rows={9} placeholder={t("ai.instructionsPlaceholder")} onChange={(event) => setProfileDraft((current) => ({ ...current, instructions: event.target.value }))} />{profileInstructionsError && <span id={`${instanceId}-ai-profile-instructions-error`} className="field-error">{t("ai.profileInstructionsRequired")}</span>}</label>


                        <label>
                          <span id={`${instanceId}-ai-blacklist-reply-label`}>{t("ai.blacklistReplyText")}</span>
                          <textarea aria-labelledby={`${instanceId}-ai-blacklist-reply-label`} aria-describedby={`${instanceId}-ai-blacklist-reply-help`} value={profileDraft.blacklist_reply_text} maxLength={4000} rows={3} placeholder={t("ai.blacklistReplyPlaceholder")} onChange={(event) => setProfileDraft((current) => ({ ...current, blacklist_reply_text: event.target.value }))} />
                          <small id={`${instanceId}-ai-blacklist-reply-help`}>{t("ai.blacklistReplyHelp")}</small>
                        </label>
                        <fieldset className="ai-assignment" aria-label={t("ai.blacklistReplyText")}>
                          <label>
                            <input type="checkbox" aria-labelledby={`${instanceId}-ai-blacklist-language-label`} aria-describedby={`${instanceId}-ai-blacklist-language-help`} checked={profileDraft.blacklist_reply_match_language} onChange={(event) => setProfileDraft((current) => ({ ...current, blacklist_reply_match_language: event.target.checked }))} />
                            <span><strong id={`${instanceId}-ai-blacklist-language-label`}>{t("ai.blacklistReplyMatchLanguage")}</strong><small id={`${instanceId}-ai-blacklist-language-help`}>{t("ai.blacklistReplyMatchLanguageHelp")}</small></span>
                          </label>
                        </fieldset>

                        {selectedProfile && <AgentBackups
                          key={selectedProfile.id}
                          auth={auth}
                          profile={selectedProfile}
                          dirty={profileDraft.instructions !== selectedProfile.instructions
                            || (profileDraft.description ?? "") !== (selectedProfile.description ?? "")
                            || profileDraft.tool_instructions !== selectedProfile.tool_instructions
                            || profileDraft.blacklist_reply_text !== (selectedProfile.blacklist_reply_text ?? "")
                            || profileDraft.blacklist_reply_match_language !== (selectedProfile.blacklist_reply_match_language ?? true)
                            || JSON.stringify([...profileDraft.knowledge_base_ids].sort()) !== JSON.stringify([...selectedProfile.knowledge_base_ids].sort())}
                          busy={saving}
                          onBusyChange={setSaving}
                          onRestored={(restored) => {
                            setProfiles((current) => current.map((profile) => profile.id === restored.id ? restored : profile));
                            setProfileDraft((current) => ({
                              ...current,
                              instructions: restored.instructions,
                              description: restored.description ?? current.description,
                              tool_instructions: restored.tool_instructions,
                              blacklist_reply_text: restored.blacklist_reply_text ?? "",
                              blacklist_reply_match_language: restored.blacklist_reply_match_language ?? true,
                              knowledge_base_ids: restored.knowledge_base_ids,
                            }));
                            void listKnowledgeBases(auth).then(setKnowledgeBases).catch(() => setNotice({ kind: "error", message: "ai.loadError" }));
                          }}
                        />}

                      </div>
                      <div className="agent-config-panel" data-profile-panel="knowledge" id={`${instanceId}-ai-profile-panel-knowledge`} role="tabpanel" aria-labelledby={`${instanceId}-ai-profile-tab-knowledge`} hidden={profileTab !== "knowledge"}>
                        <fieldset className="ai-assignment"><legend>{t("ai.knowledgeBases")}</legend>
                          {knowledgeBases.filter((base) => base.status === "active").map((base) => <label key={base.id}><input type="checkbox" checked={profileDraft.knowledge_base_ids.includes(base.id)} onChange={() => toggleProfileList("knowledge_base_ids", base.id)} /><span><strong>{base.name}</strong><small>{t("ai.knowledgeSummary", { published: base.published_article_count })}</small></span></label>)}
                          {knowledgeBases.filter((base) => base.status === "active").length === 0 && <p>{t("ai.noKnowledgeBases")}</p>}
                        </fieldset>

                        {activeTab === "profiles" && profileTab === "knowledge" && <AgentSkillAssignments key={`skills:${selectedProfileId}`} auth={auth} profileId={selectedProfile?.id} disabled={saving || sourceSaving} onSaved={setSkillAssignmentChange} onBusyChange={setSkillAssignmentSaving} />}

                        <fieldset className="ai-assignment">
                          <legend>{t("ai.capabilities")}</legend>
                          <p>{t("ai.capabilitiesHelp")}</p>
                          <label>
                            <input type="checkbox" checked={profileDraft.capabilities.http_get} onChange={(event) => setProfileDraft((current) => ({
                              ...current,
                              capabilities: { ...current.capabilities, http_get: event.target.checked },
                            }))} />
                            <span><strong>{t("ai.capabilityHttpGet")}</strong><small>{t("ai.capabilityHttpGetHelp")}</small></span>
                          </label>
                          <label>
                            <input type="checkbox" checked={profileDraft.capabilities.http_post} onChange={(event) => setProfileDraft((current) => ({
                              ...current,
                              capabilities: { ...current.capabilities, http_post: event.target.checked },
                            }))} />
                            <span><strong>{t("ai.capabilityHttpPost")}</strong><small>{t("ai.capabilityHttpPostHelp")}</small></span>
                          </label>
                          <label>
                            <input type="checkbox" checked={profileDraft.capabilities.shell} onChange={(event) => setProfileDraft((current) => ({
                              ...current,
                              capabilities: { ...current.capabilities, shell: event.target.checked },
                            }))} />
                            <span><strong>{t("ai.capabilityShell")}</strong><small>{t("ai.capabilityShellHelp")}</small></span>
                          </label>
                        </fieldset>

                        <label>
                          <span>{t("ai.httpAccess")}</span>
                          <select value={profileDraft.http_allowed_hosts.includes("*") ? "public" : "domains"} onChange={(event) => setProfileDraft((current) => ({ ...current, http_allowed_hosts: event.target.value === "public" ? ["*"] : [] }))}>
                            <option value="domains">{t("ai.httpAccessDomains")}</option>
                            <option value="public">{t("ai.httpAccessPublic")}</option>
                          </select>
                          <small>{t("ai.httpAccessHelp")}</small>
                        </label>
                        {!profileDraft.http_allowed_hosts.includes("*") && <label>
                          <span>{t("ai.httpAllowedHosts")}</span>
                          <textarea rows={3} value={profileDraft.http_allowed_hosts.join("\n")} placeholder="api.example.com" onChange={(event) => setProfileDraft((current) => ({ ...current, http_allowed_hosts: event.target.value.split("\n") }))} />
                          <small>{t("ai.httpAllowedHostsHelp")}</small>
                        </label>}

                        <AgentProxy key={`proxy:${selectedProfileId}`} auth={auth} profileId={selectedProfile?.id} disabled={saving || sourceSaving} />

                        <label>
                          <span id={`${instanceId}-ai-tool-instructions-label`}>{t("ai.toolInstructions")}</span>
                          <textarea aria-labelledby={`${instanceId}-ai-tool-instructions-label`} aria-describedby={`${instanceId}-ai-tool-instructions-help`} value={profileDraft.tool_instructions} maxLength={50000} rows={6} onChange={(event) => setProfileDraft((current) => ({ ...current, tool_instructions: event.target.value }))} />
                          <small id={`${instanceId}-ai-tool-instructions-help`}>{t("ai.toolInstructionsHelp")}</small>
                        </label>

                        <fieldset className="ai-credentials">
                          <legend>{t("ai.variables")}</legend>
                          <header>
                            <div>
                              <p>{t("ai.variablesHelp")}</p>
                      </div>
                      <div className="ai-credential-actions">
                        <button className="secondary-button" type="button" disabled={customFields.length >= 32} onClick={addCustomField}>
                          <Plus size={15} />{t("ai.addParameter")}
                        </button>
                        <button className="secondary-button" type="button" disabled={secrets.length >= 32} onClick={addSecret}>
                          <Plus size={15} />{t("ai.addSecret")}
                        </button>
                      </div>
                    </header>
                    <div className="ai-credential-list">
                      {customFields.map((field) => (
                        <div className="ai-credential-row" key={field.id}>
                          <label>
                            <span>{t("ai.parameterKey")}</span>
                            <input required value={field.key} maxLength={80} pattern="[A-Za-z0-9_][A-Za-z0-9_.-]{0,79}" placeholder="DB_HOST" onChange={(event) => updateCustomField(field.id, "key", event.target.value)} />
                          </label>
                          <label>
                            <span>{t("ai.credentialValue")}</span>
                            <input value={field.value} maxLength={4096} placeholder="db.internal" onChange={(event) => updateCustomField(field.id, "value", event.target.value)} />
                          </label>
                          <button className="icon-button ai-credential-remove" type="button" aria-label={t("ai.removeCredential", { key: field.key || t("ai.parameter") })} onClick={() => setCustomFields((current) => current.filter((item) => item.id !== field.id))}>
                            <Trash2 size={15} />
                          </button>
                        </div>
                      ))}
                      {secrets.map((secret) => {
                        const preservesStoredValue = secret.configured && secret.key.trim() === secret.originalKey;
                        const showTelegramChatIdsHint = secret.key.trim().toUpperCase() === "TELEGRAM_NOTIFY_CHAT_IDS";
                        return (
                          <div className="ai-credential-row" key={secret.id}>
                            <label>
                              <span>{t("ai.secretKey")}</span>
                              <input required value={secret.key} maxLength={80} pattern="[A-Za-z0-9_][A-Za-z0-9_.-]{0,79}" placeholder="DB_PASSWORD" onChange={(event) => updateSecret(secret.id, "key", event.target.value)} />
                            </label>
                            <label>
                              <span>{t("ai.secretValue")}{preservesStoredValue && <small>{t("ai.secretConfigured")}</small>}</span>
                              <input type="password" required={!preservesStoredValue} value={secret.value} maxLength={8192} autoComplete="new-password" aria-label={t("ai.secretValue")} aria-describedby={showTelegramChatIdsHint ? `${secret.id}-hint` : undefined} placeholder={preservesStoredValue ? t("ai.secretKeepPlaceholder") : t("ai.secretValuePlaceholder")} onChange={(event) => updateSecret(secret.id, "value", event.target.value)} />
                            </label>
                            <button className="icon-button ai-credential-remove" type="button" aria-label={t("ai.removeCredential", { key: secret.key || t("ai.secret") })} onClick={() => setSecrets((current) => current.filter((item) => item.id !== secret.id))}>
                              <Trash2 size={15} />
                            </button>
                            {showTelegramChatIdsHint && <small className="ai-credential-hint" id={`${secret.id}-hint`}>{t("ai.telegramChatIdsHint")}</small>}
                          </div>
                        );
                      })}
                      {customFields.length === 0 && secrets.length === 0 && <p className="ai-credential-empty">{t("ai.noVariables")}</p>}
                    </div>
                  </fieldset>

                  <fieldset className="ai-assignment"><legend>{t("ai.telegramNotifications")}</legend>
                    <p>{t("ai.telegramNotificationsHelp")}</p>
                    {hasSelectedActiveWidgetChannel && <label><input type="checkbox" checked={profileDraft.telegram_notifications.new_visitor} onChange={(event) => setProfileDraft((current) => ({ ...current, telegram_notifications: { ...current.telegram_notifications, new_visitor: event.target.checked } }))} /><span><strong>{t("ai.telegramNotifyNewVisitor")}</strong><small>{t("ai.telegramNotifyNewVisitorHelp")}</small></span></label>}
                    <label><input type="checkbox" checked={profileDraft.telegram_notifications.new_message} onChange={(event) => setProfileDraft((current) => ({ ...current, telegram_notifications: { ...current.telegram_notifications, new_message: event.target.checked } }))} /><span><strong>{t("ai.telegramNotifyNewMessage")}</strong><small>{t("ai.telegramNotifyNewMessageHelp")}</small></span></label>
                    <label><input type="checkbox" checked={profileDraft.telegram_notifications.operator_request} onChange={(event) => setProfileDraft((current) => ({ ...current, telegram_notifications: { ...current.telegram_notifications, operator_request: event.target.checked } }))} /><span><strong>{t("ai.telegramNotifyOperatorRequest")}</strong><small>{t("ai.telegramNotifyOperatorRequestHelp")}</small></span></label>
                  </fieldset>

                  <fieldset className="ai-assignment"><legend>{t("ai.incomingConversations")}</legend>
                    <label><input type="checkbox" checked={profileDraft.auto_join_new_conversations} onChange={(event) => setProfileDraft((current) => ({ ...current, auto_join_new_conversations: event.target.checked }))} /><span><strong>{t("ai.autoJoinNewConversations")}</strong><small>{t("ai.autoJoinNewConversationsHelp")}</small></span></label>
                    <label><input type="checkbox" checked={profileDraft.can_resolve_conversations} onChange={(event) => setProfileDraft((current) => ({ ...current, can_resolve_conversations: event.target.checked }))} /><span><strong>{t("ai.canResolveConversations")}</strong><small>{t("ai.canResolveConversationsHelp")}</small></span></label>
                  </fieldset>

                  <fieldset className="ai-assignment"><legend>{t("ai.channelAssignments")}</legend>
                    <p>{t("ai.channelAssignmentsHelp")}</p>
                    {channels.map((channel) => <label key={channel.id}><input type="checkbox" checked={profileDraft.channel_ids.includes(channel.id)} onChange={() => toggleProfileList("channel_ids", channel.id)} /><span><strong>{channel.name}</strong><small>{channelKindLabel(t, channel.kind)} · {channelStatusLabel(t, channel.status)} · {inboxes.find((inbox) => inbox.id === channel.inbox_id)?.name ?? t("ai.unknownInbox")}</small></span></label>)}
                    {channels.length === 0 && <p>{t("ai.noChannels")}</p>}
                  </fieldset>

                  {selectedProfile && canManageApi && <AgentApi key={selectedProfile.id} auth={auth} profileId={selectedProfile.id}
                    profileDirty={JSON.stringify(profileDraft) !== JSON.stringify(profileInput(selectedProfile))
                      || JSON.stringify(customFields.map(({ key, value }) => ({ key, value }))) !== JSON.stringify(selectedProfile.custom_fields)
                      || secrets.length !== selectedProfile.secrets.length
                      || secrets.some((secret) => Boolean(secret.value) || secret.key !== secret.originalKey)} />}

                  {hasSelectedActiveWidgetChannel && <section className="ai-public-identities" aria-labelledby={`${instanceId}-ai-public-identities-title`}>
                    <header>
                      <h3 id={`${instanceId}-ai-public-identities-title`}>{t("ai.publicIdentities")}</h3>
                      <p>{t("ai.publicIdentitiesHelp")}</p>
                    </header>
                    <div className="ai-public-identity-list">
                      {publicIdentities.map((identity) => (
                        <article className="ai-public-identity" key={identity.language.toLowerCase()}>
                          <div className="ai-public-identity-language">{identity.language}</div>
                          <label>
                            <span>{t("ai.publicDisplayName", { language: identity.language })}</span>
                            <input
                              required
                              value={identity.display_name}
                              maxLength={200}
                              onChange={(event) => setPublicIdentities((current) => current.map((item) => (
                                item.language.toLowerCase() === identity.language.toLowerCase()
                                  ? { ...item, display_name: event.target.value }
                                  : item
                              )))}
                            />
                          </label>
                          <AvatarUploadField
                            displayName={identity.display_name}
                            avatarUrl={identity.avatar_url}
                            label={t("ai.publicAvatar", { language: identity.language })}
                            file={identity.file}
                            removed={identity.removed}
                            disabled={saving}
                            onFileChange={(file) => setPublicIdentities((current) => current.map((item) => (
                              item.language.toLowerCase() === identity.language.toLowerCase()
                                ? { ...item, file, removed: false }
                                : item
                            )))}
                            onRemove={() => setPublicIdentities((current) => current.map((item) => (
                              item.language.toLowerCase() === identity.language.toLowerCase()
                                ? { ...item, file: null, removed: true }
                                : item
                            )))}
                          />
                        </article>
                      ))}
                    </div>
                  </section>}

                      </div>
                  <div className="ai-actions"><DemoActionButton className="primary-button" type="submit" disabled={saving || !profileDraft.name.trim()}><Save size={16} />{saving ? t("ai.saving") : t("ai.saveProfile")}</DemoActionButton>{selectedProfile && <DemoActionButton className="secondary-button table-danger-link" type="button" disabled={saving} onClick={() => void removeProfile()}><Trash2 size={16} />{t("ai.deleteProfile")}</DemoActionButton>}</div>
                    </div>
                  </fieldset>
                </form>
                <div id={`${instanceId}-ai-profile-panel-tests`} role="tabpanel" aria-labelledby={`${instanceId}-ai-profile-tab-tests`} hidden={profileTab !== "tests"}>
                  {selectedProfile && <div className="settings-form ai-profile-form">
                    <fieldset disabled={saving || sourceSaving} className="ai-profile-edit-fields">
                      <AgentTests key={selectedProfile.id} auth={auth} profile={selectedProfile} channels={channels} disabledReason={sourceSaving ? "Сохраняется инструкция. Дождитесь завершения." : saving ? "Сохраняются настройки агента. Дождитесь завершения." : undefined} onSourceBusyChange={setInstructionSourceSaving} onProfileSaved={(saved, field) => {
                        setProfiles((current) => current.map((item) => item.id === saved.id ? { ...item, [field]: saved[field] } : item));
                        setProfileDraft((current) => current[field] === selectedProfile[field] ? { ...current, [field]: saved[field] } : current);
                      }} />
                    </fieldset>
                  </div>}
                </div>
              </>
            ) : <div className="empty-state">{t("ai.selectProfile")}</div>}
          </div>
        </div>
      ) : activeTab === "tasks" ? (
        <div className="settings-layout ai-settings-layout" id={`${instanceId}-ai-settings-panel-tasks`} role="region" aria-label={t("ai.tasks")}>
          <aside className="settings-list" aria-label={t("ai.taskList")}>
            {tasks.map((task) => (
              <button
                className={`settings-row settings-row--task ${selectedTaskId === task.id ? "settings-row--active" : ""}`}
                type="button"
                key={task.id}
                onClick={() => selectTask(task)}
              >
                <strong title={task.text}>{task.text}</strong>
                <span>{taskAgentsLabel(task.agent_ids)}</span>
                <span>{taskScheduleLabel(task.schedule)}</span>
                <span>{task.next_occurrence_at
                  ? t("ai.taskNextOccurrence", {
                      date: formatDate(task.next_occurrence_at, {
                        month: "short",
                        day: "numeric",
                        hour: "2-digit",
                        minute: "2-digit",
                      }),
                    })
                  : t("ai.taskNoNextOccurrence")}</span>
              </button>
            ))}
            {tasksLoading && <div className="empty-state">{t("ai.taskLoading")}</div>}
            {!tasksLoading && tasks.length === 0 && <div className="empty-state"><CalendarClock size={20} />{t("ai.taskEmpty")}</div>}
          </aside>

          <div className="settings-panel ai-editor-panel">
            {selectedTaskId ? (
              <>
                <form onSubmit={saveTask}>
                <header>
                  <h2>{selectedTaskId === "new" ? t("ai.newTaskTitle") : t("ai.editTaskTitle")}</h2>
                  {selectedTask && <span>{selectedTask.next_occurrence_at
                    ? t("ai.taskNextOccurrence", {
                        date: formatDate(selectedTask.next_occurrence_at, {
                          year: "numeric",
                          month: "short",
                          day: "numeric",
                          hour: "2-digit",
                          minute: "2-digit",
                        }),
                      })
                    : t("ai.taskNoNextOccurrence")}</span>}
                </header>
                <div className="settings-form ai-task-form">
                  <label>
                    <span>{t("ai.taskText")}</span>
                    <textarea
                      required
                      rows={7}
                      maxLength={50_000}
                      value={taskDraft.text}
                      placeholder={t("ai.taskTextPlaceholder")}
                      onChange={(event) => setTaskDraft((current) => ({
                        ...current,
                        text: event.target.value,
                      }))}
                    />
                  </label>

                  <fieldset className="ai-assignment ai-task-agents">
                    <legend>{t("ai.taskAgents")}</legend>
                    <p className="ai-task-agents-help">{t("ai.taskAgentsHelp", {
                      count: taskDraft.agent_ids.length,
                      max: maximumTaskAgents,
                    })}</p>
                    {taskAgentOptions.map((profile) => (
                      <label key={profile.id}>
                        <input
                          type="checkbox"
                          checked={taskDraft.agent_ids.includes(profile.id)}
                          disabled={!taskDraft.agent_ids.includes(profile.id) && taskDraft.agent_ids.length >= maximumTaskAgents}
                          onChange={() => toggleTaskAgent(profile.id)}
                        />
                        <span>
                          <strong>{profile.name}</strong>
                          <small>{taskAgentAvailabilityLabel(profile)}</small>
                        </span>
                      </label>
                    ))}
                    {taskAgentOptions.length === 0 && <p>{t("ai.taskNoAgents")}</p>}
                    {taskAgentOptions.length > 0 && taskDraft.agent_ids.length === 0 && (
                      <p className="field-error">{t("ai.taskAgentsRequired")}</p>
                    )}
                  </fieldset>

                  <div className="ai-form-grid">
                    <label>
                      <span>{t("ai.taskType")}</span>
                      <select
                        value={taskDraft.schedule.kind === "once" ? "once" : "recurring"}
                        onChange={(event) => setTaskType(event.target.value as "once" | "recurring")}
                      >
                        <option value="once">{t("ai.taskTypeOnce")}</option>
                        <option value="recurring">{t("ai.taskTypeRecurring")}</option>
                      </select>
                    </label>

                    {taskDraft.schedule.kind === "once" ? (
                      <div className="ai-form-field">
                        <span id={`${instanceId}-ai-task-run-at-label`}>{t("ai.taskRunAt")}</span>
                        <DateTimeField
                          required
                          mode="datetime"
                          locale={locale}
                          aria-labelledby={`${instanceId}-ai-task-run-at-label`}
                          min={minimumRunAt}
                          value={datetimeLocalValue(taskDraft.schedule.run_at)}
                          onChange={(value) => setTaskDraft((current) => ({
                            ...current,
                            schedule: {
                              kind: "once",
                              run_at: datetimeLocalIso(value),
                            },
                          }))}
                        />
                      </div>
                    ) : (
                      <>
                        <label>
                          <span>{t("ai.taskFrequency")}</span>
                          <select
                            value={taskDraft.schedule.kind}
                            onChange={(event) => setTaskRecurrence(
                              event.target.value as Exclude<AiTaskSchedule["kind"], "once">,
                            )}
                          >
                            <option value="daily">{t("ai.taskFrequencyDaily")}</option>
                            <option value="weekly">{t("ai.taskFrequencyWeekly")}</option>
                            <option value="monthly">{t("ai.taskFrequencyMonthly")}</option>
                            <option value="yearly">{t("ai.taskFrequencyYearly")}</option>
                          </select>
                        </label>
                        <div className="ai-form-field">
                          <span id={`${instanceId}-ai-task-time-label`}>{t("ai.taskTime")}</span>
                          <DateTimeField
                            required
                            mode="time"
                            locale={locale}
                            aria-labelledby={`${instanceId}-ai-task-time-label`}
                            value={"time" in taskDraft.schedule ? taskDraft.schedule.time : ""}
                            onChange={(value) => setTaskDraft((current) => current.schedule.kind === "once"
                              ? current
                              : {
                                  ...current,
                                  schedule: { ...current.schedule, time: value },
                                })}
                          />
                        </div>
                      </>
                    )}
                  </div>

                  <p className="ai-task-timezone">
                    {t("ai.taskTimezone", {
                      timezone: "timezone" in taskDraft.schedule
                        ? taskDraft.schedule.timezone
                        : browserTimezone(),
                    })}
                  </p>

                  {taskDraft.schedule.kind === "weekly" && (
                    <fieldset className="ai-task-options">
                      <legend>{t("ai.taskWeekdays")}</legend>
                      <div className="ai-task-choice-grid ai-task-choice-grid--week">
                        {weekdayOptions.map((option) => (
                          <label key={option.value}>
                            <input
                              type="checkbox"
                              checked={taskDraft.schedule.kind === "weekly" && taskDraft.schedule.weekdays.includes(option.value)}
                              onChange={() => toggleTaskScheduleValue("weekdays", option.value)}
                            />
                            <span>{t(option.label)}</span>
                          </label>
                        ))}
                      </div>
                    </fieldset>
                  )}

                  {taskDraft.schedule.kind === "monthly" && (
                    <fieldset className="ai-task-options">
                      <legend>{t("ai.taskMonthDays")}</legend>
                      <div className="ai-task-choice-grid ai-task-choice-grid--month">
                        {monthDays.map((day) => (
                          <label key={day}>
                            <input
                              type="checkbox"
                              checked={taskDraft.schedule.kind === "monthly" && taskDraft.schedule.month_days.includes(day)}
                              onChange={() => toggleTaskScheduleValue("month_days", day)}
                            />
                            <span>{day}</span>
                          </label>
                        ))}
                      </div>
                    </fieldset>
                  )}

                  {taskDraft.schedule.kind === "yearly" && (
                    <fieldset className="ai-task-options ai-task-annual-options">
                      <legend>{t("ai.taskAnnualDates")}</legend>
                      <div className="ai-task-annual-list">
                        {taskDraft.schedule.dates.map((date, index) => (
                          <div className="ai-task-annual-row" key={index}>
                            <select
                              aria-label={t("ai.taskAnnualMonth", { number: index + 1 })}
                              value={date.month}
                              onChange={(event) => updateTaskAnnualDate(index, "month", Number(event.target.value))}
                            >
                              {monthDays.slice(0, 12).map((month) => (
                                <option value={month} key={month}>
                                  {formatDate(
                                    new Date(Date.UTC(2000, month - 1, 1)),
                                    { month: "long", timeZone: "UTC" },
                                  )}
                                </option>
                              ))}
                            </select>
                            <input
                              aria-label={t("ai.taskAnnualDay", { number: index + 1 })}
                              type="number"
                              min="1"
                              max="31"
                              step="1"
                              value={date.day}
                              onChange={(event) => updateTaskAnnualDate(index, "day", Number(event.target.value))}
                            />
                            <DemoActionButton
                              className="icon-button ai-task-annual-remove"
                              type="button"
                              aria-label={t("ai.taskRemoveAnnualDate", { number: index + 1 })}
                              onClick={() => removeTaskAnnualDate(index)}
                            >
                              <Trash2 size={15} />
                            </DemoActionButton>
                          </div>
                        ))}
                      </div>
                      {!annualDatesAreValid(taskDraft.schedule.dates) && (
                        <p className="field-error">{t("ai.taskAnnualDatesError")}</p>
                      )}
                      <button className="secondary-button ai-task-add-date" type="button" onClick={addTaskAnnualDate}>
                        <Plus size={15} />{t("ai.taskAddAnnualDate")}
                      </button>
                    </fieldset>
                  )}

                  <div className="ai-actions">
                    <DemoActionButton className="primary-button" type="submit" disabled={saving || !taskDraftIsValid(taskDraft) || !taskAssignedAgentsAreAvailable}>
                      <Save size={16} />{saving ? t("ai.saving") : t("ai.saveTask")}
                    </DemoActionButton>
                    {selectedTask && (
                      <DemoActionButton className="secondary-button table-danger-link" type="button" disabled={saving} onClick={() => void removeTask()}>
                        <Trash2 size={16} />{t("ai.deleteTask")}
                      </DemoActionButton>
                    )}
                  </div>
                </div>
              </form>
                {selectedTask && (
                  <section className="ai-task-runs" aria-label={t("ai.taskRuns")}>
                    <header>
                      <h3>{t("ai.taskRuns")}</h3>
                      <button
                        className="secondary-button"
                        type="button"
                        disabled={taskRunsState === "loading"}
                        onClick={() => void reloadTaskRuns(selectedTask.id)}
                      >
                        <RefreshCw size={15} />{t("ai.taskRunsRefresh")}
                      </button>
                    </header>

                    {taskRunsState === "loading" && (
                      <div className="ai-task-runs-state" role="status">{t("ai.taskRunsLoading")}</div>
                    )}
                    {taskRunsState === "error" && (
                      <div className="ai-task-runs-state ai-task-runs-error" role="alert">
                        <span>{t("ai.taskRunsError")}</span>
                        <button className="secondary-button" type="button" onClick={() => void reloadTaskRuns(selectedTask.id)}>
                          <RefreshCw size={15} />{t("ai.taskRunsRetry")}
                        </button>
                      </div>
                    )}
                    {taskRunsState === "ready" && taskRuns.length === 0 && (
                      <div className="ai-task-runs-state">{t("ai.taskRunsEmpty")}</div>
                    )}
                    {taskRunsState === "ready" && taskRuns.length > 0 && (
                      <div className="ai-task-run-list">
                        {taskRuns.map((run) => (
                          <article className="ai-task-run" key={run.id}>
                            <header>
                              <strong>{run.agent_name}</strong>
                              <span className={`ai-task-run-status ai-task-run-status--${run.status}`}>
                                {t(taskRunStatusKeys[run.status])}
                              </span>
                            </header>
                            <dl>
                              <div><dt>{t("ai.taskRunScheduledFor")}</dt><dd>{taskRunDate(run.scheduled_for)}</dd></div>
                              <div><dt>{t("ai.taskRunStartedAt")}</dt><dd>{taskRunDate(run.started_at)}</dd></div>
                              <div><dt>{t("ai.taskRunCompletedAt")}</dt><dd>{taskRunDate(run.completed_at)}</dd></div>
                              <div><dt>{t("ai.taskRunAttempts")}</dt><dd>{run.attempts}</dd></div>
                            </dl>
                            <AnimatedDetails className="ai-task-run-text">
                              <summary>{t("ai.taskRunTaskText")}</summary>
                              <pre>{run.task_text}</pre>
                            </AnimatedDetails>
                            {run.output !== null && (
                              <div className="ai-task-run-result">
                                <strong>{t("ai.taskRunOutput")}</strong>
                                <pre>{run.output}</pre>
                              </div>
                            )}
                            {run.error !== null && (
                              <div className="ai-task-run-result ai-task-run-result--error">
                                <strong>{t("ai.taskRunError")}</strong>
                                <pre>{run.error}</pre>
                              </div>
                            )}
                          </article>
                        ))}
                      </div>
                    )}
                  </section>
                )}
              </>
            ) : <div className="empty-state">{t("ai.selectTask")}</div>}
          </div>
        </div>
      ) : activeTab === "providers" ? (
        <div className="settings-layout ai-settings-layout" id={`${instanceId}-ai-settings-panel-providers`} role="tabpanel" aria-labelledby={`${instanceId}-ai-settings-tab-providers`}>
          <aside className="settings-list agent-profile-list" aria-label={t("ai.providerList")}>
            <label className="agent-profile-search">
              <Search size={17} aria-hidden="true" />
              <input type="search" aria-label={t("ai.providerSearch")} placeholder={t("ai.providerSearch")} value={providerSearch} onChange={(event) => setProviderSearch(event.target.value)} />
            </label>
            <section className="agent-profile-group" aria-label={t("ai.providerList")}>
              <h2>{t("ai.providerList")}</h2>
              {matchingProviders.map((provider) => (
                <button className={`agent-profile-row${selectedProviderId === provider.id ? " agent-profile-row--active" : ""}`} type="button" key={provider.id} aria-pressed={selectedProviderId === provider.id} disabled={saving || sourceSaving} onClick={() => selectProvider(provider)}>
                  <span className="agent-preset-mark" aria-hidden="true"><KeyRound size={22} /></span>
                  <span className="agent-profile-row-copy">
                    <strong>{provider.name}</strong>
                    <span className="agent-profile-row-description">{t(`ai.providerKind.${provider.provider_kind}` as MessageKey)} · {provider.default_model}</span>
                    <span className={`agent-profile-row-status agent-profile-row-status--${provider.status}`}>{t(provider.status === "active" ? "ai.statusActive" : "ai.statusDisabled")} · {t(provider.api_key_configured ? "ai.keyConfigured" : "ai.keyMissing")}</span>
                  </span>
                  <ChevronRight size={16} aria-hidden="true" />
                </button>
              ))}
              {loading && <div className="empty-state" role="status">{t("ai.loading")}</div>}
              {!loading && matchingProviders.length === 0 && <p className="agent-profile-list-empty" role="status">{t(providerQuery ? "ai.providerSearchEmpty" : "ai.providerEmpty")}</p>}
              <button className="agent-profile-create" type="button" disabled={saving || sourceSaving} onClick={startProvider}><Plus size={16} />{t("ai.addProvider")}</button>
            </section>
          </aside>

          <div ref={providerMotion} className="settings-panel ai-editor-panel">
            {selectedProviderId ? (
              <>
                <header className="agent-profile-heading">
                  <span className="agent-preset-mark" aria-hidden="true"><KeyRound size={28} /></span>
                  <div><h2>{selectedProviderId === "new" ? t("ai.newProviderTitle") : providerDraft.name}</h2><p>{t(`ai.providerKind.${providerDraft.provider_kind}` as MessageKey)} · {t(selectedProvider?.api_key_configured ? "ai.keyConfigured" : "ai.keyMissing")}</p></div>
                </header>
                <form className="ai-provider-config-form" onSubmit={saveProvider}>
                  <div className="settings-form ai-provider-form">
                    <div className="ai-form-grid">
                      <label><span>{t("ai.connectionName")}</span><input value={providerDraft.name} maxLength={200} onChange={(event) => setProviderDraft((current) => ({ ...current, name: event.target.value }))} /></label>
                      
                      <label><span id={`${instanceId}-ai-provider-kind-label`}>{t("ai.providerKind")}</span><select value={providerDraft.provider_kind} aria-labelledby={`${instanceId}-ai-provider-kind-label`} onChange={(event) => setProviderDraft((current) => ({ ...current, provider_kind: event.target.value as AiProviderInput["provider_kind"] }))}><option value="openai">{t("ai.providerKind.openai")}</option>{isChatModelConnection(providerDraft) && <option value="anthropic">{t("ai.providerKind.anthropic")}</option>}<option value="openai_compatible">{t("ai.providerKind.openai_compatible")}</option></select></label>
                      <label className="ai-wide-field"><span>{t("ai.baseUrl")}</span><input type="url" value={providerDraft.base_url} maxLength={2000} placeholder="https://api.example/v1" autoCapitalize="none" spellCheck={false} onChange={(event) => setProviderDraft((current) => ({ ...current, base_url: event.target.value }))} /></label>
                      <label>
                        <span>{t("ai.defaultModel")}</span>
                        <input
                          required
                          aria-label={t("ai.defaultModel")}
                          value={providerDraft.default_model}
                          maxLength={200}
                          autoCapitalize="none"
                          spellCheck={false}
                          onChange={(event) => setProviderDraft((current) => ({ ...current, default_model: event.target.value }))}
                        />
                      </label>
                      <label><span>{t("ai.providerStatus")}</span><select value={providerDraft.status} onChange={(event) => setProviderDraft((current) => ({ ...current, status: event.target.value as AiProviderInput["status"] }))}><option value="active">{t("ai.statusActive")}</option><option value="disabled">{t("ai.statusDisabled")}</option></select></label>
                      <label className="ai-wide-field"><span>{t("ai.apiKey")}</span><input type="password" value={providerApiKey} maxLength={4096} autoComplete="new-password" placeholder={selectedProvider?.api_key_configured ? t("ai.apiKeyKeepPlaceholder") : "sk-…"} disabled={clearProviderApiKey} onChange={(event) => setProviderApiKey(event.target.value)} /><small>{t("ai.apiKeyHelp")}</small></label>
                    </div>
                    {selectedProvider?.api_key_configured && <label className="ai-clear-secret"><input type="checkbox" checked={clearProviderApiKey} onChange={(event) => { setClearProviderApiKey(event.target.checked); if (event.target.checked) setProviderApiKey(""); }} /><span>{t("ai.clearApiKey")}</span></label>}
                    <div className="ai-actions"><DemoActionButton className="primary-button" type="submit" disabled={saving || !providerDraft.name.trim() || !providerDraft.base_url.trim() || !providerTargetIsValid}><Save size={16} />{saving ? t("ai.saving") : t("ai.saveProvider")}</DemoActionButton>{selectedProvider && <DemoActionButton className="secondary-button table-danger-link" type="button" disabled={saving} onClick={() => void removeProvider()}><Trash2 size={16} />{t("ai.deleteProvider")}</DemoActionButton>}</div>
                  </div>
                </form>
              </>
            ) : <div className="empty-state">{t("ai.selectProvider")}</div>}
          </div>
        </div>
      ) : null}
    </section>
  );
}

export function AISettingsView(props: Props) {
  return <AIWorkspaceView {...props} view="settings" />;
}

export function AITasksView(props: Props) {
  return <AITasksWorkspace {...props} />;
}
