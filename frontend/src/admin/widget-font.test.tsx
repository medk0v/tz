import { afterEach, describe, expect, it, vi } from "vitest";
import {
  applyWidgetSiteFont,
  collectWidgetSiteFont,
  isWidgetFontFamily,
  isWidgetSiteFont,
  widgetFontStack,
  type WidgetSiteFont,
} from "../widget-font";

const font: WidgetSiteFont = {
  family: "Site Sans",
  faces: [{
    url: "https://site.example/fonts/site-sans.woff2",
    weight: "100 900",
    style: "normal",
    unicodeRange: "U+0400-045F, U+0490-0491",
  }],
};

const fontsDescriptor = Object.getOwnPropertyDescriptor(document, "fonts");

function fontRule(properties: Record<string, string>): CSSRule {
  // jsdom discards font-face src declarations; supply the browser CSSOM boundary.
  return {
    type: CSSRule.FONT_FACE_RULE,
    style: { getPropertyValue: (name: string) => properties[name] ?? "" },
  } as unknown as CSSRule;
}

function useSheets(...sheets: Partial<CSSStyleSheet>[]): void {
  vi.spyOn(document, "styleSheets", "get").mockReturnValue(sheets as unknown as StyleSheetList);
}

function useBodyFont(family = '"Site Sans", Arial, sans-serif'): void {
  vi.spyOn(window, "getComputedStyle").mockReturnValue({ fontFamily: family } as CSSStyleDeclaration);
}

