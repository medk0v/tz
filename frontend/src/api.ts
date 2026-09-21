import type { ResourceVisibility } from "./resource-visibility";
import createClient from "openapi-fetch";
import type { components, operations, paths } from "./api-schema";
import type { WidgetClientContext } from "./widget-bridge";
import { productNamespace } from "./product-edition";

export type AccessToken = components["schemas"]["AccessToken"];
export type ActorContext = components["schemas"]["ActorContext"];
export type AiChannelOption = components["schemas"]["AiChannelOption"];
export type ChannelConnection = components["schemas"]["ChannelConnection"];
export type PhoneChannel = components["schemas"]["PhoneChannel"];
export type PhoneChannelInput = components["schemas"]["PhoneChannelInput"];
export type PhoneGatewayCredentials = components["schemas"]["PhoneGatewayCredentials"];
export type BlacklistReplyConfig = components["schemas"]["BlacklistReplyConfig"];
export type Contact = components["schemas"]["Contact"];
export type ContactPeriod = NonNullable<operations["listContacts"]["parameters"]["query"]>["period"];
export type ContactStatistics = components["schemas"]["ContactStatistics"];
export type ContactList = components["schemas"]["ContactList"];
export type Conversation = components["schemas"]["Conversation"];
export type ConversationAiAgent = components["schemas"]["ConversationAiAgent"];
export type ConversationOperator = components["schemas"]["ConversationOperator"];
export type OperatorConversation = components["schemas"]["OperatorConversation"];
export type JoinConversationResponse = components["schemas"]["JoinConversationResponse"];
export type ReturnConversationToAiResponse = components["schemas"]["ReturnConversationToAiResponse"];
export type CreatedAccessToken = components["schemas"]["CreatedAccessToken"];
export type Inbox = components["schemas"]["Inbox"];
export type Message = components["schemas"]["Message"];
export type MessageAttachment = components["schemas"]["MessageAttachment"];
export type OnlineVisitor = components["schemas"]["OnlineVisitor"];
export type OnlineVisitorList = components["schemas"]["OnlineVisitorList"];
export type OnlineVisitorWidget = components["schemas"]["OnlineVisitorWidget"];
export type Project = components["schemas"]["Project"];
export type ProjectAppearance = components["schemas"]["ProjectAppearance"];
export type ProjectAppearanceInput = components["schemas"]["ProjectAppearanceInput"];
export type ProjectLogos = components["schemas"]["ProjectLogos"];
export type CompanyProfile = components["schemas"]["CompanyProfile"];
export type ProjectMember = components["schemas"]["ProjectMember"];
export type Resolution = components["schemas"]["Resolution"];
export type SupportQualityItem = components["schemas"]["SupportQualityItem"];
export type SupportQualityReport = components["schemas"]["SupportQualityReport"];
export type EmailSettings = components["schemas"]["EmailSettings"];
export type EmailSettingsInput = components["schemas"]["EmailSettingsInput"];
export type PublicSupportRating = components["schemas"]["PublicSupportRating"];
export type Role = components["schemas"]["Role"];
export type RoleDefinition = components["schemas"]["RoleDefinition"];
export type RolePermission = components["schemas"]["RolePermission"];
export type WidgetSession = components["schemas"]["WidgetSession"];
export type WidgetLauncher = components["schemas"]["WidgetLauncher"];
export type WidgetLanguage = components["schemas"]["WidgetLanguage"];
export type WidgetTranslations = components["schemas"]["WidgetTranslations"];
export type WidgetTheme = components["schemas"]["WidgetTheme"];
export type WidgetPresentation = components["schemas"]["WidgetPresentation"];
export type WidgetAvailability = components["schemas"]["WidgetAvailability"];
export type WidgetContactProfile = components["schemas"]["WidgetContactProfile"];
export type VisitorObservation = components["schemas"]["VisitorObservation"];
export type VisitorIntelligence = components["schemas"]["VisitorIntelligence"];

export type OperatorAuth = (
  | { kind: "session"; projectId?: string }
  | { kind: "access_token"; token: string; projectId?: string }
) & { isDemo?: boolean };

export type OperatorMessageSender = "operator" | "ai" | "assigned_operator";

export interface OperatorChatProfile {
  user_id: string;
  display_name: string;
  avatar_url: string | null;
}

export type KnowledgeBaseStatus = "active" | "archived";
export type KnowledgeArticleStatus = "draft" | "published";

export type ReplyTemplate = components["schemas"]["ReplyTemplate"];
export type ReplyTemplateInput = components["schemas"]["ReplyTemplateInput"];
export type ReplySuggestionAgent = components["schemas"]["ReplySuggestionAgent"];
export type GenerateReplySuggestionResponse = components["schemas"]["GenerateReplySuggestionResponse"];
export type ContentDraftResponse = components["schemas"]["ContentDraftResponse"];

export interface KnowledgeBase {
  visibility?: ResourceVisibility;
  id: string;
  name: string;
  description: string;
  status: KnowledgeBaseStatus;
  article_count: number;
  published_article_count: number;
  created_at: string;
  updated_at: string;
}

export interface KnowledgeBaseInput {
  visibility?: ResourceVisibility;
  name: string;
  description: string;
  status: KnowledgeBaseStatus;
}

