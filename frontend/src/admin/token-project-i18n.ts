import type { Locale } from "../i18n";

const en = {
  project: "Project", allProjects: "All projects", unavailableProject: "Unavailable project",
  legacyUnbound: "Legacy token without a project",
  chooseRole: "Choose a role", roleHelp: "The selected role determines access to modules and available actions.",
  roleSource: "The role from project {project} applies in all projects.",
  noRoles: "This project has no roles that can be assigned to an access token.",
  noProjects: "There are no projects where you can issue access tokens.",
  legacyInboxRestriction: "Existing inbox restriction: {count}", retry: "Try again",
};
type TokenProjectTextKey = keyof typeof en;
const ru: Record<TokenProjectTextKey, string> = {
  project: "Проект", allProjects: "Все проекты", unavailableProject: "Проект недоступен",
  legacyUnbound: "Старый токен без привязки к проекту",
  chooseRole: "Выберите роль", roleHelp: "Выбранная роль определяет доступ к модулям и доступные действия.",
  roleSource: "Роль из проекта «{project}» действует во всех проектах.",
  noRoles: "В этом проекте нет ролей, которые можно назначить токену доступа.",
  noProjects: "Нет проектов, в которых вы можете выпускать токены доступа.",
  legacyInboxRestriction: "Прежнее ограничение по входящим: {count}", retry: "Повторить",
};
const ro: Record<TokenProjectTextKey, string> = {
  project: "Proiect", allProjects: "Toate proiectele", unavailableProject: "Proiect indisponibil",
  legacyUnbound: "Token vechi fără proiect asociat",
  chooseRole: "Alegeți un rol", roleHelp: "Rolul ales stabilește accesul la module și acțiunile disponibile.",
  roleSource: "Rolul din proiectul „{project}” se aplică în toate proiectele.",
  noRoles: "Acest proiect nu are roluri care pot fi atribuite unui token de acces.",
  noProjects: "Nu există proiecte în care puteți emite tokenuri de acces.",
  legacyInboxRestriction: "Restricție existentă pentru inboxuri: {count}", retry: "Reîncearcă",
};

export function tokenProjectText(locale: Locale) {
  const messages = locale === "ru" ? ru : locale === "ro" ? ro : en;
  return (key: TokenProjectTextKey, values: Record<string, string | number> = {}): string =>
    messages[key].replace(/\{(\w+)\}/g, (match, name: string) => String(values[name] ?? match));
}
