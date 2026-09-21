/** @jsxImportSource preact */

import type { JSX } from "preact";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import {
  ApiRequestError,
  createWidgetConversation,
  createWidgetRealtimeTicket,
  createWidgetSession,
  downloadWidgetAttachment,
  getWidgetPresentation,
  listWidgetMessages,
  markWidgetMessagesRead,
  openRealtimeSocket,
  rateResolution,
  resolveAvatarUrl,
  sendWidgetMessage,
  updateWidgetContact,
  updateWidgetPresence,
  updateWidgetDraft,
  uploadWidgetAttachment,
  type Conversation,
  type ConversationAiAgent,
  type ConversationOperator,
  type Message,
  type WidgetSession,
  type WidgetLanguage,
  type WidgetLauncher,
} from "./api";
import { MessageAttachments } from "./MessageAttachments";
import { MessageBody } from "./MessageBody";
import { WidgetButtonIcon } from "./WidgetButtonIcon";
import { ATTACHMENT_ACCEPT, isAllowedAttachmentFile } from "./attachment-files";
import {
  widgetMessageTypes,
  collectWidgetClientContext,
  getOrCreateVisitorId,
  isWidgetBootstrapErrorMessage,
  isWidgetBootstrapMessage,
  isWidgetCloseMessage,
  isWidgetThemeMessage,
  isWidgetSiteFontMessage,
  type WidgetSiteFontRequestMessage,
  type WidgetReadyMessage,
  type WidgetResizeMessage,
} from "./widget-bridge";
import { normalizeWidgetLanguage, widgetMessages } from "./widget-i18n";
import {
  enableWidgetNotificationSound,
  playWidgetNotificationSound,
  supportsWidgetNotificationSound,
} from "./widget-notification-sound";
import {
  normalizeWidgetColorScheme,
  normalizeWidgetTheme,
  widgetThemeCssVariables,
  widgetThemePalette,
  type WidgetColorScheme,
} from "./widget-theme";
import { applyWidgetSiteFont, widgetFontStack } from "./widget-font";
import { productNamespace } from "./product-edition";

interface WidgetState {
  session: WidgetSession;
  conversation: Conversation;
}

type WidgetTimelineItem =
  | { kind: "ai_joined"; agent: ConversationAiAgent; occurredAt: string }
  | { kind: "operator_joined"; operator: ConversationOperator; occurredAt: string }
  | { kind: "message"; message: Message; occurredAt: string };

const WIDGET_SOUND_STORAGE_PREFIX = `${productNamespace}:widget-message-sounds:`;
const WIDGET_CONTACT_FORM_STORAGE_PREFIX = `${productNamespace}:widget-contact-form:`;
const LAUNCHER_ATTENTION_CLASSES = [
  "widget-launcher--attention-pulse",
  "widget-launcher--attention-lift",
  "widget-launcher--attention-sway",
] as const;

function defaultLauncher(language: WidgetLanguage): WidgetLauncher {
  return {
    launcher_type: "icon",
    position: "bottom_right",
    label: widgetMessages(language).chatTitle,
    show_greeting: true,
    show_operator_profile: false,
    offset_x: 20,
    offset_y: 20,
    attention_animation: "pulse",
    animation_interval_seconds: 5,
    proactive_invitation_enabled: false,
    proactive_invitation_delay_seconds: 15,
  };
}

function widgetId(): string | null {
  return (
    new URLSearchParams(window.location.search).get("widget_id") ??
    import.meta.env.VITE_WIDGET_ID ??
    null
  );
}

function widgetSoundStorageKey(): string {
  return `${WIDGET_SOUND_STORAGE_PREFIX}${widgetId() ?? "default"}`;
}

function widgetContactFormStorageKey(): string {
  return `${WIDGET_CONTACT_FORM_STORAGE_PREFIX}${widgetId() ?? "default"}`;
}

function initialContactFormDismissedConversationId(): string | null {
  try {
    return window.localStorage.getItem(widgetContactFormStorageKey());
  } catch {
    return null;
  }
}

function storeContactFormDismissal(conversationId: string): void {
  try {
    window.localStorage.setItem(widgetContactFormStorageKey(), conversationId);
  } catch {
    // Storage may be blocked for third-party iframes; dismissal still works for this page view.
  }
}

function initialWidgetSoundEnabled(): boolean {
  try {
    return window.localStorage.getItem(widgetSoundStorageKey()) !== "off";
  } catch {
    return true;
  }
}

function storeWidgetSoundPreference(enabled: boolean): void {
  try {
    window.localStorage.setItem(widgetSoundStorageKey(), enabled ? "on" : "off");
  } catch {
    // Storage may be blocked for third-party iframes; the in-memory preference still works.
  }
}

function supportInitials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  const initials = parts
    .slice(0, 2)
    .map((part) => Array.from(part)[0] ?? "")
    .join("")
    .toLocaleUpperCase();
  return initials || "?";
}

function WidgetParticipantAvatar({ name, avatarUrl }: { name: string; avatarUrl: string | null }) {
  const [failedAvatarUrl, setFailedAvatarUrl] = useState<string | null>(null);
  const resolvedAvatarUrl = resolveAvatarUrl(avatarUrl);
  return (
    <span className="widget-participant-avatar" aria-hidden="true">
      {resolvedAvatarUrl && failedAvatarUrl !== resolvedAvatarUrl ? (
        <img
          src={resolvedAvatarUrl}
          alt=""
          referrerPolicy="no-referrer"
          onError={() => setFailedAvatarUrl(resolvedAvatarUrl)}
        />
      ) : (
        supportInitials(name)
      )}
    </span>
  );
}

async function initializeWidget(session: WidgetSession): Promise<WidgetState> {
  const presentation = await getWidgetPresentation(session.token);
  const presentedSession = { ...session, ...presentation };
  const conversation = await createWidgetConversation(presentedSession.token);
  return { session: presentedSession, conversation };
}

function requestedLanguage(): WidgetLanguage | undefined {
  return normalizeWidgetLanguage(new URLSearchParams(window.location.search).get("lang"));
}

function requestedColorScheme(): WidgetColorScheme {
  return normalizeWidgetColorScheme(
    new URLSearchParams(window.location.search).get("theme"),
  ) ?? "light";
}