export interface KnowledgeArticle {
  id: string;
  knowledge_base_id: string;
  title: string;
  body: string;
  status: KnowledgeArticleStatus;
  source_url: string | null;
  version: number;
  published_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface KnowledgeArticleInput {
  title: string;
  body: string;
  status: KnowledgeArticleStatus;
  source_url: string | null;
}

export type AiProviderKind = "openai" | "anthropic" | "openai_compatible";
export type AiProviderStatus = "active" | "disabled";
/** What a connected model does; connections without a type are chat models. */
export type AiModelType = components["schemas"]["AiModelType"];
export type AiProfileVoice = components["schemas"]["AiProfileVoice"];

export interface AiProvider {
  visibility?: ResourceVisibility;
  id: string;
  name: string;
  provider_kind: AiProviderKind;
  model_type?: AiModelType;
  base_url: string;
  default_model: string;
  status: AiProviderStatus;
  api_key_configured: boolean;
  created_at: string;
  updated_at: string;
}

export interface AiProviderInput {
  visibility?: ResourceVisibility;
  name: string;
  provider_kind: AiProviderKind;
  model_type?: AiModelType;
  base_url: string;
  default_model: string;
  status: AiProviderStatus;
  api_key?: string;
  clear_api_key?: boolean;
}

export type AiProfileStatus = "draft" | "active" | "disabled";
export type AiProfileCapabilities = components["schemas"]["AiProfileCapabilities"];

export interface AiProfileCustomField {
  key: string;
  value: string;
}

export interface AiProfileSecret {
  key: string;
  configured: boolean;
}

export interface AiProfileSecretInput {
  key: string;
  value: string | null;
}

export interface TelegramNotificationSettings {
  new_visitor: boolean;
  new_message: boolean;
  operator_request: boolean;
}

export interface AiProfilePublicIdentity {
  language: string;
  display_name: string;
  avatar_url: string | null;
}

export interface AiProfilePublicIdentityInput {
  language: string;
  display_name: string;
}

export interface AiProfile {
  execution_ready?: boolean,
  visibility?: ResourceVisibility;
  preset_key?: "coordinator" | "sales" | "marketing" | "finance" | "legal" | "programmer" | "support" | "operations" | "quality" | "hr" | "universal" | null;
  description?: string;
  id: string;
  provider_connection_id: string | null;
  name: string;
  avatar_url: string | null;
  status: AiProfileStatus;
  mode: "copilot";
  model: string | null;
  instructions: string;
  tool_instructions: string;
  blacklist_reply_text: string;
  blacklist_reply_match_language: boolean;
  http_allowed_hosts: string[];
  language: string;
  max_output_tokens: number;
  max_concurrent_runs?: number;
  auto_join_new_conversations: boolean;
  can_resolve_conversations: boolean;
  capabilities: AiProfileCapabilities;
  telegram_notifications: TelegramNotificationSettings;
  custom_fields: AiProfileCustomField[];
  secrets: AiProfileSecret[];
  knowledge_base_ids: string[];
  channel_ids: string[];
  public_identities: AiProfilePublicIdentity[];
  voice?: AiProfileVoice;
  created_at: string;
  updated_at: string;
}

export interface AiProfileInput {
  visibility?: ResourceVisibility;
  description?: string;
  name: string;
  status: AiProfileStatus;
  provider_connection_id: string | null;
  model: string | null;
  instructions: string;
  tool_instructions: string;
  blacklist_reply_text: string;
  blacklist_reply_match_language: boolean;
  http_allowed_hosts: string[];
  language: string;
  max_output_tokens: number;
  max_concurrent_runs?: number;
  auto_join_new_conversations: boolean;
  can_resolve_conversations: boolean;
  capabilities: AiProfileCapabilities;
  telegram_notifications: TelegramNotificationSettings;
  custom_fields: AiProfileCustomField[];
  secrets: AiProfileSecretInput[];
  knowledge_base_ids: string[];
  channel_ids: string[];
  public_identities: AiProfilePublicIdentityInput[];
  voice?: AiProfileVoice;
  /** Create only: the frontend preset template this agent is saved from. */
  preset_key?: AiProfilePresetKey | null;
}

export type AiProfilePresetKey = "coordinator" | "sales" | "marketing" | "finance" | "legal" | "programmer" | "support" | "quality" | "hr";

export type AiTaskSchedule = components["schemas"]["AiTaskSchedule"];
export type AiTaskInput = components["schemas"]["AiTaskInput"];
export type AiTask = components["schemas"]["AiTask"];
export type AiTaskRunStatus = components["schemas"]["AiTaskRunStatus"];
export type AiTaskRun = components["schemas"]["AiTaskRun"];

export type InboxRoutingAssignmentStrategy = "manual" | "least_active";
export type InboxRoutingQueueStatus = "active" | "disabled";
export type InboxRoutingFallbackAction = "ai" | "queue";
export type InboxRoutingSlaClock = "working_hours" | "elapsed";

export interface InboxRoutingQueue {
  id: string;
  name: string;
  status: InboxRoutingQueueStatus;
  member_ids: string[];
}

export interface InboxRoutingRule {
  id: string;
  position: number;
  enabled: boolean;
  channel_id: string | null;
  language: string | null;
  queue_id: string;
}

export interface InboxRoutingWeeklyInterval {
  id: string;
  weekday: number;
  starts_at: string;
  ends_at: string;
}

export interface InboxRoutingCoverage {
  timezone: string;
  weekly_intervals: InboxRoutingWeeklyInterval[];
  outside_hours_action: InboxRoutingFallbackAction;
  no_operator_action: InboxRoutingFallbackAction;
}

export interface InboxRoutingSla {
  clock: InboxRoutingSlaClock;
  first_response_seconds: number | null;
  unassigned_warning_seconds: number | null;
  resolution_seconds: number | null;
  escalation_queue_id: string | null;
}

export interface InboxRoutingTelegram {
  new_visitor: boolean;
  new_message: boolean;
  operator_request: boolean;
  unassigned_warning: boolean;
  sla_breach: boolean;
  chat_ids: string[];
  bot_token_configured: boolean;
}

export interface InboxRoutingConfiguration {
  inbox_id: string;
  project_id: string;
  configured: boolean;
  version: number;
  enabled: boolean;
  assignment_strategy: InboxRoutingAssignmentStrategy;
  default_queue_id: string | null;
  queues: InboxRoutingQueue[];
  rules: InboxRoutingRule[];
  coverage: InboxRoutingCoverage;
  sla: InboxRoutingSla;
  telegram: InboxRoutingTelegram;
}

export interface InboxRoutingChannelOption {
  id: string;
  name: string;
  kind: string;
  status: string;
}

export interface InboxRoutingOperatorOption {
  id: string;
  display_name: string;
  email: string;
  avatar_url: string | null;
  online: boolean;
}

export interface InboxRoutingResponse {
  configuration: InboxRoutingConfiguration;
  channel_options: InboxRoutingChannelOption[];
  operator_options: InboxRoutingOperatorOption[];
}

export interface InboxRoutingWrite {
  enabled: boolean;
  assignment_strategy: InboxRoutingAssignmentStrategy;
  default_queue_id: string | null;
  queues: InboxRoutingQueue[];
  rules: InboxRoutingRule[];
  coverage: InboxRoutingCoverage;
  sla: InboxRoutingSla;
  telegram: Omit<InboxRoutingTelegram, "bot_token_configured">;
}

export type TelegramBotTokenUpdate =
  | { action: "preserve" }
  | { action: "replace"; value: string }
  | { action: "clear" };

export interface UpdateInboxRoutingInput {
  expected_version: number;
  configuration: InboxRoutingWrite;
  telegram_bot_token: TelegramBotTokenUpdate;
}

export type ApiIntegrationStatus = "active" | "disabled";
export type ApiIntegrationAuthKind = "none" | "bearer" | "x_api_key";

export interface ApiIntegrationAction {
  id: string;
  key: string;
  name: string;
  description: string;
  path_template: string;
  parameter_names: string[];
}

export interface ApiIntegrationActionInput {
  key: string;
  name: string;
  description: string;
  path_template: string;
}

export interface ApiIntegrationAiProfile {
  id: string;
  name: string;
  status: AiProfileStatus;
  runtime_available: boolean;
}

export interface ApiIntegration {
  visibility?: ResourceVisibility;
  id: string;
  name: string;
  key: string;
  description: string;
  base_url: string;
  status: ApiIntegrationStatus;
  auth_kind: ApiIntegrationAuthKind;
  token_configured: boolean;
  actions: ApiIntegrationAction[];
  ai_profile_ids: string[];
  created_at: string;
  updated_at: string;
}

export interface ApiIntegrationInput {
  visibility?: ResourceVisibility;
  name: string;
  key: string;
  description: string;
  base_url: string;
  status: ApiIntegrationStatus;
  auth_kind: ApiIntegrationAuthKind;
  token?: string;
  clear_token: boolean;
  actions: ApiIntegrationActionInput[];
  ai_profile_ids: string[];
}

export interface ApiIntegrationsResponse {
  items: ApiIntegration[];
  ai_profiles: ApiIntegrationAiProfile[];
}

export class ApiRequestError extends Error {
  constructor(message: string, readonly status: number) {
    super(message);
    this.name = "ApiRequestError";
  }
}

export interface RealtimeEvent {
  event_id: string;
  inbox_id: string;
  contact_id?: string;
  type: string;
  aggregate_id: string;
  sequence?: number;
  data: Record<string, unknown>;
}

export const apiBaseUrl =
  import.meta.env.VITE_API_BASE_URL ?? "http://localhost:8080";

export function resolveAvatarUrl(value: string | null): string | null {
  if (!value) return null;
  try {
    const base = apiBaseUrl.endsWith("/") ? apiBaseUrl : `${apiBaseUrl}/`;
    const resolved = new URL(value, base);
    return resolved.protocol === "http:" || resolved.protocol === "https:"
      ? resolved.toString()
      : null;
  } catch {
    return null;
  }
}

const publicApi = createClient<paths>({
  baseUrl: apiBaseUrl,
  credentials: "omit",
});
const sessionApi = createClient<paths>({
  baseUrl: apiBaseUrl,
  credentials: "include",
});
const demoSessionApi = createClient<paths>({
  baseUrl: apiBaseUrl,
  credentials: "include",
});
demoSessionApi.use({
  onRequest({ request }) {
    requireDemoReadOnly({ kind: "session", isDemo: true }, new URL(request.url).pathname, request.method);
  },
});
const accessTokenApi = createClient<paths>({
  baseUrl: apiBaseUrl,
  credentials: "omit",
});
const widgetApi = createClient<paths>({
  baseUrl: apiBaseUrl,
  credentials: "omit",
});

function bearerHeaders(token: string) {
  return { Authorization: `Bearer ${token}` };
}

function csrfToken(): string {
  if (typeof document === "undefined") return "";
  for (const part of document.cookie.split(";")) {
    const [name, ...value] = part.trim().split("=");
    if (name === `${productNamespace}_csrf` || name === `__Host-${productNamespace}_csrf`) {
      return value.join("=");
    }
  }
  return "";
}

function operatorHeaders(auth: OperatorAuth, unsafe = false): Record<string, string> | undefined {
  const projectHeaders: Record<string, string> = auth.projectId
    ? { ["X-Tz-Project-Id"]: auth.projectId }
    : {};
  if (auth.kind === "access_token") {
    return { ...bearerHeaders(auth.token), ...projectHeaders };
  }
  return { ...projectHeaders, ...(unsafe ? { "X-CSRF-Token": csrfToken() } : {}) };
}

function operatorClient(auth: OperatorAuth) {
  if (auth.isDemo) return demoSessionApi;
  return auth.kind === "session" ? sessionApi : accessTokenApi;
}

function requireDemoReadOnly(auth: OperatorAuth, path: string, method = "GET") {
  if (!auth.isDemo || method === "GET" || method === "HEAD") return;
  if (method === "POST" && (/^\/api\/v1\/conversations\/[^/]+\/(messages|read|join)$/.test(path)
    || path === "/api/v1/realtime-tickets" || path === "/api/v1/operator-presence")) return;
  if (method === "DELETE" && path === "/api/v1/operator-presence") return;
  throw new Error("demo_readonly");
}

function errorMessage(error: unknown, fallback: string): string {
  if (typeof error === "object" && error !== null && "error" in error) {
    const envelope = error as { error?: { message?: unknown } };
    if (typeof envelope.error?.message === "string") {
      return envelope.error.message;
    }
  }
  return fallback;
}

function requireData<T>(data: T | undefined, error: unknown, fallback: string): T {
  if (data !== undefined) {
    return data;
  }
  throw new Error(errorMessage(error, fallback));
}

function requireSuccess(error: unknown, response: Response, fallback: string): void {
  if (!response.ok) {
    throw new Error(errorMessage(error, fallback));
  }
}

async function fileResponse<T>(response: Response, fallback: string): Promise<T> {
  const payload: unknown = await response.json().catch(() => undefined);
  if (!response.ok) {
    throw new ApiRequestError(errorMessage(payload, fallback), response.status);
  }
  return payload as T;
}

async function fileBlobResponse(response: Response, fallback: string): Promise<Blob> {
  if (response.ok) return response.blob();
  const payload: unknown = await response.json().catch(() => undefined);
  throw new ApiRequestError(errorMessage(payload, fallback), response.status);
}

export async function managementApiRequest<T>(auth: OperatorAuth, path: string, init?: RequestInit): Promise<T> {
  requireDemoReadOnly(auth, path, init?.method);
  const response = await fetch(`${apiBaseUrl}${path}`, {
    ...init,
    credentials: auth.kind === "session" ? "include" : "omit",
    headers: {
      ...(init?.body instanceof FormData ? {} : { "Content-Type": "application/json" }),
      ...operatorHeaders(auth, init?.method !== undefined && init.method !== "GET"),
      ...init?.headers,
    },
  });
  const payload = response.status === 204 ? undefined : await response.json();
  if (!response.ok) {
    throw new ApiRequestError(errorMessage(payload, "Could not update project settings."), response.status);
  }
  return payload as T;
}

async function avatarApiRequest<T>(
  auth: OperatorAuth,
  path: string,
  method: "PUT" | "DELETE",
  file?: File,
): Promise<T> {
  requireDemoReadOnly(auth, path, method);
  const response = await fetch(`${apiBaseUrl}${path}`, {
    method,
    body: file,
    credentials: auth.kind === "session" ? "include" : "omit",
    headers: {
      ...operatorHeaders(auth, true),
      ...(file ? { "Content-Type": file.type || "application/octet-stream" } : {}),
    },
  });
  const payload = await response.json();
  if (!response.ok) {
    throw new ApiRequestError(errorMessage(payload, "Could not update the avatar."), response.status);
  }
  return payload as T;
}

export async function listReplyTemplates(auth: OperatorAuth, inboxId: string, signal?: AbortSignal): Promise<ReplyTemplate[]> {
  const response = await managementApiRequest<{ items: ReplyTemplate[] }>(
    auth, `/api/v1/inboxes/${encodeURIComponent(inboxId)}/reply-templates`, { signal },
  );
  return response.items;
}

export async function listReplySuggestionAgents(
  auth: OperatorAuth,
  conversationId: string,
  signal?: AbortSignal,
): Promise<ReplySuggestionAgent[]> {
  const response = await managementApiRequest<{ items: ReplySuggestionAgent[] }>(
    auth,
    `/api/v1/conversations/${encodeURIComponent(conversationId)}/reply-suggestion-agents`,
    { signal },
  );
  return response.items;
}

export function generateReplySuggestion(
  auth: OperatorAuth,
  conversationId: string,
  aiProfileId: string,
  draftBody: string,
  signal?: AbortSignal,
): Promise<GenerateReplySuggestionResponse> {
  return managementApiRequest<GenerateReplySuggestionResponse>(
    auth,
    `/api/v1/conversations/${encodeURIComponent(conversationId)}/reply-suggestions`,
    {
      method: "POST",
      body: JSON.stringify({
        ai_profile_id: aiProfileId,
        ...(draftBody.trim() ? { draft_body: draftBody } : {}),
      }),
      signal,
    },
  );
}

export function createReplyTemplate(auth: OperatorAuth, inboxId: string, input: ReplyTemplateInput): Promise<ReplyTemplate> {
  return managementApiRequest<ReplyTemplate>(auth, `/api/v1/inboxes/${encodeURIComponent(inboxId)}/reply-templates`, {
    method: "POST", body: JSON.stringify(input),
  });
}

export function updateReplyTemplate(auth: OperatorAuth, inboxId: string, templateId: string, input: ReplyTemplateInput): Promise<ReplyTemplate> {
  return managementApiRequest<ReplyTemplate>(auth, `/api/v1/inboxes/${encodeURIComponent(inboxId)}/reply-templates/${encodeURIComponent(templateId)}`, {
    method: "PATCH", body: JSON.stringify(input),
  });
}

export async function deleteReplyTemplate(auth: OperatorAuth, inboxId: string, templateId: string): Promise<void> {
  await managementApiRequest<void>(auth, `/api/v1/inboxes/${encodeURIComponent(inboxId)}/reply-templates/${encodeURIComponent(templateId)}`, {
    method: "DELETE",
  });
}

export async function listReplyTemplateDraftAgents(
  auth: OperatorAuth,
  inboxId: string,
  signal?: AbortSignal,
): Promise<ReplySuggestionAgent[]> {
  const response = await managementApiRequest<{ items: ReplySuggestionAgent[] }>(
    auth,
    `/api/v1/inboxes/${encodeURIComponent(inboxId)}/reply-template-draft-agents`,
    { signal },
  );
  return response.items;
}

/** Drafts a template body from its title; an existing body is continued. Nothing is saved. */
export function generateReplyTemplateDraft(
  auth: OperatorAuth,
  inboxId: string,
  aiProfileId: string,
  title: string,
  draftBody: string,
  signal?: AbortSignal,
): Promise<ContentDraftResponse> {
  return managementApiRequest<ContentDraftResponse>(
    auth,
    `/api/v1/inboxes/${encodeURIComponent(inboxId)}/reply-template-drafts`,
    {
      method: "POST",
      body: JSON.stringify({
        ai_profile_id: aiProfileId,
        title,
        ...(draftBody.trim() ? { draft_body: draftBody } : {}),
      }),
      signal,
    },
  );
}

export async function listKnowledgeBases(auth: OperatorAuth): Promise<KnowledgeBase[]> {
  const response = await managementApiRequest<{ items: KnowledgeBase[] }>(auth, "/api/v1/knowledge-bases");
  return response.items;
}

export function createKnowledgeBase(auth: OperatorAuth, input: KnowledgeBaseInput): Promise<KnowledgeBase> {
  return managementApiRequest<KnowledgeBase>(auth, "/api/v1/knowledge-bases", {
    method: "POST",
    body: JSON.stringify(input),
  });
}

export function updateKnowledgeBase(
  auth: OperatorAuth,
  knowledgeBaseId: string,
  input: KnowledgeBaseInput,
): Promise<KnowledgeBase> {
  return managementApiRequest<KnowledgeBase>(auth, `/api/v1/knowledge-bases/${encodeURIComponent(knowledgeBaseId)}`, {
    method: "PATCH",
    body: JSON.stringify(input),
  });
}

export async function deleteKnowledgeBase(auth: OperatorAuth, knowledgeBaseId: string): Promise<void> {
  await managementApiRequest<void>(auth, `/api/v1/knowledge-bases/${encodeURIComponent(knowledgeBaseId)}`, {
    method: "DELETE",
  });
}

export async function listKnowledgeArticles(
  auth: OperatorAuth,
  knowledgeBaseId: string,
): Promise<KnowledgeArticle[]> {
  const response = await managementApiRequest<{ items: KnowledgeArticle[] }>(
    auth,
    `/api/v1/knowledge-bases/${encodeURIComponent(knowledgeBaseId)}/articles`,
  );
  return response.items;
}

export function createKnowledgeArticle(
  auth: OperatorAuth,
  knowledgeBaseId: string,
  input: KnowledgeArticleInput,
): Promise<KnowledgeArticle> {
  return managementApiRequest<KnowledgeArticle>(
    auth,
    `/api/v1/knowledge-bases/${encodeURIComponent(knowledgeBaseId)}/articles`,
    { method: "POST", body: JSON.stringify(input) },
  );
}

export function updateKnowledgeArticle(
  auth: OperatorAuth,
  knowledgeBaseId: string,
  articleId: string,
  input: KnowledgeArticleInput,
): Promise<KnowledgeArticle> {
  return managementApiRequest<KnowledgeArticle>(
    auth,
    `/api/v1/knowledge-bases/${encodeURIComponent(knowledgeBaseId)}/articles/${encodeURIComponent(articleId)}`,
    { method: "PATCH", body: JSON.stringify(input) },
  );
}

export async function deleteKnowledgeArticle(
  auth: OperatorAuth,
  knowledgeBaseId: string,
  articleId: string,
): Promise<void> {
  await managementApiRequest<void>(
    auth,
    `/api/v1/knowledge-bases/${encodeURIComponent(knowledgeBaseId)}/articles/${encodeURIComponent(articleId)}`,
    { method: "DELETE" },
  );
}

export async function listKnowledgeArticleDraftAgents(
  auth: OperatorAuth,
  knowledgeBaseId: string,
  signal?: AbortSignal,
): Promise<ReplySuggestionAgent[]> {
  const response = await managementApiRequest<{ items: ReplySuggestionAgent[] }>(
    auth,
    `/api/v1/knowledge-bases/${encodeURIComponent(knowledgeBaseId)}/article-draft-agents`,
    { signal },
  );
  return response.items;
}

/** Drafts material content from its title; existing content is continued. Nothing is saved. */
export function generateKnowledgeArticleDraft(
  auth: OperatorAuth,
  knowledgeBaseId: string,
  aiProfileId: string,
  title: string,
  draftBody: string,
  signal?: AbortSignal,
): Promise<ContentDraftResponse> {
  return managementApiRequest<ContentDraftResponse>(
    auth,
    `/api/v1/knowledge-bases/${encodeURIComponent(knowledgeBaseId)}/article-drafts`,
    {
      method: "POST",
      body: JSON.stringify({
        ai_profile_id: aiProfileId,
        title,
        ...(draftBody.trim() ? { draft_body: draftBody } : {}),
      }),
      signal,
    },
  );
}

export async function listAiProviders(auth: OperatorAuth, signal?: AbortSignal): Promise<AiProvider[]> {
  const response = await managementApiRequest<{ items: AiProvider[] }>(auth, "/api/v1/ai/providers", { signal });
  return response.items;
}

export async function listAiChannelOptions(auth: OperatorAuth): Promise<AiChannelOption[]> {
  const response = await managementApiRequest<{ items: AiChannelOption[] }>(
    auth,
    "/api/v1/ai/channel-options",
  );
  return response.items;
}

export function createAiProvider(auth: OperatorAuth, input: AiProviderInput): Promise<AiProvider> {
  return managementApiRequest<AiProvider>(auth, "/api/v1/ai/providers", {
    method: "POST",
    body: JSON.stringify(input),
  });
}

export function updateAiProvider(
  auth: OperatorAuth,
  providerId: string,
  input: AiProviderInput,
): Promise<AiProvider> {
  return managementApiRequest<AiProvider>(auth, `/api/v1/ai/providers/${encodeURIComponent(providerId)}`, {
    method: "PATCH",
    body: JSON.stringify(input),
  });
}

export async function deleteAiProvider(auth: OperatorAuth, providerId: string): Promise<void> {
  await managementApiRequest<void>(auth, `/api/v1/ai/providers/${encodeURIComponent(providerId)}`, {
    method: "DELETE",
  });
}

export async function listAiProfiles(auth: OperatorAuth): Promise<AiProfile[]> {
  const response = await managementApiRequest<{ items: AiProfile[] }>(auth, "/api/v1/ai/profiles");
  return response.items;
}

export function aiTestRequest<T>(auth: OperatorAuth, profileId: string, path: string, init?: RequestInit): Promise<T> {
  return managementApiRequest<T>(auth, `/api/v1/ai/profiles/${encodeURIComponent(profileId)}/${path}`, init);
}

export type AiAgentApiSettings = components["schemas"]["AiAgentApiSettings"];
export type AiAgentApiEndpointInput = components["schemas"]["AiAgentApiEndpointInput"];
export type AiAgentApiEndpoint = components["schemas"]["AiAgentApiEndpoint"];
export type AiAgentApiRun = components["schemas"]["AiAgentApiRun"];
export type AiAgentApiKey = components["schemas"]["AiAgentApiKey"];

function agentApiPath(profileId: string, suffix = ""): string {
  return `/api/v1/ai/profiles/${encodeURIComponent(profileId)}/api${suffix}`;
}

export function getAiAgentApiSettings(auth: OperatorAuth, profileId: string): Promise<AiAgentApiSettings> {
  return managementApiRequest(auth, agentApiPath(profileId));
}

export function updateAiAgentApiSettings(auth: OperatorAuth, profileId: string, enabled: boolean): Promise<AiAgentApiSettings> {
  return managementApiRequest(auth, agentApiPath(profileId), { method: "PATCH", body: JSON.stringify({ enabled }) });
}

export function createAiAgentApiKey(auth: OperatorAuth, profileId: string): Promise<AiAgentApiKey> {
  return managementApiRequest(auth, agentApiPath(profileId, "/key"), { method: "POST" });
}

export function revokeAiAgentApiKey(auth: OperatorAuth, profileId: string): Promise<void> {
  return managementApiRequest(auth, agentApiPath(profileId, "/key"), { method: "DELETE" });
}

export function createAiAgentApiEndpoint(auth: OperatorAuth, profileId: string, input: AiAgentApiEndpointInput): Promise<AiAgentApiEndpoint> {
  return managementApiRequest(auth, agentApiPath(profileId, "/endpoints"), { method: "POST", body: JSON.stringify(input) });
}

export function updateAiAgentApiEndpoint(auth: OperatorAuth, profileId: string, endpointId: string, input: AiAgentApiEndpointInput): Promise<AiAgentApiEndpoint> {
  return managementApiRequest(auth, agentApiPath(profileId, `/endpoints/${encodeURIComponent(endpointId)}`), { method: "PATCH", body: JSON.stringify(input) });
}

export function deleteAiAgentApiEndpoint(auth: OperatorAuth, profileId: string, endpointId: string): Promise<void> {
  return managementApiRequest(auth, agentApiPath(profileId, `/endpoints/${encodeURIComponent(endpointId)}`), { method: "DELETE" });
}

export async function listAiAgentApiRuns(auth: OperatorAuth, profileId: string): Promise<AiAgentApiRun[]> {
  const result = await managementApiRequest<{ items: AiAgentApiRun[] }>(auth, agentApiPath(profileId, "/runs"));
  return result.items;
}

export function getAiAgentApiRun(auth: OperatorAuth, profileId: string, runId: string): Promise<AiAgentApiRun> {
  return managementApiRequest(auth, agentApiPath(profileId, `/runs/${encodeURIComponent(runId)}`));
}

export function testAiAgentApiEndpoint(auth: OperatorAuth, profileId: string, endpointId: string, input: Record<string, unknown>): Promise<AiAgentApiRun> {
  return managementApiRequest(auth, agentApiPath(profileId, `/endpoints/${encodeURIComponent(endpointId)}/test`), { method: "POST", body: JSON.stringify(input) });
}

export function aiAgentApiUrl(profileId: string, slug: string): string {
  return new URL(`/${encodeURIComponent(profileId)}/api/${encodeURIComponent(slug)}`, apiBaseUrl || window.location.origin).toString();
}

export type AiProfileBackup = components["schemas"]["AiProfileBackup"];

export async function listAiProfileBackups(auth: OperatorAuth, profileId: string): Promise<AiProfileBackup[]> {
  const result = await managementApiRequest<{ items: AiProfileBackup[] }>(auth, `/api/v1/ai/profiles/${encodeURIComponent(profileId)}/backups`);
  return result.items;
}

export function createAiProfileBackup(auth: OperatorAuth, profileId: string): Promise<void> {
  return managementApiRequest(auth, `/api/v1/ai/profiles/${encodeURIComponent(profileId)}/backups`, { method: "POST" });
}

export function restoreAiProfileBackup(auth: OperatorAuth, profileId: string, backupId: string, input: { create_backup: boolean }): Promise<AiProfile> {
  return managementApiRequest(auth, `/api/v1/ai/profiles/${encodeURIComponent(profileId)}/backups/${encodeURIComponent(backupId)}/restore`, { method: "POST", body: JSON.stringify(input) });
}

export function deleteAiProfileBackup(auth: OperatorAuth, profileId: string, backupId: string): Promise<void> {
  return managementApiRequest(auth, `/api/v1/ai/profiles/${encodeURIComponent(profileId)}/backups/${encodeURIComponent(backupId)}`, { method: "DELETE" });
}

export function createAiProfile(auth: OperatorAuth, input: AiProfileInput): Promise<AiProfile> {
  return managementApiRequest<AiProfile>(auth, "/api/v1/ai/profiles", {
    method: "POST",
    body: JSON.stringify(input),
  });
}

export function updateAiProfile(
  auth: OperatorAuth,
  profileId: string,
  input: AiProfileInput,
): Promise<AiProfile> {
  return managementApiRequest<AiProfile>(auth, `/api/v1/ai/profiles/${encodeURIComponent(profileId)}`, {
    method: "PATCH",
    body: JSON.stringify(input),
  });
}

export async function deleteAiProfile(auth: OperatorAuth, profileId: string): Promise<void> {
  await managementApiRequest<void>(auth, `/api/v1/ai/profiles/${encodeURIComponent(profileId)}`, {
    method: "DELETE",
  });
}

export async function listAiTasks(auth: OperatorAuth): Promise<AiTask[]> {
  const response = await managementApiRequest<{ items: AiTask[] }>(auth, "/api/v1/ai/tasks");
  return response.items;
}

export function createAiTask(auth: OperatorAuth, input: AiTaskInput): Promise<AiTask> {
  return managementApiRequest<AiTask>(auth, "/api/v1/ai/tasks", {
    method: "POST",
    body: JSON.stringify(input),
  });
}

export function updateAiTask(
  auth: OperatorAuth,
  taskId: string,
  input: AiTaskInput,
): Promise<AiTask> {
  return managementApiRequest<AiTask>(auth, `/api/v1/ai/tasks/${encodeURIComponent(taskId)}`, {
    method: "PATCH",
    body: JSON.stringify(input),
  });
}

export async function deleteAiTask(auth: OperatorAuth, taskId: string): Promise<void> {
  await managementApiRequest<void>(auth, `/api/v1/ai/tasks/${encodeURIComponent(taskId)}`, {
    method: "DELETE",
  });
}

export async function listAiTaskRuns(auth: OperatorAuth, taskId: string): Promise<AiTaskRun[]> {
  const response = await managementApiRequest<{ items: AiTaskRun[] }>(
    auth,
    `/api/v1/ai/tasks/${encodeURIComponent(taskId)}/runs`,
  );
  return response.items;
}

export function listApiIntegrations(auth: OperatorAuth): Promise<ApiIntegrationsResponse> {
  return managementApiRequest<ApiIntegrationsResponse>(auth, "/api/v1/integrations/apis");
}

export function createApiIntegration(
  auth: OperatorAuth,
  input: ApiIntegrationInput,
): Promise<ApiIntegration> {
  return managementApiRequest<ApiIntegration>(auth, "/api/v1/integrations/apis", {
    method: "POST",
    body: JSON.stringify(input),
  });
}

export function updateApiIntegration(
  auth: OperatorAuth,
  integrationId: string,
  input: ApiIntegrationInput,
): Promise<ApiIntegration> {
  return managementApiRequest<ApiIntegration>(
    auth,
    `/api/v1/integrations/apis/${encodeURIComponent(integrationId)}`,
    { method: "PUT", body: JSON.stringify(input) },
  );
}

export async function deleteApiIntegration(
  auth: OperatorAuth,
  integrationId: string,
): Promise<void> {
  await managementApiRequest<void>(
    auth,
    `/api/v1/integrations/apis/${encodeURIComponent(integrationId)}`,
    { method: "DELETE" },
  );
}

export function uploadAiProfileAvatar(
  auth: OperatorAuth,
  profileId: string,
  file: File,
): Promise<AiProfile> {
  return avatarApiRequest<AiProfile>(
    auth,
    `/api/v1/ai/profiles/${encodeURIComponent(profileId)}/avatar`,
    "PUT",
    file,
  );
}

export function deleteAiProfileAvatar(auth: OperatorAuth, profileId: string): Promise<AiProfile> {
  return avatarApiRequest<AiProfile>(
    auth,
    `/api/v1/ai/profiles/${encodeURIComponent(profileId)}/avatar`,
    "DELETE",
  );
}

export function uploadAiProfilePublicIdentityAvatar(
  auth: OperatorAuth,
  profileId: string,
  language: string,
  file: File,
): Promise<AiProfile> {
  return avatarApiRequest<AiProfile>(
    auth,
    `/api/v1/ai/profiles/${encodeURIComponent(profileId)}/public-identities/${encodeURIComponent(language)}/avatar`,
    "PUT",
    file,
  );
}

export function deleteAiProfilePublicIdentityAvatar(
  auth: OperatorAuth,
  profileId: string,
  language: string,
): Promise<AiProfile> {
  return avatarApiRequest<AiProfile>(
    auth,
    `/api/v1/ai/profiles/${encodeURIComponent(profileId)}/public-identities/${encodeURIComponent(language)}/avatar`,
    "DELETE",
  );
}

export function updateOperatorChatProfile(
  auth: OperatorAuth,
  operatorId: string,
  input: { display_name: string; avatar_url: string | null },
): Promise<OperatorChatProfile> {
  return managementApiRequest<OperatorChatProfile>(
    auth,
    `/api/v1/operators/${encodeURIComponent(operatorId)}/chat-profile`,
    { method: "PATCH", body: JSON.stringify(input) },
  );
}

export function uploadOperatorAvatar(
  auth: OperatorAuth,
  operatorId: string,
  file: File,
): Promise<OperatorChatProfile> {
  return avatarApiRequest<OperatorChatProfile>(
    auth,
    `/api/v1/operators/${encodeURIComponent(operatorId)}/chat-profile/avatar`,
    "PUT",
    file,
  );
}

export function deleteOperatorAvatar(
  auth: OperatorAuth,
  operatorId: string,
): Promise<OperatorChatProfile> {
  return avatarApiRequest<OperatorChatProfile>(
    auth,
    `/api/v1/operators/${encodeURIComponent(operatorId)}/chat-profile/avatar`,
    "DELETE",
  );
}

export interface DemoCredentials {
  email: string;
  password: string;
  widget_url: string;
}

export async function getDemoCredentials(): Promise<DemoCredentials> {
  const { data, error } = await publicApi.GET("/api/v1/auth/demo");
  return requireData(data, error, "Could not load the demo account.");
}

export async function loginWithPassword(
  email: string,
  password: string,
): Promise<ActorContext> {
  const { data, error } = await sessionApi.POST("/api/v1/auth/login", {
    body: { email, password },
  });
  return requireData(data, error, "Could not sign in.");
}

export async function logoutSession(): Promise<void> {
  const { error, response } = await sessionApi.POST("/api/v1/auth/logout", {
    headers: { "X-CSRF-Token": csrfToken() },
  });
  requireSuccess(error, response, "Could not sign out cleanly.");
}

export async function getCurrentActor(auth: OperatorAuth): Promise<ActorContext> {
  const { data, error } = await operatorClient(auth).GET("/api/v1/me", {
    headers: operatorHeaders(auth),
  });
  return requireData(data, error, "Your session or access token is invalid.");
}

export async function selectProject(
  auth: OperatorAuth,
  projectId: string,
): Promise<ActorContext> {
  const { data, error } = await operatorClient(auth).POST("/api/v1/auth/project", {
    headers: operatorHeaders(auth, true),
    body: { project_id: projectId },
  });
  return requireData(data, error, "Could not switch projects.");
}

export const PROJECTS_CHANGED_EVENT = `${productNamespace}:projects-changed`;

export async function listProjects(auth: OperatorAuth): Promise<Project[]> {
  const { data, error } = await operatorClient(auth).GET("/api/v1/projects", {
    headers: operatorHeaders(auth),
  });
  return requireData(data, error, "Could not load projects.").items;
}

export async function createProject(
  auth: OperatorAuth,
  input: {
    name: string; slug: string; inbox_name: string;
    parent_project_id?: string | null;
    project_kind?: "project" | "company";
    company_profile?: CompanyProfile;
    departments?: Array<{ name: string; icon?: string; sidebar_items?: string[]; default_page?: string; show_default_channels?: boolean }>;
    default_department_index?: number;
    director_enabled?: boolean;
  },
): Promise<Project> {
  const { data, error } = await operatorClient(auth).POST("/api/v1/projects", {
    headers: operatorHeaders(auth, true),
    body: input,
  });
  const project = requireData(data, error, "Could not create the project.");
  window.dispatchEvent(new Event(PROJECTS_CHANGED_EVENT));
  return project;
}

export async function deleteProject(auth: OperatorAuth, projectId: string): Promise<void> {
  const { error, response } = await operatorClient(auth).DELETE("/api/v1/projects/{project_id}", {
    headers: operatorHeaders(auth, true),
    params: { path: { project_id: projectId } },
  });
  requireSuccess(error, response, "Could not delete the project.");
  window.dispatchEvent(new Event(PROJECTS_CHANGED_EVENT));
}

export async function updateProject(
  auth: OperatorAuth,
  projectId: string,
  input: { name?: string; slug?: string; status?: "active" | "disabled"; default_department_id?: string; director_enabled?: boolean; default_director_workspace?: boolean; director_menu?: import("./department-api").DirectorMenu; project_kind?: "project" | "company"; company_profile?: CompanyProfile; parent_project_id?: string | null },
): Promise<Project> {
  const { data, error } = await operatorClient(auth).PATCH("/api/v1/projects/{project_id}", {
    headers: operatorHeaders(auth, true),
    params: { path: { project_id: projectId } },
    body: input,
  });
  const project = requireData(data, error, "Could not update the project.");
  window.dispatchEvent(new Event(PROJECTS_CHANGED_EVENT));
  return project;
}

/** Replaces the colors a project sets for everyone in it; null leaves a choice to each person. */
export async function updateProjectAppearance(
  auth: OperatorAuth,
  projectId: string,
  appearance: ProjectAppearanceInput,
): Promise<Project> {
  const { data, error } = await operatorClient(auth).PATCH("/api/v1/projects/{project_id}", {
    headers: operatorHeaders(auth, true),
    params: { path: { project_id: projectId } },
    body: { appearance },
  });
  return requireData(data, error, "Could not update the project appearance.");
}

/** Replaces the project's logo for one theme, shown in its workspace instead of the product logo. */
export function uploadProjectLogo(auth: OperatorAuth, projectId: string, theme: keyof ProjectLogos, file: File): Promise<Project> {
  return avatarApiRequest<Project>(auth, `/api/v1/projects/${encodeURIComponent(projectId)}/logos/${theme}`, "PUT", file);
}

export function deleteProjectLogo(auth: OperatorAuth, projectId: string, theme: keyof ProjectLogos): Promise<Project> {
  return avatarApiRequest<Project>(auth, `/api/v1/projects/${encodeURIComponent(projectId)}/logos/${theme}`, "DELETE");
}

export async function listProjectMembers(
  auth: OperatorAuth,
  projectId: string,
): Promise<ProjectMember[]> {
  const { data, error } = await operatorClient(auth).GET(
    "/api/v1/projects/{project_id}/members",
    {
      headers: operatorHeaders(auth),
      params: { path: { project_id: projectId } },
    },
  );
  return requireData(data, error, "Could not load project access.").items;
}

export async function grantProjectMember(
  auth: OperatorAuth,
  projectId: string,
  input: { email: string; role_id: string; department_id?: string | null; director_access?: boolean },
): Promise<ProjectMember> {
  const { data, error } = await operatorClient(auth).POST(
    "/api/v1/projects/{project_id}/members",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { project_id: projectId } },
      body: input,
    },
  );
  return requireData(data, error, "Could not grant project access.");
}

