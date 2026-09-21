import { isChatModelConnection } from "../ai-model-types";
import type { AiProfile, AiProvider } from "../api";
import type { Task, TaskSchedule } from "../task-orchestration-api";

const MINUTE = 60_000;
const DAY = 86_400_000;
/** The backend moves a nonexistent local time forward by at most one day. */
const MAX_DST_FORWARD_MINUTES = 24 * 60;
const MAX_OCCURRENCES_PER_TASK = 400;

export function taskTitle(text: string, fallback: string): string { return text.trim().split("\n")[0]?.slice(0, 100) || fallback; }
export function errorText(error: unknown, fallback: string): string {
  return error instanceof Error && error.message ? error.message : fallback;
}

export function browserTimezone(): string { return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC"; }

export function profileAvailable(profile: AiProfile | undefined, providers: AiProvider[]): boolean {
  if (!profile || profile.status !== "active") return false;
  const provider = providers.find((item) => item.id === profile.provider_connection_id);
  if (!provider && profile.execution_ready !== undefined) return profile.execution_ready;
  return provider?.status === "active" && isChatModelConnection(provider) && (provider.provider_kind === "openai" || provider.provider_kind === "openai_compatible");
}

export function validSchedule(schedule: TaskSchedule): boolean {
  if (schedule.kind === "manual") return true;
  if (schedule.kind === "once") return Number.isFinite(new Date(schedule.run_at).getTime());
  if (!/^(?:[01]\d|2[0-3]):[0-5]\d$/.test(schedule.time)) return false;
  try { new Intl.DateTimeFormat("en", { timeZone: schedule.timezone }).format(); } catch { return false; }
  if (schedule.kind === "weekly") return schedule.weekdays.length > 0;
  if (schedule.kind === "monthly") return schedule.month_days.length > 0;
  if (schedule.kind === "yearly") return schedule.dates.length > 0
    && new Set(schedule.dates.map(({ month, day }) => `${month}-${day}`)).size === schedule.dates.length
    && schedule.dates.every(({ month, day }) => {
      const date = new Date(Date.UTC(2000, month - 1, day));
      return Number.isInteger(month) && Number.isInteger(day) && date.getUTCMonth() === month - 1 && date.getUTCDate() === day;
    });
  return true;
}

interface WallTime { year: number; month: number; day: number; hour: number; minute: number }

const zoneFormatters = new Map<string, Intl.DateTimeFormat>();
function zoneFormatter(timeZone: string): Intl.DateTimeFormat {
  let formatter = zoneFormatters.get(timeZone);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat("en-US", { timeZone, hourCycle: "h23", year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric" });
    zoneFormatters.set(timeZone, formatter);
  }
  return formatter;
}

/** Wall-clock time of an instant in an IANA time zone. */
export function zonedWallTime(instant: number, timeZone: string): WallTime {
  const parts: Record<string, number> = {};
  for (const part of zoneFormatter(timeZone).formatToParts(new Date(instant))) {
    if (part.type !== "literal") parts[part.type] = Number(part.value);
  }
  return { year: parts.year, month: parts.month, day: parts.day, hour: parts.hour % 24, minute: parts.minute };
}

function wallTimeValue(time: WallTime): number { return Date.UTC(time.year, time.month - 1, time.day, time.hour, time.minute); }

/**
 * Resolve a wall-clock time like the scheduler does: the earlier instant of a
 * repeated hour, and the first existing minute after a skipped one.
 */
export function zonedTimeToInstant(time: WallTime, timeZone: string): number | null {
  const wanted = wallTimeValue(time);
  const offsetAt = (instant: number) => wallTimeValue(zonedWallTime(instant, timeZone)) - Math.floor(instant / MINUTE) * MINUTE;
  const candidates = [...new Set([wanted - offsetAt(wanted - DAY), wanted - offsetAt(wanted + DAY)])]
    .filter((instant) => wallTimeValue(zonedWallTime(instant, timeZone)) === wanted);
  if (candidates.length) return Math.min(...candidates);
  // Inside a skipped interval: the first minute whose wall time is later than wanted.
  let low = wanted - offsetAt(wanted - DAY) - MAX_DST_FORWARD_MINUTES * MINUTE;
  let high = wanted - offsetAt(wanted + DAY) + MAX_DST_FORWARD_MINUTES * MINUTE;
  if (wallTimeValue(zonedWallTime(high, timeZone)) < wanted) return null;
  while (high - low > MINUTE) {
    const middle = low + Math.floor((high - low) / 2 / MINUTE) * MINUTE;
    if (wallTimeValue(zonedWallTime(middle, timeZone)) >= wanted) high = middle; else low = middle;
  }
  return high;
}

function recurringDateMatches(schedule: Exclude<TaskSchedule, { kind: "manual" } | { kind: "once" }>, year: number, month: number, day: number): boolean {
  if (schedule.kind === "daily") return true;
  if (schedule.kind === "weekly") return schedule.weekdays.includes(((new Date(Date.UTC(year, month - 1, day)).getUTCDay() + 6) % 7) + 1);
  if (schedule.kind === "monthly") return schedule.month_days.includes(day);
  return schedule.dates.some((date) => date.month === month && date.day === day);
}

function recurrenceEnd(schedule: { ends_at?: string | null; timezone: string }): number | null {
  const match = schedule.ends_at?.match(/^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/);
  if (!match) return null;
  const [year, month, day, hour, minute] = match.slice(1).map(Number);
  const seconds = Number(schedule.ends_at?.slice(17, 19) || 0);
  const instant = zonedTimeToInstant({ year, month, day, hour, minute }, schedule.timezone);
  return instant === null ? null : instant + seconds * 1000;
}

/** Upcoming occurrences of a schedule in [from, to), strictly after now. */
export function scheduleOccurrences(schedule: TaskSchedule, from: Date, to: Date, now: Date): Date[] {
  const start = Math.max(from.getTime(), now.getTime() + 1);
  const end = to.getTime();
  if (schedule.kind === "manual" || start >= end) return [];
  if (schedule.kind === "once") {
    const runAt = new Date(schedule.run_at).getTime();
    return runAt >= start && runAt < end ? [new Date(runAt)] : [];
  }
  const time = schedule.time.match(/^(\d{2}):(\d{2})$/);
  if (!time) return [];
  try {
    const endsAt = recurrenceEnd(schedule);
    const first = zonedWallTime(start - DAY, schedule.timezone);
    const last = zonedWallTime(end + DAY, schedule.timezone);
    const lastDay = Date.UTC(last.year, last.month - 1, last.day);
    const result: Date[] = [];
    for (let date = Date.UTC(first.year, first.month - 1, first.day); date <= lastDay && result.length < MAX_OCCURRENCES_PER_TASK; date += DAY) {
      const day = new Date(date);
      const wall = { year: day.getUTCFullYear(), month: day.getUTCMonth() + 1, day: day.getUTCDate(), hour: Number(time[1]), minute: Number(time[2]) };
      if (!recurringDateMatches(schedule, wall.year, wall.month, wall.day)) continue;
      const instant = zonedTimeToInstant(wall, schedule.timezone);
      if (instant === null || instant < start || instant >= end || (endsAt !== null && instant > endsAt)) continue;
      if (result.at(-1)?.getTime() !== instant) result.push(new Date(instant));
    }
    return result;
  } catch {
    return [];
  }
}

export function taskOccurrences(task: Task, from: Date, to: Date, now: Date): Date[] {
  return scheduleOccurrences(task.schedule, from, to, now);
}

/** The first upcoming occurrence, searching as far as the scheduler does. */
export function nextOccurrence(schedule: TaskSchedule, now: Date): Date | null {
  for (const days of [32, 400, 8 * 366 + 1]) {
    const [first] = scheduleOccurrences(schedule, now, new Date(now.getTime() + days * DAY), now);
    if (first) return first;
  }
  return null;
}

export function startOfDay(date: Date): Date { return new Date(date.getFullYear(), date.getMonth(), date.getDate()); }
export function addDays(date: Date, days: number): Date { return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days, date.getHours(), date.getMinutes()); }
export function addMonths(date: Date, months: number): Date {
  const target = new Date(date.getFullYear(), date.getMonth() + months, 1);
  const lastDay = new Date(target.getFullYear(), target.getMonth() + 1, 0).getDate();
  return new Date(target.getFullYear(), target.getMonth(), Math.min(date.getDate(), lastDay));
}
/** The first day of the date's week; `weekStartsOn` is a `Date#getDay()` number and defaults to Monday. */
export function startOfWeek(date: Date, weekStartsOn = 1): Date { return addDays(startOfDay(date), -((date.getDay() - weekStartsOn + 7) % 7)); }
export function isoWeekday(date: Date): number { return ((date.getDay() + 6) % 7) + 1; }
export function sameDay(left: Date, right: Date): boolean { return left.getFullYear() === right.getFullYear() && left.getMonth() === right.getMonth() && left.getDate() === right.getDate(); }
export function dateKey(date: Date): string { return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`; }
export function timeKey(date: Date): string { return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`; }
export function withTime(date: Date, hours: number, minutes: number): Date { return new Date(date.getFullYear(), date.getMonth(), date.getDate(), hours, minutes); }
export function minutesOfDay(date: Date): number { return date.getHours() * 60 + date.getMinutes(); }
