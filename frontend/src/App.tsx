import { ResourceVisibilityPermissionContext } from "./resource-visibility";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent,
} from "react";
import {
  createBrowserRouter,
  useBlocker,
  useLocation,
  useNavigate,
  type BlockerFunction,
} from "react-router";
import { RouterProvider } from "react-router/dom";
import {
  ArrowLeft,
  BookOpen,
  Brain,
  CalendarClock,
  ChartNoAxesCombined,
  FolderKanban,
  Inbox,
  KeyRound,
  LogOut,
  Menu,
  MessageSquare,
  MessagesSquare,
  NotebookPen,
  PanelBottomClose,
  PanelBottomOpen,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  PanelTopClose,
  PanelTopOpen,
  Plug,
  Radio,
  Route,
  Shield,
  Users,
  UsersRound,
  UserRoundSearch,
  X,
  type LucideIcon,
} from "lucide-react";
import {
  clearOperatorPresence,
  getCurrentActor,
  getDemoCredentials,
  listInboxes,
  loginWithPassword,
  logoutSession,
  selectProject,
  type ActorContext,
  type Inbox as InboxRecord,
  type OperatorAuth,
  type Project,
  type RealtimeEvent,
} from "./api";
import { AccessTokensView } from "./admin/AccessTokensView";
import { AISettingsView, AITasksView } from "./admin/AISettingsView";
import { ChannelsView } from "./admin/ChannelsView";
import { ParentProjectWorkspace, type ProjectWorkspaceScope } from "./admin/ParentProjectWorkspace";
import { ConversationsView } from "./admin/ConversationsView";
import { conversationLinkInboxId, conversationSegments } from "./admin/conversation-route";
import { DemoReadOnlyContext } from "./admin/DemoReadOnly";
import { ContactsView } from "./admin/DirectoryViews";
import { IntegrationsView } from "./admin/IntegrationsView";
import { InboxRoutingView } from "./admin/InboxRoutingView";
import { KnowledgeBaseView } from "./admin/KnowledgeBaseView";
import { NotesView } from "./admin/NotesView";
import {  } from "./admin/LanguageDropdown";
import {  } from "./admin/PreferenceDropdown";
import {
  THEME_COLORS,
  type PageWidth,
  type Palette,
  type SidebarPosition,
  type Theme,
} from "./admin/preference-options";
import {  } from "./admin/ProjectColorsNote";
import { ReplyTemplatesView } from "./admin/ReplyTemplatesView";
import { OnlineVisitorsView } from "./admin/OnlineVisitorsView";
import { OperatorAvatar } from "./admin/OperatorAvatar";
import { OperatorProfileDialog } from "./admin/OperatorProfileDialog";
import {
  ProjectScopedTokenRequiredScreen,
  ProjectSelectionScreen,
} from "./admin/ProjectSelectionScreen";
import { ProjectSwitcher } from "./admin/ProjectSwitcher";
import { useDepartmentWorkspace } from "./admin/useDepartmentWorkspace";
import { selectDepartment } from "./department-api";
import { departmentNavGroups, departmentStartPage } from "./department-navigation";
import "./admin/department-shell.css";
import { RolesView } from "./admin/RolesView";
import { UsersView } from "./admin/UsersView";
import {  } from "./admin/company-governance-i18n";
import {  } from "./admin/team-i18n";
import { SupportQualityView } from "./admin/SupportQualityView";
import {
  currentBrowserNotificationPermission,
  enableOperatorNotificationSound,
  enableOperatorNotifications,
  playOperatorNotificationSound,
  showOperatorBrowserNotification,
  type BrowserNotificationPermission,
  type OperatorNotificationSound,
} from "./admin/operator-notifications";
import { useOperatorRealtime } from "./admin/useOperatorRealtime";
import { useOperatorPresence } from "./admin/useOperatorPresence";
import {
  DateTimePreferencesContext,
  TIME_FORMAT_KEY,
  WEEK_START_KEY,
  dateTimePreferences,
  initialTimeFormat,
  initialWeekStart,
  type TimeFormat,
  type WeekStart,
} from "./date-time-preferences";
import {
  adminPagePath,
  appRoutePath,
  isAdminPagePath,
  legacyAdminPagePath,
  resolveAdminPage,
  resolveAdminPageSegments,
  resolveAppRoute,
  resolvePublicNoteToken,
  resolveProjectId,
  type AdminPage,
  type AdminPageSegments,
  type AppRoute,
} from "./entry-mode";
import { PageRouteContext, type PageRoute, type PageRouteNavigateOptions } from "./admin/page-route";
import { productNamespace } from "./product-edition";
import type { TaskAccessFlags } from "./admin/AITasksWorkspace";
import { PublicRatingPage } from "./PublicRatingPage";
import { PublicNotesPage } from "./PublicNotesPage";
import { noteSharesText } from "./note-shares-i18n";
import {
  createI18n,
  I18nContext,
  initialLocale,
  localizedError,
  roleLabel,
  useI18n,
  type Locale,
  type MessageKey,
} from "./i18n";

type Page = AdminPage;
type LoginMode = "password" | "access_token";

interface NavItem {
  id: Page;
  label: MessageKey;
  icon: LucideIcon;
  visible: boolean;
}

interface NavGroup {
  id: string;
  label: MessageKey;
  items: NavItem[];
}

interface NavIndicatorPosition {
  height: number;
  /** Top/bottom sidebars lay the navigation out as a row, so the indicator also tracks x and width. */
  horizontal: boolean;
  visible: boolean;
  width: number;
  x: number;
  y: number;
}

/** Employees see their own tasks; Lite keeps tasks for those who manage them. */
function taskAccess(actor: ActorContext): TaskAccessFlags {
  const has = (permission: string) => (actor.permissions as string[]).includes(permission);
  const manage = has("tasks:manage");
  return { own: has("tasks:own") || manage, manage, configure: manage && has("tasks:configure"), ai: has("ai:manage") };
}

function canOpenTasks(actor: ActorContext): boolean {
  const access = taskAccess(actor);
  return access.manage;
}

function adminNavGroups(actor: ActorContext): NavGroup[] {
  const passwordSession = actor.auth_method === "session";
  const canManageAi = actor.permissions.includes("ai:manage");
  const projectScope = hasProjectWorkspaceAccess(actor);

  return [
    {
      id: "work",
      label: "nav.group.work",
      items: [
        { id: "conversations", label: "nav.conversations", icon: MessageSquare, visible: actor.permissions.includes("conversations:read") },
        { id: "contacts", label: "nav.contacts", icon: Users, visible: actor.permissions.includes("contacts:read") },
        { id: "tasks", label: "nav.tasks", icon: CalendarClock, visible: canOpenTasks(actor) && (passwordSession || projectScope) },
        { id: "notes", label: "nav.notes", icon: NotebookPen, visible: !actor.is_demo && Boolean(actor.project_id) && actor.permissions.includes("notes:read") },
        { id: "online_visitors", label: "nav.onlineVisitors", icon: UserRoundSearch, visible: actor.permissions.includes("visitor_network:read") },
      ],
    },
    {
      id: "quality",
      label: "nav.group.quality",
      items: [
        { id: "support_quality", label: "nav.supportQuality", icon: ChartNoAxesCombined, visible: actor.permissions.includes("quality:read") },
      ],
    },
    {
      id: "management",
      label: "nav.group.management",
      items: [
        { id: "ai", label: "nav.ai", icon: Brain, visible: canManageAi },
        { id: "inbox_routing", label: "nav.inboxRouting", icon: Route, visible: actor.permissions.includes("routing:manage") },
        { id: "channels", label: "nav.channels", icon: Radio, visible: actor.permissions.includes("channels:read") },
        { id: "knowledge_base", label: "nav.knowledgeBase", icon: BookOpen, visible: actor.permissions.includes("knowledge:manage") },
        { id: "reply_templates", label: "nav.replyTemplates", icon: MessagesSquare, visible: actor.permissions.includes("conversations:reply") || actor.permissions.includes("reply_templates:manage") },
        { id: "integrations", label: "nav.integrations", icon: Plug, visible: passwordSession && Boolean(actor.project_id) && (actor.inbox_scope === null || Boolean(actor.department_id)) && actor.permissions.includes("integrations:manage") },
      ],
    },
    {
      id: "administration",
      label: "nav.group.administration",
      items: [
        { id: "users", label: "nav.users", icon: UsersRound, visible: canManageProjectUsers(actor) },
        { id: "roles", label: "nav.roles", icon: Shield, visible: passwordSession && actor.permissions.includes("roles:manage") },
        { id: "access_tokens", label: "nav.accessTokens", icon: KeyRound, visible: passwordSession && actor.permissions.includes("access_tokens:manage") },
      ],
    },
  ];
}

const TOKEN_KEY = `${productNamespace}-operator-token`;
const TOKEN_PROJECT_KEY = `${productNamespace}-operator-token-project`;
const INBOX_KEY = `${productNamespace}-active-inbox`;
const NOTIFICATIONS_KEY = `${productNamespace}-operator-notifications`;
const SIDEBAR_KEY = `${productNamespace}-admin-sidebar`;
const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "input:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

