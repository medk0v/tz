import {
  widgetMessageTypes,
  collectWidgetClientContext,
  getOrCreateVisitorId,
  isWidgetReadyMessage,
  isWidgetSiteFontRequestMessage,
  isWidgetResizeMessage,
  normalizeWidgetBootstrapSession,
  type WidgetBootstrapErrorMessage,
  type WidgetBootstrapMessage,
  type WidgetBootstrapSession,
  type WidgetClientContext,
  type WidgetLauncher,
  type WidgetThemeMessage,
  type WidgetSiteFontMessage,
} from "./widget-bridge";
import type { WidgetLanguage } from "./api";
import { normalizeWidgetLanguage } from "./widget-i18n";
import { productNamespace } from "./product-edition";
import {
  normalizeWidgetColorScheme,
  type WidgetColorScheme,
} from "./widget-theme";
import { collectWidgetSiteFont } from "./widget-font";

const FRAME_ID = `${productNamespace}-customer-widget`;
const WIDGET_LOAD_DELAY_MS = 1_000;
const DEFAULT_LAUNCHER: WidgetLauncher = {
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

interface MountedWidget {
  colorScheme: WidgetColorScheme;
  frameOrigin: string;
  frameUrl: URL;
  iframe: HTMLIFrameElement;
}

let mountedWidget: MountedWidget | null = null;

function closedFrameSize(launcher: WidgetLauncher): { width: string; height: string } {
  if (launcher.launcher_type === "icon") return { width: "86px", height: "86px" };
  return {
    width: launcher.launcher_type === "icon_text" ? "244px" : "204px",
    height: "80px",
  };
}

function setFrameStyle(
  iframe: HTMLIFrameElement,
  property: string,
  value: string,
): void {
  iframe.style.setProperty(property, value, "important");
}

function postWidgetTheme(widget: MountedWidget): void {
  const message: WidgetThemeMessage = {
    type: widgetMessageTypes.theme,
    version: 1,
    theme: widget.colorScheme,
  };
  widget.iframe.contentWindow?.postMessage(message, widget.frameOrigin);
}

export function setWidgetTheme(value: string): boolean {
  const colorScheme = normalizeWidgetColorScheme(value);
  if (!colorScheme || !mountedWidget?.iframe.isConnected) return false;

  mountedWidget.colorScheme = colorScheme;
  mountedWidget.frameUrl.searchParams.set("theme", colorScheme);
  setFrameStyle(mountedWidget.iframe, "color-scheme", colorScheme);
  if (mountedWidget.iframe.hasAttribute("src")) postWidgetTheme(mountedWidget);
  return true;
}

function applyLauncherPosition(iframe: HTMLIFrameElement, launcher: WidgetLauncher): void {
  const offsetX = `${launcher.offset_x}px`;
  setFrameStyle(iframe, "bottom", `${launcher.offset_y}px`);
  if (launcher.position === "bottom_left") {
    setFrameStyle(iframe, "left", offsetX);
    setFrameStyle(iframe, "right", "auto");
  } else {
    setFrameStyle(iframe, "right", offsetX);
    setFrameStyle(iframe, "left", "auto");
  }
}

export interface WidgetLoaderOptions {
  src?: string;
  title?: string;
  widgetId?: string;
  apiBaseUrl?: string;
  language?: string;
  theme?: string;
}

function resolveWidgetLanguage(
  options: WidgetLoaderOptions,
  script: HTMLScriptElement | null,
): WidgetLanguage | undefined {
  const explicit = options.language ?? script?.dataset.language;
  if (explicit !== undefined && explicit.trim() !== "") {
    return normalizeWidgetLanguage(explicit);
  }
  const routeLanguage = window.location.pathname
    .split("/")
    .find((segment) => segment !== "");
  if (routeLanguage && /^[a-z]{2}(?:[-_][a-z0-9]{2,8})?$/i.test(routeLanguage)) {
    return normalizeWidgetLanguage(routeLanguage);
  }
  return normalizeWidgetLanguage(document.documentElement.lang);
}

function resolveWidgetColorScheme(
  options: WidgetLoaderOptions,
  script: HTMLScriptElement | null,
): WidgetColorScheme {
  return normalizeWidgetColorScheme(options.theme ?? script?.dataset.theme) ?? "light";
}

function defaultFrameSource(script: HTMLScriptElement | null): string {
  if (!script?.src) {
    return "http://localhost:5174/widget.html";
  }

  return new URL("../widget/widget.html", script.src).toString();
}

function defaultApiBaseUrl(
  script: HTMLScriptElement | null,
  frameSource: string,
): string {
  return new URL(script?.src ?? frameSource, window.location.href).origin;
}

async function responseError(response: Response): Promise<string> {
  try {
    const value = await response.json() as unknown;
    if (
      typeof value === "object"
      && value !== null
      && "error" in value
      && typeof value.error === "object"
      && value.error !== null
      && "message" in value.error
      && typeof value.error.message === "string"
    ) {
      return value.error.message;
    }
  } catch {
    // The stable public fallback below does not expose an arbitrary response.
  }
  return "This widget is not available on the current site.";
}

async function createSession(
  apiBaseUrl: string,
  widgetId: string,
  visitorId: string,
  language: WidgetLanguage | undefined,
  clientContext: WidgetClientContext,
): Promise<WidgetBootstrapSession> {
  const response = await fetch(new URL("/widget/v1/sessions", apiBaseUrl), {
    method: "POST",
    credentials: "omit",
    referrerPolicy: "no-referrer",
    headers: { "Content-Type": "text/plain;charset=UTF-8" },
    body: JSON.stringify({
      widget_id: widgetId,
      visitor_id: visitorId,
      language: language ?? null,
      client_context: clientContext,
    }),
  });
  if (!response.ok) throw new Error(await responseError(response));
  const session = normalizeWidgetBootstrapSession(await response.json());
  if (!session) {
    throw new Error("The widget service returned an invalid session.");
  }
  return session;
}

export function mountWidget(
  options: WidgetLoaderOptions = {},
): HTMLIFrameElement {
  const existing = document.getElementById(FRAME_ID);
  if (existing instanceof HTMLIFrameElement) {
    return existing;
  }

  const script = document.currentScript;
  const loaderScript =
    script instanceof HTMLScriptElement ? script : null;
  const iframe = document.createElement("iframe");

  iframe.id = FRAME_ID;
  const source =
    options.src ??
    loaderScript?.dataset.src ??
    defaultFrameSource(loaderScript);
  const publicWidgetId = options.widgetId ?? loaderScript?.dataset.widgetId;
  const language = resolveWidgetLanguage(options, loaderScript);
  const colorScheme = resolveWidgetColorScheme(options, loaderScript);
  const apiBaseUrl =
    options.apiBaseUrl
    ?? loaderScript?.dataset.apiBase
    ?? defaultApiBaseUrl(loaderScript, source);
  const frameUrl = new URL(source, window.location.href);
  if (publicWidgetId) frameUrl.searchParams.set("widget_id", publicWidgetId);
  if (language) frameUrl.searchParams.set("lang", language);
  frameUrl.searchParams.set("theme", colorScheme);
  iframe.title =
    options.title ??
    loaderScript?.dataset.title ??
    "Customer communication";
  iframe.referrerPolicy = "no-referrer";
  iframe.setAttribute("allow", "autoplay; clipboard-write");
  setFrameStyle(iframe, "position", "fixed");
  setFrameStyle(iframe, "top", "auto");
  applyLauncherPosition(iframe, DEFAULT_LAUNCHER);
  const initialSize = closedFrameSize(DEFAULT_LAUNCHER);
  setFrameStyle(iframe, "width", initialSize.width);
  setFrameStyle(iframe, "height", initialSize.height);
  setFrameStyle(iframe, "min-width", "0");
  setFrameStyle(iframe, "min-height", "0");
  setFrameStyle(iframe, "max-width", "none");
  setFrameStyle(iframe, "max-height", "none");
  setFrameStyle(iframe, "margin", "0");
  setFrameStyle(iframe, "padding", "0");
  setFrameStyle(iframe, "border", "0");
  setFrameStyle(iframe, "border-radius", "0");
  setFrameStyle(iframe, "box-shadow", "none");
  setFrameStyle(iframe, "transform", "none");
  setFrameStyle(iframe, "background", "transparent");
  setFrameStyle(iframe, "color-scheme", colorScheme);
  setFrameStyle(iframe, "opacity", "0");
  setFrameStyle(iframe, "visibility", "hidden");
  setFrameStyle(iframe, "pointer-events", "none");
  setFrameStyle(iframe, "z-index", "2147483000");
  setFrameStyle(iframe, "will-change", "width, height, opacity");
  setFrameStyle(iframe, "transition", "width 240ms cubic-bezier(.22, .61, .36, 1), height 240ms cubic-bezier(.22, .61, .36, 1), opacity 120ms ease-out");

  const frameOrigin = frameUrl.origin;
  const widget: MountedWidget = {
    colorScheme,
    frameOrigin,
    frameUrl,
    iframe,
  };
  mountedWidget = widget;
  let frameStarted = false;
  let bootstrapped = false;
  let bootstrapPromise: Promise<WidgetBootstrapSession> | null = null;
  window.addEventListener("message", (event: MessageEvent<unknown>) => {
    if (
      !frameStarted
      || event.source !== iframe.contentWindow
      || event.origin !== frameOrigin
    ) return;
    if (bootstrapped && isWidgetSiteFontRequestMessage(event.data)) {
      const message: WidgetSiteFontMessage = {
        type: widgetMessageTypes.siteFont, version: 1, font: collectWidgetSiteFont(),
      };
      iframe.contentWindow?.postMessage(message, frameOrigin);
      return;
    }
    if (isWidgetResizeMessage(event.data)) {
      const launcher = event.data.launcher ?? DEFAULT_LAUNCHER;
      const closedSize = closedFrameSize(launcher);
      const horizontalSpace = launcher.offset_x + 12;
      const verticalSpace = launcher.offset_y + 12;
      const callToActionVisible = event.data.call_to_action_visible === true;
      applyLauncherPosition(iframe, launcher);
      setFrameStyle(iframe, "width", event.data.open
        ? `min(400px, calc(100vw - ${horizontalSpace}px))`
        : callToActionVisible
          ? `min(320px, calc(100vw - ${horizontalSpace}px))`
          : closedSize.width);
      setFrameStyle(iframe, "height", event.data.open
        ? `min(580px, calc(100vh - ${verticalSpace}px))`
        : callToActionVisible
          ? `min(176px, calc(100vh - ${verticalSpace}px))`
          : closedSize.height);
      return;
    }
    if (!isWidgetReadyMessage(event.data)) return;
    postWidgetTheme(widget);
    if (!publicWidgetId) {
      const message: WidgetBootstrapErrorMessage = {
        type: widgetMessageTypes.bootstrapError,
        version: 1,
        message: "Widget ID is missing.",
      };
      iframe.contentWindow?.postMessage(message, frameOrigin);
      return;
    }
    bootstrapPromise ??= createSession(
      apiBaseUrl,
      publicWidgetId,
      getOrCreateVisitorId(publicWidgetId),
      language,
      collectWidgetClientContext(),
    );
    void bootstrapPromise
      .then((session) => {
        const message: WidgetBootstrapMessage = {
          type: widgetMessageTypes.bootstrap,
          version: 1,
          embed_origin: window.location.origin,
          session,
        };
        iframe.contentWindow?.postMessage(message, frameOrigin);
        bootstrapped = true;
      })
      .catch((cause: unknown) => {
        const message: WidgetBootstrapErrorMessage = {
          type: widgetMessageTypes.bootstrapError,
          version: 1,
          message: cause instanceof Error
            ? cause.message
            : "Could not start the widget.",
        };
        iframe.contentWindow?.postMessage(message, frameOrigin);
      });
  });

  document.body.append(iframe);
  window.setTimeout(() => {
    if (!iframe.isConnected || iframe.hasAttribute("src")) return;
    frameStarted = true;
    iframe.src = widget.frameUrl.toString();
    setFrameStyle(iframe, "visibility", "visible");
    setFrameStyle(iframe, "pointer-events", "auto");
    setFrameStyle(iframe, "opacity", "1");
  }, WIDGET_LOAD_DELAY_MS);
  return iframe;
}

declare global {
  interface Window {
    TzWidget?: {
      mount: typeof mountWidget;
      setTheme: typeof setWidgetTheme;
    };
  }
}

window["TzWidget"] = { mount: mountWidget, setTheme: setWidgetTheme };

export function autoMountWidget(loaderScript: HTMLScriptElement): void {
  const mount = () => {
    const source =
      loaderScript.dataset.src ?? defaultFrameSource(loaderScript);
    mountWidget({
      src: source,
      title: loaderScript.dataset.title,
      widgetId: loaderScript.dataset.widgetId,
      apiBaseUrl:
        loaderScript.dataset.apiBase
        ?? defaultApiBaseUrl(loaderScript, source),
      language: loaderScript.dataset.language,
      theme: loaderScript.dataset.theme,
    });
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", mount, { once: true });
    return;
  }

  mount();
}

const currentScript =
  document.currentScript instanceof HTMLScriptElement
    ? document.currentScript
    : null;

if (currentScript && currentScript.dataset.autoload !== "false") {
  autoMountWidget(currentScript);
}
