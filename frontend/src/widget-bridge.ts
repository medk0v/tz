import type { components } from "./api-schema";
import { productNamespace } from "./product-edition";
import { isWidgetFontFamily, isWidgetSiteFont, type WidgetSiteFont } from "./widget-font";
import {
  DEFAULT_WIDGET_THEME,
  type WidgetColorScheme,
} from "./widget-theme";

export type WidgetBootstrapSession = components["schemas"]["WidgetSession"];
export type WidgetLauncher = components["schemas"]["WidgetLauncher"];
export type WidgetClientContext = components["schemas"]["ClientContext"];

export const widgetMessageTypes = {
  ready: `${productNamespace}:widget-ready`,
  resize: `${productNamespace}:widget-resize`,
  close: `${productNamespace}:widget-close`,
  theme: `${productNamespace}:widget-theme`,
  siteFontRequest: `${productNamespace}:widget-site-font-request`,
  siteFont: `${productNamespace}:widget-site-font`,
  bootstrap: `${productNamespace}:widget-bootstrap`,
  bootstrapError: `${productNamespace}:widget-bootstrap-error`,
} as const;

export interface WidgetReadyMessage {
  type: typeof widgetMessageTypes.ready;
  version: 1;
}

export interface WidgetResizeMessage {
  type: typeof widgetMessageTypes.resize;
  version: 1;
  open: boolean;
  call_to_action_visible?: boolean;
  launcher?: WidgetLauncher;
}

export interface WidgetCloseMessage {
  type: typeof widgetMessageTypes.close;
  version: 1;
}

export interface WidgetThemeMessage {
  type: typeof widgetMessageTypes.theme;
  version: 1;
  theme: WidgetColorScheme;
}

export interface WidgetSiteFontRequestMessage {
  type: typeof widgetMessageTypes.siteFontRequest;
  version: 1;
}

export interface WidgetSiteFontMessage {
  type: typeof widgetMessageTypes.siteFont;
  version: 1;
  font: WidgetSiteFont | null;
}

export interface WidgetBootstrapMessage {
  type: typeof widgetMessageTypes.bootstrap;
  version: 1;
  embed_origin: string;
  session: WidgetBootstrapSession;
}

export interface WidgetBootstrapErrorMessage {
  type: typeof widgetMessageTypes.bootstrapError;
  version: 1;
  message: string;
}

// Names nothing about the product: the visitor sees this key in their own browser.
const VISITOR_STORAGE_PREFIX = "widget-visitor";
const LEGACY_WIDGET_BORDER_RADIUS = 20;
const LEGACY_WIDGET_LAUNCHER_ANIMATION = "pulse";
const LEGACY_WIDGET_LAUNCHER_INTERVAL_SECONDS = 5;
const DEFAULT_WIDGET_SUPPORT_NAME = "Support";
const LEGACY_WIDGET_SHOW_GREETING = true;
const LEGACY_WIDGET_SHOW_OPERATOR_PROFILE = false;
const LEGACY_WIDGET_PROACTIVE_INVITATION = false;
const LEGACY_WIDGET_PROACTIVE_INVITATION_DELAY_SECONDS = 15;
const HEX_COLOR_PATTERN = /^#[0-9a-f]{6}$/i;
const UUID_V4_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const ATTRIBUTION_KEYS = [
  "utm_source",
  "utm_medium",
  "utm_campaign",
  "utm_term",
  "utm_content",
  "gclid",
  "fbclid",
  "msclkid",
] as const;
const EFFECTIVE_TYPES = ["slow-2g", "2g", "3g", "4g"] as const;

type AttributionKey = (typeof ATTRIBUTION_KEYS)[number];
type EffectiveType = (typeof EFFECTIVE_TYPES)[number];

interface UserAgentDataLike {
  brands?: ReadonlyArray<{ brand: string; version: string }>;
  mobile?: boolean;
  platform?: string;
}

