import { useContentMotion } from "./useContentMotion";
import { DemoActionButton } from "./DemoReadOnly";
import {
  useCallback,
  useEffect, useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  ArrowDown,
  ArrowUp,
  BellRing,
  Bot,
  Clock3,
  Languages,
  Plus,
  Radio,
  RefreshCw,
  Route,
  Save,
  ShieldAlert,
  Trash2,
  Users,
  type LucideIcon,
} from "lucide-react";
import {
  ApiRequestError,
  getInboxRouting,
  updateInboxRouting,
  type Inbox as InboxRecord,
  type InboxRoutingChannelOption,
  type InboxRoutingConfiguration,
  type InboxRoutingQueue,
  type InboxRoutingResponse,
  type InboxRoutingRule,
  type InboxRoutingWeeklyInterval,
  type InboxRoutingWrite,
  type OperatorAuth,
  type TelegramBotTokenUpdate,
} from "../api";
import {
  channelKindLabel,
  channelStatusLabel,
  useI18n,
  type MessageKey,
} from "../i18n";
import { useCanonicalPageRoute, usePageRoute } from "./page-route";
import { DateTimeField } from "./DateTimeField";
import "./InboxRoutingView.css";

interface InboxRoutingViewProps {
  auth: OperatorAuth;
  inboxes: InboxRecord[];
  onDirtyChange?: (dirty: boolean) => void;
}

type InspectorSelection =
  | { kind: "channel"; id: string }
  | { kind: "rule"; id: string }
  | { kind: "queue"; id: string }
  | { kind: "default" }
  | { kind: "coverage" }
  | { kind: "sla" }
  | { kind: "telegram" };

interface WorkflowConnection {
  source: string;
  target: string;
  dashed?: boolean;
}

interface Notice {
  kind: "success" | "error" | "conflict";
  message: MessageKey;
}

const weekdayKeys: Record<number, MessageKey> = {
  1: "routing.weekday.monday",
  2: "routing.weekday.tuesday",
  3: "routing.weekday.wednesday",
  4: "routing.weekday.thursday",
  5: "routing.weekday.friday",
  6: "routing.weekday.saturday",
  7: "routing.weekday.sunday",
};

function generatedId(): string {
  return globalThis.crypto.randomUUID();
}

/** Inbox & routing URLs: `/inbox-routing` edits the first inbox and `/inbox-routing/<inbox>` another one. */
function inboxRoutingSegments(inboxes: readonly InboxRecord[], inboxId: string | null): string[] {
  return inboxId && inboxId !== inboxes[0]?.id ? [inboxId] : [];
}

function selectionKey(selection: InspectorSelection | null): string | null {
  if (!selection) return null;
  if (selection.kind === "channel" || selection.kind === "rule" || selection.kind === "queue") {
    return `${selection.kind}:${selection.id}`;
  }
  return selection.kind;
}

function normalizeLanguage(value: string): string | null {
  const normalized = value.trim().replaceAll("_", "-").toLowerCase();
  return normalized || null;
}

function languageIsValid(value: string | null): boolean {
  return value === null || /^[a-z]{2,8}(?:-[a-z0-9]{1,8})*$/.test(value);
}

function botTokenIsValid(value: string): boolean {
  return /^\d+:[a-z0-9_-]{20,256}$/i.test(value) && value.length <= 256;
}

function configurationWrite(configuration: InboxRoutingConfiguration): InboxRoutingWrite {
  return {
    enabled: configuration.enabled,
    assignment_strategy: configuration.assignment_strategy,
    default_queue_id: configuration.default_queue_id,
    queues: configuration.queues.map((queue) => ({ ...queue, member_ids: [...queue.member_ids] })),
    rules: configuration.rules
      .map((rule) => ({ ...rule }))
      .sort((left, right) => left.position - right.position),
    coverage: {
      ...configuration.coverage,
      weekly_intervals: configuration.coverage.weekly_intervals.map((interval) => ({ ...interval })),
    },
    sla: { ...configuration.sla },
    telegram: {
      new_visitor: configuration.telegram.new_visitor,
      new_message: configuration.telegram.new_message,
      operator_request: configuration.telegram.operator_request,
      unassigned_warning: configuration.telegram.unassigned_warning,
      sla_breach: configuration.telegram.sla_breach,
      chat_ids: [...configuration.telegram.chat_ids],
    },
  };
}

function normalizedWrite(draft: InboxRoutingWrite): InboxRoutingWrite {
  return {
    ...draft,
    queues: draft.queues.map((queue) => ({
      ...queue,
      name: queue.name.trim(),
      member_ids: [...new Set(queue.member_ids)],
    })),
    rules: draft.rules.map((rule, index) => ({
      ...rule,
      position: index + 1,
      language: rule.language ? normalizeLanguage(rule.language) : null,
    })),
    coverage: {
      ...draft.coverage,
      timezone: draft.coverage.timezone.trim(),
      weekly_intervals: draft.coverage.weekly_intervals
        .map((interval) => ({ ...interval }))
        .sort((left, right) => (
          left.weekday - right.weekday || left.starts_at.localeCompare(right.starts_at)
        )),
    },
    telegram: {
      ...draft.telegram,
      chat_ids: [...new Set(draft.telegram.chat_ids.map((chatId) => chatId.trim()).filter(Boolean))],
    },
  };
}

function draftSignature(
  draft: InboxRoutingWrite,
  tokenAction: TelegramBotTokenUpdate["action"],
): string {
  return JSON.stringify({
    draft,
    tokenAction,
  });
}

function telegramIsEnabled(draft: InboxRoutingWrite): boolean {
  return draft.telegram.new_visitor
    || draft.telegram.new_message
    || draft.telegram.operator_request
    || draft.telegram.unassigned_warning
    || draft.telegram.sla_breach;
}

function timezoneIsValid(value: string): boolean {
  try {
    new Intl.DateTimeFormat("en", { timeZone: value }).format();
    return true;
  } catch {
    return false;
  }
}

