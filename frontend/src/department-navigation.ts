import { categorizeSidebar } from "./sidebar-categories";
import type { ActorContext } from "./api";
import type { Department } from "./department-api";
import type { AdminPage } from "./entry-mode";

export const DEPARTMENT_PAGES: readonly AdminPage[] = [
  "conversations", "contacts", "tasks", "notes", "online_visitors", "support_quality",
  "ai", "inbox_routing", "channels", "knowledge_base", "reply_templates", "integrations",
];

export function departmentStartPage(actor: ActorContext): AdminPage {
  const page = actor.department_default_page;
  return DEPARTMENT_PAGES.includes(page as AdminPage) ? page as AdminPage : "conversations";
}

/** Layout never grants access: filter by the existing role before applying department order. */
export function departmentNavGroups<
  Item extends { id: AdminPage; visible: boolean },
  Group extends { id: string; items: Item[] },
>(groups: Group[], department: Pick<Department, "sidebar_items" | "sidebar_categories"> | null): (Group & { customName?: string })[] {
  const permitted = groups.map((group) => ({ ...group, items: group.items.filter((item) => item.visible) }));
  if (!department) return permitted.filter((group) => group.items.length > 0);
  const items = new Map(permitted.filter((group) => group.id !== "administration")
    .flatMap((group) => group.items.map((item) => [item.id, item] as const)));
  const ordered = [...new Set(department.sidebar_items)]
    .flatMap((id) => {
      const item = items.get(id as AdminPage);
      return item ? [item] : [];
    });
  const legacyGroups = permitted.flatMap((group) => {
    if (group.id === "administration") return group.items.length ? [group] : [];
    return group.id === "work" && ordered.length ? [{ ...group, items: ordered }] : [];
  });
  if (!department.sidebar_categories?.length) return legacyGroups;
  return categorizeSidebar(legacyGroups.flatMap((group) => group.items), department.sidebar_categories)
    .filter((category) => category.items.length > 0)
    .map((category) => ({
      ...(permitted.find((group) => group.id === category.id) ?? permitted[0]),
      id: category.id, items: category.items, customName: category.name,
    }));
}
