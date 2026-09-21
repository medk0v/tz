import {
  generateKnowledgeArticleDraft,
  generateReplySuggestion,
  generateReplyTemplateDraft,
  listKnowledgeArticleDraftAgents,
  listReplySuggestionAgents,
  listReplyTemplateDraftAgents,
  type OperatorAuth,
  type ReplySuggestionAgent,
} from "../api";
import type { MessageKey } from "../i18n";

export interface AiDraftText {
  button: MessageKey;
  choose: MessageKey;
  empty: MessageKey;
  generateError: MessageKey;
  insertTooLong: MessageKey;
}

/** What a chosen agent drafts, and how the agent menu describes it. */
export interface AiDraftSource {
  /** Identifies the drafted content; another target discards an unfinished generation. */
  target: string;
  text: AiDraftText;
  listAgents: (signal: AbortSignal) => Promise<ReplySuggestionAgent[]>;
  /** Resolves to Markdown to insert; a non-empty draft is continued, not replaced. */
  generate: (agentId: string, draftBody: string, signal: AbortSignal) => Promise<string>;
}

const contentDraftText = {
  button: "aiDraft.button",
  choose: "aiDraft.choose",
  empty: "aiDraft.empty",
  generateError: "aiDraft.generateError",
} as const;

export function conversationReplySource(auth: OperatorAuth, conversationId: string): AiDraftSource {
  return {
    target: `conversation:${conversationId}`,
    text: {
      button: "aiReplySuggestion.button",
      choose: "aiReplySuggestion.choose",
      empty: "aiReplySuggestion.empty",
      generateError: "aiReplySuggestion.generateError",
      insertTooLong: "aiReplySuggestion.insertTooLong",
    },
    listAgents: (signal) => listReplySuggestionAgents(auth, conversationId, signal),
    generate: async (agentId, draftBody, signal) =>
      (await generateReplySuggestion(auth, conversationId, agentId, draftBody, signal)).body,
  };
}

export function replyTemplateDraftSource(auth: OperatorAuth, inboxId: string, title: string): AiDraftSource {
  return {
    target: `reply-template:${inboxId}`,
    text: { ...contentDraftText, insertTooLong: "aiDraft.templateTooLong" },
    listAgents: (signal) => listReplyTemplateDraftAgents(auth, inboxId, signal),
    generate: async (agentId, draftBody, signal) =>
      (await generateReplyTemplateDraft(auth, inboxId, agentId, title, draftBody, signal)).body,
  };
}

export function knowledgeArticleDraftSource(auth: OperatorAuth, knowledgeBaseId: string, title: string): AiDraftSource {
  return {
    target: `knowledge-article:${knowledgeBaseId}`,
    text: { ...contentDraftText, insertTooLong: "aiDraft.articleTooLong" },
    listAgents: (signal) => listKnowledgeArticleDraftAgents(auth, knowledgeBaseId, signal),
    generate: async (agentId, draftBody, signal) =>
      (await generateKnowledgeArticleDraft(auth, knowledgeBaseId, agentId, title, draftBody, signal)).body,
  };
}
