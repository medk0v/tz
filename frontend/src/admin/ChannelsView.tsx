import { AnimatedDisclosure } from "../AnimatedDisclosure";
import { DemoActionButton } from "./DemoReadOnly";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Check,
  ChevronDown,
  Copy,
  Mail,
  MessageCircle,
  Plus,
  Save,
  Send,
  Settings2,
  Trash2,
  type LucideIcon,
} from "lucide-react";
import {
  ApiRequestError,
  createWidgetChannel,
  deleteChannel,
  listChannels,
  updateWidgetChannel,
  type ChannelConnection,
  type Inbox,
  type OperatorAuth,
  type WidgetLauncher,
  type WidgetLanguage,
  type WidgetTranslations,
} from "../api";
import {
  channelStatusLabel,
  useI18n,
  type MessageKey,
} from "../i18n";
import { normalizeWidgetLanguage, widgetMessages } from "../widget-i18n";
import { WIDGET_FONT_FAMILIES, widgetFontStack } from "../widget-font";
import { WidgetButtonIcon, WIDGET_LAUNCHER_ICONS, WIDGET_SEND_ICONS } from "../WidgetButtonIcon";
import { ChannelBlacklistSettings } from "./ChannelBlacklistSettings";
import { EmailSettingsView } from "./EmailSettingsView";
import { WidgetAppearancePreview } from "./WidgetAppearancePreview";
import { TelegramBotsView } from "./TelegramBotsView";
import { CustomAiChannelIcon, CustomAiChannelsView } from "./CustomAiChannelsView";
import { customAiChannelText } from "./custom-ai-channel-i18n";
import { listCustomAiChannels, type CustomAiChannel } from "../custom-ai-channel-api";
import { useCanonicalPageRoute, usePageRoute } from "./page-route";
import {
  normalizeWidgetTheme,
  widgetThemePalette,
  type ResolvedWidgetTheme,
  type WidgetColorScheme,
  type WidgetThemePalette,
} from "../widget-theme";
import "./ChannelsView.css";

interface ChannelsViewProps {
  auth: OperatorAuth;
  inboxes: Inbox[];
  canManage: boolean;
  showDefaultChannels?: boolean;
}

interface ChannelsNotice {
  kind: "success" | "error";
  message: MessageKey;
}

const widgetFontNames = {
  arial: "Arial",
  verdana: "Verdana",
  tahoma: "Tahoma",
  trebuchet_ms: "Trebuchet MS",
  georgia: "Georgia",
  times_new_roman: "Times New Roman",
  courier_new: "Courier New",
};

type ChannelTypeId = "widgets" | "telegram" | "email";

type ChannelsScreen =
  | { name: "catalog" }
  | { name: "blacklist"; channelId: string }
  | { name: "widgets" }
  | { name: "telegram-bots" }
  | { name: "email" }
  | { name: "custom-ai"; channelId: string }
  | { name: "widget"; channelId: string; section: WidgetEditorSection };

/** Names the creation form in place of a record id, e.g. `/channels/widgets/new`. */
const NEW_CHANNEL = "new";

interface ChannelTypeDefinition {
  id: ChannelTypeId;
  label: MessageKey;
  description: MessageKey;
  icon: LucideIcon;
  kinds: string[];
  available: boolean;
}

const channelTypes: ChannelTypeDefinition[] = [
  {
    id: "widgets",
    label: "channels.type.widgets",
    description: "channels.type.widgetsDescription",
    icon: MessageCircle,
    kinds: ["widget"],
    available: true,
  },
  {
    id: "telegram",
    label: "channels.type.telegram",
    description: "channels.type.telegramDescription",
    icon: Send,
    kinds: ["telegram_bot", "telegram_business", "telegram_tdlib"],
    available: true,
  },
  {
    id: "email",
    label: "channels.type.email",
    description: "channels.type.emailDescription",
    icon: Mail,
    kinds: ["gmail", "imap_smtp"],
    available: true,
  },
];

const defaultWidgetLauncher: WidgetLauncher = {
  launcher_type: "icon",
  position: "bottom_right",
  label: "Chat with us",
  show_greeting: true,
  show_operator_profile: false,
  offset_x: 20,
  offset_y: 20,
  attention_animation: "pulse",
  animation_interval_seconds: 5,
  proactive_invitation_enabled: false,
  proactive_invitation_delay_seconds: 15,
};

function normalizeWidgetLauncher(launcher?: WidgetLauncher | null): WidgetLauncher {
  return {
    ...defaultWidgetLauncher,
    ...launcher,
    attention_animation: launcher?.attention_animation ?? "pulse",
    animation_interval_seconds: launcher?.animation_interval_seconds ?? 5,
    show_greeting: launcher?.show_greeting ?? true,
    show_operator_profile: launcher?.show_operator_profile ?? false,
    proactive_invitation_enabled: launcher?.proactive_invitation_enabled ?? false,
    proactive_invitation_delay_seconds: launcher?.proactive_invitation_delay_seconds ?? 15,
  };
}

const defaultWidgetTranslations: WidgetTranslations = {
  en: {
    support_name: "Support",
    greeting: "How can we help? Send us a message.",
    offline_message: "Operators are offline. Leave a message and your contact details.",
    launcher_label: "Chat with us",
    rating_prompt: "Thanks for chatting with us. Please rate the support you received.",
    rating_thanks: "Thank you! Your feedback helps us improve.",
    proactive_invitation_message: "Hi! Can I help you?",
    online_now: "Online now",
    offline_now: "Leave a message",
    message_placeholder: "Write a message…",
    contact_title: "Introduce yourself",
    contact_description: "Optional. Leave your name and email so we can contact you about this conversation.",
    contact_name: "Name",
    contact_name_placeholder: "Your name",
    contact_email: "Email",
    contact_email_placeholder: "you@example.com",
    contact_save: "Save details",
    contact_saving: "Saving…",
    contact_skip: "Continue anonymously",
    contact_error: "Could not save your contact details.",
  },
  ru: {
    support_name: "Support",
    greeting: "Чем мы можем помочь? Напишите нам.",
    offline_message: "Операторы офлайн. Оставьте сообщение и контакты.",
    launcher_label: "Напишите нам",
    rating_prompt: "Спасибо за обращение! Оцените, пожалуйста, качество поддержки.",
    rating_thanks: "Спасибо! Ваш отзыв поможет нам стать лучше.",
    proactive_invitation_message: "Здравствуйте! Могу помочь?",
    online_now: "Сейчас в сети",
    offline_now: "Оставьте сообщение",
    message_placeholder: "Напишите сообщение…",
    contact_title: "Представьтесь",
    contact_description: "Необязательно. Оставьте имя и email, чтобы мы могли связаться с вами по этому диалогу.",
    contact_name: "Имя",
    contact_name_placeholder: "Ваше имя",
    contact_email: "Email",
    contact_email_placeholder: "you@example.com",
    contact_save: "Сохранить",
    contact_saving: "Сохраняем…",
    contact_skip: "Продолжить анонимно",
    contact_error: "Не удалось сохранить контактные данные.",
  },
};

interface EditableWidgetTranslation {
  id: string;
  code: string;
  supportName: string;
  greeting: string;
  offlineMessage: string;
  launcherLabel: string;
  ratingPrompt: string;
  ratingThanks: string;
  proactiveInvitationMessage: string;
  onlineNow: string;
  offlineNow: string;
  messagePlaceholder: string;
  contactTitle: string;
  contactDescription: string;
  contactName: string;
  contactNamePlaceholder: string;
  contactEmail: string;
  contactEmailPlaceholder: string;
  contactSave: string;
  contactSaving: string;
  contactSkip: string;
  contactError: string;
}

const widgetEditorSections = ["General", "Languages", "Launcher", "Appearance", "Install"] as const;
type WidgetEditorSection = typeof widgetEditorSections[number];

let nextTranslationId = 0;

function editableTranslations(translations: WidgetTranslations): EditableWidgetTranslation[] {
  return Object.entries(translations).map(([code, translation]) => {
    const fallback = widgetMessages(normalizeWidgetLanguage(code) ?? "en");
    return {
      id: `widget-language-${nextTranslationId++}`,
      code,
      supportName: translation.support_name ?? "Support",
      greeting: translation.greeting ?? "",
      offlineMessage: translation.offline_message ?? fallback.offlineGreeting,
      launcherLabel: translation.launcher_label,
      ratingPrompt: translation.rating_prompt ?? fallback.ratingQuestion,
      ratingThanks: translation.rating_thanks ?? fallback.feedbackThanks,
      proactiveInvitationMessage:
        translation.proactive_invitation_message ?? fallback.proactiveInvitation,
      onlineNow: translation.online_now ?? fallback.onlineNow,
      offlineNow: translation.offline_now ?? fallback.offlineNow,
      messagePlaceholder: translation.message_placeholder ?? fallback.placeholder,
      contactTitle: translation.contact_title ?? fallback.contactTitle,
      contactDescription: translation.contact_description ?? fallback.contactDescription,
      contactName: translation.contact_name ?? fallback.contactName,
      contactNamePlaceholder: translation.contact_name_placeholder ?? fallback.contactNamePlaceholder,
      contactEmail: translation.contact_email ?? fallback.contactEmail,
      contactEmailPlaceholder: translation.contact_email_placeholder ?? fallback.contactEmailPlaceholder,
      contactSave: translation.contact_save ?? fallback.contactSave,
      contactSaving: translation.contact_saving ?? fallback.contactSaving,
      contactSkip: translation.contact_skip ?? fallback.contactSkip,
      contactError: translation.contact_error ?? fallback.contactError,
    };
  });
}

type WidgetColorKey = keyof WidgetThemePalette;
type WidgetBaseColorKey = Extract<
  WidgetColorKey,
  "accent_color" | "accent_text_color" | "surface_color" | "text_color"
>;

const baseColorKeys: readonly WidgetBaseColorKey[] = [
  "accent_color",
  "accent_text_color",
  "surface_color",
  "text_color",
];

