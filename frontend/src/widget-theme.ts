import type { WidgetTheme } from "./api";
import { isWidgetFontFamily, type WidgetFontFamily } from "./widget-font";

export type WidgetColorScheme = "light" | "dark";

type WidgetBasePalette = Pick<
  WidgetTheme,
  "accent_color" | "accent_text_color" | "surface_color" | "text_color"
>;

type SavedWidgetComponentColors = NonNullable<WidgetTheme["component_colors"]>;
export type WidgetComponentColors = Required<SavedWidgetComponentColors>;

export type WidgetThemePalette = WidgetBasePalette & WidgetComponentColors;

type WidgetDarkTheme = NonNullable<WidgetTheme["dark"]>;
type WidgetBorder = NonNullable<WidgetTheme["border"]>;

type ResolvedWidgetDarkTheme = Omit<WidgetDarkTheme, "component_colors"> & {
  component_colors: WidgetComponentColors;
};

export type ResolvedWidgetTheme = Omit<
  WidgetTheme,
  "component_colors" | "dark" | "border" | "font_family" | "use_site_font" | "reply_typing_effect" | "opening_animation" | "launcher_icon" | "send_icon"
> & {
  launcher_icon: NonNullable<WidgetTheme["launcher_icon"]>;
  send_icon: NonNullable<WidgetTheme["send_icon"]>;
  font_family: WidgetFontFamily;
  use_site_font: boolean;
  reply_typing_effect: boolean;
  opening_animation: boolean;
  component_colors: WidgetComponentColors;
  dark: ResolvedWidgetDarkTheme;
  border: WidgetBorder;
};

function mixHexColors(
  background: string,
  foreground: string,
  foregroundWeight: number,
): string {
  const channels = [1, 3, 5].map((offset) => {
    const backgroundChannel = Number.parseInt(background.slice(offset, offset + 2), 16);
    const foregroundChannel = Number.parseInt(foreground.slice(offset, offset + 2), 16);
    return Math.round(
      backgroundChannel * (1 - foregroundWeight) + foregroundChannel * foregroundWeight,
    ).toString(16).padStart(2, "0");
  });
  return `#${channels.join("")}`.toUpperCase();
}

function legacyComponentColors(
  palette: WidgetBasePalette,
  colorScheme: WidgetColorScheme,
): SavedWidgetComponentColors {
  return {
    background_color: mixHexColors(palette.surface_color, palette.text_color, 0.02),
    control_background_color: palette.surface_color,
    control_text_color: palette.text_color,
    muted_text_color: mixHexColors(palette.surface_color, palette.text_color, 0.62),
    divider_color: mixHexColors(palette.surface_color, palette.text_color, 0.12),
    online_color: "#27A857",
    offline_color: "#8A8F98",
    danger_color: colorScheme === "dark" ? "#FF8B82" : "#A93027",
    rating_color: "#F3B72F",
    shadow_color: "#000000",
  };
}

function resolveComponentColors(
  palette: WidgetBasePalette,
  colorScheme: WidgetColorScheme,
  saved?: SavedWidgetComponentColors,
): WidgetComponentColors {
  const colors = { ...legacyComponentColors(palette, colorScheme), ...saved };
  return {
    ...colors,
    status_text_color: colors.status_text_color ?? colors.muted_text_color,
    launcher_background_color: colors.launcher_background_color ?? palette.accent_color,
    launcher_icon_color: colors.launcher_icon_color ?? palette.accent_text_color,
    input_background_color: colors.input_background_color ?? colors.control_background_color,
    input_text_color: colors.input_text_color ?? colors.control_text_color,
    input_placeholder_color: colors.input_placeholder_color ?? colors.muted_text_color,
    footer_text_color: colors.footer_text_color ?? colors.muted_text_color,
    sound_icon_color: colors.sound_icon_color ?? colors.muted_text_color,
    close_icon_color: colors.close_icon_color ?? colors.muted_text_color,
    send_background_color: colors.send_background_color ?? palette.accent_color,
    send_icon_color: colors.send_icon_color ?? palette.accent_text_color,
  };
}

const DEFAULT_LIGHT_BASE: WidgetBasePalette = {
  accent_color: "#F05A28",
  accent_text_color: "#FFFFFF",
  surface_color: "#FFFFFF",
  text_color: "#1B1B1D",
};

const DEFAULT_DARK_BASE: WidgetDarkTheme = {
  accent_color: "#F05A28",
  accent_text_color: "#FFFFFF",
  surface_color: "#1C1E21",
  text_color: "#F2F3F4",
};

