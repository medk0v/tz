import { AnimatedDisclosure } from "../AnimatedDisclosure";
import { AnimatedDetails } from "../AnimatedDetails";
import { useEffect, useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight, PanelRightClose } from "lucide-react";
import { listInboxConversationPage, type OperatorAuth, type OperatorConversation, type VisitorIntelligence } from "../api";
import { conversationStatusLabel, useI18n } from "../i18n";
import { ContactBlockButton } from "./ContactBlockButton";
import { OperatorAvatar } from "./OperatorAvatar";
import { VisitorIntelligenceDetails } from "./VisitorIntelligenceDetails";
import { conversationName, conversationPreview } from "./conversation-presentation";
import { useContentMotion } from "./useContentMotion";

interface ConversationContactDetailsProps {
  auth: OperatorAuth;
  isDemo?: boolean;
  conversation: OperatorConversation;
  intelligence: VisitorIntelligence | null;
  visitorStatus: "loading" | "ready" | "unavailable";
  realtimeSyncRevision: number;
  channelLabel: ReactNode;
  attachmentsDisabled: boolean;
  canManageContacts: boolean;
  onContactBlockChange: (blocked: boolean) => void;
  onAttachmentPolicyChange: (enabled: boolean) => void;
  onSelect: (conversation: OperatorConversation) => void;
  onClose: () => void;
}

function safeExternalUrl(value: string | null | undefined): string | null {
  if (!value) return null;
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : null;
  } catch {
    return null;
  }
}

function ChannelAttribution({ intelligence }: { intelligence: VisitorIntelligence | null }) {
  const { t } = useI18n();
  const latestContext = intelligence?.observations[0]?.client_context;
  const links = [
    { key: "page", label: t("visitor.pageUrl"), url: safeExternalUrl(latestContext?.page.url) },
    { key: "referrer", label: t("visitor.referrer"), url: safeExternalUrl(latestContext?.page.referrer) },
  ].filter((link): link is { key: string; label: string; url: string } => link.url !== null);

  if (links.length === 0) return null;
  return <span className="conversation-channel-attribution">
    {links.map((link) => <span key={link.key}>
      <span>{link.label}</span>
      <a href={link.url} target="_blank" rel="noopener noreferrer">{link.url}</a>
    </span>)}
  </span>;
}

function UserNavigation({ intelligence }: { intelligence: VisitorIntelligence | null }) {
  const { t, formatDate } = useI18n();
  const pages = (intelligence?.observations ?? []).flatMap((observation) => {
    const context = observation.client_context;
    const url = safeExternalUrl(context?.page.url);
    if (!url) return [];
    const parsed = new URL(url);
    return [{
      id: observation.session_id,
      capturedAt: observation.captured_at,
      title: context?.page.title || parsed.hostname,
      displayUrl: `${parsed.host}${parsed.pathname}${parsed.search}`,
      url,
    }];
  });

  if (pages.length === 0) return null;
  return <AnimatedDetails className="conversation-user-navigation">
    <summary>
      <span>{t("conversations.userNavigation")}</span>
      <span className="conversation-user-navigation-count">{pages.length}</span>
      <ChevronDown size={16} aria-hidden="true" />
    </summary>
    <ol>
      {pages.map((page) => <li key={page.id}>
        <time dateTime={page.capturedAt}>{formatDate(page.capturedAt, { hour: "2-digit", minute: "2-digit" })}</time>
        <a href={page.url} target="_blank" rel="noopener noreferrer">
          <strong>{page.title}</strong>
          <span>{page.displayUrl}</span>
        </a>
      </li>)}
    </ol>
  </AnimatedDetails>;
}

