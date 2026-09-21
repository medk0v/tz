import type { HourCycle, WeekStartDay } from "../date-time-preferences";
import type { Locale } from "../i18n";

export type DateTimeFieldMode = "date" | "datetime" | "time";
export interface DateParts { year: number; month: number; day: number }
export interface TimeParts { hour: number; minute: number }
/** `seconds` is the raw ":ss" / ":ss.SSS" suffix of the incoming value, re-emitted untouched. */
export interface ParsedValue { date: DateParts | null; time: TimeParts | null; seconds: string }
export interface Limits { min: number | null; max: number | null }
export type InvalidReason = "required" | "min" | "max";

export const MINUTE_MS = 60_000;
export const DAY_MINUTES = 1440;
export const DAY_MS = DAY_MINUTES * MINUTE_MS;
export const DEFAULT_TIME: TimeParts = { hour: 9, minute: 0 };
export const DEFAULT_MINUTE_STEP = 5;
export const MIN_YEAR = 1;
export const MAX_YEAR = 9999;

const DATE = String.raw`(\d{4})-(\d{2})-(\d{2})`;
const TIME = String.raw`(\d{2}):(\d{2})(:\d{2}(?:\.\d{1,3})?)?`;
const patterns: Record<DateTimeFieldMode, RegExp> = {
  date: new RegExp(`^${DATE}$`),
  datetime: new RegExp(`^${DATE}T${TIME}$`),
  time: new RegExp(`^${TIME}$`),
};

const pad = (value: number, length = 2) => String(value).padStart(length, "0");

