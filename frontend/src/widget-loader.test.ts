import { waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { autoMountWidget, mountWidget, setWidgetTheme } from "./widget-loader";

const launcher = {
  launcher_type: "icon_text" as const,
  position: "bottom_left" as const,
  label: "Talk to support",
  show_greeting: true,
  show_operator_profile: false,
  offset_x: 32,
  offset_y: 24,
  attention_animation: "pulse",
  animation_interval_seconds: 5,
  proactive_invitation_enabled: false,
  proactive_invitation_delay_seconds: 15,
};

const session = {
  session_id: "00000000-0000-4000-8000-000000000001",
  token: "widget-token",
  inbox_id: "00000000-0000-4000-8000-000000000002",
  language: "en" as const,
  support_name: "Anna Petrova",
  greeting: "Welcome",
  offline_message: "Operators are offline. Leave a message and your contact details.",
  proactive_invitation_message: "Hi! Can I help you?",
  offline_now: "Leave a message",
  operators_online: true,
  contact: {
    display_name: null,
    email: null,
  },
  launcher,
  theme: {
    accent_color: "#F05A28",
    accent_text_color: "#FFFFFF",
    surface_color: "#FFFFFF",
    text_color: "#1B1B1D",
    dark: {
      accent_color: "#F05A28",
      accent_text_color: "#FFFFFF",
      surface_color: "#1C1E21",
      text_color: "#F2F3F4",
    },
    border: {
      enabled: true,
      width: 1,
      light_color: "#E3E4E6",
      dark_color: "#3A3D42",
    },
    border_radius: 20,
  },
  expires_at: "2099-08-01T00:00:00Z",
};

function mountStartedWidget(
  options: Parameters<typeof mountWidget>[0],
): HTMLIFrameElement {
  vi.useFakeTimers();
  try {
    const iframe = mountWidget(options);
    vi.advanceTimersByTime(1_000);
    return iframe;
  } finally {
    vi.useRealTimers();
  }
}

describe("mountWidget", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.lang = "";
    window.history.replaceState({}, "", "/checkout?utm_source=test&secret=hidden#token");
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(
      JSON.stringify(session),
      { status: 200, headers: { "Content-Type": "application/json" } },
    )));
  });

  afterEach(() => {
    document.getElementById("tz-customer-widget")?.remove();
    document.getElementById("tz-customer-widget")?.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    vi.unstubAllEnvs();
    vi.resetModules();
  });

  it("waits one second before loading the single iframe", () => {
    vi.useFakeTimers();
    try {
      const first = mountWidget({
        src: "https://example.com/widget",
        widgetId: "00000000-0000-0000-0000-000000000010",
      });
      const second = mountWidget({ src: "https://example.com/other" });

      expect(first).toBe(second);
      expect(first.getAttribute("src")).toBeNull();
      expect(first.style.visibility).toBe("hidden");
      vi.advanceTimersByTime(999);
      expect(first.getAttribute("src")).toBeNull();
      vi.advanceTimersByTime(1);
      expect(first.src).toBe(
        "https://example.com/widget?widget_id=00000000-0000-0000-0000-000000000010&theme=light",
      );
      expect(first.style.visibility).toBe("visible");
      expect(first.style.pointerEvents).toBe("auto");
      expect(first.style.opacity).toBe("1");
      expect(first.style.transition).toBe("width 240ms cubic-bezier(.22, .61, .36, 1), height 240ms cubic-bezier(.22, .61, .36, 1), opacity 120ms ease-out");
      expect(first.style.getPropertyPriority("width")).toBe("important");
      expect(first.style.getPropertyPriority("height")).toBe("important");
      expect(first.style.getPropertyPriority("position")).toBe("important");
      expect(first.style.getPropertyPriority("visibility")).toBe("important");
      expect(first.referrerPolicy).toBe("no-referrer");
      expect(first.getAttribute("allow")).toBe("autoplay; clipboard-write");
    } finally {
      vi.useRealTimers();
    }
  });

  it("bootstraps from the host page and sends the token only to the exact frame origin", async () => {
    const iframe = mountStartedWidget({
      src: "https://widget.example/widget.html",
      apiBaseUrl: "https://api.example",
      widgetId: "00000000-0000-4000-8000-000000000010",
    });
    const postMessage = vi.spyOn(iframe.contentWindow!, "postMessage");

    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://widget.example",
      data: { type: "tz:widget-ready", version: 1 },
    }));

    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
    const [url, options] = vi.mocked(fetch).mock.calls[0];
    expect(String(url)).toBe("https://api.example/widget/v1/sessions");
    expect(options).toMatchObject({
      method: "POST",
      credentials: "omit",
      referrerPolicy: "no-referrer",
      headers: { "Content-Type": "text/plain;charset=UTF-8" },
    });
    const body = JSON.parse(String(options?.body)) as Record<string, unknown>;
    expect(body).toMatchObject({
      widget_id: "00000000-0000-4000-8000-000000000010",
      language: null,
    });
    expect(body.visitor_id).toEqual(expect.any(String));
    expect(body.client_context).toMatchObject({
      page: { url: `${window.location.origin}/checkout` },
      attribution: { utm_source: "test" },
    });
    await waitFor(() => expect(postMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "tz:widget-bootstrap",
        session: { ...session, theme: { ...session.theme, reply_typing_effect: false, opening_animation: true } },
        embed_origin: window.location.origin,
      }),
      "https://widget.example",
    ));
    expect(iframe.src).not.toContain("widget-token");
    expect(iframe.style.visibility).toBe("visible");

    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://widget.example",
      data: {
        type: "tz:widget-resize",
        version: 1,
        open: false,
        launcher,
      },
    }));
    expect(iframe.style.width).toBe("244px");
  });

  it("retains frame resizing transitions when the saved opening animation is disabled", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(new Response(
      JSON.stringify({ ...session, theme: { ...session.theme, opening_animation: false } }),
      { status: 200, headers: { "Content-Type": "application/json" } },
    ));
    const iframe = mountStartedWidget({
      src: "https://widget.example/widget.html",
      apiBaseUrl: "https://api.example",
      widgetId: "00000000-0000-4000-8000-000000000010",
    });
    const postMessage = vi.spyOn(iframe.contentWindow!, "postMessage");

    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://widget.example",
      data: { type: "tz:widget-ready", version: 1 },
    }));

    await waitFor(() => expect(postMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "tz:widget-bootstrap",
        session: expect.objectContaining({
          theme: expect.objectContaining({ opening_animation: false }),
        }),
      }),
      "https://widget.example",
    ));
    expect(iframe.style.transition).toContain("width 240ms");
    expect(iframe.style.transition).toContain("height 240ms");
  });

  it("keeps custom colors from legacy sessions without a border radius", async () => {
    const legacyTheme = {
      accent_color: "#444444",
      accent_text_color: "#FFFFFF",
      surface_color: "#FFFFFF",
      text_color: "#1B1B1D",
    };
    vi.mocked(fetch).mockResolvedValueOnce(new Response(
      JSON.stringify({ ...session, theme: legacyTheme }),
      { status: 200, headers: { "Content-Type": "application/json" } },
    ));
    const iframe = mountStartedWidget({
      src: "https://widget.example/widget.html",
      apiBaseUrl: "https://api.example",
      widgetId: "00000000-0000-4000-8000-000000000010",
    });
    const postMessage = vi.spyOn(iframe.contentWindow!, "postMessage");

    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://widget.example",
      data: { type: "tz:widget-ready", version: 1 },
    }));

    await waitFor(() => expect(postMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "tz:widget-bootstrap",
        session: expect.objectContaining({
          theme: expect.objectContaining({ ...legacyTheme, border_radius: 20 }),
        }),
      }),
      "https://widget.example",
    ));
  });

  it("shares the host font only with the bootstrapped iframe after an authenticated request", async () => {
    const iframe = mountStartedWidget({
      src: "https://widget.example/widget.html",
      apiBaseUrl: "https://api.example",
      widgetId: "00000000-0000-4000-8000-000000000010",
    });
    const postMessage = vi.spyOn(iframe.contentWindow!, "postMessage");
    const fontRequest = { type: "tz:widget-site-font-request", version: 1 };
    vi.spyOn(window, "getComputedStyle").mockReturnValue({ fontFamily: "Georgia, serif" } as CSSStyleDeclaration);

    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://widget.example",
      data: fontRequest,
    }));
    expect(postMessage).not.toHaveBeenCalled();
    expect(fetch).not.toHaveBeenCalled();

    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://widget.example",
      data: { type: "tz:widget-ready", version: 1 },
    }));
    await waitFor(() => expect(postMessage).toHaveBeenCalledWith(
      expect.objectContaining({ type: "tz:widget-bootstrap" }),
      "https://widget.example",
    ));
    postMessage.mockClear();

    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://attacker.example",
      data: fontRequest,
    }));
    window.dispatchEvent(new MessageEvent("message", {
      source: window,
      origin: "https://widget.example",
      data: fontRequest,
    }));
    expect(postMessage).not.toHaveBeenCalled();

    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://widget.example",
      data: fontRequest,
    }));
    expect(postMessage).toHaveBeenCalledExactlyOnceWith({
      type: "tz:widget-site-font",
      version: 1,
      font: { family: "Georgia", faces: [] },
    }, "https://widget.example");
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("uses an explicit language before the route and host html languages", async () => {
    window.history.replaceState({}, "", "/ro");
    document.documentElement.lang = "ru-RU";
    const iframe = mountStartedWidget({
      src: "https://widget.example/widget.html",
      apiBaseUrl: "https://api.example",
      widgetId: "00000000-0000-4000-8000-000000000010",
      language: "zh-CN",
    });
    expect(iframe.src).toContain("lang=zh-cn");

    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://widget.example",
      data: { type: "tz:widget-ready", version: 1 },
    }));

    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
    const body = JSON.parse(String(vi.mocked(fetch).mock.calls[0][1]?.body)) as Record<string, unknown>;
    expect(body.language).toBe("zh-cn");
  });

  it.each(["ro", "hi", "zh"])("uses the %s locale route when the host html language is incorrect", async (language) => {
    window.history.replaceState({}, "", `/${language}/exchange`);
    document.documentElement.lang = "en";
    const iframe = mountStartedWidget({
      src: "https://widget.example/widget.html",
      apiBaseUrl: "https://api.example",
      widgetId: "00000000-0000-4000-8000-000000000010",
    });

    expect(new URL(iframe.src).searchParams.get("lang")).toBe(language);

    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://widget.example",
      data: { type: "tz:widget-ready", version: 1 },
    }));

    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
    const body = JSON.parse(String(vi.mocked(fetch).mock.calls[0][1]?.body)) as Record<string, unknown>;
    expect(body.language).toBe(language);
  });

  it("accepts a user-defined language identifier", async () => {
    const iframe = mountStartedWidget({
      src: "https://widget.example/widget.html",
      apiBaseUrl: "https://api.example",
      widgetId: "00000000-0000-4000-8000-000000000010",
      language: "  Client's Language  ",
    });
    expect(new URL(iframe.src).searchParams.get("lang")).toBe("client's language");

    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://widget.example",
      data: { type: "tz:widget-ready", version: 1 },
    }));

    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
    const body = JSON.parse(String(vi.mocked(fetch).mock.calls[0][1]?.body)) as Record<string, unknown>;
    expect(body.language).toBe("client's language");
  });

  it("passes an explicit dark theme from the host site to the iframe", () => {
    const iframe = mountStartedWidget({
      src: "https://widget.example/widget.html",
      widgetId: "00000000-0000-4000-8000-000000000010",
      theme: "dark",
    });

    expect(new URL(iframe.src).searchParams.get("theme")).toBe("dark");
    expect(iframe.style.colorScheme).toBe("dark");
  });

  it("switches the mounted widget theme without reloading its iframe", () => {
    const iframe = mountStartedWidget({
      src: "https://widget.example/widget.html",
      widgetId: "00000000-0000-4000-8000-000000000010",
    });
    const initialSource = iframe.src;
    const postMessage = vi.spyOn(iframe.contentWindow!, "postMessage");

    expect(setWidgetTheme("dark")).toBe(true);

    expect(iframe.src).toBe(initialSource);
    expect(iframe.style.colorScheme).toBe("dark");
    expect(postMessage).toHaveBeenCalledWith(
      { type: "tz:widget-theme", version: 1, theme: "dark" },
      "https://widget.example",
    );
  });

  it("applies a theme switch made before the iframe starts loading", () => {
    vi.useFakeTimers();
    try {
      const iframe = mountWidget({
        src: "https://widget.example/widget.html",
        widgetId: "00000000-0000-4000-8000-000000000010",
      });

      expect(setWidgetTheme("dark")).toBe(true);
      vi.advanceTimersByTime(1_000);

      expect(new URL(iframe.src).searchParams.get("theme")).toBe("dark");
      expect(iframe.style.colorScheme).toBe("dark");
    } finally {
      vi.useRealTimers();
    }
  });

  it("falls back to the host html language", () => {
    document.documentElement.lang = "ru-MD";
    const iframe = mountStartedWidget({
      src: "https://widget.example/widget.html",
      widgetId: "00000000-0000-4000-8000-000000000010",
    });

    expect(iframe.src).toContain("lang=ru-md");
  });

  it("waits for the host app to set html language before autoloading", () => {
    vi.useFakeTimers();
    try {
      vi.spyOn(document, "readyState", "get").mockReturnValue("loading");
      let readyListener: EventListenerOrEventListenerObject | undefined;
      vi.spyOn(document, "addEventListener").mockImplementation((type, listener) => {
        if (type === "DOMContentLoaded") readyListener = listener;
      });
      document.documentElement.lang = "en";
      const script = document.createElement("script");
      script.src = "https://loader.example/loader/widget-loader.js";
      script.dataset.src = "https://widget.example/widget.html";
      script.dataset.apiBase = "https://api.example";
      script.dataset.widgetId = "00000000-0000-4000-8000-000000000010";
      script.dataset.theme = "dark";

      autoMountWidget(script);
      expect(document.getElementById("tz-customer-widget")).toBeNull();

      document.documentElement.lang = "ru";
      expect(readyListener).toBeDefined();
      const readyEvent = new Event("DOMContentLoaded");
      if (typeof readyListener === "function") {
        readyListener(readyEvent);
      } else {
        readyListener?.handleEvent(readyEvent);
      }

      const iframe = document.getElementById("tz-customer-widget");
      expect(iframe).toBeInstanceOf(HTMLIFrameElement);
      expect((iframe as HTMLIFrameElement).getAttribute("src")).toBeNull();
      vi.advanceTimersByTime(1_000);
      expect((iframe as HTMLIFrameElement).src).toContain("lang=ru");
      expect((iframe as HTMLIFrameElement).src).toContain("theme=dark");
      expect((iframe as HTMLIFrameElement).style.visibility).toBe("visible");
    } finally {
      vi.useRealTimers();
    }
  });

  it("applies launcher type, position, offsets, and responsive open size", () => {
    const iframe = mountStartedWidget({
      src: "https://widget.example/widget.html",
      apiBaseUrl: "https://api.example",
      widgetId: "00000000-0000-4000-8000-000000000010",
    });
    const closedSizes = [
      ["icon", "86px", "86px"],
      ["text", "204px", "80px"],
      ["icon_text", "244px", "80px"],
    ] as const;

    for (const [launcherType, width, height] of closedSizes) {
      window.dispatchEvent(new MessageEvent("message", {
        source: iframe.contentWindow,
        origin: "https://widget.example",
        data: {
          type: "tz:widget-resize",
          version: 1,
          open: false,
          launcher: { ...launcher, launcher_type: launcherType },
        },
      }));

      expect(iframe.style.left).toBe("32px");
      expect(iframe.style.right).toBe("auto");
      expect(iframe.style.bottom).toBe("24px");
      expect(iframe.style.width).toBe(width);
      expect(iframe.style.height).toBe(height);
    }

    const setProperty = vi.spyOn(iframe.style, "setProperty");
    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://widget.example",
      data: {
        type: "tz:widget-resize",
        version: 1,
        open: false,
        call_to_action_visible: true,
        launcher,
      },
    }));
    expect(setProperty).toHaveBeenCalledWith(
      "width",
      "min(320px, calc(100vw - 44px))",
      "important",
    );
    expect(setProperty).toHaveBeenCalledWith(
      "height",
      "min(176px, calc(100vh - 36px))",
      "important",
    );

    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://widget.example",
      data: {
        type: "tz:widget-resize",
        version: 1,
        open: true,
        launcher,
      },
    }));

    expect(iframe.style.left).toBe("32px");
    expect(iframe.style.right).toBe("auto");
    expect(iframe.style.bottom).toBe("24px");
    expect(setProperty).toHaveBeenCalledWith(
      "width",
      "min(400px, calc(100vw - 44px))",
      "important",
    );
    expect(setProperty).toHaveBeenCalledWith(
      "height",
      "min(580px, calc(100vh - 36px))",
      "important",
    );
  });

  it("keeps the open widget visible when the host page is clicked", () => {
    const iframe = mountStartedWidget({
      src: "https://widget.example/widget.html",
      widgetId: "00000000-0000-4000-8000-000000000010",
    });
    const postMessage = vi.spyOn(iframe.contentWindow!, "postMessage");

    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://widget.example",
      data: {
        type: "tz:widget-resize",
        version: 1,
        open: true,
        launcher,
      },
    }));
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));

    expect(postMessage).not.toHaveBeenCalled();
  });

  it("ignores ready and resize messages from the wrong source or origin", () => {
    const iframe = mountStartedWidget({
      src: "https://widget.example/widget.html",
      apiBaseUrl: "https://api.example",
      widgetId: "00000000-0000-4000-8000-000000000010",
    });

    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://attacker.example",
      data: { type: "tz:widget-ready", version: 1 },
    }));
    window.dispatchEvent(new MessageEvent("message", {
      source: window,
      origin: "https://widget.example",
      data: { type: "tz:widget-ready", version: 1 },
    }));
    window.dispatchEvent(new MessageEvent("message", {
      source: iframe.contentWindow,
      origin: "https://attacker.example",
      data: { type: "tz:widget-resize", version: 1, open: true },
    }));

    expect(fetch).not.toHaveBeenCalled();
    expect(iframe.style.width).toBe("86px");
    expect(iframe.style.height).toBe("86px");
  });
});
