import { describe, expect, it } from "vitest";
import {
  addDays, addMonths, addYears, clampDate, clampTime, dateAllowed, dateKey, daysInMonth, displayHour, endOfWeek, formatDayLabel, formatDisplay, formatLimit, formatMonthTitle,
  formatTime, formatValue, fromDayNumber, hourAllowed, hourLabel, minuteAllowed, minuteOptions, monthAllowed, monthGrid, monthName, normalizeMinuteStep, nowParts, parseLimits, parseValue,
  periodLabel, placePopover, resolveHourCycle, startOfWeek, stepHour, stepMinute, toDayNumber, togglePeriod, typedHour, validate, weekdayIndex, weekdayNames,
} from "./date-time-field-model";

const date = (year: number, month: number, day: number) => ({ year, month, day });
const time = (hour: number, minute: number) => ({ hour, minute });

describe("date-time field values", () => {
  it("parses exactly the strings a native input emits", () => {
    expect(parseValue("date", "2026-09-19")).toEqual({ date: date(2026, 9, 19), time: null, seconds: "" });
    expect(parseValue("datetime", "2026-09-19T15:30")).toEqual({ date: date(2026, 9, 19), time: time(15, 30), seconds: "" });
    expect(parseValue("datetime", "2026-09-19T15:30:45")).toEqual({ date: date(2026, 9, 19), time: time(15, 30), seconds: ":45" });
    expect(parseValue("datetime", "2026-09-19T15:30:45.250")?.seconds).toBe(":45.250");
    expect(parseValue("time", "09:05")).toEqual({ date: null, time: time(9, 5), seconds: "" });
    expect(parseValue("date", "2024-02-29")?.date).toEqual(date(2024, 2, 29));
  });

  it("rejects everything else without throwing", () => {
    const rejected: Array<[Parameters<typeof parseValue>[0], string | null | undefined]> = [
      ["date", ""], ["date", "2026-9-19"], ["date", "2026-02-30"], ["date", "2026-13-01"], ["date", "2026-00-10"], ["date", "2025-02-29"], ["date", "0000-01-01"],
      ["date", "2026-09-19T10:00"], ["date", " 2026-09-19"], ["date", "19.09.2026"], ["date", null], ["date", undefined],
      ["datetime", "2026-09-19"], ["datetime", "2026-09-19 15:30"], ["datetime", "2026-09-19T24:00"], ["datetime", "2026-09-19T15:60"], ["datetime", "2026-09-19T15:30:60"],
      ["datetime", "2026-09-19T15:30Z"], ["datetime", "2026-09-19T15:30:00+02:00"],
      ["time", "9:00"], ["time", "24:00"], ["time", "12:60"], ["time", "noon"],
    ];
    for (const [mode, value] of rejected) expect(parseValue(mode, value), `${mode} ${String(value)}`).toBeNull();
  });

  it("formats back to the same shape and never invents seconds", () => {
    expect(formatValue("date", { date: date(2026, 9, 5), time: null, seconds: "" })).toBe("2026-09-05");
    expect(formatValue("datetime", { date: date(2026, 9, 5), time: time(7, 0), seconds: "" })).toBe("2026-09-05T07:00");
    expect(formatValue("datetime", { date: date(2026, 9, 5), time: time(7, 0), seconds: ":00" })).toBe("2026-09-05T07:00:00");
    expect(formatValue("time", { date: null, time: time(23, 59), seconds: "" })).toBe("23:59");
    expect(formatValue("datetime", { date: null, time: time(7, 0), seconds: "" })).toBe("");
    expect(dateKey(date(987, 1, 2))).toBe("0987-01-02");
    for (const value of ["2026-09-19T15:30", "2026-09-19T15:30:45", "2026-09-19T15:30:45.250"]) expect(formatValue("datetime", parseValue("datetime", value)!)).toBe(value);
  });

  it("keeps the seconds of the incoming value when another day or time is chosen", () => {
    const parsed = parseValue("datetime", "2026-09-19T15:30:45")!;
    expect(formatValue("datetime", { ...parsed, date: date(2026, 10, 1) })).toBe("2026-10-01T15:30:45");
    expect(formatValue("datetime", { ...parsed, time: time(8, 5) })).toBe("2026-09-19T08:05:45");
  });
});

