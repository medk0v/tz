import { afterEach, describe, expect, it, vi } from "vitest";
import {
  collectWidgetClientContext,
  getOrCreateVisitorId,
  isWidgetResizeMessage,
  isWidgetThemeMessage,
  normalizeWidgetBootstrapSession,
  visitorStorageKey,
} from "./widget-bridge";

const WIDGET_ID = "00000000-0000-4000-8000-000000000010";

function restoreProperty(
  target: object,
  key: PropertyKey,
  descriptor: PropertyDescriptor | undefined,
) {
  if (descriptor) {
    Object.defineProperty(target, key, descriptor);
  } else {
    Reflect.deleteProperty(target, key);
  }
}

describe("widget client context", () => {
  afterEach(() => {
    localStorage.clear();
    vi.unstubAllEnvs();
    vi.resetModules();
    window.history.replaceState({}, "", "/");
  });

  it("migrates a Lite visitor once so the existing contact keeps its chat history", async () => {
    vi.stubEnv("VITE_PRODUCT_EDITION", "lite");
    vi.resetModules();
    const bridge = await import("./widget-bridge");
    const visitorId = "11111111-1111-4111-8111-111111111111";
    const legacyKey = `tzomet-widget-visitor:${WIDGET_ID}`;
    localStorage.setItem(legacyKey, visitorId);
    expect(bridge.visitorStorageKey(WIDGET_ID)).toBe(`tz-widget-visitor:${WIDGET_ID}`);
    expect(bridge.getOrCreateVisitorId(WIDGET_ID, localStorage, () => {
      throw new Error("must preserve the visitor");
    })).toBe(visitorId);
    expect(localStorage.getItem(bridge.visitorStorageKey(WIDGET_ID))).toBe(visitorId);
    expect(localStorage.getItem(legacyKey)).toBeNull();
    expect(bridge.getOrCreateVisitorId(WIDGET_ID, localStorage)).toBe(visitorId);
  });

  it("keeps the legacy Lite visitor when storage prevents migration", async () => {
    vi.stubEnv("VITE_PRODUCT_EDITION", "lite");
    vi.resetModules();
    const bridge = await import("./widget-bridge");
    const visitorId = "11111111-1111-4111-8111-111111111111";
    const removeItem = vi.fn();
    const storage = {
      getItem: (key: string) => key.startsWith("tzomet-") ? visitorId : null,
      setItem: () => { throw new Error("storage denied"); },
      removeItem,
    } as unknown as Storage;
    expect(bridge.getOrCreateVisitorId(WIDGET_ID, storage)).toBe(visitorId);
    expect(removeItem).not.toHaveBeenCalled();
  });

  it("keeps page query and hash out of context while preserving allowlisted attribution", () => {
    window.history.replaceState(
      {},
      "",
      "/checkout/order?utm_source=search&gclid=campaign-id&token=secret#private",
    );
    document.title = "Checkout";
    const referrer = Object.getOwnPropertyDescriptor(document, "referrer");
    try {
      Object.defineProperty(document, "referrer", {
        configurable: true,
        value: "https://search.example/results?q=private#fragment",
      });
      const context = collectWidgetClientContext();

      expect(context.page).toMatchObject({
        url: `${window.location.origin}/checkout/order`,
        title: "Checkout",
        referrer: "https://search.example",
      });
      expect(context.page.url).not.toContain("token");
      expect(context.page.url).not.toContain("private");
      expect(context.attribution).toEqual({
        utm_source: "search",
        gclid: "campaign-id",
      });
    } finally {
      restoreProperty(document, "referrer", referrer);
    }
  });

  it("uses one first-party visitor UUID per widget", () => {
    const visitorId = "11111111-1111-4111-8111-111111111111";
    expect(getOrCreateVisitorId(WIDGET_ID, localStorage, () => visitorId)).toBe(visitorId);
    expect(localStorage.getItem(visitorStorageKey(WIDGET_ID))).toBe(visitorId);
    expect(getOrCreateVisitorId(WIDGET_ID, localStorage, () => {
      throw new Error("must not generate a replacement");
    })).toBe(visitorId);
  });

  it("replaces a stored visitor identifier that is not UUIDv4", () => {
    const replacement = "22222222-2222-4222-8222-222222222222";
    localStorage.setItem(
      visitorStorageKey(WIDGET_ID),
      "01989f56-0cb2-7abc-9def-0123456789ab",
    );

    expect(getOrCreateVisitorId(WIDGET_ID, localStorage, () => replacement)).toBe(
      replacement,
    );
    expect(localStorage.getItem(visitorStorageKey(WIDGET_ID))).toBe(replacement);
  });

  it("collects supported low-entropy browser, device, and network fields", () => {
    const navigator = window.navigator as Navigator & Record<string, unknown>;
    const userAgentData = Object.getOwnPropertyDescriptor(navigator, "userAgentData");
    const deviceMemory = Object.getOwnPropertyDescriptor(navigator, "deviceMemory");
    const connection = Object.getOwnPropertyDescriptor(navigator, "connection");
    const globalPrivacyControl = Object.getOwnPropertyDescriptor(navigator, "globalPrivacyControl");
    try {
      Object.defineProperties(navigator, {
        userAgentData: {
          configurable: true,
          value: {
            brands: [{ brand: "Example", version: "1" }],
            mobile: true,
            platform: "Example OS",
          },
        },
        deviceMemory: { configurable: true, value: 8 },
        connection: {
          configurable: true,
          value: { effectiveType: "4g", downlink: 10, rtt: 50, saveData: false },
        },
        globalPrivacyControl: { configurable: true, value: true },
      });

      const context = collectWidgetClientContext();
      expect(context.browser).toMatchObject({
        platform: "Example OS",
        brands: [{ brand: "Example", version: "1" }],
        mobile: true,
        global_privacy_control: true,
      });
      expect(context.device?.memory_gb).toBe(8);
      expect(context.connection).toEqual({
        effective_type: "4g",
        downlink_mbps: 10,
        rtt_ms: 50,
        save_data: false,
      });
    } finally {
      restoreProperty(navigator, "userAgentData", userAgentData);
      restoreProperty(navigator, "deviceMemory", deviceMemory);
      restoreProperty(navigator, "connection", connection);
      restoreProperty(navigator, "globalPrivacyControl", globalPrivacyControl);
    }
  });

});

