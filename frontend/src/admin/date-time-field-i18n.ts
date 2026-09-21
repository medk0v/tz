import type { Locale } from "../i18n";

const en = {
  placeholderDate: "Select date", placeholderDatetime: "Select date and time", placeholderTime: "Select time",
  dialogDate: "Choose date", dialogDatetime: "Choose date and time", dialogTime: "Choose time",
  previousMonth: "Previous month", nextMonth: "Next month", previousYear: "Previous year", nextYear: "Next year", chooseMonth: "Choose month and year", backToDays: "Back to days",
  hours: "Hours", minutes: "Minutes", switchPeriod: "Switch between AM and PM", today: "Today", now: "Now", clear: "Clear", done: "Done",
  required: "Required.", requiredDate: "Choose a date.", requiredDatetime: "Choose a date and time.", requiredTime: "Choose a time.",
  tooEarly: "Choose {limit} or later.", tooLate: "Choose {limit} or earlier.",
} as const;
export type DateTimeFieldTextKey = keyof typeof en;
const ru: Record<DateTimeFieldTextKey, string> = {
  placeholderDate: "Выберите дату", placeholderDatetime: "Выберите дату и время", placeholderTime: "Выберите время",
  dialogDate: "Выбор даты", dialogDatetime: "Выбор даты и времени", dialogTime: "Выбор времени",
  previousMonth: "Предыдущий месяц", nextMonth: "Следующий месяц", previousYear: "Предыдущий год", nextYear: "Следующий год", chooseMonth: "Выбрать месяц и год", backToDays: "Вернуться к дням",
  hours: "Часы", minutes: "Минуты", switchPeriod: "Переключить AM и PM", today: "Сегодня", now: "Сейчас", clear: "Очистить", done: "Готово",
  required: "Обязательное поле.", requiredDate: "Выберите дату.", requiredDatetime: "Выберите дату и время.", requiredTime: "Выберите время.",
  tooEarly: "Выберите значение не раньше {limit}.", tooLate: "Выберите значение не позже {limit}.",
};
const ro: Record<DateTimeFieldTextKey, string> = {
  placeholderDate: "Selectează data", placeholderDatetime: "Selectează data și ora", placeholderTime: "Selectează ora",
  dialogDate: "Alege data", dialogDatetime: "Alege data și ora", dialogTime: "Alege ora",
  previousMonth: "Luna precedentă", nextMonth: "Luna următoare", previousYear: "Anul precedent", nextYear: "Anul următor", chooseMonth: "Alege luna și anul", backToDays: "Înapoi la zile",
  hours: "Ore", minutes: "Minute", switchPeriod: "Comută între a.m. și p.m.", today: "Astăzi", now: "Acum", clear: "Șterge", done: "Gata",
  required: "Câmp obligatoriu.", requiredDate: "Selectează o dată.", requiredDatetime: "Selectează data și ora.", requiredTime: "Selectează ora.",
  tooEarly: "Alege {limit} sau mai târziu.", tooLate: "Alege {limit} sau mai devreme.",
};

export function dateTimeFieldText(locale: Locale) {
  const messages = { en, ru, ro }[locale];
  return (key: DateTimeFieldTextKey, values: Record<string, string | number> = {}): string =>
    Object.entries(values).reduce((value, [name, replacement]) => value.replaceAll(`{${name}}`, String(replacement)), messages[key] as string);
}