describe("date-time field calendar math", () => {
  it("converts dates to day numbers and back across centuries", () => {
    expect(toDayNumber(date(1970, 1, 1))).toBe(0);
    expect(toDayNumber(date(2026, 9, 19))).toBe(Date.UTC(2026, 8, 19) / 86_400_000);
    for (const sample of [date(1, 1, 1), date(99, 12, 31), date(1900, 2, 28), date(2000, 2, 29), date(2024, 12, 31), date(9999, 12, 31)]) expect(fromDayNumber(toDayNumber(sample))).toEqual(sample);
    expect(daysInMonth(2024, 2)).toBe(29);
    expect(daysInMonth(1900, 2)).toBe(28);
    expect(daysInMonth(2000, 2)).toBe(29);
    expect(daysInMonth(2026, 9)).toBe(30);
  });

  it("starts weeks on Monday", () => {
    expect(weekdayIndex(date(2026, 9, 14))).toBe(0);
    expect(weekdayIndex(date(2026, 9, 19))).toBe(5);
    expect(weekdayIndex(date(2026, 9, 20))).toBe(6);
    expect(weekdayIndex(date(1969, 12, 28))).toBe(6);
    expect(startOfWeek(date(2026, 9, 19))).toEqual(date(2026, 9, 14));
    expect(endOfWeek(date(2026, 9, 19))).toEqual(date(2026, 9, 20));
    expect(startOfWeek(date(2026, 10, 1))).toEqual(date(2026, 9, 28));
  });

  it("starts weeks on Sunday or Saturday when asked to", () => {
    expect(weekdayIndex(date(2026, 9, 20), 0)).toBe(0);
    expect(weekdayIndex(date(2026, 9, 19), 0)).toBe(6);
    expect(weekdayIndex(date(2026, 9, 19), 6)).toBe(0);
    expect(weekdayIndex(date(1969, 12, 28), 0)).toBe(0);
    expect(startOfWeek(date(2026, 9, 19), 0)).toEqual(date(2026, 9, 13));
    expect(endOfWeek(date(2026, 9, 19), 0)).toEqual(date(2026, 9, 19));
    expect(endOfWeek(date(2026, 9, 20), 6)).toEqual(date(2026, 9, 25));
    expect(monthGrid(2026, 9, 0)[0]).toEqual(date(2026, 8, 30));
    expect(monthGrid(2026, 9, 6)[0]).toEqual(date(2026, 8, 29));
    expect(monthGrid(2026, 9, 0).every((cell, index) => cell !== null && weekdayIndex(cell, 0) === index % 7)).toBe(true);
  });

  it("moves across month and year boundaries and clamps the day of month", () => {
    expect(addDays(date(2026, 9, 30), 1)).toEqual(date(2026, 10, 1));
    expect(addDays(date(2026, 1, 1), -1)).toEqual(date(2025, 12, 31));
    expect(addDays(date(2026, 3, 28), 7)).toEqual(date(2026, 4, 4));
    expect(addMonths(date(2026, 1, 31), 1)).toEqual(date(2026, 2, 28));
    expect(addMonths(date(2026, 12, 15), 1)).toEqual(date(2027, 1, 15));
    expect(addMonths(date(2026, 1, 15), -1)).toEqual(date(2025, 12, 15));
    expect(addYears(date(2024, 2, 29), 1)).toEqual(date(2025, 2, 28));
    expect(addYears(date(9999, 6, 1), 1)).toEqual(date(9999, 12, 1));
    expect(addDays(date(9999, 12, 31), 1)).toEqual(date(9999, 12, 31));
    expect(addDays(date(1, 1, 1), -1)).toEqual(date(1, 1, 1));
  });

  it("builds a six-week Monday-first grid", () => {
    const september = monthGrid(2026, 9);
    expect(september).toHaveLength(42);
    expect(september[0]).toEqual(date(2026, 8, 31));
    expect(september[1]).toEqual(date(2026, 9, 1));
    expect(september[41]).toEqual(date(2026, 10, 11));
    expect(monthGrid(2027, 2)[0]).toEqual(date(2027, 2, 1));
    expect(monthGrid(2026, 9).every((cell, index) => cell !== null && weekdayIndex(cell) === index % 7)).toBe(true);
    expect(monthGrid(1, 1).filter((cell) => cell === null)).toHaveLength(weekdayIndex(date(1, 1, 1)));
  });

  it("reads the local wall clock of a Date", () => {
    expect(nowParts(new Date(2026, 8, 19, 7, 4, 59))).toEqual({ date: date(2026, 9, 19), time: time(7, 4) });
  });
});