export function createProjectUser(
  auth: OperatorAuth,
  projectId: string,
  input: components["schemas"]["CreateProjectUserRequest"],
): Promise<ProjectMember> {
  return managementApiRequest<ProjectMember>(auth, `/api/v1/projects/${encodeURIComponent(projectId)}/users`, {
    method: "POST",
    body: JSON.stringify(input),
  });
}

export async function updateProjectMember(
  auth: OperatorAuth,
  projectId: string,
  membershipId: string,
  input: string | { role_id?: string; department_id?: string | null; director_access?: boolean },
): Promise<ProjectMember> {
  const { data, error } = await operatorClient(auth).PATCH(
    "/api/v1/projects/{project_id}/members/{membership_id}",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { project_id: projectId, membership_id: membershipId } },
      body: typeof input === "string" ? { role_id: input } : input,
    },
  );
  return requireData(data, error, "Could not update project access.");
}

export async function revokeProjectMember(
  auth: OperatorAuth,
  projectId: string,
  membershipId: string,
): Promise<void> {
  const { error, response } = await operatorClient(auth).DELETE(
    "/api/v1/projects/{project_id}/members/{membership_id}",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { project_id: projectId, membership_id: membershipId } },
    },
  );
  requireSuccess(error, response, "Could not revoke project access.");
}