export function ConversationContactDetails({ auth, isDemo = false, conversation, intelligence, visitorStatus,
  realtimeSyncRevision, channelLabel, attachmentsDisabled, canManageContacts, onContactBlockChange, onAttachmentPolicyChange, onSelect, onClose,
}: ConversationContactDetailsProps) {
  const { t, formatDate } = useI18n();
  const [historyOpen, setHistoryOpen] = useState(false);
  const [history, setHistory] = useState<{ items: OperatorConversation[]; has_more: boolean } | null>(null);
  const [historyError, setHistoryError] = useState(false);
  const [retry, setRetry] = useState(0);
  const detailsMotion = useContentMotion<HTMLElement>(conversation.id);

  useEffect(() => {
    if (!historyOpen) return;
    const controller = new AbortController();
    listInboxConversationPage(auth, conversation.inbox_id, {
      contactId: conversation.contact_id, status: "all", signal: controller.signal,
    }).then((page) => {
      if (!controller.signal.aborted) {
        setHistory(page);
        setHistoryError(false);
      }
    }).catch(() => {
      if (!controller.signal.aborted) setHistoryError(true);
    });
    return () => controller.abort();
  }, [auth, conversation.contact_id, conversation.inbox_id, historyOpen, realtimeSyncRevision, retry]);

  return (
    <aside ref={detailsMotion} className="conversation-details" id="conversation-contact-details" aria-label={t("conversations.detailContact")} onKeyDown={(event) => {
      if (event.key === "Escape") { event.stopPropagation(); onClose(); }
    }}>
      <header className="conversation-details-header">
        <h2>{t("conversations.detailContact")}</h2>
        <button type="button" className="icon-button conversation-details-close" onClick={onClose} aria-label={t("conversations.hideDetails")}>
          <PanelRightClose size={17} aria-hidden="true" />
        </button>
      </header>
      <div className="conversation-contact-identity">
        <OperatorAvatar displayName={conversationName(conversation, t)} avatarUrl={null} size="large" />
        <div>
          <strong>{conversationName(conversation, t)}</strong>
          {conversation.contact?.email && conversation.contact.email !== conversation.contact.display_name && <span>{conversation.contact.email}</span>}
          {conversation.telegram?.username && <span>@{conversation.telegram.username}</span>}
          {conversation.contact.is_blocked && <span className="contact-blocked-badge">{t("contacts.blocked")}</span>}
        </div>
      </div>
      <dl>
        <div><dt>{t("conversations.detailStatus")}</dt><dd>{conversationStatusLabel(t, conversation.status)}</dd></div>
        <div><dt>{t("conversations.detailOperator")}</dt><dd>{conversation.operator?.display_name ?? t("conversations.unassigned")}</dd></div>
        {conversation.ai_agent?.active && <div><dt>{t("conversations.detailAiAgent")}</dt><dd>{conversation.ai_agent.display_name}</dd></div>}
        <div><dt>{t("conversations.detailChannel")}</dt><dd>{channelLabel}<ChannelAttribution intelligence={intelligence} /></dd></div>
        <div><dt>{t("conversations.detailCreated")}</dt><dd>{formatDate(conversation.created_at, { day: "numeric", month: "long", hour: "2-digit", minute: "2-digit" })}</dd></div>
      </dl>
      <UserNavigation intelligence={intelligence} />
      {canManageContacts && <section className="conversation-contact-policy">
        <ContactBlockButton auth={auth} contactId={conversation.contact_id} blocked={conversation.contact.is_blocked} onUpdated={onContactBlockChange} />
        {conversation.contact.is_blocked && <p className="field-help">{t("contacts.blockHelp")}</p>}
      </section>}
      <section className="conversation-contact-history">
        <h3>{t("conversations.contactHistory")}</h3>
        <button className="conversation-history-toggle" type="button" aria-expanded={historyOpen} aria-controls="conversation-contact-history" onClick={() => setHistoryOpen((open) => !open)}>
          {conversation.contact_conversation_count == null ? t("conversations.showHistory") : t("conversations.historyCount", { count: conversation.contact_conversation_count })}
          {historyOpen ? <ChevronDown size={16} aria-hidden="true" /> : <ChevronRight size={16} aria-hidden="true" />}
        </button>
        <AnimatedDisclosure open={historyOpen} id="conversation-contact-history" className="conversation-history-list">
          {historyError ? <div role="status"><p>{t("conversations.historyError")}</p><button type="button" className="secondary-button" onClick={() => setRetry((value) => value + 1)}>{t("conversations.refresh")}</button></div>
            : !history ? <p role="status">{t("conversations.historyLoading")}</p>
              : <>
                  {history.items.map((item) => <button type="button" key={item.id} aria-current={item.id === conversation.id ? "true" : undefined} onClick={() => onSelect(item)}>
                    <span>{formatDate(item.created_at, { month: "short", day: "numeric" })} · {conversationStatusLabel(t, item.status)}</span>
                    <strong>{item.subject || conversationPreview(item, t)}</strong>
                  </button>)}
                  {history.items.length === 0 && <p>{t("conversations.empty")}</p>}
                  {history.has_more && <p>{t("conversations.historyLimited")}</p>}
                </>}
        </AnimatedDisclosure>
      </section>
      {conversation.channel.kind === "widget" && <section className="conversation-contact-policy">
        <label className="conversation-attachment-policy">
          <input type="checkbox" checked={conversation.widget_attachments_enabled} disabled={isDemo || attachmentsDisabled} onChange={(event) => onAttachmentPolicyChange(event.target.checked)} />
          <span>{t("conversations.visitorAttachments")}</span>
        </label>
      </section>}
      <AnimatedDetails className="conversation-technical-details">
        <summary>{t("conversations.technicalDetails")}<ChevronDown size={16} aria-hidden="true" /></summary>
        <dl>
          <div><dt>{t("conversations.detailContactId")}</dt><dd>{conversation.contact_id}</dd></div>
          <div><dt>{t("conversations.detailConversation")}</dt><dd>{conversation.id}</dd></div>
          <div><dt>{t("conversations.detailMessages")}</dt><dd>{conversation.last_message_sequence}</dd></div>
          {conversation.telegram?.user_id && <div><dt>{t("conversations.telegramUserId")}</dt><dd>{conversation.telegram.user_id}</dd></div>}
          {conversation.telegram?.chat_id && <div><dt>{t("conversations.telegramChatId")}</dt><dd>{conversation.telegram.chat_id}</dd></div>}
          {conversation.telegram?.language_code && <div><dt>{t("conversations.telegramLanguage")}</dt><dd>{conversation.telegram.language_code}</dd></div>}
        </dl>
        <section className="visitor-intelligence-section">
          <h3>{t("visitor.title")}</h3>
          <VisitorIntelligenceDetails intelligence={intelligence} status={visitorStatus} />
        </section>
      </AnimatedDetails>
    </aside>
  );
}
