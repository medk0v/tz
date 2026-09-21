import type { Locale } from "../i18n";

const en = {
  title: "Users", description: "Create accounts and manage access to this project.",
  create: "Create user", creating: "Creating user…", created: "User created. They can sign in with their email and password.",
  newAccount: "New user", existingAccount: "Existing account", accountAction: "Add to project",
  displayName: "Name", password: "Password", passwordHelp: "At least 8 characters. Share the sign-in details with the user.",
  passwordTooLong: "This password is too long. Use a shorter password.",
  chooseRole: "Choose a role", roleHelp: "The selected role determines the user's permissions. Department access limits their workspace.",
  roleHelpLite: "The selected role determines the user's permissions.",
  adminHelp: "Administrators can manage the entire project, its users and all permissions.",
  manageRoles: "Roles and permissions", createError: "Could not create the user.",
  accountExists: "An account with this email already exists. Select Existing account to grant project access.",
  ownAccess: "You cannot change or revoke your own project access here.", retry: "Try again",
};
type UsersTextKey = keyof typeof en;
const ru: Record<UsersTextKey, string> = {
  title: "Пользователи", description: "Создавайте пользователей и управляйте доступом к этому проекту.",
  create: "Создать пользователя", creating: "Создание пользователя…", created: "Пользователь создан. Он может войти по своей почте и паролю.",
  newAccount: "Новый пользователь", existingAccount: "Существующий аккаунт", accountAction: "Добавить в проект",
  displayName: "Имя", password: "Пароль", passwordHelp: "Не менее 8 символов. Передайте пользователю данные для входа.",
  passwordTooLong: "Этот пароль слишком длинный. Используйте более короткий пароль.",
  chooseRole: "Выберите роль", roleHelp: "Выбранная роль определяет права пользователя. Доступ к отделу ограничивает его рабочее пространство.",
  roleHelpLite: "Выбранная роль определяет права пользователя.",
  adminHelp: "Администраторы могут управлять всем проектом, его пользователями и всеми правами.",
  manageRoles: "Роли и права", createError: "Не удалось создать пользователя.",
  accountExists: "Аккаунт с этой почтой уже существует. Выберите «Существующий аккаунт», чтобы открыть ему доступ к проекту.",
  ownAccess: "Здесь нельзя изменить или отозвать собственный доступ к проекту.", retry: "Повторить",
};
const ro: Record<UsersTextKey, string> = {
  title: "Utilizatori", description: "Creați conturi și gestionați accesul la acest proiect.",
  create: "Creează utilizator", creating: "Se creează utilizatorul…", created: "Utilizatorul a fost creat. Se poate autentifica folosind adresa de e-mail și parola.",
  newAccount: "Utilizator nou", existingAccount: "Cont existent", accountAction: "Adaugă în proiect",
  displayName: "Nume", password: "Parolă", passwordHelp: "Cel puțin 8 caractere. Transmiteți utilizatorului datele de autentificare.",
  passwordTooLong: "Această parolă este prea lungă. Folosiți o parolă mai scurtă.",
  chooseRole: "Alegeți un rol", roleHelp: "Rolul ales stabilește permisiunile utilizatorului. Accesul la departament limitează spațiul său de lucru.",
  roleHelpLite: "Rolul ales stabilește permisiunile utilizatorului.",
  adminHelp: "Administratorii pot gestiona întregul proiect, utilizatorii și toate permisiunile.",
  manageRoles: "Roluri și permisiuni", createError: "Utilizatorul nu a putut fi creat.",
  accountExists: "Există deja un cont cu această adresă de e-mail. Alegeți Cont existent pentru a-i acorda acces la proiect.",
  ownAccess: "Nu puteți modifica sau revoca propriul acces la proiect aici.", retry: "Reîncearcă",
};

export function usersText(locale: Locale) {
  const messages = locale === "ru" ? ru : locale === "ro" ? ro : en;
  return (key: UsersTextKey): string => messages[key];
}