export async function refreshOperatorPresence(auth: OperatorAuth): Promise<void> {
  const { error, response } = await operatorClient(auth).POST("/api/v1/operator-presence", {
    headers: operatorHeaders(auth, true),
  });
  requireSuccess(error, response, "Could not refresh operator presence.");
}

export async function clearOperatorPresence(auth: OperatorAuth): Promise<void> {
  const { error, response } = await operatorClient(auth).DELETE("/api/v1/operator-presence", {
    headers: operatorHeaders(auth, true),
  });
  requireSuccess(error, response, "Could not clear operator presence.");
}

export async function listRoles(auth: OperatorAuth, projectId?: string): Promise<RoleDefinition[]> {
  if (projectId) {
    const result = await managementApiRequest<{ items: RoleDefinition[] }>(auth, `/api/v1/roles?project_id=${encodeURIComponent(projectId)}`);
    return result.items;
  }
  const { data, error } = await operatorClient(auth).GET("/api/v1/roles", {
    headers: operatorHeaders(auth),
  });
  return requireData(data, error, "Could not load roles.").items;
}

export async function updateRole(
  auth: OperatorAuth,
  roleId: string,
  input: { name: string; base_role: "manager" | "operator"; permissions: RolePermission[] },
): Promise<RoleDefinition> {
  const { data, error } = await operatorClient(auth).PATCH("/api/v1/roles/{role_id}", {
    headers: operatorHeaders(auth, true),
    params: { path: { role_id: roleId } },
    body: input,
  });
  return requireData(data, error, "Could not update the role.");
}

