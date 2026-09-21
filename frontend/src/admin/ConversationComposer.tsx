import {
  lazy,
  Suspense,
  type ChangeEventHandler,
  type FormEventHandler,
  type ReactNode,
  type RefObject,
} from "react";
import { Paperclip, RefreshCw, Send } from "lucide-react";
import type { OperatorAuth, OperatorConversation } from "../api";
import { ATTACHMENT_ACCEPT } from "../attachment-files";
import { useI18n } from "../i18n";
import { conversationReplySource } from "./ai-draft-sources";
import "./ConversationComposer.css";

const ChatMessageEditor = lazy(() => import("./ChatMessageEditor"));

export interface ConversationComposerProps {
  auth: OperatorAuth;
  isDemo?: boolean;
  conversation: OperatorConversation;
  channelLabel: ReactNode;
  draft: string;
  visitorDraft: string;
  canReply: boolean;
  canReplyAsAi: boolean;
  canReplyAsOperator: boolean;
  /** An administrator writes as the operator who handles the conversation. */
  canReplyForOperator?: boolean;
  sending: boolean;
  uploading: boolean;
  attachmentInputRef: RefObject<HTMLInputElement | null>;
  onDraftChange: (value: string) => void;
  onSubmit: FormEventHandler<HTMLFormElement>;
  onAttachmentChange: ChangeEventHandler<HTMLInputElement>;
}

export function ConversationComposer({
  auth,
  isDemo = false,
  conversation,
  channelLabel,
  draft,
  visitorDraft,
  canReply,
  canReplyAsAi,
  canReplyAsOperator,
  canReplyForOperator = false,
  sending,
  uploading,
  attachmentInputRef,
  onDraftChange,
  onSubmit,
  onAttachmentChange,
}: ConversationComposerProps) {
  const { t } = useI18n();
  const replyingAi = canReplyAsAi && !canReplyAsOperator ? conversation.ai_agent : null;
  const replyingForOperator = canReplyForOperator && !canReplyAsOperator ? conversation.operator : null;
  const replyingAs = replyingAi
    ? {
        name: replyingAi.display_name,
        label: t("conversations.replyAsAi", { name: replyingAi.display_name }),
        context: t("conversations.replyingAsAi", { name: replyingAi.display_name }),
      }
    : replyingForOperator
      ? {
          name: replyingForOperator.display_name,
          label: t("conversations.replyAsOperator", { name: replyingForOperator.display_name }),
          context: t("conversations.replyingAsOperator", { name: replyingForOperator.display_name }),
        }
      : null;
  const label = replyingAs?.label ?? t("conversations.reply");
  const trimmedDraft = draft.trim();
  const maxMessageLength = conversation.channel.kind === "telegram_bot" ? 4_096 : 10_000;
  const tooLong = Array.from(trimmedDraft).length > maxMessageLength;
  const canAttach = !isDemo && conversation.assigned_to_me && conversation.status !== "resolved" && !uploading;

  return (
    <form
      className={`message-composer conversation-composer${trimmedDraft ? " message-composer--typing" : ""}${sending ? " message-composer--sending" : ""}`}
      aria-busy={sending || uploading}
      onSubmit={onSubmit}
    >
      {!canReply && conversation.status !== "resolved" && (
        <div className="conversation-reply-lock" role="status">
          {conversation.operator
            ? t("conversations.assignedToOperator", { name: conversation.operator.display_name })
            : conversation.ai_agent?.active
              ? t("conversations.assignedToAiAgent", { name: conversation.ai_agent.display_name })
              : t("conversations.joinToReply")}
        </div>
      )}
      {replyingAs && (
        <div className="conversation-reply-as-context" role="status">
          {replyingAs.context}
        </div>
      )}
      {visitorDraft && (
        <div className="visitor-live-draft conversation-composer-visitor-draft" aria-live="polite">
          <span className="visitor-live-draft-label">
            <i className="visitor-typing-dots" aria-hidden="true"><b /><b /><b /></i>
            {t("conversations.visitorDraft")}
          </span>
          <p className="visitor-live-draft-text">{visitorDraft}</p>
        </div>
      )}
      <div className="conversation-composer-heading">
        <span className="message-composer-label">{label}</span>
        <span className="conversation-composer-channel" title={t("conversations.replyThrough")}>
          <span className="visually-hidden">{t("conversations.replyThrough")} </span>
          {channelLabel}
        </span>
      </div>
      <Suspense fallback={<div className="message-editor-loading" role="status">{t("conversations.editorLoading")}</div>}>
        <ChatMessageEditor
          key={conversation.id}
          value={draft}
          label={label}
          placeholder={replyingAs
            ? t("conversations.replyAsPlaceholder", { name: replyingAs.name })
            : t("conversations.replyPlaceholder")}
          disabled={!canReply || sending}
          templateSource={{ auth, inboxId: conversation.inbox_id }}
          aiDraftSource={canReplyAsOperator || canReplyAsAi ? conversationReplySource(auth, conversation.id) : undefined}
          onChange={onDraftChange}
        />
      </Suspense>
      {tooLong && (
        <p className="message-editor-error" role="alert">
          {t("conversations.messageTooLong", { count: maxMessageLength })}
        </p>
      )}
      <footer className="conversation-composer-footer">
        {conversation.channel.kind === "widget" && (
          <>
            <input
              ref={attachmentInputRef}
              className="visually-hidden"
              type="file"
              accept={ATTACHMENT_ACCEPT}
              disabled={!canAttach}
              onChange={onAttachmentChange}
            />
            <button
              className="message-attachment-button"
              type="button"
              disabled={!canAttach}
              aria-label={uploading ? t("conversations.attachmentUploading") : t("conversations.attachmentAdd")}
              title={uploading ? t("conversations.attachmentUploading") : t("conversations.attachmentAdd")}
              onClick={() => attachmentInputRef.current?.click()}
            >
              {uploading ? <RefreshCw className="spin" size={17} /> : <Paperclip size={17} />}
            </button>
          </>
        )}
        <p className="message-editor-hint">{t("conversations.editorSendHint")}</p>
        <button
          className={`primary-button message-send-button${trimmedDraft && !sending ? " message-send-button--ready" : ""}${sending ? " message-send-button--sending" : ""}`}
          type="submit"
          disabled={sending || !canReply || !trimmedDraft || tooLong}
          aria-label={sending ? t("conversations.sending") : t("conversations.send")}
        >
          <Send size={16} />
          {sending ? t("conversations.sending") : t("conversations.send")}
        </button>
      </footer>
    </form>
  );
}
