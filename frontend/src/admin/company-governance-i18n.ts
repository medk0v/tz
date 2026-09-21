import type { Locale } from "../i18n";

const en = {
  title: "Teams and responsibility", description: "Define what teams and working roles are responsible for and which decisions they can make.",
  principlesTitle: "Management principles", companyModel: "Company management model", hierarchical: "Hierarchical", hybrid: "Hybrid", self_managed: "Self-managed",
  modelHelp: "The model describes how you intend to work. It does not automatically change authority, approval rules, reporting relationships or access rights.",
  companyPurpose: "Company purpose", principles: "Shared principles", teams: "Teams", addTeam: "Add team", unnamedTeam: "New team", teamName: "Team name", teamPurpose: "Team purpose", domain: "Area of responsibility",
  department: "Department", companyWide: "Company-wide", teamModel: "Team management model", inherit: "Use company model", removeTeam: "Delete team",
  roles: "Working roles", addRole: "Add role", unnamedRole: "New role", roleName: "Role name", rolePurpose: "Expected result", accountabilities: "Responsibilities", authority: "Decides independently", consultation: "Must consult", escalation: "Requires approval or escalation", evaluation: "How results are reviewed",
  positions: "Assigned positions", positionsHelp: "Assign existing positions from this department or company-wide positions. A position can hold several working roles. These assignments do not grant system access.",
  noPositions: "No positions are available for this team. Add them in Positions and org chart.", noAssignments: "No positions assigned.", vacant: "Vacant", human: "Person", agent: "AI agent", unavailable: "Unavailable position", outOfScope: "Outside this team's department",
  delegatedBy: "Human responsible for AI delegation", choose: "Select…", aiHelp: "For a role assigned to an AI agent, specify independent authority, approval or escalation limits, and a responsible human.",
  noTeams: "Add a team to define its responsibility and working roles.", noRoles: "This team has no working roles yet.", selectRole: "Select a role to view its responsibilities and authority.", removeRole: "Delete role",
  save: "Save changes", saving: "Saving…", discard: "Discard changes", saved: "Management principles, teams and roles saved.", loading: "Loading teams and responsibility…", loadError: "Could not load teams and responsibility.", saveError: "Could not save changes. Your draft is preserved.", retry: "Retry",
  conflict: "Someone has changed these settings. Your draft is preserved. Reload the saved version before continuing.", reload: "Reload saved version", discardConfirm: "Discard all unsaved changes to management principles, teams and roles?",
  deleteTeamConfirm: "Delete this team and all its working roles from the draft? Changes take effect after saving.", deleteRoleConfirm: "Delete this working role from the draft? Changes take effect after saving.",
  sizeLimit: "Maximum 50 teams, 200 working roles and 100 positions per role.",
  readOnly: "You can view management principles, teams and responsibilities.", invalidNames: "Give every team and role a name.", invalidAssignment: "Remove unavailable or out-of-department positions from roles and check the responsible human for AI delegation.", invalidAi: "For every AI role, specify independent authority, approval or escalation limits, and a responsible human.", invalidDepartment: "Select an available department for each team.", unavailableDepartment: "Unavailable department",
} as const;

