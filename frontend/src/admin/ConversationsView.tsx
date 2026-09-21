import { Fragment, useCallback, useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import {
  Bell,
  BellOff,
  Bot,
  Check,
  CheckCheck,
  Clock3,
  Inbox as InboxIcon,
  Mail,
  MessageCircle,
  Mic,
  PanelRight,
  RefreshCw,
  Search,
  Send,
  UserPlus,
  Webhook,
  X,
  type LucideIcon,
} from "lucide-react";
import {
  ApiRequestError,
  downloadOperatorAttachment,
  getVisitorIntelligence,
  joinConversation,
  listInboxConversationPage,
  listOperatorMessages,
  markOperatorConversationRead,
  reopenConversation,
  resolveConversation,
  returnConversationToAi,
  sendOperatorMessage,
  takeOverConversation,
  updateConversationAttachmentPolicy,
  uploadOperatorAttachment,
  type Inbox,
  type Message,
  type OperatorConversation,
  type OperatorAuth,
  type RealtimeEvent,
  type VisitorIntelligence,
} from "../api";
import { MessageAttachments } from "../MessageAttachments";
import { voiceText } from "./voice-i18n";
import { MessageBody } from "../MessageBody";
import { isAllowedAttachmentFile } from "../attachment-files";
import {
  channelKindLabel,
  conversationStatusLabel,
  messageStatusLabel,
  useI18n,
  type MessageKey,
} from "../i18n";
import { ConversationContactDetails } from "./ConversationContactDetails";
import { ConversationComposer } from "./ConversationComposer";
import { useContentMotion } from "./useContentMotion";
import { DemoActionButton } from "./DemoReadOnly";
import { OperatorAvatar } from "./OperatorAvatar";
import type { BrowserNotificationPermission } from "./operator-notifications";
import { conversationName, conversationPreview, sameCalendarDay, waitingLabel } from "./conversation-presentation";
import { conversationRoute, conversationSegments } from "./conversation-route";
import { usePageRoute } from "./page-route";
import "./conversations.css";

interface ConversationsViewProps {
  auth: OperatorAuth;
  isDemo?: boolean;
  canSendAsAi: boolean;
  /** Session administrators may write for another operator or join in their place. */
  canSuperviseOperators?: boolean;
  canReplyConversations: boolean;
  canCloseConversations: boolean;
  canManageContacts?: boolean;
  inboxes: Inbox[];
  availableChannels?: ConversationChannel[];
  activeInboxId: string | null;
  realtimeEvent: RealtimeEvent | null;
  realtimeSyncRevision: number;
  notificationsChanging: boolean;
  notificationsEnabled: boolean;
  notificationPermission: BrowserNotificationPermission;
  onNotificationsChange: (enabled: boolean) => Promise<void>;
  onInboxChange: (inboxId: string) => void;
}

type StatusFilter = "active" | "resolved" | "all";
type QueueFilter = "all" | "needs_reply" | "mine";
type ConversationChannel = OperatorConversation["channel"];

function ConversationLayout({ className, children }: { className: string; children: ReactNode }) {
  return (
    <div className="conversation-frame">
      <div className="conversation-resizable">
        <div className={className}>{children}</div>
      </div>
    </div>
  );
}

function conversationsWithMessages(
  conversations: OperatorConversation[],
  includedConversationId?: string,
): OperatorConversation[] {
  return conversations.filter((conversation) => conversation.last_message_sequence > 0
    || conversation.id === includedConversationId);
}

function conversationChannels(
  conversations: OperatorConversation[],
): ConversationChannel[] {
  return Array.from(
    new Map(conversations.map((conversation) => [
      conversation.channel.id,
      conversation.channel,
    ])).values(),
  ).sort((left, right) => left.name.localeCompare(right.name));
}

function mergeConversationChannels(
  current: ConversationChannel[],
  conversations: OperatorConversation[],
): ConversationChannel[] {
  return Array.from(
    new Map([...current, ...conversationChannels(conversations)].map((channel) => [
      channel.id,
      channel,
    ])).values(),
  ).sort((left, right) => left.name.localeCompare(right.name));
}

function conversationMatchesFilters(
  conversation: OperatorConversation,
  statusFilter: StatusFilter,
  channelFilter: string,
): boolean {
  const statusMatches = statusFilter === "all"
    || (statusFilter === "resolved"
      ? conversation.status === "resolved"
      : conversation.status !== "resolved");
  const channelMatches = channelFilter === "all"
    || conversation.channel.id === channelFilter;
  return statusMatches && channelMatches;
}

interface ConversationParticipantProps {
  displayName: string;
  avatarUrl: string | null;
  label: string;
}

function ConversationParticipant({
  displayName,
  avatarUrl,
  label,
}: ConversationParticipantProps) {
  return (
    <div className="conversation-participant">
      <OperatorAvatar displayName={displayName} avatarUrl={avatarUrl} />
      <span>
        <span className="conversation-participant-label">{label}</span>
        <strong>{displayName}</strong>
      </span>
    </div>
  );
}

const channelIcons: Record<string, LucideIcon> = {
  widget: MessageCircle,
  external_api: Webhook,
  telegram_bot: Send,
  telegram_business: Send,
  telegram_tdlib: Send,
  gmail: Mail,
  imap_smtp: Mail,
};

function ConversationChannelLabel({ channel }: { channel: ConversationChannel }) {
  const { t } = useI18n();
  const ChannelIcon = channelIcons[channel.kind] ?? InboxIcon;
  const kind = channelKindLabel(t, channel.kind);

  return (
    <span className="conversation-channel-label" title={`${channel.name} · ${kind}`}>
      <ChannelIcon size={13} aria-hidden="true" />
      <span className="conversation-channel-name">{channel.name}</span>
      <span className="conversation-channel-kind">· {kind}</span>
    </span>
  );
}

function scrollMessagesToEnd(
  container: HTMLDivElement | null,
  behavior: ScrollBehavior = "auto",
) {
  if (!container) return;
  const reducedMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
  if (typeof container.scrollTo === "function") {
    container.scrollTo({
      top: container.scrollHeight,
      behavior: reducedMotion ? "auto" : behavior,
    });
  } else {
    container.scrollTop = container.scrollHeight;
  }
}

export function ConversationsView({
  auth,
  isDemo = false,
  canSendAsAi,
  canSuperviseOperators = false,
  canReplyConversations,
  canCloseConversations,
  canManageContacts = false,
  inboxes,
  availableChannels,
  activeInboxId,
  realtimeEvent,
  realtimeSyncRevision,
  notificationsChanging,
  notificationsEnabled,
  notificationPermission,
  onNotificationsChange,
  onInboxChange,
}: ConversationsViewProps) {
  const { locale, t, formatDate } = useI18n();
  const instanceId = useId();
  const route = usePageRoute();
  const navigateRoute = route.navigate;
  const linkedConversation = conversationRoute(route.segments);
  const linkedInboxId = linkedConversation?.inboxId ?? null;
  // A link into another inbox waits for that inbox; this view remounts for every active inbox.
  const linkedConversationId = linkedInboxId === activeInboxId
    ? linkedConversation?.conversationId ?? null
    : null;
  const messagesContainerRef = useRef<HTMLDivElement>(null);
  const attachmentInputRef = useRef<HTMLInputElement>(null);
  const knownMessageIdsRef = useRef<Set<string>>(new Set());
  const messageRequestRef = useRef(0);
  const realtimeSyncRevisionRef = useRef(realtimeSyncRevision);
  const conversationRequestRef = useRef<AbortController | null>(null);
  const loadConversationsRef = useRef<((preserveSelection?: boolean) => Promise<void>) | null>(null);
  const [conversations, setConversations] = useState<OperatorConversation[]>([]);
  const [channels, setChannels] = useState<ConversationChannel[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(linkedConversationId);
  const selectedIdRef = useRef(selectedId);
  const [messages, setMessages] = useState<Message[]>([]);
  const headerMotion = useContentMotion<HTMLElement>(selectedId ?? "none");
  const messagesMotion = useContentMotion<HTMLDivElement>(messages[0]?.conversation_id ?? selectedId ?? "none");
  const setMessagesContainer = useCallback((element: HTMLDivElement | null) => {
    messagesContainerRef.current = element;
    messagesMotion(element);
  }, [messagesMotion]);
  const [enteringMessageId, setEnteringMessageId] = useState<string | null>(null);
  const [statusFilter, setStatusFilter] = useState<StatusFilter>(linkedConversationId ? "all" : "active");
  const [queueFilter, setQueueFilter] = useState<QueueFilter>("all");
  const [hasMore, setHasMore] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(() => window.innerWidth >= 1526);
  const [includedConversationId, setIncludedConversationId] = useState(linkedConversationId ?? undefined);
  const [now, setNow] = useState(() => Date.now());
  const [channelFilter, setChannelFilter] = useState("all");
  const listMotion = useContentMotion<HTMLDivElement>(`${statusFilter}:${queueFilter}:${channelFilter}`);
  const [search, setSearch] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [visitorDrafts, setVisitorDrafts] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [sending, setSending] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [updatingAttachmentPolicy, setUpdatingAttachmentPolicy] = useState(false);
  const [joining, setJoining] = useState(false);
  const [returningToAi, setReturningToAi] = useState(false);
  const [error, setError] = useState<MessageKey | null>(null);
  const [visitorResult, setVisitorResult] = useState<{
    conversationId: string;
    intelligence: VisitorIntelligence | null;
  } | null>(null);

  useEffect(() => {
    selectedIdRef.current = selectedId;
  }, [selectedId]);

  const selected = conversations.find((item) => item.id === selectedId) ?? null;
  const draft = selectedId ? drafts[selectedId] ?? "" : "";
  const setDraft = (value: string) => {
    if (selectedId) setDrafts((current) => ({ ...current, [selectedId]: value }));
  };
  const canReplyAsOperator = selected?.status !== "resolved"
    && selected?.assigned_to_me === true
    && canReplyConversations;
  const canReplyAsAi = selected?.status !== "resolved"
    && selected?.operator === null
    && selected?.ai_agent?.active === true
    && canSendAsAi;
  const canReplyForOperator = selected?.status !== "resolved"
    && Boolean(selected?.operator)
    && selected?.assigned_to_me === false
    && canSuperviseOperators;
  const canReturnToAi = canReplyConversations
    && selected?.status !== "resolved"
    && Boolean(selected?.operator)
    && (selected?.assigned_to_me === true || canSuperviseOperators)
    && selected?.ai_agent?.active !== true;
  const canReply = canReplyAsOperator || canReplyAsAi || canReplyForOperator;
  const replySender = canReplyAsOperator
    ? "operator"
    : canReplyForOperator ? "assigned_operator" : "ai";
  const selectedUnreadCustomerMessages = selected?.unread_customer_messages ?? 0;
  const visitorDraft = selectedId ? visitorDrafts[selectedId] ?? "" : "";
  const visitorIntelligence = visitorResult?.conversationId === selectedId
    ? visitorResult.intelligence
    : null;
  const visitorStatus = !selectedId
    ? "unavailable"
    : visitorResult?.conversationId !== selectedId
      ? "loading"
      : visitorResult.intelligence
        ? "ready"
        : "unavailable";
  const filtered = useMemo(
    () =>
      conversations.filter((conversation) =>
        conversationMatchesFilters(conversation, statusFilter, channelFilter)
        && (queueFilter !== "needs_reply" || !!conversation.awaiting_reply_since)
        && (queueFilter !== "mine" || conversation.assigned_to_me),
      ),
    [channelFilter, conversations, queueFilter, statusFilter],
  );

  // Links and Back/Forward select through the URL; filters open up when they hide the linked conversation.
  const [routedConversationId, setRoutedConversationId] = useState(linkedConversationId);
  if (linkedConversationId !== routedConversationId) {
    setRoutedConversationId(linkedConversationId);
    if (linkedConversationId !== selectedId) {
      if (linkedConversationId && !filtered.some((conversation) => conversation.id === linkedConversationId)) {
        setStatusFilter("all");
        setQueueFilter("all");
        setChannelFilter("all");
        setSearch("");
        setDebouncedSearch("");
        setIncludedConversationId(linkedConversationId);
      }
      setSelectedId(linkedConversationId);
    }
  }

  useEffect(() => {
    if (!linkedInboxId || linkedInboxId === activeInboxId) return;
    if (inboxes.some((inbox) => inbox.id === linkedInboxId)) onInboxChange(linkedInboxId);
    else void navigateRoute([], { replace: true });
  }, [activeInboxId, inboxes, linkedInboxId, navigateRoute, onInboxChange]);

  const loadConversations = useCallback(async (preserveSelection = false) => {
    conversationRequestRef.current?.abort();
    if (!activeInboxId) {
      return;
    }
    const controller = new AbortController();
    conversationRequestRef.current = controller;
    try {
      const page = await listInboxConversationPage(auth, activeInboxId, {
          search: debouncedSearch || undefined,
          includeConversationId: includedConversationId,
          needsReply: queueFilter === "needs_reply" || undefined,
          mine: queueFilter === "mine" || undefined,
          status: statusFilter,
          channelId: channelFilter === "all" ? undefined : channelFilter,
          signal: controller.signal,
        });
      const items = conversationsWithMessages(page.items, includedConversationId);
      const focusedId = selectedIdRef.current;
      if (preserveSelection && focusedId && !items.some((item) => item.id === focusedId)) {
        const focusedPage = await listInboxConversationPage(auth, activeInboxId, {
          includeConversationId: focusedId, status: "all", signal: controller.signal,
        });
        const focused = focusedPage.items.find((item) => item.id === focusedId);
        if (focused) items.push(focused);
      }
      if (conversationRequestRef.current !== controller) return;
      if (preserveSelection && selectedIdRef.current && selectedIdRef.current !== focusedId
        && !items.some((item) => item.id === selectedIdRef.current)) {
        void loadConversationsRef.current?.(true);
        return;
      }
      setConversations(items);
      setHasMore(page.has_more);
      setChannels((current) => mergeConversationChannels(current, items));
      const shownId = selectedIdRef.current;
      if (shownId && !items.some((item) => item.id === shownId)) void navigateRoute([], { replace: true });
      setSelectedId((current) =>
        current && items.some((item) => item.id === current)
          ? current
          : null,
      );
      setError((current) => current === "conversations.loadError" ? null : current);
    } catch {
      if (!controller.signal.aborted && conversationRequestRef.current === controller) {
        setError("conversations.loadError");
      }
    } finally {
      if (conversationRequestRef.current === controller) {
        conversationRequestRef.current = null;
        setLoading(false);
      }
    }
  }, [activeInboxId, auth, channelFilter, debouncedSearch, includedConversationId, navigateRoute, queueFilter, statusFilter]);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, []);

  const applyMessages = useCallback((items: Message[], animateNewest: boolean) => {
    const newest = animateNewest
      ? [...items].reverse().find((item) => !knownMessageIdsRef.current.has(item.id))
      : undefined;
    knownMessageIdsRef.current = new Set(items.map((item) => item.id));
    setMessages(items);
    if (newest) {
      setEnteringMessageId(newest.id);
    } else if (!animateNewest) {
      setEnteringMessageId(null);
    }
  }, []);

  const loadMessages = useCallback(
    async (conversationId: string, animateNewest = false) => {
      const request = ++messageRequestRef.current;
      try {
        const items = await listOperatorMessages(auth, conversationId);
        if (request !== messageRequestRef.current) return;
        applyMessages(items, animateNewest);
      } catch {
        if (request === messageRequestRef.current) {
          setError("conversations.messagesError");
        }
      }
    },
    [applyMessages, auth],
  );
  const loadAttachment = useCallback(
    (attachmentId: string) => downloadOperatorAttachment(auth, attachmentId),
    [auth],
  );

  useEffect(() => {
    const nextSearch = search.trim();
    if (nextSearch === debouncedSearch) return undefined;
    const timeout = window.setTimeout(() => {
      setLoading(true);
      setDebouncedSearch(nextSearch);
    }, 250);
    return () => window.clearTimeout(timeout);
  }, [debouncedSearch, search]);

  useEffect(() => {
    loadConversationsRef.current = loadConversations;
  }, [loadConversations]);

  useEffect(() => {
    if (!activeInboxId) return undefined;
    const timer = window.setTimeout(() => {
      void loadConversations();
    }, 0);
    return () => {
      window.clearTimeout(timer);
      const controller = conversationRequestRef.current;
      controller?.abort();
      if (conversationRequestRef.current === controller) {
        conversationRequestRef.current = null;
      }
    };
  }, [activeInboxId, loadConversations]);

  useEffect(() => {
    if (!selectedId) return undefined;
    knownMessageIdsRef.current = new Set();
    const timer = window.setTimeout(() => void loadMessages(selectedId), 0);
    return () => {
      window.clearTimeout(timer);
      messageRequestRef.current += 1;
    };
  }, [loadMessages, selectedId]);

  useEffect(() => {
    if (!selectedId || selectedUnreadCustomerMessages <= 0) return undefined;
    let active = true;
    markOperatorConversationRead(auth, selectedId)
      .then(() => {
        if (!active) return;
        setConversations((current) => current.map((conversation) =>
          conversation.id === selectedId
            ? { ...conversation, unread_customer_messages: 0 }
            : conversation,
        ));
      })
      .catch(() => {
        if (active) setError("conversations.readError");
      });
    return () => { active = false; };
  }, [auth, selectedId, selectedUnreadCustomerMessages]);

  useEffect(() => {
    const frame = window.requestAnimationFrame(() => {
      scrollMessagesToEnd(
        messagesContainerRef.current,
        enteringMessageId ? "smooth" : "auto",
      );
    });
    return () => window.cancelAnimationFrame(frame);
  }, [enteringMessageId, messages.length, selectedId]);

  useEffect(() => {
    if (!selectedId) return undefined;
    let active = true;
    getVisitorIntelligence(auth, selectedId)
      .then((intelligence) => {
        if (!active) return;
        setVisitorResult({ conversationId: selectedId, intelligence });
      })
      .catch(() => {
        if (active) {
          setVisitorResult({ conversationId: selectedId, intelligence: null });
        }
      });
    return () => { active = false; };
  }, [auth, selectedId]);

  useEffect(() => {
    if (!realtimeEvent) return;
    if (realtimeEvent.type === "draft.updated") {
      const conversationId = realtimeEvent.data.conversation_id;
      const body = realtimeEvent.data.body;
      const timer = window.setTimeout(() => {
        if (typeof conversationId === "string" && typeof body === "string") {
          setVisitorDrafts((current) => ({ ...current, [conversationId]: body }));
        }
      }, 0);
      return () => window.clearTimeout(timer);
    }
  }, [realtimeEvent]);

  const reconcile = useCallback(async (animateNewest: boolean) => {
    await Promise.all([
      loadConversationsRef.current?.(true),
      selectedId ? loadMessages(selectedId, animateNewest) : Promise.resolve(),
    ]);
  }, [loadMessages, selectedId]);

  useEffect(() => {
    if (realtimeSyncRevisionRef.current === realtimeSyncRevision) return;
    realtimeSyncRevisionRef.current = realtimeSyncRevision;
    void reconcile(true);
  }, [realtimeSyncRevision, reconcile]);

  async function handleSend(event: React.FormEvent) {
    event.preventDefault();
    const body = draft.trim();
    const maxMessageLength = selected?.channel.kind === "telegram_bot" ? 4_096 : 10_000;
    if (!selectedId || !selected || !canReply || !body || Array.from(body).length > maxMessageLength || sending || joining || returningToAi) return;
    const conversationId = selectedId;
    messageRequestRef.current += 1;
    setSending(true);
    setError(null);
    try {
      const message = await sendOperatorMessage(auth, conversationId, body, replySender, "markdown");
      setDrafts((current) => ({ ...current, [conversationId]: "" }));
      if (selectedIdRef.current === conversationId) {
        setMessages((current) =>
          current.some((item) => item.id === message.id) ? current : [...current, message],
        );
        knownMessageIdsRef.current.add(message.id);
        setEnteringMessageId(message.id);
        window.requestAnimationFrame(() => {
          scrollMessagesToEnd(messagesContainerRef.current, "smooth");
        });
        void loadMessages(conversationId, true);
      }
      await loadConversationsRef.current?.(true);
    } catch {
      if (selectedIdRef.current === conversationId) {
        setError("conversations.sendError");
        void loadMessages(conversationId);
      }
    } finally {
      setSending(false);
    }
  }

  async function handleJoin() {
    if (!canReplyConversations || !selectedId || !selected || selected.assigned_to_me || selected.status === "resolved" || joining || returningToAi || sending || uploading) return;
    // Joining over another operator replaces them, the same way joining replaces an AI agent.
    const replacedOperator = selected.operator;
    if (replacedOperator && (!canSuperviseOperators
      || !window.confirm(t("conversations.takeOverConfirm", { name: replacedOperator.display_name })))) return;
    setJoining(true);
    setError(null);
    try {
      const joined = replacedOperator
        ? await takeOverConversation(auth, selectedId)
        : await joinConversation(auth, selectedId);
      setConversations((current) => current.map((conversation) => conversation.id === selectedId
        ? {
            ...conversation,
            status: conversation.status === "new" ? "open" : conversation.status,
            operator: joined.operator,
            ai_agent: conversation.ai_agent
              ? { ...conversation.ai_agent, active: false }
              : null,
            assigned_to_me: true,
          }
        : conversation));
    } catch {
      setError(replacedOperator ? "conversations.takeOverError" : "conversations.joinError");
      await loadConversationsRef.current?.();
    } finally {
      setJoining(false);
    }
  }

  async function handleStatusChange(requestRating = true) {
    if (!canCloseConversations || !selectedId || !selected || sending) return;
    const reopening = selected.status === "resolved";
    setSending(true);
    setError(null);
    try {
      if (reopening) {
        await reopenConversation(auth, selectedId);
        if (selectedIdRef.current === selectedId) {
          setStatusFilter((current) => current === "resolved" ? "active" : current);
        }
      } else {
        await resolveConversation(auth, selectedId, requestRating);
      }
      await loadConversationsRef.current?.(true);
    } catch {
      setError(reopening ? "conversations.reopenError" : "conversations.resolveError");
    } finally {
      setSending(false);
    }
  }

  async function handleReturnToAi() {
    if (
      isDemo
      || !selectedId
      || !canReturnToAi
      || returningToAi
      || joining
      || sending
      || uploading
    ) return;
    setReturningToAi(true);
    setError(null);
    try {
      const returned = await returnConversationToAi(auth, selectedId);
      setConversations((current) => current.map((conversation) =>
        conversation.id === selectedId
          ? {
              ...conversation,
              operator: null,
              ai_agent: returned.ai_agent,
              assigned_to_me: false,
            }
          : conversation,
      ));
    } catch {
      setError("conversations.returnToAiError");
      await loadConversationsRef.current?.();
    } finally {
      setReturningToAi(false);
    }
  }

  async function handleAttachment(event: React.ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (isDemo || !file || !selectedId || !selected?.assigned_to_me || selected.status === "resolved") return;
    if (!isAllowedAttachmentFile(file)) {
      setError("conversations.attachmentInvalid");
      return;
    }
    const conversationId = selectedId;
    messageRequestRef.current += 1;
    setUploading(true);
    setError(null);
    try {
      const message = await uploadOperatorAttachment(auth, conversationId, file);
      if (selectedIdRef.current === conversationId) {
        setMessages((current) =>
          current.some((item) => item.id === message.id) ? current : [...current, message],
        );
        knownMessageIdsRef.current.add(message.id);
        setEnteringMessageId(message.id);
        void loadMessages(conversationId, true);
      }
      await loadConversationsRef.current?.();
    } catch (cause) {
      if (selectedIdRef.current === conversationId) {
        setError(cause instanceof ApiRequestError && cause.status === 503
          ? "conversations.attachmentScanUnavailable"
          : "conversations.attachmentUploadError");
        void loadMessages(conversationId);
      }
    } finally {
      setUploading(false);
    }
  }

  async function handleAttachmentPolicy(enabled: boolean) {
    if (
      isDemo
      || !selectedId
      || !selected?.assigned_to_me
      || selected.status === "resolved"
      || auth.kind !== "session"
    ) return;
    setUpdatingAttachmentPolicy(true);
    setError(null);
    try {
      const attachmentsEnabled = await updateConversationAttachmentPolicy(
        auth,
        selectedId,
        enabled,
      );
      setConversations((current) => current.map((conversation) =>
        conversation.id === selectedId
          ? { ...conversation, widget_attachments_enabled: attachmentsEnabled }
          : conversation,
      ));
    } catch {
      setError("conversations.attachmentPolicyError");
      await loadConversationsRef.current?.();
    } finally {
      setUpdatingAttachmentPolicy(false);
    }
  }

  function closeSelectionOutsideFilters(
    nextStatusFilter: StatusFilter,
    nextChannelFilter: string,
  ) {
    const selectedConversation = conversations.find((conversation) =>
      conversation.id === selectedId
    );
    if (!selectedId || (selectedConversation && conversationMatchesFilters(
      selectedConversation,
      nextStatusFilter,
      nextChannelFilter,
    ))) return;
    setSelectedId(null);
    void navigateRoute([], { replace: true });
  }

  function handleStatusFilterChange(nextStatusFilter: StatusFilter) {
    setLoading(true);
    setStatusFilter(nextStatusFilter);
    if (nextStatusFilter === "resolved") setQueueFilter("all");
    closeSelectionOutsideFilters(nextStatusFilter, channelFilter);
  }

  function handleChannelFilterChange(nextChannelFilter: string) {
    setLoading(true);
    setChannelFilter(nextChannelFilter);
    closeSelectionOutsideFilters(statusFilter, nextChannelFilter);
  }

  function handleInboxChange(inboxId: string) {
    setChannelFilter("all");
    setChannels([]);
    setSearch("");
    setDebouncedSearch("");
    setSelectedId(null);
    setIncludedConversationId(undefined);
    // Leave the open conversation first; its link would otherwise switch back to its inbox.
    if (linkedConversation) void Promise.resolve(navigateRoute([])).then(() => onInboxChange(inboxId));
    else onInboxChange(inboxId);
  }

  function selectConversation(conversationId: string) {
    setSelectedId(conversationId);
    if (activeInboxId) void navigateRoute(conversationSegments(activeInboxId, conversationId));
  }

  function handleQueueFilterChange(nextQueue: QueueFilter) {
    setQueueFilter(nextQueue);
    if (nextQueue === "needs_reply") setStatusFilter("active");
    setLoading(true);
  }

  function handleHistorySelect(conversation: OperatorConversation) {
    setStatusFilter("all");
    setQueueFilter("all");
    setChannelFilter("all");
    setSearch("");
    setDebouncedSearch("");
    setIncludedConversationId(conversation.id);
    setConversations((current) => current.some((item) => item.id === conversation.id)
      ? current : [...current, conversation]);
    selectConversation(conversation.id);
    setDetailsOpen(false);
  }

  function handleDetailsClose() {
    setDetailsOpen(false);
    window.requestAnimationFrame(() => {
      document.querySelector<HTMLButtonElement>(".conversation-details-toggle")?.focus();
    });
  }

  function handleSelectionClose() {
    const closingId = selectedId;
    setSelectedId(null);
    void navigateRoute([]);
    window.requestAnimationFrame(() => {
      const focusTarget = closingId
        ? document.getElementById(`conversation-row-${closingId}`)
        : null;
      (focusTarget ?? document.getElementById(`${instanceId}-conversation-status-filter`))?.focus();
    });
  }

  return (
    <div className="conversations-page">
      <header className="conversation-toolbar">
        <h1>{t("conversations.title")}</h1>
        <label className="compact-field conversation-inbox-filter">
          <span className="visually-hidden">{t("conversations.inbox")}</span>
          <select
            value={activeInboxId ?? ""}
            onChange={(event) => handleInboxChange(event.target.value)}
          >
            {inboxes.map((inbox) => (
              <option value={inbox.id} key={inbox.id}>
                {inbox.name}
              </option>
            ))}
          </select>
        </label>
        <label className="compact-field conversation-channel-filter">
          <span className="visually-hidden">{t("conversations.channel")}</span>
          <select
            aria-label={t("conversations.channelFilter")}
            value={channelFilter}
            onChange={(event) => handleChannelFilterChange(event.target.value)}
          >
            <option value="all">{t("conversations.allChannels")}</option>
            {[...new Map([...channels, ...(availableChannels ?? [])].map((channel) => [channel.id, channel])).values()].map((channel) => (
              <option value={channel.id} key={channel.id}>
                {channel.name} · {channelKindLabel(t, channel.kind)}
              </option>
            ))}
          </select>
        </label>
        <div className="conversation-toolbar-actions">
          <button
            className="icon-button conversation-notifications"
            type="button"
            aria-label={t(notificationsEnabled ? "notifications.disable" : "notifications.enable")}
            title={t(notificationsEnabled ? "notifications.disable" : "notifications.enable")}
            disabled={
              notificationsChanging
              || notificationPermission === "denied"
              || notificationPermission === "unsupported"
            }
            aria-pressed={notificationsEnabled}
            onClick={() => void onNotificationsChange(!notificationsEnabled)}
          >
            {notificationsEnabled ? <Bell size={17} aria-hidden="true" /> : <BellOff size={17} aria-hidden="true" />}
          </button>
          <button
            className="icon-button"
            type="button"
            aria-label={t("conversations.refresh")}
            title={t("conversations.refresh")}
            disabled={loading}
            onClick={() => {
              setLoading(true);
              setError(null);
              void reconcile(false);
            }}
          >
            <RefreshCw size={17} aria-hidden="true" />
          </button>
        </div>
      </header>

      {notificationPermission === "denied" && (
        <div className="admin-notice admin-notice--error" role="status">
          {t("notifications.denied")}
        </div>
      )}
      {notificationPermission === "unsupported" && (
        <div className="admin-notice" role="status">
          {t("notifications.unsupported")}
        </div>
      )}
      {error && <div className="admin-notice admin-notice--error" role="alert">{t(error)}</div>}

      <ConversationLayout className={`conversation-layout${selected ? " conversation-layout--selected" : ""}${detailsOpen ? " conversation-layout--details-open" : ""}`}>
        <section className="conversation-list" aria-label={t("conversations.list")}>
          <div className="conversation-search-row">
            <label className="search-field conversation-search">
              <Search size={16} aria-hidden="true" />
              <span className="visually-hidden">{t("conversations.search")}</span>
              <input
                type="search"
                maxLength={200}
                value={search}
                placeholder={t("conversations.searchPlaceholder")}
                aria-controls={`${instanceId}-conversation-list-results`}
                onChange={(event) => setSearch(event.target.value)}
              />
            </label>
          </div>
          <div className="conversation-queue-tabs" role="group" aria-label={t("conversations.queueFilter")}>
            {(["needs_reply", "mine", "all"] as const).map((queue) => (
              <button
                key={queue}
                type="button"
                aria-pressed={queueFilter === queue}
                onClick={() => handleQueueFilterChange(queue)}
              >
                {t(queue === "needs_reply" ? "conversations.filter.needsReply" : queue === "mine" ? "conversations.filter.mine" : "conversations.filter.all")}
              </button>
            ))}
          </div>
          <div className="list-controls">
            <select
              id={`${instanceId}-conversation-status-filter`}
              aria-label={t("conversations.status")}
              value={statusFilter}
              onChange={(event) => handleStatusFilterChange(event.target.value as StatusFilter)}
            >
              <option value="active">{t("conversations.filter.active")}</option>
              <option value="resolved">{t("conversations.filter.resolved")}</option>
              <option value="all">{t("conversations.filter.all")}</option>
            </select>
            <span
              className="conversation-list-count"
              role="status"
              aria-atomic="true"
              aria-label={t("conversations.resultCount", { count: `${filtered.length}${hasMore ? "+" : ""}` })}
            >
              {filtered.length}{hasMore ? "+" : ""}
            </span>
          </div>
          <div
            ref={listMotion}
            id={`${instanceId}-conversation-list-results`}
            className="conversation-list-scroll"
            aria-busy={loading}
          >
            {filtered.map((conversation) => {
              const isSelected = selectedId === conversation.id;
              const hasUnreadCustomerMessages = !isSelected
                && conversation.unread_customer_messages > 0;
              return (
                <button
                  id={`conversation-row-${conversation.id}`}
                  className={`conversation-row${isSelected ? " conversation-row--active" : ""}${hasUnreadCustomerMessages ? " conversation-row--unread" : ""}`}
                  type="button"
                  key={conversation.id}
                  aria-current={isSelected ? "true" : undefined}
                  onClick={() => selectConversation(conversation.id)}
                >
                  <span className="conversation-row-main">
                    <strong>{conversationName(conversation, t)}</strong>
                    <span className="conversation-row-trailing">
                      <time dateTime={conversation.last_message?.created_at ?? conversation.updated_at}>
                        {formatDate(conversation.last_message?.created_at ?? conversation.updated_at,
                          sameCalendarDay(conversation.last_message?.created_at ?? conversation.updated_at, new Date(now))
                            ? { hour: "2-digit", minute: "2-digit" }
                            : { month: "short", day: "numeric" })}
                      </time>
                      {hasUnreadCustomerMessages && (
                        <span
                          className="conversation-row-unread-count"
                          aria-label={t("conversations.unreadCustomerMessages", {
                            count: conversation.unread_customer_messages,
                          })}
                        >
                          {conversation.unread_customer_messages > 99
                            ? "99+"
                            : conversation.unread_customer_messages}
                        </span>
                      )}
                    </span>
                  </span>
                  <span className="conversation-row-preview">{conversationPreview(conversation, t)}</span>
                  <span className="conversation-row-meta">
                    {conversation.awaiting_reply_since ? (
                      <span className="conversation-row-wait" title={formatDate(conversation.awaiting_reply_since, { month: "long", day: "numeric", hour: "2-digit", minute: "2-digit" })}>
                        <Clock3 size={13} aria-hidden="true" />
                        {waitingLabel(conversation.awaiting_reply_since, now, t)}
                      </span>
                    ) : <span>{conversationStatusLabel(t, conversation.status)}</span>}
                    <ConversationChannelLabel channel={conversation.channel} />
                  </span>
                </button>
              );
            })}
            {!loading && filtered.length === 0 && (
              <div className="list-empty">
                <InboxIcon size={20} />
                {t(debouncedSearch
                  ? "conversations.searchEmpty"
                  : "conversations.empty")}
              </div>
            )}
          </div>
        </section>

        <section
          className={`message-panel${selected ? "" : " message-panel--empty"}`}
          aria-label={t("conversations.messages")}
        >
          {selected ? (
            <>
              <header ref={headerMotion} className="message-panel-header">
                <div>
                  <strong>{conversationName(selected, t)}</strong>
                  <ConversationChannelLabel channel={selected.channel} />
                </div>
                <div className="message-panel-actions">
                  {(selected.operator || selected.ai_agent?.active) && (
                    <div className="conversation-participants" aria-live="polite">
                      {selected.operator && (
                        <ConversationParticipant
                          displayName={selected.operator.display_name}
                          avatarUrl={selected.operator.avatar_url}
                          label={selected.assigned_to_me
                            ? t("conversations.youJoined")
                            : t("conversations.operatorJoined")}
                        />
                      )}
                      {selected.ai_agent?.active && (
                        <ConversationParticipant
                          displayName={selected.ai_agent.display_name}
                          avatarUrl={selected.ai_agent.avatar_url}
                          label={t("conversations.aiAgentJoined")}
                        />
                      )}
                    </div>
                  )}
                  {(canReplyConversations && !selected.operator && selected.status !== "resolved")
                    || canReplyForOperator ? (
                    <button
                      className="primary-button"
                      type="button"
                      disabled={joining || returningToAi || sending || uploading}
                      title={canReplyForOperator && selected.operator
                        ? t("conversations.takeOverHint", { name: selected.operator.display_name })
                        : undefined}
                      onClick={() => void handleJoin()}
                    >
                      <UserPlus size={16} />
                      {joining ? t("conversations.joining") : t(canReplyForOperator
                        ? "conversations.takeOver" : "conversations.join")}
                    </button>
                  ) : null}
                  {canReturnToAi ? (
                    <DemoActionButton
                      className="secondary-button"
                      type="button"
                      disabled={returningToAi || joining || sending || uploading}
                      onClick={() => void handleReturnToAi()}
                    >
                      {returningToAi
                        ? <RefreshCw className="spin" size={16} />
                        : <Bot size={16} />}
                      {returningToAi
                        ? t("conversations.returningToAi")
                        : t("conversations.returnToAi")}
                    </DemoActionButton>
                  ) : null}
                  {canCloseConversations && (
                    <DemoActionButton
                      className="secondary-button"
                      type="button"
                      disabled={sending || joining || returningToAi}
                      onClick={() => void handleStatusChange()}
                    >
                      {selected.status === "resolved"
                        ? <RefreshCw className={sending ? "spin" : undefined} size={16} />
                        : <Check size={16} />}
                      {selected.status === "resolved" ? t("conversations.reopen") : t("conversations.resolve")}
                    </DemoActionButton>
                  )}
                  {canCloseConversations && selected.status !== "resolved" && (
                    <DemoActionButton
                      className="secondary-button"
                      type="button"
                      disabled={sending || joining || returningToAi}
                      onClick={() => void handleStatusChange(false)}
                    >
                      {t("conversations.resolveWithoutRating")}
                    </DemoActionButton>
                  )}
                  <button
                    className="icon-button conversation-details-toggle"
                    type="button"
                    aria-label={t("conversations.details")}
                    title={t("conversations.details")}
                    aria-expanded={detailsOpen}
                    aria-controls="conversation-contact-details"
                    onClick={() => setDetailsOpen((open) => !open)}
                  >
                    <PanelRight size={17} aria-hidden="true" />
                  </button>
                  <button
                    className="icon-button"
                    type="button"
                    aria-label={t("conversations.closeView")}
                    onClick={handleSelectionClose}
                  >
                    <X size={17} aria-hidden="true" />
                  </button>
                </div>
              </header>
              <div ref={setMessagesContainer} className="message-scroll" aria-live="polite">
                {messages.map((message, index) => (
                  <Fragment key={message.id}>
                    {(index === 0 || !sameCalendarDay(message.created_at, new Date(messages[index - 1].created_at))) && (
                      <div className="message-date-divider">
                        <time dateTime={message.created_at}>
                          {sameCalendarDay(message.created_at, new Date(now)) ? `${t("conversations.today")}, ` : ""}
                          {formatDate(message.created_at, { day: "numeric", month: "long", ...(new Date(message.created_at).getFullYear() !== new Date(now).getFullYear() ? { year: "numeric" } : {}) })}
                        </time>
                      </div>
                    )}
                  <article
                    className={`message message--${message.direction}${message.id === enteringMessageId ? " message--entering" : ""}`}
                    key={message.id}
                    onAnimationEnd={() => {
                      setEnteringMessageId((current) => current === message.id ? null : current);
                    }}
                  >
                    {message.is_voice_message && <span className="message-voice-label"><Mic aria-hidden="true" size={13} />{voiceText(locale, "voiceMessage")}</span>}
                    {message.body && <MessageBody body={message.body} format={message.body_format} />}
                    <MessageAttachments
                      attachments={message.attachments}
                      labels={{
                        download: t("conversations.attachmentDownload"),
                        loadVideo: t("conversations.attachmentLoadVideo"),
                        unavailable: t("conversations.attachmentUnavailable"),
                        loadError: t("conversations.attachmentLoadError"),
                        securityChecked: t("conversations.attachmentChecked"),
                      }}
                      loadAttachment={loadAttachment}
                      variant="operator"
                    />
                    <footer>
                      <time dateTime={message.created_at} title={formatDate(message.created_at, { month: "long", day: "numeric", hour: "2-digit", minute: "2-digit" })}>{formatDate(message.created_at, { hour: "2-digit", minute: "2-digit" })}</time>
                      {message.direction === "outbound" && (
                        <span className={`message-status message-status--${message.status}`}>
                          {message.status === "sent" && <Check size={13} aria-hidden="true" />}
                          {(message.status === "delivered" || message.status === "read")
                            && <CheckCheck size={13} aria-hidden="true" />}
                          {messageStatusLabel(t, message.status)}
                        </span>
                      )}
                    </footer>
                  </article>
                  </Fragment>
                ))}
                {messages.length === 0 && (
                  <div className="chat-empty">{t("conversations.noMessages")}</div>
                )}
              </div>
              <ConversationComposer
                isDemo={isDemo}
                auth={auth}
                conversation={selected}
                channelLabel={<ConversationChannelLabel channel={selected.channel} />}
                draft={draft}
                visitorDraft={visitorDraft}
                canReply={canReply && !joining && !returningToAi}
                canReplyAsAi={canReplyAsAi}
                canReplyAsOperator={canReplyAsOperator}
                canReplyForOperator={canReplyForOperator}
                sending={sending}
                uploading={uploading}
                attachmentInputRef={attachmentInputRef}
                onDraftChange={setDraft}
                onSubmit={handleSend}
                onAttachmentChange={(event) => void handleAttachment(event)}
              />
            </>
          ) : (
            <div className="chat-empty">{t("conversations.select")}</div>
          )}
        </section>

        {selected && (
          <ConversationContactDetails
            isDemo={isDemo}
            key={selected.contact_id}
            auth={auth}
            conversation={selected}
            intelligence={visitorIntelligence}
            visitorStatus={visitorStatus}
            realtimeSyncRevision={realtimeSyncRevision}
            channelLabel={<ConversationChannelLabel channel={selected.channel} />}
            canManageContacts={canManageContacts}
            onContactBlockChange={(blocked) => setConversations((items) => items.map((item) => item.contact_id === selected.contact_id ? { ...item, contact: { ...item.contact, is_blocked: blocked } } : item))}
            onSelect={handleHistorySelect}
            onClose={handleDetailsClose}
            attachmentsDisabled={isDemo || !selected.assigned_to_me || selected.status === "resolved" || auth.kind !== "session" || updatingAttachmentPolicy}
            onAttachmentPolicyChange={(enabled) => void handleAttachmentPolicy(enabled)}
          />
        )}
      </ConversationLayout>
    </div>
  );
}
