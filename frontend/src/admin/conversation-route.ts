import { isAdminPagePath, resolveAdminPage, resolveAdminPageSegments, type AdminPageSegments } from "../entry-mode";

/** An open conversation lives at `/cabinet/conversations/<inbox>/<conversation>`; the inbox scopes the lookup. */
export interface ConversationRoute {
  inboxId: string;
  conversationId: string;
}

export function conversationRoute(segments: AdminPageSegments): ConversationRoute | null {
  return segments.length === 2 ? { inboxId: segments[0], conversationId: segments[1] } : null;
}

export function conversationSegments(inboxId: string, conversationId: string): string[] {
  return [inboxId, conversationId];
}

/** The inbox of a conversation link in `pathname`, which takes priority over the remembered inbox. */
export function conversationLinkInboxId(pathname: string): string | null {
  if (!isAdminPagePath(pathname) || resolveAdminPage(pathname) !== "conversations") return null;
  return conversationRoute(resolveAdminPageSegments(pathname))?.inboxId ?? null;
}