const SIDEBAR_TOGGLE_ICONS: Record<SidebarPosition, { collapse: LucideIcon; expand: LucideIcon }> = {
  left: { collapse: PanelLeftClose, expand: PanelLeftOpen },
  right: { collapse: PanelRightClose, expand: PanelRightOpen },
  top: { collapse: PanelTopClose, expand: PanelTopOpen },
  bottom: { collapse: PanelBottomClose, expand: PanelBottomOpen },
};

interface OperatorToast {
  id: string;
  title: string;
  body: string;
  conversationId?: string;
  inboxId?: string;
}

function storedValue(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function focusableElements(container: HTMLElement | null): HTMLElement[] {
  if (!container) return [];
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR))
    .filter((element) => element.getAttribute("aria-hidden") !== "true");
}

function storeValue(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Preferences remain available for the current page when storage is blocked.
  }
}

function realtimeString(event: RealtimeEvent, key: string): string | null {
  const value = event.data[key];
  return typeof value === "string" && value.trim() ? value : null;
}

function initialTheme(): Theme {
  return "light";
  
}

function initialPalette(): Palette {
  return "graphite";
  
}

function initialPageWidth(): PageWidth {
  return "contained";
  
}

function initialSidebarPosition(): SidebarPosition {
  return "left";
  
}

function isHorizontalSidebar(position: SidebarPosition): boolean {
  return position === "top" || position === "bottom";
}

interface SidebarFooterProps {
  authKind: OperatorAuth["kind"];
  onLogout: () => void;
}

function SidebarFooter({ authKind, onLogout }: SidebarFooterProps) {
  const { t } = useI18n();
  const logoutLabel = t(authKind === "session" ? "nav.signOut" : "nav.disconnect");

  return (
    <div className="sidebar-footer">
      <button
        className="sidebar-footer-button sidebar-footer-button--logout"
        type="button"
        aria-label={logoutLabel}
        title={logoutLabel}
        onClick={onLogout}
      >
        <LogOut size={17} />
        <span>{logoutLabel}</span>
      </button>
    </div>
  );
}

interface LoginScreenProps {
  branded: boolean;
  error: string | null;
  loading: boolean;
  demoLoading: boolean;
  email: string;
  password: string;
  onEmailChange: (email: string) => void;
  onPasswordChange: (password: string) => void;
  onTryDemo: () => void;
  onBackToLanding?: () => void;
  onClearError: () => void;
  onPasswordLogin: (email: string, password: string) => void;
  onTokenLogin: (token: string) => void;
}

function LoginScreen({ branded, error, loading, demoLoading, email, password, onEmailChange, onPasswordChange, onTryDemo, onBackToLanding, onClearError, onPasswordLogin, onTokenLogin }: LoginScreenProps) {
  const { t } = useI18n();
  const [mode, setMode] = useState<LoginMode>("password");
  const [token, setToken] = useState("");

  function changeMode(nextMode: LoginMode) {
    setMode(nextMode);
    onClearError();
  }

  return (
    <main className={`connection-page connection-page--login${branded ? "" : " connection-page--neutral"}`}>
      <div className={`login-window${" login-window--lite"}`}>
        
        <div className="login-access">
          <section className="connection-panel" aria-labelledby="login-heading">
            
            <div className="connection-form">
              <div className="connection-heading">
                <h1 id="login-heading">{t("login.heading")}</h1>
                <p>{t("login.description")}</p>
              </div>
              {mode === "password" ? (
                <form
                  key="password"
                  aria-busy={loading || demoLoading}
                  onSubmit={(event) => {
                    event.preventDefault();
                    if (email.trim() && password) onPasswordLogin(email.trim(), password);
                  }}
                >
                  <label htmlFor="email">{t("login.email")}</label>
                  <input id="email" type="email" autoComplete="username" value={email} disabled={demoLoading} autoFocus aria-invalid={Boolean(error)} aria-describedby={error ? "login-error" : undefined} onChange={(event) => onEmailChange(event.target.value)} />
                  <label htmlFor="password">{t("login.password")}</label>
                  <input id="password" type="password" autoComplete="current-password" value={password} disabled={demoLoading} aria-invalid={Boolean(error)} aria-describedby={error ? "login-error password-help" : "password-help"} onChange={(event) => onPasswordChange(event.target.value)} />
                  <p className="field-help" id="password-help">{t("login.passwordHelp")}</p>
                  {error && <div className="admin-notice admin-notice--error" id="login-error" role="alert">{error}</div>}
                  <button className="primary-button" type="submit" disabled={loading || demoLoading || !email.trim() || !password}>
                    {loading ? t("login.signingIn") : t("login.signIn")}
                  </button>
                  {branded && <button className="secondary-button" type="button" disabled={loading || demoLoading} onClick={onTryDemo}>{t(demoLoading ? "login.demoLoading" : "login.tryDemo")}</button>}
                  <button className="login-alternate" type="button" disabled={loading || demoLoading} onClick={() => changeMode("access_token")}><KeyRound size={15} />{t("login.useAccessToken")}</button>
                </form>
              ) : (
                <form
                  key="access-token"
                  aria-busy={loading}
                  onSubmit={(event) => {
                    event.preventDefault();
                    if (token.trim()) onTokenLogin(token.trim());
                  }}
                >
                  <label htmlFor="api-token">{t("login.accessToken")}</label>
                  <input id="api-token" type="password" autoComplete="off" value={token} placeholder="tzm_…" autoFocus aria-invalid={Boolean(error)} aria-describedby={error ? "login-error token-help" : "token-help"} onChange={(event) => setToken(event.target.value)} />
                  <p className="field-help" id="token-help">{t("login.tokenHelp")}</p>
                  {error && <div className="admin-notice admin-notice--error" id="login-error" role="alert">{error}</div>}
                  <button className="primary-button" type="submit" disabled={loading || !token.trim()}>
                    {loading ? t("login.connecting") : t("login.connect")}
                  </button>
                  <button className="login-alternate" type="button" disabled={loading} onClick={() => changeMode("password")}>{t("login.backToPassword")}</button>
                </form>
              )}
            </div>
          </section>
          {onBackToLanding && (
            <button className="connection-back" type="button" onClick={onBackToLanding}>
              <ArrowLeft size={15} />
              {t("login.backToHomepage")}
            </button>
          )}
        </div>
      </div>
    </main>
  );
}

function activeInbox(inboxes: InboxRecord[], ...preferred: (string | null)[]): string | null {
  return preferred.find((inboxId) => inboxId && inboxes.some((inbox) => inbox.id === inboxId))
    ?? inboxes[0]?.id
    ?? null;
}

function loadActorInboxes(auth: OperatorAuth, actor: ActorContext): Promise<InboxRecord[]> {
  return ["conversations:read", "routing:manage", "reply_templates:manage"].some((permission) => actor.permissions.includes(permission))
    ? listInboxes(auth)
    : Promise.resolve([]);
}

function hasProjectWorkspaceAccess(actor: ActorContext): boolean {
  return actor.inbox_scope === null || (actor.auth_method === "session" && actor.department_restricted === false);
}

function canManageProjectUsers(actor: ActorContext): boolean {
  return actor.auth_method === "session" && actor.role === "admin"
    && Boolean(actor.project_id) && !actor.department_restricted && !actor.is_demo;
}

function authForProject(auth: OperatorAuth, actor: ActorContext): OperatorAuth {
  if (!actor.project_id) throw new Error("Workspace unavailable.");
  return actor.project_id
    ? { ...auth, projectId: actor.project_id }
    : auth;
}

function rememberTokenProject(auth: OperatorAuth) {
  if (auth.kind === "access_token" && auth.projectId) sessionStorage.setItem(TOKEN_PROJECT_KEY, auth.projectId);
  else sessionStorage.removeItem(TOKEN_PROJECT_KEY);
}

async function actorForPage(_auth: OperatorAuth, actor: ActorContext): Promise<ActorContext> {
  return actor;
}

const UNSAVED_CHANGES_MESSAGES = {
  inbox_routing: "routing.discardChangesNavigation",
  knowledge_base: "knowledge.discardChangesConfirm",
  notes: "notes.discardConfirm",
  reply_templates: "replyTemplates.discardConfirm",
} as const satisfies Partial<Record<Page, MessageKey>>;
type GuardedPage = keyof typeof UNSAVED_CHANGES_MESSAGES;
// Messages for moves that stay on the page but replace what its editor shows.
const UNSAVED_IN_PAGE_MESSAGES: Partial<Record<GuardedPage, MessageKey>> = {
  inbox_routing: "routing.discardChanges",
};
// Workspace switches ask about every page that can hold unsaved changes, in this order.
const GUARDED_PAGES: readonly GuardedPage[] = ["reply_templates", "inbox_routing", "knowledge_base", "notes"];
const NO_PAGE_SEGMENTS: AdminPageSegments = [];

function isGuardedPage(page: Page): page is GuardedPage {
  return (GUARDED_PAGES as readonly Page[]).includes(page);
}

function samePage(path: string, otherPath: string): boolean {
  return resolveAdminPage(path) === resolveAdminPage(otherPath) && resolveProjectId(path) === resolveProjectId(otherPath);
}