const colorGroups: Array<{
  label: MessageKey;
  fields: Array<{ key: WidgetColorKey; label: MessageKey }>;
  icon?: {
    key: "launcher_icon" | "send_icon";
    label: MessageKey;
    options: ReadonlyArray<ResolvedWidgetTheme["launcher_icon"] | ResolvedWidgetTheme["send_icon"]>;
  };
}> = [
  {
    label: "channels.colorGroupBase",
    fields: [
      { key: "accent_color", label: "channels.colorAccent" },
      { key: "accent_text_color", label: "channels.colorOnAccent" },
      { key: "surface_color", label: "channels.colorSurface" },
      { key: "text_color", label: "channels.colorText" },
      { key: "background_color", label: "channels.colorBackground" },
    ],
  },
  {
    label: "channels.colorGroupContent",
    fields: [
      { key: "control_background_color", label: "channels.colorControlBackground" },
      { key: "control_text_color", label: "channels.colorControlText" },
      { key: "muted_text_color", label: "channels.colorMutedText" },
      { key: "divider_color", label: "channels.colorDivider" },
      { key: "shadow_color", label: "channels.colorShadow" },
    ],
  },
  {
    label: "channels.colorGroupLauncher",
    icon: { key: "launcher_icon", label: "channels.launcherIcon", options: WIDGET_LAUNCHER_ICONS },
    fields: [
      { key: "launcher_background_color", label: "channels.colorLauncherBackground" },
      { key: "launcher_icon_color", label: "channels.colorLauncherIcon" },
    ],
  },
  {
    label: "channels.colorGroupInput",
    fields: [
      { key: "input_background_color", label: "channels.colorInputBackground" },
      { key: "input_text_color", label: "channels.colorInputText" },
      { key: "input_placeholder_color", label: "channels.colorInputPlaceholder" },
    ],
  },
  {
    label: "channels.colorGroupSend",
    icon: { key: "send_icon", label: "channels.sendIcon", options: WIDGET_SEND_ICONS },
    fields: [
      { key: "send_background_color", label: "channels.colorSendBackground" },
      { key: "send_icon_color", label: "channels.colorSendIcon" },
    ],
  },
  {
    label: "channels.colorGroupHeaderFooter",
    fields: [
      { key: "status_text_color", label: "channels.colorStatusText" },
      { key: "footer_text_color", label: "channels.colorFooterText" },
      { key: "sound_icon_color", label: "channels.colorSoundIcon" },
      { key: "close_icon_color", label: "channels.colorCloseIcon" },
    ],
  },
  {
    label: "channels.colorGroupStatus",
    fields: [
      { key: "online_color", label: "channels.colorOnline" },
      { key: "offline_color", label: "channels.colorOffline" },
      { key: "danger_color", label: "channels.colorDanger" },
      { key: "rating_color", label: "channels.colorRating" },
    ],
  },
];

function isWidgetBaseColorKey(key: WidgetColorKey): key is WidgetBaseColorKey {
  return baseColorKeys.includes(key as WidgetBaseColorKey);
}

function widgetInstallCode(publicId: string): string {
  const configuredUrl = import.meta.env.VITE_WIDGET_LOADER_URL?.trim();
  const loaderUrl = new URL(
    configuredUrl || "/loader/widget-loader.js",
    window.location.origin,
  ).toString();
  const attributes = [`src="${loaderUrl}"`];
  const frameUrl = import.meta.env.VITE_WIDGET_FRAME_URL?.trim();
  const widgetApiBaseUrl = import.meta.env.VITE_WIDGET_API_BASE_URL?.trim();
  if (frameUrl) attributes.push(`data-src="${new URL(frameUrl, window.location.origin)}"`);
  if (widgetApiBaseUrl) {
    attributes.push(`data-api-base="${new URL(widgetApiBaseUrl, window.location.origin)}"`);
  }
  attributes.push(`data-widget-id="${publicId}"`);
  attributes.push('data-theme="light"');

  return `<script\n${attributes.map((attribute) => `  ${attribute}`).join("\n")}\n></script>`;
}

function adaptiveWidgetThemeInstallCode(publicId: string): string {
  const configuredUrl = import.meta.env.VITE_WIDGET_LOADER_URL?.trim();
  const loaderUrl = new URL(
    configuredUrl || "/loader/widget-loader.js",
    window.location.origin,
  ).toString();
  const loaderId = "tz-widget-loader";
  const widgetGlobal = "TzWidget";
  const attributes = [`id="${loaderId}"`, `src="${loaderUrl}"`];
  const frameUrl = import.meta.env.VITE_WIDGET_FRAME_URL?.trim();
  const widgetApiBaseUrl = import.meta.env.VITE_WIDGET_API_BASE_URL?.trim();
  if (frameUrl) attributes.push(`data-src="${new URL(frameUrl, window.location.origin)}"`);
  if (widgetApiBaseUrl) attributes.push(`data-api-base="${new URL(widgetApiBaseUrl, window.location.origin)}"`);
  attributes.push(`data-widget-id="${publicId}"`);
  attributes.push('data-theme="light"');

  return `<script\n${attributes.map((attribute) => `  ${attribute}`).join("\n")}\n></script>

<script>
  (() => {
    const loader = document.getElementById('${loaderId}');

    if (!(loader instanceof HTMLScriptElement)) return;

    const getTheme = () => {
      const pageIsDark =
        document.documentElement.getAttribute('data-mode') === 'dark';

      let savedDarkMode = false;

      try {
        savedDarkMode = localStorage.getItem('darkMode') === '1';
      } catch {}

      return pageIsDark || savedDarkMode ? 'dark' : 'light';
    };

    const syncWidgetTheme = () => {
      const theme = getTheme();
      loader.dataset.theme = theme;
      window.${widgetGlobal}?.setTheme(theme);
    };

    syncWidgetTheme();

    new MutationObserver(syncWidgetTheme).observe(
      document.documentElement,
      {
        attributes: true,
        attributeFilter: ['data-mode']
      }
    );

    window.addEventListener('storage', syncWidgetTheme);
  })();
</script>`;
}

function BackButton({ onClick, label }: { onClick: () => void; label: string }) {
  return (
    <button className="back-button" type="button" onClick={onClick}>
      <ArrowLeft size={16} />
      {label}
    </button>
  );
}

function CopyButtonLabel({
  copied,
  copyLabel,
  copiedLabel,
}: {
  copied: boolean;
  copyLabel: string;
  copiedLabel: string;
}) {
  return (
    <span className="widget-copy-button-label" aria-live="polite">
      <span aria-hidden={copied}>{copyLabel}</span>
      <span aria-hidden={!copied}>{copiedLabel}</span>
    </span>
  );
}