function routingValidation(
  draft: InboxRoutingWrite,
  tokenAction: TelegramBotTokenUpdate["action"],
  tokenValue: string,
  storedTokenConfigured: boolean,
): MessageKey | null {
  if (draft.queues.length > 32 || draft.rules.length > 64) return "routing.validation.itemLimit";
  if (draft.queues.some((queue) => !queue.name.trim())) return "routing.validation.queueName";
  const queueNames = draft.queues.map((queue) => queue.name.trim().toLocaleLowerCase());
  if (new Set(queueNames).size !== queueNames.length) return "routing.validation.queueNameDuplicate";
  const activeQueueIds = new Set(
    draft.queues.filter((queue) => queue.status === "active").map((queue) => queue.id),
  );
  const defaultQueueRequired = draft.assignment_strategy === "least_active"
    || draft.coverage.outside_hours_action === "queue"
    || draft.coverage.no_operator_action === "queue";
  const anyQueueRequired = defaultQueueRequired
    || draft.rules.length > 0
    || draft.sla.escalation_queue_id !== null;
  if (anyQueueRequired && draft.queues.length === 0) {
    return "routing.validation.queueRequired";
  }
  if (
    (defaultQueueRequired && !draft.default_queue_id)
    || (draft.default_queue_id !== null && !activeQueueIds.has(draft.default_queue_id))
  ) {
    return "routing.validation.defaultQueue";
  }
  if (draft.rules.some((rule) => !draft.queues.some((queue) => queue.id === rule.queue_id))) {
    return "routing.validation.ruleQueue";
  }
  if (draft.rules.some((rule) => rule.enabled && !activeQueueIds.has(rule.queue_id))) {
    return "routing.validation.ruleQueueActive";
  }
  if (draft.rules.some((rule) => !rule.channel_id && !normalizeLanguage(rule.language ?? ""))) {
    return "routing.validation.ruleCondition";
  }
  if (draft.rules.some((rule) => !languageIsValid(normalizeLanguage(rule.language ?? "")))) {
    return "routing.validation.language";
  }
  const conditionKeys = draft.rules.map((rule) => (
    `${rule.channel_id ?? "*"}:${normalizeLanguage(rule.language ?? "") ?? "*"}`
  ));
  if (new Set(conditionKeys).size !== conditionKeys.length) {
    return "routing.validation.duplicateRule";
  }
  if (!draft.coverage.timezone || !timezoneIsValid(draft.coverage.timezone)) {
    return "routing.validation.timezone";
  }
  const byWeekday = new Map<number, InboxRoutingWeeklyInterval[]>();
  if (draft.coverage.weekly_intervals.length > 28) return "routing.validation.intervalLimit";
  for (const interval of draft.coverage.weekly_intervals) {
    if (
      !weekdayKeys[interval.weekday]
      || !/^([01]\d|2[0-3]):[0-5]\d$/.test(interval.starts_at)
      || !/^([01]\d|2[0-3]):[0-5]\d$/.test(interval.ends_at)
      || interval.starts_at >= interval.ends_at
    ) {
      return "routing.validation.interval";
    }
    const intervals = byWeekday.get(interval.weekday) ?? [];
    intervals.push(interval);
    byWeekday.set(interval.weekday, intervals);
  }
  for (const intervals of byWeekday.values()) {
    intervals.sort((left, right) => left.starts_at.localeCompare(right.starts_at));
    if (intervals.some((interval, index) => (
      index > 0 && intervals[index - 1].ends_at > interval.starts_at
    ))) {
      return "routing.validation.overlappingIntervals";
    }
  }
  const warning = draft.sla.unassigned_warning_seconds;
  const firstResponse = draft.sla.first_response_seconds;
  const resolution = draft.sla.resolution_seconds;
  if (warning !== null && (warning < 60 || warning > 604_800)) {
    return "routing.validation.slaRange";
  }
  if (firstResponse !== null && (firstResponse < 60 || firstResponse > 604_800)) {
    return "routing.validation.slaRange";
  }
  if (resolution !== null && (resolution < 60 || resolution > 2_592_000)) {
    return "routing.validation.slaRange";
  }
  if (warning !== null && firstResponse !== null && warning >= firstResponse) {
    return "routing.validation.slaOrder";
  }
  if (resolution !== null && firstResponse !== null && resolution <= firstResponse) {
    return "routing.validation.resolutionOrder";
  }
  if (
    draft.sla.clock === "working_hours"
    && draft.coverage.weekly_intervals.length === 0
    && (warning !== null || firstResponse !== null || resolution !== null)
  ) {
    return "routing.validation.slaCoverage";
  }
  if (draft.sla.clock === "working_hours" && draft.coverage.weekly_intervals.length > 0) {
    const weeklySeconds = draft.coverage.weekly_intervals.reduce((total, interval) => {
      const [startHour, startMinute] = interval.starts_at.split(":").map(Number);
      const [endHour, endMinute] = interval.ends_at.split(":").map(Number);
      return total + ((endHour * 60 + endMinute) - (startHour * 60 + startMinute)) * 60;
    }, 0);
    const feasibleSeconds = weeklySeconds * 520;
    if ([warning, firstResponse, resolution].some((value) => (
      value !== null && value > feasibleSeconds
    ))) {
      return "routing.validation.slaWorkingHoursFeasibility";
    }
  }
  if (
    draft.sla.escalation_queue_id
    && !activeQueueIds.has(draft.sla.escalation_queue_id)
  ) {
    return "routing.validation.escalationQueue";
  }
  if (draft.telegram.chat_ids.length > 32) return "routing.validation.chatIdLimit";
  if (draft.telegram.chat_ids.some((chatId) => !/^-?\d{1,20}$/.test(chatId.trim()))) {
    return "routing.validation.chatIds";
  }
  if (tokenAction === "replace" && !botTokenIsValid(tokenValue.trim())) {
    return "routing.validation.telegramToken";
  }
  const tokenWillExist = tokenAction === "replace"
    ? Boolean(tokenValue.trim())
    : tokenAction === "clear"
      ? false
      : storedTokenConfigured;
  if (telegramIsEnabled(draft) && (draft.telegram.chat_ids.length === 0 || !tokenWillExist)) {
    return "routing.validation.telegramCredentials";
  }
  return null;
}

function initialSelection(configuration: InboxRoutingConfiguration): InspectorSelection {
  const firstRule = [...configuration.rules].sort((left, right) => left.position - right.position)[0];
  if (firstRule) return { kind: "rule", id: firstRule.id };
  const firstQueue = configuration.queues[0];
  return firstQueue ? { kind: "queue", id: firstQueue.id } : { kind: "default" };
}

function useWorkflowConnectors(
  hostRef: React.RefObject<HTMLDivElement | null>,
  canvasRef: React.RefObject<HTMLCanvasElement | null>,
  connections: WorkflowConnection[],
  selected: string | null,
  layoutKey: string,
) {
  useLayoutEffect(() => {
    const host = hostRef.current;
    const canvas = canvasRef.current;
    if (!host || !canvas) return undefined;
    let frame = 0;

    const draw = () => {
      const bounds = host.getBoundingClientRect();
      if (bounds.width === 0 || bounds.height === 0) return;
      let context: CanvasRenderingContext2D | null;
      try {
        context = canvas.getContext("2d");
      } catch {
        return;
      }
      if (!context) return;
      const ratio = Math.max(1, window.devicePixelRatio || 1);
      const width = Math.max(bounds.width, host.scrollWidth);
      const height = Math.max(bounds.height, host.scrollHeight);
      canvas.width = Math.round(width * ratio);
      canvas.height = Math.round(height * ratio);
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      context.clearRect(0, 0, width, height);
      const styles = getComputedStyle(host);
      const baseColor = styles.getPropertyValue("--routing-connector").trim() || "#426176";
      const activeColor = styles.getPropertyValue("--routing-connector-active").trim() || "#79a0ff";
      const scrollLeft = host.scrollLeft;
      const scrollTop = host.scrollTop;

      for (const connection of connections) {
        const source = host.querySelector<HTMLElement>(`[data-flow-node="${connection.source}"]`);
        const target = host.querySelector<HTMLElement>(`[data-flow-node="${connection.target}"]`);
        if (!source || !target) continue;
        const sourceBounds = source.getBoundingClientRect();
        const targetBounds = target.getBoundingClientRect();
        const horizontal = targetBounds.left > sourceBounds.right + 20;
        const startX = horizontal
          ? sourceBounds.right - bounds.left + scrollLeft
          : sourceBounds.left + sourceBounds.width / 2 - bounds.left + scrollLeft;
        const startY = horizontal
          ? sourceBounds.top + sourceBounds.height / 2 - bounds.top + scrollTop
          : sourceBounds.bottom - bounds.top + scrollTop;
        const endX = horizontal
          ? targetBounds.left - bounds.left + scrollLeft
          : targetBounds.left + targetBounds.width / 2 - bounds.left + scrollLeft;
        const endY = horizontal
          ? targetBounds.top + targetBounds.height / 2 - bounds.top + scrollTop
          : targetBounds.top - bounds.top + scrollTop;
        const active = selected === connection.source || selected === connection.target;
        context.strokeStyle = active ? activeColor : baseColor;
        context.fillStyle = active ? activeColor : baseColor;
        context.lineWidth = active ? 2 : 1.25;
        context.setLineDash(connection.dashed ? [5, 5] : []);
        context.beginPath();
        context.moveTo(startX, startY);
        if (horizontal) {
          const distance = Math.max(28, (endX - startX) * 0.48);
          context.bezierCurveTo(startX + distance, startY, endX - distance, endY, endX, endY);
        } else {
          const distance = Math.max(22, (endY - startY) * 0.48);
          context.bezierCurveTo(startX, startY + distance, endX, endY - distance, endX, endY);
        }
        context.stroke();
        context.setLineDash([]);
        context.beginPath();
        if (horizontal) {
          context.moveTo(endX, endY);
          context.lineTo(endX - 6, endY - 3.5);
          context.lineTo(endX - 6, endY + 3.5);
        } else {
          context.moveTo(endX, endY);
          context.lineTo(endX - 3.5, endY - 6);
          context.lineTo(endX + 3.5, endY - 6);
        }
        context.closePath();
        context.fill();
      }
    };
    const scheduleDraw = () => {
      window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(draw);
    };
    scheduleDraw();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(scheduleDraw);
    observer?.observe(host);
    host.addEventListener("scroll", scheduleDraw, { passive: true });
    window.addEventListener("resize", scheduleDraw);
    return () => {
      window.cancelAnimationFrame(frame);
      observer?.disconnect();
      host.removeEventListener("scroll", scheduleDraw);
      window.removeEventListener("resize", scheduleDraw);
    };
  }, [canvasRef, connections, hostRef, layoutKey, selected]);
}

