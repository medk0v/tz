import * as Sentry from "@sentry/browser";

export type FrontendSurface = "admin" | "widget";

function withoutQueryOrFragment(value: unknown): unknown {
  if (typeof value !== "string") return value;
  return value.split(/[?#]/, 1)[0].replace(/\/notes\/share\/[^/]+/, "/notes/share/[redacted]");
}

function isPublicNotePage(value: string): boolean {
  return /(?:^|\/)notes\/share(?:\/|[?#]|$)/.test(value);
}

/** Initializes privacy-conscious browser error reporting when a DSN is configured. */
export function initMonitoring(surface: FrontendSurface): void {
  const dsn = import.meta.env.VITE_SENTRY_DSN?.trim();
  if (!dsn) return;

  Sentry.init({
    dsn,
    environment: import.meta.env.VITE_SENTRY_ENVIRONMENT ?? import.meta.env.MODE,
    release: import.meta.env.VITE_SENTRY_RELEASE,
    sendDefaultPii: false,
    initialScope: {
      tags: {
        surface,
      },
    },
    beforeSend(event) {
      // Public documents and their bearer links must not enter error reports.
      if (isPublicNotePage(window.location.pathname)
        || (event.request?.url && isPublicNotePage(event.request.url))) return null;
      event.user = undefined;
      if (event.request) {
        event.request = {
          method: event.request.method,
          url: event.request.url
            ? String(withoutQueryOrFragment(event.request.url))
            : undefined,
        };
      }
      return event;
    },
    beforeBreadcrumb(breadcrumb) {
      if (isPublicNotePage(window.location.pathname)) return null;
      if (breadcrumb.category === "console") return null;

      if (breadcrumb.data) {
        breadcrumb.data = {
          ...breadcrumb.data,
          ...(breadcrumb.data.url
            ? { url: withoutQueryOrFragment(breadcrumb.data.url) }
            : {}),
          ...(breadcrumb.data.from
            ? { from: withoutQueryOrFragment(breadcrumb.data.from) }
            : {}),
          ...(breadcrumb.data.to
            ? { to: withoutQueryOrFragment(breadcrumb.data.to) }
            : {}),
        };
      }
      return breadcrumb;
    },
  });
}