type CompanyGovernanceTextKey = keyof typeof en;
const ru: Record<CompanyGovernanceTextKey, string> = {
  title: "Команды и ответственность", description: "Определите, за что отвечают команды и рабочие роли и какие решения они вправе принимать.",
  principlesTitle: "Принципы управления", companyModel: "Модель управления компанией", hierarchical: "Иерархическая", hybrid: "Смешанная", self_managed: "Самоуправляемая",
  modelHelp: "Модель описывает выбранный подход к работе. Она не меняет автоматически полномочия, правила согласования, подчинение или права доступа.",
  companyPurpose: "Цель компании", principles: "Общие принципы", teams: "Команды", addTeam: "Добавить команду", unnamedTeam: "Новая команда", teamName: "Название команды", teamPurpose: "Цель команды", domain: "Область ответственности",
  department: "Отдел", companyWide: "Вся компания", teamModel: "Модель управления командой", inherit: "Как в компании", removeTeam: "Удалить команду",
  roles: "Рабочие роли", addRole: "Добавить роль", unnamedRole: "Новая роль", roleName: "Название роли", rolePurpose: "Ожидаемый результат", accountabilities: "Обязанности", authority: "Решает самостоятельно", consultation: "С кем обязан посоветоваться", escalation: "Где требуется согласование или эскалация", evaluation: "Как проверяется результат",
  positions: "Назначенные должности", positionsHelp: "Назначайте существующие должности этого отдела или всей компании. Одна должность может выполнять несколько рабочих ролей. Назначения не выдают права доступа к системе.",
  noPositions: "Нет доступных должностей для этой команды. Добавьте их в разделе «Должности и оргсхема».", noAssignments: "Должности не назначены.", vacant: "Вакансия", human: "Человек", agent: "ИИ-агент", unavailable: "Недоступная должность", outOfScope: "Должность другого отдела",
  delegatedBy: "Человек, ответственный за делегирование ИИ", choose: "Выберите…", aiHelp: "Для роли ИИ-агента укажите самостоятельные полномочия, границы согласования или эскалации и ответственного человека.",
  noTeams: "Добавьте команду, чтобы определить её ответственность и рабочие роли.", noRoles: "В этой команде пока нет рабочих ролей.", selectRole: "Выберите роль, чтобы посмотреть её обязанности и полномочия.", removeRole: "Удалить роль",
  save: "Сохранить изменения", saving: "Сохранение…", discard: "Отменить изменения", saved: "Принципы управления, команды и роли сохранены.", loading: "Загрузка команд и ответственности…", loadError: "Не удалось загрузить команды и ответственность.", saveError: "Не удалось сохранить изменения. Черновик сохранён в форме.", retry: "Повторить",
  conflict: "Кто-то уже изменил эти настройки. Ваш черновик остался в форме. Загрузите сохранённую версию, чтобы продолжить.", reload: "Загрузить сохранённую версию", discardConfirm: "Отменить все несохранённые изменения принципов управления, команд и ролей?",
  deleteTeamConfirm: "Удалить эту команду и все её рабочие роли из черновика? Изменения вступят в силу после сохранения.", deleteRoleConfirm: "Удалить эту рабочую роль из черновика? Изменения вступят в силу после сохранения.",
  sizeLimit: "Можно добавить не более 50 команд, 200 рабочих ролей и 100 должностей на одну роль.",
  readOnly: "Вам доступен просмотр принципов управления, команд и ответственности.", invalidNames: "Укажите название каждой команды и роли.", invalidAssignment: "Уберите из ролей недоступные должности и должности других отделов. Проверьте ответственного человека за делегирование ИИ.", invalidAi: "Для каждой роли ИИ укажите самостоятельные полномочия, границы согласования или эскалации и ответственного человека.", invalidDepartment: "Выберите доступный отдел для каждой команды.", unavailableDepartment: "Недоступный отдел",
};
const ro: Record<CompanyGovernanceTextKey, string> = {
  title: "Echipe și responsabilități", description: "Definiți responsabilitățile echipelor și ale rolurilor de lucru, precum și deciziile pe care le pot lua.",
  principlesTitle: "Principii de conducere", companyModel: "Modelul de conducere al companiei", hierarchical: "Ierarhic", hybrid: "Mixt", self_managed: "Autogestionat",
  modelHelp: "Modelul descrie modul de lucru ales. Nu modifică automat autoritatea, regulile de aprobare, relațiile de subordonare sau drepturile de acces.",
  companyPurpose: "Scopul companiei", principles: "Principii comune", teams: "Echipe", addTeam: "Adaugă echipă", unnamedTeam: "Echipă nouă", teamName: "Numele echipei", teamPurpose: "Scopul echipei", domain: "Domeniul de responsabilitate",
  department: "Departament", companyWide: "La nivelul companiei", teamModel: "Modelul de conducere al echipei", inherit: "Folosește modelul companiei", removeTeam: "Șterge echipa",
  roles: "Roluri de lucru", addRole: "Adaugă rol", unnamedRole: "Rol nou", roleName: "Numele rolului", rolePurpose: "Rezultatul așteptat", accountabilities: "Responsabilități", authority: "Decide independent", consultation: "Consultare obligatorie", escalation: "Necesită aprobare sau escaladare", evaluation: "Cum se evaluează rezultatele",
  positions: "Funcții atribuite", positionsHelp: "Atribuiți funcții existente din acest departament sau de la nivelul companiei. O funcție poate avea mai multe roluri de lucru. Aceste atribuiri nu acordă acces la sistem.",
  noPositions: "Nu există funcții disponibile pentru această echipă. Adăugați-le în Funcții și organigramă.", noAssignments: "Nu sunt atribuite funcții.", vacant: "Vacantă", human: "Persoană", agent: "Agent AI", unavailable: "Funcție indisponibilă", outOfScope: "În afara departamentului echipei",
  delegatedBy: "Persoana responsabilă pentru delegarea către AI", choose: "Selectează…", aiHelp: "Pentru un rol atribuit unui agent AI, precizați autoritatea independentă, limitele de aprobare sau escaladare și persoana responsabilă.",
  noTeams: "Adăugați o echipă pentru a-i defini responsabilitățile și rolurile de lucru.", noRoles: "Această echipă nu are încă roluri de lucru.", selectRole: "Selectați un rol pentru a-i vedea responsabilitățile și autoritatea.", removeRole: "Șterge rolul",
  save: "Salvează modificările", saving: "Se salvează…", discard: "Renunță la modificări", saved: "Principiile de conducere, echipele și rolurile au fost salvate.", loading: "Se încarcă echipele și responsabilitățile…", loadError: "Echipele și responsabilitățile nu au putut fi încărcate.", saveError: "Modificările nu au putut fi salvate. Ciorna este păstrată.", retry: "Reîncearcă",
  conflict: "Aceste setări au fost modificate de altcineva. Ciorna este păstrată. Reîncărcați versiunea salvată pentru a continua.", reload: "Reîncarcă versiunea salvată", discardConfirm: "Renunțați la toate modificările nesalvate ale principiilor de conducere, echipelor și rolurilor?",
  deleteTeamConfirm: "Ștergeți această echipă și toate rolurile ei din ciornă? Modificările se aplică după salvare.", deleteRoleConfirm: "Ștergeți acest rol de lucru din ciornă? Modificările se aplică după salvare.",
  sizeLimit: "Sunt permise cel mult 50 de echipe, 200 de roluri de lucru și 100 de funcții pentru fiecare rol.",
  readOnly: "Puteți vizualiza principiile de conducere, echipele și responsabilitățile.", invalidNames: "Introduceți numele fiecărei echipe și al fiecărui rol.", invalidAssignment: "Eliminați din roluri funcțiile indisponibile sau din alte departamente și verificați persoana responsabilă pentru delegarea către AI.", invalidAi: "Pentru fiecare rol AI, precizați autoritatea independentă, limitele de aprobare sau escaladare și persoana responsabilă.", invalidDepartment: "Selectați un departament disponibil pentru fiecare echipă.", unavailableDepartment: "Departament indisponibil",
};

export function companyGovernanceText(locale: Locale) {
  const messages = locale === "ru" ? ru : locale === "ro" ? ro : en;
  return (key: CompanyGovernanceTextKey): string => messages[key];
}
