import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, useSyncExternalStore, type FormEvent, type KeyboardEvent, type MouseEvent } from "react";
import { flushSync } from "react-dom";
import { Calendar, ChevronLeft, ChevronRight, Clock, X } from "lucide-react";
import { useDateTimePreferences } from "../date-time-preferences";
import type { Locale } from "../i18n";
import { dateTimeFieldText, type DateTimeFieldTextKey } from "./date-time-field-i18n";
import {
  DEFAULT_TIME, MAX_YEAR, MIN_YEAR, addDays, addMonths, addYears, clampDate, clampTime, dateAllowed, dateKey, daysInMonth, displayHour, endOfWeek, formatDayLabel, formatDisplay, formatLimit,
  formatMonthTitle, formatValue, hourAllowed, hourLabel, hourOptions, minuteAllowed, minuteOptions, monthAllowed, monthGrid, monthName, normalizeMinuteStep, nowParts,
  parseLimits, parseValue, periodLabel, placePopover, resolveHourCycle, sameDate, startOfWeek, stepHour, stepMinute, togglePeriod, typedHour, validate, weekdayNames,
  type DateParts, type DateTimeFieldMode, type InvalidReason, type TimeParts,
} from "./date-time-field-model";
import "./DateTimeField.css";

export type { DateTimeFieldMode };
export interface DateTimeFieldProps {
  mode: DateTimeFieldMode;
  /** "YYYY-MM-DD", "YYYY-MM-DDTHH:mm[:ss]" or "HH:mm" — the formats of the native inputs; "" when empty. */
  value: string;
  onChange: (value: string) => void;
  locale: Locale;
  min?: string;
  max?: string;
  required?: boolean;
  disabled?: boolean;
  clearable?: boolean;
  minuteStep?: number;
  id?: string;
  name?: string;
  placeholder?: string;
  className?: string;
  "aria-label"?: string;
  "aria-labelledby"?: string;
  "aria-describedby"?: string;
}

type FocusRequest = "day" | "month" | "option" | "time" | null;
const placeholderKeys: Record<DateTimeFieldMode, DateTimeFieldTextKey> = { date: "placeholderDate", datetime: "placeholderDatetime", time: "placeholderTime" };
const dialogKeys: Record<DateTimeFieldMode, DateTimeFieldTextKey> = { date: "dialogDate", datetime: "dialogDatetime", time: "dialogTime" };
const requiredKeys: Record<DateTimeFieldMode, DateTimeFieldTextKey> = { date: "requiredDate", datetime: "requiredDatetime", time: "requiredTime" };
const focusTargets: Record<Exclude<FocusRequest, null>, string> = {
  day: '[data-date][tabindex="0"]', month: '[data-month][tabindex="0"]', option: '[role="option"][tabindex="0"]', time: ".date-time-field-time-input",
};
const dayMoves: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 };
const monthMoves: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -3, ArrowDown: 3 };
const pad = (value: number) => String(value).padStart(2, "0");

/** A disabled <fieldset> disables the trigger natively without the field being told, like it would a native input. */
function disabledByFieldset(element: Element | null): boolean {
  for (let fieldset = element?.closest("fieldset[disabled]"); fieldset; fieldset = fieldset.parentElement?.closest("fieldset[disabled]")) {
    const legend = Array.from(fieldset.children).find((child) => child.localName === "legend");
    if (!legend?.contains(element)) return true;
  }
  return false;
}

/**
 * A box holds the digit just typed until it blurs. A press of the pointer blurs it on the way to any button, but a click that moved
 * no focus (assistive technology, Safari) does not, and React ignores the blur of a box that is removed with the popover: whatever
 * acts on the value lets the box commit first, the way that press would have. Flushed, so that the caller goes on from the new value.
 */
function commitPendingBox(popover: HTMLElement | null, focus?: HTMLElement | null) {
  const box = document.activeElement;
  if (!(box instanceof HTMLInputElement) || !popover?.contains(box)) return;
  flushSync(() => { if (focus) focus.focus(); else box.blur(); });
}

