import { describe, expect, it } from "vitest";
import { invalidSpan, shiftSpan, spanFieldValue, spanForAllDay, spanInstant, spanSegments, spanZone } from "./task-span";

const ISTANBUL = "Europe/Istanbul";

describe("task span", () => {
  it("reads and writes wall-clock time in the task's own zone", () => {
    // 09:00 in Istanbul is 06:00 UTC, whatever zone the viewer sits in.
    expect(spanInstant("2026-03-10T09:00", ISTANBUL, false, "start")).toBe("2026-03-10T06:00:00.000Z");
    expect(spanFieldValue("2026-03-10T06:00:00.000Z", ISTANBUL, false)).toBe("2026-03-10T09:00");
    expect(spanFieldValue("2026-03-10T06:00:00.000Z", "UTC", false)).toBe("2026-03-10T06:00");
  });

  it("falls back to the viewer's zone for a task that carries none", () => {
    expect(spanZone({ time_zone: ISTANBUL })).toBe(ISTANBUL);
    expect(spanZone({ time_zone: null })).toBe(Intl.DateTimeFormat().resolvedOptions().timeZone);
  });

  it("covers a whole day in the chosen zone, each edge at its own end of it", () => {
    expect(spanInstant("2026-03-10", ISTANBUL, true, "start")).toBe("2026-03-09T21:00:00.000Z");
    expect(spanInstant("2026-03-10", ISTANBUL, true, "end")).toBe("2026-03-10T20:59:00.000Z");
    expect(spanFieldValue("2026-03-09T21:00:00.000Z", ISTANBUL, true)).toBe("2026-03-10");
  });

  it("moves the stored instants onto day boundaries as All Day is switched on", () => {
    const task = { starts_at: "2026-03-10T11:30:00.000Z", due_at: "2026-03-10T14:00:00.000Z", time_zone: ISTANBUL };
    expect(spanForAllDay(task, true)).toEqual({ starts_at: "2026-03-09T21:00:00.000Z", due_at: "2026-03-10T20:59:00.000Z" });
    // Switching it back off keeps whatever instants the task already has.
    expect(spanForAllDay(task, false)).toEqual({ starts_at: task.starts_at, due_at: task.due_at });
  });

  it("empty values stay empty rather than becoming an epoch", () => {
    expect(spanFieldValue(null, ISTANBUL, false)).toBe("");
    expect(spanFieldValue("not a date", ISTANBUL, false)).toBe("");
    expect(spanInstant("", ISTANBUL, false, "start")).toBeNull();
  });

  it("rejects a start without an end, or after it", () => {
    expect(invalidSpan({ starts_at: null, due_at: null })).toBe(false);
    expect(invalidSpan({ starts_at: null, due_at: "2026-03-10T14:00:00.000Z" })).toBe(false);
    expect(invalidSpan({ starts_at: "2026-03-10T11:00:00.000Z", due_at: null })).toBe(true);
    expect(invalidSpan({ starts_at: "2026-03-10T15:00:00.000Z", due_at: "2026-03-10T14:00:00.000Z" })).toBe(true);
    expect(invalidSpan({ starts_at: "2026-03-10T14:00:00.000Z", due_at: "2026-03-10T14:00:00.000Z" })).toBe(false);
  });

  it("a move carries the whole span by however far the grabbed piece went", () => {
    const span = { starts_at: "2026-03-10T09:00:00.000Z", due_at: "2026-03-10T11:30:00.000Z" };
    const start = new Date(span.starts_at);
    expect(shiftSpan(span, start, new Date("2026-03-12T15:00:00.000Z")))
      .toEqual({ starts_at: "2026-03-12T15:00:00.000Z", due_at: "2026-03-12T17:30:00.000Z" });
    // Grabbed by its end, the span still moves by the delta rather than collapsing.
    expect(shiftSpan(span, new Date(span.due_at), new Date("2026-03-12T11:30:00.000Z")))
      .toEqual({ starts_at: "2026-03-12T09:00:00.000Z", due_at: "2026-03-12T11:30:00.000Z" });
    // A bare deadline keeps landing exactly where it was dropped.
    const due = "2026-03-10T11:30:00.000Z";
    expect(shiftSpan({ starts_at: null, due_at: due }, new Date(due), new Date("2026-03-12T15:00:00.000Z")))
      .toEqual({ starts_at: null, due_at: "2026-03-12T15:00:00.000Z" });
  });

  it("cuts a span into one piece per day it covers, clipped to the view", () => {
    // Built from local wall-clock time, because the calendar buckets by the viewer's day.
    const local = (month: number, day: number, hour: number, minute = 0) => new Date(2026, month, day, hour, minute).toISOString();
    const from = new Date(2026, 2, 1);
    const to = new Date(2026, 3, 1);
    const pieces = spanSegments({ starts_at: local(2, 10, 14), due_at: local(2, 12, 11) }, from, to);
    expect(pieces).toHaveLength(3);
    expect(pieces[0].at.toISOString()).toBe(local(2, 10, 14));
    expect(pieces[0].end?.toISOString()).toBe(new Date(2026, 2, 10, 23, 59, 59, 999).toISOString());
    // A whole middle day runs from its own midnight to its own end.
    expect(pieces[1].at.toISOString()).toBe(local(2, 11, 0));
    expect(pieces[1].end?.toISOString()).toBe(new Date(2026, 2, 11, 23, 59, 59, 999).toISOString());
    expect(pieces[2].end?.toISOString()).toBe(local(2, 12, 11));

    // A span reaching in from before the view starts at the first visible day.
    const clipped = spanSegments({ starts_at: local(1, 25, 9), due_at: local(2, 2, 9) }, from, to);
    expect(clipped).toHaveLength(2);
    expect(clipped[0].at.toISOString()).toBe(local(2, 1, 0));

    // A deadline without a start is still the single point it always was.
    const point = spanSegments({ starts_at: null, due_at: local(2, 10, 11) }, from, to);
    expect(point).toEqual([{ at: new Date(local(2, 10, 11)), end: null }]);
    expect(spanSegments({ starts_at: null, due_at: local(5, 1, 11) }, from, to)).toEqual([]);
    expect(spanSegments({ starts_at: null, due_at: null }, from, to)).toEqual([]);
  });
});