describe("date-time field limits", () => {
  it("validates required, min and max per mode and ignores unparsable limits", () => {
    const limits = parseLimits("datetime", "2026-09-19T12:00", "2026-09-25T18:00");
    expect(validate("datetime", null, limits, true)).toBe("required");
    expect(validate("datetime", null, limits, false)).toBeNull();
    expect(validate("datetime", parseValue("datetime", "2026-09-19T11:59"), limits, true)).toBe("min");
    expect(validate("datetime", parseValue("datetime", "2026-09-19T12:00"), limits, true)).toBeNull();
    expect(validate("datetime", parseValue("datetime", "2026-09-25T18:00"), limits, true)).toBeNull();
    expect(validate("datetime", parseValue("datetime", "2026-09-25T18:00:01"), limits, true)).toBe("max");
    expect(validate("date", parseValue("date", "2026-09-18"), parseLimits("date", "2026-09-19"), false)).toBe("min");
    expect(validate("time", parseValue("time", "18:01"), parseLimits("time", undefined, "18:00"), false)).toBe("max");
    expect(parseLimits("date", "yesterday", "2026-09-19T10:00")).toEqual({ min: null, max: null });
    expect(validate("date", parseValue("date", "1600-01-01"), parseLimits("date", "1700-01-01"), false)).toBe("min");
  });

  it("marks days, months, hours and minutes outside the limits", () => {
    const limits = parseLimits("datetime", "2026-09-19T15:30", "2026-10-02T09:10");
    expect(dateAllowed("datetime", date(2026, 9, 18), limits)).toBe(false);
    expect(dateAllowed("datetime", date(2026, 9, 19), limits)).toBe(true);
    expect(dateAllowed("datetime", date(2026, 10, 2), limits)).toBe(true);
    expect(dateAllowed("datetime", date(2026, 10, 3), limits)).toBe(false);
    expect(dateAllowed("date", date(2026, 9, 19), parseLimits("date", "2026-09-19", "2026-09-19"))).toBe(true);
    expect(dateAllowed("date", date(2026, 9, 20), parseLimits("date", "2026-09-19", "2026-09-19"))).toBe(false);
    expect(monthAllowed("datetime", 2026, 8, limits)).toBe(false);
    expect(monthAllowed("datetime", 2026, 9, limits)).toBe(true);
    expect(monthAllowed("datetime", 2026, 11, limits)).toBe(false);
    expect(hourAllowed("datetime", date(2026, 9, 19), 14, limits)).toBe(false);
    expect(hourAllowed("datetime", date(2026, 9, 19), 15, limits)).toBe(true);
    expect(hourAllowed("datetime", date(2026, 9, 20), 3, limits)).toBe(true);
    expect(hourAllowed("datetime", date(2026, 10, 2), 10, limits)).toBe(false);
    expect(minuteAllowed("datetime", date(2026, 9, 19), time(15, 25), limits)).toBe(false);
    expect(minuteAllowed("datetime", date(2026, 9, 19), time(15, 30), limits)).toBe(true);
    expect(minuteAllowed("datetime", date(2026, 10, 2), time(9, 15), limits)).toBe(false);
    expect(minuteAllowed("time", null, time(8, 0), parseLimits("time", "09:00", "18:00"))).toBe(false);
    expect(hourAllowed("time", null, 18, parseLimits("time", "09:00", "18:00"))).toBe(true);
    expect(hourAllowed("time", null, 19, parseLimits("time", "09:00", "18:00"))).toBe(false);
    // A preserved ":30" pushes 09:10 past a "09:10" upper limit.
    expect(minuteAllowed("datetime", date(2026, 10, 2), time(9, 10), limits, ":30")).toBe(false);
  });

  it("clamps a date into the limits", () => {
    const limits = parseLimits("datetime", "2026-09-19T15:30", "2026-10-02T09:10");
    expect(clampDate("datetime", date(2026, 9, 1), limits)).toEqual(date(2026, 9, 19));
    expect(clampDate("datetime", date(2026, 12, 1), limits)).toEqual(date(2026, 10, 2));
    expect(clampDate("datetime", date(2026, 9, 25), limits)).toEqual(date(2026, 9, 25));
    expect(clampDate("time", date(2026, 9, 1), parseLimits("time", "09:00"))).toEqual(date(2026, 9, 1));
  });

  it("clamps a time into the limits, rounding to the minute step", () => {
    const limits = parseLimits("datetime", "2026-09-19T15:32", "2026-09-25T18:03");
    expect(clampTime("datetime", date(2026, 9, 19), time(9, 0), limits, 5)).toEqual(time(15, 35));
    expect(clampTime("datetime", date(2026, 9, 19), time(16, 0), limits, 5)).toEqual(time(16, 0));
    expect(clampTime("datetime", date(2026, 9, 20), time(9, 0), limits, 5)).toEqual(time(9, 0));
    expect(clampTime("datetime", date(2026, 9, 25), time(20, 0), limits, 5)).toEqual(time(18, 0));
    expect(clampTime("datetime", date(2026, 9, 19), time(9, 0), parseLimits("datetime", "2026-09-19T15:30"), 5)).toEqual(time(15, 30));
    expect(clampTime("datetime", date(2026, 9, 19), time(9, 0), parseLimits("datetime", "2026-09-19T15:56"), 15)).toEqual(time(16, 0));
    // Rounding up would leave the day or overshoot the upper limit, so the exact limit is used.
    expect(clampTime("datetime", date(2026, 9, 19), time(9, 0), parseLimits("datetime", "2026-09-19T23:58"), 5)).toEqual(time(23, 58));
    expect(clampTime("datetime", date(2026, 9, 19), time(9, 0), parseLimits("datetime", "2026-09-19T15:32", "2026-09-19T15:34"), 5)).toEqual(time(15, 32));
    expect(clampTime("datetime", date(2026, 9, 25), time(20, 0), parseLimits("datetime", "2026-09-25T18:01", "2026-09-25T18:03"), 5)).toEqual(time(18, 3));
    expect(clampTime("time", null, time(7, 0), parseLimits("time", "09:00", "18:00"), 5)).toEqual(time(9, 0));
    expect(clampTime("time", null, time(19, 0), parseLimits("time", "09:00", "18:00"), 5)).toEqual(time(18, 0));
    expect(clampTime("datetime", date(2026, 9, 19), time(9, 0), parseLimits("datetime", "2026-09-19T15:30:00"), 5, ":30")).toEqual(time(15, 30));
    expect(clampTime("datetime", date(2026, 9, 19), time(9, 0), parseLimits("datetime", "2026-09-19T15:30:45"), 5, ":30")).toEqual(time(15, 35));
    expect(clampTime("datetime", date(2026, 9, 18), time(9, 0), limits, 5)).toEqual(time(9, 0));
  });
});