export async function createRole(
  auth: OperatorAuth,
  input: { name: string; base_role: "manager" | "operator"; permissions: RolePermission[] },
): Promise<RoleDefinition> {
  const { data, error } = await operatorClient(auth).POST("/api/v1/roles", {
    headers: operatorHeaders(auth, true),
    body: input,
  });
  return requireData(data, error, "Could not create the role.");
}

export async function deleteRole(auth: OperatorAuth, roleId: string): Promise<void> {
  const { error, response } = await operatorClient(auth).DELETE("/api/v1/roles/{role_id}", {
    headers: operatorHeaders(auth, true),
    params: { path: { role_id: roleId } },
  });
  requireSuccess(error, response, "Could not delete the role.");
}

export async function listAccessTokens(auth: OperatorAuth): Promise<AccessToken[]> {
  const { data, error } = await operatorClient(auth).GET("/api/v1/access-tokens", {
    headers: operatorHeaders(auth),
  });
  return requireData(data, error, "Could not load access tokens.").items;
}

export function listAccessTokenManagement(auth: OperatorAuth): Promise<{ items: AccessToken[]; can_create_all_projects: boolean }> {
  return managementApiRequest(auth, "/api/v1/access-tokens");
}

export async function createAccessToken(
  auth: OperatorAuth,
  input: {
    name: string;
    role_id: string;
    expires_in_days: number;
    project_id?: string;
    project_scope?: "project" | "all";
    inbox_scope?: string[] | null;
  },
): Promise<CreatedAccessToken> {
  const { data, error } = await operatorClient(auth).POST("/api/v1/access-tokens", {
    headers: operatorHeaders(auth, true),
    body: input,
  });
  return requireData(data, error, "Could not create the access token.");
}

