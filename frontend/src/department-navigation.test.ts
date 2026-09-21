import { describe, expect, it } from "vitest";
import { departmentNavGroups, departmentStartPage } from "./department-navigation";
import type { ActorContext } from "./api";
import type { Department } from "./department-api";
import type { AdminPage } from "./entry-mode";

const department: Department = {
  id: "support", name: "Support", icon: "support", sidebar_items: ["contacts", "ai", "conversations", "contacts"],
  default_page: "contacts", position: 0, inbox_ids: [], member_count: 1,
};
const groups = [
  { id: "work", items: [{ id: "conversations" as AdminPage, visible: true }, { id: "contacts" as AdminPage, visible: true }] },
  { id: "management", items: [{ id: "ai" as AdminPage, visible: false }, { id: "channels" as AdminPage, visible: true }] },
  { id: "administration", items: [{ id: "departments" as AdminPage, visible: true }] },
];

describe("department navigation", () => {
  it("applies custom order without granting hidden modules or duplicating items", () => {
    expect(departmentNavGroups(groups, department).map((group) => group.items.map((item) => item.id)))
      .toEqual([["contacts", "conversations"], ["departments"]]);
  });

  it("keeps project settings reachable even when the department has no permitted modules", () => {
    expect(departmentNavGroups(groups, { ...department, sidebar_items: ["ai"] }).map((group) => group.id))
      .toEqual(["administration"]);
  });

  it("uses saved categories without enabling hidden or disabled pages and keeps administration reachable", () => {
    const sidebar_categories = [
      { id: "favorites", name: "Favorites", items: ["contacts", "ai", "channels"] },
      { id: "work", name: "My work", items: ["conversations"] },
      { id: "administration", name: "Configuration", items: [] },
      { id: "empty", name: "Empty", items: [] },
    ];
    const result = departmentNavGroups(groups, { ...department, sidebar_categories });
    expect(result.map((group) => [group.customName, group.items.map((item) => item.id)]))
      .toEqual([["Favorites", ["contacts"]], ["My work", ["conversations"]], ["Configuration", ["departments"]]]);
    expect(departmentNavGroups(groups, { ...department, sidebar_categories: [] })).toEqual(departmentNavGroups(groups, department));
  });

  it("keeps newly enabled pages visible even without a saved category assignment", () => {
    const result = departmentNavGroups(groups, { sidebar_items: ["channels", "conversations"], sidebar_categories: [{ id: "custom", name: "Custom", items: ["conversations"] }] });
    expect(result[0].items.map((item) => item.id)).toEqual(["channels", "conversations", "departments"]);
  });

  it.each(["notes"] as const)("allows %s as the landing page while retaining role and menu filtering", (page) => {
    expect(departmentStartPage({ department_default_page: page } as ActorContext)).toBe(page);
    const processGroups = [{ id: "work", items: [{ id: page as AdminPage, visible: false }] }];
    const processDepartment = { ...department, sidebar_items: [page] };
    expect(departmentNavGroups(processGroups, processDepartment)).toEqual([]);
    processGroups[0].items[0].visible = true;
    expect(departmentNavGroups(processGroups, processDepartment)[0].items[0].id).toBe(page);
    expect(departmentNavGroups(processGroups, department)).toEqual([]);
  });
});
