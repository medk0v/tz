
export type AppRoute = "cabinet" | "rating" | "public_notes";
export type AdminPage =
  | "conversations"
  | "contacts"
  | "online_visitors"
  | "support_quality"
  | "inbox_routing"
  | "channels"
  | "ai"
  | "tasks"
  | "notes"
  | "knowledge_base"
  | "reply_templates"
  | "integrations"
  | "users"
  | "roles"
  | "access_tokens";

/** Path segments after a cabinet page's own path, e.g. `["agents", "<id>"]` in `/cabinet/ai/agents/<id>`. */
export type AdminPageSegments = readonly string[];

export const CABINET_PATH = "/cabinet";
export const RATING_PATH = "/rate-chat";
export const PUBLIC_NOTES_PATH = "/notes/share";
// Conversations own the bare cabinet path, so their nested pages need a named parent.
const CONVERSATION_PAGES_PATH = `${CABINET_PATH}/conversations`;

const ADMIN_PAGE_PATHS: Record<AdminPage, string> = {
  conversations: CABINET_PATH,
  contacts: `${CABINET_PATH}/contacts`,
  online_visitors: `${CABINET_PATH}/online-visitors`,
  support_quality: `${CABINET_PATH}/support-quality`,
  inbox_routing: `${CABINET_PATH}/inbox-routing`,
  channels: `${CABINET_PATH}/channels`,
  notes: `${CABINET_PATH}/notes`,
  knowledge_base: `${CABINET_PATH}/knowledge-base`,
  reply_templates: `${CABINET_PATH}/reply-templates`,
  tasks: `${CABINET_PATH}/tasks`,
  ai: `${CABINET_PATH}/ai`,
  integrations: `${CABINET_PATH}/integrations`,
  users: `${CABINET_PATH}/users`,
  roles: `${CABINET_PATH}/roles`,
  access_tokens: `${CABINET_PATH}/access-tokens`,
};

function normalizedPath(pathname: string): string {
  return pathname.length > 1 ? pathname.replace(/\/+$/, "") : pathname;
}

export function resolveAppRoute(pathname: string): AppRoute {
  const path = normalizedPath(pathname);
  if (path === PUBLIC_NOTES_PATH || path.startsWith(`${PUBLIC_NOTES_PATH}/`)) return "public_notes";
  if (path === RATING_PATH) return "rating";
  return "cabinet";
}

export function appRoutePath(route: AppRoute): string {
  if (route === "public_notes") return PUBLIC_NOTES_PATH;
  if (route === "rating") return RATING_PATH;
  return CABINET_PATH;
}

export function resolvePublicNoteToken(pathname: string): string | null {
  const match = normalizedPath(pathname).match(/^\/notes\/share\/([^/]+)$/);
  if (!match) return null;
  try {
    const token = decodeURIComponent(match[1]);
    return /^[A-Za-z0-9_-]{3,128}$/.test(token) ? token : null;
  } catch {
    return null;
  }
}

function nestedPagesPath(page: AdminPage): string {
  return page === "conversations" ? CONVERSATION_PAGES_PATH : ADMIN_PAGE_PATHS[page];
}

export function adminPagePath(
  page: AdminPage,
  projectId?: string | null,
  segments: AdminPageSegments = [],
): string {
  const pagePath = segments.length
    ? `${nestedPagesPath(page)}/${segments.map(encodeURIComponent).join("/")}`
    : ADMIN_PAGE_PATHS[page];
  return projectId
    ? `${CABINET_PATH}/p/${encodeURIComponent(projectId)}${pagePath.slice(CABINET_PATH.length)}`
    : pagePath;
}

export function resolveProjectId(pathname: string): string | null {
  const match = normalizedPath(pathname).match(/^\/cabinet\/p\/([^/]+)(?:\/|$)/);
  if (!match) return null;
  try {
    const projectId = decodeURIComponent(match[1]);
    return projectId && !projectId.includes("/") ? projectId : null;
  } catch {
    return null;
  }
}

function unscopedAdminPath(pathname: string): string {
  const path = normalizedPath(pathname);
  return resolveProjectId(path)
    ? path.replace(/^\/cabinet\/p\/[^/]+/, CABINET_PATH)
    : path;
}

function decodedSegments(path: string): string[] | null {
  try {
    const segments = path.split("/").filter(Boolean).map(decodeURIComponent);
    return segments.some((segment) => segment.includes("/")) ? null : segments;
  } catch {
    return null;
  }
}

function matchAdminPage(pathname: string): { page: AdminPage; segments: string[] } | null {
  const path = unscopedAdminPath(pathname);
  for (const page of Object.keys(ADMIN_PAGE_PATHS) as AdminPage[]) {
    if (path === ADMIN_PAGE_PATHS[page]) return { page, segments: [] };
    const parent = nestedPagesPath(page);
    if (path === parent) return { page, segments: [] };
    if (!path.startsWith(`${parent}/`)) continue;
    const segments = decodedSegments(path.slice(parent.length + 1));
    return segments ? { page, segments } : null;
  }
  return null;
}

export function resolveAdminPage(pathname: string): AdminPage {
  return matchAdminPage(pathname)?.page ?? "conversations";
}

export function resolveAdminPageSegments(pathname: string): string[] {
  return matchAdminPage(pathname)?.segments ?? [];
}

export function isAdminPagePath(pathname: string): boolean {
  return matchAdminPage(pathname) !== null;
}

/** Maps links from before cabinet pages had nested paths, such as `/cabinet/ai?agent=<id>`. */
export function legacyAdminPagePath(pathname: string, search: string): string | null {
  const match = matchAdminPage(pathname);
  if (!match || match.segments.length) return null;
  const params = new URLSearchParams(search);
  const legacyParam = match.page === "ai" ? "agent" : match.page === "knowledge_base" ? "base" : null;
  const id = legacyParam ? params.get(legacyParam) : null;
  if (!legacyParam || !id) return null;
  params.delete(legacyParam);
  const query = params.toString();
  const segments = match.page === "ai" ? ["agents", id] : [id];
  return `${adminPagePath(match.page, resolveProjectId(pathname), segments)}${query ? `?${query}` : ""}`;
}