export function App() {
  // Each mounted app owns a router, so it keeps the URL it was opened with.
  const [router] = useState(() => createBrowserRouter([{ path: "*", element: <RoutedApp /> }]));
  // URL changes render together with the state updates that caused them, as with direct history updates.
  return <RouterProvider router={router} useTransitions={false} />;
}

function RoutedApp() {
  const [locale] = useState<Locale>(initialLocale);
  const location = useLocation();
  const navigateTo = useNavigate();
  const pathname = location.pathname;
  const pathnameRef = useRef(pathname);
  const unsavedPagesRef = useRef(new Set<GuardedPage>());
  const publicNotesDirtyRef = useRef(false);
  const route = resolveAppRoute(pathname);
  const page = resolveAdminPage(pathname);
  const pageSegments = useMemo(() => resolveAdminPageSegments(pathname), [pathname]);
  const pagePathKnown = isAdminPagePath(pathname);
  const projectId = resolveProjectId(pathname);
  // The served document fixes the workspace language; nothing switches it at runtime.
  const setLocale = useCallback(() => undefined, []);
  const [timeFormat, setTimeFormat] = useState<TimeFormat>(initialTimeFormat);
  const [weekStart, setWeekStart] = useState<WeekStart>(initialWeekStart);
  const dateTime = useMemo(() => dateTimePreferences(timeFormat, weekStart), [timeFormat, weekStart]);
  const i18n = useMemo(() => createI18n(locale, setLocale, dateTime.hourCycle), [dateTime.hourCycle, locale, setLocale]);
  const changeTimeFormat = useCallback((nextTimeFormat: TimeFormat) => {
    setTimeFormat(nextTimeFormat);
    storeValue(TIME_FORMAT_KEY, nextTimeFormat);
  }, []);
  const changeWeekStart = useCallback((nextWeekStart: WeekStart) => {
    setWeekStart(nextWeekStart);
    storeValue(WEEK_START_KEY, nextWeekStart);
  }, []);

  useLayoutEffect(() => {
    document.documentElement.dataset.productEdition = "lite";
  }, []);

  useLayoutEffect(() => {
    pathnameRef.current = pathname;
  }, [pathname]);

  const navigatePath = useCallback((nextPath: string, replace = false) => {
    if (`${window.location.pathname}${window.location.search}${window.location.hash}` === nextPath) return undefined;
    return navigateTo(nextPath, { replace });
  }, [navigateTo]);

  useEffect(() => {
    const legacyPath = legacyAdminPagePath(location.pathname, location.search);
    if (legacyPath) navigatePath(`${legacyPath}${location.hash}`, true);
  }, [location.hash, location.pathname, location.search, navigatePath]);

  const confirmDiscard = useCallback((pages: readonly GuardedPage[], stayingOnPage = false) => {
    for (const guardedPage of pages) {
      if (!unsavedPagesRef.current.has(guardedPage)) continue;
      const inPageMessage = stayingOnPage ? UNSAVED_IN_PAGE_MESSAGES[guardedPage] : undefined;
      const message = inPageMessage ? i18n.t(inPageMessage) : i18n.t(UNSAVED_CHANGES_MESSAGES[guardedPage]);
      if (!window.confirm(message)) return false;
    }
    // A page that stays open reports for itself whether it dropped its changes.
    if (!stayingOnPage) for (const guardedPage of pages) unsavedPagesRef.current.delete(guardedPage);
    return true;
  }, [i18n]);

  // Any move away from the current path, even to the same page's root, would replace its unsaved editor.
  const confirmRoutingLeave = useCallback((nextPath: string) => {
    const currentPath = pathnameRef.current;
    const currentPage = resolveAdminPage(currentPath);
    return nextPath === currentPath || !isGuardedPage(currentPage)
      || confirmDiscard([currentPage], samePage(currentPath, nextPath));
  }, [confirmDiscard]);

  const unsavedChangesHandler = useCallback((guardedPage: GuardedPage) => (dirty: boolean) => {
    if (dirty) unsavedPagesRef.current.add(guardedPage);
    else unsavedPagesRef.current.delete(guardedPage);
  }, []);
  const updateRoutingDirty = useMemo(() => unsavedChangesHandler("inbox_routing"), [unsavedChangesHandler]);
  const updateReplyTemplatesDirty = useMemo(() => unsavedChangesHandler("reply_templates"), [unsavedChangesHandler]);
  const updateNotesDirty = useMemo(() => unsavedChangesHandler("notes"), [unsavedChangesHandler]);
  const updatePublicNotesDirty = useCallback((dirty: boolean) => { publicNotesDirtyRef.current = dirty; }, []);
  const updateKnowledgeBaseDirty = useMemo(() => unsavedChangesHandler("knowledge_base"), [unsavedChangesHandler]);

  const confirmWorkspaceLeave = useCallback(() => confirmDiscard(GUARDED_PAGES), [confirmDiscard]);

  // In-app links ask before they navigate; browser Back/Forward is held here until the operator answers.
  const shouldBlockHistoryMove = useCallback<BlockerFunction>(({ currentLocation, nextLocation, historyAction }) => {
    if (resolveAppRoute(currentLocation.pathname) === "public_notes") {
      return publicNotesDirtyRef.current
        && (currentLocation.pathname !== nextLocation.pathname || currentLocation.search !== nextLocation.search);
    }
    const currentPage = resolveAdminPage(currentLocation.pathname);
    return historyAction === "POP"
      && currentLocation.pathname !== nextLocation.pathname
      && isGuardedPage(currentPage)
      && unsavedPagesRef.current.has(currentPage);
  }, []);
  const historyBlocker = useBlocker(shouldBlockHistoryMove);
  useEffect(() => {
    if (historyBlocker.state !== "blocked") return;
    const currentPath = pathnameRef.current;
    if (resolveAppRoute(currentPath) === "public_notes") {
      if (!publicNotesDirtyRef.current || window.confirm(noteSharesText(locale)("discardConfirm"))) {
        publicNotesDirtyRef.current = false;
        historyBlocker.proceed();
      } else historyBlocker.reset();
      return;
    }
    const currentPage = resolveAdminPage(currentPath);
    if (!isGuardedPage(currentPage)
      || confirmDiscard([currentPage], samePage(currentPath, historyBlocker.location.pathname))) historyBlocker.proceed();
    else historyBlocker.reset();
  }, [confirmDiscard, historyBlocker, locale]);

  const navigate = useCallback((nextRoute: AppRoute) => {
    navigatePath(appRoutePath(nextRoute));
  }, [navigatePath]);

  const navigatePage = useCallback((
    nextPage: Page,
    nextProjectId = resolveProjectId(pathnameRef.current),
    segments: AdminPageSegments = [],
  ) => {
    const nextPath = adminPagePath(nextPage, nextProjectId, segments);
    if (!confirmRoutingLeave(nextPath)) return false;
    navigatePath(nextPath);
    return true;
  }, [confirmRoutingLeave, navigatePath]);

  const replacePage = useCallback((nextPage: Page, nextProjectId = resolveProjectId(pathnameRef.current)) => {
    const currentPath = pathnameRef.current;
    const samePage = resolveAdminPage(currentPath) === nextPage;
    const suffix = samePage ? `${window.location.search}${window.location.hash}` : "";
    const segments = samePage ? resolveAdminPageSegments(currentPath) : [];
    navigatePath(`${adminPagePath(nextPage, nextProjectId, segments)}${suffix}`, true);
  }, [navigatePath]);

  const navigatePageSegments = useCallback((
    nextPage: Page,
    segments: AdminPageSegments,
    options: PageRouteNavigateOptions = {},
  ) => navigatePath(adminPagePath(nextPage, resolveProjectId(pathnameRef.current), segments), options.replace), [navigatePath]);

  useEffect(() => {
    document.documentElement.lang = locale;
    if (route !== "public_notes") document.title = i18n.t("login.operatorWorkspace");
    const existingIcon = document.querySelector<HTMLLinkElement>('link[rel~="icon"]');
    existingIcon?.remove();
    
  }, [i18n, locale, route]);

  return (
    <I18nContext.Provider value={i18n}><DateTimePreferencesContext.Provider value={dateTime}>
      {route === "public_notes" ? (
        <PublicNotesPage key={pathname} token={resolvePublicNoteToken(pathname) ?? ""} onDirtyChange={updatePublicNotesDirty} />
      ) : route === "rating" ? (
        <PublicRatingPage />
      ) : (
        <AdminApp
          route={route}
          page={page}
          pageSegments={pageSegments}
          pagePathKnown={pagePathKnown}
          projectId={projectId}
          timeFormat={timeFormat}
          weekStart={weekStart}
          onTimeFormatChange={changeTimeFormat}
          onWeekStartChange={changeWeekStart}
          onNavigate={navigate}
          onPageNavigate={navigatePage}
          onPageReplace={replacePage}
          onPageSegmentsNavigate={navigatePageSegments}
          onRoutingDirtyChange={updateRoutingDirty}
          onReplyTemplatesDirtyChange={updateReplyTemplatesDirty}
          onKnowledgeBaseDirtyChange={updateKnowledgeBaseDirty}
          onNotesDirtyChange={updateNotesDirty}
          onWorkspaceLeave={confirmWorkspaceLeave}
        />
      )}
    </DateTimePreferencesContext.Provider></I18nContext.Provider>
  );
}

