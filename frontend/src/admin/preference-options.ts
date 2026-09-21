import { PanelBottom, PanelLeft, PanelRight, PanelTop, type LucideIcon } from "lucide-react";
import type { MessageKey } from "../i18n";
import type { OperatorNotificationSound } from "./operator-notifications";

export type Theme = "light" | "dark";
export const PALETTES = ["ocean", "ink", "graphite", "forest", "plum", "copper", "paper"] as const;
export type Palette = typeof PALETTES[number];
export type PageWidth = "contained" | "full";
export const SIDEBAR_POSITIONS = ["left", "right", "top", "bottom"] as const;
export type SidebarPosition = typeof SIDEBAR_POSITIONS[number];

export function isPalette(value: string | null | undefined): value is Palette {
  return (PALETTES as ReadonlyArray<string | null | undefined>).includes(value);
}

export function isSidebarPosition(value: string | null): value is SidebarPosition {
  return (SIDEBAR_POSITIONS as ReadonlyArray<string | null>).includes(value);
}

export const NOTIFICATION_SOUND_OPTIONS: ReadonlyArray<{
  value: OperatorNotificationSound;
  label: MessageKey;
}> = [
  { value: "glass_chime", label: "notifications.soundGlassChime" },
  { value: "pearl_chime", label: "notifications.soundPearlChime" },
  { value: "warm_keys", label: "notifications.soundWarmKeys" },
  { value: "droplet_chime", label: "notifications.soundDropletChime" },
  { value: "dawn_chime", label: "notifications.soundDawnChime" },
  { value: "bell", label: "notifications.soundBell" },
  { value: "double_bell", label: "notifications.soundDoubleBell" },
  { value: "urgent_bell", label: "notifications.soundUrgentBell" },
  { value: "soft_chime", label: "notifications.soundSoftChime" },
  { value: "crystal_ping", label: "notifications.soundCrystalPing" },
  { value: "rising_chime", label: "notifications.soundRisingChime" },
  { value: "deep_bell", label: "notifications.soundDeepBell" },
];

export const PALETTE_OPTIONS: ReadonlyArray<{
  value: Palette;
  label: MessageKey;
  swatch: readonly [string, string];
}> = [
  { value: "ocean", label: "theme.paletteOcean", swatch: ["#10263a", "#356df3"] },
  { value: "ink", label: "theme.paletteInk", swatch: ["#f4f4f4", "#000000"] },
  { value: "graphite", label: "theme.paletteGraphite", swatch: ["#3b3b3b", "#f6f881"] },
  { value: "forest", label: "theme.paletteForest", swatch: ["#16332b", "#1d9a8c"] },
  { value: "plum", label: "theme.palettePlum", swatch: ["#2b1b3d", "#7a5af5"] },
  { value: "copper", label: "theme.paletteCopper", swatch: ["#3b2a22", "#c8703b"] },
  { value: "paper", label: "theme.palettePaper", swatch: ["#ece5d7", "#2d4a7a"] },
];

export const SIDEBAR_POSITION_OPTIONS: ReadonlyArray<{
  value: SidebarPosition;
  label: MessageKey;
  icon: LucideIcon;
}> = [
  { value: "left", label: "sidebarPosition.left", icon: PanelLeft },
  { value: "right", label: "sidebarPosition.right", icon: PanelRight },
  { value: "top", label: "sidebarPosition.top", icon: PanelTop },
  { value: "bottom", label: "sidebarPosition.bottom", icon: PanelBottom },
];

/** Browser chrome color per palette, kept in sync with the boot script in index.html. */
export const THEME_COLORS: Record<Palette, Record<Theme, string>> = {
  ocean: { light: "#f2f5f9", dark: "#0d1721" },
  ink: { light: "#f4f4f4", dark: "#1e1e1e" },
  graphite: { light: "#f5f5f3", dark: "#1a1a19" },
  forest: { light: "#f1f5f2", dark: "#0e1714" },
  plum: { light: "#f5f3f8", dark: "#151020" },
  copper: { light: "#f7f3ee", dark: "#191410" },
  paper: { light: "#f3eee4", dark: "#1e1b17" },
};