export function isLeapYear(year: number): boolean { return (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0; }
export function daysInMonth(year: number, month: number): number {
  return month === 2 ? (isLeapYear(year) ? 29 : 28) : [4, 6, 9, 11].includes(month) ? 30 : 31;
}

function validDate(year: number, month: number, day: number): boolean {
  return year >= MIN_YEAR && year <= MAX_YEAR && month >= 1 && month <= 12 && day >= 1 && day <= daysInMonth(year, month);
}
function validTime(hour: number, minute: number, seconds: string): boolean {
  return hour <= 23 && minute <= 59 && (!seconds || Number(seconds.slice(1, 3)) <= 59);
}

/** Strict parser: anything a native input of the same type would not emit yields null. */
export function parseValue(mode: DateTimeFieldMode, value: string | null | undefined): ParsedValue | null {
  const match = typeof value === "string" ? patterns[mode].exec(value) : null;
  if (!match) return null;
  const groups = match.slice(1);
  const timeAt = mode === "time" ? 0 : 3;
  const date = mode === "time" ? null : { year: Number(groups[0]), month: Number(groups[1]), day: Number(groups[2]) };
  if (date && !validDate(date.year, date.month, date.day)) return null;
  if (mode === "date") return { date, time: null, seconds: "" };
  const time = { hour: Number(groups[timeAt]), minute: Number(groups[timeAt + 1]) };
  const seconds = groups[timeAt + 2] ?? "";
  return validTime(time.hour, time.minute, seconds) ? { date, time, seconds } : null;
}

export function dateKey(date: DateParts): string { return `${pad(date.year, 4)}-${pad(date.month)}-${pad(date.day)}`; }
export function timeKey(time: TimeParts): string { return `${pad(time.hour)}:${pad(time.minute)}`; }

export function formatValue(mode: DateTimeFieldMode, parsed: ParsedValue): string {
  if (mode === "date") return parsed.date ? dateKey(parsed.date) : "";
  if (!parsed.time) return "";
  if (mode === "time") return `${timeKey(parsed.time)}${parsed.seconds}`;
  return parsed.date ? `${dateKey(parsed.date)}T${timeKey(parsed.time)}${parsed.seconds}` : "";
}

/** Days since 1970-01-01 in the proleptic Gregorian calendar; no Date object, so no DST or two-digit-year surprises. */
export function toDayNumber({ year, month, day }: DateParts): number {
  const shiftedYear = month <= 2 ? year - 1 : year;
  const era = Math.floor(shiftedYear / 400);
  const yearOfEra = shiftedYear - era * 400;
  const dayOfYear = Math.floor((153 * (month + (month > 2 ? -3 : 9)) + 2) / 5) + day - 1;
  const dayOfEra = yearOfEra * 365 + Math.floor(yearOfEra / 4) - Math.floor(yearOfEra / 100) + dayOfYear;
  return era * 146097 + dayOfEra - 719468;
}

export function fromDayNumber(dayNumber: number): DateParts {
  const shifted = dayNumber + 719468;
  const era = Math.floor(shifted / 146097);
  const dayOfEra = shifted - era * 146097;
  const yearOfEra = Math.floor((dayOfEra - Math.floor(dayOfEra / 1460) + Math.floor(dayOfEra / 36524) - Math.floor(dayOfEra / 146096)) / 365);
  const dayOfYear = dayOfEra - (365 * yearOfEra + Math.floor(yearOfEra / 4) - Math.floor(yearOfEra / 100));
  const monthIndex = Math.floor((5 * dayOfYear + 2) / 153);
  const month = monthIndex < 10 ? monthIndex + 3 : monthIndex - 9;
  return { year: yearOfEra + era * 400 + (month <= 2 ? 1 : 0), month, day: dayOfYear - Math.floor((153 * monthIndex + 2) / 5) + 1 };
}

const FIRST_DAY = toDayNumber({ year: MIN_YEAR, month: 1, day: 1 });
const LAST_DAY = toDayNumber({ year: MAX_YEAR, month: 12, day: 31 });

/** The day's column in a week starting on `weekStartsOn` (a `Date#getDay()` number, Monday unless the user chose otherwise). */
export function weekdayIndex(date: DateParts, weekStartsOn: WeekStartDay = 1): number { return (((toDayNumber(date) + 4 - weekStartsOn) % 7) + 7) % 7; }
export function sameDate(left: DateParts | null, right: DateParts | null): boolean {
  return left !== null && right !== null && left.year === right.year && left.month === right.month && left.day === right.day;
}
export function compareDates(left: DateParts, right: DateParts): number { return toDayNumber(left) - toDayNumber(right); }
export function addDays(date: DateParts, amount: number): DateParts {
  return fromDayNumber(Math.min(LAST_DAY, Math.max(FIRST_DAY, toDayNumber(date) + amount)));
}
export function addMonths(date: DateParts, amount: number): DateParts {
  const index = Math.min(MAX_YEAR * 12 + 11, Math.max(MIN_YEAR * 12, date.year * 12 + date.month - 1 + amount));
  const year = Math.floor(index / 12);
  const month = (index % 12) + 1;
  return { year, month, day: Math.min(date.day, daysInMonth(year, month)) };
}
export function addYears(date: DateParts, amount: number): DateParts { return addMonths(date, amount * 12); }
export function startOfWeek(date: DateParts, weekStartsOn: WeekStartDay = 1): DateParts { return addDays(date, -weekdayIndex(date, weekStartsOn)); }
export function endOfWeek(date: DateParts, weekStartsOn: WeekStartDay = 1): DateParts { return addDays(date, 6 - weekdayIndex(date, weekStartsOn)); }

/** Six weeks covering the month; cells before year 1 / after year 9999 are null. */
export function monthGrid(year: number, month: number, weekStartsOn: WeekStartDay = 1): Array<DateParts | null> {
  const first = { year, month, day: 1 };
  const start = toDayNumber(first) - weekdayIndex(first, weekStartsOn);
  return Array.from({ length: 42 }, (_, index) => {
    const dayNumber = start + index;
    return dayNumber < FIRST_DAY || dayNumber > LAST_DAY ? null : fromDayNumber(dayNumber);
  });
}

export function nowParts(now: Date): { date: DateParts; time: TimeParts } {
  return { date: { year: now.getFullYear(), month: now.getMonth() + 1, day: now.getDate() }, time: { hour: now.getHours(), minute: now.getMinutes() } };
}

function secondsMs(seconds: string): number { return seconds ? Math.round(Number(seconds.slice(1)) * 1000) : 0; }
function minuteOfDay(time: TimeParts): number { return time.hour * 60 + time.minute; }

/** A comparable integer for a parsed value: milliseconds of wall-clock time (no zone involved). */
export function valueStamp(mode: DateTimeFieldMode, parsed: ParsedValue): number {
  const day = mode !== "time" && parsed.date ? toDayNumber(parsed.date) * DAY_MS : 0;
  const time = mode !== "date" && parsed.time ? minuteOfDay(parsed.time) * MINUTE_MS + secondsMs(parsed.seconds) : 0;
  return day + time;
}

export function parseLimits(mode: DateTimeFieldMode, min?: string, max?: string): Limits {
  const stamp = (limit?: string) => {
    const parsed = parseValue(mode, limit);
    return parsed ? valueStamp(mode, parsed) : null;
  };
  return { min: stamp(min), max: stamp(max) };
}

function rangeAllowed(from: number, to: number, limits: Limits): boolean {
  return (limits.max === null || from <= limits.max) && (limits.min === null || to >= limits.min);
}

export function validate(mode: DateTimeFieldMode, parsed: ParsedValue | null, limits: Limits, required: boolean): InvalidReason | null {
  if (!parsed) return required ? "required" : null;
  const stamp = valueStamp(mode, parsed);
  if (limits.min !== null && stamp < limits.min) return "min";
  if (limits.max !== null && stamp > limits.max) return "max";
  return null;
}

/** Whether any minute of the day (carrying the preserved seconds) fits the limits. */
export function dateAllowed(mode: DateTimeFieldMode, date: DateParts, limits: Limits, seconds = ""): boolean {
  const start = toDayNumber(date) * DAY_MS;
  if (mode === "date") return rangeAllowed(start, start, limits);
  return rangeAllowed(start + secondsMs(seconds), start + (DAY_MINUTES - 1) * MINUTE_MS + secondsMs(seconds), limits);
}
export function monthAllowed(mode: DateTimeFieldMode, year: number, month: number, limits: Limits): boolean {
  const start = toDayNumber({ year, month, day: 1 }) * DAY_MS;
  const end = toDayNumber({ year, month, day: daysInMonth(year, month) }) * DAY_MS + (mode === "date" ? 0 : DAY_MS - 1);
  return rangeAllowed(start, end, limits);
}
/** The nearest day inside the limits; time-of-day limits (time mode) never move a date. */
export function clampDate(mode: DateTimeFieldMode, date: DateParts, limits: Limits): DateParts {
  if (mode === "time") return date;
  const dayNumber = toDayNumber(date);
  if (limits.min !== null && dayNumber < Math.floor(limits.min / DAY_MS)) return fromDayNumber(Math.floor(limits.min / DAY_MS));
  if (limits.max !== null && dayNumber > Math.floor(limits.max / DAY_MS)) return fromDayNumber(Math.floor(limits.max / DAY_MS));
  return date;
}

function dayStart(mode: DateTimeFieldMode, date: DateParts | null): number { return mode === "datetime" && date ? toDayNumber(date) * DAY_MS : 0; }
export function hourAllowed(mode: DateTimeFieldMode, date: DateParts | null, hour: number, limits: Limits, seconds = ""): boolean {
  const start = dayStart(mode, date) + hour * 60 * MINUTE_MS + secondsMs(seconds);
  return rangeAllowed(start, start + 59 * MINUTE_MS, limits);
}
export function minuteAllowed(mode: DateTimeFieldMode, date: DateParts | null, time: TimeParts, limits: Limits, seconds = ""): boolean {
  const stamp = dayStart(mode, date) + minuteOfDay(time) * MINUTE_MS + secondsMs(seconds);
  return rangeAllowed(stamp, stamp, limits);
}

export function normalizeMinuteStep(step: number | undefined): number {
  return typeof step === "number" && Number.isFinite(step) && Math.round(step) >= 1 && Math.round(step) <= 60 ? Math.round(step) : DEFAULT_MINUTE_STEP;
}
export function hourOptions(): number[] { return Array.from({ length: 24 }, (_, hour) => hour); }
/** Multiples of the step, plus the current minute when it sits off the step. */
export function minuteOptions(step: number, current?: number | null): number[] {
  const size = normalizeMinuteStep(step);
  const options = Array.from({ length: Math.ceil(60 / size) }, (_, index) => index * size);
  if (typeof current === "number" && current >= 0 && current <= 59 && !options.includes(current)) options.push(current);
  return options.sort((left, right) => left - right);
}

/**
 * Moves a time on the given day into [min, max]. A time pushed up to the lower limit is rounded up to the
 * minute step while that stays on the same day and inside the range; one pushed down is rounded down.
 */
export function clampTime(mode: DateTimeFieldMode, date: DateParts | null, time: TimeParts, limits: Limits, step: number, seconds = ""): TimeParts {
  const size = normalizeMinuteStep(step);
  const base = dayStart(mode, date) + secondsMs(seconds);
  const lowest = limits.min === null ? 0 : Math.max(0, Math.ceil((limits.min - base) / MINUTE_MS));
  const highest = limits.max === null ? DAY_MINUTES - 1 : Math.min(DAY_MINUTES - 1, Math.floor((limits.max - base) / MINUTE_MS));
  const current = minuteOfDay(time);
  if (lowest > highest || (current >= lowest && current <= highest)) return time;
  let next: number;
  if (current < lowest) {
    const minute = lowest % 60;
    const stepped = Math.ceil(minute / size) * size;
    const rounded = lowest - minute + (stepped > 59 ? 60 : stepped);
    next = rounded <= highest ? rounded : lowest;
  } else {
    const rounded = highest - (highest % 60) + Math.floor((highest % 60) / size) * size;
    next = rounded >= lowest ? rounded : highest;
  }
  return { hour: Math.floor(next / 60), minute: next % 60 };
}

export function stepHour(hour: number, direction: 1 | -1): number { return (hour + direction + 24) % 24; }
/** The number shown in the hour box: 1–12 on a 12-hour clock. */
export function displayHour(hour: number, hourCycle: HourCycle): number { return hourCycle === "h12" ? hour % 12 || 12 : hour; }
/** On a 12-hour clock a typed 1–12 stays in the current half of the day; 0 and 13–23 can only mean the 24-hour value. */
export function typedHour(typed: number, current: number, hourCycle: HourCycle): number {
  const hour = Math.min(23, typed);
  return hourCycle === "h23" || hour === 0 || hour > 12 ? hour : (hour % 12) + (current >= 12 ? 12 : 0);
}
export function togglePeriod(hour: number): number { return (hour + 12) % 24; }
/** Next / previous multiple of the step, wrapping inside the hour. */
export function stepMinute(minute: number, direction: 1 | -1, step: number): number {
  const size = normalizeMinuteStep(step);
  const next = direction === 1 ? (Math.floor(minute / size) + 1) * size : (Math.ceil(minute / size) - 1) * size;
  if (next > 59) return 0;
  return next < 0 ? Math.floor(59 / size) * size : next;
}

const formatters = new Map<string, Intl.DateTimeFormat>();
function formatter(locale: Locale, options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const key = `${locale}:${JSON.stringify(options)}`;
  let cached = formatters.get(key);
  if (!cached) {
    cached = new Intl.DateTimeFormat(locale, { ...options, timeZone: "UTC" });
    formatters.set(key, cached);
  }
  return cached;
}
/** The calendar date at UTC midnight, only ever handed to formatters pinned to UTC. */
function formatDate(date: DateParts): Date {
  const instant = new Date(0);
  instant.setUTCFullYear(date.year, date.month - 1, date.day);
  return instant;
}
function capitalize(value: string, locale: Locale): string { return value.charAt(0).toLocaleUpperCase(locale) + value.slice(1); }
function formatClock(time: TimeParts): Date { return new Date(Date.UTC(2024, 0, 1, time.hour, time.minute)); }

const localeCycles = new Map<Locale, HourCycle>();
/** The user's forced hour cycle, or the one the interface language uses — what every other time in the cabinet follows. */
export function resolveHourCycle(locale: Locale, preferred?: HourCycle): HourCycle {
  if (preferred) return preferred;
  let cycle = localeCycles.get(locale);
  if (!cycle) {
    const resolved = new Intl.DateTimeFormat(locale, { hour: "numeric" }).resolvedOptions().hourCycle;
    cycle = resolved === "h12" || resolved === "h11" ? "h12" : "h23";
    localeCycles.set(locale, cycle);
  }
  return cycle;
}
/** "15:30" on a 24-hour clock whatever the language, the language's own "3:30 PM" on a 12-hour one. */
export function formatTime(time: TimeParts, locale: Locale, hourCycle: HourCycle = "h23"): string {
  return hourCycle === "h12" ? formatter(locale, { hour: "numeric", minute: "2-digit", hourCycle }).format(formatClock(time)) : timeKey(time);
}
/** An entry of the hour list: "15", or "3 PM". */
export function hourLabel(hour: number, locale: Locale, hourCycle: HourCycle = "h23"): string {
  return hourCycle === "h12" ? formatter(locale, { hour: "numeric", hourCycle }).format(formatClock({ hour, minute: 0 })) : pad(hour);
}
/** The language's name for the half of the day an hour falls into: "AM" / "PM", "a.m." / "p.m.". */
export function periodLabel(hour: number, locale: Locale): string {
  const parts = formatter(locale, { hour: "numeric", hourCycle: "h12" }).formatToParts(formatClock({ hour, minute: 0 }));
  return parts.find((part) => part.type === "dayPeriod")?.value ?? (hour < 12 ? "AM" : "PM");
}
function dateText(locale: Locale, date: DateParts, month: "short" | "long"): string {
  // The Russian pattern ends with a "г." literal that only adds noise in a field.
  return formatter(locale, { day: "numeric", month, year: "numeric" }).formatToParts(formatDate(date))
    .map((part) => (part.type === "literal" ? part.value.replace(/\s*г\.?/u, "") : part.value)).join("").trim();
}

/** Trigger text: "Sep 19, 2026, 15:30" / "19 сент. 2026, 15:30" / "09:00" / "Sep 19, 2026, 3:30 PM". */
export function formatDisplay(mode: DateTimeFieldMode, parsed: ParsedValue | null, locale: Locale, hourCycle: HourCycle = "h23"): string {
  if (!parsed) return "";
  if (mode === "time") return parsed.time ? formatTime(parsed.time, locale, hourCycle) : "";
  if (!parsed.date) return "";
  const date = dateText(locale, parsed.date, "short");
  return mode === "datetime" && parsed.time ? `${date}, ${formatTime(parsed.time, locale, hourCycle)}` : date;
}
/** Human text for a min/max limit inside an error message. */
export function formatLimit(mode: DateTimeFieldMode, limit: string | undefined, locale: Locale, hourCycle: HourCycle = "h23"): string {
  return formatDisplay(mode, parseValue(mode, limit), locale, hourCycle);
}
/** Day-first long date used as a day's accessible name: "19 September 2026". */
export function formatDayLabel(date: DateParts, locale: Locale): string {
  const parts = formatter(locale, { day: "numeric", month: "long", year: "numeric" }).formatToParts(formatDate(date));
  const month = parts.find((part) => part.type === "month")?.value ?? "";
  return `${date.day} ${month} ${date.year}`;
}
export function monthName(month: number, locale: Locale, width: "long" | "short" = "long"): string {
  return capitalize(formatter(locale, { month: width }).format(formatDate({ year: 2024, month, day: 1 })), locale);
}
export function formatMonthTitle(year: number, month: number, locale: Locale): string { return `${monthName(month, locale)} ${year}`; }
/** Weekday names in the order of a week starting on `weekStartsOn`. */
export function weekdayNames(locale: Locale, weekStartsOn: WeekStartDay = 1): Array<{ short: string; long: string }> {
  // 2024-01-07 is a Sunday, day 0 of `Date#getDay()`.
  return Array.from({ length: 7 }, (_, index) => {
    const day = formatDate({ year: 2024, month: 1, day: 7 + ((weekStartsOn + index) % 7) });
    return { short: formatter(locale, { weekday: "short" }).format(day), long: formatter(locale, { weekday: "long" }).format(day) };
  });
}

export interface PopoverBox { top: number; left: number; placement: "bottom" | "top" }
interface Box { top: number; bottom: number; left: number }
/** Below-start by default, above when only that side fits, always kept inside the viewport margin. */
export function placePopover(trigger: Box, size: { width: number; height: number }, viewport: { width: number; height: number }, margin = 8, gap = 4): PopoverBox {
  const below = viewport.height - margin - (trigger.bottom + gap);
  const above = trigger.top - gap - margin;
  const placement = size.height <= below || below >= above ? "bottom" : "top";
  const wantedTop = placement === "bottom" ? trigger.bottom + gap : trigger.top - gap - size.height;
  const clamp = (value: number, limit: number) => Math.round(Math.max(margin, Math.min(value, limit)));
  return { placement, top: clamp(wantedTop, viewport.height - margin - size.height), left: clamp(trigger.left, viewport.width - margin - size.width) };
}