interface SessionFailure {
  cause: unknown;
  fallback: MessageKey;
}

function useCabinetPageRoute(page: Page | undefined, segments: readonly string[], onNavigate: (
  page: Page, segments: readonly string[], options?: PageRouteNavigateOptions,
) => void | Promise<void>): PageRoute {
  const navigate = useCallback<PageRoute["navigate"]>((next, options) => page
    ? onNavigate(page, next, options) : undefined, [page, onNavigate]);
  return useMemo(() => ({ segments, navigate }), [segments, navigate]);
}

function AdminApp({
  route,
  page,
  pageSegments,
  pagePathKnown,
  projectId,
  onNavigate,
  onPageNavigate,
  onPageReplace,
  onPageSegmentsNavigate,
  onRoutingDirtyChange,
  onReplyTemplatesDirtyChange,
  onKnowledgeBaseDirtyChange,
  onNotesDirtyChange,
  onWorkspaceLeave,
}: {
  route: AppRoute;
  page: Page;
  pageSegments: AdminPageSegments;
  pagePathKnown: boolean;
  projectId: string | null;
  timeFormat: TimeFormat;
  weekStart: WeekStart;
  onTimeFormatChange: (timeFormat: TimeFormat) => void;
  onWeekStartChange: (weekStart: WeekStart) => void;
  onNavigate: (route: AppRoute) => void;
  onPageNavigate: (page: Page, projectId?: string | null, segments?: AdminPageSegments) => boolean;
  onPageReplace: (page: Page, projectId?: string | null) => void;
  onPageSegmentsNavigate: (
    page: Page,
    segments: AdminPageSegments,
    options?: PageRouteNavigateOptions,
  ) => void | Promise<void>;
  onRoutingDirtyChange: (dirty: boolean) => void;
  onReplyTemplatesDirtyChange: (dirty: boolean) => void;
  onKnowledgeBaseDirtyChange: (dirty: boolean) => void;
  onNotesDirtyChange: (dirty: boolean) => void;
  onWorkspaceLeave: () => boolean;
}) {
  const { t } = useI18n();
  const [menuOpen, setMenuOpen] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(
    () => storedValue(SIDEBAR_KEY) === "collapsed",
  );
  const theme = initialTheme();
  const palette = initialPalette();
  const pageWidth = initialPageWidth();
  const sidebarPosition = initialSidebarPosition();
  const [auth, setAuth] = useState<OperatorAuth | null>(null);
  const [actor, setActor] = useState<ActorContext | null>(null);
  const [workspaceProjects, setWorkspaceProjects] = useState<{ auth: OperatorAuth; items: Project[] } | null>(null);
  const loadedProjectRef = useRef<string | null | undefined>(undefined);
  const workspaceRevisionRef = useRef(0);
  const departments = useDepartmentWorkspace(auth, actor);
  // Reminders belong to whoever may open the task; a demo session shows none.
  const [workspaceChanging, setWorkspaceChanging] = useState(false);
  const [workspaceError, setWorkspaceError] = useState(false);
  const [workspaceRefreshError, setWorkspaceRefreshError] = useState(false);
  const [projectSelectionRequired, setProjectSelectionRequired] = useState(false);
  const [projectTokenRequired, setProjectTokenRequired] = useState(false);
  const [inboxes, setInboxes] = useState<InboxRecord[]>([]);
  const [activeInboxId, setActiveInboxId] = useState<string | null>(null);
  const [checkingSession, setCheckingSession] = useState(true);
  const [loginLoading, setLoginLoading] = useState(false);
  const [demoLoading, setDemoLoading] = useState(false);
  const [loginEmail, setLoginEmail] = useState("");
  const [loginPassword, setLoginPassword] = useState("");
  const [sessionError, setSessionError] = useState<SessionFailure | null>(null);
  const [realtimeEvent, setRealtimeEvent] = useState<RealtimeEvent | null>(null);
  const [realtimeSyncRevision, setRealtimeSyncRevision] = useState(0);
  const [notificationPermission, setNotificationPermission] = useState<BrowserNotificationPermission>(
    () => currentBrowserNotificationPermission(),
  );
  const [notificationsEnabled, setNotificationsEnabled] = useState(
    () => notificationPermission === "granted" && storedValue(NOTIFICATIONS_KEY) === "enabled",
  );
  const [notificationsChanging, setNotificationsChanging] = useState(false);
  const soundEnabled = true;
  const notificationSound: OperatorNotificationSound = "pearl_chime";
  const [notificationToast, setNotificationToast] = useState<OperatorToast | null>(null);
  const [profileOpen, setProfileOpen] = useState(false);
  const notificationsChangePendingRef = useRef(false);
  const sidebarRef = useRef<HTMLElement>(null);
  const navRef = useRef<HTMLElement>(null);
  const mobileMenuButtonRef = useRef<HTMLButtonElement>(null);
  const mobileCloseButtonRef = useRef<HTMLButtonElement>(null);
  const [navIndicator, setNavIndicator] = useState<NavIndicatorPosition>({
    height: 0,
    horizontal: false,
    visible: false,
    width: 0,
    x: 0,
    y: 0,
  });
  const [navIndicatorReady, setNavIndicatorReady] = useState(false);
  const closeMobileMenu = useCallback(() => {
    setMenuOpen(false);
    mobileMenuButtonRef.current?.focus();
  }, []);

  const appliedPalette = palette;
  const appliedTheme = theme;


  useEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = appliedTheme;
    root.dataset.palette = appliedPalette;
    root.style.colorScheme = appliedTheme;
    
    const themeColor = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]');
    if (themeColor) themeColor.content = THEME_COLORS[appliedPalette][appliedTheme];
  }, [appliedPalette, appliedTheme, palette, theme]);



  useEffect(() => {
    if (!menuOpen) return undefined;
    mobileCloseButtonRef.current?.focus();

    const handleMobileMenuKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        closeMobileMenu();
        return;
      }
      if (event.key !== "Tab") return;

      const elements = focusableElements(sidebarRef.current)
        .filter((element) => !element.classList.contains("sidebar-collapse-toggle"));
      const first = elements[0];
      const last = elements.at(-1);
      if (!first || !last) return;
      if (!sidebarRef.current?.contains(document.activeElement)) {
        event.preventDefault();
        first.focus();
      } else if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleMobileMenuKeyDown);
    return () => document.removeEventListener("keydown", handleMobileMenuKeyDown);
  }, [closeMobileMenu, menuOpen]);

  useEffect(() => {
    if (!notificationToast) return undefined;
    const timer = window.setTimeout(() => setNotificationToast(null), 7_000);
    return () => window.clearTimeout(timer);
  }, [notificationToast]);

  useEffect(() => {
    if (!soundEnabled) return undefined;
    const unlockSound = () => {
      document.removeEventListener("pointerdown", unlockSound);
      document.removeEventListener("keydown", unlockSound);
      void enableOperatorNotificationSound();
    };
    document.addEventListener("pointerdown", unlockSound);
    document.addEventListener("keydown", unlockSound);
    return () => {
      document.removeEventListener("pointerdown", unlockSound);
      document.removeEventListener("keydown", unlockSound);
    };
  }, [soundEnabled]);

  useEffect(() => {
    const syncNotificationPermission = () => {
      const permission = currentBrowserNotificationPermission();
      setNotificationPermission(permission);
      setNotificationsEnabled(
        permission === "granted" && storedValue(NOTIFICATIONS_KEY) === "enabled",
      );
    };
    window.addEventListener("focus", syncNotificationPermission);
    document.addEventListener("visibilitychange", syncNotificationPermission);
    return () => {
      window.removeEventListener("focus", syncNotificationPermission);
      document.removeEventListener("visibilitychange", syncNotificationPermission);
    };
  }, []);

  const handleRealtime = useCallback((event: RealtimeEvent) => {
    setRealtimeEvent(event);
    if (
      event.type === "contact.updated"
      || event.type.startsWith("conversation.")
      || event.type.startsWith("message.")
    ) {
      setRealtimeSyncRevision((current) => current + 1);
    }
    let notification: Omit<OperatorToast, "id"> | null = null;
    if (event.type === "visitor.entered" && event.data.notify === true) {
      const widget = realtimeString(event, "widget_name") ?? t("notifications.websiteWidget");
      const page = realtimeString(event, "page_title") ?? realtimeString(event, "page_url");
      notification = {
        title: t("notifications.newVisitor"),
        body: page
          ? t("notifications.newVisitorPage", { widget, page })
          : t("notifications.newVisitorWidget", { widget }),
      };
    } else if (event.type === "message.created" && event.data.direction === "inbound" && event.data.contact_is_blocked !== true && event.data.phone_call !== true) {
      const conversationId = realtimeString(event, "conversation_id");
      notification = {
        title: t("notifications.newMessage"),
        body: t("notifications.newMessageBody"),
        ...(conversationId ? { conversationId, inboxId: event.inbox_id } : {}),
      };
    }
    if (!notification) return;
    setNotificationToast({ ...notification, id: event.event_id });
    if (soundEnabled) {
      playOperatorNotificationSound(notificationSound);
    }
    if (notificationsEnabled) {
      showOperatorBrowserNotification(notification.title, notification.body);
    }
  }, [notificationsEnabled, setNotificationToast, soundEnabled, t]);

  const handleRealtimeSync = useCallback(() => {
    setRealtimeSyncRevision((current) => current + 1);
  }, []);

  const cabinetAuth = route === "cabinet"
    && !projectSelectionRequired
    && !projectTokenRequired
    && !checkingSession
    && (!projectId || actor?.project_id === projectId)
    ? auth
    : null;
  useOperatorRealtime(cabinetAuth, activeInboxId, handleRealtime, handleRealtimeSync);
  useOperatorPresence(cabinetAuth);

  useEffect(() => {
    if (loadedProjectRef.current === projectId) return;
    let active = true;
    const savedToken = sessionStorage.getItem(TOKEN_KEY);
    const savedProjectId = sessionStorage.getItem(TOKEN_PROJECT_KEY);
    const sessionAuth: OperatorAuth = { kind: "session", ...(projectId ? { projectId } : {}) };
    const candidates: OperatorAuth[] = savedToken
      ? projectId ? [
          { kind: "access_token", token: savedToken, projectId }, sessionAuth,
        ] : [
          ...(savedProjectId ? [{ kind: "access_token" as const, token: savedToken, projectId: savedProjectId }] : []),
          { kind: "access_token", token: savedToken }, sessionAuth,
        ]
      : [sessionAuth];

    async function bootstrap() {
      workspaceRevisionRef.current += 1;
      loadedProjectRef.current = undefined;
      setCheckingSession(true);
      setWorkspaceChanging(false);
      setWorkspaceError(false);
      setWorkspaceRefreshError(false);
      setAuth(null);
      setActor(null);
      setInboxes([]);
      setActiveInboxId(null);
      setRealtimeEvent(null);
      setNotificationToast(null);
      setSessionError(null);
      for (const candidate of candidates) {
        try {
          const restoredActor = await getCurrentActor(candidate);
          if (projectId && restoredActor.project_id !== projectId) throw new Error("Project unavailable.");
          if (!active) return;
          const currentActor = await actorForPage(authForProject(candidate, restoredActor), restoredActor);
          const resolvedAuth = authForProject(candidate, currentActor);
          const needsProjectSelection = candidate.kind === "session"
            && !currentActor.project_id;
          const needsProjectToken = candidate.kind === "access_token"
            && !currentActor.project_id;
          const visibleInboxes = needsProjectSelection || needsProjectToken
            ? []
            : await loadActorInboxes(resolvedAuth, currentActor);
          if (!active) return;
          loadedProjectRef.current = currentActor.project_id ?? null;
          setAuth(currentActor.is_demo ? { ...resolvedAuth, isDemo: true } : resolvedAuth);
          rememberTokenProject(resolvedAuth);
          setActor(currentActor);
          setProjectSelectionRequired(needsProjectSelection);
          setProjectTokenRequired(needsProjectToken);
          setInboxes(visibleInboxes);
          setActiveInboxId(activeInbox(
            visibleInboxes,
            conversationLinkInboxId(window.location.pathname),
            storedValue(INBOX_KEY),
          ));
          setCheckingSession(false);
          if (resolveAppRoute(window.location.pathname) === "cabinet"
            && !window.location.search && !window.location.hash
            && (window.location.pathname === adminPagePath("conversations", projectId) || !isAdminPagePath(window.location.pathname))) {
            onPageReplace(departmentStartPage(currentActor), currentActor.project_id);
          }
          return;
        } catch (cause) {
          if (!active) return;
          if (projectId) setSessionError({ cause, fallback: "projects.switchError" });
          if (candidate.kind === "access_token" && !projectId) {
            sessionStorage.removeItem(TOKEN_PROJECT_KEY);
            if (!candidate.projectId) sessionStorage.removeItem(TOKEN_KEY);
          }
        }
      }
    }

    void bootstrap().finally(() => {
      if (active) setCheckingSession(false);
    });
    return () => { active = false; };
  }, [onPageReplace, projectId]);

  const activeDepartment = departments.data?.items.find((item) => item.id === actor?.department_id) ?? null;
  const departmentNavigationReady = !departments.enabled || Boolean(departments.data);
  const availableNavGroups = departmentNavigationReady
    ? departmentNavGroups(actor ? adminNavGroups(actor) : [], null)
    : [];
  const visibleNavGroups = availableNavGroups.map((group) => ({
    ...group,
    name: group.customName ?? t(group.label),
  }));
  const visiblePages = visibleNavGroups.flatMap((group) => group.items.map((item) => item.id));
  const displayedPage = visiblePages.includes(page) ? page : visiblePages[0];
  const navLayoutKey = JSON.stringify([visibleNavGroups.map((group) => [group.id, group.name, group.items.map((item) => item.id)])]);
  const displayedPageSegments = displayedPage === page && pagePathKnown ? pageSegments : NO_PAGE_SEGMENTS;
  const canSelectAllTaskProjects = false;
  const allTaskProjects = canSelectAllTaskProjects && displayedPageSegments[0] === "all-projects";
  const taskView = displayedPageSegments[allTaskProjects ? 1 : 0];
  const taskViewSegments = taskView === "calendar" || taskView === "list" ? [taskView] : [];
  const pageRoute = useCabinetPageRoute(displayedPage, displayedPageSegments, onPageSegmentsNavigate);

  const positionNavIndicator = useCallback(() => {
    const nav = navRef.current;
    const activeLink = nav?.querySelector<HTMLElement>('[aria-current="page"]');
    if (!nav || !activeLink) {
      setNavIndicator((current) => (
        current.visible ? { ...current, visible: false } : current
      ));
      return;
    }

    const navBounds = nav.getBoundingClientRect();
    const activeBounds = activeLink.getBoundingClientRect();
    // The stylesheet decides the layout (the mobile drawer stays vertical for every position).
    const horizontal = getComputedStyle(nav).getPropertyValue("--nav-orientation").trim() === "horizontal";
    const nextPosition = {
      height: activeBounds.height,
      horizontal,
      visible: true,
      width: horizontal ? activeBounds.width : 0,
      x: horizontal ? activeBounds.left - navBounds.left + nav.scrollLeft : 0,
      y: activeBounds.top - navBounds.top + nav.scrollTop,
    };
    setNavIndicator((current) => (
      current.height === nextPosition.height
      && current.horizontal === nextPosition.horizontal
      && current.visible === nextPosition.visible
      && current.width === nextPosition.width
      && current.x === nextPosition.x
      && current.y === nextPosition.y
        ? current
        : nextPosition
    ));
  }, []);

  useLayoutEffect(() => {
    positionNavIndicator();
    const nav = navRef.current;
    const resizeObserver = nav && typeof ResizeObserver !== "undefined"
      ? new ResizeObserver(positionNavIndicator)
      : null;
    if (nav) {
      resizeObserver?.observe(nav);
      // Horizontal bars keep the nav size while link labels animate, so watch the links too.
      nav.querySelectorAll(".nav-link").forEach((link) => resizeObserver?.observe(link));
    }
    window.addEventListener("resize", positionNavIndicator);
    return () => {
      resizeObserver?.disconnect();
      window.removeEventListener("resize", positionNavIndicator);
    };
  }, [
    checkingSession,
    displayedPage,
    navLayoutKey,
    positionNavIndicator,
    projectSelectionRequired,
    projectTokenRequired,
    route,
    sidebarCollapsed,
    sidebarPosition,
    t,
  ]);

  useEffect(() => {
    if (!navIndicator.visible || navIndicatorReady) return undefined;
    const timer = window.setTimeout(() => setNavIndicatorReady(true), 0);
    return () => window.clearTimeout(timer);
  }, [navIndicator.visible, navIndicatorReady]);

  useEffect(() => {
    if (
      route === "cabinet"
      && actor
      && !checkingSession
      && (!projectId || actor.project_id === projectId)
      && !projectSelectionRequired
      && !projectTokenRequired
      && displayedPage
      && (displayedPage !== page || !pagePathKnown || (!projectId && actor.project_id))
    ) {
      onPageReplace(displayedPage, actor.project_id);
    }
  }, [actor, checkingSession, displayedPage, onPageReplace, page, pagePathKnown, projectId, projectSelectionRequired, projectTokenRequired, route]);

  async function completeLogin(
    nextAuth: OperatorAuth,
    currentActor?: ActorContext,
    forceProjectSelection = false,
  ) {
    nextAuth = projectId ? { ...nextAuth, projectId } : nextAuth;
    const restoredActor = currentActor && (!projectId || currentActor.project_id === projectId)
      ? currentActor : await getCurrentActor(nextAuth);
    if (projectId && restoredActor.project_id !== projectId) throw new Error("Project unavailable.");
    const resolvedActor = await actorForPage(authForProject(nextAuth, restoredActor), restoredActor);
    const resolvedAuth = authForProject(nextAuth, resolvedActor);
    forceProjectSelection = false;
    const needsProjectSelection = (nextAuth.kind === "session"
      && ((!resolvedActor.is_demo && forceProjectSelection) || !resolvedActor.project_id))
      || (resolvedActor.access_token_project_scope === "all" && forceProjectSelection);
    const needsProjectToken = nextAuth.kind === "access_token"
      && !resolvedActor.project_id;
    const visibleInboxes = needsProjectSelection || needsProjectToken
      ? []
      : await loadActorInboxes(resolvedAuth, resolvedActor);
    loadedProjectRef.current = resolvedActor.project_id ?? null;
    const workspaceAuth = needsProjectSelection ? nextAuth : resolvedAuth;
    setAuth(resolvedActor.is_demo ? { ...workspaceAuth, isDemo: true } : workspaceAuth);
    rememberTokenProject(resolvedAuth);
    setActor(resolvedActor);
    setProjectSelectionRequired(needsProjectSelection);
    setProjectTokenRequired(needsProjectToken);
    setInboxes(visibleInboxes);
    setActiveInboxId(activeInbox(
      visibleInboxes,
      conversationLinkInboxId(window.location.pathname),
      storedValue(INBOX_KEY),
    ));
  }

  async function handlePasswordLogin(email: string, password: string) {
    setLoginLoading(true);
    setSessionError(null);
    try {
      const currentActor = await loginWithPassword(email, password);
      sessionStorage.removeItem(TOKEN_KEY);
      await completeLogin({ kind: "session" }, currentActor, true);
      setLoginPassword("");
    } catch (cause) {
      setSessionError({ cause, fallback: "login.error" });
    } finally {
      setLoginLoading(false);
    }
  }

  async function handleTryDemo() {
    if (demoLoading) return;
    setDemoLoading(true);
    setSessionError(null);
    onNavigate("cabinet");
    try {
      const credentials = await getDemoCredentials();
      setLoginEmail(credentials.email);
      setLoginPassword(credentials.password);
    } catch (cause) {
      setSessionError({ cause, fallback: "login.demoError" });
    } finally {
      setDemoLoading(false);
    }
  }

  async function handleTokenLogin(token: string) {
    setLoginLoading(true);
    setSessionError(null);
    try {
      const nextAuth: OperatorAuth = { kind: "access_token", token };
      await completeLogin(nextAuth, undefined, true);
      sessionStorage.setItem(TOKEN_KEY, token);
    } catch (cause) {
      sessionStorage.removeItem(TOKEN_KEY);
      sessionStorage.removeItem(TOKEN_PROJECT_KEY);
      setSessionError({ cause, fallback: "login.tokenError" });
    } finally {
      setLoginLoading(false);
    }
  }

  async function applyWorkspaceActor(currentActor: ActorContext, workspaceAuth: OperatorAuth | null, revision: number) {
    if (!workspaceAuth || revision !== workspaceRevisionRef.current) return false;
    workspaceAuth = authForProject(workspaceAuth, currentActor);
    const visibleInboxes = await loadActorInboxes(workspaceAuth, currentActor);
    if (revision !== workspaceRevisionRef.current) return false;
    loadedProjectRef.current = currentActor.project_id ?? null;
    setActor(currentActor);
    setAuth({ ...workspaceAuth });
    setRealtimeEvent(null);
    setNotificationToast(null);
    setInboxes(visibleInboxes);
    setActiveInboxId(activeInbox(visibleInboxes, storedValue(INBOX_KEY)));
    setProjectSelectionRequired(false);
    setProjectTokenRequired(false);
    rememberTokenProject(workspaceAuth);
    return true;
  }

  async function changeProject(projectId: string) {
    if (!auth || (auth.kind !== "session" && actor?.access_token_project_scope !== "all")) throw new Error("Project selection is unavailable.");
    if (workspaceChanging || !onWorkspaceLeave()) return;
    if (projectId === "all-projects") {
      if (canSelectAllTaskProjects) {
        await onPageSegmentsNavigate("tasks", ["all-projects", ...taskViewSegments]);
        setMenuOpen(false);
      }
      return;
    }
    const revision = ++workspaceRevisionRef.current;
    setWorkspaceChanging(true);
    setWorkspaceError(false);
    try {
      const nextAuth: OperatorAuth = { ...auth, projectId };
      const nextActor = auth.kind === "session" ? await selectProject(auth, projectId) : await getCurrentActor(nextAuth);
      if (!await applyWorkspaceActor(nextActor, nextAuth, revision)) return;
      onPageNavigate(displayedPage === "tasks" && canOpenTasks(nextActor) ? "tasks" : departmentStartPage(nextActor), projectId,
        displayedPage === "tasks" && canOpenTasks(nextActor) ? taskViewSegments : undefined);
      setMenuOpen(false);
    } finally {
      if (revision === workspaceRevisionRef.current) setWorkspaceChanging(false);
    }
  }

  async function changeDepartment(departmentId: string | null) {
    if (!auth || auth.kind !== "session" || workspaceChanging || !onWorkspaceLeave()) return;
    const revision = ++workspaceRevisionRef.current;
    setWorkspaceChanging(true);
    setWorkspaceError(false);
    try {
      const nextActor = await selectDepartment(auth, departmentId);
      if (!await applyWorkspaceActor(nextActor, auth, revision)) return;
      onPageReplace(departmentStartPage(nextActor));
      setMenuOpen(false);
    } catch {
      if (revision !== workspaceRevisionRef.current) return;
      setWorkspaceError(true);
      setInboxes([]);
      setActiveInboxId(null);
    } finally {
      if (revision === workspaceRevisionRef.current) setWorkspaceChanging(false);
    }
  }

  async function refreshWorkspace() {
    if (!auth) return;
    const revision = workspaceRevisionRef.current;
    setWorkspaceRefreshError(false);
    try {
      const nextActor = await getCurrentActor(auth);
      const visibleInboxes = await loadActorInboxes(auth, nextActor);
      if (revision !== workspaceRevisionRef.current) return;
      setActor(nextActor);
      setInboxes(visibleInboxes);
      setActiveInboxId((current) => activeInbox(visibleInboxes, current));
      setWorkspaceError(false);
      departments.reload();
    } catch {
      if (revision !== workspaceRevisionRef.current) return;
      setWorkspaceRefreshError(true);
    }
  }

  function selectInbox(inboxId: string) {
    setActiveInboxId(inboxId);
    storeValue(INBOX_KEY, inboxId);
  }

  function openNotificationConversation(toast: OperatorToast) {
    const inboxId = toast.inboxId ?? activeInboxId;
    if (!toast.conversationId || !inboxId) return;
    openConversation(toast.conversationId, inboxId);
    setNotificationToast(null);
  }

  // Conversations switch to the linked inbox themselves, so every way into a conversation link behaves alike.
  function openConversation(conversationId: string, inboxId: string) {
    if (onPageNavigate("conversations", undefined, conversationSegments(inboxId, conversationId))) setMenuOpen(false);
  }

  async function changeNotifications(enabled: boolean) {
    if (!enabled) {
      setNotificationsEnabled(false);
      storeValue(NOTIFICATIONS_KEY, "disabled");
      return;
    }
    if (notificationsChangePendingRef.current) return;
    notificationsChangePendingRef.current = true;
    setNotificationsChanging(true);
    let permission: BrowserNotificationPermission;
    try {
      permission = await enableOperatorNotifications();
    } catch {
      permission = currentBrowserNotificationPermission();
    } finally {
      notificationsChangePendingRef.current = false;
      setNotificationsChanging(false);
    }
    setNotificationPermission(permission);
    const granted = permission === "granted";
    setNotificationsEnabled(granted);
    storeValue(NOTIFICATIONS_KEY, granted ? "enabled" : "disabled");
  }



  const horizontalSidebar = isHorizontalSidebar(sidebarPosition);
  const SidebarToggleIcon = sidebarCollapsed
    ? SIDEBAR_TOGGLE_ICONS[sidebarPosition].expand
    : SIDEBAR_TOGGLE_ICONS[sidebarPosition].collapse;

  function toggleSidebar() {
    setSidebarCollapsed((current) => {
      const next = !current;
      storeValue(SIDEBAR_KEY, next ? "collapsed" : "expanded");
      return next;
    });
  }

  async function logout() {
    if (auth) {
      try {
        await clearOperatorPresence(auth);
      } catch {
        // A missing final heartbeat expires automatically on the server.
      }
    }
    if (auth?.kind === "session") {
      try {
        await logoutSession();
      } catch {
        // Clear the local authenticated view even if the API is unavailable.
      }
    }
    sessionStorage.removeItem(TOKEN_KEY);
    sessionStorage.removeItem(TOKEN_PROJECT_KEY);
    setAuth(null);
    setActor(null);
    setProjectSelectionRequired(false);
    setProjectTokenRequired(false);
    setInboxes([]);
    setActiveInboxId(null);
    setSessionError(null);
    loadedProjectRef.current = null;
    onPageReplace("conversations", null);
  }

  

  if (checkingSession || (actor && projectId && actor.project_id !== projectId)) {
    return (
      <main className={`connection-page${" connection-page--neutral"}`}>
        <div className="auth-checking" role="status" aria-live="polite">
          
          <span>{t("login.checking")}</span>
        </div>
      </main>
    );
  }

  if (!actor || !auth) {
    return (
      <LoginScreen
        branded={false}
        error={sessionError ? localizedError(sessionError.cause, t, sessionError.fallback) : null}
        loading={loginLoading}
        demoLoading={demoLoading}
        email={loginEmail}
        password={loginPassword}
        onEmailChange={setLoginEmail}
        onPasswordChange={setLoginPassword}
        onTryDemo={() => void handleTryDemo()}
        onBackToLanding={undefined}
        onClearError={() => setSessionError(null)}
        onPasswordLogin={(email, password) => void handlePasswordLogin(email, password)}
        onTokenLogin={(token) => void handleTokenLogin(token)}
      />
    );
  }

  if (projectSelectionRequired && (auth.kind === "session" || actor.access_token_project_scope === "all")) {
    return (
      <ProjectSelectionScreen
        auth={auth}
        branded={false}
        headerActions={null}
        onProjectChange={changeProject}
        onSignOut={logout}
      />
    );
  }

  if (projectTokenRequired && auth.kind === "access_token") {
    return (
      <ProjectScopedTokenRequiredScreen
        branded={false}
        headerActions={null}
        onDisconnect={logout}
      />
    );
  }

  function renderProjectWorkspace(scope?: ProjectWorkspaceScope) {
    if (!actor || !auth) return null;
    return renderScopedWorkspace(scope ?? {
      actor, auth, inboxes, activeInboxId, realtimeEvent, realtimeSyncRevision,
      selectInbox, refresh: refreshWorkspace, department: activeDepartment,
      navigate: onPageNavigate, changeDepartment,
      dirtyChange: (name, dirty) => ({ routing: onRoutingDirtyChange, templates: onReplyTemplatesDirtyChange,
        knowledge: onKnowledgeBaseDirtyChange, notes: onNotesDirtyChange })[name](dirty),
    }, displayedPage);
  }

  function renderScopedWorkspace(scope: ProjectWorkspaceScope, displayedPage: Page | undefined) {
    const { actor, auth, inboxes, activeInboxId, realtimeEvent, realtimeSyncRevision, selectInbox,
      refresh: refreshWorkspace, department: activeDepartment, navigate: onPageNavigate } = scope;
    const openConversation = (conversationId: string, inboxId: string) => {
      onPageNavigate("conversations", actor.project_id, conversationSegments(inboxId, conversationId));
    };
    const onRoutingDirtyChange = (dirty: boolean) => scope.dirtyChange("routing", dirty);
    const onReplyTemplatesDirtyChange = (dirty: boolean) => scope.dirtyChange("templates", dirty);
    const onKnowledgeBaseDirtyChange = (dirty: boolean) => scope.dirtyChange("knowledge", dirty);
    const onNotesDirtyChange = (dirty: boolean) => scope.dirtyChange("notes", dirty);
    const canReadContacts = actor.permissions.includes("contacts:read");
    const canReadOnlineVisitors = actor.permissions.includes("visitor_network:read");
    const canReadChannels = actor.permissions.includes("channels:read");
    const canManageChannels = actor.permissions.includes("channels:manage");
    const canReadQuality = actor.permissions.includes("quality:read");
    const canManageRouting = actor.permissions.includes("routing:manage");
    const canManageAi = actor.permissions.includes("ai:manage");
    const canManageTasks = canOpenTasks(actor) && (actor.auth_method === "session" || hasProjectWorkspaceAccess(actor));
    const canManageKnowledge = actor.permissions.includes("knowledge:manage");
    const canManageReplyTemplates = actor.permissions.includes("reply_templates:manage");
    const canReadReplyTemplates = canManageReplyTemplates || actor.permissions.includes("conversations:reply");
    const canManageIntegrations = actor.auth_method === "session"
      && Boolean(actor.project_id)
      && (actor.inbox_scope === null || Boolean(actor.department_id))
      && actor.permissions.includes("integrations:manage");
    const canAdminister = actor.role === "admin";
    const canSendAsAi = actor.auth_method === "session" && canAdminister;
    const canSuperviseOperators = canSendAsAi
      && !actor.department_restricted
      && actor.permissions.includes("conversations:reply");
    const canManageAccessTokens = actor.auth_method === "session" && actor.permissions.includes("access_tokens:manage");
    const canManageRoles = actor.auth_method === "session" && actor.permissions.includes("roles:manage");
    return <ResourceVisibilityPermissionContext.Provider value={actor.role === "admin" || actor.is_director === true}>
          {!displayedPage && <div className="page department-workspace-message">{t("departments.noModules")}</div>}
          {displayedPage === "conversations" && <ConversationsView key={activeInboxId ?? "none"} auth={auth} isDemo={actor.is_demo} canSendAsAi={!actor.is_demo && canSendAsAi} canSuperviseOperators={!actor.is_demo && canSuperviseOperators} canManageContacts={actor.permissions.includes("contacts:manage")} canReplyConversations={actor.permissions.includes("conversations:reply")} canCloseConversations={actor.permissions.includes("conversations:close")} inboxes={inboxes} availableChannels={scope.channels?.filter((channel) => channel.inbox_id === activeInboxId)} activeInboxId={activeInboxId} realtimeEvent={realtimeEvent} realtimeSyncRevision={realtimeSyncRevision} notificationsChanging={notificationsChanging} notificationsEnabled={notificationsEnabled} notificationPermission={notificationPermission} onNotificationsChange={changeNotifications} onInboxChange={selectInbox} />}
          {displayedPage === "contacts" && canReadContacts && <ContactsView auth={auth} canManage={actor.permissions.includes("contacts:manage")} />}
          {displayedPage === "online_visitors" && canReadOnlineVisitors && (
            <OnlineVisitorsView
              auth={auth}
              inboxes={inboxes}
              activeInboxId={activeInboxId}
              realtimeEvent={realtimeEvent}
              onInboxChange={selectInbox}
              canStartConversation={actor.permissions.includes("conversations:read") && actor.permissions.includes("conversations:reply")}
              onOpenConversation={openConversation}
            />
          )}
          {displayedPage === "support_quality" && canReadQuality && (
            <SupportQualityView
              auth={auth}
              inboxes={inboxes}
              onOpenConversation={openConversation}
            />
          )}
          {displayedPage === "inbox_routing" && canManageRouting && (
            <InboxRoutingView auth={auth} inboxes={inboxes} onDirtyChange={onRoutingDirtyChange} />
          )}
          {displayedPage === "channels" && canReadChannels && <ChannelsView auth={auth} inboxes={inboxes} canManage={canManageChannels} showDefaultChannels={activeDepartment?.show_default_channels} />}
          {displayedPage === "notes" && !actor.is_demo && actor.project_id && actor.permissions.includes("notes:read") && <NotesView auth={auth} projectId={actor.project_id} canWrite={actor.permissions.includes("notes:write")} onDirtyChange={onNotesDirtyChange} />}
          {displayedPage === "knowledge_base" && canManageKnowledge && actor.project_id && <KnowledgeBaseView auth={auth} projectId={actor.project_id} onDirtyChange={onKnowledgeBaseDirtyChange} />}
          {displayedPage === "reply_templates" && canReadReplyTemplates && <ReplyTemplatesView key={actor.project_id} auth={auth} inboxes={inboxes} activeInboxId={activeInboxId} onInboxChange={selectInbox} canManage={canManageReplyTemplates} onDirtyChange={onReplyTemplatesDirtyChange} />}
          {displayedPage === "tasks" && canManageTasks && <AITasksView key={actor.project_id} auth={auth} inboxes={inboxes} access={taskAccess(actor)} />}
          {displayedPage === "ai" && canManageAi && <AISettingsView key={actor.project_id} auth={auth} inboxes={inboxes} canManageApi={hasProjectWorkspaceAccess(actor)} />}
          {displayedPage === "integrations" && canManageIntegrations && (
            <IntegrationsView auth={auth} canManageAi={canManageAi} />
          )}
          {displayedPage === "access_tokens" && canManageAccessTokens && actor.project_id && <AccessTokensView key={actor.project_id} auth={auth} projectId={actor.project_id} />}
          {displayedPage === "users" && canManageProjectUsers(actor) && actor.project_id && (
            <UsersView
              key={actor.project_id}
              auth={auth}
              actorId={actor.actor_id}
              projectId={actor.project_id}
              onChanged={() => void refreshWorkspace()}
              onManageRoles={() => onPageNavigate("roles")}
            />
          )}
          {displayedPage === "roles" && canManageRoles && <RolesView auth={auth} />}
    </ResourceVisibilityPermissionContext.Provider>;
  }

  const actorChatDisplayName = actor.chat_display_name
    ?? actor.display_name
    ?? roleLabel(t, actor.role);
  const activeInboxName = inboxes.find((inbox) => inbox.id === activeInboxId)?.name ?? t("nav.noInbox");

  function openDefaultWorkspace() {
    if (!actor || workspaceChanging) return;
    
    if (onPageNavigate("conversations")) setMenuOpen(false);
  }

  function navigateFromPageLink(event: MouseEvent<HTMLAnchorElement>, nextPage: Page) {
    if (
      event.defaultPrevented
      || event.button !== 0
      || event.metaKey
      || event.ctrlKey
      || event.shiftKey
      || event.altKey
    ) {
      return;
    }
    event.preventDefault();
    if (!onPageNavigate(nextPage)) return;
    setMenuOpen(false);
  }

  return (
    <DemoReadOnlyContext.Provider value={actor.is_demo === true}><div className="shell" data-sidebar-position={sidebarPosition}>
      {menuOpen && <button className="backdrop" type="button" aria-label={t("nav.closeMenu")} onClick={closeMobileMenu} />}
      <aside className={`sidebar${menuOpen ? " sidebar--open" : ""}${sidebarCollapsed ? (horizontalSidebar ? " sidebar--compact" : " sidebar--collapsed") : ""}`} id="admin-sidebar" ref={sidebarRef}>
        <div className="sidebar-brand">
          <button type="button" className="workspace-home-button" aria-label={t("departments.workspace")}
            disabled={workspaceChanging || (false)}
            onClick={openDefaultWorkspace}>
          <span className="brandless-workspace-mark" aria-hidden="true"><Inbox size={20} /></span>
          </button>
          <div className="sidebar-brand-actions">
            <button
              className="sidebar-collapse-toggle"
              type="button"
              aria-controls="admin-sidebar"
              aria-expanded={!sidebarCollapsed}
              aria-label={t(sidebarCollapsed ? "nav.expandSidebar" : "nav.collapseSidebar")}
              title={t(sidebarCollapsed ? "nav.expandSidebar" : "nav.collapseSidebar")}
              onClick={toggleSidebar}
            >
              <SidebarToggleIcon size={18} />
            </button>
            <button className="mobile-close" type="button" aria-label={t("nav.closeMenu")} ref={mobileCloseButtonRef} onClick={closeMobileMenu}><X size={22} /></button>
          </div>
        </div>
        {actor.project_id && !actor.is_demo && (
          <div className="department-context">
            <div className="department-project-row">
              <FolderKanban size={17} aria-hidden="true" />
              {actor.auth_method === "session" || actor.access_token_project_scope === "all" ? (
                <ProjectSwitcher
                  key={actor.project_id}
                  auth={auth}
                  projectId={actor.project_id}
                  projectName={actor.project_name ?? t("nav.projects")}
                  disabled={workspaceChanging}
                  allowAllProjects={canSelectAllTaskProjects}
                  allProjects={allTaskProjects}
                  onChange={changeProject}
                  onProjectsLoaded={(items) => setWorkspaceProjects({ auth, items })}
                />
              ) : <strong>{actor.project_name ?? t("nav.projects")}</strong>}
            </div>
            
            
          </div>
        )}
        {actor.auth_method === "session" ? (
          <button
            className="workspace-context workspace-context--editable"
            type="button"
            aria-label={t("profile.edit")}
            title={sidebarCollapsed ? actorChatDisplayName : undefined}
            onClick={() => setProfileOpen(true)}
          >
            <OperatorAvatar displayName={actorChatDisplayName} avatarUrl={actor.avatar_url} />
            <div><strong>{actorChatDisplayName}</strong><span>{activeInboxName} · {actor.role_name ?? roleLabel(t, actor.role)}</span></div>
          </button>
        ) : (
          <div className="workspace-context" title={sidebarCollapsed ? activeInboxName : undefined}>
            <Inbox size={16} />
            <div><strong>{activeInboxName}</strong><span>{actor.role_name ?? roleLabel(t, actor.role)}</span></div>
          </div>
        )}
        <nav className="nav" aria-label={t("nav.main")} ref={navRef}>
          <span
            className={`nav-active-indicator${navIndicator.visible ? " nav-active-indicator--visible" : ""}${navIndicatorReady ? " nav-active-indicator--ready" : ""}`}
            aria-hidden="true"
            style={navIndicator.horizontal ? {
              width: `${navIndicator.width}px`,
              height: `${navIndicator.height}px`,
              transform: `translate3d(${navIndicator.x}px, ${navIndicator.y}px, 0)`,
            } : {
              height: `${navIndicator.height}px`,
              transform: `translate3d(0, ${navIndicator.y}px, 0)`,
            }}
          />
          {visibleNavGroups.map((group) => (
            <section className="nav-group" aria-labelledby={`nav-group-${group.id}`} key={group.id}>
              <h2 className="nav-group-title" id={`nav-group-${group.id}`}>{group.name}</h2>
              {group.items.map((item) => {
                const Icon = item.icon;
                return (
                  <div className="nav-item-row" key={item.id}>
                    <a
                        className={`nav-link ${displayedPage === item.id ? "nav-link--active" : ""}`}
                      href={adminPagePath(item.id, actor.project_id)}
                      aria-label={t(item.label)}
                        aria-current={displayedPage === item.id ? "page" : undefined}
                      title={sidebarCollapsed ? t(item.label) : undefined}
                      onClick={(event) => navigateFromPageLink(event, item.id)}
                    >
                      <Icon size={17} strokeWidth={1.8} />
                      <span>{t(item.label)}</span>
                    </a>
                    </div>
                );
              })}
            </section>
          ))}
        </nav>
        <SidebarFooter authKind={auth.kind} onLogout={() => void logout()} />
      </aside>
      <div className="workspace">
        <header className="mobile-topbar"><button className="menu-button" type="button" aria-label={t("nav.openMenu")} aria-expanded={menuOpen} aria-controls="admin-sidebar" ref={mobileMenuButtonRef} onClick={() => setMenuOpen(true)}><Menu /></button><button type="button" className="workspace-home-button" aria-label={t("departments.workspace")} disabled={workspaceChanging || (false)} onClick={openDefaultWorkspace}><span className="brandless-workspace-mark brandless-workspace-mark--mobile" aria-hidden="true"><Inbox size={18} /></span></button></header>
        {!workspaceError && !(departments.error && !departments.data) && !workspaceChanging && !departments.loading && (workspaceRefreshError || Boolean(departments.error)) && (
          <div className="workspace-refresh-notice" data-page-width={pageWidth}>
            <div className="admin-notice admin-notice--error" role="alert">
              <span>{t("departments.loadError")}</span>
              <button type="button" className="text-button" onClick={() => void refreshWorkspace()}>{t("departments.retry")}</button>
            </div>
          </div>
        )}
        <main className="content" data-page-width={pageWidth} aria-busy={workspaceChanging || departments.loading}>
          {workspaceError || (departments.error && !departments.data) ? (
            <div className="page department-workspace-message" role="alert">
              <p>{t("departments.loadError")}</p>
              <button type="button" className="secondary-button" onClick={() => void refreshWorkspace()}>{t("departments.retry")}</button>
            </div>
          ) : workspaceChanging || departments.loading ? (
            <div className="page department-workspace-message" role="status">{t("departments.loading")}</div>
          ) : <ResourceVisibilityPermissionContext.Provider key={`${actor.project_id ?? ""}:${actor.department_id ?? ""}:${actor.is_director ?? false}`} value={actor.role === "admin" || actor.is_director === true}><PageRouteContext.Provider value={pageRoute}>
          <ParentProjectWorkspace
            key={`${actor.project_id}:${displayedPage}`}
            enabled={false}
            auth={auth} actor={actor} page={displayedPage}
            projects={workspaceProjects?.auth === auth ? workspaceProjects.items : []}
            canShowPage={(child, department) => departmentNavGroups(adminNavGroups(child), department).some((group) => group.items.some((item) => item.id === displayedPage))}
            onNavigate={onPageNavigate} onBeforeChange={onWorkspaceLeave}
            onDepartmentSelect={async (scopeProjectId, departmentId) => {
              if (scopeProjectId === actor.project_id) return changeDepartment(departmentId);
              if (!onWorkspaceLeave()) return;
              const scopedActor = await selectDepartment({ ...auth, projectId: scopeProjectId }, departmentId);
              onPageNavigate(departmentStartPage(scopedActor), scopeProjectId);
            }}
            onDirtyChange={(name, dirty) => ({ routing: onRoutingDirtyChange, templates: onReplyTemplatesDirtyChange,
              knowledge: onKnowledgeBaseDirtyChange, notes: onNotesDirtyChange })[name](dirty)}
            renderWorkspace={renderProjectWorkspace}
          />
          </PageRouteContext.Provider></ResourceVisibilityPermissionContext.Provider>}
        </main>
      </div>
      {notificationToast && (
        <aside className="operator-notification" role="status" aria-live="polite">
          <div className="operator-notification-content">
            <strong>{notificationToast.title}</strong>
            <span>{notificationToast.body}</span>
            {notificationToast.conversationId && (
              <button
                className="operator-notification-action"
                type="button"
                onClick={() => openNotificationConversation(notificationToast)}
              >
                <MessageSquare size={14} />
                {t("notifications.openChat")}
              </button>
            )}
          </div>
          <button className="operator-notification-dismiss" type="button" aria-label={t("notifications.dismiss")} onClick={() => setNotificationToast(null)}>
            <X size={16} />
          </button>
        </aside>
      )}
      {profileOpen && actor.auth_method === "session" && (
        <OperatorProfileDialog
          auth={auth}
          operatorId={actor.actor_id}
          displayName={actorChatDisplayName}
          avatarUrl={actor.avatar_url}
          mode="self"
          onClose={() => setProfileOpen(false)}
          onSaved={(saved) => {
            setActor((current) => current === null ? current : {
              ...current,
              chat_display_name: saved.display_name,
              avatar_url: saved.avatar_url,
            });
            setProfileOpen(false);
          }}
        />
      )}
    </div></DemoReadOnlyContext.Provider>
  );
}