interface NetworkInformationLike {
  effectiveType?: string;
  downlink?: number;
  rtt?: number;
  saveData?: boolean;
}

interface ContextNavigator extends Navigator {
  userAgentData?: UserAgentDataLike;
  deviceMemory?: number;
  connection?: NetworkInformationLike;
  globalPrivacyControl?: boolean;
}

function truncated(value: string, maxLength: number): string {
  return value.slice(0, maxLength);
}

function optionalString(value: unknown, maxLength: number): string | undefined {
  return typeof value === "string" && value.trim()
    ? truncated(value.trim(), maxLength)
    : undefined;
}

function optionalNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && value >= 0
    ? value
    : undefined;
}

function isEffectiveType(value: string): value is EffectiveType {
  return EFFECTIVE_TYPES.some((candidate) => candidate === value);
}

function boundedNumber(
  value: unknown,
  minimum: number,
  maximum: number,
): number | undefined {
  const number = optionalNumber(value);
  return number !== undefined && number >= minimum && number <= maximum
    ? number
    : undefined;
}

function boundedInteger(
  value: unknown,
  minimum: number,
  maximum: number,
): number | undefined {
  const number = boundedNumber(value, minimum, maximum);
  return number === undefined ? undefined : Math.round(number);
}

function publicPageUrl(location: Location): string {
  return truncated(`${location.origin}${location.pathname}`, 2_048);
}

function referrerOrigin(referrer: string, baseUrl: string): string | undefined {
  if (!referrer) return undefined;
  try {
    const url = new URL(referrer, baseUrl);
    return url.protocol === "http:" || url.protocol === "https:"
      ? url.origin
      : undefined;
  } catch {
    return undefined;
  }
}

function timezone(): string | undefined {
  try {
    return optionalString(
      Intl.DateTimeFormat().resolvedOptions().timeZone,
      100,
    );
  } catch {
    return undefined;
  }
}

function mediaMatches(sourceWindow: Window, query: string): boolean {
  try {
    return sourceWindow.matchMedia(query).matches;
  } catch {
    return false;
  }
}

function collectAttribution(location: Location): WidgetClientContext["attribution"] {
  const values: Partial<Record<AttributionKey, string>> = {};
  const params = new URL(location.href).searchParams;
  for (const key of ATTRIBUTION_KEYS) {
    const value = optionalString(params.get(key), 256);
    if (value) values[key] = value;
  }
  return Object.keys(values).length > 0 ? values : undefined;
}