describe("date-time field time options", () => {
  it("lists minutes by step and keeps an off-step current minute", () => {
    expect(minuteOptions(5)).toEqual([0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55]);
    expect(minuteOptions(15, 30)).toEqual([0, 15, 30, 45]);
    expect(minuteOptions(15, 37)).toEqual([0, 15, 30, 37, 45]);
    expect(minuteOptions(5, null)).toHaveLength(12);
    expect(minuteOptions(1)).toHaveLength(60);
    expect(minuteOptions(7)).toEqual([0, 7, 14, 21, 28, 35, 42, 49, 56]);
  });

  it("falls back to five minutes for an unusable step", () => {
    expect(normalizeMinuteStep(undefined)).toBe(5);
    expect(normalizeMinuteStep(0)).toBe(5);
    expect(normalizeMinuteStep(-10)).toBe(5);
    expect(normalizeMinuteStep(Number.NaN)).toBe(5);
    expect(normalizeMinuteStep(90)).toBe(5);
    expect(normalizeMinuteStep(15)).toBe(15);
    expect(normalizeMinuteStep(2.4)).toBe(2);
  });

  it("steps hours and minutes with wrap-around", () => {
    expect(stepHour(23, 1)).toBe(0);
    expect(stepHour(0, -1)).toBe(23);
    expect(stepMinute(55, 1, 5)).toBe(0);
    expect(stepMinute(0, -1, 5)).toBe(55);
    expect(stepMinute(37, 1, 15)).toBe(45);
    expect(stepMinute(37, -1, 15)).toBe(30);
    expect(stepMinute(30, -1, 15)).toBe(15);
  });
});

