import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { PreferenceDropdown } from "./PreferenceDropdown";

afterEach(cleanup);
const options = [
  { value: "director", label: "Центр управления" },
  { value: "support", label: "Support" },
  { value: "hr", label: "HR" },
  { value: "sales", label: "Sales" },
];

function setup(disabled = false) {
  const onChange = vi.fn();
  render(<PreferenceDropdown label="Отдел" placeholder="Выберите отдел" value="support" options={options} disabled={disabled} onChange={onChange} />);
  return { trigger: screen.getByRole("combobox"), onChange };
}

it("shows the selected department and changes it from the menu", () => {
  const { trigger, onChange } = setup();
  trigger.focus();
  fireEvent.click(trigger);
  expect(screen.getByRole("option", { name: "Support" })).toHaveAttribute("aria-selected", "true");
  fireEvent.click(screen.getByRole("option", { name: "HR" }));
  expect(onChange).toHaveBeenCalledWith("hr");
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
});

it("supports arrows, text search and Escape without committing a cancelled choice", () => {
  const { trigger, onChange } = setup();
  trigger.focus();
  fireEvent.keyDown(trigger, { key: "ArrowDown" });
  fireEvent.keyDown(trigger, { key: "ArrowDown" });
  expect(trigger.getAttribute("aria-activedescendant")).toBe(screen.getByRole("option", { name: "HR" }).id);
  fireEvent.keyDown(trigger, { key: "Escape" });
  expect(onChange).not.toHaveBeenCalled();
  expect(trigger).toHaveFocus();
  fireEvent.keyDown(trigger, { key: "h" });
  fireEvent.keyDown(trigger, { key: "Enter" });
  expect(onChange).toHaveBeenCalledWith("hr");
});

it("closes on an outside click and does not open while disabled", () => {
  const { trigger } = setup();
  fireEvent.click(trigger);
  fireEvent.pointerDown(document.body);
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  cleanup();
  const { trigger: disabledTrigger, onChange } = setup(true);
  fireEvent.click(disabledTrigger);
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  expect(onChange).not.toHaveBeenCalled();
});

it("filters a searchable list, handles empty results and chooses with the keyboard", () => {
  const onChange = vi.fn();
  const onSubmit = vi.fn((event) => event.preventDefault());
  render(<form onSubmit={onSubmit}><PreferenceDropdown label="Отдел" value="support" options={options} onChange={onChange}
    search={{ placeholder: "Поиск отдела", emptyLabel: "Ничего не найдено" }} /></form>);
  const trigger = screen.getByRole("button", { name: "Отдел" });
  fireEvent.click(trigger);
  const search = screen.getByRole("combobox", { name: "Поиск отдела" });
  expect(search).toHaveFocus();
  fireEvent.change(search, { target: { value: "нет такого" } });
  expect(screen.getByRole("status")).toHaveTextContent("Ничего не найдено");
  expect(search).not.toHaveAttribute("aria-activedescendant");
  fireEvent.keyDown(search, { key: "ArrowDown" });
  fireEvent.keyDown(search, { key: "Enter" });
  expect(onChange).not.toHaveBeenCalled();
  expect(onSubmit).not.toHaveBeenCalled();
  fireEvent.keyDown(search, { key: "Escape" });
  expect(trigger).toHaveFocus();
  fireEvent.keyDown(trigger, { key: "ArrowDown" });
  const reopened = screen.getByRole("combobox", { name: "Поиск отдела" });
  expect(reopened).toHaveValue("");
  fireEvent.change(reopened, { target: { value: "ПРАВЛ" } });
  expect(screen.getAllByRole("option")).toHaveLength(1);
  expect(reopened.getAttribute("aria-activedescendant")).toBe(screen.getByRole("option", { name: "Центр управления" }).id);
  fireEvent.keyDown(reopened, { key: "Enter" });
  expect(onChange).toHaveBeenCalledWith("director");
  expect(trigger).toHaveFocus();
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
});

it("opens upward when a dialog leaves too little room below the field", () => {
  const measure = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    if (this.tagName === "DIALOG") return new DOMRect(0, 80, 600, 460);
    if (this.classList.contains("preference-dropdown-trigger")) return new DOMRect(40, 460, 300, 36);
    if (this.classList.contains("preference-dropdown-options")) return new DOMRect(40, 500, 300, 190);
    return new DOMRect();
  });
  try {
    render(<dialog open><PreferenceDropdown label="Отдел" value="support" options={options} onChange={vi.fn()} placement="auto"
      search={{ placeholder: "Поиск отдела", emptyLabel: "Ничего не найдено" }} /></dialog>);
    fireEvent.click(screen.getByRole("button", { name: "Отдел" }));
    const menu = screen.getByRole("listbox").parentElement;
    expect(menu).toHaveAttribute("data-placement", "top");
    fireEvent.change(screen.getByRole("combobox", { name: "Поиск отдела" }), { target: { value: "hr" } });
    expect(menu).toHaveAttribute("data-placement", "top");
  } finally { measure.mockRestore(); }
});