export function collectWidgetClientContext(
  sourceWindow: Window = window,
  sourceDocument: Document = document,
): WidgetClientContext {
  const navigator = sourceWindow.navigator as ContextNavigator;
  const userAgentData = navigator.userAgentData;
  const language = optionalString(navigator.language, 100);
  const languages = Array.from(navigator.languages ?? [])
    .map((value) => optionalString(value, 100))
    .filter((value): value is string => value !== undefined)
    .slice(0, 16);
  const browser: WidgetClientContext["browser"] = {
    brands: [],
    cookie_enabled: navigator.cookieEnabled,
  };
  const platform = optionalString(
    userAgentData?.platform ?? navigator.platform,
    100,
  );
  if (platform) browser.platform = platform;
  if (userAgentData?.brands?.length) {
    browser.brands = userAgentData.brands.slice(0, 16).flatMap((item) => {
      const brand = optionalString(item.brand, 100);
      const version = optionalString(item.version, 50);
      return brand && version ? [{ brand, version }] : [];
    });
  }
  if (typeof userAgentData?.mobile === "boolean") {
    browser.mobile = userAgentData.mobile;
  }
  const doNotTrack = optionalString(navigator.doNotTrack, 20);
  if (doNotTrack) browser.do_not_track = doNotTrack;
  if (typeof navigator.globalPrivacyControl === "boolean") {
    browser.global_privacy_control = navigator.globalPrivacyControl;
  }

  const device: NonNullable<WidgetClientContext["device"]> = {};
  const memory = boundedNumber(navigator.deviceMemory, 0, 4_096);
  if (memory !== undefined) device.memory_gb = memory;
  const processors = boundedInteger(navigator.hardwareConcurrency, 1, 4_096);
  if (processors !== undefined) device.logical_processors = processors;
  const touchPoints = boundedInteger(navigator.maxTouchPoints, 0, 1_024);
  if (touchPoints !== undefined) device.max_touch_points = touchPoints;

  const connection: NonNullable<WidgetClientContext["connection"]> = {};
  const effectiveType = optionalString(navigator.connection?.effectiveType, 30);
  if (effectiveType && isEffectiveType(effectiveType)) {
    connection.effective_type = effectiveType;
  }
  const downlink = boundedNumber(navigator.connection?.downlink, 0, 100_000);
  if (downlink !== undefined) connection.downlink_mbps = downlink;
  const rtt = boundedInteger(navigator.connection?.rtt, 0, 3_600_000);
  if (rtt !== undefined) connection.rtt_ms = rtt;
  if (typeof navigator.connection?.saveData === "boolean") {
    connection.save_data = navigator.connection.saveData;
  }

  const locale: WidgetClientContext["locale"] = { languages };
  if (language) locale.language = language;
  const resolvedTimezone = timezone();
  if (resolvedTimezone) locale.timezone = resolvedTimezone;

  const context: WidgetClientContext = {
    schema_version: 1,
    captured_at: new Date().toISOString(),
    page: {
      url: publicPageUrl(sourceWindow.location),
      title: truncated(sourceDocument.title, 500),
    },
    locale,
    display: {
      viewport_width: boundedInteger(sourceWindow.innerWidth, 1, 100_000) ?? 1,
      viewport_height: boundedInteger(sourceWindow.innerHeight, 1, 100_000) ?? 1,
      screen_width: boundedInteger(sourceWindow.screen.width, 1, 100_000) ?? 1,
      screen_height: boundedInteger(sourceWindow.screen.height, 1, 100_000) ?? 1,
    },
    browser,
    preferences: {
      color_scheme: mediaMatches(sourceWindow, "(prefers-color-scheme: dark)")
        ? "dark"
        : "light",
      reduced_motion: mediaMatches(sourceWindow, "(prefers-reduced-motion: reduce)"),
    },
  };
  const devicePixelRatio = boundedNumber(sourceWindow.devicePixelRatio, 0.1, 16);
  if (devicePixelRatio !== undefined) {
    context.display.device_pixel_ratio = devicePixelRatio;
  }
  const colorDepth = boundedInteger(sourceWindow.screen.colorDepth, 1, 128);
  if (colorDepth !== undefined) context.display.color_depth = colorDepth;
  const referrer = referrerOrigin(sourceDocument.referrer, sourceWindow.location.href);
  if (referrer) context.page.referrer = referrer;
  if (Object.keys(device).length > 0) context.device = device;
  if (Object.keys(connection).length > 0) context.connection = connection;
  const attribution = collectAttribution(sourceWindow.location);
  if (attribution) context.attribution = attribution;
  return context;
}

function randomVisitorId(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (value) => value.toString(16).padStart(2, "0"));
  return `${hex.slice(0, 4).join("")}-${hex.slice(4, 6).join("")}-${hex.slice(6, 8).join("")}-${hex.slice(8, 10).join("")}-${hex.slice(10).join("")}`;
}

export function visitorStorageKey(widgetId: string): string {
  return `${VISITOR_STORAGE_PREFIX}:${widgetId}`;
}