describe("widget launcher bridge validation", () => {
  const validLauncher = {
    launcher_type: "icon_text",
    position: "bottom_left",
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

  it("accepts a complete launcher in a resize message", () => {
    expect(isWidgetResizeMessage({
      type: "tz:widget-resize",
      version: 1,
      open: false,
      call_to_action_visible: true,
      launcher: validLauncher,
    })).toBe(true);
  });

  it("rejects a non-boolean CTA visibility flag", () => {
    expect(isWidgetResizeMessage({
      type: "tz:widget-resize",
      version: 1,
      open: false,
      call_to_action_visible: "yes",
      launcher: validLauncher,
    })).toBe(false);
  });

  it.each([
    ["unknown launcher type", { ...validLauncher, launcher_type: "image" }],
    ["unknown position", { ...validLauncher, position: "top_left" }],
    ["blank label", { ...validLauncher, label: "   " }],
    ["label over 40 characters", { ...validLauncher, label: "x".repeat(41) }],
    ["non-boolean greeting visibility", { ...validLauncher, show_greeting: "yes" }],
    ["non-boolean operator profile visibility", { ...validLauncher, show_operator_profile: "yes" }],
    ["negative horizontal offset", { ...validLauncher, offset_x: -1 }],
    ["horizontal offset over 120", { ...validLauncher, offset_x: 121 }],
    ["fractional horizontal offset", { ...validLauncher, offset_x: 1.5 }],
    ["non-finite horizontal offset", { ...validLauncher, offset_x: Number.POSITIVE_INFINITY }],
    ["negative vertical offset", { ...validLauncher, offset_y: -1 }],
    ["vertical offset over 120", { ...validLauncher, offset_y: 121 }],
    ["unknown animation", { ...validLauncher, attention_animation: "spin" }],
    ["animation interval below 1", { ...validLauncher, animation_interval_seconds: 0 }],
    ["animation interval over 300", { ...validLauncher, animation_interval_seconds: 301 }],
    ["non-boolean proactive invitation", { ...validLauncher, proactive_invitation_enabled: "yes" }],
    ["proactive delay below 1", { ...validLauncher, proactive_invitation_delay_seconds: 0 }],
    ["proactive delay over 300", { ...validLauncher, proactive_invitation_delay_seconds: 301 }],
  ])("rejects %s", (_caseName, launcher) => {
    expect(isWidgetResizeMessage({
      type: "tz:widget-resize",
      version: 1,
      open: false,
      launcher,
    })).toBe(false);
  });
});

describe("widget theme bridge validation", () => {
  it("accepts only supported runtime theme messages", () => {
    expect(isWidgetThemeMessage({
      type: "tz:widget-theme",
      version: 1,
      theme: "dark",
    })).toBe(true);
    expect(isWidgetThemeMessage({
      type: "tz:widget-theme",
      version: 1,
      theme: "system",
    })).toBe(false);
  });
});

describe("legacy widget bootstrap normalization", () => {
  it("defaults missing availability to offline without losing legacy sessions", () => {
    const session = normalizeWidgetBootstrapSession({
      session_id: "33333333-3333-4333-8333-333333333333",
      token: "widget-token",
      inbox_id: "44444444-4444-4444-8444-444444444444",
      language: "ru",
      expires_at: "2026-08-10T00:00:00Z",
      launcher: {
        launcher_type: "icon",
        position: "bottom_right",
        label: "Напишите нам",
        offset_x: 20,
        offset_y: 20,
      },
      theme: {
        accent_color: "#2563eb",
        accent_text_color: "#ffffff",
        surface_color: "#ffffff",
        text_color: "#111827",
      },
    });

    expect(session?.operators_online).toBe(false);
    expect(session?.launcher.attention_animation).toBe("pulse");
    expect(session?.launcher.animation_interval_seconds).toBe(5);
    expect(session?.support_name).toBe("Support");
    expect(session?.launcher.show_greeting).toBe(true);
    expect(session?.launcher.show_operator_profile).toBe(false);
    expect(session?.launcher.proactive_invitation_enabled).toBe(false);
    expect(session?.launcher.proactive_invitation_delay_seconds).toBe(15);
    expect(session?.offline_message).toBe(
      "Операторы офлайн. Оставьте сообщение и контакты.",
    );
    expect(session?.proactive_invitation_message).toBe("Здравствуйте! Могу помочь?");
    expect(session?.contact).toEqual({ display_name: null, email: null });
    expect(session?.theme.border_radius).toBe(20);
    expect(session?.theme.opening_animation).toBe(true);
    expect(normalizeWidgetBootstrapSession({
      ...session, theme: { ...session?.theme, opening_animation: false },
    })?.theme.opening_animation).toBe(false);
    for (const opening_animation of ["true", 1, null]) {
      expect(normalizeWidgetBootstrapSession({
        ...session, theme: { ...session?.theme, opening_animation },
      })).toBeNull();
    }
    expect(session?.theme.reply_typing_effect).toBe(false);
    expect(normalizeWidgetBootstrapSession({
      ...session, theme: { ...session?.theme, reply_typing_effect: true },
    })?.theme.reply_typing_effect).toBe(true);
    for (const reply_typing_effect of ["true", 1, null]) {
      expect(normalizeWidgetBootstrapSession({
        ...session, theme: { ...session?.theme, reply_typing_effect },
      })).toBeNull();
    }
    expect(session?.theme.dark).toEqual(expect.objectContaining({
      accent_color: "#F05A28",
      accent_text_color: "#FFFFFF",
      surface_color: "#1C1E21",
      text_color: "#F2F3F4",
      component_colors: expect.objectContaining({
        background_color: "#202225",
        control_background_color: "#1C1E21",
        online_color: "#27A857",
        danger_color: "#FF8B82",
      }),
    }));
    expect(session?.theme.border).toEqual({
      enabled: true,
      width: 1,
      light_color: "#E3E4E6",
      dark_color: "#3A3D42",
    });
  });
});
