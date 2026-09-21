import { createContext, useContext } from "react";
import { productNamespace } from "./product-edition";

export const TIME_FORMATS = ["auto", "24h", "12h"] as const;
export type TimeFormat = typeof TIME_FORMATS[number];
export const WEEK_STARTS = ["monday", "sunday", "saturday"] as const;
export type WeekStart = typeof WEEK_STARTS[number];
export type HourCycle = "h12" | "h23";
/** A first day of the week as returned by `Date#getDay()`. */
export type WeekStartDay = 0 | 1 | 6;

export const TIME_FORMAT_KEY = `${productNamespace}-admin-time-format`;
export const WEEK_START_KEY = `${productNamespace}-admin-week-start`;

export interface DateTimePreferences {
  timeFormat: TimeFormat;
  weekStart: WeekStart;
  /** The forced hour cycle, or undefined to follow the interface language. */
  hourCycle: HourCycle | undefined;
  weekStartsOn: WeekStartDay;
}

const HOUR_CYCLES: Record<TimeFormat, HourCycle | undefined> = { auto: undefined, "24h": "h23", "12h": "h12" };
const WEEK_START_DAYS: Record<WeekStart, WeekStartDay> = { sunday: 0, monday: 1, saturday: 6 };

export function dateTimePreferences(timeFormat: TimeFormat, weekStart: WeekStart): DateTimePreferences {
  return { timeFormat, weekStart, hourCycle: HOUR_CYCLES[timeFormat], weekStartsOn: WEEK_START_DAYS[weekStart] };
}

export const DEFAULT_DATE_TIME_PREFERENCES = dateTimePreferences("auto", "monday");

/** Screens rendered without the cabinet (tests, isolated views) keep the language defaults and Monday weeks. */
export const DateTimePreferencesContext = createContext<DateTimePreferences>(DEFAULT_DATE_TIME_PREFERENCES);

export function useDateTimePreferences(): DateTimePreferences {
  return useContext(DateTimePreferencesContext);
}

export function isTimeFormat(value: string | null | undefined): value is TimeFormat {
  return (TIME_FORMATS as ReadonlyArray<string | null | undefined>).includes(value);
}

export function isWeekStart(value: string | null | undefined): value is WeekStart {
  return (WEEK_STARTS as ReadonlyArray<string | null | undefined>).includes(value);
}

export function initialTimeFormat(): TimeFormat {
  return "auto";
  
}

export function initialWeekStart(): WeekStart {
  return "monday";
  
}

/** Adds the preferred hour cycle to options that show a time of day. */
export function withHourCycle(options: Intl.DateTimeFormatOptions, hourCycle: HourCycle | undefined): Intl.DateTimeFormatOptions {
  return hourCycle && (options.hour !== undefined || options.timeStyle !== undefined) ? { ...options, hourCycle } : options;
}