export function getOrCreateVisitorId(
  widgetId: string,
  storage?: Storage,
  createId: () => string = randomVisitorId,
): string {
  let generatedId: string | undefined;
  try {
    const target = storage ?? window.localStorage;
    const key = visitorStorageKey(widgetId);
    const existing = target.getItem(key);
    if (existing && UUID_V4_PATTERN.test(existing)) return existing;
    generatedId = createId();
    target.setItem(key, generatedId);
    return generatedId;
  } catch {
    // Storage can be disabled. The valid ephemeral UUID still creates a session.
  }
  return generatedId ?? createId();
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isHexColor(value: unknown): value is string {
  return typeof value === "string" && HEX_COLOR_PATTERN.test(value);
}

function isWidgetComponentColors(value: unknown): boolean {
  return isRecord(value)
    && isHexColor(value.background_color)
    && isHexColor(value.control_background_color)
    && isHexColor(value.control_text_color)
    && isHexColor(value.muted_text_color)
    && isHexColor(value.divider_color)
    && isHexColor(value.online_color)
    && isHexColor(value.offline_color)
    && isHexColor(value.danger_color)
    && isHexColor(value.rating_color)
    && isHexColor(value.shadow_color);
}

function isWidgetDarkTheme(value: unknown): boolean {
  return isRecord(value)
    && isHexColor(value.accent_color)
    && isHexColor(value.accent_text_color)
    && isHexColor(value.surface_color)
    && isHexColor(value.text_color)
    && (value.component_colors === undefined
      || isWidgetComponentColors(value.component_colors));
}

function isWidgetBorder(value: unknown): boolean {
  return isRecord(value)
    && typeof value.enabled === "boolean"
    && Number.isInteger(value.width)
    && (value.width === 1 || value.width === 2)
    && isHexColor(value.light_color)
    && isHexColor(value.dark_color);
}

function isExactOrigin(value: unknown): value is string {
  if (typeof value !== "string") return false;
  try {
    return new URL(value).origin === value;
  } catch {
    return false;
  }
}

export function isWidgetReadyMessage(value: unknown): value is WidgetReadyMessage {
  return isRecord(value)
    && value.type === widgetMessageTypes.ready
    && value.version === 1;
}

export function isWidgetResizeMessage(value: unknown): value is WidgetResizeMessage {
  return isRecord(value)
    && value.type === widgetMessageTypes.resize
    && value.version === 1
    && typeof value.open === "boolean"
    && (value.call_to_action_visible === undefined
      || typeof value.call_to_action_visible === "boolean")
    && (value.launcher === undefined || isWidgetLauncher(value.launcher));
}

export function isWidgetCloseMessage(value: unknown): value is WidgetCloseMessage {
  return isRecord(value)
    && value.type === widgetMessageTypes.close
    && value.version === 1;
}

export function isWidgetThemeMessage(value: unknown): value is WidgetThemeMessage {
  return isRecord(value)
    && value.type === widgetMessageTypes.theme
    && value.version === 1
    && (value.theme === "light" || value.theme === "dark");
}

export function isWidgetSiteFontRequestMessage(value: unknown): value is WidgetSiteFontRequestMessage {
  return isRecord(value) && value.type === widgetMessageTypes.siteFontRequest && value.version === 1;
}

export function isWidgetSiteFontMessage(value: unknown): value is WidgetSiteFontMessage {
  return isRecord(value) && value.type === widgetMessageTypes.siteFont && value.version === 1
    && (value.font === null || isWidgetSiteFont(value.font));
}

function isWidgetLauncher(value: unknown): value is WidgetLauncher {
  const label = isRecord(value) && typeof value.label === "string"
    ? value.label.trim()
    : "";
  return isRecord(value)
    && ["icon", "text", "icon_text"].includes(String(value.launcher_type))
    && ["bottom_right", "bottom_left"].includes(String(value.position))
    && label.length > 0
    && Array.from(label).length <= 40
    && typeof value.show_greeting === "boolean"
    && typeof value.show_operator_profile === "boolean"
    && typeof value.offset_x === "number"
    && Number.isInteger(value.offset_x)
    && value.offset_x >= 0
    && value.offset_x <= 120
    && typeof value.offset_y === "number"
    && Number.isInteger(value.offset_y)
    && value.offset_y >= 0
    && value.offset_y <= 120
    && ["pulse", "lift", "sway", "none"].includes(String(value.attention_animation))
    && typeof value.animation_interval_seconds === "number"
    && Number.isInteger(value.animation_interval_seconds)
    && value.animation_interval_seconds >= 1
    && value.animation_interval_seconds <= 300
    && typeof value.proactive_invitation_enabled === "boolean"
    && typeof value.proactive_invitation_delay_seconds === "number"
    && Number.isInteger(value.proactive_invitation_delay_seconds)
    && value.proactive_invitation_delay_seconds >= 1
    && value.proactive_invitation_delay_seconds <= 300;
}

export function isWidgetBootstrapMessage(value: unknown): value is WidgetBootstrapMessage {
  if (!isRecord(value) || !isRecord(value.session)) return false;
  return value.type === widgetMessageTypes.bootstrap
    && value.version === 1
    && isExactOrigin(value.embed_origin)
    && isWidgetBootstrapSession(value.session);
}

export function isWidgetBootstrapSession(
  value: unknown,
): value is WidgetBootstrapSession {
  if (!isRecord(value)) return false;
  return typeof value.session_id === "string"
    && typeof value.token === "string"
    && typeof value.inbox_id === "string"
    && typeof value.language === "string"
    && value.language.length >= 1
    && value.language.length <= 35
    && typeof value.expires_at === "string"
    && typeof value.operators_online === "boolean"
    && typeof value.support_name === "string"
    && Array.from(value.support_name.trim()).length >= 1
    && Array.from(value.support_name.trim()).length <= 80
    && typeof value.offline_message === "string"
    && Array.from(value.offline_message.trim()).length >= 1
    && Array.from(value.offline_message.trim()).length <= 500
    && typeof value.offline_now === "string"
    && Array.from(value.offline_now.trim()).length >= 1
    && Array.from(value.offline_now.trim()).length <= 80
    && typeof value.proactive_invitation_message === "string"
    && Array.from(value.proactive_invitation_message.trim()).length >= 1
    && Array.from(value.proactive_invitation_message.trim()).length <= 240
    && isRecord(value.theme)
    && (value.theme.font_family === undefined || isWidgetFontFamily(value.theme.font_family))
    && (value.theme.use_site_font === undefined || typeof value.theme.use_site_font === "boolean")
    && (value.theme.reply_typing_effect === undefined || typeof value.theme.reply_typing_effect === "boolean")
    && (value.theme.opening_animation === undefined || typeof value.theme.opening_animation === "boolean")
    && isHexColor(value.theme.accent_color)
    && isHexColor(value.theme.accent_text_color)
    && isHexColor(value.theme.surface_color)
    && isHexColor(value.theme.text_color)
    && (value.theme.component_colors === undefined
      || isWidgetComponentColors(value.theme.component_colors))
    && isWidgetDarkTheme(value.theme.dark)
    && isWidgetBorder(value.theme.border)
    && Number.isInteger(value.theme.border_radius)
    && Number(value.theme.border_radius) >= 0
    && Number(value.theme.border_radius) <= 32
    && isWidgetContactProfile(value.contact)
    && (value.launcher === undefined || isWidgetLauncher(value.launcher));
}

function isWidgetContactProfile(value: unknown): boolean {
  if (!isRecord(value)) return false;
  return (value.display_name === null || typeof value.display_name === "string")
    && (value.email === null || typeof value.email === "string");
}

export function normalizeWidgetBootstrapSession(
  value: unknown,
): WidgetBootstrapSession | null {
  if (!isRecord(value) || !isRecord(value.theme)) return null;
  const normalized = {
    ...value,
    contact: isRecord(value.contact)
      ? value.contact
      : { display_name: null, email: null },
    operators_online: value.operators_online === undefined ? false : value.operators_online,
    support_name: typeof value.support_name === "string" && value.support_name.trim()
      ? value.support_name.trim()
      : isRecord(value.launcher) && typeof value.launcher.support_name === "string" && value.launcher.support_name.trim()
        ? value.launcher.support_name.trim()
        : DEFAULT_WIDGET_SUPPORT_NAME,
    offline_message: value.offline_message
      ?? (String(value.language).split("-", 1)[0].toLowerCase() === "ru"
        ? "Операторы офлайн. Оставьте сообщение и контакты."
        : String(value.language).split("-", 1)[0].toLowerCase() === "ro"
          ? "Operatorii sunt offline. Lăsați un mesaj și datele de contact."
          : "Operators are offline. Leave a message and your contact details."),
    offline_now: value.offline_now
      ?? (String(value.language).split("-", 1)[0].toLowerCase() === "ru"
        ? "Оставьте сообщение"
        : String(value.language).split("-", 1)[0].toLowerCase() === "ro"
          ? "Lăsați un mesaj"
          : String(value.language).split("-", 1)[0].toLowerCase() === "hi"
            ? "संदेश छोड़ें"
            : String(value.language).split("-", 1)[0].toLowerCase() === "zh"
              ? "请留言"
              : "Leave a message"),
    proactive_invitation_message: value.proactive_invitation_message
      ?? (String(value.language).split("-", 1)[0].toLowerCase() === "ru"
        ? "Здравствуйте! Могу помочь?"
        : String(value.language).split("-", 1)[0].toLowerCase() === "ro"
          ? "Bună ziua! Vă putem ajuta?"
          : "Hi! Can I help you?"),
    launcher: isRecord(value.launcher)
      ? {
          ...value.launcher,
          attention_animation: value.launcher.attention_animation
            ?? LEGACY_WIDGET_LAUNCHER_ANIMATION,
          animation_interval_seconds: value.launcher.animation_interval_seconds
            ?? LEGACY_WIDGET_LAUNCHER_INTERVAL_SECONDS,
          show_greeting: value.launcher.show_greeting ?? LEGACY_WIDGET_SHOW_GREETING,
          show_operator_profile: value.launcher.show_operator_profile
            ?? LEGACY_WIDGET_SHOW_OPERATOR_PROFILE,
          proactive_invitation_enabled: value.launcher.proactive_invitation_enabled
            ?? LEGACY_WIDGET_PROACTIVE_INVITATION,
          proactive_invitation_delay_seconds: value.launcher.proactive_invitation_delay_seconds
            ?? LEGACY_WIDGET_PROACTIVE_INVITATION_DELAY_SECONDS,
        }
      : value.launcher,
    theme: {
      ...value.theme,
      reply_typing_effect: value.theme.reply_typing_effect === undefined
        ? DEFAULT_WIDGET_THEME.reply_typing_effect
        : value.theme.reply_typing_effect,
      opening_animation: value.theme.opening_animation === undefined
        ? DEFAULT_WIDGET_THEME.opening_animation
        : value.theme.opening_animation,
      border_radius: value.theme.border_radius === undefined
        ? LEGACY_WIDGET_BORDER_RADIUS
        : value.theme.border_radius,
      dark: value.theme.dark === undefined
        ? { ...DEFAULT_WIDGET_THEME.dark }
        : value.theme.dark,
      border: value.theme.border === undefined
        ? { ...DEFAULT_WIDGET_THEME.border }
        : value.theme.border,
    },
  };
  return isWidgetBootstrapSession(normalized) ? normalized : null;
}

export function isWidgetBootstrapErrorMessage(
  value: unknown,
): value is WidgetBootstrapErrorMessage {
  return isRecord(value)
    && value.type === widgetMessageTypes.bootstrapError
    && value.version === 1
    && typeof value.message === "string";
}
