import type { Task, TaskInput } from "../task-orchestration-api";
import { addDays, browserTimezone, startOfDay, zonedTimeToInstant, zonedWallTime } from "./task-schedule";

/** The end of an all-day span is the last minute of its day, not the next day's midnight. */
const LAST_MINUTE = { hour: 23, minute: 59 };

export type SpanEdge = "start" | "end";

/** The zone a task's span is read and written in; without one, the viewer's own. */
export function spanZone(task: Pick<TaskInput, "time_zone">): string {
  return task.time_zone || browserTimezone();
}

/** Wall-clock text for a DateTimeField: "YYYY-MM-DD" all day, "YYYY-MM-DDTHH:mm" otherwise. */
export function spanFieldValue(instant: string | null | undefined, zone: string, allDay: boolean): string {
  if (!instant) return "";
  const at = new Date(instant);
  if (Number.isNaN(at.getTime())) return "";
  const wall = zonedWallTime(at.getTime(), zone);
  const date = `${wall.year}-${String(wall.month).padStart(2, "0")}-${String(wall.day).padStart(2, "0")}`;
  return allDay ? date : `${date}T${String(wall.hour).padStart(2, "0")}:${String(wall.minute).padStart(2, "0")}`;
}

/**
 * The instant a DateTimeField's wall-clock text means in a zone. An all-day
 * value covers its whole day, so which edge of the span it is decides the time.
 */
export function spanInstant(value: string, zone: string, allDay: boolean, edge: SpanEdge): string | null {
  if (!value) return null;
  const [date, time] = value.split("T");
  const [year, month, day] = date.split("-").map(Number);
  if (!year || !month || !day) return null;
  const clock = allDay || !time
    ? (edge === "end" ? LAST_MINUTE : { hour: 0, minute: 0 })
    : { hour: Number(time.slice(0, 2)), minute: Number(time.slice(3, 5)) };
  const instant = zonedTimeToInstant({ year, month, day, ...clock }, zone);
  return instant === null ? null : new Date(instant).toISOString();
}

/** Moves the stored instants onto day boundaries, or back off them, as All Day is switched. */
export function spanForAllDay(task: Pick<TaskInput, "starts_at" | "due_at" | "time_zone">, allDay: boolean): Pick<TaskInput, "starts_at" | "due_at"> {
  const zone = spanZone(task);
  return {
    starts_at: task.starts_at ? spanInstant(spanFieldValue(task.starts_at, zone, allDay), zone, allDay, "start") : null,
    due_at: task.due_at ? spanInstant(spanFieldValue(task.due_at, zone, allDay), zone, allDay, "end") : null,
  };
}

/** A start must have an end, and must not sit after it. */
export function invalidSpan(task: Pick<TaskInput, "starts_at" | "due_at">): boolean {
  if (!task.starts_at) return false;
  if (!task.due_at) return true;
  return new Date(task.starts_at).getTime() > new Date(task.due_at).getTime();
}

/**
 * Both ends moved by however far `at` is from `anchor`, so a span keeps its
 * length wherever it is grabbed -- including a day in the middle of it.
 */
export function shiftSpan(task: Pick<Task, "starts_at" | "due_at">, anchor: Date, at: Date): { starts_at: string | null; due_at: string } {
  const due = task.due_at ? Date.parse(task.due_at) : NaN;
  if (Number.isNaN(due)) return { starts_at: null, due_at: at.toISOString() };
  const delta = at.getTime() - anchor.getTime();
  const start = task.starts_at ? Date.parse(task.starts_at) : NaN;
  return {
    starts_at: Number.isNaN(start) ? null : new Date(start + delta).toISOString(),
    due_at: new Date(due + delta).toISOString(),
  };
}

/** One minute before the next day begins. */
function endOfDay(day: Date): Date {
  return new Date(day.getFullYear(), day.getMonth(), day.getDate(), 23, 59, 59, 999);
}

/**
 * A span as the calendar draws it: one piece per day it covers, clipped to that
 * day and to the visible range. The day columns bucket an event by the day of
 * its own start, so a span that is not cut up would show on one day only.
 * A task with no start keeps being the single point its deadline always was.
 */
export function spanSegments(task: Pick<Task, "starts_at" | "due_at">, from: Date, to: Date): { at: Date; end: Date | null }[] {
  if (!task.due_at) return [];
  const due = new Date(task.due_at);
  if (Number.isNaN(due.getTime())) return [];
  const start = task.starts_at ? new Date(task.starts_at) : null;
  if (!start || Number.isNaN(start.getTime()) || start.getTime() >= due.getTime()) {
    return due >= from && due < to ? [{ at: due, end: null }] : [];
  }
  const segments: { at: Date; end: Date | null }[] = [];
  // Only the visible days are walked, so the count is bounded by the view.
  let day = new Date(Math.max(startOfDay(start).getTime(), startOfDay(from).getTime()));
  while (day.getTime() <= due.getTime() && day.getTime() < to.getTime()) {
    const at = new Date(Math.max(start.getTime(), day.getTime()));
    const end = new Date(Math.min(due.getTime(), endOfDay(day).getTime()));
    if (end >= from) segments.push({ at, end });
    day = addDays(day, 1);
  }
  return segments;
}
