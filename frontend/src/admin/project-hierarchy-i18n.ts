import type { Locale } from "../i18n";

const en = {
  parent: "Parent project",
  projectScope: "Project",
  dataScope: "Show data",
  currentProject: "Current project",
  currentAndSubprojects: "Current and subprojects",
  selectedProjects: "Selected projects: {count}",
  selectProjects: "Select projects",
  searchProjects: "Search projects",
  noProjectsFound: "No projects found",
  selectProjectsHint: "Select projects to show their data.",
  allWithSubprojects: "Parent and all subprojects",
  openProject: "Open project",
  standalone: "Standalone project",
  subproject: "Subproject",
  addSubproject: "Add subproject",
  newSubproject: "New subproject",
  subprojects: "Subprojects",
  expand: "Show subprojects of {name}",
  collapse: "Hide subprojects of {name}",
  unavailableParent: "Current parent project (unavailable)",
  hasChildren: "A project with subprojects cannot be nested inside another project.",
};
type HierarchyTextKey = keyof typeof en;
const ru: Record<HierarchyTextKey, string> = {
  parent: "Родительский проект",
  projectScope: "Проект",
  dataScope: "Показывать данные",
  currentProject: "Текущий проект",
  currentAndSubprojects: "Текущий и подпроекты",
  selectedProjects: "Выбрано проектов: {count}",
  selectProjects: "Выберите проекты",
  searchProjects: "Поиск проектов",
  noProjectsFound: "Проекты не найдены",
  selectProjectsHint: "Выберите проекты, данные которых нужно показать.",
  allWithSubprojects: "Родительский и все подпроекты",
  openProject: "Открыть проект",
  standalone: "Самостоятельный проект",
  subproject: "Подпроект",
  addSubproject: "Добавить подпроект",
  newSubproject: "Новый подпроект",
  subprojects: "Подпроекты",
  expand: "Показать подпроекты: {name}",
  collapse: "Скрыть подпроекты: {name}",
  unavailableParent: "Текущий родительский проект (недоступен)",
  hasChildren: "Проект с подпроектами нельзя вложить в другой проект.",
};
const ro: Record<HierarchyTextKey, string> = {
  parent: "Proiect părinte",
  projectScope: "Proiect",
  dataScope: "Afișează datele",
  currentProject: "Proiectul curent",
  currentAndSubprojects: "Proiectul curent și subproiectele",
  selectedProjects: "Proiecte selectate: {count}",
  selectProjects: "Selectează proiectele",
  searchProjects: "Caută proiecte",
  noProjectsFound: "Nu s-au găsit proiecte",
  selectProjectsHint: "Selectează proiectele pentru a afișa datele lor.",
  allWithSubprojects: "Proiectul părinte și toate subproiectele",
  openProject: "Deschide proiectul",
  standalone: "Proiect independent",
  subproject: "Subproiect",
  addSubproject: "Adaugă subproiect",
  newSubproject: "Subproiect nou",
  subprojects: "Subproiecte",
  expand: "Afișează subproiectele: {name}",
  collapse: "Ascunde subproiectele: {name}",
  unavailableParent: "Proiectul părinte actual (indisponibil)",
  hasChildren: "Un proiect cu subproiecte nu poate fi inclus într-un alt proiect.",
};

export function projectHierarchyText(locale: Locale) {
  const messages = locale === "ru" ? ru : locale === "ro" ? ro : en;
  return (key: HierarchyTextKey, values: Record<string, string> = {}) =>
    messages[key].replace(/\{(\w+)\}/g, (match, name: string) => values[name] ?? match);
}