function TimeInput({ label, value, max, onCommit, onStep, onFilled }: {
  label: string; value: number | null; max: number; onCommit: (value: number) => void; onStep: (direction: 1 | -1) => void; onFilled?: () => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [draft, setDraft] = useState<string | null>(null);
  // Read by the blur that auto-advancing to the next input triggers before this component re-renders.
  const pending = useRef<string | null>(null);
  const shown = draft ?? (value === null ? "" : pad(value));
  // The box is one segment, as in a native time input: typing replaces what it holds. Focus selects it to say so, but a value
  // React writes afterwards (auto-advance lands here first, the arrow keys rewrite it) leaves only a caret at its end.
  useLayoutEffect(() => {
    const input = inputRef.current;
    if (draft === null && input && document.activeElement === input) input.select();
  }, [shown, draft]);
  function edit(text: string | null) {
    pending.current = text;
    setDraft(text);
  }
  function commit() {
    const text = pending.current;
    edit(null);
    if (text) onCommit(Math.min(max, Number(text)));
  }
  return <input
    ref={inputRef}
    className="date-time-field-time-input"
    type="text"
    inputMode="numeric"
    autoComplete="off"
    placeholder="--"
    aria-label={label}
    value={shown}
    onFocus={(event) => event.currentTarget.select()}
    onChange={(event) => {
      const typed = (event.nativeEvent as Partial<InputEvent>).data ?? "";
      // Like a native time input, a key that is not a digit does nothing.
      if (/\D/.test(typed)) return;
      const held = event.target.value.replace(/\D/g, "");
      // No maxLength: once a click or an arrow key has parked the caret in a full box, it would swallow every digit typed.
      // The box overflows instead, and what was just typed starts it over.
      const digits = (held.length > 2 && typed ? typed : held).slice(0, 2);
      edit(digits);
      if (digits.length < 2) return;
      commit();
      onFilled?.();
    }}
    onBlur={commit}
    onKeyDown={(event) => {
      // Enter must never reach the enclosing form as an implicit submit.
      if (event.key === "Enter") { event.preventDefault(); commit(); }
      if (event.key === "ArrowUp" || event.key === "ArrowDown") { event.preventDefault(); edit(null); onStep(event.key === "ArrowUp" ? 1 : -1); }
    }}
  />;
}

function OptionColumn({ label, options, selected, format = pad, isDisabled, onSelect }: {
  label: string; options: number[]; selected: number | null; format?: (option: number) => string; isDisabled: (option: number) => boolean; onSelect: (option: number) => void;
}) {
  const listRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const list = listRef.current;
    const option = list?.querySelector<HTMLElement>('[aria-selected="true"]');
    // Not scrollIntoView: it would also scroll a dialog the popover deliberately overflows.
    if (list && option) list.scrollTop = option.offsetTop - (list.clientHeight - option.offsetHeight) / 2;
  }, [selected]);
  // The tab stop follows arrow-key focus, so Tab leaves the list; it returns to the selected option once focus is elsewhere.
  const [stop, setStop] = useState<number | null>(null);
  const tabbable = stop !== null && options.includes(stop) ? stop : selected !== null && options.includes(selected) ? selected : options[0];

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const buttons = Array.from(listRef.current?.querySelectorAll<HTMLElement>('[role="option"]') ?? []);
    const index = buttons.indexOf(event.target as HTMLElement);
    const next = event.key === "ArrowDown" ? index + 1 : event.key === "ArrowUp" ? index - 1 : event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : null;
    if (index === -1 || next === null) return;
    event.preventDefault();
    buttons[Math.max(0, Math.min(buttons.length - 1, next))].focus();
  }

  return <div
    ref={listRef}
    role="listbox"
    aria-label={label}
    className="date-time-field-options"
    onKeyDown={handleKeyDown}
    onFocus={(event) => { const option = (event.target as HTMLElement).dataset.option; if (option !== undefined) setStop(Number(option)); }}
    onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setStop(null); }}
  >
    {options.map((option) => {
      const disabled = isDisabled(option);
      return <button
        key={option}
        type="button"
        role="option"
        className="date-time-field-option"
        data-option={option}
        aria-selected={option === selected}
        aria-disabled={disabled || undefined}
        tabIndex={option === tabbable ? 0 : -1}
        onClick={() => { if (!disabled) onSelect(option); }}
      >{format(option)}</button>;
    })}
  </div>;
}