export async function revokeAccessToken(
  auth: OperatorAuth,
  tokenId: string,
): Promise<void> {
  const { error, response } = await operatorClient(auth).POST(
    "/api/v1/access-tokens/{token_id}/revoke",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { token_id: tokenId } },
    },
  );
  requireSuccess(error, response, "Could not revoke the access token.");
}

export async function deleteAccessToken(
  auth: OperatorAuth,
  tokenId: string,
): Promise<void> {
  const { error, response } = await operatorClient(auth).DELETE(
    "/api/v1/access-tokens/{token_id}",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { token_id: tokenId } },
    },
  );
  requireSuccess(error, response, "Could not delete the access token.");
}

export async function listInboxes(auth: OperatorAuth): Promise<Inbox[]> {
  const { data, error } = await operatorClient(auth).GET("/api/v1/inboxes", {
    headers: operatorHeaders(auth),
  });
  return requireData(data, error, "Could not load Inbox records.").items;
}

export function getInboxRouting(
  auth: OperatorAuth,
  inboxId: string,
): Promise<InboxRoutingResponse> {
  return managementApiRequest<InboxRoutingResponse>(
    auth,
    `/api/v1/inboxes/${encodeURIComponent(inboxId)}/routing`,
  );
}

export function updateInboxRouting(
  auth: OperatorAuth,
  inboxId: string,
  input: UpdateInboxRoutingInput,
  idempotencyKey: string,
): Promise<InboxRoutingResponse> {
  return managementApiRequest<InboxRoutingResponse>(
    auth,
    `/api/v1/inboxes/${encodeURIComponent(inboxId)}/routing`,
    {
      method: "PUT",
      headers: { "Idempotency-Key": idempotencyKey },
      body: JSON.stringify(input),
    },
  );
}

export async function setContactBlocked(auth: OperatorAuth, contactId: string, blocked: boolean): Promise<{ id: string; is_blocked: boolean }> {
  const { data, error } = await operatorClient(auth).PATCH("/api/v1/contacts/{contact_id}/block", {
    headers: operatorHeaders(auth, true),
    params: { path: { contact_id: contactId } },
    body: { blocked },
  });
  return requireData(data, error, "Could not update the contact blacklist state.");
}

export async function updateChannelBlacklist(auth: OperatorAuth, channelId: string, input: BlacklistReplyConfig): Promise<ChannelConnection> {
  const { data, error } = await operatorClient(auth).PATCH("/api/v1/channels/{channel_id}/blacklist", {
    headers: operatorHeaders(auth, true),
    params: { path: { channel_id: channelId } },
    body: input,
  });
  return requireData(data, error, "Could not save blacklist reply settings.");
}

export async function listContacts(
  auth: OperatorAuth,
  query: { page: number; per_page: number; search?: string; period?: ContactPeriod; from?: string; to?: string },
): Promise<ContactList> {
  const { data, error } = await operatorClient(auth).GET("/api/v1/contacts", {
    headers: operatorHeaders(auth),
    params: { query },
  });
  return requireData(data, error, "Could not load contacts.");
}

export async function listOnlineVisitorWidgets(auth: OperatorAuth): Promise<OnlineVisitorWidget[]> {
  const { data, error } = await operatorClient(auth).GET("/api/v1/widgets", {
    headers: operatorHeaders(auth),
  });
  return requireData(data, error, "Could not load website widgets.").items;
}

export async function startVisitorConversation(
  auth: OperatorAuth,
  sessionId: string,
): Promise<{ id: string; inbox_id: string }> {
  const { data, error } = await operatorClient(auth).POST(
    "/api/v1/visitor-sessions/{session_id}/conversation",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { session_id: sessionId } },
    },
  );
  return requireData(data, error, "Could not start a conversation with this visitor.");
}

export async function listOnlineVisitors(
  auth: OperatorAuth,
  widgetId: string,
): Promise<OnlineVisitorList> {
  const { data, error } = await operatorClient(auth).GET(
    "/api/v1/widgets/{widget_id}/online-visitors",
    {
      headers: operatorHeaders(auth),
      params: { path: { widget_id: widgetId } },
    },
  );
  return requireData(data, error, "Could not load online visitors.");
}

export async function listChannels(auth: OperatorAuth): Promise<ChannelConnection[]> {
  const { data, error } = await operatorClient(auth).GET("/api/v1/channels", {
    headers: operatorHeaders(auth),
  });
  return requireData(data, error, "Could not load channels.").items;
}

export async function createWidgetChannel(
  auth: OperatorAuth,
  input: {
    kind: "widget";
    inbox_id: string;
    name: string;
    allowed_origins: string[];
    default_language: WidgetLanguage;
    translations: WidgetTranslations;
    launcher: WidgetLauncher;
    theme: WidgetTheme;
    notify_on_new_visitor: boolean;
    attachments_enabled: boolean;
  },
): Promise<ChannelConnection> {
  const { data, error } = await operatorClient(auth).POST("/api/v1/channels", {
    headers: operatorHeaders(auth, true),
    body: input,
  });
  return requireData(data, error, "Could not create the widget channel.");
}

export async function createTelegramBotChannel(
  auth: OperatorAuth,
  input: { inbox_id: string; name: string; bot_token: string },
): Promise<ChannelConnection> {
  const { data, error } = await operatorClient(auth).POST(
    "/api/v1/channels/telegram-bots",
    {
      headers: operatorHeaders(auth, true),
      body: input,
    },
  );
  return requireData(data, error, "Could not connect the Telegram bot.");
}

export async function createPhoneChannel(
  auth: OperatorAuth,
  input: PhoneChannelInput,
): Promise<PhoneGatewayCredentials> {
  const { data, error } = await operatorClient(auth).POST("/api/v1/channels/phone", {
    headers: operatorHeaders(auth, true),
    body: input,
  });
  return requireData(data, error, "Could not connect the phone number.");
}

