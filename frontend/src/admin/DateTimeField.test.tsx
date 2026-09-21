import { useState } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_DATE_TIME_PREFERENCES, DateTimePreferencesContext, dateTimePreferences, type DateTimePreferences } from "../date-time-preferences";
import { DateTimeField, type DateTimeFieldProps } from "./DateTimeField";
import { changeDateTimeField, dateTimeFieldInput } from "./date-time-field-test-utils";

type HarnessProps = Partial<Omit<DateTimeFieldProps, "value" | "mode">> & { mode: DateTimeFieldProps["mode"]; initial?: string; preferences?: DateTimePreferences };

/** Pinned to a 24-hour clock and Monday weeks unless a test is about the preferences themselves. */
function Harness({ initial = "", onChange, locale = "en", preferences = dateTimePreferences("24h", "monday"), ...props }: HarnessProps) {
  const [value, setValue] = useState(initial);
  const labelled = props["aria-label"] || props["aria-labelledby"] ? {} : { "aria-label": "Due date" };
  return <DateTimePreferencesContext.Provider value={preferences}>
    <DateTimeField {...labelled} {...props} locale={locale} value={value} onChange={(next) => { onChange?.(next); setValue(next); }} />
  </DateTimePreferencesContext.Provider>;
}

const trigger = (name = "Due date") => screen.getByLabelText(name);
const day = (name: string) => screen.getByRole("button", { name });
const focused = () => document.activeElement as HTMLElement;
const press = (key: string, init: KeyboardEventInit = {}) => fireEvent.keyDown(focused(), { key, ...init });

/**
 * Types into whatever holds focus the way a browser does: the key goes in at the caret or replaces the selection, and a box
 * that already holds maxLength characters with nothing selected swallows it. fireEvent.change on its own bypasses all of that.
 */
function typeKeys(keys: string) {
  for (const key of keys) {
    const input = focused() as HTMLInputElement;
    if (!fireEvent.keyDown(input, { key })) continue;
    const start = input.selectionStart ?? input.value.length;
    const next = input.value.slice(0, start) + key + input.value.slice(input.selectionEnd ?? start);
    if (input.maxLength < 0 || next.length <= input.maxLength) fireEvent.input(input, { target: { value: next }, data: key, inputType: "insertText" });
  }
}

