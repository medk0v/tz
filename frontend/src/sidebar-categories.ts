import type { SidebarCategory } from "./department-api";
import type { MessageKey } from "./i18n";

export const SIDEBAR_CATEGORY_DEFAULTS = [
  { id: "work", label: "nav.group.work", items: ["director", "conversations", "contacts", "tasks", "notes", "processes", "online_visitors"] },
  { id: "quality", label: "nav.group.quality", items: ["support_quality"] },
  { id: "management", label: "nav.group.management", items: ["team", "ai", "inbox_routing", "channels", "knowledge_base", "reply_templates", "integrations"] },
  { id: "administration", label: "nav.group.administration", items: ["departments", "projects", "users", "roles", "access_tokens"] },
] satisfies { id: string; label: MessageKey; items: string[] }[];

export function defaultSidebarCategories(t: (key: MessageKey) => string, director: boolean, workspaceName?: string): SidebarCategory[] {
  return SIDEBAR_CATEGORY_DEFAULTS.map((group) => ({
    id: group.id,
    name: group.id === "work" ? workspaceName || t(group.label) : t(group.label),
    // Existing workspace menus have one working group plus administration until categorized.
    items: (group.id === "work" ? SIDEBAR_CATEGORY_DEFAULTS.filter((category) => category.id !== "administration").flatMap((category) => category.items)
      : group.id === "administration" ? group.items : []).filter((id) => director || id !== "director"),
  }));
}

export function validSidebarCategories(categories: SidebarCategory[] = []): boolean {
  return categories.length <= 32 && categories.every((category) => category.name.trim().length > 0 && category.name.length <= 80);
}

/** Items have already passed access and enabled-page filters; assignments only affect presentation. */
export function categorizeSidebar<Item extends { id: string }>(items: Item[], categories: SidebarCategory[]) {
  return categories.map((category, index) => ({
    ...category,
    items: items.filter((item) => {
      const assigned = categories.find((candidate) => candidate.items.includes(item.id));
      if (assigned) return assigned.id === category.id;
      const fallback = SIDEBAR_CATEGORY_DEFAULTS.find((group) => group.items.includes(item.id));
      const defaultGroup = categories.find((candidate) => candidate.id === fallback?.id);
      return defaultGroup ? defaultGroup.id === category.id : index === 0;
    }),
  }));
}