function formatTime(value: string, language: WidgetLanguage): string {
  const primary = language.split("-", 1)[0].toLowerCase();
  return new Intl.DateTimeFormat(primary === "ru" ? "ru-RU" : primary === "ro" ? "ro-RO" : "en-US", {
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function buildWidgetTimeline(
  messages: Message[],
  aiAgent: ConversationAiAgent | null,
  operator: ConversationOperator | null,
): WidgetTimelineItem[] {
  const items: WidgetTimelineItem[] = messages.map((message) => ({
    kind: "message",
    message,
    occurredAt: message.created_at,
  }));
  if (aiAgent) {
    items.push({ kind: "ai_joined", agent: aiAgent, occurredAt: aiAgent.joined_at });
  }
  if (operator) {
    items.push({ kind: "operator_joined", operator, occurredAt: operator.joined_at });
  }
  return items.sort((left, right) => {
    const timeDifference = Date.parse(left.occurredAt) - Date.parse(right.occurredAt);
    if (timeDifference !== 0) return timeDifference;
    if (left.kind === "message" && right.kind === "message") {
      return left.message.sequence - right.message.sequence;
    }
    const rank = (item: WidgetTimelineItem) => {
      if (item.kind === "message") return 0;
      return item.kind === "ai_joined" ? 1 : 2;
    };
    return rank(left) - rank(right);
  });
}

function ParticipantJoinedNotice({
  participant,
  language,
  label,
}: {
  participant: ConversationAiAgent | ConversationOperator;
  language: WidgetLanguage;
  label: string;
}) {
  return (
    <div className="widget-participant-event" data-participant="operator">
      <WidgetParticipantAvatar name={participant.display_name} avatarUrl={participant.avatar_url} />
      <span>{label}</span>
      <time>{formatTime(participant.joined_at, language)}</time>
    </div>
  );
}

function CloseGlyph() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24">
      <path d="m7 7 10 10M17 7 7 17" />
    </svg>
  );
}

function SoundGlyph({ muted }: { muted: boolean }) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24">
      <path d="M5 10v4h3l4 3V7l-4 3H5Z" />
      {muted ? (
        <path d="m16 10 4 4m0-4-4 4" />
      ) : (
        <path d="M16 9.5a4 4 0 0 1 0 5m2-7.5a7 7 0 0 1 0 10" />
      )}
    </svg>
  );
}

function ErrorGlyph() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24">
      <path d="M12 8v5m0 3h.01" />
      <circle cx="12" cy="12" r="9" />
    </svg>
  );
}

function StarGlyph() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24">
      <path d="m12 2.8 2.75 5.57 6.15.9-4.45 4.33 1.05 6.12L12 16.83l-5.5 2.89 1.05-6.12L3.1 9.27l6.15-.9L12 2.8Z" />
    </svg>
  );
}

function FeedbackCheckGlyph() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24">
      <path d="m6.5 12.5 3.25 3.25 7.75-8" />
      <circle cx="12" cy="12" r="9" />
    </svg>
  );
}

function publicWidgetError(cause: unknown, fallback: string): string {
  if (!(cause instanceof Error)) return fallback;
  const normalized = cause.message.trim().toLowerCase();
  if (
    normalized === "load failed"
    || normalized.includes("failed to fetch")
    || normalized.includes("networkerror")
  ) {
    return fallback;
  }
  return cause.message;
}

function scrollMessagesToEnd(
  container: HTMLDivElement | null,
  behavior: ScrollBehavior = "auto",
) {
  if (!container) return;
  const reducedMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
  container.scrollTo({
    top: container.scrollHeight,
    behavior: reducedMotion ? "auto" : behavior,
  });
}

function PaperclipGlyph() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="m20.5 11.5-8.1 8.1a6 6 0 0 1-8.5-8.5l9-9a4 4 0 0 1 5.7 5.7l-9.1 9.1a2 2 0 0 1-2.8-2.8l8.4-8.4" />
    </svg>
  );
}

