# Project departments

A project contains departments. Each department has a name, icon, ordered sidebar modules, start page, and one or more inboxes. Creating a project can create up to 100 departments in the same transaction; each receives an inbox. The project selects its default department and can enable the director workspace.

## Workspace behavior

The sidebar shows the current project and a department selector. Selecting another project opens its default department. An employee assigned to one department opens that department instead. The start page must be one of the department's enabled modules; the UI also filters every module by the employee's existing role permissions.

The Departments page lets project administrators add departments, edit their menus and start pages, and move inboxes into a department. Every inbox belongs to one department. To move an inbox, select it in the destination department. Disabling a sidebar module changes navigation; it does not revoke the underlying role permission.

When the Channels module is enabled, department creation and editing also offer “Show standard channels”. Turning it off hides web widgets, Telegram, email, and their blacklist picker from that department's channel catalog while keeping custom channels available. Existing connections keep working. The director workspace shows the standard types. The setting defaults to enabled and persists when the Channels module is removed and added again.

The director workspace is a whole-project view with real conversation, employee, and inbox totals and a row for each department. Empty widget sessions without messages are excluded from conversation counts. Whole-project employees are counted once in the project total and included in each department's employee count.

## Employee access

Project membership combines a role with an optional department assignment. A department assignment restricts the employee's inbox access on the server; it cannot be combined with an administrator role or director access. Tenant-wide access must be removed before granting department-only access to the same employee.

An unrestricted project member can be granted director access. Project administrators have director access when the project enables it. Selecting a department narrows the current working view for an unrestricted employee without removing their project administration permissions.

Department-only employees cannot use tools that lack department isolation, including agent invocation APIs. Knowledge bases, tasks, reply templates, AI profiles, and custom channel configurations use their explicit resource visibility and the employee’s module permissions. Administrators and users in the director workspace can choose bindings; an empty selection is shared across the organization. Other users create resources bound to their current project and department, with the selectors hidden, and preserve existing bindings when editing. Inbox-scoped conversations, contacts, presence, quality reports, and supported settings continue to use the role's permissions within the allowed inboxes.

HTTP requests and WebSocket events revalidate live membership. Access tokens also retain their own inbox and permission limits; department access never widens them. Queue assignment eligibility observes department membership. Narrowing an employee's department access releases their assignments in other departments. Routing also clears an assignment that became invalid after an inbox move, and the widget refreshes the displayed operator after departure.

## Storage and API

Migration `0083_project_departments.sql` introduces departments and extends projects, inboxes, memberships, and operator sessions. Existing projects receive a Support department with their existing inboxes. Existing memberships remain unrestricted, preserving their previous access. Apply this migration with the backend release before serving the new frontend.

Migration `0087_department_default_channels.sql` adds `show_default_channels` with a default of true for existing and new departments. Project department drafts and department creation accept this optional boolean; department updates preserve it when omitted. Department responses include the saved value. Apply this migration before starting the updated backend.

The OpenAPI contract describes:

- `GET/POST /api/v1/projects/{project_id}/departments`
- `PATCH /api/v1/projects/{project_id}/departments/{department_id}`
- `GET /api/v1/projects/{project_id}/overview`
- `POST /api/v1/auth/department` — a department UUID selects that department; null selects the director workspace.

Project creation accepts `departments`, `default_department_index`, and `director_enabled`. Project updates accept `default_department_id` and `director_enabled`. Membership requests accept `department_id` and `director_access`; omitting a field on update preserves it, while an explicit null department clears the assignment.

The migration was validated in disposable local PostgreSQL databases. Running tests or the local preview does not apply it to a deployed environment.
