export const WIDGET_FONT_FAMILIES = [
  "system", "arial", "verdana", "tahoma", "trebuchet_ms", "georgia", "times_new_roman", "courier_new",
] as const;

export type WidgetFontFamily = typeof WIDGET_FONT_FAMILIES[number];

const FONT_STACKS: Record<WidgetFontFamily, string> = {
  system: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif',
  arial: 'Arial, "Helvetica Neue", Helvetica, sans-serif',
  verdana: 'Verdana, Geneva, sans-serif',
  tahoma: 'Tahoma, Verdana, sans-serif',
  trebuchet_ms: '"Trebuchet MS", Arial, sans-serif',
  georgia: 'Georgia, "Times New Roman", serif',
  times_new_roman: '"Times New Roman", Times, serif',
  courier_new: '"Courier New", Courier, monospace',
};

export function isWidgetFontFamily(value: unknown): value is WidgetFontFamily {
  return typeof value === "string" && WIDGET_FONT_FAMILIES.some((family) => family === value);
}

export function widgetFontStack(family?: unknown): string {
  return FONT_STACKS[isWidgetFontFamily(family) ? family : "system"];
}

export interface WidgetSiteFontFace {
  url: string;
  weight: string;
  style: string;
  unicodeRange: string;
}

export interface WidgetSiteFont {
  family: string;
  faces: WidgetSiteFontFace[];
}

const MAX_FONT_FACES = 16;
const FAMILY_PATTERN = /^[\p{L}\p{N} _-]{1,100}$/u;
const WEIGHT_PATTERN = /^(normal|bold|[1-9]\d{0,2}|1000)( ([1-9]\d{0,2}|1000))?$/;
const STYLE_PATTERN = /^(normal|italic|oblique(?: -?\d{1,2}deg(?: -?\d{1,2}deg)?)?)$/;
const RANGE_PATTERN = /^U\+[0-9a-f?]{1,6}(?:-[0-9a-f]{1,6})?(?:,\s*U\+[0-9a-f?]{1,6}(?:-[0-9a-f]{1,6})?)*$/i;
const GENERIC_FAMILIES = new Set(["serif", "sans-serif", "monospace", "cursive", "fantasy", "system-ui", "ui-serif", "ui-sans-serif", "ui-monospace", "ui-rounded", "emoji", "math", "fangsong", "-apple-system", "BlinkMacSystemFont"]);

function fontUrl(value: string, base?: string): string | null {
  if (value.length > 2048) return null;
  try {
    const url = new URL(value, base);
    return ["https:", "http:"].includes(url.protocol) && !url.username && !url.password
      ? url.href : null;
  } catch { return null; }
}

function fontFamily(value: string): string {
  return value.trim().replace(/^("|')(.*)\1$/, "$2");
}

export function isWidgetSiteFont(value: unknown): value is WidgetSiteFont {
  if (!value || typeof value !== "object") return false;
  const font = value as Partial<WidgetSiteFont>;
  return typeof font.family === "string" && FAMILY_PATTERN.test(font.family)
    && Array.isArray(font.faces) && font.faces.length <= MAX_FONT_FACES
    && font.faces.every((face: unknown) => {
      if (!face || typeof face !== "object") return false;
      const item = face as Partial<WidgetSiteFontFace>;
      return typeof item.url === "string" && fontUrl(item.url) !== null
        && typeof item.weight === "string" && WEIGHT_PATTERN.test(item.weight)
        && typeof item.style === "string" && STYLE_PATTERN.test(item.style)
        && typeof item.unicodeRange === "string" && item.unicodeRange.length <= 1024 && RANGE_PATTERN.test(item.unicodeRange);
    });
}

/** Read only the primary page font and matching, readable @font-face rules. */
export function collectWidgetSiteFont(): WidgetSiteFont | null {
  const family = fontFamily(getComputedStyle(document.body).fontFamily.split(",")[0] ?? "");
  if (!FAMILY_PATTERN.test(family)) return null;
  const faces: WidgetSiteFontFace[] = [];
  const visited = new Set<CSSStyleSheet>();
  let remaining = 4000;
  const visitRules = (rules: CSSRuleList, base: string) => {
    for (const rule of Array.from(rules)) {
      if (--remaining < 0 || faces.length >= MAX_FONT_FACES) return;
      if (rule.type === CSSRule.FONT_FACE_RULE) {
        const style = (rule as CSSFontFaceRule).style;
        if (fontFamily(style.getPropertyValue("font-family")).toLowerCase() !== family.toLowerCase()) continue;
        const source = style.getPropertyValue("src");
        const match = /url\(\s*(?:"([^"]+)"|'([^']+)'|([^\s)]+))\s*\)/i.exec(source);
        const url = match && fontUrl(match[1] ?? match[2] ?? match[3], base);
        if (!url) continue;
        const face = {
          url,
          weight: style.getPropertyValue("font-weight").trim() || "normal",
          style: style.getPropertyValue("font-style").trim() || "normal",
          unicodeRange: style.getPropertyValue("unicode-range").trim() || "U+0-10FFFF",
        };
        if (isWidgetSiteFont({ family, faces: [face] })) faces.push(face);
      } else if (rule.type === CSSRule.IMPORT_RULE) {
        const imported = (rule as CSSImportRule).styleSheet;
        if (imported) visitSheet(imported);
      } else if ("cssRules" in rule) {
        visitRules((rule as CSSGroupingRule).cssRules, base);
      }
    }
  };
  const visitSheet = (sheet: CSSStyleSheet) => {
    if (visited.has(sheet)) return;
    visited.add(sheet);
    try { visitRules(sheet.cssRules, sheet.href ?? document.baseURI); } catch { /* Cross-origin stylesheets may be unreadable. */ }
  };
  for (const sheet of Array.from(document.styleSheets)) visitSheet(sheet);
  return { family, faces };
}

/** Load under a private family name; failed or revoked loads never replace the fallback. */
export function applyWidgetSiteFont(font: WidgetSiteFont, onReady: (family: string) => void): () => void {
  if (font.faces.length === 0) {
    onReady(GENERIC_FAMILIES.has(font.family) ? font.family : JSON.stringify(font.family));
    return () => {};
  }
  if (typeof FontFace === "undefined" || !document.fonts) return () => {};
  let active = true;
  const loaded: FontFace[] = [];
  const family = `${"Tz"}SiteFont-${crypto.randomUUID()}`;
  for (const face of font.faces) {
    try {
      const resource = new FontFace(family, `url(${JSON.stringify(face.url)})`, {
        weight: face.weight, style: face.style, unicodeRange: face.unicodeRange, display: "swap",
      });
      void resource.load().then((ready) => {
        if (!active) return;
        document.fonts.add(ready);
        loaded.push(ready);
        onReady(JSON.stringify(family));
      }).catch(() => { /* Keep the selected fallback if the font cannot load. */ });
    } catch { /* Unsupported font descriptors also use the fallback. */ }
  }
  return () => {
    active = false;
    for (const face of loaded) document.fonts.delete(face);
  };
}
