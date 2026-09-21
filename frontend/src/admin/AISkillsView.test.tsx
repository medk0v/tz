import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AiProfile, AiProvider } from "../api";
import * as api from "../ai-skills-api";
import { createI18n, I18nContext, type Locale } from "../i18n";
import { AISkillsView } from "./AISkillsView";
import { DemoReadOnlyContext } from "./DemoReadOnly";
import { CurrentPageRoute, MemoryPageRoute } from "./MemoryPageRoute";
import { usePageRoute } from "./page-route";

vi.mock("../ai-skills-api", () => ({ listAiSkills: vi.fn(), createAiSkill: vi.fn(), updateAiSkill: vi.fn(), deleteAiSkill: vi.fn(), generateAiSkill: vi.fn() }));

const auth = { kind: "session" } as const;
const profiles = [
  { id: "agent-1", name: "Support agent", description: "Support questions" },
  { id: "agent-2", name: "Research agent", description: "Research and sources" },
] as AiProfile[];
const providers = [{ id: "provider-1", name: "Our model", provider_kind: "openai_compatible", default_model: "skill-model", status: "active" }] as AiProvider[];
const skill: api.AiSkill = {
  id: "skill-1", name: "Source review", description: "Check sources", instructions: "Verify the source before answering.", ai_profile_ids: ["agent-1"],
  created_at: "2026-09-11T12:00:00Z", updated_at: "2026-09-11T12:00:00Z",
};

/** Changes the AI page URL from outside the library, as another tab, a link or browser Back/Forward does. */
function RouteLink({ segments }: { segments: string[] }) {
  const route = usePageRoute();
  return <button type="button" onClick={() => void route.navigate(segments)}>{`Open /${segments.join("/")}`}</button>;
}

function show({ locale = "en", demo = false, segments = ["skills"], links = [] }: { locale?: Locale; demo?: boolean; segments?: string[]; links?: string[][] } = {}) {
  return render(<I18nContext.Provider value={createI18n(locale, vi.fn())}><DemoReadOnlyContext.Provider value={demo}><MemoryPageRoute initialSegments={segments}>
    <AISkillsView auth={demo ? { ...auth, isDemo: true } : auth} profiles={profiles} providers={providers} /><CurrentPageRoute />{links.map((link) => <RouteLink key={link.join("/")} segments={link} />)}
  </MemoryPageRoute></DemoReadOnlyContext.Provider></I18nContext.Provider>);
}

const pageRoute = () => screen.getByTestId("page-route").textContent;

async function loaded() {
  await waitFor(() => expect(screen.getByRole("button", { name: "Refresh skills" })).toBeEnabled());
}