function MonthChooser({ label, year, shown, locale, isDisabled, onSelect, onStepYear }: {
  label: string; year: number; shown: number; locale: Locale; isDisabled: (month: number) => boolean; onSelect: (month: number) => void; onStepYear: (direction: 1 | -1) => void;
}) {
  // As in OptionColumn: browsing with the arrows moves the tab stop, not the shown month.
  const [stop, setStop] = useState<number | null>(null);

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "PageUp" || event.key === "PageDown") { event.preventDefault(); onStepYear(event.key === "PageUp" ? -1 : 1); return; }
    const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLElement>("[data-month]"));
    const index = buttons.indexOf(event.target as HTMLElement);
    const next = event.key in monthMoves ? index + monthMoves[event.key] : event.key === "Home" ? 0 : event.key === "End" ? 11 : null;
    if (index === -1 || next === null) return;
    event.preventDefault();
    buttons[Math.max(0, Math.min(11, next))].focus();
  }

  return <div
    role="group"
    aria-label={label}
    className="date-time-field-months"
    onKeyDown={handleKeyDown}
    onFocus={(event) => { const month = (event.target as HTMLElement).dataset.month; if (month !== undefined) setStop(Number(month)); }}
    onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setStop(null); }}
  >
    {Array.from({ length: 12 }, (_, index) => index + 1).map((month) => {
      const disabled = isDisabled(month);
      return <button
        key={month}
        type="button"
        className="date-time-field-month"
        data-month={month}
        aria-label={formatMonthTitle(year, month, locale)}
        aria-pressed={month === shown}
        aria-disabled={disabled || undefined}
        tabIndex={month === (stop ?? shown) ? 0 : -1}
        onClick={() => { if (!disabled) onSelect(month); }}
      >{monthName(month, locale, "short")}</button>;
    })}
  </div>;
}