afterEach(() => {
  if (fontsDescriptor) Object.defineProperty(document, "fonts", fontsDescriptor);
  else Reflect.deleteProperty(document, "fonts");
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("widget font settings", () => {
  it("uses local font stacks with generic fallbacks and defaults unknown choices", () => {
    expect(isWidgetFontFamily("arial")).toBe(true);
    expect(widgetFontStack("arial")).toMatch(/^Arial,.*sans-serif$/);
    expect(widgetFontStack("georgia")).toMatch(/^Georgia,.*serif$/);
    expect(widgetFontStack("courier_new")).toMatch(/^"Courier New",.*monospace$/);
    expect(isWidgetFontFamily("Some remote font")).toBe(false);
    expect(widgetFontStack("Some remote font")).toBe(widgetFontStack("system"));
    expect(widgetFontStack()).toBe(widgetFontStack("system"));
  });
});

describe("site font payload validation", () => {
  it("accepts a bounded variable font with Cyrillic subsets or a local family", () => {
    expect(isWidgetSiteFont(font)).toBe(true);
    expect(isWidgetSiteFont({ family: "Arial", faces: [] })).toBe(true);
  });

  it.each([
    "javascript:alert(1)",
    "data:font/woff2;base64,AAAA",
    "file:///tmp/site.woff2",
    "https://user:secret@site.example/font.woff2",
    "/relative/font.woff2",
  ])("rejects unsafe or unresolved font URL %s", (url) => {
    expect(isWidgetSiteFont({ ...font, faces: [{ ...font.faces[0], url }] })).toBe(false);
  });

  it("rejects injected CSS, invalid descriptors, oversized names and excess faces", () => {
    expect(isWidgetSiteFont({ ...font, family: 'Site Sans"; color: red;' })).toBe(false);
    expect(isWidgetSiteFont({ ...font, family: "s".repeat(101) })).toBe(false);
    expect(isWidgetSiteFont({ ...font, faces: Array.from({ length: 17 }, () => font.faces[0]) })).toBe(false);
    for (const descriptor of [
      { weight: "normal; color: red" },
      { style: "italic; background: url(https://other.example)" },
      { unicodeRange: "U+0000-FFFF; font-family: Other" },
      { url: `https://site.example/${"a".repeat(2048)}` },
    ]) {
      expect(isWidgetSiteFont({ ...font, faces: [{ ...font.faces[0], ...descriptor }] })).toBe(false);
    }
  });
});

describe("site font discovery", () => {
  it("collects the primary family's matching faces, preserving descriptors and stylesheet-relative URLs", () => {
    useBodyFont();
    useSheets({
      href: "https://site.example/assets/main.css",
      cssRules: [
        fontRule({ "font-family": "Other", src: "url(other.woff2)" }),
        fontRule({
          "font-family": '"site sans"',
          src: 'local("Site Sans"), url("../fonts/site-sans.woff2") format("woff2")',
          "font-weight": "100 900",
          "font-style": "italic",
          "unicode-range": "U+0400-045F, U+0490-0491",
        }),
        {
          type: CSSRule.MEDIA_RULE,
          cssRules: [fontRule({ "font-family": '"Site Sans"', src: "url(regular.woff2)" })],
        } as unknown as CSSRule,
      ] as unknown as CSSRuleList,
    });

    expect(collectWidgetSiteFont()).toEqual({
      family: "Site Sans",
      faces: [
        { ...font.faces[0], style: "italic" },
        { url: "https://site.example/assets/regular.woff2", weight: "normal", style: "normal", unicodeRange: "U+0-10FFFF" },
      ],
    });
  });

  it("skips inaccessible stylesheets and unsafe sources without losing local-family inheritance", () => {
    useBodyFont("Arial, sans-serif");
    useSheets(
      { get cssRules(): CSSRuleList { throw new DOMException("Cross-origin stylesheet", "SecurityError"); } },
      { cssRules: [fontRule({ "font-family": "Arial", src: 'url("data:font/woff2;base64,AAAA")' })] as unknown as CSSRuleList },
    );

    expect(collectWidgetSiteFont()).toEqual({ family: "Arial", faces: [] });
  });

  it("bounds copied faces and skips invalid computed families", () => {
    useBodyFont();
    useSheets({
      cssRules: Array.from({ length: 20 }, (_, index) =>
        fontRule({ "font-family": '"Site Sans"', src: `url(https://site.example/${index}.woff2)` }),
      ) as unknown as CSSRuleList,
    });
    expect(collectWidgetSiteFont()?.faces).toHaveLength(16);

    vi.mocked(window.getComputedStyle).mockReturnValue({ fontFamily: "" } as CSSStyleDeclaration);
    expect(collectWidgetSiteFont()).toBeNull();
  });
});

describe("site font application", () => {
  function mockFontLoading() {
    const fonts = { add: vi.fn(), delete: vi.fn() };
    Object.defineProperty(document, "fonts", { configurable: true, value: fonts });
    const pending: { resource: FontFace; resolve: (face: FontFace) => void; reject: (cause: Error) => void }[] = [];
    const FontFaceMock = vi.fn(function (this: FontFace, family: string, source: string, descriptors: FontFaceDescriptors) {
      Object.assign(this, { family, source, descriptors });
      const promise = new Promise<FontFace>((resolve, reject) => {
        pending.push({ resource: this, resolve, reject });
      });
      this.load = vi.fn(() => promise);
    });
    vi.stubGlobal("FontFace", FontFaceMock);
    return { fonts, pending, FontFaceMock };
  }

  it("uses a local family immediately without fetching font files", () => {
    const onReady = vi.fn();
    const fetch = vi.fn();
    vi.stubGlobal("fetch", fetch);

    applyWidgetSiteFont({ family: "Arial", faces: [] }, onReady)();

    expect(onReady).toHaveBeenCalledWith('"Arial"');
    expect(fetch).not.toHaveBeenCalled();
  });

  it("preserves a generic site family as a CSS keyword rather than a quoted font name", () => {
    const onReady = vi.fn();

    applyWidgetSiteFont({ family: "system-ui", faces: [] }, onReady)();

    expect(onReady).toHaveBeenCalledWith("system-ui");
  });

  it("loads web fonts under a private family with their descriptors and removes them on cleanup", async () => {
    const { fonts, pending, FontFaceMock } = mockFontLoading();
    const onReady = vi.fn();
    const cleanup = applyWidgetSiteFont(font, onReady);

    expect(FontFaceMock).toHaveBeenCalledWith(
      expect.stringMatching(/^TzSiteFont-/),
      'url("https://site.example/fonts/site-sans.woff2")',
      { weight: "100 900", style: "normal", unicodeRange: "U+0400-045F, U+0490-0491", display: "swap" },
    );
    expect(onReady).not.toHaveBeenCalled();

    pending[0].resolve(pending[0].resource);
    await Promise.resolve();
    expect(fonts.add).toHaveBeenCalledWith(pending[0].resource);
    expect(onReady).toHaveBeenCalledWith(JSON.stringify(pending[0].resource.family));

    cleanup();
    expect(fonts.delete).toHaveBeenCalledWith(pending[0].resource);
  });

  it("keeps the fallback on failed loads and ignores resources resolved after cleanup", async () => {
    const { fonts, pending } = mockFontLoading();
    const onReady = vi.fn();
    const cleanup = applyWidgetSiteFont({ ...font, faces: [font.faces[0], { ...font.faces[0], style: "italic" }] }, onReady);

    pending[0].reject(new Error("CORS denied"));
    await Promise.resolve();
    await Promise.resolve();
    cleanup();
    pending[1].resolve(pending[1].resource);
    await Promise.resolve();

    expect(onReady).not.toHaveBeenCalled();
    expect(fonts.add).not.toHaveBeenCalled();
  });

  it("keeps the fallback when FontFace is unavailable or rejects unsupported descriptors", () => {
    const onReady = vi.fn();
    vi.stubGlobal("FontFace", undefined);
    expect(() => applyWidgetSiteFont(font, onReady)()).not.toThrow();

    mockFontLoading();
    vi.stubGlobal("FontFace", vi.fn(function () { throw new Error("Unsupported font"); }));
    expect(() => applyWidgetSiteFont(font, onReady)()).not.toThrow();
    expect(onReady).not.toHaveBeenCalled();
  });
});
