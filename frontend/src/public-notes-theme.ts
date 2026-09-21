import { useLayoutEffect } from "react";
import { THEME_COLORS, type Palette, type Theme } from "./admin/preference-options";

/// Public note pages always render in the product's own fixed appearance.
const PALETTE: Palette = "graphite";
const MODE: Theme = "light";

export function usePublicNotesTheme() {
  useLayoutEffect(() => {
    const root = document.documentElement;
    const previous = { palette: root.getAttribute("data-palette"), theme: root.getAttribute("data-theme"), scheme: root.style.colorScheme };
    const existing = document.head.querySelector<HTMLMetaElement>('meta[name="theme-color"]');
    const themeColor = existing ?? document.createElement("meta");
    const previousColor = themeColor.getAttribute("content");
    if (!existing) { themeColor.name = "theme-color"; document.head.append(themeColor); }
    root.dataset.palette = PALETTE;
    root.dataset.theme = MODE;
    root.style.colorScheme = MODE;
    themeColor.content = THEME_COLORS[PALETTE][MODE];
    return () => {
      for (const [name, value] of [["data-palette", previous.palette], ["data-theme", previous.theme]] as const) {
        if (value === null) root.removeAttribute(name); else root.setAttribute(name, value);
      }
      root.style.colorScheme = previous.scheme;
      if (!existing) themeColor.remove();
      else if (previousColor === null) themeColor.removeAttribute("content");
      else themeColor.content = previousColor;
    };
  }, []);

  return { palette: PALETTE, mode: MODE };
}