export const DEFAULT_WIDGET_THEME: ResolvedWidgetTheme = {
  ...DEFAULT_LIGHT_BASE,
  component_colors: resolveComponentColors(DEFAULT_LIGHT_BASE, "light"),
  dark: {
    ...DEFAULT_DARK_BASE,
    component_colors: resolveComponentColors(DEFAULT_DARK_BASE, "dark"),
  },
  border: {
    enabled: true,
    width: 1,
    light_color: "#E3E4E6",
    dark_color: "#3A3D42",
  },
  border_radius: 20,
  font_family: "system",
  use_site_font: false,
  reply_typing_effect: false,
  opening_animation: true,
  launcher_icon: "chat",
  send_icon: "send",
  footer_text: null,
};

export function normalizeWidgetColorScheme(
  value: unknown,
): WidgetColorScheme | undefined {
  if (typeof value !== "string") return undefined;
  const normalized = value.trim().toLowerCase();
  return normalized === "light" || normalized === "dark"
    ? normalized
    : undefined;
}

export function normalizeWidgetTheme(
  theme?: WidgetTheme | null,
): ResolvedWidgetTheme {
  const lightBase: WidgetBasePalette = {
    accent_color: theme?.accent_color ?? DEFAULT_WIDGET_THEME.accent_color,
    accent_text_color: theme?.accent_text_color ?? DEFAULT_WIDGET_THEME.accent_text_color,
    surface_color: theme?.surface_color ?? DEFAULT_WIDGET_THEME.surface_color,
    text_color: theme?.text_color ?? DEFAULT_WIDGET_THEME.text_color,
  };
  const darkBase: WidgetDarkTheme = {
    accent_color: theme?.dark?.accent_color ?? DEFAULT_WIDGET_THEME.dark.accent_color,
    accent_text_color:
      theme?.dark?.accent_text_color ?? DEFAULT_WIDGET_THEME.dark.accent_text_color,
    surface_color: theme?.dark?.surface_color ?? DEFAULT_WIDGET_THEME.dark.surface_color,
    text_color: theme?.dark?.text_color ?? DEFAULT_WIDGET_THEME.dark.text_color,
  };
  return {
    ...DEFAULT_WIDGET_THEME,
    ...theme,
    font_family: isWidgetFontFamily(theme?.font_family) ? theme.font_family : "system",
    use_site_font: theme?.use_site_font === true,
    reply_typing_effect: theme?.reply_typing_effect === true,
    opening_animation: theme?.opening_animation !== false,
    launcher_icon: theme?.launcher_icon ?? "chat",
    send_icon: theme?.send_icon ?? "send",
    ...lightBase,
    component_colors: resolveComponentColors(lightBase, "light", theme?.component_colors),
    dark: {
      ...darkBase,
      component_colors: resolveComponentColors(darkBase, "dark", theme?.dark?.component_colors),
    },
    border: {
      ...DEFAULT_WIDGET_THEME.border,
      ...theme?.border,
    },
  };
}

export function widgetThemePalette(
  theme: ResolvedWidgetTheme,
  colorScheme: WidgetColorScheme,
): WidgetThemePalette {
  const palette = colorScheme === "dark" ? theme.dark : theme;
  return {
    accent_color: palette.accent_color,
    accent_text_color: palette.accent_text_color,
    surface_color: palette.surface_color,
    text_color: palette.text_color,
    ...palette.component_colors,
  };
}

export function widgetThemeCssVariables(
  theme: ResolvedWidgetTheme,
  colorScheme: WidgetColorScheme,
): Record<string, string> {
  const palette = widgetThemePalette(theme, colorScheme);
  return {
    "--widget-accent": palette.accent_color,
    "--widget-on-accent": palette.accent_text_color,
    "--widget-surface": palette.surface_color,
    "--widget-text": palette.text_color,
    "--widget-background": palette.background_color,
    "--widget-control": palette.control_background_color,
    "--widget-control-text": palette.control_text_color,
    "--widget-muted": palette.muted_text_color,
    "--widget-divider": palette.divider_color,
    "--widget-online": palette.online_color,
    "--widget-offline": palette.offline_color,
    "--widget-danger": palette.danger_color,
    "--widget-rating": palette.rating_color,
    "--widget-shadow": palette.shadow_color,
    "--widget-radius": `${theme.border_radius}px`,
    "--widget-border-width": theme.border.enabled ? `${theme.border.width}px` : "0px",
    "--widget-border-color": colorScheme === "dark"
      ? theme.border.dark_color
      : theme.border.light_color,
    "--widget-status-text": palette.status_text_color,
    "--widget-launcher-background": palette.launcher_background_color,
    "--widget-launcher-icon": palette.launcher_icon_color,
    "--widget-input-background": palette.input_background_color,
    "--widget-input-text": palette.input_text_color,
    "--widget-input-placeholder": palette.input_placeholder_color,
    "--widget-footer-text": palette.footer_text_color,
    "--widget-sound-icon": palette.sound_icon_color,
    "--widget-close-icon": palette.close_icon_color,
    "--widget-send-background": palette.send_background_color,
    "--widget-send-icon": palette.send_icon_color,
  };
}