function ChannelsCatalog({
  channels,
  customChannels,
  customLoadError,
  canManage,
  showDefaultChannels,
  onCreate,
  onOpenCustom,
  onReloadCustom,
  onOpen,
  onBlacklist,
}: {
  channels: ChannelConnection[];
  customChannels: CustomAiChannel[];
  customLoadError: boolean;
  canManage: boolean;
  showDefaultChannels: boolean;
  onCreate: () => void;
  onOpenCustom: (id: string) => void;
  onReloadCustom: () => void;
  onOpen: (channelType: ChannelTypeId) => void;
  onBlacklist: (channelId: string) => void;
}) {
  const { t, locale } = useI18n();
  const customAiText = customAiChannelText(locale);
  const catalogTypes = [
    ...(showDefaultChannels ? channelTypes : []).map((item) => ({ ...item, label: t(item.label), description: t(item.description), status: t(item.available ? "channels.available" : "channels.planned") })),
  ];
  const blacklistChannels = showDefaultChannels ? channels.filter((channel) => channel.kind !== "custom_ai") : [];

  return (
    <div className="page channels-page channels-page--table">
      <header className="page-toolbar">
        <div><h1>{t("channels.title")}</h1><p>{t("channels.catalogDescription")}</p></div>
        {blacklistChannels.length > 0 && <label className="blacklist-channel-picker"><span>{t("channels.blacklistSettings")}</span><select value="" onChange={(event) => { if (event.target.value) onBlacklist(event.target.value); }}><option value="">{t("channels.selectChannel")}</option>{blacklistChannels.map((channel) => <option key={channel.id} value={channel.id}>{channel.name}</option>)}</select></label>}
        {canManage && <DemoActionButton className="primary-button" type="button" onClick={onCreate}><Plus size={16} />{customAiText("create")}</DemoActionButton>}
      </header>
      {customLoadError && <div className="admin-notice admin-notice--error custom-ai-load-error" role="alert"><span>{customAiText("loadError")}</span><button className="secondary-button" type="button" onClick={onReloadCustom}>{customAiText("retry")}</button></div>}
      <div className="table-panel channel-catalog-table">
        <table>
          <thead>
            <tr>
              <th>{t("channels.catalogType")}</th>
              <th>{t("channels.catalogConnections")}</th>
              <th>{t("channels.catalogStatus")}</th>
              <th><span className="visually-hidden">{t("common.actions")}</span></th>
            </tr>
          </thead>
          <tbody>
            {!showDefaultChannels && customChannels.length === 0 && !customLoadError && <tr><td colSpan={4}><div className="empty-state">{customAiText("empty")}</div></td></tr>}
            {customChannels.map((channel) => <tr key={channel.id}>
              <td><div className="channel-type-cell"><CustomAiChannelIcon icon={channel.icon} /><div><strong>{channel.name}</strong><span className="custom-ai-source-description">{channel.source_url || customAiText(channel.connection_type === "external_api" ? "apiConnection" : "aiConnection")} · {customAiText(channel.destination === "tasks" ? "destinationTasks" : channel.destination === "custom" ? "destinationCustom" : "destinationConversations")}</span></div></div></td>
              <td>—</td><td>{customAiText("draft")}</td>
              <td className="table-action-cell"><button className="table-link" type="button" onClick={() => onOpenCustom(channel.id)}>{customAiText("edit")}<ArrowRight size={15} /></button></td>
            </tr>)}
            
            {catalogTypes.map((channelType) => {
              const Icon = channelType.icon;
              const count = channels.filter((channel) => channelType.kinds.includes(channel.kind)).length;
              return (
                <tr key={channelType.id}>
                  <td>
                    <div className="channel-type-cell">
                      <Icon size={18} strokeWidth={1.8} />
                      <div><strong>{channelType.label}</strong><span>{channelType.description}</span></div>
                    </div>
                  </td>
                  <td>{count}</td>
                  <td>{channelType.status}</td>
                  <td className="table-action-cell">
                    <button className="table-link" type="button" onClick={() => onOpen(channelType.id)}>
                      {t("common.open")}<ArrowRight size={15} />
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function WidgetsList({
  widgets,
  inboxes,
  canManage,
  notice,
  deletingChannelId,
  onBack,
  onCreate,
  onEdit,
  onDelete,
  onBlacklist,
}: {
  widgets: ChannelConnection[];
  inboxes: Inbox[];
  canManage: boolean;
  notice: ChannelsNotice | null;
  deletingChannelId: string | null;
  onBack: () => void;
  onCreate: () => void;
  onEdit: (channelId: string) => void;
  onDelete: (channel: ChannelConnection) => void;
  onBlacklist: (channelId: string) => void;
}) {
  const { t, formatDate } = useI18n();

  return (
    <div className="page channels-page channels-page--table">
      <BackButton onClick={onBack} label={t("channels.backToChannels")} />
      <header className="page-toolbar">
        <div><h1>{t("channels.widgetsTitle")}</h1><p>{t("channels.widgetsDescription")}</p></div>
        {canManage && <button className="primary-button" type="button" onClick={onCreate}><Plus size={16} />{t("channels.createWidget")}</button>}
      </header>
      {notice && (
        <div
          className={`admin-notice admin-notice--${notice.kind}`}
          role={notice.kind === "error" ? "alert" : "status"}
        >
          {t(notice.message)}
        </div>
      )}
      <div className="table-panel widgets-table">
        <table>
          <thead>
            <tr>
              <th>{t("channels.widgetName")}</th>
              <th>{t("channels.inbox")}</th>
              <th>{t("channels.catalogStatus")}</th>
              <th>{t("channels.originsCount")}</th>
              <th>{t("channels.updated")}</th>
              <th><span className="visually-hidden">{t("common.actions")}</span></th>
            </tr>
          </thead>
          <tbody>
            {widgets.map((widget) => (
              <tr key={widget.id}>
                <td><button className="table-primary-link" type="button" onClick={() => onEdit(widget.id)}>{widget.name}</button></td>
                <td>{inboxes.find((inbox) => inbox.id === widget.inbox_id)?.name ?? widget.inbox_id}</td>
                <td><span className={`status-text status-text--${widget.status}`}>{channelStatusLabel(t, widget.status)}</span></td>
                <td>{widget.allowed_origins?.length ?? 0}</td>
                <td>{formatDate(widget.updated_at, { year: "numeric", month: "short", day: "numeric" })}</td>
                <td className="table-action-cell">
                  <div className="table-actions">
                    <button className="table-link" type="button" disabled={deletingChannelId !== null} onClick={() => onEdit(widget.id)}>
                      <Settings2 size={15} />{t(canManage ? "channels.configure" : "channels.view")}
                    </button>
                    <button className="table-link" type="button" onClick={() => onBlacklist(widget.id)}>{t("channels.blacklist")}</button>
                    {canManage && (
                      <DemoActionButton
                        className="table-link table-danger-link"
                        type="button"
                        disabled={deletingChannelId !== null}
                        aria-label={t("channels.deleteWidget", { name: widget.name })}
                        onClick={() => onDelete(widget)}
                      >
                        <Trash2 size={15} />
                        {t(deletingChannelId === widget.id ? "channels.deleting" : "channels.delete")}
                      </DemoActionButton>
                    )}
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {widgets.length === 0 && (
          <div className="empty-state empty-state--action">
            <span>{t("channels.widgetsEmpty")}</span>
            {canManage && <button className="primary-button" type="button" onClick={onCreate}><Plus size={16} />{t("channels.createWidget")}</button>}
          </div>
        )}
      </div>
    </div>
  );
}

function WidgetEditor({
  auth,
  inboxes,
  channel,
  canManage,
  notice,
  section: activeSection,
  onSectionChange: setActiveSection,
  onBack,
  onCreated,
  onUpdated,
}: {
  auth: OperatorAuth;
  inboxes: Inbox[];
  channel: ChannelConnection | null;
  canManage: boolean;
  notice: MessageKey | null;
  /** The open settings section, which the page keeps in its URL. */
  section: WidgetEditorSection;
  onSectionChange: (section: WidgetEditorSection) => void;
  onBack: () => void;
  onCreated: (channel: ChannelConnection) => void;
  onUpdated: (channel: ChannelConnection) => void;
}) {
  const { locale, t } = useI18n();
  const creating = channel === null;
  const instanceId = useId();
  const [name, setName] = useState(channel?.name ?? "");
  const [inboxId, setInboxId] = useState(channel?.inbox_id ?? inboxes[0]?.id ?? "");
  const [origins, setOrigins] = useState(channel?.allowed_origins?.join("\n") ?? "");
  const [defaultLanguage, setDefaultLanguage] = useState<WidgetLanguage>(channel?.default_language ?? "en");
  const [translations, setTranslations] = useState<EditableWidgetTranslation[]>(() =>
    editableTranslations(channel?.translations ?? defaultWidgetTranslations),
  );
  const [expandedLanguages, setExpandedLanguages] = useState<string[]>([]);
  const [launcher, setLauncher] = useState<WidgetLauncher>(() =>
    normalizeWidgetLauncher(channel?.launcher),
  );
  const [launcherPreviewCycle, setLauncherPreviewCycle] = useState(0);
  const [theme, setTheme] = useState<ResolvedWidgetTheme>(() =>
    normalizeWidgetTheme(channel?.theme),
  );
  const [previewColorScheme, setPreviewColorScheme] = useState<WidgetColorScheme>("light");
  const [notifyOnNewVisitor, setNotifyOnNewVisitor] = useState(channel?.notify_on_new_visitor ?? false);
  const [attachmentsEnabled, setAttachmentsEnabled] = useState(channel?.attachments_enabled ?? false);
  const [saving, setSaving] = useState(false);
  const [copiedInstallCodeId, setCopiedInstallCodeId] = useState<string | null>(null);
  const [copiedAdaptiveCodeId, setCopiedAdaptiveCodeId] = useState<string | null>(null);
  const [error, setError] = useState<MessageKey | null>(null);
  const installPublicId = channel?.public_id ?? "YOUR_WIDGET_ID";
  const installCode = widgetInstallCode(installPublicId);
  const adaptiveThemeInstallCode = adaptiveWidgetThemeInstallCode(installPublicId);
  const installCodeWasCopied = channel !== null && copiedInstallCodeId === channel.id;
  const adaptiveCodeWasCopied = channel !== null && copiedAdaptiveCodeId === channel.id;
  const normalizedLanguageCodes = translations.map((translation) => normalizeWidgetLanguage(translation.code));
  const uniqueLanguageCodes = new Set(normalizedLanguageCodes.filter((code): code is string => Boolean(code)));
  const translationsAreValid = translations.length > 0
    && translations.length <= 20
    && normalizedLanguageCodes.every(Boolean)
    && uniqueLanguageCodes.size === translations.length
    && translations.every((translation) => (
      translation.launcherLabel.trim().length > 0
      && translation.offlineMessage.trim().length > 0
      && translation.ratingPrompt.trim().length > 0
      && translation.ratingThanks.trim().length > 0
      && translation.proactiveInvitationMessage.trim().length > 0
      && translation.onlineNow.trim().length > 0
      && translation.offlineNow.trim().length > 0
      && translation.messagePlaceholder.trim().length > 0
      && translation.contactTitle.trim().length > 0
      && translation.contactDescription.trim().length > 0
      && translation.contactName.trim().length > 0
      && translation.contactNamePlaceholder.trim().length > 0
      && translation.contactEmail.trim().length > 0
      && translation.contactEmailPlaceholder.trim().length > 0
      && translation.contactSave.trim().length > 0
      && translation.contactSaving.trim().length > 0
      && translation.contactSkip.trim().length > 0
      && translation.contactError.trim().length > 0
    ))
    && uniqueLanguageCodes.has(defaultLanguage);
  const previewTranslation = translations.find((translation) => normalizeWidgetLanguage(translation.code) === locale)
    ?? translations.find((translation) => normalizeWidgetLanguage(translation.code) === defaultLanguage)
    ?? translations[0];
  const previewLanguage = normalizeWidgetLanguage(previewTranslation?.code) ?? defaultLanguage;
  const defaultFooterText = widgetMessages(previewLanguage).footer;
  const footerVisible = theme.footer_text === null || theme.footer_text === undefined || theme.footer_text.trim().length > 0;
  const previewPalette = widgetThemePalette(theme, previewColorScheme);
  const previewBorderColor = previewColorScheme === "dark"
    ? theme.border.dark_color
    : theme.border.light_color;
  const launcherAttentionAnimation = !launcher.show_greeting
    && launcher.attention_animation === "pulse"
    ? "lift"
    : launcher.attention_animation;

  useEffect(() => {
    if (
      activeSection !== "Launcher"
      || launcherAttentionAnimation === "none"
      || window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true
    ) {
      return undefined;
    }
    const timer = window.setInterval(
      () => setLauncherPreviewCycle((cycle) => cycle + 1),
      launcher.animation_interval_seconds * 1_000,
    );
    return () => window.clearInterval(timer);
  }, [activeSection, launcher.animation_interval_seconds, launcherAttentionAnimation]);

  function updateThemeColor(key: WidgetColorKey, value: string) {
    setTheme((current) => {
      if (previewColorScheme === "dark") {
        return isWidgetBaseColorKey(key)
          ? { ...current, dark: { ...current.dark, [key]: value } }
          : {
              ...current,
              dark: {
                ...current.dark,
                component_colors: { ...current.dark.component_colors, [key]: value },
              },
            };
      }
      return isWidgetBaseColorKey(key)
        ? { ...current, [key]: value }
        : {
            ...current,
            component_colors: { ...current.component_colors, [key]: value },
          };
    });
  }

  function updateBorderColor(value: string) {
    const key = previewColorScheme === "dark" ? "dark_color" : "light_color";
    setTheme((current) => ({
      ...current,
      border: { ...current.border, [key]: value },
    }));
  }

  function updateTranslation(
    id: string,
    field: Exclude<keyof EditableWidgetTranslation, "id">,
    value: string,
  ) {
    if (field === "code") {
      const previousCode = normalizeWidgetLanguage(
        translations.find((translation) => translation.id === id)?.code,
      );
      const nextCode = normalizeWidgetLanguage(value);
      if (previousCode === defaultLanguage && nextCode) setDefaultLanguage(nextCode);
    }
    setTranslations((current) => current.map((translation) =>
      translation.id === id ? { ...translation, [field]: value } : translation,
    ));
  }

  function addTranslation() {
    const id = `widget-language-${nextTranslationId++}`;
    setExpandedLanguages((current) => [...current, id]);
    setTranslations((current) => [
      ...current,
      {
        id,
        code: "",
        supportName: "Support",
        greeting: "",
        offlineMessage: "Operators are offline. Leave a message and your contact details.",
        launcherLabel: "",
        ratingPrompt: "Thanks for chatting with us. Please rate the support you received.",
        ratingThanks: "Thank you! Your feedback helps us improve.",
        proactiveInvitationMessage: "Hi! Can I help you?",
        onlineNow: "Online now",
        offlineNow: "Leave a message",
        messagePlaceholder: "Write a message…",
        contactTitle: "Introduce yourself",
        contactDescription: "Optional. Leave your name and email so we can contact you about this conversation.",
        contactName: "Name",
        contactNamePlaceholder: "Your name",
        contactEmail: "Email",
        contactEmailPlaceholder: "you@example.com",
        contactSave: "Save details",
        contactSaving: "Saving…",
        contactSkip: "Continue anonymously",
        contactError: "Could not save your contact details.",
      },
    ]);
  }

  function removeTranslation(id: string) {
    const removed = translations.find((translation) => translation.id === id);
    const remaining = translations.filter((translation) => translation.id !== id);
    if (remaining.length > 0 && normalizeWidgetLanguage(removed?.code) === defaultLanguage) {
      setDefaultLanguage(normalizeWidgetLanguage(remaining[0].code) ?? "");
    }
    setTranslations((current) => {
      if (current.length === 1) return current;
      return current.filter((translation) => translation.id !== id);
    });
  }

  async function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    if (!canManage || !name.trim() || !translationsAreValid || !previewTranslation) return;
    const translationPayload = Object.fromEntries(translations.map((translation) => [
      normalizeWidgetLanguage(translation.code)!,
      {
        support_name: translation.supportName.trim() || "Support",
        greeting: translation.greeting.trim() || null,
        offline_message: translation.offlineMessage.trim(),
        launcher_label: translation.launcherLabel.trim(),
        rating_prompt: translation.ratingPrompt.trim(),
        rating_thanks: translation.ratingThanks.trim(),
        proactive_invitation_message: translation.proactiveInvitationMessage.trim(),
        online_now: translation.onlineNow.trim(),
        offline_now: translation.offlineNow.trim(),
        message_placeholder: translation.messagePlaceholder.trim(),
        contact_title: translation.contactTitle.trim(),
        contact_description: translation.contactDescription.trim(),
        contact_name: translation.contactName.trim(),
        contact_name_placeholder: translation.contactNamePlaceholder.trim(),
        contact_email: translation.contactEmail.trim(),
        contact_email_placeholder: translation.contactEmailPlaceholder.trim(),
        contact_save: translation.contactSave.trim(),
        contact_saving: translation.contactSaving.trim(),
        contact_skip: translation.contactSkip.trim(),
        contact_error: translation.contactError.trim(),
      },
    ]));
    const defaultTranslation = translationPayload[defaultLanguage];
    const input = {
      name: name.trim(),
      allowed_origins: origins.split("\n").map((origin) => origin.trim()).filter(Boolean),
      default_language: defaultLanguage,
      translations: translationPayload,
      launcher: { ...launcher, label: defaultTranslation.launcher_label },
      theme,
      notify_on_new_visitor: notifyOnNewVisitor,
      attachments_enabled: attachmentsEnabled,
    };
    setSaving(true);
    setError(null);
    try {
      if (creating) {
        const created = await createWidgetChannel(auth, {
          kind: "widget",
          inbox_id: inboxId,
          ...input,
        });
        onCreated(created);
      } else {
        onUpdated(await updateWidgetChannel(auth, channel.id, input));
      }
    } catch (cause) {
      if (cause instanceof ApiRequestError && (cause.status === 401 || cause.status === 403)) {
        setError("channels.saveAuthError");
      } else {
        setError(creating ? "channels.createError" : "channels.saveError");
      }
    } finally {
      setSaving(false);
    }
  }

  async function handleCopyInstallCode() {
    if (!channel) return;
    try {
      await navigator.clipboard.writeText(installCode);
      setCopiedInstallCodeId(channel.id);
      setError((current) => current === "channels.copyInstallError" ? null : current);
    } catch {
      setCopiedInstallCodeId(null);
      setError("channels.copyInstallError");
    }
  }

  async function handleCopyAdaptiveThemeCode() {
    if (!channel) return;
    try {
      await navigator.clipboard.writeText(adaptiveThemeInstallCode);
      setCopiedAdaptiveCodeId(channel.id);
      setError((current) => current === "channels.copyInstallError" ? null : current);
    } catch {
      setCopiedAdaptiveCodeId(null);
      setError("channels.copyInstallError");
    }
  }

  return (
    <div className="page channels-page channels-page--editor">
      <BackButton onClick={onBack} label={t("channels.backToWidgets")} />
      <header className="page-toolbar">
        <div>
          <h1>{creating ? t("channels.newWidgetTitle") : channel.name}</h1>
          <p>{t(creating ? "channels.newWidgetDescription" : "channels.editWidgetDescription")}</p>
        </div>
        {canManage && <DemoActionButton className="primary-button" type="submit" form={`${instanceId}-widget-editor-form`} disabled={saving || !name.trim() || !inboxId || !origins.trim() || !translationsAreValid}><Save size={16} />{saving ? t(creating ? "channels.creating" : "channels.saving") : t(creating ? "channels.create" : "channels.save")}</DemoActionButton>}
      </header>
      <nav className="section-tabs widget-editor-navigation" aria-label={t("channels.widgetSettings")} role="tablist">
        {widgetEditorSections.map((section, index) => (
          <button
            key={section}
            id={`${instanceId}-widget-tab-${section}`}
            type="button"
            role="tab"
            aria-controls={`${instanceId}-widget-panel-${section}`}
            aria-selected={activeSection === section}
            tabIndex={activeSection === section ? 0 : -1}
            className={activeSection === section ? "section-tab--active" : ""}
            onClick={() => setActiveSection(section)}
            onKeyDown={(event) => {
              if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
              event.preventDefault();
              const next = event.key === "Home" ? widgetEditorSections[0]
                : event.key === "End" ? widgetEditorSections[widgetEditorSections.length - 1]
                  : widgetEditorSections[(index + (event.key === "ArrowRight" ? 1 : -1) + widgetEditorSections.length) % widgetEditorSections.length];
              setActiveSection(next);
              document.getElementById(`${instanceId}-widget-tab-${next}`)?.focus();
            }}
          >{t(`channels.section${section}`)}</button>
        ))}
      </nav>
      {error && <div className="admin-notice admin-notice--error" role="alert">{t(error)}</div>}
      {notice && <div className="admin-notice admin-notice--success" role="status">{t(notice)}</div>}
      <section className="settings-panel widget-editor-panel">
        <form id={`${instanceId}-widget-editor-form`} onSubmit={handleSubmit} onInvalidCapture={(event) => {
          const input = event.target as HTMLInputElement;
          const panel = input.closest<HTMLElement>('[role="tabpanel"]');
          const languageFields = input.closest<HTMLElement>(".widget-language-fields");
          if (!panel?.hidden && !languageFields?.hidden) return;
          event.preventDefault();
          if (panel) setActiveSection(panel.dataset.section as WidgetEditorSection);
          if (languageFields) {
            const id = languageFields.id.replace(/-fields$/, "");
            setExpandedLanguages((current) => current.includes(id) ? current : [...current, id]);
          }
          requestAnimationFrame(() => { input.focus(); input.reportValidity(); });
        }}>
          <header>
            <h2>{t(creating ? "channels.widgetDetails" : "channels.widgetSettings")}</h2>
            {channel && <span>{channelStatusLabel(t, channel.status)}</span>}
          </header>
          {creating ? (
            <div className="widget-create-fields">
              <label>{t("channels.widgetName")}<input value={name} maxLength={200} required autoFocus onChange={(event) => setName(event.target.value)} /></label>
              <label>{t("channels.inbox")}<select value={inboxId} required onChange={(event) => setInboxId(event.target.value)}>{inboxes.map((inbox) => <option value={inbox.id} key={inbox.id}>{inbox.name}</option>)}</select></label>
            </div>
          ) : (
            <dl className="settings-metadata">
              <div><dt>{t("channels.inbox")}</dt><dd>{inboxes.find((inbox) => inbox.id === channel.inbox_id)?.name ?? channel.inbox_id}</dd></div>
            </dl>
          )}
          <div className="settings-form">
            <section className="widget-install" id={`${instanceId}-widget-panel-Install`} role="tabpanel" data-section="Install" aria-labelledby={`${instanceId}-widget-tab-Install`} hidden={activeSection !== "Install"}>
                <div className="field-heading">
                  <strong id={`${instanceId}-widget-install-title`}>{t("channels.installTitle")}</strong>
                  <span>{t("channels.installDescription")}</span>
                </div>
                <ol className="widget-install-steps">
                  <li>{t("channels.installStepOrigin")}</li>
                  <li>{t("channels.installStepSave")}</li>
                  <li>{t("channels.installStepEmbed")}</li>
                </ol>
                <div className="widget-install-language">
                  <p>{t("channels.installLanguagePriority")}</p>
                  <p>{t("channels.installLanguageOverride")}</p>
                  <p>{t("channels.installThemeSelection")}</p>
                </div>
                <div className="widget-install-code">
                  <pre tabIndex={0}><code>{installCode}</code></pre>
                  <button className="secondary-button" type="button" disabled={!channel} onClick={() => void handleCopyInstallCode()}>
                    {installCodeWasCopied ? <Check size={15} /> : <Copy size={15} />}
                    <CopyButtonLabel
                      copied={installCodeWasCopied}
                      copyLabel={t("channels.copyInstallCode")}
                      copiedLabel={t("channels.installCodeCopied")}
                    />
                  </button>
                </div>
                {!channel && <p className="widget-install-pending">{t("channels.installAvailableAfterCreate")}</p>}
                <section className="widget-adaptive-theme" aria-labelledby={`${instanceId}-widget-adaptive-theme-title`}>
                  <div className="field-heading">
                    <strong id={`${instanceId}-widget-adaptive-theme-title`}>{t("channels.adaptiveThemeTitle")}</strong>
                    <span>{t("channels.adaptiveThemeDescription")}</span>
                  </div>
                  <p>{t("channels.adaptiveThemeHelp")}</p>
                  <div className="widget-install-code">
                    <pre tabIndex={0}><code>{adaptiveThemeInstallCode}</code></pre>
                    <button className="secondary-button" type="button" disabled={!channel} onClick={() => void handleCopyAdaptiveThemeCode()}>
                      {adaptiveCodeWasCopied ? <Check size={15} /> : <Copy size={15} />}
                      <CopyButtonLabel
                        copied={adaptiveCodeWasCopied}
                        copyLabel={t("channels.copyAdaptiveThemeCode")}
                        copiedLabel={t("channels.installCodeCopied")}
                      />
                    </button>
                  </div>
                </section>
              </section>
            <section className="widget-general-settings" id={`${instanceId}-widget-panel-General`} role="tabpanel" data-section="General" aria-labelledby={`${instanceId}-widget-tab-General`} hidden={activeSection !== "General"}>
            <div className="field-heading"><strong id={`${instanceId}-widget-general-title`}>{t("channels.sectionGeneral")}</strong></div>
            {!creating && <label>{t("channels.widgetName")}<input value={name} maxLength={200} required readOnly={!canManage} onChange={(event) => setName(event.target.value)} /></label>}
            <label>{t("channels.allowedOrigins")}<textarea rows={3} required readOnly={!canManage} value={origins} onChange={(event) => setOrigins(event.target.value)} /><span>{t("channels.originsHelp")}</span></label>
            <label className="widget-notification-setting">
              <input
                type="checkbox"
                disabled={!canManage}
                checked={notifyOnNewVisitor}
                onChange={(event) => setNotifyOnNewVisitor(event.target.checked)}
              />
              <span>
                <strong>{t("channels.notifyOnNewVisitor")}</strong>
                <span>{t("channels.notifyOnNewVisitorHelp")}</span>
              </span>
            </label>
            <label className="widget-notification-setting">
              <input
                type="checkbox"
                disabled={!canManage}
                checked={attachmentsEnabled}
                onChange={(event) => setAttachmentsEnabled(event.target.checked)}
              />
              <span>
                <strong>{t("channels.attachmentsEnabled")}</strong>
                <span>{t("channels.attachmentsEnabledHelp")}</span>
              </span>
            </label>
            </section>
            <section className="widget-localization" id={`${instanceId}-widget-panel-Languages`} role="tabpanel" data-section="Languages" aria-labelledby={`${instanceId}-widget-tab-Languages`} hidden={activeSection !== "Languages"}>
              <div className="widget-localization-heading">
                <div className="field-heading">
                  <strong id={`${instanceId}-widget-localization-title`}>{t("channels.languageTitle")}</strong>
                  <span>{t("channels.languageDescription")}</span>
                </div>
                {canManage && (
                  <button className="secondary-button" type="button" disabled={translations.length >= 20} onClick={addTranslation}>
                    <Plus size={15} />{t("channels.addLanguage")}
                  </button>
                )}
              </div>
              <label className="widget-default-language">{t("channels.defaultLanguage")}
                <select disabled={!canManage} value={defaultLanguage} onChange={(event) => setDefaultLanguage(event.target.value)}>
                  {translations.map((translation) => {
                    const code = normalizeWidgetLanguage(translation.code);
                    return code ? <option value={code} key={translation.id}>{code}</option> : null;
                  })}
                </select>
                <span>{t("channels.defaultLanguageHelp")}</span>
              </label>
              {!translationsAreValid && <div className="field-error" role="alert">{t("channels.languageValidation")}</div>}
              <div className="widget-translation-grid">
                {translations.map((translation) => (
                  <fieldset className="widget-language-card" key={translation.id}>
                    <legend className="visually-hidden">{normalizeWidgetLanguage(translation.code) ?? t("channels.newLanguage")}</legend>
                    <div className="widget-language-heading">
                      <button
                        className="widget-language-toggle"
                        type="button"
                        aria-expanded={expandedLanguages.includes(translation.id)}
                        aria-controls={`${translation.id}-fields`}
                        onClick={() => setExpandedLanguages((current) => current.includes(translation.id)
                          ? current.filter((id) => id !== translation.id)
                          : [...current, translation.id])}
                      >
                        <span className="widget-language-code">{normalizeWidgetLanguage(translation.code) ?? t("channels.newLanguage")}</span>
                        <span className="widget-language-name">{translation.supportName || t("channels.supportName")}</span>
                        {normalizeWidgetLanguage(translation.code) === defaultLanguage && <span className="widget-language-default">{t("channels.defaultLanguage")}</span>}
                        <ChevronDown size={16} aria-hidden="true" />
                      </button>
                    {canManage && translations.length > 1 && (
                      <DemoActionButton className="language-remove-button" type="button" aria-label={t("channels.removeLanguage", { code: translation.code || t("channels.newLanguage") })} onClick={() => removeTranslation(translation.id)}>
                        <Trash2 size={15} />
                      </DemoActionButton>
                    )}
                    </div>
                    <AnimatedDisclosure className="widget-language-fields" id={`${translation.id}-fields`} open={expandedLanguages.includes(translation.id)} keepMounted>
                    <label>{t("channels.languageCode")}
                      <input value={translation.code} maxLength={35} required readOnly={!canManage} autoCapitalize="none" spellCheck={false} placeholder={t("channels.languageCodePlaceholder")} onChange={(event) => updateTranslation(translation.id, "code", event.target.value)} />
                      <span>{t("channels.languageCodeHelp")}</span>
                    </label>
                    <label>{t("channels.supportName")}
                      <input maxLength={80} readOnly={!canManage} value={translation.supportName} placeholder="Support" onChange={(event) => updateTranslation(translation.id, "supportName", event.target.value)} />
                      <span>{t("channels.supportNameHelp")}</span>
                    </label>
                    {launcher.show_greeting && (
                      <>
                        <label>{t("channels.greeting")}
                          <textarea rows={3} maxLength={500} readOnly={!canManage} value={translation.greeting} onChange={(event) => updateTranslation(translation.id, "greeting", event.target.value)} />
                        </label>
                        <label>{t("channels.offlineMessage")}
                          <textarea rows={3} maxLength={500} required readOnly={!canManage} value={translation.offlineMessage} onChange={(event) => updateTranslation(translation.id, "offlineMessage", event.target.value)} />
                          <span>{t("channels.offlineMessageHelp")}</span>
                        </label>
                      </>
                    )}
                    <label>{t("channels.launcherLabel")}
                      <input maxLength={40} required readOnly={!canManage} value={translation.launcherLabel} onChange={(event) => updateTranslation(translation.id, "launcherLabel", event.target.value)} />
                    </label>
                    <label>{t("channels.ratingPrompt")}
                      <textarea rows={3} maxLength={500} required readOnly={!canManage} value={translation.ratingPrompt} onChange={(event) => updateTranslation(translation.id, "ratingPrompt", event.target.value)} />
                      <span>{t("channels.ratingPromptHelp")}</span>
                    </label>
                    <label>{t("channels.ratingThanks")}
                      <textarea rows={2} maxLength={300} required readOnly={!canManage} value={translation.ratingThanks} onChange={(event) => updateTranslation(translation.id, "ratingThanks", event.target.value)} />
                    </label>
                    <label>{t("channels.onlineNowText")}
                      <input maxLength={80} required readOnly={!canManage} value={translation.onlineNow} onChange={(event) => updateTranslation(translation.id, "onlineNow", event.target.value)} />
                    </label>
                    <label>{t("channels.offlineNowText")}
                      <input maxLength={80} required readOnly={!canManage} value={translation.offlineNow} onChange={(event) => updateTranslation(translation.id, "offlineNow", event.target.value)} />
                    </label>
                    <label>{t("channels.messagePlaceholder")}
                      <input maxLength={160} required readOnly={!canManage} value={translation.messagePlaceholder} onChange={(event) => updateTranslation(translation.id, "messagePlaceholder", event.target.value)} />
                    </label>
                    <label>{t("channels.contactTitle")}
                      <input maxLength={120} required readOnly={!canManage} value={translation.contactTitle} onChange={(event) => updateTranslation(translation.id, "contactTitle", event.target.value)} />
                    </label>
                    <label>{t("channels.contactDescription")}
                      <textarea rows={3} maxLength={500} required readOnly={!canManage} value={translation.contactDescription} onChange={(event) => updateTranslation(translation.id, "contactDescription", event.target.value)} />
                    </label>
                    <label>{t("channels.contactName")}
                      <input maxLength={80} required readOnly={!canManage} value={translation.contactName} onChange={(event) => updateTranslation(translation.id, "contactName", event.target.value)} />
                    </label>
                    <label>{t("channels.contactNamePlaceholder")}
                      <input maxLength={160} required readOnly={!canManage} value={translation.contactNamePlaceholder} onChange={(event) => updateTranslation(translation.id, "contactNamePlaceholder", event.target.value)} />
                    </label>
                    <label>{t("channels.contactEmail")}
                      <input maxLength={80} required readOnly={!canManage} value={translation.contactEmail} onChange={(event) => updateTranslation(translation.id, "contactEmail", event.target.value)} />
                    </label>
                    <label>{t("channels.contactEmailPlaceholder")}
                      <input maxLength={160} required readOnly={!canManage} value={translation.contactEmailPlaceholder} onChange={(event) => updateTranslation(translation.id, "contactEmailPlaceholder", event.target.value)} />
                    </label>
                    <label>{t("channels.contactSave")}
                      <input maxLength={80} required readOnly={!canManage} value={translation.contactSave} onChange={(event) => updateTranslation(translation.id, "contactSave", event.target.value)} />
                    </label>
                    <label>{t("channels.contactSaving")}
                      <input maxLength={80} required readOnly={!canManage} value={translation.contactSaving} onChange={(event) => updateTranslation(translation.id, "contactSaving", event.target.value)} />
                    </label>
                    <label>{t("channels.contactSkip")}
                      <input maxLength={120} required readOnly={!canManage} value={translation.contactSkip} onChange={(event) => updateTranslation(translation.id, "contactSkip", event.target.value)} />
                    </label>
                    <label>{t("channels.contactError")}
                      <textarea rows={2} maxLength={300} required readOnly={!canManage} value={translation.contactError} onChange={(event) => updateTranslation(translation.id, "contactError", event.target.value)} />
                    </label>
                    </AnimatedDisclosure>
                  </fieldset>
                ))}
              </div>
            </section>
            <section className="launcher-settings" id={`${instanceId}-widget-panel-Launcher`} role="tabpanel" data-section="Launcher" aria-labelledby={`${instanceId}-widget-tab-Launcher`} hidden={activeSection !== "Launcher"}>
              <div className="launcher-controls">
                <div className="field-heading"><strong id="launcher-settings-title">{t("channels.launcherTitle")}</strong><span>{t("channels.launcherDescription")}</span></div>
                <div className="launcher-fields">
                  <label>{t("channels.launcherType")}
                    <select disabled={!canManage} value={launcher.launcher_type} onChange={(event) => setLauncher((current) => ({ ...current, launcher_type: event.target.value as WidgetLauncher["launcher_type"] }))}>
                      <option value="icon">{t("channels.launcherTypeIcon")}</option>
                      <option value="text">{t("channels.launcherTypeText")}</option>
                      <option value="icon_text">{t("channels.launcherTypeIconText")}</option>
                    </select>
                  </label>
                  <label>{t("channels.launcherPosition")}
                    <select disabled={!canManage} value={launcher.position} onChange={(event) => setLauncher((current) => ({ ...current, position: event.target.value as WidgetLauncher["position"] }))}>
                      <option value="bottom_right">{t("channels.launcherPositionRight")}</option>
                      <option value="bottom_left">{t("channels.launcherPositionLeft")}</option>
                    </select>
                  </label>
                  <label>{t("channels.launcherAnimation")}
                    <select disabled={!canManage} value={launcherAttentionAnimation} onChange={(event) => setLauncher((current) => ({ ...current, attention_animation: event.target.value as WidgetLauncher["attention_animation"] }))}>
                      {launcher.show_greeting && <option value="pulse">{t("channels.launcherAnimationPulse")}</option>}
                      <option value="lift">{t("channels.launcherAnimationLift")}</option>
                      <option value="sway">{t("channels.launcherAnimationSway")}</option>
                      <option value="none">{t("channels.launcherAnimationNone")}</option>
                    </select>
                  </label>
                  <label>{t("channels.launcherAnimationInterval")}
                    <input
                      type="number"
                      min={1}
                      max={300}
                      step={1}
                      required
                      disabled={!canManage || launcherAttentionAnimation === "none"}
                      value={launcher.animation_interval_seconds}
                      onChange={(event) => {
                        const seconds = Math.min(
                          300,
                          Math.max(1, Math.round(event.currentTarget.valueAsNumber || 1)),
                        );
                        setLauncher((current) => ({
                          ...current,
                          animation_interval_seconds: seconds,
                        }));
                      }}
                    />
                  </label>
                  <label>{t("channels.launcherOffsetX")}<input type="number" min={0} max={120} required readOnly={!canManage} value={launcher.offset_x} onChange={(event) => setLauncher((current) => ({ ...current, offset_x: Number(event.target.value) }))} /></label>
                  <label>{t("channels.launcherOffsetY")}<input type="number" min={0} max={120} required readOnly={!canManage} value={launcher.offset_y} onChange={(event) => setLauncher((current) => ({ ...current, offset_y: Number(event.target.value) }))} /></label>
                </div>
                <label className="widget-notification-setting">
                  <input
                    type="checkbox"
                    disabled={!canManage}
                    checked={launcher.show_greeting}
                    onChange={(event) => setLauncher((current) => ({
                      ...current,
                      show_greeting: event.target.checked,
                    }))}
                  />
                  <span>
                    <strong>{t("channels.showGreeting")}</strong>
                    <span>{t("channels.showGreetingHelp")}</span>
                  </span>
                </label>
                <label className="widget-notification-setting">
                  <input
                    type="checkbox"
                    disabled={!canManage}
                    checked={launcher.show_operator_profile}
                    onChange={(event) => setLauncher((current) => ({
                      ...current,
                      show_operator_profile: event.target.checked,
                    }))}
                  />
                  <span>
                    <strong>{t("channels.showOperatorProfile")}</strong>
                    <span>{t("channels.showOperatorProfileHelp")}</span>
                  </span>
                </label>
                <div className="widget-invitation-control">
                  <label className="widget-notification-setting">
                    <input
                      type="checkbox"
                      disabled={!canManage}
                      checked={launcher.proactive_invitation_enabled}
                      onChange={(event) => setLauncher((current) => ({
                        ...current,
                        proactive_invitation_enabled: event.target.checked,
                      }))}
                    />
                    <span>
                      <strong>{t("channels.proactiveInvitation")}</strong>
                      <span>{t("channels.proactiveInvitationHelp")}</span>
                    </span>
                  </label>
                  {launcher.proactive_invitation_enabled && (
                    <div className="widget-invitation-settings">
                    <div className="launcher-fields">
                      <label>{t("channels.proactiveInvitationDelay")}
                        <input
                          type="number"
                          min={1}
                          max={300}
                          step={1}
                          required
                          readOnly={!canManage}
                          value={launcher.proactive_invitation_delay_seconds}
                          onChange={(event) => {
                            const seconds = Math.min(
                              300,
                              Math.max(1, Math.round(event.currentTarget.valueAsNumber || 1)),
                            );
                            setLauncher((current) => ({
                              ...current,
                              proactive_invitation_delay_seconds: seconds,
                            }));
                          }}
                        />
                        <span>{t("channels.proactiveInvitationDelayHelp")}</span>
                      </label>
                    </div>
                    <div className="widget-invitation-translations">
                      {translations.map((translation) => (
                        <label key={translation.id}>
                          {t("channels.proactiveInvitationMessage")} · {normalizeWidgetLanguage(translation.code) ?? t("channels.newLanguage")}
                          <textarea
                            rows={2}
                            maxLength={240}
                            required
                            readOnly={!canManage}
                            value={translation.proactiveInvitationMessage}
                            onChange={(event) => updateTranslation(
                              translation.id,
                              "proactiveInvitationMessage",
                              event.target.value,
                            )}
                          />
                          <span>{t("channels.proactiveInvitationMessageHelp")}</span>
                        </label>
                      ))}
                    </div>
                    </div>
                  )}
                </div>
              </div>
              <aside className={`launcher-preview launcher-preview--${launcher.position}`} aria-label={t("channels.launcherPreview")} style={{
                "--launcher-preview-shadow": previewPalette.shadow_color,
                fontFamily: widgetFontStack(theme.font_family),
              } as React.CSSProperties}>
                <div className="launcher-preview-stack">
                  {launcher.proactive_invitation_enabled && (
                    <div className="launcher-preview-cta" style={{
                      background: previewPalette.surface_color,
                      color: previewPalette.text_color,
                      borderColor: previewPalette.divider_color,
                    }}>
                      <strong>{previewTranslation?.supportName.trim() || "Support"}</strong>
                      <span>{previewTranslation?.proactiveInvitationMessage}</span>
                    </div>
                  )}
                  <button key={`${activeSection}-${launcher.launcher_type}-${launcherAttentionAnimation}-${launcher.animation_interval_seconds}-${launcherPreviewCycle}`} className={`launcher-preview-button launcher-preview-button--${launcher.launcher_type} launcher-preview-button--attention-${launcherAttentionAnimation}`} type="button" tabIndex={-1} style={{
                    "--launcher-preview-accent": previewPalette.launcher_background_color,
                    background: previewPalette.launcher_background_color,
                    color: previewPalette.launcher_icon_color,
                    borderRadius: launcher.launcher_type === "icon" ? "50%" : `${Math.min(theme.border_radius, 26)}px`,
                  } as React.CSSProperties}>
                    {launcher.launcher_type !== "text" && <WidgetButtonIcon icon={theme.launcher_icon} />}
                    {launcher.launcher_type !== "icon" && <span>{previewTranslation?.launcherLabel || t("channels.launcherDefaultLabel")}</span>}
                  </button>
                </div>
              </aside>
            </section>
            <div className="theme-settings" id={`${instanceId}-widget-panel-Appearance`} role="tabpanel" data-section="Appearance" aria-labelledby={`${instanceId}-widget-tab-Appearance`} hidden={activeSection !== "Appearance"}>
              <div className="theme-controls">
                <div className="widget-theme-heading">
                  <div className="field-heading"><strong>{t("channels.widgetColors")}</strong><span>{t("channels.colorsHelp")}</span></div>
                  <div className="widget-theme-switcher" role="group" aria-label={t("channels.themePalette")}>
                    {(["light", "dark"] as const).map((colorScheme) => (
                      <button
                        className={previewColorScheme === colorScheme ? "widget-theme-option widget-theme-option--active" : "widget-theme-option"}
                        type="button"
                        aria-label={t(colorScheme === "light" ? "channels.editLightTheme" : "channels.editDarkTheme")}
                        aria-pressed={previewColorScheme === colorScheme}
                        key={colorScheme}
                        onClick={() => setPreviewColorScheme(colorScheme)}
                      >
                        {t(colorScheme === "light" ? "theme.light" : "theme.dark")}
                      </button>
                    ))}
                  </div>
                </div>
                <div className="widget-font-settings">
                  <label>
                    {t("channels.fontFamily")}
                    <select
                      disabled={!canManage}
                      value={theme.font_family}
                      aria-describedby={`${instanceId}-widget-font-help`}
                      onChange={(event) => {
                        const fontFamily = WIDGET_FONT_FAMILIES.find((family) => family === event.target.value) ?? "system";
                        setTheme((current) => ({ ...current, font_family: fontFamily }));
                      }}
                    >
                      {WIDGET_FONT_FAMILIES.map((family) => (
                        <option key={family} value={family}>
                          {family === "system" ? t("channels.fontSystem") : widgetFontNames[family]}
                        </option>
                      ))}
                    </select>
                  </label>
                  <p className="widget-font-help" id={`${instanceId}-widget-font-help`}>{t("channels.fontFamilyHelp")}</p>
                  <label className="toggle-row">
                    <input
                      type="checkbox"
                      disabled={!canManage}
                      checked={theme.use_site_font}
                      onChange={(event) => setTheme((current) => ({ ...current, use_site_font: event.target.checked }))}
                    />
                    <span>
                      <strong>{t("channels.useSiteFont")}</strong>
                      <span>{t("channels.useSiteFontHelp")}</span>
                    </span>
                  </label>
                </div>
                
                <label className="toggle-row">
                  <input
                    type="checkbox"
                    disabled={!canManage}
                    checked={theme.reply_typing_effect}
                    onChange={(event) => setTheme((current) => ({ ...current, reply_typing_effect: event.target.checked }))}
                  />
                  <span>
                    <strong>{t("channels.replyTypingEffect")}</strong>
                    <span>{t("channels.replyTypingEffectHelp")}</span>
                  </span>
                </label>
                <div className="color-groups">
                  {colorGroups.map((group) => (
                    <fieldset className="color-group" key={group.label}>
                      <legend>{t(group.label)}</legend>
                      {group.icon && (
                        <label className="widget-icon-field">{t(group.icon.label)}
                          <select
                            disabled={!canManage}
                            value={theme[group.icon.key]}
                            onChange={(event) => {
                              const icon = group.icon;
                              if (!icon) return;
                              const value = icon.options.find((option) => option === event.target.value);
                              if (value) setTheme((current) => ({ ...current, [icon.key]: value }));
                            }}
                          >
                            {group.icon.options.map((icon) => (
                              <option key={icon} value={icon}>{t(`channels.icon.${icon}`)}</option>
                            ))}
                          </select>
                        </label>
                      )}
                      <div className="color-grid">
                        {group.fields.map((field) => (
                          <label className="color-field" key={field.key}>
                            <span>{t(field.label)}</span>
                            <div>
                              <input type="color" disabled={!canManage} value={previewPalette[field.key]} aria-label={t("channels.colorPicker", { label: t(field.label) })} onChange={(event) => updateThemeColor(field.key, event.target.value.toUpperCase())} />
                              <input className="color-value" readOnly={!canManage} value={previewPalette[field.key]} pattern="#[0-9A-Fa-f]{6}" maxLength={7} aria-label={t("channels.hexColor", { label: t(field.label) })} onChange={(event) => updateThemeColor(field.key, event.target.value)} />
                            </div>
                          </label>
                        ))}
                      </div>
                    </fieldset>
                  ))}
                </div>
                <div className="widget-border-settings">
                  <label className="toggle-row">
                    <input
                      type="checkbox"
                      disabled={!canManage}
                      checked={theme.border.enabled}
                      onChange={(event) => setTheme((current) => ({
                        ...current,
                        border: { ...current.border, enabled: event.target.checked },
                      }))}
                    />
                    <span>
                      <strong>{t("channels.showBorder")}</strong>
                      <span>{t("channels.showBorderHelp")}</span>
                    </span>
                  </label>
                  {theme.border.enabled && (
                    <div className="widget-border-fields">
                      <label>{t("channels.borderWidth")}
                        <select
                          disabled={!canManage}
                          value={theme.border.width}
                          onChange={(event) => setTheme((current) => ({
                            ...current,
                            border: { ...current.border, width: Number(event.target.value) as 1 | 2 },
                          }))}
                        >
                          <option value={1}>1 px</option>
                          <option value={2}>2 px</option>
                        </select>
                      </label>
                      <label className="color-field">
                        <span>{t("channels.borderColor")}</span>
                        <div>
                          <input type="color" disabled={!canManage} value={previewBorderColor} aria-label={t("channels.colorPicker", { label: t("channels.borderColor") })} onChange={(event) => updateBorderColor(event.target.value.toUpperCase())} />
                          <input className="color-value" readOnly={!canManage} value={previewBorderColor} pattern="#[0-9A-Fa-f]{6}" maxLength={7} aria-label={t("channels.hexColor", { label: t("channels.borderColor") })} onChange={(event) => updateBorderColor(event.target.value)} />
                        </div>
                      </label>
                    </div>
                  )}
                </div>
                <label className="radius-field">
                  <span>{t("channels.borderRadius")}</span>
                  <input
                    type="number"
                    min={0}
                    max={32}
                    step={1}
                    required
                    readOnly={!canManage}
                    aria-label={t("channels.borderRadius")}
                    value={theme.border_radius}
                    onChange={(event) => {
                      const borderRadius = Math.min(
                        32,
                        Math.max(0, Math.round(event.currentTarget.valueAsNumber || 0)),
                      );
                      setTheme((current) => ({ ...current, border_radius: borderRadius }));
                    }}
                  />
                  <span>{t("channels.borderRadiusHelp")}</span>
                </label>
                <div className="footer-settings">
                  <label className="toggle-row">
                    <input
                      type="checkbox"
                      disabled={!canManage}
                      checked={footerVisible}
                      onChange={(event) => setTheme((current) => ({
                        ...current,
                        footer_text: event.target.checked ? null : "",
                      }))}
                    />
                    <span>
                      <strong>{t("channels.showFooter")}</strong>
                      <span>{t("channels.showFooterHelp")}</span>
                    </span>
                  </label>
                  {footerVisible && (
                    <label>{t("channels.footerText")}
                      <input
                        maxLength={160}
                        readOnly={!canManage}
                        value={theme.footer_text ?? ""}
                        placeholder={defaultFooterText}
                        onChange={(event) => setTheme((current) => ({ ...current, footer_text: event.target.value || null }))}
                      />
                      <span>{t("channels.footerTextHelp")}</span>
                    </label>
                  )}
                </div>
              </div>
              <div className="widget-appearance-examples">
                <WidgetAppearancePreview
                  active={activeSection === "Appearance"}
                  position={launcher.position}
                  title={t("channels.preview")}
                  theme={theme}
                  colorScheme={previewColorScheme}
                  supportName={previewTranslation?.supportName.trim() || "Support"}
                  onlineNow={previewTranslation?.onlineNow || t("channels.online")}
                  greeting={previewTranslation?.greeting || t("channels.previewGreeting")}
                  showGreeting={launcher.show_greeting}
                  messageText={t("channels.previewMessage")}
                  replyText={t("channels.previewReply")}
                  ratingPrompt={previewTranslation?.ratingPrompt || ""}
                  messagePlaceholder={previewTranslation?.messagePlaceholder || t("channels.writeMessage")}
                  footerText={footerVisible ? theme.footer_text || defaultFooterText : ""}
                  attachmentsEnabled={attachmentsEnabled}
                />
                <div className={`widget-appearance-launcher widget-appearance-launcher--${launcher.position}`} aria-label={t("channels.launcherPreview")}>
                  <button
                    className={`launcher-preview-button launcher-preview-button--${launcher.launcher_type}`}
                    type="button"
                    tabIndex={-1}
                    aria-label={previewTranslation?.launcherLabel || t("channels.launcherDefaultLabel")}
                    style={{
                      "--launcher-preview-shadow": previewPalette.shadow_color,
                      background: previewPalette.launcher_background_color,
                      color: previewPalette.launcher_icon_color,
                      borderRadius: launcher.launcher_type === "icon" ? "50%" : `${Math.min(theme.border_radius, 26)}px`,
                      fontFamily: widgetFontStack(theme.font_family),
                    } as React.CSSProperties}
                  >
                    {launcher.launcher_type !== "text" && <WidgetButtonIcon icon={theme.launcher_icon} />}
                    {launcher.launcher_type !== "icon" && <span>{previewTranslation?.launcherLabel || t("channels.launcherDefaultLabel")}</span>}
                  </button>
                </div>
              </div>
            </div>
          </div>
        </form>
      </section>
    </div>
  );
}

/**
 * Channels URLs: `/channels` (catalog), `/channels/widgets`, `/channels/widgets/<id|new>[/languages|/launcher|/appearance|/install]`,
 * `/channels/telegram-bots`, `/channels/email`, `/channels/custom-ai/<id|new>` and `/channels/blacklist/<id>`.
 */
function channelsRoute(segments: readonly string[]): ChannelsScreen {
  const [name, id, section] = segments;
  if (name === "widgets") return id ? { name: "widget", channelId: id, section: widgetEditorSections.find((item) => item.toLowerCase() === section) ?? "General" } : { name };
  if (name === "telegram-bots" || name === "email") return { name };
  if ((name === "custom-ai" || name === "blacklist") && id) return { name, channelId: id };
  return { name: "catalog" };
}

function channelsSegments(screen: ChannelsScreen): string[] {
  if (screen.name === "catalog") return [];
  if (screen.name === "widget") return screen.section === "General" ? ["widgets", screen.channelId] : ["widgets", screen.channelId, screen.section.toLowerCase()];
  if (screen.name === "custom-ai" || screen.name === "blacklist") return [screen.name, screen.channelId];
  return [screen.name];
}

/** The screen shown for a URL: a record or screen that is unavailable falls back to its list or the catalog. */
function availableChannelsScreen(screen: ChannelsScreen, channels: ChannelConnection[], customChannels: CustomAiChannel[], canManage: boolean, showDefaultChannels: boolean): ChannelsScreen {
  if (screen.name === "catalog") return screen;
  if (screen.name === "custom-ai") return (screen.channelId === NEW_CHANNEL ? canManage : customChannels.some((channel) => channel.id === screen.channelId)) ? screen : { name: "catalog" };
  // The department's catalog setting hides every standard channel screen.
  if (!showDefaultChannels) return { name: "catalog" };
  if (screen.name === "widget") return (screen.channelId === NEW_CHANNEL ? canManage : channels.some((channel) => channel.id === screen.channelId && channel.kind === "widget")) ? screen : { name: "widgets" };
  if (screen.name === "blacklist") return channels.some((channel) => channel.id === screen.channelId && channel.kind !== "custom_ai") ? screen : { name: "catalog" };
  return screen;
}

export function ChannelsView({ auth, inboxes, canManage, showDefaultChannels = true }: ChannelsViewProps) {
  const { t } = useI18n();
  const route = usePageRoute();
  const navigateRoute = route.navigate;
  const routeSegmentsRef = useRef(route.segments);
  const [channels, setChannels] = useState<ChannelConnection[]>([]);
  const [customChannels, setCustomChannels] = useState<CustomAiChannel[]>([]);
  const [customLoaded, setCustomLoaded] = useState(false);
  const [customLoadError, setCustomLoadError] = useState(false);
  const [customRevision, setCustomRevision] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<MessageKey | null>(null);
  // A widget's notice stays with that widget, however the operator returns to it.
  const [notice, setNotice] = useState<{ channelId: string; message: MessageKey } | null>(null);
  const [listNotice, setListNotice] = useState<ChannelsNotice | null>(null);
  const [deletingChannelId, setDeletingChannelId] = useState<string | null>(null);
  const widgets = useMemo(() => channels.filter((channel) => channel.kind === "widget"), [channels]);
  const telegramBots = useMemo(() => channels.filter((channel) => channel.kind === "telegram_bot"), [channels]);
  const replaceChannels = useCallback((items: ChannelConnection[]) => setChannels(items), []);
  const requested = channelsRoute(route.segments);
  // A custom channel's link waits for the custom channel list; when that fails, the catalog reports the error.
  const customPending = requested.name === "custom-ai" && requested.channelId !== NEW_CHANNEL && !customLoaded;
  const screen: ChannelsScreen = customPending
    ? customLoadError ? { name: "catalog" } : requested
    : availableChannelsScreen(requested, channels, customChannels, canManage, showDefaultChannels);

  useEffect(() => { routeSegmentsRef.current = route.segments; }, [route.segments]);
  useCanonicalPageRoute(route, channelsSegments(screen), !loading && !loadError && !customPending);

  useEffect(() => {
    let active = true;
    listChannels(auth)
      .then((items) => { if (active) setChannels(items); })
      .catch(() => { if (active) setLoadError("channels.loadError"); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [auth]);

  useEffect(() => {
    let active = true;
    listCustomAiChannels(auth).then((items) => { if (active) { setCustomChannels(items); setCustomLoaded(true); setCustomLoadError(false); } }).catch(() => { if (active) setCustomLoadError(true); });
    return () => { active = false; };
  }, [auth, customRevision]);

  async function handleDeleteWidget(channel: ChannelConnection) {
    if (!window.confirm(t("channels.deleteConfirm", { name: channel.name }))) return;

    setDeletingChannelId(channel.id);
    setListNotice(null);
    try {
      await deleteChannel(auth, channel.id);
      setChannels((items) => items.filter((item) => item.id !== channel.id));
      setListNotice({ kind: "success", message: "channels.deleted" });
    } catch (cause) {
      setListNotice({
        kind: "error",
        message: cause instanceof ApiRequestError && cause.status === 401
          ? "channels.deleteAuthError"
          : "channels.deleteError",
      });
    } finally {
      setDeletingChannelId(null);
    }
  }

  if (loading || (customPending && !customLoadError)) return <div className="page channels-page channels-page--table"><header className="page-toolbar"><div><h1>{t("channels.title")}</h1><p>{t("channels.catalogDescription")}</p></div></header><div className="empty-state">{t("channels.loading")}</div></div>;
  if (loadError) return <div className="page channels-page channels-page--table"><header className="page-toolbar"><div><h1>{t("channels.title")}</h1><p>{t("channels.catalogDescription")}</p></div></header><div className="admin-notice admin-notice--error" role="alert">{t(loadError)}</div></div>;

  const openBlacklist = (channelId: string) => void navigateRoute(channelsSegments({ name: "blacklist", channelId }));
  const openCatalog = () => void navigateRoute([]);

  if (screen.name === "blacklist") {
    const channel = channels.find((item) => item.id === screen.channelId);
    if (channel) return <ChannelBlacklistSettings key={channel.id} auth={auth} channel={channel} canManage={canManage} onBack={openCatalog} onUpdated={(updated) => setChannels((items) => items.map((item) => item.id === updated.id ? updated : item))} />;
  }
  if (screen.name === "widgets") {
    return (
      <WidgetsList
        widgets={widgets}
        inboxes={inboxes}
        canManage={canManage}
        notice={listNotice}
        deletingChannelId={deletingChannelId}
        onBack={openCatalog}
        onCreate={() => { setNotice(null); setListNotice(null); void navigateRoute(channelsSegments({ name: "widget", channelId: NEW_CHANNEL, section: "General" })); }}
        onEdit={(channelId) => { setNotice(null); setListNotice(null); void navigateRoute(channelsSegments({ name: "widget", channelId, section: "General" })); }}
        onDelete={(channel) => { void handleDeleteWidget(channel); }}
        onBlacklist={openBlacklist}
      />
    );
  }
  if (screen.name === "email") {
    return <EmailSettingsView auth={auth} canManage={canManage} onBack={openCatalog} />;
  }
  if (screen.name === "custom-ai") {
    const selected = customChannels.find((channel) => channel.id === screen.channelId) ?? null;
    return <CustomAiChannelsView key={screen.channelId} auth={auth} inboxes={inboxes} channel={selected} canManage={canManage} onBack={openCatalog} onSaved={(channel) => {
      setCustomChannels((items) => items.some((item) => item.id === channel.id) ? items.map((item) => item.id === channel.id ? channel : item) : [...items, channel]);
      openCatalog();
      void listChannels(auth).then(replaceChannels).catch(() => undefined);
    }} onDeleted={() => {
      setCustomChannels((items) => items.filter((item) => item.id !== screen.channelId));
      openCatalog();
      void listChannels(auth).then(replaceChannels).catch(() => undefined);
    }} />;
  }
  if (screen.name === "telegram-bots") {
    return <TelegramBotsView auth={auth} inboxes={inboxes} bots={telegramBots} canManage={canManage} onBlacklist={openBlacklist} onBack={openCatalog} onCreated={(created) => setChannels((items) => [...items, created])} onDeleted={(channelId) => setChannels((items) => items.filter((item) => item.id !== channelId))} onReloaded={replaceChannels} />;
  }
  if (screen.name === "widget") {
    const selected = widgets.find((widget) => widget.id === screen.channelId) ?? null;
    return (
      <WidgetEditor
        key={screen.channelId}
        auth={auth}
        inboxes={inboxes}
        channel={selected}
        canManage={canManage}
        notice={selected && notice?.channelId === selected.id ? notice.message : null}
        section={screen.section}
        // Sections share one draft, so switching them does not add history entries.
        onSectionChange={(section) => void navigateRoute(channelsSegments({ ...screen, section }), { replace: true })}
        onBack={() => { setNotice(null); void navigateRoute(channelsSegments({ name: "widgets" })); }}
        onCreated={(created) => {
          setChannels((items) => [...items, created]);
          setNotice({ channelId: created.id, message: "channels.created" });
          // The saved widget takes the creation form's place in history, in the section open now.
          const current = channelsRoute(routeSegmentsRef.current);
          if (current.name === "widget" && current.channelId === NEW_CHANNEL) void navigateRoute(channelsSegments({ ...current, channelId: created.id }), { replace: true });
        }}
        onUpdated={(updated) => {
          setChannels((items) => items.map((item) => item.id === updated.id ? updated : item));
          setNotice({ channelId: updated.id, message: "channels.saved" });
        }}
      />
    );
  }
  return <ChannelsCatalog channels={channels} customChannels={customChannels} customLoadError={customLoadError} canManage={canManage} showDefaultChannels={showDefaultChannels} onCreate={() => void navigateRoute(channelsSegments({ name: "custom-ai", channelId: NEW_CHANNEL }))} onOpenCustom={(channelId) => void navigateRoute(channelsSegments({ name: "custom-ai", channelId }))} onReloadCustom={() => setCustomRevision((value) => value + 1)} onBlacklist={openBlacklist} onOpen={(channelType) => void navigateRoute(channelsSegments(channelType === "widgets" ? { name: "widgets" } : channelType === "telegram" ? { name: "telegram-bots" } : { name: "email" }))} />;
}
