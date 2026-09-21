import type { Locale } from "../i18n";

export type AgentProfileTab = "settings" | "instructions" | "knowledge" | "tests";
export const agentProfileTabs: AgentProfileTab[] = ["settings", "instructions", "knowledge", "tests"];
const labels = {
  en: { settings: "Settings", instructions: "Instructions", knowledge: "Knowledge and tools", tests: "Tests" },
  ru: { settings: "Настройки", instructions: "Инструкции", knowledge: "Знания и инструменты", tests: "Тесты" },
  ro: { settings: "Setări", instructions: "Instrucțiuni", knowledge: "Cunoștințe și instrumente", tests: "Teste" },
};
export function agentProfileTabLabel(locale: Locale, tab: AgentProfileTab) { return labels[locale][tab]; }
