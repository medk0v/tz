import { describe, expect, it } from "vitest";
import type { TaskSchedule } from "../task-orchestration-api";
import { nextOccurrence, scheduleOccurrences, startOfWeek, validSchedule, zonedTimeToInstant } from "./task-schedule";

const utc = (value: string) => new Date(value);
const iso = (dates: Date[]) => dates.map((date) => date.toISOString());

describe("task schedule occurrences", () => {
  it("accepts past one-time dates for saving and still rejects missing or invalid dates", () => {
    expect(validSchedule({ kind: "once", run_at: "2000-01-01T09:00:00Z" })).toBe(true);
    expect(validSchedule({ kind: "once", run_at: "" })).toBe(false);
    expect(validSchedule({ kind: "once", run_at: "invalid" })).toBe(false);
  });

  it("expands recurring schedules in their own time zone and skips the past", () => {
    const daily: TaskSchedule = { kind: "daily", time: "09:00", timezone: "Europe/Moscow" };
    expect(iso(scheduleOccurrences(daily, utc("2026-09-14T00:00:00Z"), utc("2026-09-21T00:00:00Z"), utc("2026-09-16T07:00:00Z")))).toEqual([
      "2026-09-17T06:00:00.000Z", "2026-09-18T06:00:00.000Z", "2026-09-19T06:00:00.000Z", "2026-09-20T06:00:00.000Z",
    ]);
    const weekly: TaskSchedule = { kind: "weekly", time: "14:30", timezone: "Asia/Tokyo", weekdays: [1, 3] };
    expect(iso(scheduleOccurrences(weekly, utc("2026-09-13T15:00:00Z"), utc("2026-09-27T15:00:00Z"), utc("2026-09-01T00:00:00Z")))).toEqual([
      "2026-09-14T05:30:00.000Z", "2026-09-16T05:30:00.000Z", "2026-09-21T05:30:00.000Z", "2026-09-23T05:30:00.000Z",
    ]);
  });

  it("includes one-time runs only inside the period and after now", () => {
    const once: TaskSchedule = { kind: "once", run_at: "2026-09-20T10:00:00Z" };
    expect(iso(scheduleOccurrences(once, utc("2026-09-01T00:00:00Z"), utc("2026-10-01T00:00:00Z"), utc("2026-09-18T00:00:00Z")))).toEqual(["2026-09-20T10:00:00.000Z"]);
    expect(scheduleOccurrences(once, utc("2026-09-01T00:00:00Z"), utc("2026-10-01T00:00:00Z"), utc("2026-09-21T00:00:00Z"))).toEqual([]);
    expect(scheduleOccurrences({ kind: "manual" }, utc("2026-09-01T00:00:00Z"), utc("2026-10-01T00:00:00Z"), utc("2026-09-01T00:00:00Z"))).toEqual([]);
  });

  it("resolves skipped and repeated local times like the scheduler", () => {
    const berlin: TaskSchedule = { kind: "daily", time: "02:30", timezone: "Europe/Berlin" };
    expect(nextOccurrence(berlin, utc("2026-03-28T12:00:00Z"))?.toISOString()).toBe("2026-03-29T01:00:00.000Z");
    expect(nextOccurrence(berlin, utc("2026-10-24T12:00:00Z"))?.toISOString()).toBe("2026-10-25T00:30:00.000Z");
    expect(zonedTimeToInstant({ year: 2026, month: 9, day: 18, hour: 9, minute: 0 }, "UTC")).toBe(Date.parse("2026-09-18T09:00:00Z"));
  });

  it("skips missing month days, keeps leap days and stops at the deadline", () => {
    const monthly: TaskSchedule = { kind: "monthly", time: "08:00", timezone: "UTC", month_days: [31] };
    expect(iso(scheduleOccurrences(monthly, utc("2026-09-01T00:00:00Z"), utc("2026-11-01T00:00:00Z"), utc("2026-09-01T00:00:00Z")))).toEqual(["2026-10-31T08:00:00.000Z"]);
    const leap: TaskSchedule = { kind: "yearly", time: "08:00", timezone: "UTC", dates: [{ month: 2, day: 29 }] };
    expect(nextOccurrence(leap, utc("2026-03-01T00:00:00Z"))?.toISOString()).toBe("2028-02-29T08:00:00.000Z");
    const limited: TaskSchedule = { kind: "daily", time: "09:00", timezone: "Europe/Moscow", ends_at: "2026-09-20T09:00:00" };
    expect(iso(scheduleOccurrences(limited, utc("2026-09-18T00:00:00Z"), utc("2026-09-25T00:00:00Z"), utc("2026-09-18T00:00:00Z")))).toEqual([
      "2026-09-18T06:00:00.000Z", "2026-09-19T06:00:00.000Z", "2026-09-20T06:00:00.000Z",
    ]);
    expect(scheduleOccurrences({ kind: "daily", time: "09:00", timezone: "Not/AZone" }, utc("2026-09-18T00:00:00Z"), utc("2026-09-25T00:00:00Z"), utc("2026-09-18T00:00:00Z"))).toEqual([]);
  });
});

describe("calendar weeks", () => {
  it("start on Monday by default or on the chosen weekday", () => {
    const friday = new Date(2026, 8, 18, 15, 30);
    expect(startOfWeek(friday)).toEqual(new Date(2026, 8, 14));
    expect(startOfWeek(friday, 0)).toEqual(new Date(2026, 8, 13));
    expect(startOfWeek(friday, 6)).toEqual(new Date(2026, 8, 12));
    expect(startOfWeek(new Date(2026, 8, 13), 1)).toEqual(new Date(2026, 8, 7));
  });
});