/** A whole pointer press in the order a browser fires it: mousedown moves focus, unless prevented, long before the click lands. */
function pointerPress(element: HTMLElement) {
  fireEvent.pointerDown(element);
  if (fireEvent.mouseDown(element)) act(() => element.focus());
  fireEvent.mouseUp(element);
  fireEvent.click(element);
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 8, 19, 12, 0));
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("DateTimeField trigger", () => {
  it("exposes one labelled control however the label is attached", () => {
    render(<>
      <Harness mode="datetime" initial="2026-09-19T15:30" />
      <div><span id="move-label">Move to</span><Harness mode="datetime" aria-labelledby="move-label" aria-describedby="move-help" /><span id="move-help">Local time</span></div>
      <label><span>Date</span><Harness mode="date" aria-label={undefined} initial="2026-09-19" /></label>
      <label><span>Starts</span><Harness mode="date" aria-label={undefined} required initial="2026-09-20" /></label>
    </>);
    expect(screen.getAllByLabelText("Due date")).toHaveLength(1);
    expect(trigger()).toHaveClass("date-time-field-trigger");
    expect(trigger()).toHaveTextContent("Sep 19, 2026, 15:30");
    expect(trigger()).toHaveAttribute("aria-haspopup", "dialog");
    expect(trigger()).toHaveAttribute("aria-expanded", "false");
    expect(screen.getAllByLabelText("Move to")).toEqual([screen.getByRole("button", { name: "Move to" })]);
    expect(trigger("Move to")).toHaveTextContent("Select date and time");
    expect(trigger("Move to")).toHaveAccessibleDescription("Local time Select date and time");
    expect(screen.getAllByLabelText("Date")).toHaveLength(1);
    expect(dateTimeFieldInput(trigger("Date"))).toHaveValue("2026-09-19");
    // What says "required" is part of neither the caption of a wrapping label nor the name of the trigger.
    expect(screen.getAllByLabelText("Starts")).toEqual([screen.getByRole("button", { name: "Starts" })]);
    expect(trigger("Starts")).toHaveAccessibleDescription("Sep 20, 2026 Required.");
    expect(trigger("Starts").querySelector("[hidden]")).toHaveTextContent("Required.");
    expect(dateTimeFieldInput(trigger())).toHaveValue("2026-09-19T15:30");
    expect(dateTimeFieldInput(trigger())).toHaveAttribute("aria-hidden", "true");
    expect(dateTimeFieldInput(trigger())).toHaveAttribute("tabindex", "-1");
  });

  it("takes focus from a click on the caption that labels it", () => {
    render(<div><span id="move-label">Move to</span><Harness mode="datetime" aria-labelledby="move-label" /></div>);
    fireEvent.click(screen.getByText("Move to"));
    expect(trigger("Move to")).toHaveFocus();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("forwards id, name and className and shows a custom placeholder", () => {
    const { container } = render(<Harness mode="time" id="starts" name="starts_at" className="compact" placeholder="Any time" />);
    expect(container.querySelector(".date-time-field")).toHaveClass("compact");
    expect(container.querySelector(".date-time-field")).toHaveAttribute("data-mode", "time");
    expect(trigger()).toHaveAttribute("id", "starts");
    expect(trigger()).toHaveTextContent("Any time");
    expect(dateTimeFieldInput(trigger())).toHaveAttribute("name", "starts_at");
  });

  it("renders an unparsable value as empty", () => {
    render(<Harness mode="date" initial="19.09.2026" />);
    expect(trigger()).toHaveTextContent("Select date");
    expect(dateTimeFieldInput(trigger())).toHaveValue("");
    expect(trigger()).toBeValid();
    expect(screen.queryByRole("button", { name: "Clear" })).not.toBeInTheDocument();
  });

  it("never opens while disabled", () => {
    render(<Harness mode="date" disabled initial="2026-09-19" />);
    expect(trigger()).toBeDisabled();
    fireEvent.click(trigger());
    fireEvent.keyDown(trigger(), { key: "Enter" });
    fireEvent.keyDown(trigger(), { key: "ArrowDown" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(dateTimeFieldInput(trigger())).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Clear" })).not.toBeInTheDocument();
  });

  it("reports no error while disabled", () => {
    function Form() {
      const [busy, setBusy] = useState(false);
      return <><Harness mode="datetime" disabled={busy} min="2026-09-19T12:00" initial="2026-09-17T15:30" /><button type="button" onClick={() => setBusy(!busy)}>Toggle</button></>;
    }
    render(<Form />);
    expect(trigger()).toBeInvalid();
    expect(screen.getByText("Choose Sep 19, 2026, 12:00 or later.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Toggle" }));
    expect(trigger()).toBeDisabled();
    expect(trigger()).not.toHaveAttribute("aria-invalid");
    expect(trigger().closest(".date-time-field")).toHaveAttribute("data-invalid", "false");
    expect(screen.queryByText(/or later/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Toggle" }));
    expect(trigger()).toBeInvalid();
    // The error it was mounted with comes back as quietly as it first showed.
    expect(screen.getByText("Choose Sep 19, 2026, 12:00 or later.")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("follows a disabled fieldset the way a native input does", async () => {
    function Form() {
      const [busy, setBusy] = useState(false);
      return <>
        <fieldset disabled={busy}><Harness mode="date" initial="2026-09-01" /></fieldset>
        <fieldset disabled><legend><Harness mode="date" aria-label="In the legend" initial="2026-09-01" /></legend><Harness mode="date" aria-label="Locked" min="2026-09-19" initial="2026-09-01" /></fieldset>
        <button type="button" onClick={() => setBusy(!busy)}>Toggle</button>
      </>;
    }
    render(<Form />);
    const root = (name?: string) => trigger(name).closest(".date-time-field");
    expect(root("Locked")).toHaveAttribute("data-disabled", "true");
    expect(trigger("Locked")).not.toHaveAttribute("aria-invalid");
    expect(root("In the legend")).toHaveAttribute("data-disabled", "false");
    expect(root()).toHaveAttribute("data-disabled", "false");
    expect(screen.getAllByRole("button", { name: "Clear" })).toHaveLength(2);
    fireEvent.click(trigger());
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Toggle" }));
    await waitFor(() => expect(root()).toHaveAttribute("data-disabled", "true"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Clear" })).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Toggle" }));
    await waitFor(() => expect(root()).toHaveAttribute("data-disabled", "false"));
    expect(screen.getAllByRole("button", { name: "Clear" })).toHaveLength(2);
  });
});

describe("DateTimeField popover", () => {
  it("opens from the pointer and the keyboard and closes on a second click", () => {
    render(<Harness mode="date" />);
    fireEvent.click(trigger());
    const dialog = screen.getByRole("dialog", { name: "Choose date" });
    expect(dialog).not.toHaveAttribute("popover");
    expect(dialog.closest(".date-time-field")).toContainElement(trigger());
    expect(trigger()).toHaveAttribute("aria-expanded", "true");
    expect(trigger()).toHaveAttribute("aria-controls", dialog.id);
    expect(day("19 September 2026")).toHaveFocus();
    expect(day("19 September 2026")).toHaveAttribute("aria-current", "date");
    fireEvent.click(trigger());
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    for (const key of ["Enter", " ", "ArrowDown"]) {
      fireEvent.keyDown(trigger(), { key });
      expect(screen.getByRole("dialog")).toBeInTheDocument();
      press("Escape");
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    }
  });

  it("keeps Escape away from an enclosing dialog and returns focus to the trigger", () => {
    const onKeyDown = vi.fn();
    const onDocumentKeyDown = vi.fn();
    document.addEventListener("keydown", onDocumentKeyDown);
    render(<div onKeyDown={onKeyDown}><Harness mode="date" /></div>);
    fireEvent.click(trigger());
    expect(press("Escape")).toBe(false);
    expect(onKeyDown).not.toHaveBeenCalled();
    expect(onDocumentKeyDown).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger()).toHaveFocus();
    expect(fireEvent.keyDown(trigger(), { key: "Escape" })).toBe(true);
    expect(onKeyDown).toHaveBeenCalledOnce();
    document.removeEventListener("keydown", onDocumentKeyDown);
  });

  it("closes on an outside press or when focus leaves, not on a press inside", () => {
    render(<><Harness mode="datetime" /><button type="button">Elsewhere</button></>);
    fireEvent.click(trigger());
    fireEvent.pointerDown(screen.getByRole("grid"));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.click(trigger());
    fireEvent.focusOut(focused(), { relatedTarget: screen.getByRole("button", { name: "Elsewhere" }) });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.click(trigger());
    fireEvent.focusOut(focused(), { relatedTarget: null });
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("cycles Tab inside the popover", () => {
    render(<Harness mode="datetime" initial="2026-09-19T15:30" />);
    fireEvent.click(trigger());
    const dialog = screen.getByRole("dialog", { name: "Choose date and time" });
    const done = within(dialog).getByRole("button", { name: "Done" });
    done.focus();
    expect(press("Tab")).toBe(false);
    expect(within(dialog).getByRole("button", { name: "Previous month" })).toHaveFocus();
    expect(press("Tab", { shiftKey: true })).toBe(false);
    expect(done).toHaveFocus();
    day("19 September 2026").focus();
    expect(press("Tab")).toBe(true);
  });

  it("shows itself in the top layer next to the trigger when the Popover API exists", () => {
    const showPopover = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "showPopover", { configurable: true, value: showPopover });
    try {
      render(<Harness mode="date" />);
      vi.spyOn(trigger(), "getBoundingClientRect").mockReturnValue({ top: 100, bottom: 144, left: 40, right: 340, width: 300, height: 44, x: 40, y: 100, toJSON: () => ({}) });
      fireEvent.click(trigger());
      // jsdom hides [popover] elements it cannot open, so the role query would not see this one.
      const popover = document.querySelector<HTMLElement>(".date-time-field-popover")!;
      expect(showPopover).toHaveBeenCalledOnce();
      expect(showPopover.mock.contexts[0]).toBe(popover);
      expect(popover).toHaveAttribute("popover", "manual");
      expect(popover).toHaveStyle({ position: "fixed", top: "148px", left: "40px" });
      expect(popover).toHaveAttribute("data-placement", "bottom");
      expect(popover.parentElement).toHaveClass("date-time-field");
    } finally {
      Reflect.deleteProperty(HTMLElement.prototype, "showPopover");
    }
  });

  it("scrolls a popover too tall for the viewport to the day the keyboard moved to, clear of the pinned footer", () => {
    render(<Harness mode="date" />);
    fireEvent.click(trigger());
    const popover = screen.getByRole("dialog");
    const rect = (top: number, bottom: number) => ({ top, bottom, left: 0, right: 0, width: 0, height: bottom - top, x: 0, y: top, toJSON: () => ({}) });
    Object.defineProperties(popover, { clientHeight: { configurable: true, value: 200 }, scrollHeight: { configurable: true, value: 361 } });
    vi.spyOn(popover, "getBoundingClientRect").mockReturnValue(rect(0, 200));
    vi.spyOn(popover.querySelector(".date-time-field-footer")!, "getBoundingClientRect").mockReturnValue(rect(147, 200));
    vi.spyOn(day("26 September 2026"), "getBoundingClientRect").mockReturnValue(rect(230, 266));
    vi.spyOn(day("19 September 2026"), "getBoundingClientRect").mockReturnValue(rect(-30, 6));
    const focus = vi.spyOn(day("26 September 2026"), "focus");
    press("ArrowDown");
    // The page and an enclosing dialog still must not scroll; only the popover follows.
    expect(focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
    expect(popover.scrollTop).toBe(266 - (147 - 4));
    press("ArrowUp");
    expect(day("19 September 2026")).toHaveFocus();
    expect(popover.scrollTop).toBe(266 - (147 - 4) - (4 + 30));
  });

  it("leaves a popover that fits alone when focus moves", () => {
    render(<Harness mode="date" />);
    fireEvent.click(trigger());
    press("ArrowDown");
    expect(day("26 September 2026")).toHaveFocus();
    expect(screen.getByRole("dialog").scrollTop).toBe(0);
  });

  it("does not let a wrapping label forward clicks inside the popover to the trigger", () => {
    render(<label><span>Date</span><Harness mode="date" aria-label={undefined} /></label>);
    fireEvent.click(trigger("Date"));
    fireEvent.click(screen.getByRole("columnheader", { name: "Monday" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});

describe("DateTimeField calendar", () => {
  it("emits YYYY-MM-DD and closes when a day is picked in date mode", () => {
    const onChange = vi.fn();
    render(<Harness mode="date" onChange={onChange} />);
    fireEvent.click(trigger());
    expect(screen.getByRole("button", { name: "September 2026" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Done" })).not.toBeInTheDocument();
    fireEvent.click(day("25 September 2026"));
    expect(onChange).toHaveBeenCalledExactlyOnceWith("2026-09-25");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger()).toHaveFocus();
    expect(trigger()).toHaveTextContent("Sep 25, 2026");
    fireEvent.click(trigger());
    expect(day("25 September 2026")).toHaveFocus();
    expect(screen.getByRole("gridcell", { selected: true })).toContainElement(day("25 September 2026"));
  });

  it("navigates and selects days of the adjacent months", () => {
    const onChange = vi.fn();
    render(<Harness mode="date" onChange={onChange} />);
    fireEvent.click(trigger());
    expect(day("31 August 2026")).toHaveAttribute("data-outside", "true");
    fireEvent.click(screen.getByRole("button", { name: "Next month" }));
    expect(screen.getByRole("button", { name: "October 2026" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Previous month" }));
    fireEvent.click(day("2 October 2026"));
    expect(onChange).toHaveBeenCalledExactlyOnceWith("2026-10-02");
  });

  it("keeps the popover open in datetime mode, defaults to 09:00 and clamps to min", () => {
    const onChange = vi.fn();
    render(<Harness mode="datetime" min="2026-09-19T15:32" onChange={onChange} />);
    fireEvent.click(trigger());
    fireEvent.click(day("21 September 2026"));
    expect(onChange).toHaveBeenLastCalledWith("2026-09-21T09:00");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(day("21 September 2026")).toHaveFocus();
    fireEvent.click(day("19 September 2026"));
    expect(onChange).toHaveBeenLastCalledWith("2026-09-19T15:35");
    expect(within(screen.getByRole("listbox", { name: "Hours" })).getByRole("option", { name: "14" })).toHaveAttribute("aria-disabled", "true");
    expect(within(screen.getByRole("listbox", { name: "Minutes" })).getByRole("option", { name: "30" })).toHaveAttribute("aria-disabled", "true");
    expect(within(screen.getByRole("listbox", { name: "Minutes" })).getByRole("option", { name: "35" })).toHaveAttribute("aria-selected", "true");
    fireEvent.click(within(screen.getByRole("listbox", { name: "Hours" })).getByRole("option", { name: "14" }));
    expect(onChange).toHaveBeenCalledTimes(2);
    fireEvent.click(within(screen.getByRole("listbox", { name: "Hours" })).getByRole("option", { name: "17" }));
    expect(onChange).toHaveBeenLastCalledWith("2026-09-19T17:35");
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger()).toHaveFocus();
    expect(trigger()).toHaveTextContent("Sep 19, 2026, 17:35");
  });

  it("preserves incoming seconds on every emit and never invents them", () => {
    const onChange = vi.fn();
    render(<Harness mode="datetime" initial="2026-09-19T15:30:45" onChange={onChange} />);
    fireEvent.click(trigger());
    fireEvent.click(day("22 September 2026"));
    expect(onChange).toHaveBeenLastCalledWith("2026-09-22T15:30:45");
    fireEvent.click(within(screen.getByRole("listbox", { name: "Minutes" })).getByRole("option", { name: "10" }));
    expect(onChange).toHaveBeenLastCalledWith("2026-09-22T15:10:45");
    fireEvent.click(screen.getByRole("button", { name: "Now" }));
    expect(onChange).toHaveBeenLastCalledWith("2026-09-19T12:00:45");
  });

  it("moves through the grid with the keyboard, across months and years", () => {
    const onChange = vi.fn();
    render(<Harness mode="date" onChange={onChange} />);
    trigger().focus();
    press("Enter");
    expect(day("19 September 2026")).toHaveFocus();
    expect(day("19 September 2026")).toHaveAttribute("tabindex", "0");
    expect(day("20 September 2026")).toHaveAttribute("tabindex", "-1");
    expect(press("ArrowRight")).toBe(false);
    expect(day("20 September 2026")).toHaveFocus();
    press("ArrowLeft");
    press("ArrowDown");
    expect(day("26 September 2026")).toHaveFocus();
    press("ArrowDown");
    expect(screen.getByRole("button", { name: "October 2026" })).toBeInTheDocument();
    expect(day("3 October 2026")).toHaveFocus();
    press("Home");
    expect(day("28 September 2026")).toHaveFocus();
    expect(screen.getByRole("button", { name: "September 2026" })).toBeInTheDocument();
    press("End");
    expect(day("4 October 2026")).toHaveFocus();
    press("ArrowUp");
    expect(day("27 September 2026")).toHaveFocus();
    press("PageDown");
    expect(day("27 October 2026")).toHaveFocus();
    press("PageUp");
    press("PageUp");
    expect(day("27 August 2026")).toHaveFocus();
    press("PageDown", { shiftKey: true });
    expect(screen.getByRole("button", { name: "August 2027" })).toBeInTheDocument();
    expect(day("27 August 2027")).toHaveFocus();
    press("PageUp", { shiftKey: true });
    expect(onChange).not.toHaveBeenCalled();
    press("Enter");
    expect(onChange).toHaveBeenCalledExactlyOnceWith("2026-08-27");
    expect(trigger()).toHaveFocus();
    press(" ");
    press(" ");
    expect(onChange).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("disables days outside min and max", () => {
    const onChange = vi.fn();
    render(<Harness mode="date" min="2026-09-10" max="2026-09-25" onChange={onChange} />);
    fireEvent.click(trigger());
    expect(day("9 September 2026")).toHaveAttribute("aria-disabled", "true");
    expect(day("10 September 2026")).not.toHaveAttribute("aria-disabled");
    expect(day("25 September 2026")).not.toHaveAttribute("aria-disabled");
    expect(day("26 September 2026")).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("button", { name: "Previous month" })).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("button", { name: "Next month" })).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(screen.getByRole("button", { name: "Next month" }));
    expect(screen.getByRole("button", { name: "September 2026" })).toBeInTheDocument();
    fireEvent.click(day("26 September 2026"));
    day("9 September 2026").focus();
    press("Enter");
    expect(onChange).not.toHaveBeenCalled();
    day("18 September 2026").focus();
    press("ArrowRight");
    expect(day("19 September 2026")).toHaveFocus();
    press("ArrowUp");
    press("ArrowUp");
    expect(day("5 September 2026")).toHaveFocus();
    press(" ");
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Today" })).toBeEnabled();
    fireEvent.click(day("10 September 2026"));
    expect(onChange).toHaveBeenCalledExactlyOnceWith("2026-09-10");
  });

  it("starts on the nearest allowed day when today is outside the limits", () => {
    render(<Harness mode="date" min="2026-11-05" />);
    fireEvent.click(trigger());
    expect(screen.getByRole("button", { name: "November 2026" })).toBeInTheDocument();
    expect(day("5 November 2026")).toHaveFocus();
    expect(screen.getByRole("button", { name: "Today" })).toBeDisabled();
  });

  it("jumps months and years through the chooser", () => {
    const onChange = vi.fn();
    render(<Harness mode="date" max="2027-06-30" onChange={onChange} />);
    fireEvent.click(trigger());
    const title = screen.getByRole("button", { name: "September 2026" });
    expect(title).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(title);
    expect(screen.queryByRole("grid")).not.toBeInTheDocument();
    const months = screen.getByRole("group", { name: "Choose month and year" });
    expect(within(months).getAllByRole("button")).toHaveLength(12);
    expect(within(months).getByRole("button", { name: "September 2026" })).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByRole("button", { name: "Next year" }));
    expect(screen.getByRole("button", { name: "2027", expanded: true })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next year" })).toHaveAttribute("aria-disabled", "true");
    expect(within(months).getByRole("button", { name: "July 2027" })).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(within(months).getByRole("button", { name: "July 2027" }));
    expect(screen.queryByRole("grid")).not.toBeInTheDocument();
    act(() => within(months).getByRole("button", { name: "September 2027" }).focus());
    press("ArrowUp");
    expect(within(months).getByRole("button", { name: "June 2027" })).toHaveFocus();
    press("ArrowLeft");
    press("Home");
    expect(within(months).getByRole("button", { name: "January 2027" })).toHaveFocus();
    fireEvent.click(within(months).getByRole("button", { name: "March 2027" }));
    expect(screen.getByRole("button", { name: "March 2027" })).toHaveAttribute("aria-expanded", "false");
    expect(day("19 March 2027")).toHaveFocus();
    fireEvent.click(day("8 March 2027"));
    expect(onChange).toHaveBeenCalledExactlyOnceWith("2027-03-08");
  });

  it("keeps one tab stop in the month chooser, on the month the arrow keys reached", () => {
    render(<Harness mode="date" required min="2026-10-01" initial="2026-10-05" />);
    fireEvent.click(trigger());
    fireEvent.click(screen.getByRole("button", { name: "October 2026" }));
    const months = screen.getByRole("group", { name: "Choose month and year" });
    const month = (name: string) => within(months).getByRole("button", { name });
    const stops = () => within(months).getAllByRole("button").filter((button) => button.tabIndex === 0).map((button) => button.getAttribute("aria-label"));
    expect(stops()).toEqual(["October 2026"]);
    act(() => month("October 2026").focus());
    press("ArrowRight");
    expect(month("November 2026")).toHaveFocus();
    expect(stops()).toEqual(["November 2026"]);
    // Browsing moves neither the shown month nor the pressed state.
    expect(month("October 2026")).toHaveAttribute("aria-pressed", "true");
    press("PageDown");
    expect(month("November 2027")).toHaveFocus();
    expect(stops()).toEqual(["November 2027"]);
    // Today is disabled here, which makes the focused month the last stop of the popover: Tab wraps instead of leaving it.
    expect(press("Tab")).toBe(false);
    expect(screen.getByRole("button", { name: "Previous year" })).toHaveFocus();
    expect(stops()).toEqual(["October 2027"]);
  });

  it("renders Monday-first Russian weekday and month names", () => {
    render(<Harness mode="datetime" locale="ru" initial="2026-09-19T15:30" />);
    expect(trigger()).toHaveTextContent("19 сент. 2026, 15:30");
    fireEvent.click(trigger());
    expect(screen.getByRole("dialog", { name: "Выбор даты и времени" })).toBeInTheDocument();
    expect(screen.getAllByRole("columnheader").map((header) => header.textContent)).toEqual(["пн", "вт", "ср", "чт", "пт", "сб", "вс"]);
    expect(screen.getByRole("columnheader", { name: "понедельник" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Сентябрь 2026" })).toBeInTheDocument();
    expect(day("19 сентября 2026")).toHaveFocus();
    expect(within(screen.getAllByRole("row")[1]).getAllByRole("button")[0]).toHaveAccessibleName("31 августа 2026");
    expect(screen.getByRole("button", { name: "Сейчас" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Готово" })).toBeInTheDocument();
  });
});

describe("DateTimeField time", () => {
  it("accepts typed hours and minutes", () => {
    const onChange = vi.fn();
    render(<form onSubmit={onChange}><Harness mode="time" onChange={onChange} /></form>);
    trigger().focus();
    press("Enter");
    expect(screen.getByRole("dialog", { name: "Choose time" })).toBeInTheDocument();
    const hours = screen.getByRole("textbox", { name: "Hours" });
    const minutes = screen.getByRole("textbox", { name: "Minutes" });
    expect(hours).toHaveFocus();
    expect(hours).toHaveAttribute("inputmode", "numeric");
    fireEvent.change(hours, { target: { value: "1" } });
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.change(hours, { target: { value: "14" } });
    expect(onChange).toHaveBeenLastCalledWith("14:00");
    expect(minutes).toHaveFocus();
    fireEvent.change(minutes, { target: { value: "x7" } });
    expect(minutes).toHaveValue("7");
    expect(press("Enter")).toBe(false);
    expect(onChange).toHaveBeenLastCalledWith("14:07");
    expect(minutes).toHaveValue("07");
    press("ArrowUp");
    expect(onChange).toHaveBeenLastCalledWith("14:10");
    press("ArrowDown");
    press("ArrowDown");
    expect(onChange).toHaveBeenLastCalledWith("14:00");
    fireEvent.change(hours, { target: { value: "9" } });
    fireEvent.blur(hours);
    expect(onChange).toHaveBeenLastCalledWith("09:00");
    fireEvent.change(hours, { target: { value: "77" } });
    expect(onChange).toHaveBeenLastCalledWith("23:00");
    hours.focus();
    press("ArrowUp");
    expect(onChange).toHaveBeenLastCalledWith("00:00");
    expect(onChange.mock.calls.map(([value]) => value)).toEqual(["14:00", "14:07", "14:10", "14:05", "14:00", "09:00", "23:00", "00:00"]);
    expect(trigger()).toHaveTextContent("00:00");
  });

  it("keeps a focused box ready for the next digits after its value was written for it", () => {
    const onChange = vi.fn();
    render(<Harness mode="time" onChange={onChange} />);
    trigger().focus();
    press("Enter");
    const hours = screen.getByRole<HTMLInputElement>("textbox", { name: "Hours" });
    const minutes = screen.getByRole<HTMLInputElement>("textbox", { name: "Minutes" });
    const selection = (input: HTMLInputElement) => [input.selectionStart, input.selectionEnd];
    fireEvent.change(hours, { target: { value: "1" } });
    fireEvent.change(hours, { target: { value: "14" } });
    // Auto-advance lands in a box React fills only afterwards: it is selected again, like any box that takes focus.
    expect(minutes).toHaveFocus();
    expect(minutes).toHaveValue("00");
    expect(selection(minutes)).toEqual([0, 2]);
    press("ArrowUp");
    expect(minutes).toHaveValue("05");
    expect(selection(minutes)).toEqual([0, 2]);
    fireEvent.change(minutes, { target: { value: "7" } });
    expect(selection(minutes)).toEqual([1, 1]);
    press("Enter");
    expect(minutes).toHaveValue("07");
    expect(selection(minutes)).toEqual([0, 2]);
    fireEvent.change(minutes, { target: { value: "30" } });
    expect(onChange).toHaveBeenLastCalledWith("14:30");
    expect(selection(minutes)).toEqual([0, 2]);
    // The box that is not focused is left alone.
    expect(selection(hours)).toEqual([2, 2]);
  });

  it("takes a time typed straight through, and again after the arrow keys changed it", () => {
    const onChange = vi.fn();
    render(<Harness mode="time" onChange={onChange} />);
    trigger().focus();
    press("Enter");
    const hours = screen.getByRole("textbox", { name: "Hours" });
    const minutes = screen.getByRole("textbox", { name: "Minutes" });
    typeKeys("1430");
    expect(minutes).toHaveFocus();
    expect(hours).toHaveValue("14");
    expect(minutes).toHaveValue("30");
    // The box is full, yet all of it is selected: typing starts over instead of being swallowed.
    typeKeys("45");
    press("ArrowUp");
    typeKeys("07");
    act(() => hours.focus());
    press("ArrowDown");
    typeKeys("0905");
    expect(minutes).toHaveFocus();
    expect(onChange.mock.calls.map(([value]) => value)).toEqual(["14:00", "14:30", "14:45", "14:50", "14:07", "13:07", "09:07", "09:05"]);
    expect(trigger()).toHaveTextContent("09:05");
  });

  it("takes a typed time over the one a datetime already has", () => {
    const onChange = vi.fn();
    render(<Harness mode="datetime" initial="2026-09-21T09:00" onChange={onChange} />);
    fireEvent.click(trigger());
    act(() => screen.getByRole("textbox", { name: "Hours" }).focus());
    typeKeys("1645");
    expect(onChange.mock.calls.map(([value]) => value)).toEqual(["2026-09-21T16:00", "2026-09-21T16:45"]);
    expect(trigger()).toHaveTextContent("Sep 21, 2026, 16:45");
  });

  it("starts a full box over when the caret was parked in it, and ignores keys that are not digits", () => {
    const onChange = vi.fn();
    render(<Harness mode="time" initial="09:00" onChange={onChange} />);
    trigger().focus();
    press("Enter");
    const hours = screen.getByRole<HTMLInputElement>("textbox", { name: "Hours" });
    const minutes = screen.getByRole<HTMLInputElement>("textbox", { name: "Minutes" });
    // A second click, or an arrow key, leaves a caret where focus had selected everything.
    hours.setSelectionRange(1, 1);
    typeKeys("1");
    expect(hours).toHaveValue("1");
    typeKeys("4");
    expect(minutes).toHaveFocus();
    minutes.setSelectionRange(2, 2);
    typeKeys("x:3");
    expect(minutes).toHaveValue("3");
    typeKeys("0");
    expect(onChange.mock.calls.map(([value]) => value)).toEqual(["14:00", "14:30"]);
    expect([hours.value, minutes.value]).toEqual(["14", "30"]);
  });

  it.each<[string, () => void, boolean]>([
    ["Done", () => pointerPress(screen.getByRole("button", { name: "Done" })), false],
    ["Escape", () => press("Escape"), false],
    ["a press outside", () => fireEvent.pointerDown(document.body), false],
    ["a second press on the trigger", () => pointerPress(trigger()), false],
    // Safari does not focus a pressed button, so nothing blurs the box before the click closes the popover.
    ["a click that moved no focus", () => fireEvent.click(trigger()), false],
    ["the submit button", () => pointerPress(screen.getByRole("button", { name: "Save" })), true],
    // requestSubmit(), or assistive technology activating the button: no pointer press comes first.
    ["a submit that no press led to", () => fireEvent.click(screen.getByRole("button", { name: "Save" })), true],
  ])("commits a digit typed into a box when %s closes the popover", (_, close, submits) => {
    const onChange = vi.fn();
    const onSubmit = vi.fn((event: React.FormEvent<HTMLFormElement>) => { event.preventDefault(); return new FormData(event.currentTarget).get("at"); });
    render(<form onSubmit={onSubmit}><Harness mode="time" name="at" initial="09:00" onChange={onChange} /><button type="submit">Save</button></form>);
    trigger().focus();
    press("Enter");
    act(() => screen.getByRole("textbox", { name: "Minutes" }).focus());
    typeKeys("5");
    expect(onChange).not.toHaveBeenCalled();
    close();
    expect(onChange).toHaveBeenCalledExactlyOnceWith("09:05");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger()).toHaveTextContent("09:05");
    expect(dateTimeFieldInput(trigger())).toHaveValue("09:05");
    // The form's own handler already reads the committed value.
    expect(onSubmit.mock.results.map((result) => result.value)).toEqual(submits ? ["09:05"] : []);
  });

  describe.each<[string, (element: HTMLElement) => void]>([
    ["a press of the pointer", pointerPress],
    // Assistive technology, or Safari, which does not focus a pressed button: nothing has blurred the box by the time the click lands.
    ["a click that moved no focus", (element) => { fireEvent.click(element); }],
  ])("shortcuts reached by %s", (_, activate) => {
    it.each<[string, string, () => HTMLElement, string[], boolean]>([
      ["the clear button of the trigger", "09:00", () => screen.getAllByRole("button", { name: "Clear" })[0], ["09:05", ""], true],
      ["Clear", "09:00", () => within(screen.getByRole("dialog")).getByRole("button", { name: "Clear" }), ["09:05", ""], false],
      ["Clear, in a field that was empty", "", () => within(screen.getByRole("dialog")).getByRole("button", { name: "Clear" }), ["09:05", ""], false],
      ["Now", "09:00", () => screen.getByRole("button", { name: "Now" }), ["09:05", "12:00"], true],
      ["Now, which is what the field held", "12:00", () => screen.getByRole("button", { name: "Now" }), ["12:05", "12:00"], true],
    ])("let a digit typed into a box commit before %s replaces the value", (_, initial, button, emitted, staysOpen) => {
      const onChange = vi.fn();
      render(<Harness mode="time" name="at" initial={initial} onChange={onChange} />);
      trigger().focus();
      press("Enter");
      act(() => screen.getByRole("textbox", { name: "Minutes" }).focus());
      typeKeys("5");
      const target = button();
      activate(target);
      expect(onChange.mock.calls.map(([value]) => value)).toEqual(emitted);
      expect(dateTimeFieldInput(trigger())).toHaveValue(emitted[1]);
      // Focus went where that press would have taken it, or to the trigger when the button is gone: the keyboard still reaches the field.
      expect(target.isConnected ? target : trigger()).toHaveFocus();
      if (staysOpen) {
        expect(screen.getByRole("textbox", { name: "Minutes" })).toHaveValue(emitted[1].slice(3));
        fireEvent.click(screen.getByRole("button", { name: "Done" }));
      }
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      // Nothing was left behind to overwrite it afterwards.
      expect(onChange).toHaveBeenCalledTimes(2);
      expect(dateTimeFieldInput(trigger())).toHaveValue(emitted[1]);
    });
  });

  it("keeps one tab stop per list, on the option the arrow keys reached", () => {
    render(<Harness mode="time" initial="09:30" />);
    fireEvent.click(trigger());
    const hours = screen.getByRole("listbox", { name: "Hours" });
    const stops = () => within(hours).getAllByRole("option").filter((option) => option.tabIndex === 0).map((option) => option.textContent);
    expect(within(hours).getByRole("option", { name: "09" })).toHaveFocus();
    expect(stops()).toEqual(["09"]);
    press("ArrowDown");
    press("ArrowDown");
    expect(within(hours).getByRole("option", { name: "11" })).toHaveFocus();
    expect(stops()).toEqual(["11"]);
    expect(within(hours).getByRole("option", { name: "09" })).toHaveAttribute("aria-selected", "true");
    // Once focus is elsewhere the list is entered through its selected option again.
    act(() => screen.getByRole("textbox", { name: "Minutes" }).focus());
    expect(stops()).toEqual(["09"]);
  });

  it("picks from the option columns and lists an off-step minute", () => {
    const onChange = vi.fn();
    render(<Harness mode="time" initial="09:07" minuteStep={15} min="08:00" max="18:30" onChange={onChange} />);
    fireEvent.click(trigger());
    const hours = screen.getByRole("listbox", { name: "Hours" });
    const minutes = screen.getByRole("listbox", { name: "Minutes" });
    expect(within(hours).getAllByRole("option")).toHaveLength(24);
    expect(within(minutes).getAllByRole("option").map((option) => option.textContent)).toEqual(["00", "07", "15", "30", "45"]);
    expect(within(minutes).getByRole("option", { name: "07" })).toHaveAttribute("aria-selected", "true");
    expect(within(hours).getByRole("option", { name: "09" })).toHaveFocus();
    expect(within(hours).getByRole("option", { name: "07" })).toHaveAttribute("aria-disabled", "true");
    expect(within(hours).getByRole("option", { name: "19" })).toHaveAttribute("aria-disabled", "true");
    press("ArrowDown");
    expect(within(hours).getByRole("option", { name: "10" })).toHaveFocus();
    press("End");
    expect(within(hours).getByRole("option", { name: "23" })).toHaveFocus();
    fireEvent.click(within(minutes).getByRole("option", { name: "30" }));
    expect(onChange).toHaveBeenLastCalledWith("09:30");
    expect(within(minutes).getAllByRole("option")).toHaveLength(4);
    fireEvent.click(within(hours).getByRole("option", { name: "18" }));
    expect(onChange).toHaveBeenLastCalledWith("18:30");
    expect(within(minutes).getByRole("option", { name: "45" })).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(within(minutes).getByRole("option", { name: "45" }));
    fireEvent.click(within(hours).getByRole("option", { name: "07" }));
    expect(onChange).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger()).toHaveTextContent("18:30");
  });
});

describe("DateTimeField preferences", () => {
  it("shows a 12-hour clock when the language or the user asks for one", () => {
    const onChange = vi.fn();
    render(<>
      <Harness mode="datetime" preferences={DEFAULT_DATE_TIME_PREFERENCES} min="2026-09-19T02:00" initial="2026-09-19T15:30" onChange={onChange} />
      <Harness mode="time" aria-label="Starts" locale="ru" preferences={DEFAULT_DATE_TIME_PREFERENCES} initial="15:30" />
      <Harness mode="time" aria-label="Ends" locale="ru" preferences={dateTimePreferences("12h", "monday")} initial="15:30" />
      <Harness mode="date" aria-label="Day" preferences={DEFAULT_DATE_TIME_PREFERENCES} />
    </>);
    expect(trigger()).toHaveTextContent("Sep 19, 2026, 3:30 PM");
    expect(trigger("Starts")).toHaveTextContent("15:30");
    expect(trigger("Ends")).toHaveTextContent("3:30 PM");
    expect(trigger().closest(".date-time-field")).toHaveAttribute("data-hour-cycle", "h12");
    expect(trigger("Starts").closest(".date-time-field")).toHaveAttribute("data-hour-cycle", "h23");
    expect(trigger("Day").closest(".date-time-field")).not.toHaveAttribute("data-hour-cycle");

    fireEvent.click(trigger());
    const hours = screen.getByRole("listbox", { name: "Hours" });
    const hourBox = screen.getByRole("textbox", { name: "Hours" });
    expect(within(hours).getAllByRole("option")).toHaveLength(24);
    expect(within(hours).getByRole("option", { name: /^3\sPM$/ })).toHaveAttribute("aria-selected", "true");
    expect(within(hours).getByRole("option", { name: /^1\sAM$/ })).toHaveAttribute("aria-disabled", "true");
    expect(hourBox).toHaveValue("03");
    fireEvent.click(screen.getByRole("button", { name: "PM" }));
    expect(onChange).toHaveBeenLastCalledWith("2026-09-19T03:30");
    expect(trigger()).toHaveTextContent("Sep 19, 2026, 3:30 AM");
    fireEvent.change(hourBox, { target: { value: "10" } });
    expect(onChange).toHaveBeenLastCalledWith("2026-09-19T10:30");
    fireEvent.click(screen.getByRole("button", { name: "AM" }));
    expect(onChange).toHaveBeenLastCalledWith("2026-09-19T22:30");
    fireEvent.change(hourBox, { target: { value: "12" } });
    expect(onChange).toHaveBeenLastCalledWith("2026-09-19T12:30");
    expect(hourBox).toHaveValue("12");
    fireEvent.change(hourBox, { target: { value: "17" } });
    expect(onChange).toHaveBeenLastCalledWith("2026-09-19T17:30");
    expect(hourBox).toHaveValue("05");
    fireEvent.click(within(hours).getByRole("option", { name: /^1\sPM$/ }));
    expect(onChange).toHaveBeenLastCalledWith("2026-09-19T13:30");
    // 01:30 would fall before the limit, so the other half of the day is out of reach.
    expect(screen.getByRole("button", { name: "PM" })).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(screen.getByRole("button", { name: "PM" }));
    expect(onChange).toHaveBeenLastCalledWith("2026-09-19T13:30");
    changeDateTimeField(trigger(), "2026-09-18T10:00");
    expect(screen.getByRole("alert")).toHaveTextContent("Choose Sep 19, 2026, 2:00 AM or later.");
  });

  it("offers no AM/PM switch on a 24-hour clock", () => {
    render(<Harness mode="time" initial="15:30" />);
    fireEvent.click(trigger());
    expect(screen.getByRole("textbox", { name: "Hours" })).toHaveValue("15");
    expect(screen.queryByRole("button", { name: /^[AP]M$/ })).not.toBeInTheDocument();
  });

  it("starts weeks on the day the user chose", () => {
    const { unmount } = render(<Harness mode="date" preferences={dateTimePreferences("24h", "sunday")} />);
    fireEvent.click(trigger());
    expect(screen.getAllByRole("columnheader").map((header) => header.textContent)).toEqual(["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]);
    expect(within(screen.getAllByRole("row")[1]).getAllByRole("button")[0]).toHaveAccessibleName("30 August 2026");
    press("Home");
    expect(day("13 September 2026")).toHaveFocus();
    press("End");
    expect(day("19 September 2026")).toHaveFocus();
    unmount();
    render(<Harness mode="date" preferences={dateTimePreferences("24h", "saturday")} />);
    fireEvent.click(trigger());
    expect(screen.getAllByRole("columnheader").map((header) => header.textContent)).toEqual(["Sat", "Sun", "Mon", "Tue", "Wed", "Thu", "Fri"]);
    expect(within(screen.getAllByRole("row")[1]).getAllByRole("button")[0]).toHaveAccessibleName("29 August 2026");
    press("End");
    expect(day("25 September 2026")).toHaveFocus();
  });
});

describe("DateTimeField shortcuts", () => {
  it("clears from the trigger and from the footer", () => {
    const onChange = vi.fn();
    render(<Harness mode="date" initial="2026-09-01" onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(onChange).toHaveBeenLastCalledWith("");
    expect(trigger()).toHaveFocus();
    expect(trigger()).toHaveTextContent("Select date");
    expect(screen.queryByRole("button", { name: "Clear" })).not.toBeInTheDocument();
    changeDateTimeField(trigger(), "2026-09-02");
    fireEvent.click(trigger());
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Clear" }));
    expect(onChange).toHaveBeenLastCalledWith("");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger()).toHaveFocus();
  });

  it("offers no way to clear a required field unless asked to", () => {
    render(<><Harness mode="date" required initial="2026-09-01" /><Harness mode="date" aria-label="Optional" required clearable initial="2026-09-01" /></>);
    // aria-required is not allowed on a button, so the description says it instead.
    expect(trigger()).not.toHaveAttribute("aria-required");
    expect(trigger()).toHaveAccessibleDescription("Sep 1, 2026 Required.");
    expect(dateTimeFieldInput(trigger())).toBeRequired();
    expect(screen.getAllByRole("button", { name: "Clear" })).toHaveLength(1);
    fireEvent.click(trigger());
    expect(within(screen.getByRole("dialog")).queryByRole("button", { name: "Clear" })).not.toBeInTheDocument();
  });

  it("picks today and now from the current local time", () => {
    const onChange = vi.fn();
    render(<><Harness mode="date" onChange={onChange} /><Harness mode="datetime" aria-label="Starts" onChange={onChange} /><Harness mode="time" aria-label="At" onChange={onChange} /></>);
    fireEvent.click(trigger());
    fireEvent.click(screen.getByRole("button", { name: "Today" }));
    expect(onChange).toHaveBeenLastCalledWith("2026-09-19");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    vi.setSystemTime(new Date(2026, 8, 19, 12, 7));
    fireEvent.click(trigger("Starts"));
    fireEvent.click(screen.getByRole("button", { name: "Now" }));
    expect(onChange).toHaveBeenLastCalledWith("2026-09-19T12:07");
    expect(within(screen.getByRole("listbox", { name: "Minutes" })).getByRole("option", { name: "07" })).toHaveAttribute("aria-selected", "true");
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    fireEvent.click(trigger("At"));
    fireEvent.click(screen.getByRole("button", { name: "Now" }));
    expect(onChange).toHaveBeenLastCalledWith("12:07");
  });

  it("disables Now when the current time is outside the limits", () => {
    render(<Harness mode="datetime" min="2026-09-19T12:01" />);
    fireEvent.click(trigger());
    expect(screen.getByRole("button", { name: "Now" })).toBeDisabled();
  });
});

describe("DateTimeField form value", () => {
  it("emits valid values set on the carrier and ignores anything else", () => {
    const onChange = vi.fn();
    render(<Harness mode="datetime" onChange={onChange} />);
    changeDateTimeField(trigger(), "2026-10-01T10:00");
    expect(onChange).toHaveBeenLastCalledWith("2026-10-01T10:00");
    expect(dateTimeFieldInput(trigger())).toHaveValue("2026-10-01T10:00");
    expect(trigger()).toHaveTextContent("Oct 1, 2026, 10:00");
    changeDateTimeField(trigger(), "2026-10-01");
    changeDateTimeField(trigger(), "tomorrow");
    expect(onChange).toHaveBeenCalledOnce();
    expect(dateTimeFieldInput(trigger())).toHaveValue("2026-10-01T10:00");
    changeDateTimeField(trigger(), "");
    expect(onChange).toHaveBeenLastCalledWith("");
    expect(() => dateTimeFieldInput(document.body)).toThrow();
  });

  it("is invalid as soon as the value leaves min or max", () => {
    render(<Harness mode="datetime" min="2026-09-19T12:00" max="2026-09-30T18:00" initial="2026-09-23T15:30" />);
    expect(trigger()).toBeValid();
    changeDateTimeField(trigger(), "2026-09-17T15:30");
    expect(trigger()).toBeInvalid();
    expect(screen.getByRole("alert")).toHaveTextContent("Choose Sep 19, 2026, 12:00 or later.");
    expect(trigger()).toHaveAccessibleDescription(/Choose Sep 19, 2026, 12:00 or later\./);
    expect(dateTimeFieldInput(trigger()).validity.customError).toBe(true);
    expect(trigger().closest(".date-time-field")).toHaveAttribute("data-invalid", "true");
    changeDateTimeField(trigger(), "2026-10-01T09:00");
    expect(screen.getByRole("alert")).toHaveTextContent("Choose Sep 30, 2026, 18:00 or earlier.");
    changeDateTimeField(trigger(), "2026-09-23T15:30");
    expect(trigger()).toBeValid();
    expect(dateTimeFieldInput(trigger()).validity.valid).toBe(true);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows an error it was mounted with without announcing it", () => {
    render(<Harness mode="datetime" min="2026-09-19T12:00" initial="2026-09-17T15:30" />);
    expect(trigger()).toBeInvalid();
    expect(screen.getByText("Choose Sep 19, 2026, 12:00 or later.")).toBeInTheDocument();
    expect(trigger()).toHaveAccessibleDescription("Sep 17, 2026, 15:30 Choose Sep 19, 2026, 12:00 or later.");
    expect(dateTimeFieldInput(trigger()).validity.customError).toBe(true);
    // Nobody did anything yet: an assertive alert would talk over the form as it opens.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    changeDateTimeField(trigger(), "2026-09-23T15:30");
    expect(trigger()).toBeValid();
    changeDateTimeField(trigger(), "2026-09-17T15:30");
    expect(screen.getByRole("alert")).toHaveTextContent("Choose Sep 19, 2026, 12:00 or later.");
  });

  it("keeps an error nobody raised out of the live alerts, however often a moving limit rewords it", () => {
    const field = (value: string, min: string) => <DateTimePreferencesContext.Provider value={dateTimePreferences("24h", "monday")}>
      <form><DateTimeField mode="datetime" locale="en" aria-label="Move to" value={value} min={min} onChange={() => undefined} /><button type="submit">Move</button></form>
    </DateTimePreferencesContext.Provider>;
    const { rerender } = render(field("2026-09-19T12:05", "2026-09-19T12:00"));
    expect(trigger("Move to")).toBeValid();
    // "Not before now": the clock overtakes the value, then rewords the message minute after minute.
    for (const minute of ["06", "07"]) {
      rerender(field("2026-09-19T12:05", `2026-09-19T12:${minute}`));
      expect(trigger("Move to")).toBeInvalid();
      expect(trigger("Move to")).toHaveAccessibleDescription(`Sep 19, 2026, 12:05 Choose Sep 19, 2026, 12:${minute} or later.`);
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    }
    // Another record loaded into the form is nobody's mistake either.
    rerender(field("2026-09-18T09:00", "2026-09-19T12:07"));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    // A submit the error blocks is the user's: the note is replaced by an alert, since only a new one is read out...
    const note = screen.getByText("Choose Sep 19, 2026, 12:07 or later.");
    fireEvent.click(screen.getByRole("button", { name: "Move" }));
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Choose Sep 19, 2026, 12:07 or later.");
    expect(note).not.toBeInTheDocument();
    // ...which is gone before the next minute could make it repeat itself.
    rerender(field("2026-09-18T09:00", "2026-09-19T12:08"));
    expect(alert).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(trigger("Move to")).toHaveAccessibleDescription(/Choose Sep 19, 2026, 12:08 or later\./);
  });

  it("raises an alert once: not for the same record coming back, nor when the form is no longer busy", () => {
    const field = (value: string, busy = false) => <DateTimePreferencesContext.Provider value={dateTimePreferences("24h", "monday")}>
      <form><DateTimeField mode="datetime" locale="en" aria-label="Run at" value={value} min="2026-09-19T12:00" disabled={busy} onChange={() => undefined} /><button type="submit">Save</button></form>
    </DateTimePreferencesContext.Provider>;
    const overdue = "Choose Sep 19, 2026, 12:00 or later.";
    const { rerender } = render(field("2026-09-17T15:30"));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(screen.getByRole("alert")).toHaveTextContent(overdue);
    // The form moves on to a record that is fine, then back to the one the submit was blocked for: nobody raised anything this time.
    rerender(field("2026-09-23T15:30"));
    expect(trigger("Run at")).toBeValid();
    rerender(field("2026-09-17T15:30"));
    expect(trigger("Run at")).toBeInvalid();
    expect(trigger("Run at")).toHaveAccessibleDescription(`Sep 17, 2026, 15:30 ${overdue}`);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    // A second blocked submit is the user's again...
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(screen.getByRole("alert")).toHaveTextContent(overdue);
    // ...and what comes back with a form that was busy meanwhile is not.
    rerender(field("2026-09-17T15:30", true));
    expect(screen.queryByText(overdue)).not.toBeInTheDocument();
    rerender(field("2026-09-17T15:30"));
    expect(screen.getByText(overdue)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("starts over, untouched, when it is handed an empty value it did not clear itself", () => {
    const field = (value: string) => <DateTimePreferencesContext.Provider value={dateTimePreferences("24h", "monday")}>
      <DateTimeField mode="date" locale="en" aria-label="Due date" required clearable value={value} onChange={() => undefined} />
    </DateTimePreferencesContext.Provider>;
    const { rerender } = render(field("2026-09-17"));
    fireEvent.click(trigger());
    press("Escape");
    // The user has been through the field, which would mark it once empty; the next record being empty is not their doing.
    rerender(field(""));
    expect(trigger()).toBeValid();
    expect(screen.queryByText("Choose a date.")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    fireEvent.click(trigger());
    press("Escape");
    expect(screen.getByRole("alert")).toHaveTextContent("Choose a date.");
  });

  it("still marks a required field the user cleared after having been through it", () => {
    render(<Harness mode="date" required clearable initial="2026-09-17" />);
    fireEvent.click(trigger());
    press("Escape");
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(trigger()).toBeInvalid();
    expect(screen.getByRole("alert")).toHaveTextContent("Choose a date.");
  });

  it("announces a value the user set only in the words it was raised with", () => {
    const { rerender } = render(<Harness mode="datetime" min="2026-09-19T12:00" initial="2026-09-23T15:30" />);
    changeDateTimeField(trigger(), "2026-09-17T15:30");
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Choose Sep 19, 2026, 12:00 or later.");
    rerender(<Harness mode="datetime" min="2026-09-19T12:01" initial="2026-09-23T15:30" />);
    expect(alert).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByText("Choose Sep 19, 2026, 12:01 or later.")).toBeInTheDocument();
    // The next value the user sets is a new mistake, and is announced as one.
    changeDateTimeField(trigger(), "2026-09-16T15:30");
    expect(screen.getByRole("alert")).toHaveTextContent("Choose Sep 19, 2026, 12:01 or later.");
  });

  it("blocks the submit of an empty required field and explains why", () => {
    const onSubmit = vi.fn((event: React.FormEvent) => event.preventDefault());
    render(<form onSubmit={onSubmit}><input aria-label="Title" /><Harness mode="date" required name="due" /><button type="submit">Save</button></form>);
    expect(trigger()).toBeValid();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onSubmit).not.toHaveBeenCalled();
    expect(trigger()).toBeInvalid();
    expect(trigger()).toHaveFocus();
    expect(screen.getByRole("alert")).toHaveTextContent("Choose a date.");
    fireEvent.click(trigger());
    fireEvent.click(day("21 September 2026"));
    expect(trigger()).toBeValid();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onSubmit).toHaveBeenCalledOnce();
    expect(new FormData(screen.getByRole("button", { name: "Save" }).closest("form")!).get("due")).toBe("2026-09-21");
  });

  it("leaves focus to an earlier invalid control and blocks an out-of-range submit", () => {
    const onSubmit = vi.fn((event: React.FormEvent) => event.preventDefault());
    render(<form onSubmit={onSubmit}><input aria-label="Title" required /><Harness mode="date" min="2026-09-19" initial="2026-09-01" /><button type="submit">Save</button></form>);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onSubmit).not.toHaveBeenCalled();
    expect(trigger()).not.toHaveFocus();
    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Plan" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onSubmit).not.toHaveBeenCalled();
    expect(trigger()).toHaveFocus();
    expect(trigger()).toHaveAccessibleDescription(/Choose Sep 19, 2026 or later\./);
  });

  it("marks a required field the user left empty after closing the popover", () => {
    render(<Harness mode="time" required />);
    fireEvent.click(trigger());
    expect(trigger()).toBeValid();
    press("Escape");
    expect(trigger()).toBeInvalid();
    expect(screen.getByRole("alert")).toHaveTextContent("Choose a time.");
  });
});