export function DateTimeField({
  mode, value, onChange, locale, min, max, required = false, disabled = false, clearable = !required, minuteStep, id, name, placeholder, className,
  "aria-label": ariaLabel, "aria-labelledby": ariaLabelledBy, "aria-describedby": ariaDescribedBy,
}: DateTimeFieldProps) {
  const uid = useId();
  const valueId = `${uid}-value`;
  const errorId = `${uid}-error`;
  const requiredId = `${uid}-required`;
  const popoverId = `${uid}-popover`;
  const titleId = `${uid}-title`;
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const carrierRef = useRef<HTMLInputElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  const focusRequest = useRef<FocusRequest>(null);
  const [open, setOpen] = useState(false);
  const [revealed, setRevealed] = useState(false);
  const [panel, setPanel] = useState<"days" | "months">("days");
  const [today, setToday] = useState<{ date: DateParts; time: TimeParts } | null>(null);
  const [focusDate, setFocusDate] = useState<DateParts>({ year: 2000, month: 1, day: 1 });
  const preferences = useDateTimePreferences();
  const weekStartsOn = preferences.weekStartsOn;
  const hourCycle = resolveHourCycle(locale, preferences.hourCycle);
  const subscribeToFieldsets = useCallback((notify: () => void) => {
    const observer = typeof MutationObserver === "function" ? new MutationObserver(notify) : null;
    for (let node = rootRef.current?.parentElement; node; node = node.parentElement) {
      if (node.localName === "fieldset") observer?.observe(node, { attributes: true, attributeFilter: ["disabled"] });
    }
    return () => observer?.disconnect();
  }, []);
  const inert = useSyncExternalStore(subscribeToFieldsets, () => disabledByFieldset(rootRef.current)) || disabled;

  const text = dateTimeFieldText(locale);
  const step = normalizeMinuteStep(minuteStep);
  const parsed = parseValue(mode, value);
  const limits = parseLimits(mode, min, max);
  const seconds = parsed?.seconds ?? "";
  const reason = validate(mode, parsed, limits, required);
  const rangeText = (kind: InvalidReason | null) => kind === "min" ? text("tooEarly", { limit: formatLimit(mode, min, locale, hourCycle) }) : kind === "max" ? text("tooLate", { limit: formatLimit(mode, max, locale, hourCycle) }) : "";
  const rangeMessage = rangeText(reason);
  // A value outside the limits is wrong however it got there; an empty required field only once the user had a chance to fill it.
  // A disabled control is barred from validation, so it reports nothing either way.
  const invalid = !inert && reason !== null && (revealed || reason !== "required");
  const message = !invalid ? "" : reason === "required" ? text(requiredKeys[mode]) : rangeMessage;
  // A range error interrupts as a live alert only for the value the user just set here, or tried to submit, and only in the words
  // it was raised with. One the field was handed (mounted with it, another record loaded into the form, a limit that follows the
  // clock and rewords the message every minute) is shown and described all the same, but never talks over the form.
  const [alerted, setAlerted] = useState<{ value: string; message: string } | null>(null);
  // And only once: a value that did not come from here (another record, even the same one coming back) starts the field over, as
  // quiet and as untouched as a newly mounted one, and an error that returns once the form is no longer busy is not news either.
  const [handed, setHanded] = useState(value);
  if (handed !== value) {
    setHanded(value);
    if (alerted?.value !== value) {
      setAlerted(null);
      if (!open) setRevealed(false);
    }
  }
  if (alerted && inert) setAlerted(null);
  const assertive = reason === "required" || (alerted?.value === value && alerted.message === rangeMessage);
  const display = formatDisplay(mode, parsed, locale, hourCycle);
  if (open && inert) setOpen(false);
  const active = open && !inert;
  const latestValue = useRef(value);
  useLayoutEffect(() => { latestValue.current = value; });

  useEffect(() => { carrierRef.current?.setCustomValidity(rangeMessage); }, [rangeMessage]);

  // A caption referenced by aria-labelledby is not a <label>: clicking it would not reach the field the way it reached a native input.
  useEffect(() => {
    const caption = ariaLabelledBy ? document.getElementById(ariaLabelledBy.trim().split(/\s+/)[0]) : null;
    const focusTrigger = () => triggerRef.current?.focus();
    caption?.addEventListener("click", focusTrigger);
    return () => caption?.removeEventListener("click", focusTrigger);
  }, [ariaLabelledBy]);

  useLayoutEffect(() => {
    const popover = popoverRef.current;
    const trigger = triggerRef.current;
    if (!active || !popover || !trigger) return;
    if (typeof popover.showPopover === "function") {
      // Set here rather than in JSX: where the API is missing the bare attribute would only hide the element.
      popover.setAttribute("popover", "manual");
      try { popover.showPopover(); } catch { /* already in the top layer */ }
    }
    const place = () => {
      const viewport = { width: document.documentElement.clientWidth || window.innerWidth, height: window.innerHeight };
      const box = placePopover(trigger.getBoundingClientRect(), { width: popover.offsetWidth, height: popover.offsetHeight }, viewport);
      popover.style.top = `${box.top}px`;
      popover.style.left = `${box.left}px`;
      popover.dataset.placement = box.placement;
    };
    place();
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(place) : null;
    observer?.observe(popover);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [active]);

  function applyFocusRequest() {
    const request = focusRequest.current;
    focusRequest.current = null;
    const popover = popoverRef.current;
    const target = request ? popover?.querySelector<HTMLElement>(focusTargets[request]) : null;
    if (!popover || !target) return;
    target.focus({ preventScroll: true });
    // preventScroll spares the page and an enclosing <dialog>, but also the popover's own scrollport, which a short viewport
    // or a high zoom turns into a scroller: follow focus there by hand. Rects, because an option's offsetTop is relative to its list.
    if (popover.scrollHeight <= popover.clientHeight) return;
    const rect = target.getBoundingClientRect();
    const edge = 4; // The focus outline and its offset.
    const top = popover.getBoundingClientRect().top + popover.clientTop;
    // The footer is pinned over the bottom of the scrollport.
    const bottom = popover.querySelector(".date-time-field-footer")?.getBoundingClientRect().top ?? top + popover.clientHeight;
    if (rect.top < top + edge) popover.scrollTop -= top + edge - rect.top;
    else if (rect.bottom > bottom - edge) popover.scrollTop += rect.bottom - (bottom - edge);
  }
  // After every render: the element a handler asked to focus exists only once its state change is committed.
  useLayoutEffect(applyFocusRequest);

  const closePopover = useCallback((returnFocus: boolean) => {
    // Whichever way the popover closes; a form whose submit closes it then reads the committed value.
    commitPendingBox(popoverRef.current);
    setOpen(false);
    setRevealed(true);
    if (returnFocus) triggerRef.current?.focus();
  }, []);

  useEffect(() => {
    if (!active) return;
    const closeOutside = (event: Event) => {
      if (event.target instanceof Node && !rootRef.current?.contains(event.target)) closePopover(false);
    };
    // Capture phase on the document: Escape must close only this popover, never the <dialog> or drawer around the field.
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      closePopover(true);
    };
    document.addEventListener("pointerdown", closeOutside);
    // A submit no press led to (requestSubmit, assistive technology) reaches the form's handler only after this.
    document.addEventListener("submit", closeOutside, true);
    document.addEventListener("keydown", closeOnEscape, true);
    return () => {
      document.removeEventListener("pointerdown", closeOutside);
      document.removeEventListener("submit", closeOutside, true);
      document.removeEventListener("keydown", closeOnEscape, true);
    };
  }, [active, closePopover]);

  function emit(next: string) {
    // Not the `value` of this render: a handler that let a pending box commit first goes on in the closure of the render before it.
    if (next === latestValue.current) return;
    latestValue.current = next;
    setAlerted({ value: next, message: rangeText(validate(mode, parseValue(mode, next), limits, false)) });
    onChange(next);
  }

  function openPopover(source: "keyboard" | "pointer") {
    if (inert || open) return;
    const current = nowParts(new Date());
    setToday(current);
    setFocusDate(parsed?.date ?? clampDate(mode, current.date, limits));
    setPanel("days");
    setOpen(true);
    // Typing is the fast path from the keyboard; on touch a focused input would only raise the on-screen keyboard.
    focusRequest.current = mode !== "time" ? "day" : source === "keyboard" ? "time" : "option";
  }

  function moveFocus(next: DateParts, request: FocusRequest = "day") {
    focusRequest.current = request;
    // Focus can sit on another day than the roving one (a disabled day takes it on click); no render follows an unchanged date.
    if (sameDate(next, focusDate)) applyFocusRequest();
    else setFocusDate(next);
  }
  function pickDate(date: DateParts) {
    if (!dateAllowed(mode, date, limits, seconds)) return;
    if (mode === "date") {
      emit(dateKey(date));
      closePopover(true);
      return;
    }
    emit(formatValue(mode, { date, time: clampTime(mode, date, parsed?.time ?? DEFAULT_TIME, limits, step, seconds), seconds }));
    setFocusDate(date);
    focusRequest.current = "day";
  }
  function pickTime(next: TimeParts) {
    const date = mode === "datetime" ? parsed?.date ?? clampDate(mode, (today ?? nowParts(new Date())).date, limits) : null;
    emit(formatValue(mode, { date, time: clampTime(mode, date, next, limits, step, seconds), seconds }));
    if (date && !parsed?.date) setFocusDate(date);
  }
  function pickNow(button: HTMLElement) {
    commitPendingBox(popoverRef.current, button);
    const current = nowParts(new Date());
    setToday(current);
    if (mode === "date") { pickDate(current.date); return; }
    const date = mode === "datetime" ? current.date : null;
    if (!minuteAllowed(mode, date, current.time, limits, seconds)) return;
    emit(formatValue(mode, { date, time: current.time, seconds }));
    if (date) setFocusDate(date);
  }

  function handleInvalid(event: FormEvent<HTMLInputElement>) {
    // The native bubble would point at an invisible input; the inline message replaces it.
    event.preventDefault();
    setRevealed(true);
    setAlerted({ value, message: rangeMessage });
    const carrier = event.currentTarget;
    const firstInvalid = Array.from(carrier.form?.elements ?? []).find((element) => "validity" in element && (element as HTMLInputElement).willValidate && !(element as HTMLInputElement).validity.valid);
    if (firstInvalid === carrier) triggerRef.current?.focus();
  }

  function handleTriggerKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (event.key !== "Enter" && event.key !== " " && event.key !== "ArrowDown") return;
    event.preventDefault();
    if (active && event.key !== "ArrowDown") closePopover(true);
    else openPopover("keyboard");
  }

  function handleGridKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const button = (event.target as HTMLElement).closest<HTMLElement>("[data-date]");
    const origin = parseValue("date", button?.dataset.date)?.date ?? focusDate;
    if (event.key === "Enter" || event.key === " ") { event.preventDefault(); pickDate(origin); return; }
    const next = event.key in dayMoves ? addDays(origin, dayMoves[event.key])
      : event.key === "Home" ? startOfWeek(origin, weekStartsOn)
      : event.key === "End" ? endOfWeek(origin, weekStartsOn)
      : event.key === "PageUp" ? (event.shiftKey ? addYears(origin, -1) : addMonths(origin, -1))
      : event.key === "PageDown" ? (event.shiftKey ? addYears(origin, 1) : addMonths(origin, 1))
      : null;
    if (!next) return;
    event.preventDefault();
    moveFocus(next);
  }

  function trapTab(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "Tab") return;
    const stops = Array.from(event.currentTarget.querySelectorAll<HTMLElement>("button, input")).filter((element) => element.tabIndex >= 0 && !element.hasAttribute("disabled"));
    const first = stops[0];
    const last = stops[stops.length - 1];
    if (!first) return;
    if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
    else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
  }

  /**
   * Keeps focus inside the popover for every pointer interaction: blank areas would otherwise drop it on <body>,
   * and Safari does not focus a clicked button at all, which would leave the keyboard handlers unreachable.
   */
  function keepFocus(event: MouseEvent<HTMLDivElement>) {
    const target = event.target as HTMLElement;
    // Inputs take focus themselves; a press on a list's own box is its scrollbar.
    if (target.closest("input") || target.getAttribute("role") === "listbox") return;
    event.preventDefault();
    target.closest<HTMLElement>("button")?.focus({ preventScroll: true });
  }

  const shownMonth = { year: focusDate.year, month: focusDate.month };
  const monthSpan = panel === "days" ? 1 : 12;
  // The header steps months, or years while the month chooser is up; a step is pointless once nothing beyond it fits the limits.
  const stepBlocked = (direction: 1 | -1) => {
    const edge = panel === "days" ? addMonths({ ...shownMonth, day: 1 }, direction) : { year: shownMonth.year + direction, month: direction === 1 ? 1 : 12 };
    if (edge.year < MIN_YEAR || edge.year > MAX_YEAR || (edge.year === shownMonth.year && edge.month === shownMonth.month)) return true;
    return !monthAllowed(mode, edge.year, edge.month, direction === 1 ? { min: null, max: limits.max } : { min: limits.min, max: null });
  };
  const previousDisabled = stepBlocked(-1);
  const nextDisabled = stepBlocked(1);
  const currentTime = parsed?.time ?? null;
  const shownTime = currentTime ?? DEFAULT_TIME;
  const timeDate = mode === "datetime" ? parsed?.date ?? (today ? clampDate(mode, today.date, limits) : null) : null;
  const periodBlocked = mode !== "date" && !hourAllowed(mode, timeDate, togglePeriod(shownTime.hour), limits, seconds);
  const nowDisabled = !today || (mode === "date" ? !dateAllowed(mode, today.date, limits) : !minuteAllowed(mode, mode === "datetime" ? today.date : null, today.time, limits, seconds));
  const Icon = mode === "time" ? Clock : Calendar;
  // aria-required is not supported on a button; the carrier that is natively required is hidden from assistive technology.
  const describedBy = [ariaDescribedBy, valueId, required && !inert ? requiredId : undefined, message ? errorId : undefined].filter(Boolean).join(" ");

  return <div
    ref={rootRef}
    className={`date-time-field${className ? ` ${className}` : ""}`}
    data-mode={mode}
    data-hour-cycle={mode === "date" ? undefined : hourCycle}
    data-open={active}
    data-invalid={invalid}
    data-disabled={inert}
    onBlur={(event) => {
      if (active && event.relatedTarget instanceof Node && !event.currentTarget.contains(event.relatedTarget)) closePopover(false);
    }}
  >
    <button
      ref={triggerRef}
      type="button"
      id={id}
      className="date-time-field-trigger"
      aria-haspopup="dialog"
      aria-expanded={active}
      aria-controls={active ? popoverId : undefined}
      aria-label={ariaLabel}
      aria-labelledby={ariaLabelledBy}
      aria-describedby={describedBy}
      aria-invalid={invalid || undefined}
      disabled={disabled}
      data-clearable={clearable && Boolean(parsed) && !inert}
      // Safari does not focus a clicked button, so closing has to bring focus back from the popover itself.
      onClick={() => { if (active) closePopover(true); else openPopover("pointer"); }}
      onKeyDown={handleTriggerKeyDown}
    >
      <Icon size={15} aria-hidden="true" />
      <span id={valueId} className="date-time-field-value" data-placeholder={!display}>{display || (placeholder ?? text(placeholderKeys[mode]))}</span>
      {/* In here, where neither the button's name (it comes from its label) nor the text of a wrapping <label> picks it up. */}
      {required && !inert && <span id={requiredId} hidden>{text("required")}</span>}
    </button>
    {clearable && parsed && !inert && <button type="button" className="date-time-field-clear" aria-label={text("clear")} onClick={() => { commitPendingBox(popoverRef.current, triggerRef.current); emit(""); triggerRef.current?.focus(); }}>
      <X size={14} aria-hidden="true" />
    </button>}
    <input
      ref={carrierRef}
      className="date-time-field-native"
      type="text"
      tabIndex={-1}
      aria-hidden="true"
      autoComplete="off"
      name={name}
      required={required}
      disabled={disabled}
      value={parsed ? value : ""}
      onChange={(event) => { if (event.target.value === "" || parseValue(mode, event.target.value)) emit(event.target.value); }}
      onInvalid={handleInvalid}
      onFocus={() => triggerRef.current?.focus()}
    />
    {/* Keyed, so a note that turns into an alert is inserted anew, which is what gets an alert read out, and an alert that
        turns into a note is gone before its wording changes. */}
    {message && <p key={assertive ? "alert" : "note"} id={errorId} className="date-time-field-error" role={assertive ? "alert" : undefined}>{message}</p>}
    {active && <div
      ref={popoverRef}
      id={popoverId}
      role="dialog"
      aria-label={text(dialogKeys[mode])}
      className="date-time-field-popover"
      data-mode={mode}
      onKeyDown={trapTab}
      onMouseDown={keepFocus}
      // Inside a wrapping <label> a click on a blank area would otherwise be forwarded to the trigger and close the popover.
      onClick={(event) => event.preventDefault()}
    >
      <div className="date-time-field-panels">
        {mode !== "time" && <div className="date-time-field-calendar">
          <div className="date-time-field-header">
            <button type="button" className="date-time-field-nav" aria-label={text(panel === "days" ? "previousMonth" : "previousYear")} aria-disabled={previousDisabled || undefined} onClick={() => { if (!previousDisabled) moveFocus(addMonths(focusDate, -monthSpan), null); }}>
              <ChevronLeft size={16} aria-hidden="true" />
            </button>
            <button type="button" id={titleId} className="date-time-field-title" aria-expanded={panel === "months"} title={text(panel === "days" ? "chooseMonth" : "backToDays")} onClick={() => setPanel(panel === "days" ? "months" : "days")}>
              <span aria-live="polite" aria-atomic="true">{panel === "days" ? formatMonthTitle(shownMonth.year, shownMonth.month, locale) : shownMonth.year}</span>
            </button>
            <button type="button" className="date-time-field-nav" aria-label={text(panel === "days" ? "nextMonth" : "nextYear")} aria-disabled={nextDisabled || undefined} onClick={() => { if (!nextDisabled) moveFocus(addMonths(focusDate, monthSpan), null); }}>
              <ChevronRight size={16} aria-hidden="true" />
            </button>
          </div>
          {panel === "days" ? <div role="grid" aria-labelledby={titleId} className="date-time-field-grid" onKeyDown={handleGridKeyDown}>
            <div role="row" className="date-time-field-week">
              {weekdayNames(locale, weekStartsOn).map((weekday) => <span role="columnheader" aria-label={weekday.long} className="date-time-field-weekday" key={weekday.long}>{weekday.short}</span>)}
            </div>
            {Array.from({ length: 6 }, (_, week) => <div role="row" className="date-time-field-week" key={week}>
              {monthGrid(shownMonth.year, shownMonth.month, weekStartsOn).slice(week * 7, week * 7 + 7).map((date, index) => date ? <div role="gridcell" aria-selected={sameDate(date, parsed?.date ?? null)} key={dateKey(date)}>
                <button
                  type="button"
                  className="date-time-field-day"
                  data-date={dateKey(date)}
                  data-selected={sameDate(date, parsed?.date ?? null) || undefined}
                  data-outside={date.month !== shownMonth.month || undefined}
                  aria-label={formatDayLabel(date, locale)}
                  aria-current={sameDate(date, today?.date ?? null) ? "date" : undefined}
                  aria-disabled={!dateAllowed(mode, date, limits, seconds) || undefined}
                  tabIndex={sameDate(date, focusDate) ? 0 : -1}
                  onClick={() => pickDate(date)}
                >{date.day}</button>
              </div> : <div role="gridcell" key={`empty-${index}`} />)}
            </div>)}
          </div> : <MonthChooser
            label={text("chooseMonth")}
            year={shownMonth.year}
            shown={shownMonth.month}
            locale={locale}
            isDisabled={(month) => !monthAllowed(mode, shownMonth.year, month, limits)}
            onSelect={(month) => {
              setFocusDate({ ...shownMonth, month, day: Math.min(focusDate.day, daysInMonth(shownMonth.year, month)) });
              setPanel("days");
              focusRequest.current = "day";
            }}
            onStepYear={(direction) => moveFocus(addYears(focusDate, direction), null)}
          />}
        </div>}
        {mode !== "date" && <div className="date-time-field-time">
          <div className="date-time-field-time-inputs">
            <TimeInput
              label={text("hours")}
              value={currentTime ? displayHour(currentTime.hour, hourCycle) : null}
              max={23}
              onCommit={(typed) => pickTime({ ...shownTime, hour: typedHour(typed, shownTime.hour, hourCycle) })}
              onStep={(direction) => pickTime({ ...shownTime, hour: stepHour(shownTime.hour, direction) })}
              onFilled={() => popoverRef.current?.querySelectorAll<HTMLElement>(".date-time-field-time-input")[1]?.focus()}
            />
            <span aria-hidden="true">:</span>
            <TimeInput
              label={text("minutes")}
              value={currentTime?.minute ?? null}
              max={59}
              onCommit={(minute) => pickTime({ ...shownTime, minute })}
              onStep={(direction) => pickTime({ ...shownTime, minute: stepMinute(shownTime.minute, direction, step) })}
            />
            {hourCycle === "h12" && <button
              type="button"
              className="date-time-field-period"
              title={text("switchPeriod")}
              aria-disabled={periodBlocked || undefined}
              onClick={() => { if (!periodBlocked) pickTime({ ...shownTime, hour: togglePeriod(shownTime.hour) }); }}
            >{periodLabel(shownTime.hour, locale)}</button>}
          </div>
          <div className="date-time-field-columns">
            <OptionColumn
              label={text("hours")}
              options={hourOptions()}
              selected={currentTime?.hour ?? null}
              format={(hour) => hourLabel(hour, locale, hourCycle)}
              isDisabled={(hour) => !hourAllowed(mode, timeDate, hour, limits, seconds)}
              onSelect={(hour) => pickTime({ ...shownTime, hour })}
            />
            <OptionColumn
              label={text("minutes")}
              options={minuteOptions(step, currentTime?.minute)}
              selected={currentTime?.minute ?? null}
              isDisabled={(minute) => currentTime !== null && !minuteAllowed(mode, timeDate, { hour: currentTime.hour, minute }, limits, seconds)}
              onSelect={(minute) => pickTime({ ...shownTime, minute })}
            />
          </div>
        </div>}
      </div>
      <div className="date-time-field-footer">
        <button type="button" className="date-time-field-action" disabled={nowDisabled} onClick={(event) => pickNow(event.currentTarget)}>{text(mode === "date" ? "today" : "now")}</button>
        {clearable && <button type="button" className="date-time-field-action" onClick={() => { closePopover(true); emit(""); }}>{text("clear")}</button>}
        {mode !== "date" && <button type="button" className="date-time-field-done" onClick={() => closePopover(true)}>{text("done")}</button>}
      </div>
    </div>}
  </div>;
}
