# Resource visibility

Model connections, AI tasks, reply templates, AI profiles, and custom channels support a `visibility` object with optional `project_ids` and `department_ids` arrays. Empty project selections mean every project in the tenant; empty department selections mean every department in the selected projects. When departments are selected, they must belong to the selected projects. The director workspace includes resources assigned to any department of its current project.

Knowledge bases are strictly bound to their owning project. Their `visibility.project_ids` is always that single project; omitted or empty project bindings on creation are normalized to it. Explicit bindings to another project are rejected. Department restrictions can be selected only within the owning project, with no department selection meaning all its departments. Migration `0097_knowledge_base_project_scope.sql` normalizes existing bindings. Listing, article access, agent assignments, and runtime knowledge reads enforce project ownership even for legacy global bindings and shared agents.

Administrators and users currently in the director workspace can choose these bindings. For everyone else, creation is automatically bound to the current project and department on the server, regardless of the submitted visibility. Those users do not see the selectors and cannot change bindings on existing resources. Editing shared content preserves its existing visibility. Existing records receive empty bindings through migration `0086_resource_visibility.sql`.

A task launched from a process is the one resource whose creation takes no `visibility`: it gets the bindings above, narrowed to the places where the process is visible, because it copies the process's content. See [processes.md](processes.md#launching-a-process).

`GET /api/v1/resource-visibility-options` returns `can_choose`, accessible projects, and their departments. Non-admin department users receive `can_choose: false` with empty choice lists. Omitting visibility on an update preserves it; administrators and directors clear bindings by sending empty arrays explicitly.

Storage ownership remains stable: sharing a resource does not move its conversations, channel destination, task history, credentials, or scheduled execution to another project. Server list and direct-resource endpoints enforce visibility, tenant isolation, and module permissions. Existing API token project and Inbox restrictions remain in force. Knowledge can be assigned only to agents owned by the same project. Shared agents can be selected for tasks in projects where they are available, but cannot read another project's knowledge there.

Existing model connections receive empty bindings through migration `0096_ai_provider_visibility.sql`. A visible connection can be assigned to an agent in another project without copying its credentials. Changing its visibility affects listing and new assignments; agents already using it keep their connection, including when edited from another workspace. Disabling the connection stops its use by agents in every project. Creating, changing, and deleting model connections still requires project-wide AI management access.

The frontend uses the same fields in creation and editing, including the quick agent creation dialog. Changing a selected project removes selected departments that belong to excluded projects. Unknown saved bindings remain intact until explicitly removed.

Validation uses disposable local PostgreSQL databases, API regression tests, frontend component tests, TypeScript, Clippy, and the admin build. Local validation does not apply the migration to production.