function WorkflowNode({
  flowId,
  icon: Icon,
  title,
  summary,
  selected,
  muted = false,
  children,
  onClick,
}: {
  flowId: string;
  icon: LucideIcon;
  title: string;
  summary: string;
  selected: boolean;
  muted?: boolean;
  children?: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      className={`routing-node${selected ? " routing-node--selected" : ""}${muted ? " routing-node--muted" : ""}`}
      type="button"
      data-flow-node={flowId}
      aria-label={`${title}: ${summary}`}
      aria-pressed={selected}
      onClick={onClick}
    >
      <Icon size={16} aria-hidden="true" />
      <span className="routing-node-copy"><strong>{title}</strong><span>{summary}</span></span>
      {children}
    </button>
  );
}

function ToggleRow({
  checked,
  title,
  description,
  disabled = false,
  onChange,
}: {
  checked: boolean;
  title: string;
  description: string;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="routing-toggle-row">
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.currentTarget.checked)}
      />
      <span><strong>{title}</strong><span>{description}</span></span>
    </label>
  );
}

export function InboxRoutingView({ auth, inboxes, onDirtyChange }: InboxRoutingViewProps) {
  const instanceId = useId();
  const { locale, t } = useI18n();
  const route = usePageRoute();
  const navigateRoute = route.navigate;
  const [response, setResponse] = useState<InboxRoutingResponse | null>(null);
  const [draft, setDraft] = useState<InboxRoutingWrite | null>(null);
  const [baseline, setBaseline] = useState("");
  const [selection, setSelection] = useState<InspectorSelection | null>(null);
  const [tokenAction, setTokenAction] = useState<TelegramBotTokenUpdate["action"]>("preserve");
  const [tokenValue, setTokenValue] = useState("");
  const [loading, setLoading] = useState(inboxes.length > 0);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [reloadSequence, setReloadSequence] = useState(0);
  const requestSequence = useRef(0);
  const saveSequence = useRef(0);
  const idempotencyKey = useRef<string | null>(null);
  const selectedInboxIdRef = useRef<string | null>(null);
  const graphRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const selectedKey = selectionKey(selection);
  const inspectorMotion = useContentMotion<HTMLFieldSetElement>(selectedKey ?? "none");
  const selectedInboxId = inboxes.find((inbox) => inbox.id === route.segments[0])?.id ?? inboxes[0]?.id ?? null;
  const selectedInbox = inboxes.find((inbox) => inbox.id === selectedInboxId) ?? null;
  const configuration = response?.configuration ?? null;
  const signature = draft ? draftSignature(draft, tokenAction) : "";
  const dirty = Boolean(draft && baseline && signature !== baseline);
  const canAddRule = Boolean(
    draft?.default_queue_id
    && draft.queues.some((queue) => (
      queue.id === draft.default_queue_id && queue.status === "active"
    )),
  );

  // The inbox changes through the URL, also by Back/Forward; the previous inbox's unsaved draft is discarded
  // at once, and nothing can be saved until the new inbox has loaded.
  const [shownInboxId, setShownInboxId] = useState(selectedInboxId);
  if (selectedInboxId !== shownInboxId) {
    setShownInboxId(selectedInboxId);
    setLoading(selectedInboxId !== null);
    setNotice(null);
    setDraft(response && configurationWrite(response.configuration));
    setTokenAction("preserve");
    setTokenValue("");
  }

  useCanonicalPageRoute(route, inboxRoutingSegments(inboxes, selectedInboxId));

  useLayoutEffect(() => {
    selectedInboxIdRef.current = selectedInboxId;
  }, [selectedInboxId]);

  useEffect(() => {
    onDirtyChange?.(dirty);
  }, [dirty, onDirtyChange]);

  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange]);

  useEffect(() => {
    if (!dirty) return undefined;
    const warnBeforeUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", warnBeforeUnload);
    return () => window.removeEventListener("beforeunload", warnBeforeUnload);
  }, [dirty]);

  useEffect(() => {
    if (!selectedInboxId) return undefined;
    let active = true;
    const timer = window.setTimeout(() => {
      const sequence = requestSequence.current + 1;
      requestSequence.current = sequence;
      setLoading(true);
      setNotice(null);
      getInboxRouting(auth, selectedInboxId)
        .then((nextResponse) => {
          if (!active || requestSequence.current !== sequence) return;
          const nextDraft = configurationWrite(nextResponse.configuration);
          setResponse(nextResponse);
          setDraft(nextDraft);
          setBaseline(draftSignature(nextDraft, "preserve"));
          setTokenAction("preserve");
          setTokenValue("");
          setSelection(initialSelection(nextResponse.configuration));
          idempotencyKey.current = null;
        })
        .catch(() => {
          if (active && requestSequence.current === sequence) {
            setResponse(null);
            setDraft(null);
            setNotice({ kind: "error", message: "routing.loadError" });
          }
        })
        .finally(() => {
          if (active && requestSequence.current === sequence) setLoading(false);
        });
    }, 0);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [auth, reloadSequence, selectedInboxId]);

  const mutateDraft = useCallback((update: (current: InboxRoutingWrite) => InboxRoutingWrite) => {
    setDraft((current) => current ? update(current) : current);
    setNotice(null);
    idempotencyKey.current = null;
  }, []);

  function mutateTelegramToken(
    action: TelegramBotTokenUpdate["action"],
    value = "",
  ) {
    setTokenAction(action);
    setTokenValue(value);
    setNotice(null);
    idempotencyKey.current = null;
  }

  function changeInbox(inboxId: string) {
    if (saving || loading || inboxId === selectedInboxId) return;
    if (dirty && !window.confirm(t("routing.discardChanges"))) return;
    void navigateRoute(inboxRoutingSegments(inboxes, inboxId));
  }

  function addQueue() {
    if ((draft?.queues.length ?? 0) >= 32) return;
    const id = generatedId();
    mutateDraft((current) => ({
      ...current,
      default_queue_id: current.default_queue_id ?? id,
      queues: [...current.queues, {
        id,
        name: t("routing.queueNewName"),
        status: "active",
        member_ids: [],
      }],
    }));
    setSelection({ kind: "queue", id });
  }

  function removeQueue(queue: InboxRoutingQueue) {
    if (!draft) return;
    const replacement = draft.queues.find((item) => (
      item.id !== queue.id && item.status === "active"
    )) ?? draft.queues.find((item) => item.id !== queue.id) ?? null;
    const requiresQueue = draft.rules.some((rule) => rule.queue_id === queue.id)
      || draft.assignment_strategy === "least_active"
      || draft.coverage.outside_hours_action === "queue"
      || draft.coverage.no_operator_action === "queue";
    if (!replacement && requiresQueue) return;
    const used = draft.default_queue_id === queue.id
      || draft.sla.escalation_queue_id === queue.id
      || draft.rules.some((rule) => rule.queue_id === queue.id);
    if (used && !window.confirm(t("routing.queueDeleteConfirm", { name: queue.name }))) return;
    mutateDraft((current) => ({
      ...current,
      default_queue_id: current.default_queue_id === queue.id
        ? replacement?.id ?? null
        : current.default_queue_id,
      queues: current.queues.filter((item) => item.id !== queue.id),
      rules: current.rules.map((rule) => (
        rule.queue_id === queue.id && replacement
          ? { ...rule, queue_id: replacement.id }
          : rule
      )),
      sla: {
        ...current.sla,
        escalation_queue_id: current.sla.escalation_queue_id === queue.id
          ? replacement?.id ?? null
          : current.sla.escalation_queue_id,
      },
    }));
    setSelection(replacement ? { kind: "queue", id: replacement.id } : { kind: "default" });
  }

  function addRule() {
    if (!draft?.default_queue_id || !canAddRule || draft.rules.length >= 64) return;
    const id = generatedId();
    mutateDraft((current) => ({
      ...current,
      rules: [...current.rules, {
        id,
        position: current.rules.length + 1,
        enabled: true,
        channel_id: response?.channel_options[0]?.id ?? null,
        language: null,
        queue_id: current.default_queue_id!,
      }],
    }));
    setSelection({ kind: "rule", id });
  }

  function moveRule(ruleId: string, direction: -1 | 1) {
    mutateDraft((current) => {
      const rules = [...current.rules].sort((left, right) => left.position - right.position);
      const index = rules.findIndex((rule) => rule.id === ruleId);
      const target = index + direction;
      if (index < 0 || target < 0 || target >= rules.length) return current;
      [rules[index], rules[target]] = [rules[target], rules[index]];
      return {
        ...current,
        rules: rules.map((rule, ruleIndex) => ({ ...rule, position: ruleIndex + 1 })),
      };
    });
  }

  async function saveRouting() {
    if (!draft || !configuration || !selectedInboxId || saving || loading) return;
    const nextDraft = normalizedWrite(draft);
    const validation = routingValidation(
      nextDraft,
      tokenAction,
      tokenValue,
      configuration.telegram.bot_token_configured,
    );
    if (validation) {
      setNotice({ kind: "error", message: validation });
      return;
    }
    const tokenUpdate: TelegramBotTokenUpdate = tokenAction === "replace"
      ? { action: "replace", value: tokenValue.trim() }
      : { action: tokenAction };
    const requestKey = idempotencyKey.current ?? generatedId();
    idempotencyKey.current = requestKey;
    const requestInboxId = selectedInboxId;
    const sequence = saveSequence.current + 1;
    saveSequence.current = sequence;
    setSaving(true);
    setNotice(null);
    try {
      const saved = await updateInboxRouting(auth, requestInboxId, {
        expected_version: configuration.version,
        configuration: nextDraft,
        telegram_bot_token: tokenUpdate,
      }, requestKey);
      if (
        saveSequence.current !== sequence
        || selectedInboxIdRef.current !== requestInboxId
      ) return;
      const savedDraft = configurationWrite(saved.configuration);
      setResponse(saved);
      setDraft(savedDraft);
      setBaseline(draftSignature(savedDraft, "preserve"));
      setTokenAction("preserve");
      setTokenValue("");
      setNotice({ kind: "success", message: "routing.saved" });
      idempotencyKey.current = null;
    } catch (cause) {
      if (
        saveSequence.current !== sequence
        || selectedInboxIdRef.current !== requestInboxId
      ) return;
      const deterministicFailure = cause instanceof ApiRequestError
        && cause.status >= 400
        && cause.status < 500;
      if (deterministicFailure) {
        idempotencyKey.current = null;
        if (tokenUpdate.action === "replace") {
          setTokenAction("preserve");
          setTokenValue("");
        }
      }
      if (cause instanceof ApiRequestError && cause.status === 409) {
        setNotice({ kind: "conflict", message: "routing.conflict" });
      } else {
        setNotice({ kind: "error", message: "routing.saveError" });
      }
    } finally {
      if (saveSequence.current === sequence) setSaving(false);
    }
  }

  const orderedRules = useMemo(
    () => [...(draft?.rules ?? [])].sort((left, right) => left.position - right.position),
    [draft?.rules],
  );
  const connections = useMemo<WorkflowConnection[]>(() => {
    if (!draft || !response) return [];
    const next: WorkflowConnection[] = [];
    for (const rule of orderedRules) {
      const channels = rule.channel_id
        ? response.channel_options.filter((channel) => channel.id === rule.channel_id)
        : response.channel_options;
      for (const channel of channels) {
        next.push({ source: `channel:${channel.id}`, target: `rule:${rule.id}` });
      }
      next.push({
        source: `rule:${rule.id}`,
        target: `queue:${rule.queue_id}`,
        dashed: !rule.enabled,
      });
    }
    for (const channel of response.channel_options) {
      next.push({ source: `channel:${channel.id}`, target: "default", dashed: true });
    }
    if (draft.default_queue_id) {
      next.push({ source: "default", target: `queue:${draft.default_queue_id}`, dashed: true });
    }
    for (const queue of draft.queues) {
      next.push({ source: `queue:${queue.id}`, target: "coverage" });
      if (
        draft.sla.first_response_seconds !== null
        || draft.sla.unassigned_warning_seconds !== null
        || draft.sla.resolution_seconds !== null
      ) {
        next.push({ source: `queue:${queue.id}`, target: "sla" });
      }
      if (telegramIsEnabled(draft)) {
        next.push({ source: `queue:${queue.id}`, target: "telegram" });
      }
    }
    return next;
  }, [draft, orderedRules, response]);
  const graphLayoutKey = JSON.stringify({
    channels: response?.channel_options.map((channel) => channel.id),
    rules: orderedRules.map((rule) => rule.id),
    queues: draft?.queues.map((queue) => queue.id),
    connections,
  });
  useWorkflowConnectors(graphRef, canvasRef, connections, selectedKey, graphLayoutKey);

  function channelName(channelId: string | null): string {
    if (!channelId) return t("routing.anyChannel");
    return response?.channel_options.find((channel) => channel.id === channelId)?.name
      ?? t("routing.missingChannel");
  }

  function queueName(queueId: string | null): string {
    if (!queueId) return t("routing.noQueue");
    return draft?.queues.find((queue) => queue.id === queueId)?.name ?? t("routing.missingQueue");
  }

  function slaNodeSummary(): string {
    if (!draft) return t("routing.slaDisabled");
    const parts = [
      draft.sla.unassigned_warning_seconds === null
        ? null
        : t("routing.slaWarningShort", { minutes: draft.sla.unassigned_warning_seconds / 60 }),
      draft.sla.first_response_seconds === null
        ? null
        : t("routing.slaReplyShort", { minutes: draft.sla.first_response_seconds / 60 }),
      draft.sla.resolution_seconds === null
        ? null
        : t("routing.slaResolutionShort", { minutes: draft.sla.resolution_seconds / 60 }),
    ].filter((part): part is string => part !== null);
    return parts.length > 0 ? parts.join(" · ") : t("routing.slaDisabled");
  }

  function renderChannelInspector(channel: InboxRoutingChannelOption) {
    const matching = orderedRules.filter((rule) => !rule.channel_id || rule.channel_id === channel.id);
    return (
      <div className="routing-inspector-fields">
        <dl className="routing-inspector-summary">
          <div><dt>{t("routing.channelKind")}</dt><dd>{channelKindLabel(t, channel.kind)}</dd></div>
          <div><dt>{t("routing.channelStatus")}</dt><dd>{channelStatusLabel(t, channel.status)}</dd></div>
        </dl>
        <div className="routing-related-list">
          <strong>{t("routing.channelRules")}</strong>
          {matching.map((rule) => (
            <button type="button" key={rule.id} onClick={() => setSelection({ kind: "rule", id: rule.id })}>
              {rule.language ?? t("routing.anyLanguage")} → {queueName(rule.queue_id)}
            </button>
          ))}
          {matching.length === 0 && <p>{t("routing.channelNoRules")}</p>}
        </div>
      </div>
    );
  }

  function renderDefaultInspector() {
    if (!draft) return null;
    return (
      <div className="routing-inspector-fields">
        <label><span>{t("routing.assignmentStrategy")}</span>
          <select
            value={draft.assignment_strategy}
            onChange={(event) => mutateDraft((current) => ({
              ...current,
              assignment_strategy: event.currentTarget.value as InboxRoutingWrite["assignment_strategy"],
            }))}
          >
            <option value="manual">{t("routing.strategyManual")}</option>
            <option value="least_active">{t("routing.strategyLeastActive")}</option>
          </select>
        </label>
        <p className="routing-field-help">{t(draft.assignment_strategy === "manual"
          ? "routing.strategyManualHelp"
          : "routing.strategyLeastActiveHelp")}</p>
        <label><span>{t("routing.defaultQueue")}</span>
          <select
            value={draft.default_queue_id ?? ""}
            onChange={(event) => mutateDraft((current) => ({
              ...current,
              default_queue_id: event.currentTarget.value || null,
            }))}
          >
            <option value="">{t("routing.noQueue")}</option>
            {draft.queues.map((queue) => <option value={queue.id} key={queue.id}>{queue.name}</option>)}
          </select>
        </label>
      </div>
    );
  }

  function renderQueueInspector(queue: InboxRoutingQueue) {
    if (!draft || !response) return null;
    const knownOperatorIds = new Set(response.operator_options.map((operator) => operator.id));
    const unavailableMemberIds = queue.member_ids.filter((memberId) => !knownOperatorIds.has(memberId));
    const lastQueueIsRequired = draft.queues.length === 1 && (
      draft.rules.some((rule) => rule.queue_id === queue.id)
      || draft.assignment_strategy === "least_active"
      || draft.coverage.outside_hours_action === "queue"
      || draft.coverage.no_operator_action === "queue"
    );
    const removeMember = (memberId: string) => mutateDraft((current) => ({
      ...current,
      queues: current.queues.map((item) => item.id === queue.id ? {
        ...item,
        member_ids: item.member_ids.filter((id) => id !== memberId),
      } : item),
    }));
    return (
      <div className="routing-inspector-fields">
        <label><span>{t("routing.queueName")}</span>
          <input
            value={queue.name}
            maxLength={120}
            onChange={(event) => mutateDraft((current) => ({
              ...current,
              queues: current.queues.map((item) => (
                item.id === queue.id ? { ...item, name: event.currentTarget.value } : item
              )),
            }))}
          />
        </label>
        <label><span>{t("routing.queueStatus")}</span>
          <select
            value={queue.status}
            onChange={(event) => mutateDraft((current) => ({
              ...current,
              queues: current.queues.map((item) => item.id === queue.id ? {
                ...item,
                status: event.currentTarget.value as InboxRoutingQueue["status"],
              } : item),
            }))}
          >
            <option value="active">{t("routing.statusActive")}</option>
            <option value="disabled">{t("routing.statusDisabled")}</option>
          </select>
        </label>
        <fieldset className="routing-operator-list">
          <legend>{t("routing.queueOperators")}</legend>
          {response.operator_options.map((operator) => (
            <label key={operator.id}>
              <input
                type="checkbox"
                checked={queue.member_ids.includes(operator.id)}
                onChange={() => mutateDraft((current) => ({
                  ...current,
                  queues: current.queues.map((item) => item.id === queue.id ? {
                    ...item,
                    member_ids: item.member_ids.includes(operator.id)
                      ? item.member_ids.filter((id) => id !== operator.id)
                      : [...item.member_ids, operator.id],
                  } : item),
                }))}
              />
              <span><strong>{operator.display_name}</strong><span>{operator.email}</span></span>
              <em>{t(operator.online ? "routing.operatorOnline" : "routing.operatorOffline")}</em>
            </label>
          ))}
          {unavailableMemberIds.length > 0 && (
            <div className="routing-unavailable-members">
              <p>{t("routing.unavailableOperatorsHelp")}</p>
              {unavailableMemberIds.map((memberId) => (
                <div key={memberId}>
                  <code>{memberId}</code>
                  <DemoActionButton
                    className="routing-icon-button"
                    type="button"
                    aria-label={t("routing.removeUnavailableOperator", { id: memberId })}
                    onClick={() => removeMember(memberId)}
                  >
                    <Trash2 size={14} />
                  </DemoActionButton>
                </div>
              ))}
            </div>
          )}
          {response.operator_options.length === 0 && <p>{t("routing.noOperators")}</p>}
        </fieldset>
        <DemoActionButton
          className="secondary-button routing-delete-button"
          type="button"
          disabled={lastQueueIsRequired}
          title={lastQueueIsRequired ? t("routing.deleteLastQueueBlocked") : undefined}
          onClick={() => removeQueue(queue)}
        >
          <Trash2 size={15} />{t("routing.deleteQueue")}
        </DemoActionButton>
      </div>
    );
  }

  function renderRuleInspector(rule: InboxRoutingRule) {
    if (!draft || !response) return null;
    const index = orderedRules.findIndex((item) => item.id === rule.id);
    const updateRule = (patch: Partial<InboxRoutingRule>) => mutateDraft((current) => ({
      ...current,
      rules: current.rules.map((item) => item.id === rule.id ? { ...item, ...patch } : item),
    }));
    return (
      <div className="routing-inspector-fields">
        <ToggleRow
          checked={rule.enabled}
          title={t("routing.ruleEnabled")}
          description={t("routing.ruleEnabledHelp")}
          onChange={(enabled) => updateRule({ enabled })}
        />
        <label><span>{t("routing.ruleChannel")}</span>
          <select value={rule.channel_id ?? ""} onChange={(event) => updateRule({ channel_id: event.currentTarget.value || null })}>
            <option value="">{t("routing.anyChannel")}</option>
            {response.channel_options.map((channel) => <option value={channel.id} key={channel.id}>{channel.name}</option>)}
          </select>
        </label>
        <label><span>{t("routing.ruleLanguage")}</span>
          <input
            value={rule.language ?? ""}
            maxLength={35}
            placeholder={t("routing.anyLanguage")}
            autoCapitalize="none"
            spellCheck={false}
            onBlur={(event) => updateRule({ language: normalizeLanguage(event.currentTarget.value) })}
            onChange={(event) => updateRule({ language: event.currentTarget.value || null })}
          />
        </label>
        <p className="routing-field-help">{t("routing.ruleLanguageHelp")}</p>
        <label><span>{t("routing.ruleQueue")}</span>
          <select value={rule.queue_id} onChange={(event) => updateRule({ queue_id: event.currentTarget.value })}>
            {draft.queues.map((queue) => <option value={queue.id} key={queue.id}>{queue.name}</option>)}
          </select>
        </label>
        <div className="routing-rule-actions">
          <button className="secondary-button" type="button" disabled={index <= 0} aria-label={t("routing.moveRuleUp")} onClick={() => moveRule(rule.id, -1)}><ArrowUp size={15} /></button>
          <button className="secondary-button" type="button" disabled={index < 0 || index === orderedRules.length - 1} aria-label={t("routing.moveRuleDown")} onClick={() => moveRule(rule.id, 1)}><ArrowDown size={15} /></button>
          <button className="secondary-button routing-delete-button" type="button" onClick={() => {
            mutateDraft((current) => ({ ...current, rules: current.rules.filter((item) => item.id !== rule.id) }));
            setSelection({ kind: "default" });
          }}><Trash2 size={15} />{t("routing.deleteRule")}</button>
        </div>
      </div>
    );
  }

  function renderCoverageInspector() {
    if (!draft) return null;
    const updateCoverage = (patch: Partial<InboxRoutingWrite["coverage"]>) => mutateDraft((current) => ({
      ...current,
      coverage: { ...current.coverage, ...patch },
    }));
    const intervals = draft.coverage.weekly_intervals;
    return (
      <div className="routing-inspector-fields">
        <label><span>{t("routing.timezone")}</span>
          <input
            value={draft.coverage.timezone}
            maxLength={64}
            placeholder="Europe/Istanbul"
            autoCapitalize="none"
            spellCheck={false}
            onChange={(event) => updateCoverage({ timezone: event.currentTarget.value })}
          />
        </label>
        <div className="routing-interval-heading">
          <strong>{t("routing.workingHours")}</strong>
          <button className="secondary-button" type="button" onClick={() => updateCoverage({
            weekly_intervals: [...intervals, {
              id: generatedId(),
              weekday: 1,
              starts_at: "09:00",
              ends_at: "18:00",
            }],
          })} disabled={intervals.length >= 28}><Plus size={14} />{t("routing.addInterval")}</button>
        </div>
        <div className="routing-interval-list">
          {intervals.map((interval, index) => (
            <div className="routing-interval-row" key={interval.id}>
              <select
                aria-label={t("routing.weekday")}
                value={interval.weekday}
                onChange={(event) => updateCoverage({
                  weekly_intervals: intervals.map((item, itemIndex) => itemIndex === index ? {
                    ...item,
                    weekday: Number(event.currentTarget.value),
                  } : item),
                })}
              >
                {Object.entries(weekdayKeys).map(([weekday, key]) => <option value={weekday} key={weekday}>{t(key)}</option>)}
              </select>
              <DateTimeField
                className="routing-interval-time"
                mode="time"
                locale={locale}
                clearable={false}
                aria-label={t("routing.startsAt")}
                value={interval.starts_at}
                onChange={(startsAt) => updateCoverage({
                  weekly_intervals: intervals.map((item, itemIndex) => itemIndex === index ? {
                    ...item,
                    starts_at: startsAt,
                  } : item),
                })}
              />
              <DateTimeField
                className="routing-interval-time"
                mode="time"
                locale={locale}
                clearable={false}
                aria-label={t("routing.endsAt")}
                value={interval.ends_at}
                onChange={(endsAt) => updateCoverage({
                  weekly_intervals: intervals.map((item, itemIndex) => itemIndex === index ? {
                    ...item,
                    ends_at: endsAt,
                  } : item),
                })}
              />
              <button
                className="routing-icon-button"
                type="button"
                aria-label={t("routing.removeInterval")}
                onClick={() => updateCoverage({
                  weekly_intervals: intervals.filter((_, itemIndex) => itemIndex !== index),
                })}
              ><Trash2 size={14} /></button>
            </div>
          ))}
          {intervals.length === 0 && <p>{t("routing.noIntervals")}</p>}
        </div>
        <label><span>{t("routing.outsideHoursAction")}</span>
          <select value={draft.coverage.outside_hours_action} onChange={(event) => updateCoverage({ outside_hours_action: event.currentTarget.value as InboxRoutingWrite["coverage"]["outside_hours_action"] })}>
            <option value="queue">{t("routing.actionQueue")}</option>
            <option value="ai">{t("routing.actionAi")}</option>
          </select>
        </label>
        <label><span>{t("routing.noOperatorAction")}</span>
          <select value={draft.coverage.no_operator_action} onChange={(event) => updateCoverage({ no_operator_action: event.currentTarget.value as InboxRoutingWrite["coverage"]["no_operator_action"] })}>
            <option value="queue">{t("routing.actionQueue")}</option>
            <option value="ai">{t("routing.actionAi")}</option>
          </select>
        </label>
        <p className="routing-field-help">{t("routing.aiFallbackHelp")}</p>
      </div>
    );
  }

  function renderSlaInspector() {
    if (!draft) return null;
    const updateSla = (patch: Partial<InboxRoutingWrite["sla"]>) => mutateDraft((current) => ({
      ...current,
      sla: { ...current.sla, ...patch },
    }));
    return (
      <div className="routing-inspector-fields">
        <label><span>{t("routing.slaClock")}</span>
          <select value={draft.sla.clock} onChange={(event) => updateSla({ clock: event.currentTarget.value as InboxRoutingWrite["sla"]["clock"] })}>
            <option value="working_hours">{t("routing.slaWorkingHours")}</option>
            <option value="elapsed">{t("routing.slaElapsed")}</option>
          </select>
        </label>
        <label><span>{t("routing.unassignedWarningMinutes")}</span>
          <input
            type="number"
            min={1}
            max={10_080}
            step={1}
            value={draft.sla.unassigned_warning_seconds === null ? "" : draft.sla.unassigned_warning_seconds / 60}
            placeholder={t("routing.disabledValue")}
            onChange={(event) => updateSla({
              unassigned_warning_seconds: event.currentTarget.value === ""
                ? null
                : Math.round(Number(event.currentTarget.value) * 60),
            })}
          />
        </label>
        <label><span>{t("routing.firstResponseMinutes")}</span>
          <input
            type="number"
            min={1}
            max={10_080}
            step={1}
            value={draft.sla.first_response_seconds === null ? "" : draft.sla.first_response_seconds / 60}
            placeholder={t("routing.disabledValue")}
            onChange={(event) => updateSla({
              first_response_seconds: event.currentTarget.value === ""
                ? null
                : Math.round(Number(event.currentTarget.value) * 60),
            })}
          />
        </label>
        <label><span>{t("routing.resolutionMinutes")}</span>
          <input
            type="number"
            min={1}
            max={43_200}
            step={1}
            value={draft.sla.resolution_seconds === null ? "" : draft.sla.resolution_seconds / 60}
            placeholder={t("routing.disabledValue")}
            onChange={(event) => updateSla({
              resolution_seconds: event.currentTarget.value === ""
                ? null
                : Math.round(Number(event.currentTarget.value) * 60),
            })}
          />
        </label>
        <label><span>{t("routing.escalationQueue")}</span>
          <select value={draft.sla.escalation_queue_id ?? ""} onChange={(event) => updateSla({ escalation_queue_id: event.currentTarget.value || null })}>
            <option value="">{t("routing.noEscalation")}</option>
            {draft.queues.map((queue) => <option value={queue.id} key={queue.id}>{queue.name}</option>)}
          </select>
        </label>
        <p className="routing-field-help">{t("routing.slaHelp")}</p>
      </div>
    );
  }

  function renderTelegramInspector() {
    if (!draft || !configuration) return null;
    const updateTelegram = (patch: Partial<InboxRoutingWrite["telegram"]>) => mutateDraft((current) => ({
      ...current,
      telegram: { ...current.telegram, ...patch },
    }));
    const storedToken = configuration.telegram.bot_token_configured;
    return (
      <div className="routing-inspector-fields">
        <p className="routing-routing-owner-note">{t("routing.telegramRoutingOwnership")}</p>
        <div className="routing-toggle-list">
          <ToggleRow checked={draft.telegram.new_visitor} title={t("routing.telegramNewVisitor")} description={t("routing.telegramNewVisitorHelp")} onChange={(new_visitor) => updateTelegram({ new_visitor })} />
          <ToggleRow checked={draft.telegram.new_message} title={t("routing.telegramNewMessage")} description={t("routing.telegramNewMessageHelp")} onChange={(new_message) => updateTelegram({ new_message })} />
          <ToggleRow checked={draft.telegram.operator_request} title={t("routing.telegramOperatorRequest")} description={t("routing.telegramOperatorRequestHelp")} onChange={(operator_request) => updateTelegram({ operator_request })} />
          <ToggleRow checked={draft.telegram.unassigned_warning} title={t("routing.telegramUnassigned")} description={t("routing.telegramUnassignedHelp")} onChange={(unassigned_warning) => updateTelegram({ unassigned_warning })} />
          <ToggleRow checked={draft.telegram.sla_breach} title={t("routing.telegramSla")} description={t("routing.telegramSlaHelp")} onChange={(sla_breach) => updateTelegram({ sla_breach })} />
        </div>
        <label><span>{t("routing.telegramChatIds")}</span>
          <textarea
            rows={4}
            value={draft.telegram.chat_ids.join("\n")}
            placeholder="-1001234567890"
            onChange={(event) => updateTelegram({
              chat_ids: event.currentTarget.value.split(/[\n,]/),
            })}
          />
        </label>
        <p className="routing-field-help">{t("routing.telegramChatIdsHelp")}</p>
        <label><span>{t("routing.telegramBotToken")}</span>
          <input
            type="password"
            value={tokenValue}
            maxLength={256}
            autoComplete="new-password"
            disabled={tokenAction === "clear" || auth.kind !== "session"}
            placeholder={auth.kind !== "session"
              ? t("routing.telegramTokenSessionOnly")
              : storedToken
                ? t("routing.telegramTokenStored")
                : t("routing.telegramTokenPlaceholder")}
            onChange={(event) => {
              const value = event.currentTarget.value;
              mutateTelegramToken(value ? "replace" : "preserve", value);
            }}
          />
        </label>
        <p className="routing-field-help">{t(auth.kind === "session"
          ? "routing.telegramTokenHelp"
          : "routing.telegramTokenSessionOnly")}</p>
        <ToggleRow
          checked={tokenAction === "clear"}
          disabled={auth.kind !== "session" || !storedToken}
          title={t("routing.telegramClearToken")}
          description={t("routing.telegramClearTokenHelp")}
          onChange={(clear) => mutateTelegramToken(clear ? "clear" : "preserve")}
        />
      </div>
    );
  }

  function renderInspector() {
    if (!draft || !response || !selection) return <div className="routing-inspector-empty">{t("routing.selectNode")}</div>;
    if (selection.kind === "channel") {
      const channel = response.channel_options.find((item) => item.id === selection.id);
      return channel ? renderChannelInspector(channel) : null;
    }
    if (selection.kind === "rule") {
      const rule = draft.rules.find((item) => item.id === selection.id);
      return rule ? renderRuleInspector(rule) : null;
    }
    if (selection.kind === "queue") {
      const queue = draft.queues.find((item) => item.id === selection.id);
      return queue ? renderQueueInspector(queue) : null;
    }
    if (selection.kind === "default") return renderDefaultInspector();
    if (selection.kind === "coverage") return renderCoverageInspector();
    if (selection.kind === "sla") return renderSlaInspector();
    return renderTelegramInspector();
  }

  function inspectorTitle(): string {
    if (!draft || !response || !selection) return t("routing.inspector");
    if (selection.kind === "channel") return response.channel_options.find((item) => item.id === selection.id)?.name ?? t("routing.inspector");
    if (selection.kind === "rule") return t("routing.ruleNumber", { number: orderedRules.findIndex((item) => item.id === selection.id) + 1 });
    if (selection.kind === "queue") return draft.queues.find((item) => item.id === selection.id)?.name ?? t("routing.inspector");
    if (selection.kind === "default") return t("routing.defaultRoute");
    if (selection.kind === "coverage") return t("routing.coverageTitle");
    if (selection.kind === "sla") return t("routing.slaTitle");
    return t("routing.telegramTitle");
  }

  return (
    <section className="page routing-page">
      <header className="page-toolbar">
        <div><h1>{t("routing.title")}</h1><p>{t("routing.description")}</p></div>
      </header>
      {notice && (
        <div className={`admin-notice admin-notice--${notice.kind === "success" ? "success" : "error"}`} role={notice.kind === "success" ? "status" : "alert"}>
          <span>{t(notice.message)}</span>
          {notice.kind === "conflict" && (
            <button className="secondary-button" type="button" onClick={() => {
              setLoading(true);
              setReloadSequence((value) => value + 1);
            }}>
              <RefreshCw size={15} />{t("routing.reload")}
            </button>
          )}
        </div>
      )}
      {inboxes.length === 0 ? (
        <div className="empty-state">{t("routing.noInboxes")}</div>
      ) : (
        <div className={`routing-workbench${saving ? " routing-workbench--saving" : ""}`} aria-busy={saving}>
          <div className="routing-workbench-toolbar">
            <label><span>{t("routing.inbox")}</span>
              <select disabled={saving || loading} value={selectedInboxId ?? ""} onChange={(event) => changeInbox(event.currentTarget.value)}>
                {inboxes.map((inbox) => <option value={inbox.id} key={inbox.id}>{inbox.name}</option>)}
              </select>
            </label>
            {draft && (
              <ToggleRow
                checked={draft.enabled}
                disabled={saving || loading}
                title={t("routing.enabled")}
                description={t(draft.enabled ? "routing.enabledHelp" : "routing.disabledHelp")}
                onChange={(enabled) => mutateDraft((current) => ({ ...current, enabled }))}
              />
            )}
            <div className="routing-toolbar-actions">
              {draft && !canAddRule && (
                <span className="routing-toolbar-help">{t("routing.addRuleNeedsQueue")}</span>
              )}
              <button
                className="secondary-button"
                type="button"
                disabled={!draft || saving || loading || !canAddRule || draft.rules.length >= 64}
                title={draft && !canAddRule ? t("routing.addRuleNeedsQueue") : undefined}
                onClick={addRule}
              ><Plus size={15} />{t("routing.addRule")}</button>
              <button className="secondary-button" type="button" disabled={!draft || saving || loading || draft.queues.length >= 32} onClick={addQueue}><Plus size={15} />{t("routing.addQueue")}</button>
              <DemoActionButton className="primary-button" type="button" disabled={!dirty || saving || loading} onClick={() => void saveRouting()}>
                <Save size={15} />{saving ? t("routing.saving") : t("routing.save")}
              </DemoActionButton>
            </div>
          </div>
          {loading ? (
            <div className="routing-workbench-loading" role="status">{t("routing.loading")}</div>
          ) : draft && response && selectedInbox ? (
            <div className="routing-workbench-body">
              <div className="routing-canvas" ref={graphRef} aria-label={t("routing.workflowLabel")}>
                <canvas ref={canvasRef} aria-hidden="true" />
                <div className="routing-graph-columns">
                  <section className="routing-graph-column" aria-labelledby={`${instanceId}-routing-column-channels`}>
                    <h2 id={`${instanceId}-routing-column-channels`}><Radio size={15} />{t("routing.columnChannels")}</h2>
                    <div className="routing-node-list">
                      {response.channel_options.map((channel) => (
                        <WorkflowNode
                          flowId={`channel:${channel.id}`}
                          icon={Radio}
                          title={channel.name}
                          summary={`${channelKindLabel(t, channel.kind)} · ${channelStatusLabel(t, channel.status)}`}
                          selected={selectedKey === `channel:${channel.id}`}
                          muted={channel.status !== "active"}
                          key={channel.id}
                          onClick={() => setSelection({ kind: "channel", id: channel.id })}
                        />
                      ))}
                      {response.channel_options.length === 0 && <p className="routing-column-empty">{t("routing.noChannels")}</p>}
                    </div>
                  </section>
                  <section className="routing-graph-column" aria-labelledby={`${instanceId}-routing-column-rules`}>
                    <h2 id={`${instanceId}-routing-column-rules`}><Languages size={15} />{t("routing.columnRules")}</h2>
                    <div className="routing-node-list">
                      {orderedRules.map((rule, index) => (
                        <WorkflowNode
                          flowId={`rule:${rule.id}`}
                          icon={Route}
                          title={t("routing.ruleNumber", { number: index + 1 })}
                          summary={`${channelName(rule.channel_id)} · ${rule.language ?? t("routing.anyLanguage")}`}
                          selected={selectedKey === `rule:${rule.id}`}
                          muted={!rule.enabled}
                          key={rule.id}
                          onClick={() => setSelection({ kind: "rule", id: rule.id })}
                        />
                      ))}
                      <WorkflowNode
                        flowId="default"
                        icon={Route}
                        title={t("routing.defaultRoute")}
                        summary={queueName(draft.default_queue_id)}
                        selected={selectedKey === "default"}
                        onClick={() => setSelection({ kind: "default" })}
                      />
                    </div>
                  </section>
                  <section className="routing-graph-column" aria-labelledby={`${instanceId}-routing-column-queues`}>
                    <h2 id={`${instanceId}-routing-column-queues`}><Users size={15} />{t("routing.columnQueues")}</h2>
                    <div className="routing-node-list">
                      {draft.queues.map((queue) => {
                        const members = response.operator_options.filter((operator) => queue.member_ids.includes(operator.id));
                        return (
                          <WorkflowNode
                            flowId={`queue:${queue.id}`}
                            icon={Users}
                            title={queue.name}
                            summary={t("routing.operatorCount", { count: queue.member_ids.length })}
                            selected={selectedKey === `queue:${queue.id}`}
                            muted={queue.status === "disabled"}
                            key={queue.id}
                            onClick={() => setSelection({ kind: "queue", id: queue.id })}
                          >
                            {members.length > 0 && (
                              <span className="routing-node-members" aria-hidden="true">
                                {members.slice(0, 3).map((operator) => <i key={operator.id}>{operator.display_name.trim().slice(0, 1).toUpperCase()}</i>)}
                              </span>
                            )}
                          </WorkflowNode>
                        );
                      })}
                    </div>
                  </section>
                  <section className="routing-graph-column" aria-labelledby={`${instanceId}-routing-column-actions`}>
                    <h2 id={`${instanceId}-routing-column-actions`}><Clock3 size={15} />{t("routing.columnActions")}</h2>
                    <div className="routing-node-list">
                      <WorkflowNode
                        flowId="coverage"
                        icon={Bot}
                        title={t("routing.coverageTitle")}
                        summary={`${draft.coverage.timezone} · ${t(draft.coverage.outside_hours_action === "ai" ? "routing.actionAiShort" : "routing.actionQueueShort")}`}
                        selected={selectedKey === "coverage"}
                        onClick={() => setSelection({ kind: "coverage" })}
                      />
                      <WorkflowNode
                        flowId="telegram"
                        icon={BellRing}
                        title={t("routing.telegramTitle")}
                        summary={telegramIsEnabled(draft) ? t("routing.telegramEnabled") : t("routing.telegramDisabled")}
                        selected={selectedKey === "telegram"}
                        muted={!telegramIsEnabled(draft)}
                        onClick={() => setSelection({ kind: "telegram" })}
                      />
                      <WorkflowNode
                        flowId="sla"
                        icon={ShieldAlert}
                        title={t("routing.slaTitle")}
                        summary={slaNodeSummary()}
                        selected={selectedKey === "sla"}
                        muted={draft.sla.first_response_seconds === null
                          && draft.sla.unassigned_warning_seconds === null
                          && draft.sla.resolution_seconds === null}
                        onClick={() => setSelection({ kind: "sla" })}
                      />
                    </div>
                  </section>
                </div>
              </div>
              <aside className="routing-inspector" aria-labelledby={`${instanceId}-routing-inspector-title`}>
                <header>
                  <div><span>{t("routing.inspector")}</span><h2 id={`${instanceId}-routing-inspector-title`}>{inspectorTitle()}</h2></div>
                  {configuration?.configured && <em>{t("routing.version", { version: configuration.version })}</em>}
                </header>
                <fieldset ref={inspectorMotion} className="routing-inspector-content" disabled={saving}>
                  {renderInspector()}
                </fieldset>
              </aside>
            </div>
          ) : (
            <div className="routing-workbench-loading">{t("routing.loadError")}</div>
          )}
        </div>
      )}
    </section>
  );
}
