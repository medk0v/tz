import { useI18n, type Locale } from "../i18n";
import { PreferenceDropdown, type PreferenceDropdownOption } from "./PreferenceDropdown";

const languages: ReadonlyArray<PreferenceDropdownOption<Locale>> = [
  { value: "en", label: "English", lang: "en" },
  { value: "ru", label: "Русский", lang: "ru" },
  { value: "ro", label: "Română", lang: "ro" },
];

export function LanguageDropdown() {
  const { locale, setLocale, t } = useI18n();
  return <PreferenceDropdown label={t("language.label")} options={languages} value={locale} onChange={setLocale} />;
}