export function App() {
  const messagesContainerRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const attachmentInputRef = useRef<HTMLInputElement>(null);
  const launcherButtonRef = useRef<HTMLButtonElement>(null);
  const notifiedMessageIdsRef = useRef(new Set<string>());
  const seenMessageIdsRef = useRef(new Set<string>());
  const readMessageIdsRef = useRef(new Set<string>());
  const smoothScrollMessageIdRef = useRef<string | null>(null);
  const focusComposerAfterResolutionRef = useRef(false);
  const proactiveInvitationHandledRef = useRef(window.parent === window);
  const [state, setState] = useState<WidgetState | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [enteringMessageId, setEnteringMessageId] = useState<string | null>(null);
  const [typingMessageIds, setTypingMessageIds] = useState(new Set<string>());
  const [draft, setDraft] = useState("");
  const [loading, setLoading] = useState(true);
  const [sending, setSending] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [resolutionId, setResolutionId] = useState<string | null>(null);
  const [selectedRating, setSelectedRating] = useState<number | null>(null);
  const [hoveredRating, setHoveredRating] = useState<number | null>(null);
  const [selectedReasons, setSelectedReasons] = useState<string[]>([]);
  const [ratingComment, setRatingComment] = useState("");
  const [ratingSubmitting, setRatingSubmitting] = useState(false);
  const [rated, setRated] = useState(false);
  const [open, setOpen] = useState(() => window.parent === window);
  const [closing, setClosing] = useState(false);
  const [colorScheme, setColorScheme] = useState(requestedColorScheme);
  const [parentOrigin, setParentOrigin] = useState<string | null>(null);
  const [siteFontFamily, setSiteFontFamily] = useState<string | null>(null);
  const [soundEnabled, setSoundEnabled] = useState(initialWidgetSoundEnabled);
  const [proactiveInvitationVisible, setProactiveInvitationVisible] = useState(false);
  const [contactName, setContactName] = useState("");
  const [contactEmail, setContactEmail] = useState("");
  const [contactSaving, setContactSaving] = useState(false);
  const [contactError, setContactError] = useState<string | null>(null);
  const [contactFormDismissedConversationId, setContactFormDismissedConversationId] = useState(
    initialContactFormDismissedConversationId,
  );
  const soundEnabledRef = useRef(soundEnabled);
  const language = state?.session.language ?? requestedLanguage() ?? "en";
  const text = widgetMessages(language);
  const onlineNowText = state?.session.online_now || text.onlineNow;
  const offlineNowText = state?.session.offline_now || text.offlineNow;
  const messagePlaceholder = state?.session.message_placeholder || text.placeholder;
  const contactTitle = state?.session.contact_title || text.contactTitle;
  const contactDescription = state?.session.contact_description || text.contactDescription;
  const contactNameLabel = state?.session.contact_name || text.contactName;
  const contactNamePlaceholder = state?.session.contact_name_placeholder || text.contactNamePlaceholder;
  const contactEmailLabel = state?.session.contact_email || text.contactEmail;
  const contactEmailPlaceholder = state?.session.contact_email_placeholder || text.contactEmailPlaceholder;
  const contactSave = state?.session.contact_save || text.contactSave;
  const contactSavingText = state?.session.contact_saving || text.contactSaving;
  const contactSkip = state?.session.contact_skip || text.contactSkip;
  const contactErrorText = state?.session.contact_error || text.contactError;
  const theme = normalizeWidgetTheme(state?.session.theme);
  const palette = widgetThemePalette(theme, colorScheme);
  const footerText = theme.footer_text ?? text.footer;
  const operatorsOnline = state?.session.operators_online ?? false;
  const soundSupported = supportsWidgetNotificationSound();
  const sessionToken = state?.session.token;
  const launcher = useMemo(
    () => state?.session.launcher ?? defaultLauncher(language),
    [language, state?.session.launcher],
  );
  const launcherAttentionAnimation = !launcher.show_greeting
    && launcher.attention_animation === "pulse"
    ? "lift"
    : launcher.attention_animation;
  const conversationOperator = launcher.show_operator_profile
    ? state?.conversation.operator ?? null
    : null;
  const conversationAiAgent = state?.conversation.ai_agent ?? null;
  const activeAiAgent = conversationAiAgent?.active ? conversationAiAgent : null;
  const activeParticipant = conversationOperator ?? activeAiAgent;
  const supportName = conversationOperator?.display_name
    ?? activeAiAgent?.display_name
    ?? (state?.session.support_name.trim() || text.support);
  const supportAvatarUrl = activeParticipant?.avatar_url ?? null;
  const supportAvailable = activeParticipant !== null || operatorsOnline;
  const supportStatus = conversationOperator
    ? text.joinedChat
    : activeAiAgent
      ? text.joinedChat
      : operatorsOnline
        ? onlineNowText
        : offlineNowText;
  const welcomeStatus = conversationOperator
    ? text.joinedChat
    : activeAiAgent
      ? text.joinedChat
      : operatorsOnline
        ? text.online
        : text.offline;
  const timeline = useMemo(
    () => buildWidgetTimeline(messages, conversationAiAgent, conversationOperator),
    [conversationAiAgent, conversationOperator, messages],
  );
  const callToActionBody = proactiveInvitationVisible
    ? state?.session.proactive_invitation_message ?? text.proactiveInvitation
    : null;
  const callToActionVisible = !open && callToActionBody !== null;
  const contactProfileComplete = Boolean(
    state?.session.contact.display_name?.trim() && state.session.contact.email?.trim(),
  );
  const contactMessageAuthor = state?.session.contact.display_name?.trim();
  const contactFormVisible = state !== null
    && !contactProfileComplete
    && contactFormDismissedConversationId !== state.conversation.id;
  const themeStyle = {
    fontFamily: theme.use_site_font && siteFontFamily
      ? `${siteFontFamily}, ${widgetFontStack(theme.font_family)}`
      : widgetFontStack(theme.font_family),
    ...widgetThemeCssVariables(theme, colorScheme),
  } as JSX.CSSProperties;

  useEffect(() => {
    if (!theme.use_site_font || !parentOrigin || window.parent === window) return;
    let disposeFont: (() => void) | undefined;
    const receiveFont = (event: MessageEvent<unknown>) => {
      if (event.source !== window.parent || event.origin !== parentOrigin || !isWidgetSiteFontMessage(event.data)) return;
      disposeFont?.();
      setSiteFontFamily(null);
      if (event.data.font) disposeFont = applyWidgetSiteFont(event.data.font, setSiteFontFamily);
    };
    window.addEventListener("message", receiveFont);
    const request: WidgetSiteFontRequestMessage = { type: widgetMessageTypes.siteFontRequest, version: 1 };
    window.parent.postMessage(request, parentOrigin);
    return () => {
      window.removeEventListener("message", receiveFont);
      disposeFont?.();
    };
  }, [parentOrigin, theme.use_site_font]);

  useEffect(() => {
    document.documentElement.lang = language;
  }, [language]);

  useEffect(() => {
    soundEnabledRef.current = soundEnabled;
  }, [soundEnabled]);

  useEffect(() => {
    if (typingMessageIds.size === 0) return;
    const finishTyping = () => setTypingMessageIds(new Set());
    if (!open || closing || !theme.reply_typing_effect) {
      finishTyping();
      return;
    }
    // Keep the accessible text nodes stable when the CSS animation finishes.
    // Clear before hiding the chat so opening history never replays it.
    const handleVisibility = () => {
      if (document.visibilityState !== "visible") finishTyping();
    };
    document.addEventListener("visibilitychange", handleVisibility);
    return () => {
      document.removeEventListener("visibilitychange", handleVisibility);
    };
  }, [typingMessageIds, open, closing, theme.reply_typing_effect]);

  useEffect(() => {
    const enableSoundAfterInteraction = () => {
      if (soundEnabledRef.current) void enableWidgetNotificationSound();
    };
    window.addEventListener("pointerdown", enableSoundAfterInteraction, { passive: true });
    window.addEventListener("keydown", enableSoundAfterInteraction);
    return () => {
      window.removeEventListener("pointerdown", enableSoundAfterInteraction);
      window.removeEventListener("keydown", enableSoundAfterInteraction);
    };
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = colorScheme;
    document.documentElement.style.colorScheme = colorScheme;
    const themeColor = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]');
    if (themeColor) themeColor.content = palette.surface_color;
  }, [colorScheme, palette.surface_color]);

  const latestMessageId = messages.at(-1)?.id;

  useEffect(() => {
    const container = messagesContainerRef.current;
    if (!state || loading || !open || closing || !container) return undefined;
    const { token } = state.session;
    const conversationId = state.conversation.id;
    const unreadIds = new Set(messages
      .filter((message) => message.direction === "outbound"
        && message.status !== "read" && message.status !== "failed")
      .map((message) => message.id));

    const reportVisibleMessages = () => {
      if (document.visibilityState !== "visible") return;
      const viewport = container.getBoundingClientRect();
      const top = Math.max(0, viewport.top);
      const bottom = Math.min(window.innerHeight, viewport.bottom);
      const left = Math.max(0, viewport.left);
      const right = Math.min(window.innerWidth, viewport.right);
      const ids: string[] = [];
      container.querySelectorAll<HTMLElement>("[data-read-message-id]").forEach((element) => {
        const id = element.dataset.readMessageId!;
        if (!unreadIds.has(id) || readMessageIdsRef.current.has(id)) return;
        const bounds = element.getBoundingClientRect();
        // Seeing the timestamp confirms the visitor reached the end of the bubble.
        if (bounds.height > 0 && bounds.width > 0 && bounds.top >= top
          && bounds.bottom <= bottom && bounds.left >= left && bounds.right <= right) {
          ids.push(id);
        }
      });
      if (ids.length === 0) return;
      const batch = ids.slice(0, 100);
      batch.forEach((id) => readMessageIdsRef.current.add(id));
      void markWidgetMessagesRead(token, conversationId, batch).catch(() => {
        // Retry on the next visibility check without interrupting the conversation.
        batch.forEach((id) => readMessageIdsRef.current.delete(id));
      });
    };

    const frame = window.requestAnimationFrame(reportVisibleMessages);
    const retry = window.setInterval(reportVisibleMessages, 5_000);
    container.addEventListener("scroll", reportVisibleMessages, { passive: true });
    window.addEventListener("resize", reportVisibleMessages);
    window.addEventListener("focus", reportVisibleMessages);
    document.addEventListener("visibilitychange", reportVisibleMessages);
    const observer = typeof ResizeObserver === "undefined"
      ? undefined : new ResizeObserver(reportVisibleMessages);
    observer?.observe(container);
    return () => {
      window.cancelAnimationFrame(frame);
      window.clearInterval(retry);
      container.removeEventListener("scroll", reportVisibleMessages);
      window.removeEventListener("resize", reportVisibleMessages);
      window.removeEventListener("focus", reportVisibleMessages);
      document.removeEventListener("visibilitychange", reportVisibleMessages);
      observer?.disconnect();
    };
  }, [state, messages, loading, open, closing]);

  const messageScrollTrigger = latestMessageId ?? state?.conversation.id;

  useEffect(() => {
    if (!open || !messageScrollTrigger) return undefined;
    const smooth = smoothScrollMessageIdRef.current === latestMessageId;
    const frame = window.requestAnimationFrame(() => {
      scrollMessagesToEnd(messagesContainerRef.current, smooth ? "smooth" : "auto");
      if (smoothScrollMessageIdRef.current === latestMessageId) {
        smoothScrollMessageIdRef.current = null;
      }
    });
    return () => window.cancelAnimationFrame(frame);
  }, [latestMessageId, messageScrollTrigger, open]);

  useEffect(() => {
    if (!resolutionId) return undefined;
    const frame = window.requestAnimationFrame(() => {
      scrollMessagesToEnd(messagesContainerRef.current, "smooth");
    });
    return () => window.cancelAnimationFrame(frame);
  }, [resolutionId]);

  useEffect(() => {
    if (resolutionId !== null || !focusComposerAfterResolutionRef.current) return undefined;
    focusComposerAfterResolutionRef.current = false;
    const frame = window.requestAnimationFrame(() => composerRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [resolutionId]);

  useEffect(() => {
    if (window.parent === window) return;
    const message: WidgetResizeMessage = {
      type: widgetMessageTypes.resize,
      version: 1,
      open,
      call_to_action_visible: callToActionVisible,
      launcher,
    };
    window.parent.postMessage(message, parentOrigin ?? "*");
  }, [callToActionVisible, launcher, open, parentOrigin]);

  useEffect(() => {
    const button = launcherButtonRef.current;
    if (
      open
      || !button
      || launcherAttentionAnimation === "none"
      || window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true
    ) {
      return undefined;
    }
    const animationClass = `widget-launcher--attention-${launcherAttentionAnimation}`;
    const replayAnimation = () => {
      button.classList.remove(...LAUNCHER_ATTENTION_CLASSES);
      void button.offsetWidth;
      button.classList.add(animationClass);
    };
    replayAnimation();
    const timer = window.setInterval(
      replayAnimation,
      launcher.animation_interval_seconds * 1_000,
    );
    return () => {
      window.clearInterval(timer);
      button.classList.remove(...LAUNCHER_ATTENTION_CLASSES);
    };
  }, [
    launcher.animation_interval_seconds,
    launcherAttentionAnimation,
    open,
    state?.conversation.id,
  ]);

  useEffect(() => {
    if (!closing) return undefined;
    const timer = window.setTimeout(() => {
      setOpen(false);
      setClosing(false);
    }, 180);
    return () => window.clearTimeout(timer);
  }, [closing]);

  useEffect(() => {
    const conversationId = state?.conversation.id;
    if (
      !conversationId
      || open
      || !launcher.proactive_invitation_enabled
      || !operatorsOnline
      || messages.length > 0
      || proactiveInvitationHandledRef.current
    ) {
      setProactiveInvitationVisible(false);
      return undefined;
    }

    const timer = window.setTimeout(() => {
      if (proactiveInvitationHandledRef.current) return;
      proactiveInvitationHandledRef.current = true;
      setProactiveInvitationVisible(true);
    }, launcher.proactive_invitation_delay_seconds * 1_000);
    return () => window.clearTimeout(timer);
  }, [
    launcher.proactive_invitation_delay_seconds,
    launcher.proactive_invitation_enabled,
    messages.length,
    open,
    operatorsOnline,
    state?.conversation.id,
  ]);

  useEffect(() => {
    let active = true;
    let started = false;
    let verifiedParentOrigin: string | null = null;
    const initialText = widgetMessages(requestedLanguage() ?? "en");
    const loadSession = (session: WidgetSession) => {
      if (started) return;
      started = true;
      void initializeWidget(session).then(async (nextState) => {
        const history = await listWidgetMessages(
          nextState.session.token,
          nextState.conversation.id,
        );
        if (active) {
          setContactName(nextState.session.contact.display_name ?? "");
          setContactEmail(nextState.session.contact.email ?? "");
          setState(nextState);
          for (const message of history) seenMessageIdsRef.current.add(message.id);
          setMessages(history);
        }
      })
      .catch((cause: unknown) => {
        if (active) setError(publicWidgetError(cause, initialText.openError));
      })
      .finally(() => { if (active) setLoading(false); });
    };

    if (window.parent === window) {
      const id = widgetId();
      if (!id) {
        setError(initialText.missingId);
        setLoading(false);
        return () => { active = false; };
      }
      void createWidgetSession(
        id,
        getOrCreateVisitorId(id),
        requestedLanguage(),
        collectWidgetClientContext(),
      ).then(loadSession).catch((cause: unknown) => {
        if (active) {
          setError(publicWidgetError(cause, initialText.openError));
          setLoading(false);
        }
      });
      return () => { active = false; };
    }

    const handleBootstrap = (event: MessageEvent<unknown>) => {
      if (event.source !== window.parent) return;
      if (isWidgetThemeMessage(event.data)) {
        if (verifiedParentOrigin !== null && event.origin !== verifiedParentOrigin) return;
        setColorScheme(event.data.theme);
        return;
      }
      if (isWidgetBootstrapMessage(event.data)) {
        if (event.origin !== event.data.embed_origin) return;
        verifiedParentOrigin = event.origin;
        setParentOrigin(event.origin);
        loadSession(event.data.session);
        return;
      }
      if (
        isWidgetCloseMessage(event.data)
        && verifiedParentOrigin !== null
        && event.origin === verifiedParentOrigin
      ) {
        setClosing(true);
        return;
      }
      if (isWidgetBootstrapErrorMessage(event.data)) {
        setError(event.data.message);
        setLoading(false);
      }
    };
    window.addEventListener("message", handleBootstrap);
    const ready: WidgetReadyMessage = {
      type: widgetMessageTypes.ready,
      version: 1,
    };
    window.parent.postMessage(ready, "*");
    return () => {
      active = false;
      window.removeEventListener("message", handleBootstrap);
    };
  }, []);

  useEffect(() => {
    if (!sessionToken) return undefined;
    let cancelled = false;
    const heartbeat = () => {
      void updateWidgetPresence(sessionToken)
        .then((availability) => {
          if (cancelled) return;
          setState((current) => {
            if (!current || current.session.operators_online === availability.operators_online) {
              return current;
            }
            return {
              ...current,
              session: {
                ...current.session,
                operators_online: availability.operators_online,
              },
            };
          });
        })
        .catch(() => {
          // Presence is best effort; chat requests remain independently usable.
        });
    };
    const handleVisibility = () => {
      if (!cancelled && document.visibilityState === "visible") heartbeat();
    };
    heartbeat();
    const timer = window.setInterval(heartbeat, 25_000);
    document.addEventListener("visibilitychange", handleVisibility);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", handleVisibility);
    };
  }, [sessionToken]);

  useEffect(() => {
    if (!state) return undefined;
    const timer = window.setTimeout(() => {
      void updateWidgetDraft(
        state.session.token,
        state.conversation.id,
        draft,
      ).catch(() => {
        // Draft visibility is best effort and must never block the visitor's chat.
      });
    }, 120);
    return () => window.clearTimeout(timer);
  }, [draft, state]);

  useEffect(() => {
    const token = state?.session.token;
    if (!token) return undefined;
    let cancelled = false;
    const refreshPresentation = () => {
      void getWidgetPresentation(token)
        .then((presentation) => {
          if (cancelled) return;
          setState((current) => current === null
            ? current
            : { ...current, session: { ...current.session, ...presentation } });
        })
        .catch(() => {
          // Keep the last known presentation when a background refresh fails.
        });
    };
    const handleVisibility = () => {
      if (document.visibilityState === "visible") refreshPresentation();
    };
    window.addEventListener("focus", refreshPresentation);
    document.addEventListener("visibilitychange", handleVisibility);
    return () => {
      cancelled = true;
      window.removeEventListener("focus", refreshPresentation);
      document.removeEventListener("visibilitychange", handleVisibility);
    };
  }, [state?.session.token]);

  useEffect(() => {
    if (!state) return undefined;
    let cancelled = false;
    let socket: WebSocket | undefined;
    let reconnectTimer: number | undefined;

    const refresh = async (enteringOperatorMessageId?: string) => {
      const history = await listWidgetMessages(state.session.token, state.conversation.id);
      if (cancelled) return;
      const enteringOperatorMessage = enteringOperatorMessageId
        ? history.find((message) => message.id === enteringOperatorMessageId)
        : undefined;
      if (enteringOperatorMessage) {
        smoothScrollMessageIdRef.current = enteringOperatorMessage.id;
        setEnteringMessageId(enteringOperatorMessage.id);
      }
      if (
        enteringOperatorMessage
        && state.session.theme.reply_typing_effect === true
        && document.visibilityState === "visible"
        && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches !== true
      ) {
        const newReplies = history.filter((message) => !seenMessageIdsRef.current.has(message.id)
          && message.direction === "outbound"
          && (message.author_kind === "operator" || message.author_kind === "ai")
          && message.body);
        if (newReplies.length > 0) {
          setTypingMessageIds((current) => new Set([...current, ...newReplies.map((message) => message.id)]));
        }
      }
      for (const message of history) seenMessageIdsRef.current.add(message.id);
      setMessages(history);
    };
    const refreshConversation = async () => {
      const conversation = await createWidgetConversation(state.session.token);
      if (cancelled) return;
      if (conversation.id !== state.conversation.id) {
        const history = await listWidgetMessages(state.session.token, conversation.id);
        if (cancelled) return;
        for (const message of history) seenMessageIdsRef.current.add(message.id);
        setTypingMessageIds(new Set());
        setMessages(history);
        setResolutionId(null);
      }
      setState((current) => current === null ? current : { ...current, conversation });
    };
    const connect = async () => {
      try {
        const ticket = await createWidgetRealtimeTicket(state.session.token);
        if (cancelled) return;
        socket = openRealtimeSocket(
          ticket,
          (event) => {
            if (event.type === "conversation.created") {
              void refreshConversation();
              return;
            }
            if (event.type === "conversation.resolved") {
              const id = event.data.resolution_id;
              if (typeof id === "string") {
                setResolutionId(id);
                setSelectedRating(null);
                setSelectedReasons([]);
                setRatingComment("");
                setRated(false);
              }
            }
            if (event.data.conversation_id !== state.conversation.id) return;
            if (event.type === "conversation.reopened") {
              setResolutionId(null);
              void refreshConversation();
              return;
            }
            if (
              event.type === "conversation.operator_joined"
              || event.type === "conversation.operator_left"
              || event.type === "conversation.ai_joined"
              || event.type === "conversation.ai_left"
              || event.type === "conversation.attachment_policy_changed"
            ) {
              void refreshConversation();
              return;
            }
            if (event.type === "message.created" && event.data.direction === "outbound") {
              const messageId = typeof event.data.message_id === "string"
                ? event.data.message_id
                : event.aggregate_id;
              if (!notifiedMessageIdsRef.current.has(messageId)) {
                notifiedMessageIdsRef.current.add(messageId);
                if (soundEnabledRef.current) playWidgetNotificationSound();
                handleOpenWidget();
              }
              void refresh(messageId);
              return;
            }
            void refresh();
          },
          () => {
            if (!cancelled) reconnectTimer = window.setTimeout(() => void connect(), 2_000);
          },
        );
      } catch {
        if (!cancelled) reconnectTimer = window.setTimeout(() => void connect(), 5_000);
      }
    };
    void connect();
    return () => {
      cancelled = true;
      if (reconnectTimer !== undefined) window.clearTimeout(reconnectTimer);
      socket?.close();
    };
  }, [state]);

  async function handleSend(event: SubmitEvent) {
    event.preventDefault();
    const body = draft.trim();
    if (!state || !body || sending) return;
    setSending(true);
    setError(null);
    try {
      const message = await sendWidgetMessage(
        state.session.token,
        state.conversation.id,
        body,
      );
      smoothScrollMessageIdRef.current = message.id;
      setMessages((current) =>
        current.some((item) => item.id === message.id) ? current : [...current, message],
      );
      setEnteringMessageId(message.id);
      setDraft("");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : text.sendError);
    } finally {
      setSending(false);
    }
  }

  async function handleAttachment(event: JSX.TargetedEvent<HTMLInputElement, Event>) {
    const file = event.currentTarget.files?.[0];
    event.currentTarget.value = "";
    if (
      !file
      || !state
      || !state.conversation.widget_attachments_enabled
      || uploading
      || resolutionId
    ) return;
    if (!isAllowedAttachmentFile(file)) {
      setError(text.attachmentInvalid);
      return;
    }
    setUploading(true);
    setError(null);
    try {
      const message = await uploadWidgetAttachment(
        state.session.token,
        state.conversation.id,
        file,
      );
      smoothScrollMessageIdRef.current = message.id;
      setMessages((current) =>
        current.some((item) => item.id === message.id) ? current : [...current, message],
      );
      setEnteringMessageId(message.id);
    } catch (cause) {
      setError(cause instanceof ApiRequestError && cause.status === 503
        ? text.attachmentScanUnavailable
        : text.attachmentUploadError);
    } finally {
      setUploading(false);
    }
  }

  async function handleContactSubmit(event: SubmitEvent) {
    event.preventDefault();
    const displayName = contactName.trim();
    const email = contactEmail.trim();
    if (!state || !displayName || !email || contactSaving) return;
    setContactSaving(true);
    setContactError(null);
    try {
      const contact = await updateWidgetContact(state.session.token, {
        display_name: displayName,
        email,
      });
      setState((current) => current === null
        ? current
        : { ...current, session: { ...current.session, contact } });
    } catch {
      setContactError(contactErrorText);
    } finally {
      setContactSaving(false);
    }
  }

  function handleDismissContactForm() {
    if (!state) return;
    setContactFormDismissedConversationId(state.conversation.id);
    setContactError(null);
    storeContactFormDismissal(state.conversation.id);
  }

  function handleComposerKeyDown(event: JSX.TargetedKeyboardEvent<HTMLTextAreaElement>) {
    if (event.key !== "Enter" || event.shiftKey || event.isComposing) return;
    event.preventDefault();
    if (!sending && draft.trim()) event.currentTarget.form?.requestSubmit();
  }

  function handleSoundToggle() {
    const enabled = !soundEnabledRef.current;
    soundEnabledRef.current = enabled;
    setSoundEnabled(enabled);
    storeWidgetSoundPreference(enabled);
    if (enabled) void enableWidgetNotificationSound();
  }

  function handleOpenWidget() {
    proactiveInvitationHandledRef.current = true;
    setProactiveInvitationVisible(false);
    setClosing(false);
    setOpen(true);
  }

  function handleDismissCallToAction() {
    proactiveInvitationHandledRef.current = true;
    setProactiveInvitationVisible(false);
  }

  function selectRating(rating: number) {
    setSelectedRating(rating);
    setSelectedReasons([]);
    setError(null);
  }

  function toggleRatingReason(reason: string) {
    setSelectedReasons((current) => current.includes(reason)
      ? current.filter((item) => item !== reason)
      : [...current, reason]);
  }

  async function handleFeedbackSubmit() {
    if (!state || !resolutionId || !selectedRating || ratingSubmitting) return;
    setRatingSubmitting(true);
    setError(null);
    try {
      await rateResolution(
        state.session.token,
        resolutionId,
        selectedRating,
        selectedReasons,
        ratingComment.trim() || null,
      );
      setRated(true);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : text.ratingError);
    } finally {
      setRatingSubmitting(false);
    }
  }

  function handleContinueConversation() {
    focusComposerAfterResolutionRef.current = true;
    setResolutionId(null);
    setSelectedRating(null);
    setHoveredRating(null);
    setSelectedReasons([]);
    setRatingComment("");
    setRated(false);
    setError(null);
  }

  if (!open && window.parent !== window && !state) return null;

  if (!open) {
    return (
      <div className={`widget-stage widget-stage--closed${launcher.position === "bottom_left" ? " widget-stage--bottom-left" : ""}`} data-theme={colorScheme} style={themeStyle}>
        <div className="widget-launcher-stack">
          {callToActionVisible && callToActionBody && (
            <div className="widget-operator-cta" aria-live="polite">
              <button
                type="button"
                className="widget-operator-cta-content"
                aria-label={text.openInvitation}
                onClick={handleOpenWidget}
              >
                <strong>{supportName}</strong>
                <span>{callToActionBody}</span>
              </button>
              <button
                type="button"
                className="widget-operator-cta-close"
                aria-label={text.dismissInvitation}
                onClick={handleDismissCallToAction}
              >
                <CloseGlyph />
              </button>
            </div>
          )}
          <button
            ref={launcherButtonRef}
            type="button"
            className={`widget-launcher widget-launcher--${launcher.launcher_type}`}
            aria-label={launcher.label}
            aria-expanded="false"
            onClick={handleOpenWidget}
          >
            {launcher.launcher_type !== "text" && <WidgetButtonIcon icon={theme.launcher_icon} />}
            {launcher.launcher_type !== "icon" && <span>{launcher.label}</span>}
            <span className={`launcher-status${operatorsOnline ? "" : " launcher-status--offline"}`} />
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className={`widget-stage widget-stage--open${launcher.position === "bottom_left" ? " widget-stage--bottom-left" : ""}`} data-theme={colorScheme} style={themeStyle}>
      <main className={`widget-shell${closing ? " widget-shell--closing" : ""}`}>
        <header className="widget-header">
          <div className={`widget-brand-mark${activeParticipant ? " widget-brand-mark--participant" : ""}`}>
            {activeParticipant
              ? <WidgetParticipantAvatar name={supportName} avatarUrl={supportAvatarUrl} />
              : <WidgetButtonIcon icon="chat" />}
          </div>
          <div className="widget-title">
            <strong>{supportName}</strong>
            <span aria-live="polite">
              <i className={supportAvailable ? undefined : "availability-dot--offline"} />
              {state ? supportStatus : text.connecting}
            </span>
          </div>
          {(soundSupported || window.parent !== window) && (
            <div className="widget-header-actions">
              {soundSupported && (
                <button
                  type="button"
                  className="widget-header-button widget-sound-button"
                  aria-label={text.messageSounds}
                  aria-pressed={soundEnabled}
                  title={soundEnabled ? text.soundsEnabled : text.soundsDisabled}
                  onClick={handleSoundToggle}
                >
                  <SoundGlyph muted={!soundEnabled} />
                </button>
              )}
              {window.parent !== window && (
                <button type="button" className="widget-header-button widget-close-button" aria-label={text.close} onClick={() => setClosing(true)}>
                  <CloseGlyph />
                </button>
              )}
            </div>
          )}
        </header>

        {loading ? (
          <div className="widget-state" role="status">
            <span className="widget-loading-dots" aria-hidden="true"><i /><i /><i /></span>
            <p>{text.connectingStatus}</p>
          </div>
        ) : error && !state ? (
          <div className="widget-state widget-state--error" role="alert">
            <span className="widget-state-icon"><ErrorGlyph /></span>
            <strong>{error}</strong>
            <button type="button" onClick={() => window.location.reload()}>{text.retry}</button>
          </div>
        ) : state ? (
          <>
            {contactFormVisible && (
              <section className="widget-contact-card" aria-labelledby="widget-contact-title">
                <button
                  type="button"
                  className="widget-contact-close"
                  aria-label={text.contactDismiss}
                  onClick={handleDismissContactForm}
                >
                  <CloseGlyph />
                </button>
                <div className="widget-contact-copy">
                  <strong id="widget-contact-title">{contactTitle}</strong>
                  <p>{contactDescription}</p>
                </div>
                <form onSubmit={handleContactSubmit}>
                  <div className="widget-contact-fields">
                    <label>
                      <span>{contactNameLabel}</span>
                      <input
                        type="text"
                        name="name"
                        autoComplete="name"
                        maxLength={200}
                        required
                        value={contactName}
                        placeholder={contactNamePlaceholder}
                        onInput={(event) => setContactName(event.currentTarget.value)}
                      />
                    </label>
                    <label>
                      <span>{contactEmailLabel}</span>
                      <input
                        type="email"
                        name="email"
                        autoComplete="email"
                        maxLength={320}
                        required
                        value={contactEmail}
                        placeholder={contactEmailPlaceholder}
                        onInput={(event) => setContactEmail(event.currentTarget.value)}
                      />
                    </label>
                  </div>
                  {contactError && <div className="widget-contact-error" role="alert">{contactError}</div>}
                  <div className="widget-contact-actions">
                    <button type="button" className="widget-contact-skip" onClick={handleDismissContactForm}>
                      {contactSkip}
                    </button>
                    <button type="submit" className="widget-contact-save" disabled={contactSaving}>
                      {contactSaving ? contactSavingText : contactSave}
                    </button>
                  </div>
                </form>
              </section>
            )}
            <div ref={messagesContainerRef} className="widget-messages" aria-live="polite">
              {messages.length === 0 && launcher.show_greeting && (
                <section className="widget-welcome">
                  <div className="welcome-agent">
                    <WidgetParticipantAvatar name={supportName} avatarUrl={supportAvatarUrl} />
                    <div>
                      <strong>{supportName}</strong>
                      <small className={supportAvailable ? undefined : "widget-availability--offline"}>
                        {welcomeStatus}
                      </small>
                    </div>
                  </div>
                  <p>
                    {supportAvailable
                      ? state.session.greeting || text.fallbackGreeting
                      : state.session.offline_message || text.offlineGreeting}
                  </p>
                </section>
              )}
              {timeline.map((item) => {
                if (item.kind !== "message") {
                  const participant = item.kind === "ai_joined" ? item.agent : item.operator;
                  return (
                    <ParticipantJoinedNotice
                      key={`${item.kind}-${participant.joined_at}`}
                      participant={participant}
                      language={language}
                      label={text.operatorJoined(participant.display_name)}
                    />
                  );
                }
                const { message } = item;
                return (
                  <article
                    className={`widget-message widget-message--${message.direction}${message.id === enteringMessageId ? " widget-message--entering" : ""}`}
                    key={message.id}
                    {...{
                      onanimationend: (event: AnimationEvent) => {
                        if (event.target !== event.currentTarget) return;
                        setEnteringMessageId((current) => current === message.id ? null : current);
                      },
                    }}
                  >
                    {message.author_kind === "contact" && contactMessageAuthor && (
                      <div className="widget-message-author">
                        <strong>{contactMessageAuthor}</strong>
                      </div>
                    )}
                    {message.author_kind === "ai" && (
                      <div className="widget-message-author">
                        <strong>{conversationAiAgent?.display_name ?? text.operator}</strong>
                        {conversationAiAgent && <span> {text.operator}</span>}
                      </div>
                    )}
                    {message.body && (
                      <MessageBody
                        body={message.body}
                        format={message.body_format}
                        animate={typingMessageIds.has(message.id)}
                      />
                    )}
                    <MessageAttachments
                      attachments={message.attachments}
                      labels={{
                        download: text.attachmentDownload,
                        loadVideo: text.attachmentLoadVideo,
                        unavailable: text.attachmentUnavailable,
                        loadError: text.attachmentLoadError,
                        securityChecked: text.attachmentChecked,
                      }}
                      loadAttachment={(attachmentId) => downloadWidgetAttachment(
                        state.session.token,
                        attachmentId,
                      )}
                      variant="widget"
                    />
                    <time data-read-message-id={message.direction === "outbound" ? message.id : undefined}>
                      {formatTime(message.created_at, language)}
                    </time>
                  </article>
                );
              })}
              {sending && (
                <div className="widget-sending" role="status">
                  <span>{text.sending}</span>
                  <i className="widget-sending-dots" aria-hidden="true"><b /><b /><b /></i>
                </div>
              )}
              {resolutionId && (
                <section className={`widget-rating${rated ? " widget-rating--complete" : ""}`}>
                  {rated ? (
                    <div className="widget-rating-thanks">
                      <FeedbackCheckGlyph />
                      <p role="status">{state.session.rating_thanks || text.feedbackThanks}</p>
                      <button
                        className="widget-continue-conversation"
                        type="button"
                        onClick={handleContinueConversation}
                      >
                        {text.continueConversation}
                      </button>
                    </div>
                  ) : (
                    <form
                      onSubmit={(event) => {
                        event.preventDefault();
                        void handleFeedbackSubmit();
                      }}
                    >
                      <p className="widget-rating-prompt" id="widget-rating-prompt">
                        {state.session.rating_prompt || text.ratingQuestion}
                      </p>
                      <div
                        className="widget-stars"
                        role="group"
                        aria-labelledby="widget-rating-prompt"
                        onMouseLeave={() => setHoveredRating(null)}
                      >
                        {[1, 2, 3, 4, 5].map((rating) => {
                          const previewRating = hoveredRating ?? selectedRating ?? 0;
                          const active = rating <= previewRating;
                          return (
                            <button
                              className={`${active ? "widget-star widget-star--active" : "widget-star"}${selectedRating === rating ? " widget-star--selected" : ""}`}
                              type="button"
                              key={rating}
                              aria-label={text.rate(rating)}
                              aria-pressed={selectedRating === rating}
                              onMouseEnter={() => setHoveredRating(rating)}
                              onFocus={() => setHoveredRating(rating)}
                              onBlur={() => setHoveredRating(null)}
                              onClick={() => selectRating(rating)}
                            >
                              <StarGlyph />
                            </button>
                          );
                        })}
                      </div>
                      {selectedRating && (
                        <div className="widget-feedback-fields">
                          <span className="widget-rating-selection" aria-live="polite">
                            {text.selectedRating(selectedRating)}
                          </span>
                          <fieldset className="widget-rating-reasons">
                            <legend>
                              <strong>{text.ratingReasons}</strong>
                              <span>{text.ratingReasonsOptional}</span>
                            </legend>
                            <div>
                              {text.ratingReasonOptions(selectedRating).map((reason) => (
                                <label className={selectedReasons.includes(reason.value) ? "widget-reason widget-reason--selected" : "widget-reason"} key={reason.value}>
                                  <input
                                    type="checkbox"
                                    checked={selectedReasons.includes(reason.value)}
                                    onChange={() => toggleRatingReason(reason.value)}
                                  />
                                  <span>{reason.label}</span>
                                </label>
                              ))}
                            </div>
                          </fieldset>
                          <label className="widget-comment-field" htmlFor="widget-rating-comment">
                            <span>{text.commentLabel}<small>{text.commentOptional}</small></span>
                            <textarea
                              id="widget-rating-comment"
                              rows={3}
                              maxLength={2_000}
                              value={ratingComment}
                              placeholder={text.commentPlaceholder}
                              onInput={(event) => setRatingComment(event.currentTarget.value)}
                            />
                          </label>
                          <button className="widget-feedback-submit" type="submit" disabled={ratingSubmitting}>
                            {ratingSubmitting ? text.submittingFeedback : text.submitFeedback}
                          </button>
                        </div>
                      )}
                      <button
                        className="widget-continue-conversation"
                        type="button"
                        disabled={ratingSubmitting}
                        onClick={handleContinueConversation}
                      >
                        {text.continueConversation}
                      </button>
                    </form>
                  )}
                </section>
              )}
            </div>
            {error && <div className="widget-error" role="alert">{error}</div>}
            {!resolutionId && <form
              className={`widget-composer${state.conversation.widget_attachments_enabled ? " widget-composer--attachments" : ""}${draft.trim() ? " widget-composer--typing" : ""}${sending ? " widget-composer--sending" : ""}`}
              aria-busy={sending || uploading}
              onSubmit={handleSend}
            >
              <label className="visually-hidden" htmlFor="widget-message">{text.message}</label>
              {state.conversation.widget_attachments_enabled && (
                <>
                  <input
                    ref={attachmentInputRef}
                    className="visually-hidden"
                    type="file"
                    accept={ATTACHMENT_ACCEPT}
                    disabled={uploading}
                    onChange={(event) => void handleAttachment(event)}
                  />
                  <button
                    type="button"
                    className="widget-attachment-button"
                    aria-label={uploading ? text.attachmentUploading : text.attachmentAdd}
                    disabled={uploading}
                    onClick={() => attachmentInputRef.current?.click()}
                  >
                    <PaperclipGlyph />
                  </button>
                </>
              )}
              <textarea
                ref={composerRef}
                id="widget-message"
                rows={1}
                maxLength={10_000}
                value={draft}
                placeholder={messagePlaceholder}
                aria-keyshortcuts="Enter"
                onInput={(event) => setDraft(event.currentTarget.value)}
                onKeyDown={handleComposerKeyDown}
              />
              <button
                type="submit"
                className={`widget-send-button${draft.trim() && !sending ? " widget-send-button--ready" : ""}${sending ? " widget-send-button--sending" : ""}`}
                aria-label={sending ? text.sending : text.send}
                disabled={sending || !draft.trim()}
              >
                <WidgetButtonIcon icon={theme.send_icon} />
              </button>
            </form>}
            {footerText.trim() && <footer className="widget-footer">{footerText}</footer>}
          </>
        ) : null}
      </main>
    </div>
  );
}