export async function updatePhoneChannel(
  auth: OperatorAuth,
  channelId: string,
  input: PhoneChannelInput,
): Promise<PhoneChannel> {
  const { data, error } = await operatorClient(auth).PATCH("/api/v1/channels/phone/{channel_id}", {
    headers: operatorHeaders(auth, true),
    params: { path: { channel_id: channelId } },
    body: input,
  });
  return requireData(data, error, "Could not save the phone channel.");
}

export async function rotatePhoneGatewayToken(
  auth: OperatorAuth,
  channelId: string,
): Promise<PhoneGatewayCredentials> {
  const { data, error } = await operatorClient(auth).POST("/api/v1/channels/phone/{channel_id}/gateway-token", {
    headers: operatorHeaders(auth, true),
    params: { path: { channel_id: channelId } },
  });
  return requireData(data, error, "Could not replace the gateway token.");
}

export async function deleteChannel(
  auth: OperatorAuth,
  channelId: string,
): Promise<void> {
  const { error, response } = await operatorClient(auth).DELETE(
    "/api/v1/channels/{channel_id}",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { channel_id: channelId } },
    },
  );
  if (!response.ok) {
    throw new ApiRequestError(
      errorMessage(error, "Could not delete the channel."),
      response.status,
    );
  }
}

export async function updateWidgetChannel(
  auth: OperatorAuth,
  channelId: string,
  input: {
    name?: string;
    allowed_origins: string[];
    default_language: WidgetLanguage;
    translations: WidgetTranslations;
    launcher: WidgetLauncher;
    theme: WidgetTheme;
    notify_on_new_visitor: boolean;
    attachments_enabled: boolean;
  },
): Promise<ChannelConnection> {
  const { data, error, response } = await operatorClient(auth).PATCH(
    "/api/v1/channels/{channel_id}/widget",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { channel_id: channelId } },
      body: input,
    },
  );
  if (data === undefined) {
    throw new ApiRequestError(
      errorMessage(error, "Could not update the widget channel."),
      response.status,
    );
  }
  return data;
}

export interface InboxConversationListOptions {
  search?: string;
  includeConversationId?: string;
  signal?: AbortSignal;
  needsReply?: boolean;
  mine?: boolean;
  contactId?: string;
  status?: "active" | "resolved" | "all";
  channelId?: string;
}

export interface InboxConversationPage {
  items: OperatorConversation[];
  has_more: boolean;
}

export async function listInboxConversationPage(
  auth: OperatorAuth,
  inboxId: string,
  options: InboxConversationListOptions = {},
): Promise<InboxConversationPage> {
  const { data, error } = await operatorClient(auth).GET(
    "/api/v1/inboxes/{inbox_id}/conversations",
    {
      cache: "no-store",
      headers: operatorHeaders(auth),
      params: {
        path: { inbox_id: inboxId },
        query: {
          limit: 100,
          search: options.search,
          include_conversation_id: options.includeConversationId,
          needs_reply: options.needsReply,
          mine: options.mine,
          contact_id: options.contactId,
          status: options.status,
          channel_id: options.channelId,
        },
      },
      signal: options.signal,
    },
  );
  const result = requireData(data, error, "Could not load conversations.");
  return { items: result.items, has_more: result.has_more ?? false };
}

export async function listInboxConversations(
  auth: OperatorAuth,
  inboxId: string,
  options: InboxConversationListOptions = {},
): Promise<OperatorConversation[]> {
  return (await listInboxConversationPage(auth, inboxId, options)).items;
}

export async function markOperatorConversationRead(
  auth: OperatorAuth,
  conversationId: string,
): Promise<void> {
  const { error, response } = await operatorClient(auth).POST(
    "/api/v1/conversations/{conversation_id}/read",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { conversation_id: conversationId } },
    },
  );
  requireSuccess(error, response, "Could not mark the conversation as read.");
}

export async function joinConversation(
  auth: OperatorAuth,
  conversationId: string,
): Promise<JoinConversationResponse> {
  const { data, error } = await operatorClient(auth).POST(
    "/api/v1/conversations/{conversation_id}/join",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { conversation_id: conversationId } },
    },
  );
  return requireData(data, error, "Could not join the conversation.");
}

export async function takeOverConversation(
  auth: OperatorAuth,
  conversationId: string,
): Promise<JoinConversationResponse> {
  const { data, error } = await operatorClient(auth).POST(
    "/api/v1/conversations/{conversation_id}/take-over",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { conversation_id: conversationId } },
    },
  );
  return requireData(data, error, "Could not take over the conversation.");
}

export async function returnConversationToAi(
  auth: OperatorAuth,
  conversationId: string,
): Promise<ReturnConversationToAiResponse> {
  const { data, error } = await operatorClient(auth).POST(
    "/api/v1/conversations/{conversation_id}/return-to-ai",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { conversation_id: conversationId } },
    },
  );
  return requireData(data, error, "Could not return the conversation to AI.");
}

export async function listOperatorMessages(
  auth: OperatorAuth,
  conversationId: string,
): Promise<Message[]> {
  const { data, error } = await operatorClient(auth).GET(
    "/api/v1/conversations/{conversation_id}/messages",
    {
      cache: "no-store",
      headers: operatorHeaders(auth),
      params: {
        path: { conversation_id: conversationId },
        query: { after_sequence: 0, limit: 100 },
      },
    },
  );
  return requireData(data, error, "Could not load messages.").items;
}

export async function getVisitorIntelligence(
  auth: OperatorAuth,
  conversationId: string,
): Promise<VisitorIntelligence | null> {
  const { data, error, response } = await operatorClient(auth).GET(
    "/api/v1/conversations/{conversation_id}/visitor-intelligence",
    {
      cache: "no-store",
      headers: operatorHeaders(auth),
      params: { path: { conversation_id: conversationId } },
    },
  );
  if (response.status === 403 || response.status === 404) return null;
  return requireData(data, error, "Could not load visitor intelligence.");
}

export async function sendOperatorMessage(
  auth: OperatorAuth,
  conversationId: string,
  body: string,
  sendAs: OperatorMessageSender = "operator",
  bodyFormat: "plain" | "markdown" = "plain",
): Promise<Message> {
  const { data, error } = await operatorClient(auth).POST(
    "/api/v1/conversations/{conversation_id}/messages",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { conversation_id: conversationId } },
      body: { client_message_id: crypto.randomUUID(), body, send_as: sendAs, body_format: bodyFormat },
    },
  );
  return requireData(data, error, "Could not send the message.");
}

export async function uploadOperatorAttachment(
  auth: OperatorAuth,
  conversationId: string,
  file: File,
): Promise<Message> {
  requireDemoReadOnly(auth, `/api/v1/conversations/${encodeURIComponent(conversationId)}/attachments`, "POST");
  const form = new FormData();
  form.append("file", file, file.name);
  const response = await fetch(
    `${apiBaseUrl}/api/v1/conversations/${encodeURIComponent(conversationId)}/attachments`,
    {
      method: "POST",
      body: form,
      credentials: auth.kind === "session" ? "include" : "omit",
      headers: {
        ...operatorHeaders(auth, true),
        "Idempotency-Key": crypto.randomUUID(),
      },
    },
  );
  return fileResponse<Message>(response, "Could not upload the file.");
}

export async function updateConversationAttachmentPolicy(
  auth: OperatorAuth,
  conversationId: string,
  enabled: boolean,
): Promise<boolean> {
  const { data, error } = await operatorClient(auth).PATCH(
    "/api/v1/conversations/{conversation_id}/widget-attachments",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { conversation_id: conversationId } },
      body: { enabled },
    },
  );
  return requireData(data, error, "Could not update visitor file uploads.").attachments_enabled;
}

export async function downloadOperatorAttachment(
  auth: OperatorAuth,
  attachmentId: string,
): Promise<Blob> {
  const response = await fetch(
    `${apiBaseUrl}/api/v1/attachments/${encodeURIComponent(attachmentId)}`,
    {
      cache: "no-store",
      credentials: auth.kind === "session" ? "include" : "omit",
      headers: operatorHeaders(auth),
    },
  );
  return fileBlobResponse(response, "Could not download the file.");
}

export async function downloadTaskScreenshot(auth: OperatorAuth, attachmentId: string): Promise<File> {
  const response = await fetch(`${apiBaseUrl}/api/v1/ai/task-attachments/${encodeURIComponent(attachmentId)}`, {
    cache: "no-store",
    credentials: auth.kind === "session" ? "include" : "omit",
    headers: operatorHeaders(auth),
  });
  const blob = await fileBlobResponse(response, "Could not download the file.");
  const disposition = response.headers.get("content-disposition") ?? "";
  const encodedName = /filename\*=UTF-8''([^;]+)/i.exec(disposition)?.[1];
  const fileName = encodedName ? decodeURIComponent(encodedName) : /filename="([^"]+)"/i.exec(disposition)?.[1] ?? `attachment-${attachmentId}`;
  return new File([blob], fileName, { type: blob.type });
}

export async function resolveConversation(
  auth: OperatorAuth,
  conversationId: string,
  requestRating = true,
): Promise<Resolution> {
  const { data, error } = await operatorClient(auth).POST(
    "/api/v1/conversations/{conversation_id}/resolve",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { conversation_id: conversationId } },
      body: { request_rating: requestRating },
    },
  );
  return requireData(data, error, "Could not resolve the conversation.");
}