describe("date-time field text", () => {
  it("formats the trigger value per locale with a 24-hour clock", () => {
    const parsed = parseValue("datetime", "2026-09-19T15:30");
    expect(formatDisplay("datetime", parsed, "en")).toBe("Sep 19, 2026, 15:30");
    expect(formatDisplay("datetime", parsed, "ru")).toBe("19 сент. 2026, 15:30");
    expect(formatDisplay("datetime", parsed, "ro")).toBe("19 sept. 2026, 15:30");
    expect(formatDisplay("date", parseValue("date", "2026-05-03"), "ru")).toBe("3 мая 2026");
    expect(formatDisplay("date", parseValue("date", "2026-01-01"), "en")).toBe("Jan 1, 2026");
    expect(formatDisplay("time", parseValue("time", "09:00"), "en")).toBe("09:00");
    expect(formatDisplay("datetime", parseValue("datetime", "2026-09-19T00:05:30"), "en")).toBe("Sep 19, 2026, 00:05");
    expect(formatDisplay("date", null, "en")).toBe("");
    expect(formatLimit("datetime", "2026-09-19T12:00", "en")).toBe("Sep 19, 2026, 12:00");
    expect(formatLimit("date", "nonsense", "en")).toBe("");
  });

  it("formats times on a 12-hour clock in the language's own notation", () => {
    const parsed = parseValue("datetime", "2026-09-19T15:30");
    expect(formatDisplay("datetime", parsed, "en", "h12")).toMatch(/^Sep 19, 2026, 3:30\sPM$/);
    expect(formatDisplay("datetime", parsed, "ru", "h12")).toMatch(/^19 сент\. 2026, 3:30\sPM$/);
    expect(formatDisplay("time", parseValue("time", "00:05"), "ro", "h12")).toMatch(/^12:05\sa\.m\.$/);
    expect(formatDisplay("date", parseValue("date", "2026-01-01"), "en", "h12")).toBe("Jan 1, 2026");
    expect(formatLimit("datetime", "2026-09-19T12:00", "en", "h12")).toMatch(/^Sep 19, 2026, 12:00\sPM$/);
    expect(formatTime(time(9, 0), "en", "h12")).toMatch(/^9:00\sAM$/);
    expect(formatTime(time(9, 0), "en")).toBe("09:00");
    expect(hourLabel(0, "en", "h12")).toMatch(/^12\sAM$/);
    expect(hourLabel(15, "ro", "h12")).toMatch(/^3\sp\.m\.$/);
    expect(hourLabel(5, "en")).toBe("05");
    expect([periodLabel(0, "en"), periodLabel(11, "en"), periodLabel(12, "en"), periodLabel(23, "ro")]).toEqual(["AM", "AM", "PM", "p.m."]);
  });

  it("follows the language's clock unless the user forced one", () => {
    expect(resolveHourCycle("en")).toBe("h12");
    expect(resolveHourCycle("ru")).toBe("h23");
    expect(resolveHourCycle("ro")).toBe("h23");
    expect(resolveHourCycle("en", "h23")).toBe("h23");
    expect(resolveHourCycle("ru", "h12")).toBe("h12");
  });

  it("reads typed hours of a 12-hour clock within the current half of the day", () => {
    expect([0, 9, 12, 13, 23].map((hour) => displayHour(hour, "h12"))).toEqual([12, 9, 12, 1, 11]);
    expect(displayHour(13, "h23")).toBe(13);
    expect(typedHour(10, 9, "h12")).toBe(10);
    expect(typedHour(10, 15, "h12")).toBe(22);
    expect(typedHour(12, 9, "h12")).toBe(0);
    expect(typedHour(12, 15, "h12")).toBe(12);
    expect(typedHour(0, 15, "h12")).toBe(0);
    expect(typedHour(17, 9, "h12")).toBe(17);
    expect(typedHour(77, 9, "h12")).toBe(23);
    expect(typedHour(10, 15, "h23")).toBe(10);
    expect(typedHour(77, 9, "h23")).toBe(23);
    expect([togglePeriod(0), togglePeriod(9), togglePeriod(12), togglePeriod(22)]).toEqual([12, 21, 0, 10]);
  });

  it("names days, months and weekdays", () => {
    expect(formatDayLabel(date(2026, 9, 19), "en")).toBe("19 September 2026");
    expect(formatDayLabel(date(2026, 9, 19), "ru")).toBe("19 сентября 2026");
    expect(formatDayLabel(date(2026, 9, 19), "ro")).toBe("19 septembrie 2026");
    expect(formatMonthTitle(2026, 9, "en")).toBe("September 2026");
    expect(formatMonthTitle(2026, 9, "ru")).toBe("Сентябрь 2026");
    expect(formatMonthTitle(2026, 9, "ro")).toBe("Septembrie 2026");
    expect(monthName(5, "ru", "short")).toBe("Май");
    expect(weekdayNames("en").map((weekday) => weekday.short)).toEqual(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]);
    expect(weekdayNames("ru").map((weekday) => weekday.short)).toEqual(["пн", "вт", "ср", "чт", "пт", "сб", "вс"]);
    expect(weekdayNames("ru")[0].long).toBe("понедельник");
    expect(weekdayNames("ro")[0].long).toBe("luni");
    expect(weekdayNames("en", 0).map((weekday) => weekday.short)).toEqual(["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]);
    expect(weekdayNames("en", 6).map((weekday) => weekday.short)).toEqual(["Sat", "Sun", "Mon", "Tue", "Wed", "Thu", "Fri"]);
  });
});

describe("date-time field popover placement", () => {
  const viewport = { width: 1000, height: 800 };
  const size = { width: 280, height: 340 };

  it("opens below the trigger, aligned to its start", () => {
    expect(placePopover({ top: 100, bottom: 144, left: 40 }, size, viewport)).toEqual({ placement: "bottom", top: 148, left: 40 });
  });

  it("flips above when only that side has room", () => {
    expect(placePopover({ top: 600, bottom: 644, left: 40 }, size, viewport)).toEqual({ placement: "top", top: 256, left: 40 });
  });

  it("stays inside the viewport margin", () => {
    expect(placePopover({ top: 100, bottom: 144, left: 900 }, size, viewport)).toEqual({ placement: "bottom", top: 148, left: 712 });
    expect(placePopover({ top: 100, bottom: 144, left: -50 }, size, viewport).left).toBe(8);
    expect(placePopover({ top: 150, bottom: 194, left: 40 }, size, { width: 320, height: 400 })).toEqual({ placement: "bottom", top: 52, left: 32 });
    expect(placePopover({ top: 10, bottom: 54, left: 40 }, { width: 400, height: 900 }, { width: 320, height: 400 })).toEqual({ placement: "bottom", top: 8, left: 8 });
  });
});