describe("AI skills", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.listAiSkills).mockResolvedValue([skill]);
    vi.mocked(api.createAiSkill).mockImplementation(async (_auth, input) => ({ ...skill, ...input, id: "skill-2" }));
    vi.mocked(api.updateAiSkill).mockImplementation(async (_auth, id, input) => ({ ...skill, ...input, id }));
    vi.mocked(api.deleteAiSkill).mockResolvedValue(undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); });

  it("creates a skill with several agents and edits its instructions and assignments", async () => {
    show();
    await loaded();
    fireEvent.click(screen.getByRole("button", { name: "Create skill" }));
    expect(screen.getByRole("button", { name: "Save skill" })).toBeDisabled();
    fireEvent.change(screen.getByRole("textbox", { name: "Skill name" }), { target: { value: " FAQ answers " } });
    fireEvent.change(screen.getByRole("textbox", { name: "Description" }), { target: { value: "Use approved answers" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Skill instructions" }), { target: { value: "Use the FAQ and cite its entries." } });
    fireEvent.click(screen.getByRole("checkbox", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: /Research agent/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save skill" }));
    await screen.findByText("Skill saved.");
    expect(api.createAiSkill).toHaveBeenCalledWith(auth, { name: "FAQ answers", description: "Use approved answers", instructions: "Use the FAQ and cite its entries.", ai_profile_ids: ["agent-1", "agent-2"] });
    fireEvent.change(screen.getByRole("textbox", { name: "Skill instructions" }), { target: { value: "Ask when the FAQ has no answer." } });
    fireEvent.click(screen.getByRole("checkbox", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save skill" }));
    await waitFor(() => expect(api.updateAiSkill).toHaveBeenCalledWith(auth, "skill-2", expect.objectContaining({ instructions: "Ask when the FAQ has no answer.", ai_profile_ids: ["agent-2"] })));
    await screen.findByText("Skill saved.");
    expect(screen.getByRole("checkbox", { name: /Support agent/ })).not.toBeChecked();
  });

  it("imports SKILL.md metadata into an editable draft and preserves the entire instruction file", async () => {
    show();
    await loaded();
    const source = '---\nname: "Source verification"\ndescription: Check original sources\n---\n# Review\nUse references/guide.md.\n```sh\necho example\n```\n';
    fireEvent.change(screen.getByLabelText("Skill file"), { target: { files: [new File([source], "SKILL.md", { type: "text/markdown" })] } });
    await screen.findByText("Instructions imported. Review the skill and save it.");
    expect(screen.getByRole("textbox", { name: "Skill name" })).toHaveValue("Source verification");
    expect(screen.getByRole("textbox", { name: "Description" })).toHaveValue("Check original sources");
    expect(screen.getByRole("textbox", { name: "Skill instructions" })).toHaveValue(source);
    expect(api.createAiSkill).not.toHaveBeenCalled();
    fireEvent.change(screen.getByRole("textbox", { name: "Skill name" }), { target: { value: "Our source verification" } });
    fireEvent.click(screen.getByRole("button", { name: "Save skill" }));
    await screen.findByText("Skill saved.");
    expect(api.createAiSkill).toHaveBeenCalledWith(auth, expect.objectContaining({ name: "Our source verification", instructions: source, ai_profile_ids: [] }));
  });

  it("generates editable instructions from materials and saves only after review with chosen agents", async () => {
    vi.mocked(api.generateAiSkill).mockResolvedValue({ name: "Policy review", description: "Review the company policy", instructions: "Apply the rules from the handbook." });
    show();
    await loaded();
    fireEvent.click(screen.getByRole("button", { name: "Create with AI" }));
    fireEvent.change(screen.getByRole("textbox", { name: "What should the skill do?" }), { target: { value: "Review policy using my handbook" } });
    const book = new File(["handbook"], "handbook.epub");
    fireEvent.change(screen.getByLabelText("Source files", { selector: "input" }), { target: { files: [book] } });
    fireEvent.click(screen.getByRole("button", { name: "Generate instructions" }));
    await screen.findByText("Instructions are ready. Review them, assign agents if needed, and save the skill.");
    expect(api.generateAiSkill).toHaveBeenCalledWith(auth, expect.objectContaining({ files: [book] }), expect.any(AbortSignal));
    expect(screen.getByRole("textbox", { name: "Skill name" })).toHaveValue("Policy review");
    expect(screen.getByRole("textbox", { name: "Skill instructions" })).toHaveValue("Apply the rules from the handbook.");
    expect(api.createAiSkill).not.toHaveBeenCalled();
    fireEvent.change(screen.getByRole("textbox", { name: "Skill instructions" }), { target: { value: "Apply the handbook rules. Ask if the request is unclear." } });
    fireEvent.click(screen.getByRole("checkbox", { name: /Research agent/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save skill" }));
    await screen.findByText("Skill saved.");
    expect(api.createAiSkill).toHaveBeenCalledWith(auth, { name: "Policy review", description: "Review the company policy", instructions: "Apply the handbook rules. Ask if the request is unclear.", ai_profile_ids: ["agent-2"] });
  });

  it("protects both manual drafts and generation materials when switching editors", async () => {
    show();
    await loaded();
    fireEvent.click(screen.getByRole("button", { name: "Create skill" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Skill name" }), { target: { value: "Manual draft" } });
    vi.mocked(window.confirm).mockReturnValueOnce(false);
    fireEvent.click(screen.getByRole("button", { name: "Create with AI" }));
    expect(screen.getByRole("textbox", { name: "Skill name" })).toHaveValue("Manual draft");
    fireEvent.click(screen.getByRole("button", { name: "Create with AI" }));
    fireEvent.change(screen.getByRole("textbox", { name: "What should the skill do?" }), { target: { value: "Keep my generation goal" } });
    vi.mocked(window.confirm).mockReturnValue(false);
    fireEvent.click(screen.getByRole("button", { name: "Create skill" }));
    fireEvent.click(screen.getByRole("button", { name: /Source review/ }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("textbox", { name: "What should the skill do?" })).toHaveValue("Keep my generation goal");
    expect(window.confirm).toHaveBeenLastCalledWith("Discard the unsaved text and files for creating this skill?");
  });

  it("keeps complex frontmatter intact and allows assignment after saving", async () => {
    show();
    await loaded();
    const source = "---\nname: Source check\ndescription: |\n  A multiline description\n  with further details.\n---\nCheck facts.\n";
    fireEvent.change(screen.getByLabelText("Skill file"), { target: { files: [new File([source], "SKILL.md")] } });
    await screen.findByText("Instructions imported. Review the skill and save it.");
    expect(screen.getByRole("textbox", { name: "Description" })).toHaveValue("");
    expect(screen.getByRole("textbox", { name: "Skill instructions" })).toHaveValue(source);
    fireEvent.click(screen.getByRole("button", { name: "Save skill" }));
    await screen.findByText("Skill saved.");
    fireEvent.click(screen.getByRole("checkbox", { name: /Research agent/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save skill" }));
    await waitFor(() => expect(api.updateAiSkill).toHaveBeenCalledWith(auth, "skill-2", expect.objectContaining({ ai_profile_ids: ["agent-2"] })));
    await screen.findByText("Skill saved.");
  });

  it("rejects empty, oversized and unsupported files without replacing the draft", async () => {
    show();
    await loaded();
    fireEvent.click(screen.getByRole("button", { name: /Source review/ }));
    for (const file of [new File(["archive"], "skill.zip"), new File([""], "SKILL.md"), new File(["a".repeat(50001)], "SKILL.md")]) {
      fireEvent.change(screen.getByLabelText("Skill file"), { target: { files: [file] } });
      expect(await screen.findByRole("alert")).toHaveTextContent("Choose a non-empty .md or .txt");
      await waitFor(() => expect(screen.getByRole("button", { name: "Import skill" })).toBeEnabled());
      expect(screen.getByRole("textbox", { name: "Skill instructions" })).toHaveValue(skill.instructions);
    }
    expect(api.createAiSkill).not.toHaveBeenCalled();
  });

  it("retains edited instructions and agent selections after save and refresh failures", async () => {
    show();
    await loaded();
    fireEvent.click(screen.getByRole("button", { name: /Source review/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "Skill instructions" }), { target: { value: "Keep my work" } });
    fireEvent.click(screen.getByRole("checkbox", { name: /Research agent/ }));
    vi.mocked(api.updateAiSkill).mockRejectedValueOnce(new Error("offline"));
    fireEvent.click(screen.getByRole("button", { name: "Save skill" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Your changes are still in the editor");
    vi.mocked(api.listAiSkills).mockRejectedValueOnce(new Error("offline"));
    fireEvent.click(screen.getByRole("button", { name: "Refresh skills" }));
    await screen.findByText("Could not load skills. Try again.");
    expect(screen.getByRole("textbox", { name: "Skill instructions" })).toHaveValue("Keep my work");
    expect(screen.getByRole("checkbox", { name: /Research agent/ })).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await loaded();
    expect(screen.getByRole("textbox", { name: "Skill instructions" })).toHaveValue("Keep my work");
    fireEvent.click(screen.getByRole("button", { name: "Save skill" }));
    await screen.findByText("Skill saved.");
  });

  it("names the skill in delete confirmation, preserves it on cancel or failure, then deletes it", async () => {
    show();
    await loaded();
    fireEvent.click(screen.getByRole("button", { name: /Source review/ }));
    vi.mocked(window.confirm).mockReturnValueOnce(false);
    fireEvent.click(screen.getByRole("button", { name: "Delete skill" }));
    expect(api.deleteAiSkill).not.toHaveBeenCalled();
    expect(window.confirm).toHaveBeenCalledWith("Delete “Source review”? The skill will be removed from all assigned agents.");
    vi.mocked(api.deleteAiSkill).mockRejectedValueOnce(new Error("offline"));
    fireEvent.click(screen.getByRole("button", { name: "Delete skill" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not delete the skill");
    expect(screen.getByRole("textbox", { name: "Skill name" })).toHaveValue(skill.name);
    fireEvent.click(screen.getByRole("button", { name: "Delete skill" }));
    await screen.findByText("Skill deleted and removed from its agents.");
    expect(screen.queryByRole("button", { name: /Source review/ })).not.toBeInTheDocument();
  });

  it("protects unsaved input when another skill or a new draft is selected", async () => {
    show();
    await loaded();
    fireEvent.click(screen.getByRole("button", { name: "Create skill" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Skill name" }), { target: { value: "Draft skill" } });
    vi.mocked(window.confirm).mockReturnValue(false);
    fireEvent.click(screen.getByRole("button", { name: /Source review/ }));
    fireEvent.click(screen.getByRole("button", { name: "Create skill" }));
    expect(screen.getByRole("textbox", { name: "Skill name" })).toHaveValue("Draft skill");
    expect(api.createAiSkill).not.toHaveBeenCalled();
  });

  it("allows demo browsing while disabling imports, creation, changes and deletion", async () => {
    show({ demo: true });
    await loaded();
    expect(screen.getByRole("button", { name: "Create skill" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Create with AI" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Import skill" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: /Source review/ }));
    expect(screen.getByRole("textbox", { name: "Skill instructions" })).toBeDisabled();
    expect(screen.getByRole("checkbox", { name: /Support agent/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save skill" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete skill" })).toBeDisabled();
    expect(api.createAiSkill).not.toHaveBeenCalled();
    expect(api.updateAiSkill).not.toHaveBeenCalled();
    expect(api.deleteAiSkill).not.toHaveBeenCalled();
  });

  it("opens the skill named by the URL and keeps the URL current", async () => {
    show({ segments: ["skills", "skill-1"] });
    expect(await screen.findByRole("textbox", { name: "Skill name" })).toHaveValue("Source review");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(pageRoute()).toBe("skills");
    fireEvent.click(screen.getByRole("button", { name: /Source review/ }));
    expect(pageRoute()).toBe("skills/skill-1");
    fireEvent.click(screen.getByRole("button", { name: "Create skill" }));
    expect(pageRoute()).toBe("skills/new");
    fireEvent.change(screen.getByRole("textbox", { name: "Skill name" }), { target: { value: "FAQ answers" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Skill instructions" }), { target: { value: "Use the FAQ." } });
    fireEvent.click(screen.getByRole("button", { name: "Save skill" }));
    await screen.findByText("Skill saved.");
    expect(pageRoute()).toBe("skills/skill-2");
    expect(screen.getByRole("textbox", { name: "Skill name" })).toHaveValue("FAQ answers");
    fireEvent.click(screen.getByRole("button", { name: "Create with AI" }));
    expect(pageRoute()).toBe("skills");
    expect(screen.getByRole("textbox", { name: "What should the skill do?" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Source review/ }));
    fireEvent.click(screen.getByRole("button", { name: "Delete skill" }));
    await screen.findByText("Skill deleted and removed from its agents.");
    expect(pageRoute()).toBe("skills");
  });

  it("follows links and history between skills and leaves its draft alone on other AI tabs", async () => {
    show({ links: [["skills", "skill-1"], ["skills", "new"], ["skills"], ["agents"]] });
    await loaded();
    fireEvent.click(screen.getByRole("button", { name: "Open /skills/skill-1" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Skill name" }), { target: { value: "Unsaved name" } });
    fireEvent.click(screen.getByRole("button", { name: "Open /agents" }));
    fireEvent.click(screen.getByRole("button", { name: "Open /skills/skill-1" }));
    expect(screen.getByRole("textbox", { name: "Skill name" })).toHaveValue("Unsaved name");
    fireEvent.click(screen.getByRole("button", { name: "Open /skills/new" }));
    expect(screen.getByRole("heading", { name: "New skill" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Skill name" })).toHaveValue("");
    fireEvent.click(screen.getByRole("button", { name: "Open /skills/skill-1" }));
    expect(screen.getByRole("textbox", { name: "Skill name" })).toHaveValue("Source review");
    fireEvent.click(screen.getByRole("button", { name: "Open /skills" }));
    expect(screen.getByText("Select a skill, create one or import a SKILL.md file.")).toBeInTheDocument();
    expect(window.confirm).not.toHaveBeenCalled();
  });

  it.each([[["skills", "missing"], "skills"], [["skills", "skill-1", "extra"], "skills/skill-1"]])("replaces the stale skills URL %j with the skill it shows", async (segments, canonical) => {
    show({ segments });
    await waitFor(() => expect(pageRoute()).toBe(canonical));
  });

  it("ignores the URL of other AI tabs", async () => {
    show({ segments: ["agents", "skill-1"] });
    await loaded();
    expect(screen.queryByRole("textbox", { name: "Skill name" })).not.toBeInTheDocument();
    expect(pageRoute()).toBe("agents/skill-1");
  });

  it.each([ ["ru", "Создать скилл", "Подключить готовый"], ["ro", "Creează abilitate", "Importă abilitate"] ] as const)("shows localized actions in %s", async (locale, create, importLabel) => {
    show({ locale });
    expect(screen.getByRole("button", { name: create })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: importLabel })).toBeInTheDocument();
    await screen.findByRole("button", { name: /Source review/ });
  });
});