export async function reopenConversation(
  auth: OperatorAuth,
  conversationId: string,
): Promise<void> {
  const { error, response } = await operatorClient(auth).POST(
    "/api/v1/conversations/{conversation_id}/reopen",
    {
      headers: operatorHeaders(auth, true),
      params: { path: { conversation_id: conversationId } },
    },
  );
  requireSuccess(error, response, "Could not reopen the conversation.");
}

export interface SupportQualityFilters {
  inboxId?: string;
  operatorId?: string;
  channelId?: string;
  periodDays?: number;
  rating?: number;
  limit?: number;
}

export async function getSupportQuality(
  auth: OperatorAuth,
  filters: SupportQualityFilters = {},
): Promise<SupportQualityReport> {
  const { data, error } = await operatorClient(auth).GET("/api/v1/support-quality", {
    headers: operatorHeaders(auth),
    params: {
      query: {
        inbox_id: filters.inboxId,
        operator_id: filters.operatorId,
        channel_id: filters.channelId,
        period_days: filters.periodDays,
        rating: filters.rating,
        limit: filters.limit,
      },
    },
  });
  return requireData(data, error, "Could not load support quality.");
}

export async function getEmailSettings(auth: OperatorAuth): Promise<EmailSettings> {
  const { data, error } = await operatorClient(auth).GET("/api/v1/email-settings", {
    cache: "no-store",
    headers: operatorHeaders(auth),
  });
  return requireData(data, error, "Could not load email settings.");
}

export async function updateEmailSettings(
  auth: OperatorAuth,
  input: EmailSettingsInput,
): Promise<EmailSettings> {
  const { data, error } = await operatorClient(auth).PUT("/api/v1/email-settings", {
    headers: operatorHeaders(auth, true),
    body: input,
  });
  return requireData(data, error, "Could not save email settings.");
}

export async function getPublicSupportRating(token: string): Promise<PublicSupportRating> {
  const { data, error } = await publicApi.GET("/public/v1/support-rating", {
    cache: "no-store",
    params: { header: { "X-Support-Rating-Token": token } },
  });
  return requireData(data, error, "This rating link is invalid or has expired.");
}

export async function submitPublicSupportRating(
  token: string,
  rating: number,
  reasons: string[],
  comment: string | null,
): Promise<boolean> {
  const { data, error, response } = await publicApi.POST("/public/v1/support-rating", {
    params: { header: { "X-Support-Rating-Token": token } },
    body: { rating, reasons, comment },
  });
  if (response.status === 409) return false;
  requireData(data, error, "Could not save your rating.");
  return true;
}

export async function createOperatorRealtimeTicket(
  auth: OperatorAuth,
  inboxId: string,
  signal?: AbortSignal,
): Promise<string> {
  const { data, error } = await operatorClient(auth).POST(
    "/api/v1/realtime-tickets",
    {
      signal,
      headers: operatorHeaders(auth, true),
      body: { inbox_id: inboxId },
    },
  );
  return requireData(data, error, "Could not authorize realtime updates.").ticket;
}

interface WidgetSessionApi {
  POST(
    path: "/widget/v1/sessions",
    options: {
      body: {
        widget_id: string;
        visitor_id: string;
        language: WidgetLanguage | null;
        client_context: WidgetClientContext;
      };
    },
  ): Promise<{ data?: WidgetSession; error?: unknown }>;
}

export async function createWidgetSession(
  widgetId: string,
  visitorId: string,
  language: WidgetLanguage | undefined,
  clientContext: WidgetClientContext,
): Promise<WidgetSession> {
  // Browsers emit Origin themselves. The generated OpenAPI client models that
  // required header as caller-supplied, so this adapter deliberately avoids
  // trying to write a browser-controlled header.
  const client = widgetApi as unknown as WidgetSessionApi;
  const { data, error } = await client.POST("/widget/v1/sessions", {
    body: {
      widget_id: widgetId,
      visitor_id: visitorId,
      language: language ?? null,
      client_context: clientContext,
    },
  });
  return requireData(data, error, "This widget is not available on the current site.");
}

export async function createWidgetConversation(token: string): Promise<Conversation> {
  const { data, error } = await widgetApi.POST("/widget/v1/conversations", {
    headers: bearerHeaders(token),
    body: {},
  });
  return requireData(data, error, "Could not start a conversation.");
}

export async function getWidgetPresentation(token: string): Promise<WidgetPresentation> {
  const { data, error } = await widgetApi.GET("/widget/v1/presentation", {
    headers: bearerHeaders(token),
  });
  return requireData(data, error, "Could not refresh the widget design.");
}

export async function updateWidgetPresence(token: string): Promise<WidgetAvailability> {
  const { data, error } = await widgetApi.POST("/widget/v1/presence", {
    headers: bearerHeaders(token),
  });
  return requireData(data, error, "Could not refresh widget presence.");
}

export async function updateWidgetContact(
  token: string,
  input: { display_name: string; email: string },
): Promise<WidgetContactProfile> {
  const { data, error } = await widgetApi.PATCH("/widget/v1/contact", {
    headers: bearerHeaders(token),
    body: input,
  });
  return requireData(data, error, "Could not save your contact details.");
}

export async function listWidgetMessages(
  token: string,
  conversationId: string,
): Promise<Message[]> {
  const { data, error } = await widgetApi.GET(
    "/widget/v1/conversations/{conversation_id}/messages",
    {
      headers: bearerHeaders(token),
      params: {
        path: { conversation_id: conversationId },
        query: { after_sequence: 0, limit: 100 },
      },
    },
  );
  return requireData(data, error, "Could not load this conversation.").items;
}

export async function markWidgetMessagesRead(
  token: string,
  conversationId: string,
  messageIds: string[],
): Promise<void> {
  const { error, response } = await widgetApi.POST(
    "/widget/v1/conversations/{conversation_id}/read",
    {
      headers: bearerHeaders(token),
      params: { path: { conversation_id: conversationId } },
      body: { message_ids: messageIds },
    },
  );
  requireSuccess(error, response, "Could not mark messages as read.");
}

export async function sendWidgetMessage(
  token: string,
  conversationId: string,
  body: string,
): Promise<Message> {
  const { data, error } = await widgetApi.POST(
    "/widget/v1/conversations/{conversation_id}/messages",
    {
      headers: bearerHeaders(token),
      params: { path: { conversation_id: conversationId } },
      body: { client_message_id: crypto.randomUUID(), body },
    },
  );
  return requireData(data, error, "Could not send the message.");
}

export async function uploadWidgetAttachment(
  token: string,
  conversationId: string,
  file: File,
): Promise<Message> {
  const form = new FormData();
  form.append("file", file, file.name);
  const response = await fetch(
    `${apiBaseUrl}/widget/v1/conversations/${encodeURIComponent(conversationId)}/attachments`,
    {
      method: "POST",
      body: form,
      credentials: "omit",
      headers: {
        ...bearerHeaders(token),
        "Idempotency-Key": crypto.randomUUID(),
      },
    },
  );
  return fileResponse<Message>(response, "Could not upload the file.");
}

export async function downloadWidgetAttachment(
  token: string,
  attachmentId: string,
): Promise<Blob> {
  const response = await fetch(
    `${apiBaseUrl}/widget/v1/attachments/${encodeURIComponent(attachmentId)}`,
    {
      cache: "no-store",
      credentials: "omit",
      headers: bearerHeaders(token),
    },
  );
  return fileBlobResponse(response, "Could not download the file.");
}

export async function updateWidgetDraft(
  token: string,
  conversationId: string,
  body: string,
): Promise<void> {
  const { error, response } = await widgetApi.POST(
    "/widget/v1/conversations/{conversation_id}/draft",
    {
      headers: bearerHeaders(token),
      params: { path: { conversation_id: conversationId } },
      body: { body },
    },
  );
  requireSuccess(error, response, "Could not publish the draft.");
}

export async function createWidgetRealtimeTicket(token: string): Promise<string> {
  const { data, error } = await widgetApi.POST("/widget/v1/realtime-tickets", {
    headers: bearerHeaders(token),
  });
  return requireData(data, error, "Could not authorize realtime updates.").ticket;
}

export async function rateResolution(
  token: string,
  resolutionId: string,
  rating: number,
  reasons: string[],
  comment: string | null,
): Promise<components["schemas"]["Rating"]> {
  const { data, error } = await widgetApi.POST(
    "/widget/v1/resolutions/{resolution_id}/rating",
    {
      headers: bearerHeaders(token),
      params: { path: { resolution_id: resolutionId } },
      body: { rating, reasons, comment },
    },
  );
  return requireData(data, error, "Could not save your rating.");
}

export function openRealtimeSocket(
  ticket: string,
  onEvent: (event: RealtimeEvent) => void,
  onClose?: () => void,
  onOpen?: () => void,
): WebSocket {
  const url = new URL("/ws", apiBaseUrl);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.searchParams.set("ticket", ticket);
  const socket = new WebSocket(url);
  socket.addEventListener("message", (message) => {
    try {
      onEvent(JSON.parse(String(message.data)) as RealtimeEvent);
    } catch {
      // Ignore malformed frames; REST remains the source of truth.
    }
  });
  if (onClose) {
    socket.addEventListener("close", onClose);
  }
  if (onOpen) {
    socket.addEventListener("open", onOpen);
  }
  return socket;
}
